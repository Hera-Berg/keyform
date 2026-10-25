#![allow(clippy::unwrap_used)]
//! Round-trip tests over locally available sample files.
//!
//! Samples are licensed test inputs and are never committed
//! (keyform-spec/test-data/README.md). Directories searched: `$KEYFORM_SAMPLES`
//! (colon-separated), else the workspace root. Tests pass vacuously when no
//! sample is present.

use std::path::Path;

use keyform_formats::caff::CaffArchive;
use keyform_formats::xml::XmlDocument;

mod common;
use common::sample_files;

#[test]
fn cmo3_main_xml_is_byte_exact() {
    for path in sample_files("cmo3").into_iter().chain(sample_files("can3")) {
        let data = std::fs::read(&path).unwrap();
        let archive = CaffArchive::read(&data).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        assert!(archive.guard_ok, "{path:?}: guard bytes");
        let xml = &archive.main_xml().expect("main.xml").data;
        let doc = XmlDocument::parse(xml).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let written = doc.write();
        if written != *xml {
            let at = written
                .iter()
                .zip(xml.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            let lo = at.saturating_sub(200);
            panic!(
                "{path:?}: XML differs at byte {at}\n--- original\n{}\n--- written\n{}",
                String::from_utf8_lossy(&xml[lo..(at + 200).min(xml.len())]),
                String::from_utf8_lossy(&written[lo..(at + 200).min(written.len())]),
            );
        }
        eprintln!(
            "{path:?}: {} entries, main.xml {} bytes OK",
            archive.entries.len(),
            xml.len()
        );
    }
}

#[test]
fn cmo3_caff_roundtrip() {
    for path in sample_files("cmo3").into_iter().chain(sample_files("can3")) {
        let data = std::fs::read(&path).unwrap();
        let a = CaffArchive::read(&data).unwrap();
        let b = CaffArchive::read(&a.write().unwrap()).unwrap();
        assert_eq!(a, b, "{path:?}");
    }
}

#[test]
fn moc3_raw_roundtrip_is_byte_exact() {
    use keyform_formats::moc3::Moc3Raw;
    for path in sample_files("moc3") {
        let data = std::fs::read(&path).unwrap();
        let raw = Moc3Raw::read(&data).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let written = raw.write().unwrap();
        if written != data {
            let at = written
                .iter()
                .zip(&data)
                .position(|(a, b)| a != b)
                .unwrap_or(written.len().min(data.len()));
            panic!(
                "{path:?}: differs at byte {at} (len {} vs {})",
                written.len(),
                data.len()
            );
        }
        eprintln!("{path:?}: v{} byte-exact", raw.version);
    }
}

/// Exports every sample project that has a reference-exported moc3 next to it
/// (same stem, or the stem without its `_tNN` revision suffix) and requires
/// every section to match. The keys pool may carry trailing per-parameter key
/// lists that older editors did not write (spec: moc3.md §8.5; 5.3 always
/// appends them).
#[test]
fn moc3_export_matches_reference() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::moc3::{
        export_moc3, layout::SECTIONS, ExportOptions, Moc3Raw, SectionData,
    };
    let mocs = sample_files("moc3");
    let stem = |p: &Path| p.file_stem().unwrap().to_string_lossy().into_owned();
    let base = |s: &str| match s.rfind("_t") {
        Some(i) if s[i + 2..].chars().all(|c| c.is_ascii_digit()) => s[..i].to_owned(),
        _ => s.to_owned(),
    };
    for cmo3 in sample_files("cmo3") {
        let s = stem(&cmo3);
        let Some(reference) = mocs
            .iter()
            .find(|m| stem(m) == s)
            .or_else(|| mocs.iter().find(|m| stem(m) == base(&s)))
        else {
            continue;
        };
        let doc = Cmo3Document::read(&std::fs::read(&cmo3).unwrap()).unwrap();
        let want = Moc3Raw::read(&std::fs::read(reference).unwrap()).unwrap();
        let mut opts = ExportOptions::for_model(&doc.model);
        opts.version = want.version;
        let (got, rep) = export_moc3(&doc.model, &opts).unwrap();
        // cdi3.json next to the reference moc3 must match byte for byte.
        let cdi3 = reference.with_extension("cdi3.json");
        if let Ok(want_cdi3) = std::fs::read_to_string(&cdi3) {
            use keyform_formats::runtime_json::{cdi3_json, to_json_string};
            let got_cdi3 = to_json_string(&cdi3_json(&doc.model, &rep.parameters, &rep.all_parts));
            assert_eq!(got_cdi3, want_cdi3, "{s}: cdi3.json differs from {cdi3:?}");
        }
        // The written file must read back identically.
        assert_eq!(Moc3Raw::read(&got.write().unwrap()).unwrap(), got);
        assert_eq!(got.canvas, want.canvas, "{s}: canvas");
        let mut bad = Vec::new();
        for sec in SECTIONS.iter().filter(|x| x.since <= want.version) {
            let (g, w) = (&got.sections[sec.name], &want.sections[sec.name]);
            let ok = match (sec.name, g, w) {
                ("keys", SectionData::F32(g), SectionData::F32(w)) => {
                    g.starts_with(w) || w.starts_with(g)
                }
                _ => g == w,
            };
            if !ok {
                bad.push(sec.name);
            }
        }
        assert!(
            bad.is_empty(),
            "{s}: sections differ from {reference:?}: {bad:?}"
        );
        eprintln!("{s}: export matches {reference:?} (v{})", want.version);
    }
}

