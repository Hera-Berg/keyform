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

/// File references found in a model3.json (paths relative to its folder).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model3 {
    pub moc: String,
    pub textures: Vec<String>,
    pub physics: Option<String>,
    pub pose: Option<String>,
    pub display_info: Option<String>,
    pub user_data: Option<String>,
    pub expressions: Vec<(String, String)>,
    pub motions: Vec<(String, Vec<String>)>,
    pub groups: Vec<(String, Vec<String>)>,
    pub hit_areas: Vec<(String, String)>,
}

impl Model3 {
    /// Parses a model3.json leniently (unknown members ignored).
    pub fn parse(text: &str) -> Result<Self> {
        let v: Value = serde_json::from_str(text)
            .map_err(|e| FormatError::Invalid(format!("model3.json: {e}")))?;
        let s = |v: &Value| v.as_str().map(str::to_owned);
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
            pose: s(&fr["Pose"]),
            display_info: s(&fr["DisplayInfo"]),
            user_data: s(&fr["UserData"]),
            expressions: fr["Expressions"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| Some((s(&e["Name"])?, s(&e["File"])?)))
                        .collect()
                })
                .unwrap_or_default(),
            motions: fr["Motions"]
                .as_object()
                .map(|o| {
                    o.iter()
                        .map(|(k, g)| {
                            (
                                k.clone(),
                                g.as_array()
                                    .map(|a| a.iter().filter_map(|m| s(&m["File"])).collect())
                                    .unwrap_or_default(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            groups: v["Groups"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|g| Some((s(&g["Name"])?, list(&g["Ids"]))))
                        .collect()
                })
                .unwrap_or_default(),
            hit_areas: v["HitAreas"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|h| Some((s(&h["Id"])?, s(&h["Name"]).unwrap_or_default())))
                        .collect()
                })
                .unwrap_or_default(),
        })
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
        assert_eq!(m.motions[0].1, vec!["m.motion3.json"]);
        assert!(Model3::parse("{}").is_err());
        assert!(Model3::parse("not json").is_err());
    }
}
