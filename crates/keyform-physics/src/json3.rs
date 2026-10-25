//! physics3.json read/write. spec: 70-advanced/physics.md §4

use serde_json::{json, Map, Number, Value};

use keyform_model::*;

use crate::PhysicsError;

/// At most 3 fraction digits; integral values without a decimal point.
fn num(v: f32) -> Value {
    let v = f64::from(v);
    if !v.is_finite() {
        return Value::from(0);
    }
    let r = (v * 1000.0).round() / 1000.0;
    if r == r.trunc() && r.abs() < 9.0e15 {
        Value::from(r as i64)
    } else {
        Number::from_f64(r).map_or(Value::from(0), Value::Number)
    }
}

fn kind_name(k: PhysicsKind) -> &'static str {
    match k {
        PhysicsKind::X => "X",
        PhysicsKind::Y => "Y",
        PhysicsKind::Angle => "Angle",
    }
}

/// Export verification on a copy (§4): invalid inputs/outputs and settings are
/// removed, each removal reported.
pub fn verify(rig: &PhysicsRig, params: &[Parameter]) -> (PhysicsRig, Vec<String>) {
    let mut r = rig.clone();
    let mut msgs = Vec::new();
    for s in &mut r.settings {
        let nv = s.vertices.len();
        let before = s.outputs.len();
        s.outputs
            .retain(|o| o.vertex_index >= 1 && o.vertex_index < nv);
        if s.outputs.len() != before {
            msgs.push(format!(
                "{}: A nonexistent Pendulum No is specified.",
                s.name
            ));
        }
        let before = s.inputs.len();
        s.inputs.retain(|i| i.parameter < params.len());
        if s.inputs.len() != before {
            msgs.push(format!(
                "{}: There are input settings for nonexistent parameter.",
                s.name
            ));
        }
        let before = s.outputs.len();
        s.outputs.retain(|o| o.parameter < params.len());
        if s.outputs.len() != before {
            msgs.push(format!(
                "{}: There are output settings for nonexistent parameter.",
                s.name
            ));
        }
    }
    r.settings.retain(|s| {
        if s.inputs.is_empty() || s.vertices.len() < 2 {
            msgs.push(format!(
                "{}: Input settings or Physics model do not exist.",
                s.name
            ));
            false
        } else if s.outputs.is_empty() {
            msgs.push(format!("{}: Output settings do not exist.", s.name));
            false
        } else {
            true
        }
    });
    (r, msgs)
}

/// physics3.json for `rig` (already verified). `None` when nothing remains.
pub fn to_json(rig: &PhysicsRig, params: &[Parameter], include_fps: bool) -> Option<Value> {
    if rig.settings.is_empty() {
        return None;
    }
    let pid = |i: usize| params.get(i).map_or(String::new(), |p| p.id.clone());
    let ids: Vec<String> = (1..=rig.settings.len())
        .map(|i| format!("PhysicsSetting{i}"))
        .collect();
    let mut meta = Map::new();
    meta.insert("PhysicsSettingCount".into(), json!(rig.settings.len()));
    meta.insert(
        "TotalInputCount".into(),
        json!(rig.settings.iter().map(|s| s.inputs.len()).sum::<usize>()),
    );
    meta.insert(
        "TotalOutputCount".into(),
        json!(rig.settings.iter().map(|s| s.outputs.len()).sum::<usize>()),
    );
    meta.insert(
        "VertexCount".into(),
        json!(rig.settings.iter().map(|s| s.vertices.len()).sum::<usize>()),
    );
    if let (true, Some(f)) = (include_fps, rig.fps) {
        meta.insert("Fps".into(), num(f));
    }
    // Gravity and wind Y are negated relative to the editor's y-down values.
    meta.insert(
        "EffectiveForces".into(),
        json!({
            "Gravity": { "X": num(rig.gravity[0]), "Y": num(-rig.gravity[1]) },
            "Wind": { "X": num(rig.wind[0]), "Y": num(-rig.wind[1]) },
        }),
    );
    meta.insert(
        "PhysicsDictionary".into(),
        Value::Array(
            rig.settings
                .iter()
                .zip(&ids)
                .map(|(s, id)| json!({ "Id": id, "Name": s.name }))
                .collect(),
        ),
    );
    let settings: Vec<Value> = rig
        .settings
        .iter()
        .zip(&ids)
        .map(|(s, id)| {
            let norm = |n: Normalization| json!({ "Minimum": num(n.minimum), "Default": num(n.default), "Maximum": num(n.maximum) });
            json!({
                "Id": id,
                "Input": s.inputs.iter().map(|i| json!({
                    "Source": { "Target": "Parameter", "Id": pid(i.parameter) },
                    "Weight": num(i.weight),
                    "Type": kind_name(i.kind),
                    "Reflect": i.reflect,
                })).collect::<Vec<_>>(),
                "Output": s.outputs.iter().map(|o| json!({
                    "Destination": { "Target": "Parameter", "Id": pid(o.parameter) },
                    "VertexIndex": o.vertex_index,
                    "Scale": num(o.scale()),
                    "Weight": num(o.weight),
                    "Type": kind_name(o.kind),
                    "Reflect": o.reflect,
                })).collect::<Vec<_>>(),
                "Vertices": s.vertices.iter().map(|v| json!({
                    "Position": { "X": num(v.position[0]), "Y": num(v.position[1]) },
                    "Mobility": num(v.mobility),
                    "Delay": num(v.delay),
                    "Acceleration": num(v.acceleration),
                    "Radius": num(v.radius),
                })).collect::<Vec<_>>(),
                "Normalization": { "Position": norm(s.position), "Angle": norm(s.angle) },
            })
        })
        .collect();
    let mut root = Map::new();
    root.insert("Version".into(), json!(3));
    root.insert("Meta".into(), Value::Object(meta));
    root.insert("PhysicsSettings".into(), Value::Array(settings));
    Some(Value::Object(root))
}

