//! Pose (pose3.json): mutually exclusive part groups.
//! spec: 70-advanced/viewer.md §1.2, §2

use serde_json::{json, Map, Value};

use crate::json::{f32_of, num};
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

/// Runtime pose switching: in each group one part is shown; it fades in
/// over the fade time and the others fade out complementarily.
/// TODO(Q-0009): exact runtime fade rule.
#[derive(Clone, Debug, Default)]
pub struct PoseState {
    /// Current opacity per (group, node).
    opacity: Vec<Vec<f32>>,
    visible: Vec<usize>,
}

impl PoseState {
    pub fn new(pose: &Pose) -> Self {
        Self {
            opacity: pose
                .groups
                .iter()
                .map(|g| {
                    (0..g.len())
                        .map(|i| if i == 0 { 1.0 } else { 0.0 })
                        .collect()
                })
                .collect(),
            visible: vec![0; pose.groups.len()],
        }
    }

    /// Advances by `dt` seconds. `requested(id)` is the part opacity requested
    /// this frame (by a motion), if any; the first node requesting > 0.001
    /// becomes the visible one. Returns (part id, opacity) for every node and link.
    pub fn update(
        &mut self,
        pose: &Pose,
        dt: f32,
        requested: &dyn Fn(&str) -> Option<f32>,
    ) -> Vec<(String, f32)> {
        let fade = pose.fade_in.unwrap_or(0.5).max(0.0);
        let mut out = Vec::new();
        for (gi, g) in pose.groups.iter().enumerate() {
            if self.opacity.get(gi).is_none_or(|o| o.len() != g.len()) {
                *self = Self::new(pose);
            }
            if let Some(i) = g
                .iter()
                .position(|n| requested(&n.id).is_some_and(|o| o > 0.001))
            {
                self.visible[gi] = i;
            }
            let vis = self.visible[gi];
            let ops = &mut self.opacity[gi];
            ops[vis] = if fade > 0.0 {
                (ops[vis] + dt / fade).min(1.0)
            } else {
                1.0
            };
            let rest = 1.0 - ops[vis];
            for (i, o) in ops.iter_mut().enumerate() {
                if i != vis {
                    *o = o.min(rest);
                }
            }
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
    fn switching_fades() {
        let pose = Pose::parse(
            r#"{"Type":"Live2D Pose","Groups":[[{"Id":"A","Link":["A2"]},{"Id":"B","Link":[]}]]}"#,
        )
        .unwrap();
        let mut st = PoseState::new(&pose);
        let none = |_: &str| None;
        let o = st.update(&pose, 0.1, &none);
        assert_eq!(
            o,
            vec![("A".into(), 1.0), ("A2".into(), 1.0), ("B".into(), 0.0)]
        );
        let want_b = |id: &str| (id == "B").then_some(1.0);
        let o = st.update(&pose, 0.25, &want_b);
        assert_eq!(o[2], ("B".into(), 0.5));
        assert_eq!(o[0], ("A".into(), 0.5));
        let o = st.update(&pose, 1.0, &want_b);
        assert_eq!((o[0].1, o[2].1), (0.0, 1.0));
        assert_eq!(
            Pose::parse(&crate::json::to_string(&pose.to_json())).unwrap(),
            pose
        );
    }
}
