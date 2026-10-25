//! Runtime JSON files: model3.json, cdi3.json, userdata3.json, and the
//! "Export For Runtime" file set.
//!
//! spec: 10-formats/runtime-json.md, 70-advanced/export-dialog.md

use std::path::{Path, PathBuf};

use keyform_model::{Model, ParamEntry};
use serde_json::{json, Map, Value};

use crate::error::{FormatError, Result};
use crate::image::RgbaImage;
use crate::moc3::{export_moc3, ExportOptions, ExportReport};
use crate::texture_cleanup;

/// Pretty-prints like the reference: tab indent, `\n` line ends, no
/// trailing newline.
pub fn to_json_string(v: &Value) -> String {
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    // Serializing a Value into a Vec cannot fail.
    if serde::Serialize::serialize(v, &mut ser).is_err() {
        return String::new();
    }
    String::from_utf8(out).unwrap_or_default()
}

/// A motion entry of model3.json (viewer.md §1.3).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MotionRef {
    pub file: String,
    pub sound: Option<String>,
    pub motion_sync: Option<String>,
    /// Seconds; `None` = not overridden.
    pub fade_in: Option<f32>,
    pub fade_out: Option<f32>,
}

/// A parameter (or part) group of model3.json, e.g. EyeBlink / LipSync.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model3Group {
    pub target: String,
    pub name: String,
    pub ids: Vec<String>,
}

/// model3.json. The parsed document is kept, so saving preserves unknown
/// members and the original key order; typed fields are written back into
/// place, new keys in the order of runtime-json.md §2.
#[derive(Clone, Debug, PartialEq)]
pub struct Model3 {
    pub moc: String,
    pub textures: Vec<String>,
    pub physics: Option<String>,
    pub user_data: Option<String>,
    pub pose: Option<String>,
    pub display_info: Option<String>,
    pub motion_sync: Option<String>,
    /// (name, file); `None` = member absent.
    pub expressions: Option<Vec<(String, String)>>,
    /// Groups in file order.
    pub motions: Option<Vec<(String, Vec<MotionRef>)>>,
    pub groups: Option<Vec<Model3Group>>,
    /// (id, name).
    pub hit_areas: Option<Vec<(String, String)>>,
    raw: Value,
    trailing_newline: bool,
}

/// Canonical member order (runtime-json.md §2).
const FILE_REF_ORDER: [&str; 10] = [
    "Moc",
    "Textures",
    "Physics",
    "UserData",
    "Pose",
    "DisplayInfo",
    "MotionSync",
    "TrackCursorInfo",
    "Expressions",
    "Motions",
];
const ROOT_ORDER: [&str; 5] = ["Version", "FileReferences", "Groups", "HitAreas", "Layout"];

/// Sets or removes `key`, keeping its position if present; a new key goes
/// before the first existing key that follows it in `order`.
fn set_ordered(map: &mut Map<String, Value>, key: &str, value: Option<Value>, order: &[&str]) {
    match value {
        None => {
            map.shift_remove(key);
        }
        Some(v) if map.contains_key(key) => {
            map.insert(key.to_owned(), v);
        }
        Some(v) => {
            let rank = |k: &str| order.iter().position(|o| *o == k).unwrap_or(usize::MAX);
            let r = rank(key);
            let mut out = Map::new();
            let mut placed = false;
            for (k, old) in std::mem::take(map) {
                if !placed && rank(&k) > r && rank(&k) != usize::MAX {
                    out.insert(key.to_owned(), v.clone());
                    placed = true;
                }
                out.insert(k, old);
            }
            if !placed {
                out.insert(key.to_owned(), v);
            }
            *map = out;
        }
    }
}

fn fade_value(x: f32) -> Value {
    serde_json::Number::from_f64((f64::from(x) * 1000.0).round() / 1000.0)
        .map_or(Value::Null, Value::Number)
}

