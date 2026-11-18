//! Physics presets (input and physics-model), stored as XML.
//! spec: 70-advanced/physics-editor.md §7. Keyform ships its own built-in
//! presets; the reference's values are not reproduced.

use keyform_model::{
    Normalization, Parameter, PhysicsInput, PhysicsKind, PhysicsSetting, PhysicsVertex,
};

/// User preset file name (in the user settings folder).
pub const USER_FILE: &str = "physics_preset40.xml";

#[derive(Clone, Debug, PartialEq)]
pub struct InputEntry {
    /// Alternative parameter IDs; the first one present in the model is used.
    pub sources: Vec<String>,
    pub weight: f32,
    pub kind: PhysicsKind,
    pub reflect: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Preset {
    Input {
        name: String,
        /// Position (0) and angle (1) normalisation, when given.
        position: Option<Normalization>,
        angle: Option<Normalization>,
        inputs: Vec<InputEntry>,
    },
    Model {
        name: String,
        vertices: Vec<PhysicsVertex>,
    },
}

impl Preset {
    pub fn name(&self) -> &str {
        match self {
            Preset::Input { name, .. } | Preset::Model { name, .. } => name,
        }
    }

    pub fn is_input(&self) -> bool {
        matches!(self, Preset::Input { .. })
    }
}

/// Keyform's built-in presets.
pub fn builtin() -> Vec<Preset> {
    let inp = |ids: &[&str], weight: f32, kind: PhysicsKind| InputEntry {
        sources: ids.iter().map(|s| (*s).to_owned()).collect(),
        weight,
        kind,
        reflect: false,
    };
    let v = |y: f32, mobility: f32, delay: f32, acceleration: f32, radius: f32| PhysicsVertex {
        position: [0.0, y],
        mobility,
        delay,
        acceleration,
        radius,
    };
    vec![
        Preset::Input {
            name: "Head and body (Keyform)".into(),
            position: Some(Normalization {
                minimum: -10.0,
                default: 0.0,
                maximum: 10.0,
            }),
            angle: Some(Normalization {
                minimum: -10.0,
                default: 0.0,
                maximum: 10.0,
            }),
            inputs: vec![
                inp(&["ParamAngleX", "PARAM_ANGLE_X"], 55.0, PhysicsKind::X),
                inp(&["ParamAngleZ", "PARAM_ANGLE_Z"], 55.0, PhysicsKind::Angle),
                inp(
                    &["ParamBodyAngleX", "PARAM_BODY_ANGLE_X"],
                    45.0,
                    PhysicsKind::X,
                ),
                inp(
                    &["ParamBodyAngleZ", "PARAM_BODY_ANGLE_Z"],
                    45.0,
                    PhysicsKind::Angle,
                ),
            ],
        },
        Preset::Input {
            name: "Body only (Keyform)".into(),
            position: Some(Normalization {
                minimum: -10.0,
                default: 0.0,
                maximum: 10.0,
            }),
            angle: Some(Normalization {
                minimum: -10.0,
                default: 0.0,
                maximum: 10.0,
            }),
            inputs: vec![
                inp(
                    &["ParamBodyAngleX", "PARAM_BODY_ANGLE_X"],
                    60.0,
                    PhysicsKind::X,
                ),
                inp(
                    &["ParamBodyAngleZ", "PARAM_BODY_ANGLE_Z"],
                    60.0,
                    PhysicsKind::Angle,
                ),
            ],
        },
        Preset::Model {
            name: "Short sway (Keyform)".into(),
            vertices: vec![v(0.0, 1.0, 1.0, 1.0, 0.0), v(8.0, 0.92, 0.85, 1.4, 8.0)],
        },
        Preset::Model {
            name: "Long sway (Keyform)".into(),
            vertices: vec![
                v(0.0, 1.0, 1.0, 1.0, 0.0),
                v(12.0, 0.9, 0.8, 1.2, 12.0),
                v(26.0, 0.88, 0.75, 1.1, 14.0),
            ],
        },
    ]
}

fn f3(v: f32) -> String {
    format!("{v:.3}")
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn unesc(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Writes presets in the §7 layout (numbers with 3 decimals).
pub fn write(presets: &[Preset]) -> String {
    let mut o = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<physics>\n");
    let item = |o: &mut String, n: &str, v: &str| {
        o.push_str(&format!(
            "      <item name=\"{n}\" value=\"{}\"/>\n",
            esc(v)
        ))
    };
    for p in presets {
        match p {
            Preset::Input {
                name,
                position,
                angle,
                inputs,
            } => {
                o.push_str(&format!(
                    "  <preset name=\"{}\" value=\"input\">\n",
                    esc(name)
                ));
                for (k, n) in [(0, position), (1, angle)] {
                    if let Some(n) = n {
                        o.push_str(&format!("    <group name=\"normalize\" value=\"{k}\">\n"));
                        item(&mut o, "min", &f3(n.minimum));
                        item(&mut o, "mid", &f3(n.default));
                        item(&mut o, "max", &f3(n.maximum));
                        o.push_str("    </group>\n");
                    }
                }
                for i in inputs {
                    o.push_str("    <group>\n");
                    for s in &i.sources {
                        item(&mut o, "source", s);
                    }
                    item(&mut o, "weight", &f3(i.weight));
                    let t = match i.kind {
                        PhysicsKind::X => "0",
                        PhysicsKind::Y => "1",
                        PhysicsKind::Angle => "2",
                    };
                    item(&mut o, "type", t);
                    item(&mut o, "reflect", if i.reflect { "1" } else { "0" });
                    o.push_str("    </group>\n");
                }
            }
            Preset::Model { name, vertices } => {
                o.push_str(&format!(
                    "  <preset name=\"{}\" value=\"physics_model\">\n",
                    esc(name)
                ));
                for (k, v) in vertices.iter().enumerate() {
                    o.push_str("    <group>\n");
                    item(&mut o, "index", &k.to_string());
                    item(&mut o, "position_x", &f3(v.position[0]));
                    item(&mut o, "position_y", &f3(v.position[1]));
                    item(&mut o, "mobility", &f3(v.mobility));
                    item(&mut o, "delay", &f3(v.delay));
                    item(&mut o, "acceleration", &f3(v.acceleration));
                    item(&mut o, "radius", &f3(v.radius));
                    o.push_str("    </group>\n");
                }
            }
        }
        o.push_str("  </preset>\n");
    }
    o.push_str("</physics>\n");
    o
}

/// One tag: (closing, name, attributes).
fn tags(text: &str) -> Vec<(bool, String, Vec<(String, String)>)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let Some(j) = rest.find('>') else { break };
        let body = rest[..j].trim().trim_end_matches('/');
        rest = &rest[j + 1..];
        if body.starts_with('?') || body.starts_with('!') {
            continue;
        }
        let closing = body.starts_with('/');
        let body = body.trim_start_matches('/');
        let name: String = body.chars().take_while(|c| !c.is_whitespace()).collect();
        let mut attrs = Vec::new();
        let mut a = &body[name.len()..];
        while let Some(eq) = a.find('=') {
            let key = a[..eq].trim().to_owned();
            let after = a[eq + 1..].trim_start();
            let Some(q) = after.chars().next() else { break };
            let after = &after[1..];
            let Some(end) = after.find(q) else { break };
            attrs.push((key, unesc(&after[..end])));
            a = &after[end + 1..];
        }
        out.push((closing, name, attrs));
    }
    out
}

/// Reads presets; unknown content is ignored.
pub fn parse(text: &str) -> Vec<Preset> {
    let get = |a: &[(String, String)], k: &str| a.iter().find(|x| x.0 == k).map(|x| x.1.clone());
    let num = |s: Option<String>| s.and_then(|x| x.parse::<f32>().ok());
    let mut out = Vec::new();
    let mut cur: Option<Preset> = None;
    let mut group: Option<(Option<String>, Option<String>, Vec<(String, String)>)> = None;
    for (closing, name, attrs) in tags(text) {
        match (name.as_str(), closing) {
            ("preset", false) => {
                let n = get(&attrs, "name").unwrap_or_default();
                cur = Some(
                    if get(&attrs, "value").as_deref() == Some("physics_model") {
                        Preset::Model {
                            name: n,
                            vertices: Vec::new(),
                        }
                    } else {
                        Preset::Input {
                            name: n,
                            position: None,
                            angle: None,
                            inputs: Vec::new(),
                        }
                    },
                );
            }
            ("preset", true) => out.extend(cur.take()),
            ("group", false) => {
                group = Some((get(&attrs, "name"), get(&attrs, "value"), Vec::new()))
            }
            ("item", false) => {
                if let Some(g) = &mut group {
                    g.2.push((
                        get(&attrs, "name").unwrap_or_default(),
                        get(&attrs, "value").unwrap_or_default(),
                    ));
                }
            }
            ("group", true) => {
                let Some((gname, gval, items)) = group.take() else {
                    continue;
                };
                let it = |k: &str| items.iter().find(|x| x.0 == k).map(|x| x.1.clone());
                match &mut cur {
                    Some(Preset::Input {
                        position,
                        angle,
                        inputs,
                        ..
                    }) => {
                        if gname.as_deref() == Some("normalize") {
                            let n = Normalization {
                                minimum: num(it("min")).unwrap_or(-10.0),
                                default: num(it("mid")).unwrap_or(0.0),
                                maximum: num(it("max")).unwrap_or(10.0),
                            };
                            if gval.as_deref() == Some("1") {
                                *angle = Some(n);
                            } else {
                                *position = Some(n);
                            }
                        } else {
                            inputs.push(InputEntry {
                                sources: items
                                    .iter()
                                    .filter(|x| x.0 == "source")
                                    .map(|x| x.1.clone())
                                    .collect(),
                                weight: num(it("weight")).unwrap_or(0.0),
                                kind: match it("type").as_deref() {
                                    Some("1") => PhysicsKind::Y,
                                    Some("2") => PhysicsKind::Angle,
                                    _ => PhysicsKind::X,
                                },
                                reflect: it("reflect").as_deref() == Some("1"),
                            });
                        }
                    }
                    Some(Preset::Model { vertices, .. }) => vertices.push(PhysicsVertex {
                        position: [
                            num(it("position_x")).unwrap_or(0.0),
                            num(it("position_y")).unwrap_or(0.0),
                        ],
                        mobility: num(it("mobility")).unwrap_or(1.0),
                        delay: num(it("delay")).unwrap_or(1.0),
                        acceleration: num(it("acceleration")).unwrap_or(1.0),
                        radius: num(it("radius")).unwrap_or(0.0),
                    }),
                    None => {}
                }
            }
            _ => {}
        }
    }
    out
}

/// Applies an input preset ("Reflect"): replaces inputs and normalisation.
/// Returns the IDs that weren't found ("Only existing parameters will be
/// applied").
pub fn apply_input(
    s: &mut PhysicsSetting,
    p: &Preset,
    params: &[impl std::borrow::Borrow<Parameter>],
) -> Vec<String> {
    let Preset::Input {
        position,
        angle,
        inputs,
        ..
    } = p
    else {
        return Vec::new();
    };
    let mut missing = Vec::new();
    s.inputs.clear();
    for i in inputs {
        match i
            .sources
            .iter()
            .find_map(|id| params.iter().position(|q| &q.borrow().id == id))
        {
            Some(parameter) => s.inputs.push(PhysicsInput {
                parameter,
                weight: i.weight,
                kind: i.kind,
                reflect: i.reflect,
            }),
            None => missing.push(i.sources.first().cloned().unwrap_or_default()),
        }
    }
    if let Some(n) = position {
        s.position = *n;
    }
    if let Some(n) = angle {
        s.angle = *n;
    }
    missing
}

/// Applies a physics-model preset: replaces the pendulum.
pub fn apply_model(s: &mut PhysicsSetting, p: &Preset) {
    if let Preset::Model { vertices, .. } = p {
        s.vertices = vertices.clone();
    }
}

/// An input preset from a setting.
pub fn input_from(
    name: &str,
    s: &PhysicsSetting,
    params: &[impl std::borrow::Borrow<Parameter>],
) -> Preset {
    Preset::Input {
        name: name.to_owned(),
        position: Some(s.position),
        angle: Some(s.angle),
        inputs: s
            .inputs
            .iter()
            .map(|i| InputEntry {
                sources: vec![params
                    .get(i.parameter)
                    .map_or(String::new(), |q| q.borrow().id.clone())],
                weight: i.weight,
                kind: i.kind,
                reflect: i.reflect,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_apply() {
        let b = builtin();
        let text = write(&b);
        assert!(text.contains("value=\"physics_model\"") && text.contains("value=\"55.000\""));
        assert_eq!(parse(&text), b);
        let params = vec![std::sync::Arc::new(Parameter::new(
            "ParamAngleX",
            -30.0,
            30.0,
            0.0,
        ))];
        let mut s = crate::edit::new_setting(&Default::default(), "g");
        let missing = apply_input(&mut s, &b[0], &params);
        assert_eq!(s.inputs.len(), 1);
        assert_eq!(missing.len(), 3);
        apply_model(&mut s, &b[3]);
        assert_eq!(s.vertices.len(), 3);
        // Escaping.
        let p = vec![Preset::Model {
            name: "a \"<&>\" b".into(),
            vertices: Vec::new(),
        }];
        assert_eq!(parse(&write(&p)), p);
    }
}