/// Parses a physics3.json; parameter IDs are resolved with `param`. Unknown
/// IDs are reported and their inputs/outputs skipped.
pub fn parse(
    text: &str,
    param: &dyn Fn(&str) -> Option<usize>,
) -> Result<(PhysicsRig, Vec<String>), PhysicsError> {
    let v: Value = serde_json::from_str(text).map_err(|e| PhysicsError::Json(e.to_string()))?;
    let f = |v: &Value, d: f32| v.as_f64().filter(|x| x.is_finite()).map_or(d, |x| x as f32);
    let kind = |v: &Value| match v.as_str() {
        Some("Y") => PhysicsKind::Y,
        Some("Angle") => PhysicsKind::Angle,
        _ => PhysicsKind::X,
    };
    let mut warnings = Vec::new();
    let meta = &v["Meta"];
    let forces = &meta["EffectiveForces"];
    let mut rig = PhysicsRig {
        gravity: [
            f(&forces["Gravity"]["X"], 0.0),
            -f(&forces["Gravity"]["Y"], -1.0),
        ],
        wind: [f(&forces["Wind"]["X"], 0.0), -f(&forces["Wind"]["Y"], 0.0)],
        fps: meta["Fps"].as_f64().map(|x| x as f32).filter(|x| *x > 0.0),
        settings: Vec::new(),
    };
    let names: Vec<(String, String)> = meta["PhysicsDictionary"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| {
                    Some((d["Id"].as_str()?.to_owned(), d["Name"].as_str()?.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    let empty = Vec::new();
    for s in v["PhysicsSettings"].as_array().unwrap_or(&empty) {
        let id = s["Id"].as_str().unwrap_or_default().to_owned();
        let mut resolve = |v: &Value| {
            let pid = v["Id"].as_str().unwrap_or_default();
            let r = param(pid);
            if r.is_none() {
                warnings.push(format!("{id}: unknown parameter {pid:?}"));
            }
            r
        };
        let mut inputs = Vec::new();
        for i in s["Input"].as_array().unwrap_or(&empty) {
            if let Some(parameter) = resolve(&i["Source"]) {
                inputs.push(PhysicsInput {
                    parameter,
                    weight: f(&i["Weight"], 100.0),
                    kind: kind(&i["Type"]),
                    reflect: i["Reflect"].as_bool().unwrap_or(false),
                });
            }
        }
        let mut outputs = Vec::new();
        for o in s["Output"].as_array().unwrap_or(&empty) {
            if let Some(parameter) = resolve(&o["Destination"]) {
                let k = kind(&o["Type"]);
                let sc = f(&o["Scale"], 1.0);
                outputs.push(PhysicsOutput {
                    parameter,
                    vertex_index: o["VertexIndex"].as_u64().unwrap_or(1).min(1 << 16) as usize,
                    translation_scale: [
                        if k == PhysicsKind::X { sc } else { 0.0 },
                        if k == PhysicsKind::Y { sc } else { 0.0 },
                    ],
                    angle_scale: if k == PhysicsKind::Angle { sc } else { 0.0 },
                    weight: f(&o["Weight"], 100.0),
                    kind: k,
                    reflect: o["Reflect"].as_bool().unwrap_or(false),
                });
            }
        }
        let vertices = s["Vertices"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .map(|v| PhysicsVertex {
                position: [f(&v["Position"]["X"], 0.0), f(&v["Position"]["Y"], 0.0)],
                mobility: f(&v["Mobility"], 1.0),
                delay: f(&v["Delay"], 1.0),
                acceleration: f(&v["Acceleration"], 1.0),
                radius: f(&v["Radius"], 0.0),
            })
            .collect();
        let n = |v: &Value| Normalization {
            minimum: f(&v["Minimum"], -10.0),
            default: f(&v["Default"], 0.0),
            maximum: f(&v["Maximum"], 10.0),
        };
        rig.settings.push(PhysicsSetting {
            guid: String::new(),
            name: names
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| id.clone()),
            id,
            enabled: true,
            inputs,
            outputs,
            vertices,
            position: n(&s["Normalization"]["Position"]),
            angle: n(&s["Normalization"]["Angle"]),
        });
    }
    Ok((rig, warnings))
}