/// Texture cleanup (runtime-json.md §1.2) against reference-exported pages.
/// Locked atlas pages are exported from the stored image and must match bit
/// for bit; unlocked pages are re-rendered by the reference first
/// (texture-atlas.md §6, not implemented yet), so they are only reported.
#[test]
fn texture_cleanup_matches_reference_pages() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::image::RgbaImage;
    use keyform_formats::texture_cleanup::{default_cleanup, extend_edge_color};
    let mocs = sample_files("moc3");
    for cmo3 in sample_files("cmo3") {
        let stem = cmo3.file_stem().unwrap().to_string_lossy().into_owned();
        let base = stem
            .rsplit_once("_t")
            .map_or(stem.as_str(), |(b, _)| b)
            .to_owned();
        let doc = Cmo3Document::read(&std::fs::read(&cmo3).unwrap()).unwrap();
        let Some(w0) = doc.model.textures.first().map(|t| t.width) else {
            continue;
        };
        let tex_dir = |m: &Path| {
            m.with_file_name(format!("{}.{w0}", m.file_stem().unwrap().to_string_lossy()))
        };
        let Some(reference) = mocs.iter().find(|m| {
            let s = m.file_stem().unwrap().to_string_lossy();
            (s == stem.as_str() || s == base.as_str()) && tex_dir(m).is_dir()
        }) else {
            continue;
        };
        for (i, tex) in doc.model.textures.iter().enumerate() {
            let dir = tex_dir(reference);
            let Ok(want) = std::fs::read(dir.join(format!("texture_{i:02}.png"))) else {
                continue;
            };
            let want = RgbaImage::decode_png(&want).unwrap();
            let page = RgbaImage::decode_png(tex.png.as_ref().unwrap()).unwrap();
            let got = default_cleanup(&page);
            let diff = got
                .data
                .chunks_exact(4)
                .zip(want.data.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count();
            eprintln!(
                "{stem} page {i} (locked={}): default cleanup differs in {diff} px",
                tex.locked
            );
            if tex.locked {
                assert_eq!(diff, 0, "{stem} page {i}: default cleanup not bit-exact");
            }
            // Extend edge color never changes alpha (§1.1).
            let ext = extend_edge_color(&page);
            assert!(ext
                .data
                .iter()
                .skip(3)
                .step_by(4)
                .eq(page.data.iter().skip(3).step_by(4)));
        }
    }
}

/// model3.json files written by the reference (any found under the sample
/// directories, one level deep) must re-serialize byte for byte.
#[test]
fn model3_json_roundtrip_is_byte_exact() {
    use keyform_formats::runtime_json::Model3;
    let mut files = Vec::new();
    for d in std::env::var("KEYFORM_SAMPLES")
        .map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
        .unwrap_or_default()
    {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.path().to_string_lossy().ends_with(".model3.json") {
                files.push(e.path());
            }
        }
    }
    for p in files {
        let text = std::fs::read_to_string(&p).unwrap();
        let m = Model3::parse(&text).unwrap();
        assert_eq!(m.to_string_pretty(), text, "{p:?}");
        eprintln!("{p:?}: model3.json byte-exact");
    }
}

