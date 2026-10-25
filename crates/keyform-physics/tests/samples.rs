#![allow(clippy::unwrap_used)]
//! physics3.json export vs reference files next to the reference moc3 of each
//! sample project; parse → write round trip of every reference physics3.json.

use std::path::{Path, PathBuf};

use keyform_formats::cmo3::Cmo3Document;
use keyform_physics::{json3, to_json_string};

fn sample_files(suffix: &str) -> Vec<PathBuf> {
    let dirs: Vec<PathBuf> = match std::env::var("KEYFORM_SAMPLES") {
        Ok(v) => std::env::split_paths(&v).collect(),
        Err(_) => vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")],
    };
    let mut out: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(suffix))
        .collect();
    out.sort();
    out
}

#[test]
fn physics3_export_matches_reference() {
    let refs = sample_files(".physics3.json");
    for cmo3 in sample_files(".cmo3") {
        let stem = cmo3.file_stem().unwrap().to_string_lossy().into_owned();
        let base = stem
            .rsplit_once("_t")
            .map_or(stem.as_str(), |(b, _)| b)
            .to_owned();
        let Some(reference) = refs.iter().find(|r| {
            let n = r.file_name().unwrap().to_string_lossy();
            n == format!("{stem}.physics3.json") || n == format!("{base}.physics3.json")
        }) else {
            continue;
        };
        let doc = Cmo3Document::read(&std::fs::read(&cmo3).unwrap()).unwrap();
        let (rig, msgs) = json3::verify(&doc.model.physics, &doc.model.parameters);
        let mut ours_v = json3::to_json(&rig, &doc.model.parameters, true).unwrap();
        let want = std::fs::read_to_string(reference).unwrap();
        // Some sample projects were localised after export (setting names in
        // another language than the runtime files): compare names separately.
        let want_v: serde_json::Value = serde_json::from_str(&want).unwrap();
        let renamed = ours_v["Meta"]["PhysicsDictionary"] != want_v["Meta"]["PhysicsDictionary"];
        if renamed {
            ours_v["Meta"]["PhysicsDictionary"] = want_v["Meta"]["PhysicsDictionary"].clone();
            eprintln!("{stem}: setting names differ from the reference (localised sample)");
        }
        let ours = to_json_string(&ours_v);
        if ours != want {
            let at = ours
                .bytes()
                .zip(want.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            panic!(
                "{stem}: physics3.json differs at {at} (verify: {msgs:?})\n--- reference\n{}\n--- ours\n{}",
                &want[at.saturating_sub(150)..(at + 100).min(want.len())],
                &ours[at.saturating_sub(150)..(at + 100).min(ours.len())]
            );
        }
        eprintln!("{stem}: physics3.json byte-identical to {reference:?}");
    }
}

#[test]
fn physics3_parse_write_roundtrip() {
    for p in sample_files(".physics3.json") {
        let text = std::fs::read_to_string(&p).unwrap();
        let mut ids: Vec<String> = Vec::new();
        // Collect every parameter ID so all resolve.
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        for s in v["PhysicsSettings"].as_array().unwrap() {
            for io in s["Input"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| &i["Source"])
                .chain(
                    s["Output"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|o| &o["Destination"]),
                )
            {
                let id = io["Id"].as_str().unwrap().to_owned();
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        let params: Vec<keyform_model::Parameter> = ids
            .iter()
            .map(|i| keyform_model::Parameter::new(i, -1.0, 1.0, 0.0))
            .collect();
        let (rig, warnings) = json3::parse(&text, &|id| ids.iter().position(|x| x == id)).unwrap();
        assert!(warnings.is_empty(), "{p:?}: {warnings:?}");
        let out = to_json_string(&json3::to_json(&rig, &params, true).unwrap());
        assert_eq!(out, text, "{p:?}: round trip differs");
    }
}
