//! Expressions (exp3.json). spec: 70-advanced/viewer.md §1.1, §2

use serde_json::{json, Map, Value};

use crate::json::{f32_of, num};
use crate::AnimError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpBlend {
    Add,
    Multiply,
    Overwrite,
}

impl ExpBlend {
    pub fn name(self) -> &'static str {
        match self {
            ExpBlend::Add => "Add",
            ExpBlend::Multiply => "Multiply",
            ExpBlend::Overwrite => "Overwrite",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExpParam {
    pub id: String,
    /// Stored ("calculation") value.
    pub value: f32,
    pub blend: ExpBlend,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Expression {
    /// Seconds; `None` = unset (runtime default).
    pub fade_in: Option<f32>,
    pub fade_out: Option<f32>,
    pub params: Vec<ExpParam>,
}

/// Rounds through a decimal string so Add/Multiply conversions don't leak
/// float noise (the reference uses decimal arithmetic, viewer.md §1.1).
fn decimal(v: f64) -> f32 {
    format!("{v:.6}").parse::<f64>().unwrap_or(v) as f32
}

/// Stored value for target value `t` on a parameter with default `d`.
pub fn stored_from_target(blend: ExpBlend, t: f32, d: f32) -> f32 {
    match blend {
        ExpBlend::Add => decimal(f64::from(t) - f64::from(d)),
        ExpBlend::Multiply if d == 0.0 => 1.0,
        ExpBlend::Multiply => decimal(f64::from(t) / f64::from(d)),
        ExpBlend::Overwrite => t,
    }
}

/// Target value shown in the editor for stored value `v`.
pub fn target_from_stored(blend: ExpBlend, v: f32, d: f32) -> f32 {
    match blend {
        ExpBlend::Add => decimal(f64::from(v) + f64::from(d)),
        ExpBlend::Multiply if d == 0.0 => v,
        ExpBlend::Multiply => decimal(f64::from(v) * f64::from(d)),
        ExpBlend::Overwrite => v,
    }
}

impl Expression {
    pub fn parse(text: &str) -> Result<Self, AnimError> {
        let v: Value = serde_json::from_str(text).map_err(|e| AnimError::Json(e.to_string()))?;
        let mut params = Vec::new();
        for p in v["Parameters"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let Some(id) = p["Id"].as_str() else { continue };
            let blend = match p["Blend"].as_str() {
                Some("Multiply") => ExpBlend::Multiply,
                Some("Overwrite") => ExpBlend::Overwrite,
                _ => ExpBlend::Add,
            };
            let value = f32_of(&p["Value"]).ok_or_else(|| {
                AnimError::Invalid(format!("expression parameter {id}: no value"))
            })?;
            params.push(ExpParam {
                id: id.to_owned(),
                value,
                blend,
            });
        }
        Ok(Self {
            fade_in: f32_of(&v["FadeInTime"]),
            fade_out: f32_of(&v["FadeOutTime"]),
            params,
        })
    }

    pub fn to_json(&self) -> Value {
        let mut root = Map::new();
        root.insert("Version".into(), json!(3));
        root.insert("Type".into(), json!("Live2D Expression"));
        if let Some(f) = self.fade_in {
            root.insert("FadeInTime".into(), num(f64::from(f), 3));
        }
        if let Some(f) = self.fade_out {
            root.insert("FadeOutTime".into(), num(f64::from(f), 3));
        }
        root.insert(
            "Parameters".into(),
            Value::Array(
                self.params
                    .iter()
                    .map(|p| json!({ "Id": p.id, "Value": num(f64::from(p.value), 3), "Blend": p.blend.name() }))
                    .collect(),
            ),
        );
        Value::Object(root)
    }
}

/// An expression from a scene (Animator expression scenes): every keyed
/// parameter of model track `ti` at the scene's first frame, as Add
/// (stored value = target − default); fades from the scene when set.
/// The reference's file has no `Version` member (see [`scene_exp3_json`]).
/// Observed on chitose's expression scenes and their exported exp3 files.
pub fn from_scene(
    s: &keyform_model::animation::Scene,
    ti: usize,
    default_of: &dyn Fn(&str) -> Option<f32>,
) -> Option<Expression> {
    let keyform_model::animation::TrackKind::Model(m) = &s.tracks.get(ti)?.kind else {
        return None;
    };
    let frame = f64::from(s.work_start.max(0));
    let mut params = Vec::new();
    for (k, a) in m.params.iter().enumerate() {
        if a.curve.keys.is_empty() {
            continue;
        }
        let Some(id) = crate::scene::param_id(&a.id) else {
            continue;
        };
        let Some(d) = default_of(id) else {
            continue;
        };
        let t = crate::scene::param_value(m, k, frame) as f32;
        params.push(ExpParam {
            id: id.to_owned(),
            value: stored_from_target(ExpBlend::Add, t, d),
            blend: ExpBlend::Add,
        });
    }
    let secs = |ms: i32| (ms >= 0).then(|| ms as f32 / 1000.0);
    Some(Expression {
        fade_in: secs(s.fade_in_ms),
        fade_out: secs(s.fade_out_ms),
        params,
    })
}

/// The exp3.json the Animator writes for an expression scene (no
/// `Version` member).
pub fn scene_exp3_json(e: &Expression) -> Value {
    let mut v = e.to_json();
    if let Some(o) = v.as_object_mut() {
        o.remove("Version");
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_conversions() {
        assert_eq!(stored_from_target(ExpBlend::Add, 0.3, 0.1), 0.2);
        assert_eq!(target_from_stored(ExpBlend::Add, 0.2, 0.1), 0.3);
        assert_eq!(stored_from_target(ExpBlend::Multiply, 3.0, 2.0), 1.5);
        assert_eq!(stored_from_target(ExpBlend::Multiply, 3.0, 0.0), 1.0);
        assert_eq!(target_from_stored(ExpBlend::Multiply, 3.0, 0.0), 3.0);
        assert_eq!(stored_from_target(ExpBlend::Overwrite, 0.7, 5.0), 0.7);
    }

    #[test]
    fn parse_lenient_and_write() {
        let e = Expression::parse(r#"{"Type":"Live2D Expression","Parameters":[{"Id":"A","Value":-1,"Blend":"Add"},{"Id":"B","Value":2,"Blend":"Multiply"}]}"#).unwrap();
        assert_eq!(e.params.len(), 2);
        assert_eq!(e.fade_in, None);
        let s = crate::json::to_string(&e.to_json());
        assert!(s.contains("\"Value\": -1"));
        assert_eq!(Expression::parse(&s).unwrap(), e);
    }
}