/// Saving without edits reproduces main.xml byte for byte; edits survive a
/// save and reload and touch nothing else; unsupported edits fail cleanly.
#[test]
fn cmo3_save_after_edits() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::DeformerKind;
    use std::sync::Arc;
    for path in sample_files("cmo3") {
        let data = std::fs::read(&path).unwrap();
        let mut doc = Cmo3Document::read(&data).unwrap();
        let original = doc.xml.write();
        let unchanged = doc.save_bytes().unwrap();
        let back = Cmo3Document::read(&unchanged).unwrap();
        assert_eq!(back.xml.write(), original, "{path:?}: unchanged save");

        let m = &mut doc.model;
        if let Some(p) = m.parameters.first_mut() {
            let p = Arc::make_mut(p);
            p.name = format!("{} edited & <renamed>", p.name);
            p.max += 1.0;
        }
        if let Some(p) = m.parts.get_mut(1) {
            let p = Arc::make_mut(p);
            p.info.visible = !p.info.visible;
            p.info.name = "x".repeat(70);
        }
        if let Some(a) = m.art_meshes.first_mut() {
            let a = Arc::make_mut(a);
            let f = &mut a.grid.keyforms[0];
            f.positions[0] += 12.5;
            f.opacity = 0.25;
            f.multiply = [0.5, 0.25, 1.0, 1.0];
            a.culling = !a.culling;
        }
        if let Some(d) = m
            .deformers
            .iter_mut()
            .find(|d| matches!(d.kind, DeformerKind::Rotation(_)))
        {
            if let DeformerKind::Rotation(r) = &mut Arc::make_mut(d).kind {
                r.grid.keyforms[0].angle += 10.0;
            }
        }
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap();
        assert!(!doc.has_unsynced_changes());
        let back = Cmo3Document::read(&bytes).unwrap();
        assert_eq!(back.model, edited, "{path:?}: edits after reload");

        let (a, b) = (String::from_utf8_lossy(&original), back.xml.write());
        let b = String::from_utf8_lossy(&b);
        // Lines of the new document not in the old one (as a multiset).
        let mut pool = std::collections::HashMap::<&str, i64>::new();
        for l in a.lines() {
            *pool.entry(l).or_default() += 1;
        }
        let mut changed = 0;
        for l in b.lines() {
            match pool.get_mut(l) {
                Some(c) if *c > 0 => *c -= 1,
                _ => changed += 1,
            }
        }
        assert!(changed <= 12, "{path:?}: {changed} lines changed");
        eprintln!("{path:?}: edited save OK ({changed} lines changed)");

        // Structural edits are refused and leave the document as it was.
        let before = doc.xml.write();
        doc.model.parts.pop();
        assert!(doc.sync().is_err());
        assert!(doc.has_unsynced_changes());
        assert_eq!(doc.xml.write(), before);
    }
}

/// Keys added and removed (keys.md) survive a save and reload: new keyforms,
/// new bindings and dropped keyforms are written into the grid.
#[test]
fn cmo3_save_after_key_edits() {
    use keyform_edit::{add_key, keys_of, remove_key, ObjectRef};
    use keyform_formats::cmo3::Cmo3Document;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &mut doc.model;
        // A mesh bound to one parameter with ≥ 2 keys: add the midpoint key.
        let pick = (0..m.art_meshes.len()).find(|&i| {
            let g = &m.art_meshes[i].grid;
            g.bindings.len() == 1 && g.bindings[0].keys.len() >= 2
        });
        let Some(a) = pick else { continue };
        let o = ObjectRef::ArtMesh(a);
        let p = m.art_meshes[a].grid.bindings[0].parameter;
        let k = keys_of(m, o, p);
        let mid = (k[0] + k[1]) / 2.0;
        assert_eq!(add_key(m, o, p, mid), Ok(true));
        // Bind a second, unrelated parameter with two keys.
        let q = (0..m.parameters.len())
            .find(|&q| {
                q != p
                    && !m.parameters[q].repeat
                    && m.parameters[q].kind == keyform_model::ParameterKind::Normal
            })
            .unwrap();
        let (lo, hi) = (m.parameters[q].min, m.parameters[q].max);
        assert_eq!(add_key(m, o, q, lo), Ok(true));
        assert_eq!(add_key(m, o, q, hi), Ok(true));
        // Another object: remove a key.
        let other = (0..m.art_meshes.len()).find(|&i| {
            i != a
                && m.art_meshes[i]
                    .grid
                    .bindings
                    .iter()
                    .any(|b| b.keys.len() >= 3)
        });
        if let Some(b) = other {
            let bind = &m.art_meshes[b].grid.bindings;
            let j = bind.iter().position(|x| x.keys.len() >= 3).unwrap();
            let (pp, kk) = (bind[j].parameter, bind[j].keys[1]);
            if !m.parameters[pp].repeat {
                assert_eq!(remove_key(m, ObjectRef::ArtMesh(b), pp, kk), Ok(true));
            }
        }
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{path:?}: {:?}", back.warnings);
        assert!(back.model.same_data(&edited), "{path:?}: keys after reload");
        assert_eq!(keys_of(&back.model, o, q), vec![lo, hi]);
        // Saving again without changes keeps the bytes.
        let mut again = back;
        assert_eq!(again.save_bytes().unwrap(), bytes);
        eprintln!("{path:?}: key edits saved and reloaded");
    }
}