impl Model3 {
    /// A new model3.json for `moc` with `textures`.
    pub fn new(moc: &str, textures: Vec<String>) -> Self {
        Self {
            moc: moc.to_owned(),
            textures,
            physics: None,
            user_data: None,
            pose: None,
            display_info: None,
            motion_sync: None,
            expressions: None,
            motions: None,
            groups: None,
            hit_areas: None,
            raw: json!({ "Version": 3 }),
            trailing_newline: false,
        }
    }

    /// Parses a model3.json leniently (unknown members are kept).
    pub fn parse(text: &str) -> Result<Self> {
        let v: Value = serde_json::from_str(text)
            .map_err(|e| FormatError::Invalid(format!("model3.json: {e}")))?;
        let s = |v: &Value| v.as_str().map(str::to_owned);
        let f = |v: &Value| v.as_f64().filter(|x| x.is_finite()).map(|x| x as f32);
        let fr = &v["FileReferences"];
        let moc = s(&fr["Moc"])
            .ok_or_else(|| FormatError::Invalid("model3.json lacks FileReferences.Moc".into()))?;
        let list = |v: &Value| {
            v.as_array()
                .map(|a| a.iter().filter_map(s).collect())
                .unwrap_or_default()
        };
        Ok(Self {
            moc,
            textures: list(&fr["Textures"]),
            physics: s(&fr["Physics"]),
            user_data: s(&fr["UserData"]),
            pose: s(&fr["Pose"]),
            display_info: s(&fr["DisplayInfo"]),
            motion_sync: s(&fr["MotionSync"]),
            expressions: fr["Expressions"].as_array().map(|a| {
                a.iter()
                    .filter_map(|e| Some((s(&e["Name"]).unwrap_or_default(), s(&e["File"])?)))
                    .collect()
            }),
            motions: fr["Motions"].as_object().map(|o| {
                o.iter()
                    .map(|(k, g)| {
                        let refs = g
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|m| {
                                        Some(MotionRef {
                                            file: s(&m["File"])?,
                                            sound: s(&m["Sound"]),
                                            motion_sync: s(&m["MotionSync"]),
                                            fade_in: f(&m["FadeInTime"]),
                                            fade_out: f(&m["FadeOutTime"]),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        (k.clone(), refs)
                    })
                    .collect()
            }),
            groups: v["Groups"].as_array().map(|a| {
                a.iter()
                    .map(|g| Model3Group {
                        target: s(&g["Target"]).unwrap_or_else(|| "Parameter".into()),
                        name: s(&g["Name"]).unwrap_or_default(),
                        ids: list(&g["Ids"]),
                    })
                    .collect()
            }),
            hit_areas: v["HitAreas"].as_array().map(|a| {
                a.iter()
                    .filter_map(|h| Some((s(&h["Id"])?, s(&h["Name"]).unwrap_or_default())))
                    .collect()
            }),
            trailing_newline: text.ends_with('\n'),
            raw: v,
        })
    }

    /// Serializes, preserving members this type doesn't model.
    pub fn to_json(&self) -> Value {
        let mut root = self.raw.as_object().cloned().unwrap_or_default();
        let mut fr = root
            .get("FileReferences")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let str_opt = |v: &Option<String>| v.as_ref().map(|x| json!(x));
        set_ordered(&mut fr, "Moc", Some(json!(self.moc)), &FILE_REF_ORDER);
        set_ordered(
            &mut fr,
            "Textures",
            Some(json!(self.textures)),
            &FILE_REF_ORDER,
        );
        set_ordered(&mut fr, "Physics", str_opt(&self.physics), &FILE_REF_ORDER);
        set_ordered(
            &mut fr,
            "UserData",
            str_opt(&self.user_data),
            &FILE_REF_ORDER,
        );
        set_ordered(&mut fr, "Pose", str_opt(&self.pose), &FILE_REF_ORDER);
        set_ordered(
            &mut fr,
            "DisplayInfo",
            str_opt(&self.display_info),
            &FILE_REF_ORDER,
        );
        set_ordered(
            &mut fr,
            "MotionSync",
            str_opt(&self.motion_sync),
            &FILE_REF_ORDER,
        );
        set_ordered(
            &mut fr,
            "Expressions",
            self.expressions.as_ref().map(|e| {
                Value::Array(
                    e.iter()
                        .map(|(n, f)| json!({ "Name": n, "File": f }))
                        .collect(),
                )
            }),
            &FILE_REF_ORDER,
        );
        let motions = self.motions.as_ref().map(|groups| {
            let old = fr
                .get("Motions")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let mut out = Map::new();
            for (g, refs) in groups {
                let old_list = old.get(g).and_then(Value::as_array);
                let arr = refs
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        // Keep members of the existing entry we don't model.
                        let mut o = old_list
                            .and_then(|l| l.get(i))
                            .and_then(Value::as_object)
                            .filter(|o| {
                                o.get("File").and_then(Value::as_str) == Some(r.file.as_str())
                            })
                            .cloned()
                            .unwrap_or_default();
                        let order = ["File", "Sound", "MotionSync", "FadeInTime", "FadeOutTime"];
                        set_ordered(&mut o, "File", Some(json!(r.file)), &order);
                        set_ordered(&mut o, "Sound", r.sound.as_ref().map(|x| json!(x)), &order);
                        set_ordered(
                            &mut o,
                            "MotionSync",
                            r.motion_sync.as_ref().map(|x| json!(x)),
                            &order,
                        );
                        set_ordered(&mut o, "FadeInTime", r.fade_in.map(fade_value), &order);
                        set_ordered(&mut o, "FadeOutTime", r.fade_out.map(fade_value), &order);
                        Value::Object(o)
                    })
                    .collect();
                out.insert(g.clone(), Value::Array(arr));
            }
            Value::Object(out)
        });
        set_ordered(&mut fr, "Motions", motions, &FILE_REF_ORDER);
        set_ordered(
            &mut root,
            "FileReferences",
            Some(Value::Object(fr)),
            &ROOT_ORDER,
        );
        set_ordered(
            &mut root,
            "Groups",
            self.groups.as_ref().map(|g| {
                Value::Array(
                    g.iter()
                        .map(|g| json!({ "Target": g.target, "Name": g.name, "Ids": g.ids }))
                        .collect(),
                )
            }),
            &ROOT_ORDER,
        );
        set_ordered(
            &mut root,
            "HitAreas",
            self.hit_areas.as_ref().map(|h| {
                Value::Array(
                    h.iter()
                        .map(|(i, n)| json!({ "Id": i, "Name": n }))
                        .collect(),
                )
            }),
            &ROOT_ORDER,
        );
        if !root.contains_key("Version") {
            set_ordered(&mut root, "Version", Some(json!(3)), &ROOT_ORDER);
        }
        Value::Object(root)
    }

    /// Text as written by [`Self::to_json`], keeping the source file's
    /// trailing-newline convention.
    pub fn to_string_pretty(&self) -> String {
        let mut s = to_json_string(&self.to_json());
        if self.trailing_newline {
            s.push('\n');
        }
        s
    }
}

/// model3.json written by the Modeler export (§2): Moc, Textures, Physics,
/// UserData, DisplayInfo, MotionSync in that order; Groups EyeBlink then LipSync.
pub fn model3_json(
    name: &str,
    textures: &[String],
    m: &Model,
    physics: bool,
    user_data: bool,
    display_info: bool,
) -> Value {
    let mut fr = Map::new();
    fr.insert("Moc".into(), json!(format!("{name}.moc3")));
    fr.insert("Textures".into(), json!(textures));
    if physics {
        fr.insert("Physics".into(), json!(format!("{name}.physics3.json")));
    }
    if user_data {
        fr.insert("UserData".into(), json!(format!("{name}.userdata3.json")));
    }
    if display_info {
        fr.insert("DisplayInfo".into(), json!(format!("{name}.cdi3.json")));
    }
    let mut root = Map::new();
    root.insert("Version".into(), json!(3));
    root.insert("FileReferences".into(), Value::Object(fr));
    let mut groups = Vec::new();
    for (gname, params) in [
        ("EyeBlink", &m.eye_blink_parameters),
        ("LipSync", &m.lip_sync_parameters),
    ] {
        let ids: Vec<&str> = params
            .iter()
            .filter_map(|&p| m.parameters.get(p))
            .map(|p| p.id.as_str())
            .collect();
        if !ids.is_empty() {
            groups.push(json!({ "Target": "Parameter", "Name": gname, "Ids": ids }));
        }
    }
    if !groups.is_empty() {
        root.insert("Groups".into(), Value::Array(groups));
    }
    Value::Object(root)
}

/// cdi3.json (§4). `params` is the export (palette) order.
pub fn cdi3_json(m: &Model, params: &[usize], parts: &[usize]) -> Value {
    let group_id = |g: Option<usize>| {
        g.and_then(|g| m.parameter_groups.get(g))
            .map_or(String::new(), |g| g.id.clone())
    };
    let parameters: Vec<Value> = params
        .iter()
        .map(|&p| {
            let q = &m.parameters[p];
            json!({ "Id": q.id, "GroupId": group_id(q.group), "Name": q.name })
        })
        .collect();
    // Groups in palette (tree) order.
    let mut group_order = Vec::new();
    fn walk(m: &Model, entries: &[ParamEntry], out: &mut Vec<usize>, depth: usize) {
        for e in entries {
            if let ParamEntry::Group(g) = *e {
                if g < m.parameter_groups.len() && !out.contains(&g) && depth < 64 {
                    out.push(g);
                    walk(m, &m.parameter_groups[g].children, out, depth + 1);
                }
            }
        }
    }
    walk(m, &m.root_parameter_entries, &mut group_order, 0);
    let groups: Vec<Value> = group_order
        .iter()
        .map(|&g| {
            let gr = &m.parameter_groups[g];
            json!({ "Id": gr.id, "GroupId": group_id(gr.parent), "Name": gr.name })
        })
        .collect();
    let parts_v: Vec<Value> = parts
        .iter()
        .map(|&p| {
            let i = &m.parts[p].info;
            json!({ "Id": i.id, "Name": i.display_name() })
        })
        .collect();
    let mut root = Map::new();
    root.insert("Version".into(), json!(3));
    root.insert("Parameters".into(), Value::Array(parameters));
    root.insert("ParameterGroups".into(), Value::Array(groups));
    root.insert("Parts".into(), Value::Array(parts_v));
    // Each parameter flagged "combined" pairs with the next one in palette order.
    let mut combined = Vec::new();
    let mut i = 0;
    while i + 1 < params.len() {
        if m.parameters[params[i]].combined {
            combined.push(json!([
                m.parameters[params[i]].id,
                m.parameters[params[i + 1]].id
            ]));
            i += 2;
        } else {
            i += 1;
        }
    }
    if !combined.is_empty() {
        root.insert("CombinedParameters".into(), Value::Array(combined));
    }
    Value::Object(root)
}

/// Applies a cdi3.json (§4) to a model imported from moc3: parameter names
/// and groups (the Parameter palette tree), part names. Unknown IDs are
/// ignored. Returns how many entries were applied.
pub fn apply_display_info(m: &mut Model, text: &str) -> Result<usize> {
    use keyform_model::ParameterGroup;
    let v: Value =
        serde_json::from_str(text).map_err(|e| FormatError::Invalid(format!("cdi3.json: {e}")))?;
    let s = |v: &Value| v.as_str().map(str::to_owned);
    let mut applied = 0;
    // Groups first, so parameters can be placed into them.
    let mut group_index = std::collections::HashMap::new();
    let groups: Vec<(String, String, String)> = v["ParameterGroups"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|g| {
                    Some((
                        s(&g["Id"])?,
                        s(&g["GroupId"]).unwrap_or_default(),
                        s(&g["Name"]).unwrap_or_default(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    m.parameter_groups.clear();
    for (id, _, name) in &groups {
        group_index.insert(id.clone(), m.parameter_groups.len());
        m.parameter_groups.push(std::sync::Arc::new(ParameterGroup {
            guid: String::new(),
            id: id.clone(),
            name: name.clone(),
            parent: None,
            children: Vec::new(),
        }));
    }
    let mut root = Vec::new();
    for (i, (_, parent, _)) in groups.iter().enumerate() {
        match group_index.get(parent) {
            Some(&p) if p != i => {
                std::sync::Arc::make_mut(&mut m.parameter_groups[i]).parent = Some(p);
                std::sync::Arc::make_mut(&mut m.parameter_groups[p])
                    .children
                    .push(ParamEntry::Group(i));
            }
            _ => root.push(ParamEntry::Group(i)),
        }
    }
    let pindex: std::collections::HashMap<String, usize> = m
        .parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.clone(), i))
        .collect();
    let mut placed = vec![false; m.parameters.len()];
    for p in v["Parameters"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let Some(&pi) = s(&p["Id"]).and_then(|id| pindex.get(&id)) else {
            continue;
        };
        if placed[pi] {
            continue;
        }
        placed[pi] = true;
        if let Some(n) = s(&p["Name"]).filter(|n| !n.is_empty()) {
            std::sync::Arc::make_mut(&mut m.parameters[pi]).name = n;
        }
        match s(&p["GroupId"]).and_then(|g| group_index.get(&g).copied()) {
            Some(g) => {
                std::sync::Arc::make_mut(&mut m.parameters[pi]).group = Some(g);
                std::sync::Arc::make_mut(&mut m.parameter_groups[g])
                    .children
                    .push(ParamEntry::Parameter(pi));
            }
            None => root.push(ParamEntry::Parameter(pi)),
        }
        applied += 1;
    }
    // Parameters cdi3 doesn't mention stay at the top level.
    root.extend(
        (0..m.parameters.len())
            .filter(|&i| !placed[i])
            .map(ParamEntry::Parameter),
    );
    m.root_parameter_entries = root;
    for part in v["Parts"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let (Some(id), Some(name)) = (s(&part["Id"]), s(&part["Name"])) else {
            continue;
        };
        if let Some(p) = m.parts.iter_mut().find(|p| p.info.id == id) {
            std::sync::Arc::make_mut(p).info.name = name;
            applied += 1;
        }
    }
    Ok(applied)
}

/// userdata3.json (§3), or `None` when no exported art mesh has user data.
pub fn userdata3_json(m: &Model, meshes: &[usize]) -> Option<Value> {
    let entries: Vec<(&str, &str)> = meshes
        .iter()
        .map(|&a| &m.art_meshes[a])
        .filter(|a| !a.user_data.is_empty())
        .map(|a| (a.info.id.as_str(), a.user_data.as_str()))
        .collect();
    if entries.is_empty() {
        return None;
    }
    let total: usize = entries.iter().map(|(_, v)| v.len()).sum();
    Some(json!({
        "Version": 3,
        "Meta": { "UserDataCount": entries.len(), "TotalUserDataSize": total },
        "UserData": entries.iter().map(|(id, v)| json!({ "Target": "ArtMesh", "Id": id, "Value": v })).collect::<Vec<_>>(),
    }))
}

#[derive(Clone, Debug)]
pub struct RuntimeExportOptions {
    pub moc3: ExportOptions,
    pub export_physics: bool,
    pub extend_edge_color: bool,
    pub export_display_info: bool,
    pub export_user_data: bool,
}

/// Writes the "Export For Runtime" file set into `dir` with base `name`.
/// Motion sync is not written yet. Returns the written paths and the export
/// report (physics verification messages are added to its warnings).
pub fn export_runtime(
    m: &Model,
    opts: &RuntimeExportOptions,
    dir: &Path,
    name: &str,
) -> Result<(Vec<PathBuf>, ExportReport)> {
    let (raw, mut rep) = export_moc3(m, &opts.moc3)?;
    let mut written = Vec::new();
    let mut write = |path: PathBuf, bytes: &[u8]| -> Result<()> {
        std::fs::write(&path, bytes)?;
        written.push(path);
        Ok(())
    };
    write(dir.join(format!("{name}.moc3")), &raw.write()?)?;

    let mut tex_paths = Vec::new();
    if let Some(&first) = rep.textures.first() {
        // Folder `<name>.<W>` where W = first page width (texture scale 1).
        let folder = format!("{name}.{}", m.textures[first].width);
        std::fs::create_dir_all(dir.join(&folder))?;
        for (i, &t) in rep.textures.iter().enumerate() {
            let tex = &m.textures[t];
            let png = tex.png.as_ref().ok_or_else(|| {
                FormatError::Invalid(format!("texture {:?} has no image", tex.name))
            })?;
            let img = RgbaImage::decode_png(png)?;
            let img = if opts.extend_edge_color {
                texture_cleanup::extend_edge_color(&img)
            } else {
                texture_cleanup::default_cleanup(&img)
            };
            let rel = format!("{folder}/texture_{i:02}.png");
            write(dir.join(&rel), &img.encode_png()?)?;
            tex_paths.push(rel);
        }
    }
    // physics3.json (70-advanced/physics.md §4): verified copy, written only
    // when at least one setting remains.
    let mut physics = false;
    if opts.export_physics {
        let (rig, msgs) = keyform_physics::json3::verify(&m.physics, &m.parameters);
        rep.warnings.extend(msgs);
        if let Some(v) = keyform_physics::json3::to_json(&rig, &m.parameters, m.export.physics_fps)
        {
            write(
                dir.join(format!("{name}.physics3.json")),
                to_json_string(&v).as_bytes(),
            )?;
            physics = true;
        }
    }
    let mut user_data = false;
    if opts.export_user_data {
        if let Some(u) = userdata3_json(m, &rep.art_meshes) {
            write(
                dir.join(format!("{name}.userdata3.json")),
                to_json_string(&u).as_bytes(),
            )?;
            user_data = true;
        }
    }
    if opts.export_display_info {
        let v = cdi3_json(m, &rep.parameters, &rep.all_parts);
        write(
            dir.join(format!("{name}.cdi3.json")),
            to_json_string(&v).as_bytes(),
        )?;
    }
    let v = model3_json(
        name,
        &tex_paths,
        m,
        physics,
        user_data,
        opts.export_display_info,
    );
    write(
        dir.join(format!("{name}.model3.json")),
        to_json_string(&v).as_bytes(),
    )?;
    Ok((written, rep))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_layout_matches_reference_style() {
        let v = json!({ "Version": 3, "FileReferences": { "Moc": "a.moc3", "Textures": ["a.2048/texture_00.png"] } });
        assert_eq!(
            to_json_string(&v),
            "{\n\t\"Version\": 3,\n\t\"FileReferences\": {\n\t\t\"Moc\": \"a.moc3\",\n\t\t\"Textures\": [\n\t\t\t\"a.2048/texture_00.png\"\n\t\t]\n\t}\n}"
        );
    }

    #[test]
    fn model3_parse_is_lenient() {
        let m = Model3::parse(r#"{"Version":3,"FileReferences":{"Moc":"x.moc3","Textures":["t.png",5],"Motions":{"Idle":[{"File":"m.motion3.json"}]}},"Groups":[{"Name":"EyeBlink","Ids":["A"]}]}"#).unwrap();
        assert_eq!(m.textures, vec!["t.png"]);
        assert_eq!(m.motions.as_ref().unwrap()[0].1[0].file, "m.motion3.json");
        assert!(Model3::parse("{}").is_err());
        // Edits keep unknown members and insert new keys in canonical order.
        let mut m = Model3::parse(r#"{"Version":3,"FileReferences":{"Moc":"x.moc3","Textures":[],"Motions":{}},"Custom":1}"#).unwrap();
        m.pose = Some("x.pose3.json".into());
        m.physics = Some("x.physics3.json".into());
        let v = m.to_json();
        let keys: Vec<&String> = v["FileReferences"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["Moc", "Textures", "Physics", "Pose", "Motions"]);
        assert_eq!(v["Custom"], 1);
        assert!(Model3::parse("not json").is_err());
    }
}
