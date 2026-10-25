//! Pose (pose3.json): mutually exclusive part groups.
//! spec: 70-advanced/viewer.md §1.2, §2

use serde_json::{json, Map, Value};

use crate::json::{f32_of, num};
use crate::player::Params;
use crate::AnimError;

#[derive(Clone, Debug, PartialEq)]
pub struct PoseNode {
    pub id: String,
    pub links: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Pose {
    pub fade_in: Option<f32>,
    pub groups: Vec<Vec<PoseNode>>,
}

impl Pose {
    pub fn parse(text: &str) -> Result<Self, AnimError> {
        let v: Value = serde_json::from_str(text).map_err(|e| AnimError::Json(e.to_string()))?;
        let groups = v["Groups"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|g| {
                g.as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(|n| {
                        Some(PoseNode {
                            id: n["Id"].as_str()?.to_owned(),
                            links: n["Link"]
                                .as_array()
                                .map(|l| {
                                    l.iter()
                                        .filter_map(|x| x.as_str().map(str::to_owned))
                                        .collect()
                                })
                                .unwrap_or_default(),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|g| !g.is_empty())
            .collect();
        Ok(Self {
            fade_in: f32_of(&v["FadeInTime"]).filter(|f| *f >= 0.0),
            groups,
        })
    }

    pub fn to_json(&self) -> Value {
        let mut root = Map::new();
        root.insert("Version".into(), json!(3));
        root.insert("Type".into(), json!("Live2D Pose"));
        if let Some(f) = self.fade_in {
            root.insert("FadeInTime".into(), num(f64::from(f), 3));
        }
        root.insert(
            "Groups".into(),
            Value::Array(
                self.groups
                    .iter()
                    .map(|g| {
                        Value::Array(
                            g.iter()
                                .map(|n| json!({ "Id": n.id, "Link": n.links }))
                                .collect(),
                        )
                    })
                    .collect(),
            ),
        );
        Value::Object(root)
    }
}

/// Runtime pose switching. The visible entry of each group is chosen by the
/// part-ID parameters that motion `PartOpacity` curves write; part opacities
/// persist between frames. spec: 70-advanced/viewer.md §3.4
#[derive(Clone, Debug, Default)]
pub struct PoseState {
    /// Current opacity per (group, entry).
    opacity: Vec<Vec<f32>>,
}

impl PoseState {
    /// Reset: the first entry of each group is shown and its part-ID
    /// parameter set to 1; the others get opacity 0 and parameter 0.
    pub fn reset(pose: &Pose, p: &mut Params) -> Self {
        let opacity = pose
            .groups
            .iter()
            .map(|g| {
                g.iter()
                    .enumerate()
                    .map(|(i, n)| {
                        let v = if i == 0 { 1.0 } else { 0.0 };
                        p.set(&n.id, v, 1.0);
                        v
                    })
                    .collect()
            })
            .collect();
        Self { opacity }
    }

    /// Advances by `dt` seconds (negative = 0). Returns (part ID, opacity)
    /// for every entry and link part.
    pub fn update(&mut self, pose: &Pose, dt: f32, p: &mut Params) -> Vec<(String, f32)> {
        if self.opacity.len() != pose.groups.len()
            || self
                .opacity
                .iter()
                .zip(&pose.groups)
                .any(|(o, g)| o.len() != g.len())
        {
            *self = Self::reset(pose, p);
        }
        let dt = dt.max(0.0);
        let fade = pose.fade_in.filter(|f| *f >= 0.0).unwrap_or(0.5);
        let mut out = Vec::new();
        for (g, ops) in pose.groups.iter().zip(&mut self.opacity) {
            // 1–2. Visible entry and its new opacity.
            let (vis, v) = match g.iter().position(|n| p.get(&n.id) > 0.001) {
                Some(i) => (
                    i,
                    if fade == 0.0 {
                        1.0
                    } else {
                        (ops[i] + dt / fade).min(1.0)
                    },
                ),
                None => (0, 1.0),
            };
            // 3. The others only lose opacity, down to a ceiling.
            ops[vis] = v;
            let mut b = 1.0 - v;
            if (1.0 - b) * (1.0 - v) > 0.15 {
                b = 1.0 - 0.15 / (1.0 - v);
            }
            for (i, o) in ops.iter_mut().enumerate() {
                if i != vis {
                    *o = o.min(b);
                }
            }
            // 4. Links follow their entry.
            for (n, &o) in g.iter().zip(ops.iter()) {
                out.push((n.id.clone(), o));
                out.extend(n.links.iter().map(|l| (l.clone(), o)));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_by_parameter_and_ceiling() {
        use crate::player::ParamRange;
        use std::collections::HashMap;
        let pose = Pose::parse(
            r#"{"Type":"Live2D Pose","Groups":[[{"Id":"A","Link":["A2"]},{"Id":"B","Link":[]}]]}"#,
        )
        .unwrap();
        let mut values: Vec<f32> = Vec::new();
        let ranges: Vec<ParamRange> = Vec::new();
        let index: HashMap<String, usize> = HashMap::new();
        let mut virtuals: HashMap<String, f32> = HashMap::new();
        let mut p = Params {
            values: &mut values,
            ranges: &ranges,
            index: &index,
            virtuals: &mut virtuals,
        };
        let mut st = PoseState::reset(&pose, &mut p);
        assert_eq!(p.get("A"), 1.0);
        let o = st.update(&pose, 0.1, &mut p);
        assert_eq!(
            o,
            vec![("A".into(), 1.0), ("A2".into(), 1.0), ("B".into(), 0.0)]
        );
        // A motion selects B: it fades in linearly; A stays under the ceiling.
        p.set("A", 0.0, 1.0);
        p.set("B", 1.0, 1.0);
        let o = st.update(&pose, 0.25, &mut p);
        assert_eq!(o[2].1, 0.5);
        // v = 0.5: (1 − b)(1 − v) = 0.25 > 0.15 → b = 1 − 0.15/0.5 = 0.7.
        assert!((o[0].1 - 0.7).abs() < 1e-6, "{o:?}");
        let o = st.update(&pose, 1.0, &mut p);
        assert_eq!((o[0].1, o[2].1), (0.0, 1.0));
        // Nothing selected: the first entry shows instantly.
        p.set("B", 0.0, 1.0);
        let o = st.update(&pose, 0.01, &mut p);
        assert_eq!(o[0].1, 1.0);
        assert_eq!(
            Pose::parse(&crate::json::to_string(&pose.to_json())).unwrap(),
            pose
        );
    }
}