/// The mask check on the samples, and on hiyori with Angle X widened (the
/// reference listed Warp10 and Warp11 for that edit).
#[test]
fn mask_check_on_samples() {
    use keyform_edit::{mask_issues, ObjectRef};
    use keyform_formats::cmo3::Cmo3Document;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let i = mask_issues(&doc.model);
        eprintln!(
            "{path:?}: deleted {}, hidden {}, outside {}",
            i.deleted.len(),
            i.hidden.len(),
            i.outside.len()
        );
        let m = &mut doc.model;
        let Some(p) = m.parameters.iter().position(|p| p.id == "ParamAngleX") else {
            continue;
        };
        std::sync::Arc::make_mut(&mut m.parameters[p]).max += 10.0;
        let after = mask_issues(m);
        let names: Vec<String> = after
            .outside
            .iter()
            .filter(|(_, b)| b.iter().any(|b| b.parameter == p && b.max))
            .map(|(o, _)| match o {
                ObjectRef::Deformer(d) => m.deformers[*d].info.id.clone(),
                ObjectRef::ArtMesh(a) => m.art_meshes[*a].info.id.clone(),
                ObjectRef::Part(x) => m.parts[*x].info.id.clone(),
                ObjectRef::Glue(g) => m.glues[*g].info.id.clone(),
            })
            .collect();
        eprintln!("  Angle X max +10 → {names:?}");
        if path.to_string_lossy().contains("hiyori_pro") {
            assert!(names.contains(&"Warp10".to_owned()) && names.contains(&"Warp11".to_owned()));
        }
    }
}

/// Parameters created and deleted (with their bindings, groups, effect
/// lists, tracking entries and physics) survive a save and reload.
#[test]
fn cmo3_save_after_parameter_edits() {
    use keyform_edit::objects::{add_parameter, remove_parameter};
    use keyform_edit::{add_key, ObjectRef};
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::Parameter;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &mut doc.model;
        // Delete a parameter that objects are bound to.
        let victim = (0..m.parameters.len())
            .find(|&p| {
                m.art_meshes
                    .iter()
                    .any(|a| a.grid.bindings.iter().any(|b| b.parameter == p))
            })
            .unwrap();
        let victim_id = m.parameters[victim].id.clone();
        assert!(remove_parameter(m, victim));
        // New parameters: top level, and inside the first group.
        let top = add_parameter(
            m,
            Parameter::new("ParamKeyformTest", -10.0, 10.0, 0.0),
            None,
        );
        let group = (!m.parameter_groups.is_empty()).then_some(0);
        let inner = add_parameter(m, Parameter::new("ParamKeyformTest2", 0.0, 1.0, 0.0), group);
        add_key(m, ObjectRef::ArtMesh(0), top, -10.0).unwrap();
        add_key(m, ObjectRef::ArtMesh(0), top, 10.0).unwrap();
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{path:?}: {:?}", back.warnings);
        assert!(back.model.same_data(&edited), "{path:?}");
        assert!(back.model.parameters.iter().all(|p| p.id != victim_id));
        assert_eq!(back.model.parameters[inner].group, group);
        let mut again = back;
        assert_eq!(again.save_bytes().unwrap(), bytes);
        eprintln!("{path:?}: deleted {victim_id}, added two parameters");
    }
}

/// Creating and deleting deformers keeps every adopted object's canvas
/// shape (change-parent.md), at the defaults and at its keyforms.
#[test]
fn deformer_creation_keeps_shapes() {
    use keyform_edit::objects::{
        create_rotation_deformer, create_warp_deformer, delete_deformer, NewDeformer,
    };
    use keyform_edit::ObjectRef;
    use keyform_eval::Evaluator;
    use keyform_formats::cmo3::Cmo3Document;
    for path in sample_files("cmo3") {
        let doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let mut m = doc.model.clone();
        let defaults: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
        // A mesh under a warp deformer with at least one binding.
        let Some(a) = (0..m.art_meshes.len()).find(|&a| {
            let x = &m.art_meshes[a];
            x.grid.bindings.len() == 1
                && x.target.is_some_and(|t| {
                    matches!(m.deformers[t].kind, keyform_model::DeformerKind::Warp(_))
                })
        }) else {
            continue;
        };
        let b = m.art_meshes[a].grid.bindings[0].clone();
        let sets: Vec<Vec<f32>> = std::iter::once(defaults.clone())
            .chain(b.keys.iter().map(|&k| {
                let mut v = defaults.clone();
                v[b.parameter] = k;
                v
            }))
            .collect();
        let shape = |m: &keyform_model::Model| -> Vec<Vec<f32>> {
            let ev = Evaluator::new(m);
            sets.iter()
                .map(|v| ev.evaluate(v).meshes[a].positions.clone())
                .collect()
        };
        let before = shape(&m);
        let close = |x: &[Vec<f32>], y: &[Vec<f32>], what: &str| {
            for (p, q) in x.iter().zip(y) {
                let err = p
                    .iter()
                    .zip(q)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0f32, f32::max);
                assert!(err < 0.05, "{path:?} {what}: max error {err} px");
            }
        };
        let parent = m.art_meshes[a].target;
        let part = m.art_meshes[a].info.parent_part;
        let children = [ObjectRef::ArtMesh(a)];
        let nd = NewDeformer {
            name: "Test Rotation".into(),
            part,
            parent,
            children: &children,
            values: &defaults,
        };
        let r = create_rotation_deformer(&mut m, &nd).unwrap();
        assert_eq!(m.art_meshes[a].target, Some(r));
        close(&before, &shape(&m), "under a new rotation deformer");
        let nd = NewDeformer {
            name: "Test Warp".into(),
            part,
            parent: Some(r),
            children: &children,
            values: &defaults,
        };
        let w = create_warp_deformer(&mut m, &nd, 5, 5, (2, 2)).unwrap();
        assert_eq!(m.art_meshes[a].target, Some(w));
        close(&before, &shape(&m), "under a new warp in it");
        delete_deformer(&mut m, w, &defaults).unwrap();
        delete_deformer(&mut m, r, &defaults).unwrap();
        assert_eq!(m.art_meshes[a].target, parent);
        close(&before, &shape(&m), "after deleting both");
        assert_eq!(m.deformers.len(), doc.model.deformers.len());
        eprintln!("{path:?}: mesh {} kept its shape", m.art_meshes[a].info.id);
    }
}

/// New and deleted parts and deformers, and objects moved under them, are
/// written into the project and reload exactly.
#[test]
fn cmo3_save_after_structural_edits() {
    use keyform_edit::objects::{
        create_part, create_rotation_deformer, create_warp_deformer, delete_deformer, delete_part,
        NewDeformer,
    };
    use keyform_edit::ObjectRef;
    use keyform_formats::cmo3::Cmo3Document;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let defaults: Vec<f32> = doc.model.parameters.iter().map(|p| p.default).collect();
        let m = &mut doc.model;
        let Some(a) = (0..m.art_meshes.len()).find(|&a| m.art_meshes[a].target.is_some()) else {
            continue;
        };
        let (parent, part) = (m.art_meshes[a].target, m.art_meshes[a].info.parent_part);
        let new_part = create_part(m, "Keyform Part", part);
        let children = [ObjectRef::ArtMesh(a)];
        let r = create_rotation_deformer(
            m,
            &NewDeformer {
                name: "Keyform Rotation".into(),
                part: Some(new_part),
                parent,
                children: &children,
                values: &defaults,
            },
        )
        .unwrap();
        let w = create_warp_deformer(
            m,
            &NewDeformer {
                name: "Keyform Warp".into(),
                part: Some(new_part),
                parent: Some(r),
                children: &children,
                values: &defaults,
            },
            4,
            3,
            (2, 2),
        )
        .unwrap();
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{path:?}: {:?}", back.warnings);
        assert!(back.model.same_data(&edited), "{path:?}: after creation");
        // Undo it all by deletion and save again.
        let mut doc = back;
        let m = &mut doc.model;
        delete_deformer(m, w, &defaults).unwrap();
        delete_deformer(m, r, &defaults).unwrap();
        delete_part(m, new_part).unwrap();
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.model.same_data(&edited), "{path:?}: after deletion");
        assert_eq!(back.model.art_meshes[a].target, parent);
        eprintln!("{path:?}: structural edits saved and reloaded");
    }
}

/// Re-triangulating an unchanged editable mesh reproduces the stored
/// triangles (spec: 30-algorithms/mesh-triangulation.md), and the graph's
/// points are the art mesh's base positions (10-formats/cmo3.md §4).
#[test]
fn retriangulation_matches_samples() {
    use keyform_edit::mesh::MeshGraph;
    use keyform_formats::cmo3::Cmo3Document;
    use std::collections::HashSet;
    let set = |ix: &[u32]| -> HashSet<[u32; 3]> {
        ix.chunks_exact(3)
            .map(|t| {
                let mut s = [t[0], t[1], t[2]];
                s.sort_unstable();
                s
            })
            .collect()
    };
    for path in sample_files("cmo3") {
        let doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let (mut same, mut total, mut tris, mut tri_same) = (0, 0, 0, 0);
        let mut worst = Vec::new();
        for a in &doc.model.art_meshes {
            if a.mesh.edges.is_empty() {
                continue;
            }
            total += 1;
            let mut g = MeshGraph::of(a);
            let t = g.triangulate().unwrap();
            let (want, got) = (set(&a.indices), set(&t.indices));
            tris += want.len();
            tri_same += want.intersection(&got).count();
            if want == got {
                same += 1;
            } else {
                worst.push((
                    a.info.id.clone(),
                    want.len(),
                    got.len(),
                    want.intersection(&got).count(),
                    t.unresolved,
                ));
            }
        }
        worst.sort_by_key(|w| w.3 as isize - w.1 as isize);
        eprintln!(
            "{path:?}: {same}/{total} meshes identical, {tri_same}/{tris} triangles; worst {:?}",
            &worst[..worst.len().min(5)]
        );
        assert!(tri_same * 100 >= tris * 97, "{tri_same}/{tris}");
    }
}

/// Mesh-editor topology changes (new, split and deleted points with
/// remapped keyforms) survive a save and reload.
#[test]
fn cmo3_save_after_mesh_edits() {
    use keyform_edit::mesh::{set_mesh, MeshGraph};
    use keyform_formats::cmo3::Cmo3Document;
    use std::collections::HashSet;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &mut doc.model;
        let glued: HashSet<usize> = m
            .glues
            .iter()
            .flat_map(|g| [g.mesh_a, g.mesh_b])
            .flatten()
            .collect();
        let picks: Vec<usize> = (0..m.art_meshes.len())
            .filter(|&i| {
                let a = &m.art_meshes[i];
                !a.mesh.edges.is_empty()
                    && !a.mesh.observed
                    && !glued.contains(&i)
                    && a.grid.keyforms.len() > 1
            })
            .take(3)
            .collect();
        for &i in &picks {
            let a = m.art_meshes[i].clone();
            let mut g = MeshGraph::of(&a);
            let t = &a.indices[..3];
            let c = t.iter().fold([0.0f32; 2], |c, &v| {
                let p = g.point(v as usize);
                [c[0] + p[0] / 3.0, c[1] + p[1] / 3.0]
            });
            g.add_point(c);
            let [p, q] = g.mesh.edges[0].map(|v| g.point(v as usize));
            g.split_edge(0, [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0]);
            g.delete_points(&HashSet::from([g.len() / 2]));
            set_mesh(m, i, &g).unwrap();
            assert_eq!(m.art_meshes[i].vertex_count(), a.vertex_count() + 1);
        }
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{path:?}: {:?}", back.warnings);
        assert!(back.model.same_data(&edited), "{path:?}");
        // The export still works with the new vertex counts.
        let opts = keyform_formats::moc3::ExportOptions::for_model(&back.model);
        keyform_formats::moc3::export_moc3(&back.model, &opts)
            .unwrap_or_else(|e| panic!("{path:?}: {e}"));
        eprintln!(
            "{path:?}: {} meshes edited, saved and reloaded",
            picks.len()
        );
    }
}
