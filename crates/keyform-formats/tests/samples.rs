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
/// triangles (spec: 30-algorithms/mesh-triangulation.md §2–3, face fans
/// §3a). Stored lists that are a strict subset (some hiyori meshes) get
/// filled, as in the reference (Q-0015).
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
        let (mut fan_total, mut fan_exact) = (0, 0);
        let mut worst = Vec::new();
        for a in &doc.model.art_meshes {
            if a.mesh.edges.is_empty() {
                continue;
            }
            total += 1;
            let mut g = MeshGraph::of(a);
            let t = g.triangulate().unwrap();
            let (want, got) = (set(&a.indices), set(&t.indices));
            if !a.mesh.delaunay {
                // Face fans keep their traced orientation (§3a): compare
                // the triangles with their vertex order (up to rotation).
                let cyc = |ix: &[u32]| -> HashSet<[u32; 3]> {
                    ix.chunks_exact(3)
                        .map(|t| {
                            let k = (0..3).min_by_key(|&k| t[k]).unwrap_or(0);
                            [t[k], t[(k + 1) % 3], t[(k + 2) % 3]]
                        })
                        .collect()
                };
                fan_total += 1;
                fan_exact += usize::from(cyc(&a.indices) == cyc(&t.indices));
            }
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
        if fan_total > 0 {
            eprintln!(
                "  face-fan meshes with identical oriented triangles: {fan_exact}/{fan_total}"
            );
            assert_eq!(fan_exact, fan_total, "{path:?}");
        }
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

/// Automatic mesh generation from the model images (auto-mesh.md):
/// generated meshes and their settings survive a save and reload.
#[test]
fn automesh_on_samples() {
    use keyform_edit::automesh::{auto_mesh, is_glued, presets, AlphaImage};
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::image::RgbaImage;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &mut doc.model;
        let with_image = m
            .art_meshes
            .iter()
            .filter(|a| a.model_image.is_some())
            .count();
        let picks: Vec<usize> = (0..m.art_meshes.len())
            .filter(|&i| {
                let a = &m.art_meshes[i];
                a.model_image.is_some() && !a.mesh.observed && !is_glued(m, i)
            })
            .take(3)
            .collect();
        let (_, setting, scale) = presets()[1];
        for &i in &picks {
            let mi = &m.model_images[m.art_meshes[i].model_image.unwrap()];
            let img = RgbaImage::decode_png(mi.png.as_ref().unwrap()).unwrap();
            let img = AlphaImage {
                width: img.width,
                height: img.height,
                alpha: img.data.chunks_exact(4).map(|p| p[3]).collect(),
            };
            auto_mesh(m, i, &setting, scale, &img).unwrap();
            assert_eq!(m.art_meshes[i].generator, setting);
        }
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.model.same_data(&edited), "{path:?}");
        eprintln!(
            "{path:?}: {with_image}/{} meshes have a model image; {} regenerated",
            edited.art_meshes.len(),
            picks.len()
        );
    }
}

/// Clipping edits (palettes.md §4): new mask lists, cleared lists and
/// invert survive a save and reload; invalid IDs are dropped.
#[test]
fn cmo3_save_after_clip_edits() {
    use keyform_edit::objects::{clip_users, set_clip_ids};
    use keyform_formats::cmo3::Cmo3Document;
    use std::sync::Arc;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &mut doc.model;
        if m.art_meshes.len() < 4 {
            continue;
        }
        let ids: Vec<String> = m.art_meshes.iter().map(|a| a.info.id.clone()).collect();
        // Mesh 0 clipped by meshes 1 and 2; mesh 1 can't then be clipped by 0.
        let dropped = set_clip_ids(m, 0, &[&ids[1], &ids[2], "NoSuchMesh", &ids[0]]);
        assert_eq!(dropped, vec!["NoSuchMesh".to_owned(), ids[0].clone()]);
        assert_eq!(set_clip_ids(m, 1, &[&ids[0]]), vec![ids[0].clone()]);
        assert_eq!(clip_users(m, 2), vec![0]);
        Arc::make_mut(&mut m.art_meshes[0]).invert_clip = true;
        // Clear an existing clip list, if any.
        let cleared = (0..m.art_meshes.len()).find(|&k| k > 2 && !m.art_meshes[k].clip.is_empty());
        if let Some(k) = cleared {
            set_clip_ids(m, k, &[]);
        }
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{path:?}: {:?}", back.warnings);
        assert!(back.model.same_data(&edited), "{path:?}");
        assert_eq!(back.model.art_meshes[0].clip, vec![1, 2]);
        eprintln!("{path:?}: clip edits saved (cleared {cleared:?})");
    }
}

/// Deleting art meshes and creating, editing and deleting glue
/// (glue.md "Editing", dialogs.md §21 deleted masks) survive a save and
/// reload.
#[test]
fn cmo3_save_after_mesh_and_glue_edits() {
    use keyform_edit::objects::{bind_vertices, delete_art_mesh, delete_glue, unbind_vertices};
    use keyform_formats::cmo3::Cmo3Document;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let original = doc.model.clone();
        let m = &mut doc.model;
        // A mask that others use: deleting it leaves deleted-mask references.
        let mask = (0..m.art_meshes.len()).find(|&k| {
            m.art_meshes.iter().any(|a| a.clip.contains(&k))
                && !m
                    .glues
                    .iter()
                    .any(|g| g.mesh_a == Some(k) || g.mesh_b == Some(k))
        });
        let users = mask.map_or(0, |k| {
            m.art_meshes.iter().filter(|a| a.clip.contains(&k)).count()
        });
        if let Some(k) = mask {
            delete_art_mesh(m, k).unwrap();
        }
        // Glue two unglued meshes, then add and remove a pair.
        let free: Vec<usize> = (0..m.art_meshes.len())
            .filter(|&k| m.art_meshes[k].vertex_count() >= 3 && !m.art_meshes[k].mesh.observed)
            .take(2)
            .collect();
        let g = bind_vertices(m, (free[0], 0), (free[1], 1)).unwrap();
        bind_vertices(m, (free[0], 2), (free[1], 2)).unwrap();
        assert!(
            bind_vertices(m, (free[1], 1), (free[0], 0)).is_err(),
            "same points"
        );
        assert!(unbind_vertices(m, (free[0], 2), (free[1], 2)).unwrap());
        // Delete an existing glue, if any (other than the new one).
        let old_glue = (0..m.glues.len()).find(|&k| k != g);
        if let Some(k) = old_glue {
            delete_glue(m, k).unwrap();
        }
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{path:?}: {:?}", back.warnings);
        assert!(back.model.same_data(&edited), "{path:?}: reload differs");
        let missing = back
            .model
            .art_meshes
            .iter()
            .filter(|a| a.clip_missing)
            .count();
        eprintln!(
            "{path:?}: deleted mask {mask:?} ({users} users, {missing} now reference a deleted mask); \
             glue created; old glue deleted: {old_glue:?}"
        );
        // Re-saving the reloaded file is stable.
        let mut again = back;
        let b2 = again.save_bytes().unwrap();
        assert_eq!(b2, bytes, "{path:?}: re-save");
        // Undoing everything after the save: the deleted objects come back
        // as they were.
        doc.model = original.clone();
        let bytes = doc
            .save_bytes()
            .unwrap_or_else(|e| panic!("{path:?} undo: {e}"));
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{path:?}: {:?}", back.warnings);
        assert!(back.model.same_data(&original), "{path:?}: after undo");
    }
}

/// PSD reader on the sample PSDs: layer tree and pixels (psd-import.md §3).
/// Writes a reduced composite of the visible layers for inspection when
/// `KEYFORM_PSD_PREVIEW` names a folder.
#[test]
fn psd_samples() {
    use keyform_formats::psd::{Psd, PsdNode};
    for path in sample_files("psd") {
        let data = std::fs::read(&path).unwrap();
        let psd = Psd::parse(&data).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        fn count(n: &[PsdNode], depth: usize, groups: &mut usize, out: &mut Vec<String>) {
            for x in n {
                match x {
                    PsdNode::Layer(l) => out.push(format!(
                        "{}{} {:?} {}x{}{}",
                        "  ".repeat(depth),
                        l.name,
                        std::str::from_utf8(&l.blend).unwrap_or("?"),
                        l.bounds.width,
                        l.bounds.height,
                        if l.visible { "" } else { " (hidden)" }
                    )),
                    PsdNode::Group(g) => {
                        *groups += 1;
                        out.push(format!(
                            "{}[{}]{}",
                            "  ".repeat(depth),
                            g.name,
                            if g.visible { "" } else { " (hidden)" }
                        ));
                        count(&g.children, depth + 1, groups, out);
                    }
                }
            }
        }
        let (mut groups, mut lines) = (0, Vec::new());
        count(&psd.nodes, 0, &mut groups, &mut lines);
        let layers = psd.layers();
        eprintln!(
            "{path:?}: {}x{}, {} layers, {groups} groups; first lines:\n{}",
            psd.width,
            psd.height,
            layers.len(),
            lines
                .iter()
                .take(25)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(!layers.is_empty());
        // Decode every layer (one at a time) and composite the visible ones
        // at 1/8 scale, ignoring group visibility for simplicity.
        let k = 8u32;
        let (w, h) = (psd.width / k, psd.height / k);
        let mut comp = vec![255.0f32; (w * h * 3) as usize];
        fn visible_layers<'a>(
            n: &'a [PsdNode],
            vis: bool,
            out: &mut Vec<(&'a keyform_formats::psd::PsdLayer, bool)>,
        ) {
            for x in n {
                match x {
                    PsdNode::Layer(l) => out.push((l, vis && l.visible)),
                    PsdNode::Group(g) => visible_layers(&g.children, vis && g.visible, out),
                }
            }
        }
        let mut order = Vec::new();
        visible_layers(&psd.nodes, true, &mut order);
        // Bottom first.
        for (l, vis) in order.into_iter().rev() {
            let img = psd
                .pixels(l)
                .unwrap_or_else(|e| panic!("{path:?} {}: {e}", l.name));
            assert_eq!(
                img.data.len(),
                (l.bounds.width * l.bounds.height * 4) as usize
            );
            if !vis {
                continue;
            }
            let op = f32::from(l.opacity) / 255.0;
            for y in 0..h {
                for x in 0..w {
                    let (sx, sy) = (
                        (x * k) as i64 - i64::from(l.bounds.left),
                        (y * k) as i64 - i64::from(l.bounds.top),
                    );
                    if sx < 0
                        || sy < 0
                        || sx >= i64::from(l.bounds.width)
                        || sy >= i64::from(l.bounds.height)
                    {
                        continue;
                    }
                    let j = 4 * (sy as usize * l.bounds.width as usize + sx as usize);
                    let a = f32::from(img.data[j + 3]) / 255.0 * op;
                    let o = 3 * (y * w + x) as usize;
                    for c in 0..3 {
                        comp[o + c] = comp[o + c] * (1.0 - a) + f32::from(img.data[j + c]) * a;
                    }
                }
            }
        }
        if let Some(dir) = std::env::var_os("KEYFORM_PSD_PREVIEW") {
            let mut out = keyform_formats::image::RgbaImage::new(w, h);
            for (p, c) in out.data.chunks_exact_mut(4).zip(comp.chunks_exact(3)) {
                p.copy_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
            }
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            std::fs::write(
                Path::new(&dir).join(format!("{name}.png")),
                out.encode_png().unwrap(),
            )
            .unwrap();
        }
    }
}

/// Texture manager data: layered images, model image groups and sources,
/// atlas entries (psd-import.md §8, texture-atlas.md §1).
#[test]
fn texture_manager_loads() {
    use keyform_formats::cmo3::Cmo3Document;
    for path in sample_files("cmo3") {
        let doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &doc.model;
        let sourced = m.model_images.iter().filter(|i| i.source.is_some()).count();
        let grouped = m.model_images.iter().filter(|i| i.group.is_some()).count();
        let entries: usize = m.textures.iter().map(|t| t.entries.len()).sum();
        eprintln!(
            "{path:?}: {} layered images {:?}, {} groups, {} model images ({grouped} grouped, {sourced} with a PSD layer), {entries} atlas entries",
            m.layered_images.len(),
            m.layered_images.iter().map(|l| (&l.name, l.width, l.height, l.replaced)).collect::<Vec<_>>(),
            m.model_image_groups.len(),
            m.model_images.len(),
        );
        assert_eq!(grouped, m.model_images.len());
        // Every model image's source names a known layered image.
        for mi in &m.model_images {
            if let Some((psd, _)) = &mi.source {
                assert!(m.layered_images.iter().any(|l| &l.guid == psd), "{path:?}");
            }
        }
        // Atlas entries map the model image to where its mesh UVs point:
        // the canvas → atlas transform of an art mesh's vertices gives
        // its UVs (texture-atlas.md §4).
        let mut checked = 0;
        for a in m.art_meshes.iter().take(20) {
            let (Some(t), Some(mi)) = (a.texture, a.model_image) else {
                continue;
            };
            let tex = &m.textures[t];
            let Some(e) = tex.entries.iter().find(|e| e.model_image == mi) else {
                continue;
            };
            let c = e.atlas_to_canvas;
            let det = c[0] * c[4] - c[1] * c[3];
            let (x, y) = (a.positions[0] - c[2], a.positions[1] - c[5]);
            let px = (c[4] * x - c[1] * y) / det;
            let py = (-c[3] * x + c[0] * y) / det;
            let (u, v) = (px / tex.width as f32, py / tex.height as f32);
            assert!(
                (u - a.uvs[0]).abs() < 2e-3 && (v - a.uvs[1]).abs() < 2e-3,
                "{path:?} {}: uv {u},{v} vs {:?}",
                a.info.id,
                &a.uvs[..2]
            );
            checked += 1;
        }
        assert!(checked > 0, "{path:?}");
    }
}

/// PSD import into a project (psd-import.md §6.1): layered image, model
/// images, art meshes, parts and an atlas page are saved and reload as the
/// edited model; a second save with nothing changed is byte-stable.
#[test]
fn psd_import_saves() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::psd::Psd;
    use keyform_formats::psd_import::{add_psd_layers, ImportOptions};
    // The smaller PSD keeps the test quick.
    let Some(psd_path) = sample_files("psd")
        .into_iter()
        .min_by_key(|p| std::fs::metadata(p).map_or(0, |m| m.len()))
    else {
        return;
    };
    let data = std::fs::read(&psd_path).unwrap();
    let psd = Psd::parse(&data).unwrap();
    let Some(cmo3) = sample_files("cmo3")
        .into_iter()
        .find(|p| p.to_string_lossy().contains("taka"))
    else {
        return;
    };
    let mut doc = Cmo3Document::read(&std::fs::read(&cmo3).unwrap()).unwrap();
    let n = doc.model.art_meshes.len();
    let r = add_psd_layers(
        &mut doc.model,
        "import.psd",
        &psd,
        &ImportOptions::default(),
    )
    .unwrap();
    assert_eq!(r.art_meshes.len(), psd.layers().len());
    assert_eq!(doc.model.art_meshes.len(), n + r.art_meshes.len());
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert!(back.model.same_data(&edited));
    // The new art meshes use the new atlas and read their model image.
    let a = &back.model.art_meshes[n];
    assert_eq!(a.texture, r.atlas);
    assert!(a.model_image.is_some());
    let mut again = back;
    assert_eq!(again.save_bytes().unwrap(), bytes);
}

/// A new model from a PSD can take new deformers (built-in prototypes for
/// the first deformer of a project) and saves and reloads.
#[test]
fn new_model_with_deformers() {
    use keyform_edit::objects::{create_rotation_deformer, create_warp_deformer, NewDeformer};
    use keyform_edit::ObjectRef;
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::psd::Psd;
    use keyform_formats::psd_import::new_model_from_psd;
    let Some(psd_path) = sample_files("psd")
        .into_iter()
        .min_by_key(|p| std::fs::metadata(p).map_or(0, |m| m.len()))
    else {
        return;
    };
    let data = std::fs::read(&psd_path).unwrap();
    let psd = Psd::parse(&data).unwrap();
    let (mut doc, _) = new_model_from_psd("t.psd", &psd, &Default::default()).unwrap();
    let defaults: Vec<f32> = doc.model.parameters.iter().map(|p| p.default).collect();
    let m = &mut doc.model;
    let kids = [ObjectRef::ArtMesh(0)];
    let part = m.art_meshes[0].info.parent_part;
    let nd = |name: &str, parent| NewDeformer {
        name: name.into(),
        part,
        parent,
        children: &kids,
        values: &defaults,
    };
    let r = create_rotation_deformer(m, &nd("R", None)).unwrap();
    create_warp_deformer(m, &nd("W", Some(r)), 4, 4, (2, 2)).unwrap();
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert!(back.model.same_data(&edited));
    assert_eq!(back.model.deformers.len(), 2);
}

/// Texture atlas on a new model from a PSD (texture-atlas.md §3–6): a new
/// page switches the art meshes from their model images to the atlas;
/// Auto Layout of that saved page rewrites it in place. Both saves reload
/// as the edited model.
#[test]
fn atlas_create_and_layout_saves() {
    use keyform_formats::atlas::LayoutSettings;
    use keyform_formats::atlas_edit::{auto_layout, create_atlas, default_size, Insert};
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::psd::Psd;
    use keyform_formats::psd_import::new_model_from_psd;
    let Some(psd_path) = sample_files("psd")
        .into_iter()
        .min_by_key(|p| std::fs::metadata(p).map_or(0, |m| m.len()))
    else {
        return;
    };
    let data = std::fs::read(&psd_path).unwrap();
    let psd = Psd::parse(&data).unwrap();
    let (mut doc, _) = new_model_from_psd("t.psd", &psd, &Default::default()).unwrap();
    let bytes = doc.save_bytes().unwrap();
    let mut doc = Cmo3Document::read(&bytes).unwrap();
    let (w, h) = default_size(&doc.model);
    let t = create_atlas(&mut doc.model, "Texture0", w, h, Insert::All).unwrap();
    assert!(doc.model.art_meshes.iter().all(|a| a.texture == Some(t)));
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert!(back.model.same_data(&edited), "after create");
    let mut doc = back;
    let missed = auto_layout(&mut doc.model, t, &LayoutSettings::default(), true).unwrap();
    eprintln!("auto layout: {missed} images did not fit");
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert!(back.model.same_data(&edited), "after layout");
    let mut again = back;
    assert_eq!(again.save_bytes().unwrap(), bytes);
}

/// Project palette ▸ layer ▸ Create ArtMesh on stored PSDs: the layer tree
/// reloads; a mesh for a layer with a model image reuses it, and on a
/// registered-only PSD a model image (and group) is made. Both save and
/// reload. spec: 50-ui/palettes.md §6, psd-import.md §5
#[test]
fn create_art_mesh_from_stored_layers() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::psd::Psd;
    use keyform_formats::psd_import::{
        create_art_mesh_from_layer, new_model_from_psd, register_psd, ImportOptions,
    };
    use keyform_model::LayerEntry;
    fn first_layer(e: &[LayerEntry]) -> Option<String> {
        e.iter().find_map(|x| match x {
            LayerEntry::Layer(l) => Some(l.common.guid.clone()),
            LayerEntry::Group(g) => first_layer(&g.children),
        })
    }
    let Some(psd_path) = sample_files("psd")
        .into_iter()
        .min_by_key(|p| std::fs::metadata(p).map_or(0, |m| m.len()))
    else {
        return;
    };
    let data = std::fs::read(&psd_path).unwrap();
    let psd = Psd::parse(&data).unwrap();
    let o = ImportOptions::default();
    // (a) New model: every layer already has a model image.
    let (mut doc, _) = new_model_from_psd("t.psd", &psd, &o).unwrap();
    let bytes = doc.save_bytes().unwrap();
    let mut doc = Cmo3Document::read(&bytes).unwrap();
    let tree = doc.model.layered_images[0]
        .tree
        .clone()
        .expect("stored layer tree");
    let layer = first_layer(&tree).unwrap();
    let (ni, nm) = (doc.model.model_images.len(), doc.model.art_meshes.len());
    let a = create_art_mesh_from_layer(&mut doc.model, 0, &layer, None, &o).unwrap();
    assert_eq!(doc.model.model_images.len(), ni, "model image reused");
    assert_eq!(a, nm);
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert!(back.model.same_data(&edited));
    // (b) A registered PSD in an existing project: a new model image and
    // group.
    let Some(cmo3) = sample_files("cmo3")
        .into_iter()
        .find(|p| p.to_string_lossy().contains("taka"))
    else {
        return;
    };
    let mut doc = Cmo3Document::read(&std::fs::read(&cmo3).unwrap()).unwrap();
    let li = register_psd(&mut doc.model, "reg.psd", &psd, &o).unwrap();
    let bytes = doc.save_bytes().unwrap();
    let mut doc = Cmo3Document::read(&bytes).unwrap();
    let tree = doc.model.layered_images[li].tree.clone().unwrap();
    let layer = first_layer(&tree).unwrap();
    let ni = doc.model.model_images.len();
    create_art_mesh_from_layer(&mut doc.model, li, &layer, None, &o).unwrap();
    assert_eq!(doc.model.model_images.len(), ni + 1);
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert!(back.model.same_data(&edited));
    // For a black-box check in the reference editor.
    if let Some(dir) = std::env::var_os("KEYFORM_BB_OUT") {
        std::fs::write(Path::new(&dir).join("taka_layer_mesh.cmo3"), &bytes).unwrap();
    }
}

/// Preview resolution prompt (psd-import.md §3.1): labels and the
/// recommended choice; a chosen ratio is saved with the new model.
#[test]
fn preview_resolution_choices() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::psd::Psd;
    use keyform_formats::psd_import::{new_model_from_psd, preview_choices, ImportOptions};
    let mut smallest = None;
    for path in sample_files("psd") {
        let data = std::fs::read(&path).unwrap();
        let psd = Psd::parse(&data).unwrap();
        let (choices, pre, ask) = preview_choices(&psd);
        eprintln!("{path:?}: preselect {pre}, ask {ask}");
        for c in &choices {
            eprintln!("  {}", c.label);
        }
        assert_eq!(choices.len(), 4);
        assert!(choices[pre].label.ends_with(" < Recommended"));
        assert_eq!(ask, pre != 0);
        if smallest
            .as_ref()
            .is_none_or(|(n, _): &(usize, _)| data.len() < *n)
        {
            smallest = Some((data.len(), data));
        }
    }
    let Some((_, data)) = smallest else { return };
    let psd = Psd::parse(&data).unwrap();
    let o = ImportOptions {
        preview_reduction: 2,
        ..ImportOptions::default()
    };
    let (mut doc, _) = new_model_from_psd("t.psd", &psd, &o).unwrap();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert_eq!(back.model.preview_reduction, 2);
}

/// File ▸ New ▸ Model: a blank model saves and reloads; its parts and
/// parameters match a blank model saved by the reference
/// (`test-data/blackbox/blank_ref.cmo3`, when present).
#[test]
fn blank_model_matches_reference() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::psd_import::new_blank_model;
    let mut doc = new_blank_model("Untitled Model", 1000, 2000).unwrap();
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert!(back.model.same_data(&edited));
    // Random pose (psd-import.md §4.2): one setting, every parameter
    // enabled, every group expanded.
    let summary = |doc: &Cmo3Document| -> (usize, Vec<String>, Vec<String>, Vec<usize>) {
        let xml = String::from_utf8(doc.xml.write()).unwrap();
        let settings = xml.matches("<CRandomPoseSetting>").count();
        let flags = |tag: &str| -> Vec<String> {
            xml.split(&format!("<{tag}>"))
                .skip(1)
                .filter_map(|t| t.split("</b>").next())
                .map(|t| t.rsplit('>').next().unwrap_or("").to_owned())
                .collect()
        };
        let keys = xml
            .match_indices("xs.n=\"parameters.keys\"")
            .map(|(k, _)| {
                let end = xml[k..].find("</array_list>").unwrap() + k;
                xml[k..end].matches("<CParameterId").count()
            })
            .collect();
        (
            settings,
            flags("CRandomPoseParamData"),
            flags("CRandomPoseGroupData"),
            keys,
        )
    };
    let (n, p, g, k) = summary(&back);
    assert_eq!(n, 1);
    assert_eq!(p.len(), 27);
    assert!(p.iter().all(|v| v == "true"), "{p:?}");
    assert_eq!(g, vec!["true".to_owned()]);
    assert_eq!(k, vec![27]);
    let reference =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/blackbox/blank_ref.cmo3");
    let Ok(data) = std::fs::read(&reference) else {
        return;
    };
    let r = Cmo3Document::read(&data).unwrap().model;
    let m = &back.model;
    let parts = |m: &keyform_model::Model| -> Vec<(String, String, i32, bool, f32)> {
        m.parts
            .iter()
            .map(|p| {
                (
                    p.info.id.clone(),
                    p.info.name.clone(),
                    p.default_order,
                    p.sketch,
                    p.grid.keyforms[0].draw_order,
                )
            })
            .collect()
    };
    assert_eq!(parts(m), parts(&r));
    let params = |m: &keyform_model::Model| -> Vec<(String, String, f32, f32, f32)> {
        m.parameters
            .iter()
            .map(|p| (p.id.clone(), p.name.clone(), p.min, p.max, p.default))
            .collect()
    };
    assert_eq!(params(m), params(&r));
    assert_eq!(
        (m.canvas.width, m.canvas.height),
        (r.canvas.width, r.canvas.height)
    );
    let rd = Cmo3Document::read(&data).unwrap();
    assert_eq!(summary(&back), summary(&rd));
}

/// Parts editing (palettes.md §1): Move Here for an art mesh and a part,
/// moving a part into its own child refused, Delete empty parts; saved
/// and reloaded.
#[test]
fn parts_editing_saves() {
    use keyform_edit::objects::{create_part, delete_empty_parts, move_to_part, PART_INTO_ITSELF};
    use keyform_edit::ObjectRef;
    use keyform_formats::cmo3::Cmo3Document;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &mut doc.model;
        if m.art_meshes.is_empty() {
            continue;
        }
        let a = create_part(m, "A", None);
        let b = create_part(m, "B", Some(a));
        assert_eq!(
            move_to_part(m, &[ObjectRef::Part(a)], b),
            Err(PART_INTO_ITSELF.into())
        );
        assert_eq!(move_to_part(m, &[ObjectRef::ArtMesh(0)], b), Ok(1));
        assert_eq!(m.art_meshes[0].info.parent_part, Some(b));
        assert_eq!(move_to_part(m, &[ObjectRef::ArtMesh(0)], b), Ok(0));
        // Drag & drop: reorder within a part, and before another part.
        use keyform_edit::objects::place_in_part;
        use keyform_model::PartChild;
        let c = create_part(m, "C", Some(a));
        assert_eq!(
            place_in_part(m, ObjectRef::Part(c), a, Some(ObjectRef::Part(b))),
            Ok(true)
        );
        assert_eq!(
            m.parts[a].children[..2],
            [PartChild::Part(c), PartChild::Part(b)]
        );
        if let Some(q) = m.root_part {
            let first = m.parts[q].children[0];
            let first = match first {
                PartChild::Part(i) => Some(ObjectRef::Part(i)),
                PartChild::ArtMesh(i) => Some(ObjectRef::ArtMesh(i)),
                _ => None,
            };
            assert_eq!(place_in_part(m, ObjectRef::Part(c), q, first), Ok(true));
            assert_eq!(m.parts[q].children[0], PartChild::Part(c));
            assert_eq!(m.parts[c].info.parent_part, Some(q));
        }
        assert!(
            place_in_part(m, ObjectRef::Part(a), c, None).is_ok(),
            "C is no longer below A"
        );
        let e = create_part(m, "Empty", None);
        create_part(m, "Empty child", Some(e));
        // The sample's own empty parts go too.
        assert!(delete_empty_parts(m) >= 2);
        assert!(!m.parts.iter().any(|p| p.info.name.starts_with("Empty")));
        assert!(m.parts.iter().any(|p| p.info.name == "B"));
        let _ = (a, b);
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        bb_out(&path, "bb_parts.cmo3", &bytes);
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{path:?}: {:?}", back.warnings);
        assert!(back.model.same_data(&edited), "{path:?}");
        let b = back.model.art_meshes[0].info.parent_part.unwrap();
        assert_eq!(back.model.parts[b].info.name, "B");
    }
}

/// Random-pose settings follow the parameter set (psd-import.md §4.2):
/// after adding and deleting parameters every setting has one entry per
/// parameter, kept entries keep their value, new ones are enabled.
#[test]
fn random_pose_follows_parameters() {
    use keyform_edit::objects::{add_parameter, remove_parameter};
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::Parameter;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let before = String::from_utf8(doc.xml.write()).unwrap();
        if !before.contains("<CRandomPoseSetting") {
            continue;
        }
        let m = &mut doc.model;
        let victim = m.parameters.len() - 1;
        assert!(remove_parameter(m, victim));
        let mut p = Parameter::new("ParamRandomPoseTest", 0.0, 1.0, 0.0);
        p.guid = keyform_model::new_guid();
        add_parameter(m, p, None);
        let n = doc.model.parameters.len();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let back = Cmo3Document::read(&bytes).unwrap();
        let xml = String::from_utf8(back.xml.write()).unwrap();
        for (k, _) in xml.match_indices("xs.n=\"parameters.keys\"") {
            let end = xml[k..].find("</array_list>").unwrap() + k;
            assert_eq!(xml[k..end].matches("<CParameterId").count(), n, "{path:?}");
        }
        for (k, _) in xml.match_indices("xs.n=\"parameters.values\"") {
            let end = xml[k..].find("</array_list>").unwrap() + k;
            assert_eq!(
                xml[k..end].matches("<CRandomPoseParamData>").count(),
                n,
                "{path:?}"
            );
        }
        eprintln!("{path:?}: random pose synced ({n} parameters)");
    }
}

/// Inspector edits (palettes.md §4, inspector-fields.md): keyform draw
/// order, transparency, colours, rotation angle, Bulk Set Draw Order,
/// blend modes, part offscreen / draw-order group, rotation handle and
/// circle; saved and reloaded on every sample.
#[test]
fn inspector_edits_save() {
    use keyform_edit::forms::{form_fields, set_draw_order_all, set_form_fields};
    use keyform_edit::ObjectRef;
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::{AlphaComposition, ColorComposition, DeformerKind};
    use std::sync::Arc;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &mut doc.model;
        if m.art_meshes.len() < 2 || m.parts.len() < 2 {
            continue;
        }
        let mut f = form_fields(m, ObjectRef::ArtMesh(0), 0).unwrap();
        f.draw_order = Some(321.4);
        f.opacity = 0.75;
        f.multiply = [0.5, 0.6, 0.7, 1.0];
        f.screen = [0.1, 0.0, 0.2, 1.0];
        assert!(set_form_fields(m, ObjectRef::ArtMesh(0), 0, &f).unwrap());
        assert_eq!(m.art_meshes[0].grid.keyforms[0].draw_order, 321.0);
        set_draw_order_all(m, ObjectRef::ArtMesh(1), 777.0);
        assert!(m.art_meshes[1]
            .grid
            .keyforms
            .iter()
            .all(|k| k.draw_order == 777.0));
        let p = (0..m.parts.len())
            .find(|&p| Some(p) != m.root_part)
            .unwrap();
        let mut pf = form_fields(m, ObjectRef::Part(p), 0).unwrap();
        pf.opacity = 0.5;
        set_form_fields(m, ObjectRef::Part(p), 0, &pf).unwrap();
        {
            let a = Arc::make_mut(&mut m.art_meshes[0]);
            a.color_composition = ColorComposition::MultiplyR2;
            a.alpha_composition = AlphaComposition::Atop;
            let x = Arc::make_mut(&mut m.parts[p]);
            x.offscreen = !x.offscreen;
            x.draw_order_group = !x.draw_order_group;
        }
        if let Some(d) = (0..m.deformers.len())
            .find(|&d| matches!(m.deformers[d].kind, DeformerKind::Rotation(_)))
        {
            let mut rf = form_fields(m, ObjectRef::Deformer(d), 0).unwrap();
            rf.angle = Some(12.5);
            set_form_fields(m, ObjectRef::Deformer(d), 0, &rf).unwrap();
            if let DeformerKind::Rotation(r) = &mut Arc::make_mut(&mut m.deformers[d]).kind {
                r.handle_length = 123.0;
                r.circle_radius = 45.0;
            }
        }
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        bb_out(&path, "bb_inspector.cmo3", &bytes);
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{path:?}: {:?}", back.warnings);
        assert!(back.model.same_data(&edited), "{path:?}");
    }
}

/// Form tools (form-tools.md §1–4, §5.2) on the samples: pasting an
/// object's own copy keeps its keyform, flipping twice and reversing keys
/// twice are identities, synthesized corners follow
/// K = F(c1,b2) + F(b1,c2) − F(b1,b2); the results save and reload.
#[test]
fn form_tools_on_samples() {
    use keyform_edit::form_tools::*;
    use keyform_edit::ObjectRef;
    use keyform_eval::Evaluator;
    use keyform_formats::cmo3::Cmo3Document;
    let close = |a: &[f32], b: &[f32]| {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 2e-2)
    };
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &mut doc.model;
        let values: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
        let frame = Evaluator::new(m).evaluate(&values);
        // Paste of the object's own copy: unchanged (within float noise).
        let mut tried = 0;
        for i in 0..m.art_meshes.len() {
            let o = ObjectRef::ArtMesh(i);
            let Ok(cell) = keyform_edit::current_cell(m, o, &values) else {
                continue;
            };
            let Some(c) = copy_form(m, &frame, &values, o, None) else {
                continue;
            };
            let before = m.art_meshes[i].grid.keyforms[cell].positions.clone();
            let mut t = m.clone();
            paste_form(&mut t, &values, &[c], &[o], 1.0, PasteMirror::default()).unwrap();
            let after = &t.art_meshes[i].grid.keyforms[cell].positions;
            assert!(close(&before, after), "{path:?} mesh {i}");
            tried += 1;
            if tried >= 5 {
                break;
            }
        }
        // Flip twice = identity.
        let objs: Vec<ObjectRef> = (0..m.art_meshes.len().min(10))
            .map(ObjectRef::ArtMesh)
            .collect();
        let s = FlipSettings {
            horizontal: true,
            all_keyforms: true,
            keep_culling: true,
            ..Default::default()
        };
        let orig = m.clone();
        flip(m, &values, &objs, &s);
        assert!(!m.same_data(&orig));
        flip(m, &values, &objs, &s);
        for (a, b) in m.art_meshes.iter().zip(&orig.art_meshes) {
            for (x, y) in a.grid.keyforms.iter().zip(&b.grid.keyforms) {
                assert!(close(&x.positions, &y.positions), "{path:?}");
            }
        }
        // Reverse keys twice = identity.
        if let Some(p) = m.parameters.iter().position(|p| p.id == "ParamAngleX") {
            let snap = m.clone();
            let n = batch_reflect(m, &[p]);
            assert!(n == 0 || !m.same_data(&snap));
            batch_reflect(m, &[p]);
            for (a, b) in m.art_meshes.iter().zip(&snap.art_meshes) {
                for (x, y) in a.grid.bindings.iter().zip(&b.grid.bindings) {
                    assert!(close(&x.keys, &y.keys), "{path:?}");
                }
                assert_eq!(a.grid.keyforms, b.grid.keyforms, "{path:?}");
            }
        }
        // Synthesize corners on Angle X / Angle Y.
        let px = m.parameters.iter().position(|p| p.id == "ParamAngleX");
        let py = m.parameters.iter().position(|p| p.id == "ParamAngleY");
        if let (Some(p1), Some(p2)) = (px, py) {
            let s = CornerSettings {
                p1,
                p2,
                centre: false,
                corners: vec![
                    Corner::TopLeft,
                    Corner::TopRight,
                    Corner::BottomLeft,
                    Corner::BottomRight,
                ],
            };
            let objects = all_objects(m);
            let before = m.clone();
            let n = synthesize_corners(m, &values, &objects, &s);
            eprintln!("{path:?}: corners synthesized on {n} objects");
            // Check one processed art mesh at the top-right corner.
            let ev_before = Evaluator::new(&before);
            let ev_after = Evaluator::new(m);
            for i in 0..m.art_meshes.len() {
                let b = &m.art_meshes[i].grid.bindings;
                let k = |p| b.iter().find(|x| x.parameter == p).map(|x| x.keys.clone());
                let (Some(k1), Some(k2)) = (k(p1), k(p2)) else {
                    continue;
                };
                if k1.len() < 3 || k2.len() < 3 {
                    continue;
                }
                let at = |a: f32, c: f32| {
                    let mut v = values.clone();
                    v[p1] = a;
                    v[p2] = c;
                    v
                };
                let (c1, c2) = (k1[k1.len() - 1], k2[k2.len() - 1]);
                let (b1, b2) = (values[p1], values[p2]);
                // Evaluate local forms through a mesh with no parent effect:
                // compare canvas results only when the mesh is unparented.
                if m.art_meshes[i].target.is_some() {
                    continue;
                }
                let f = |ev: &Evaluator, a: f32, c: f32| {
                    ev.evaluate(&at(a, c)).meshes[i].positions.clone()
                };
                let (fa, fb, fc) = (
                    f(&ev_before, c1, b2),
                    f(&ev_before, b1, c2),
                    f(&ev_before, b1, b2),
                );
                let want: Vec<f32> = fa
                    .iter()
                    .zip(&fb)
                    .zip(&fc)
                    .map(|((a, b), c)| a + b - c)
                    .collect();
                let got = f(&ev_after, c1, c2);
                assert!(close(&want, &got), "{path:?} mesh {i}");
                break;
            }
        }
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        bb_out(&path, "bb_forms.cmo3", &bytes);
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.model.same_data(&edited), "{path:?}");
    }
}

/// Motion Mirroring (form-tools.md §5.1) on the samples: an art mesh
/// keyed on Angle X at its max mirrors to the min key; finite positions,
/// saved and reloaded.
#[test]
fn motion_mirroring_on_samples() {
    use keyform_edit::form_tools::{motion_mirror, MirrorSettings};
    use keyform_edit::ObjectRef;
    use keyform_formats::cmo3::Cmo3Document;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &mut doc.model;
        let Some(p) = m.parameters.iter().position(|p| p.id == "ParamAngleX") else {
            continue;
        };
        let mut values: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
        values[p] = m.parameters[p].max;
        let Some(i) = (0..m.art_meshes.len()).find(|&i| {
            let o = ObjectRef::ArtMesh(i);
            m.art_meshes[i].grid.bindings.len() == 1
                && keyform_edit::keys_of(m, o, p).contains(&values[p])
        }) else {
            continue;
        };
        let s = MirrorSettings {
            horizontal: true,
            vertical: false,
            axis_x: m.canvas.width as f32 / 2.0,
            axis_y: 0.0,
        };
        let before = m.clone();
        let o = ObjectRef::ArtMesh(i);
        let v = motion_mirror(m, &values, &[o], p, &s).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        assert_eq!(v, 2.0 * m.parameters[p].default - values[p]);
        assert!(keyform_edit::keys_of(m, o, p).contains(&v));
        assert!(!m.same_data(&before));
        assert!(m.art_meshes[i]
            .grid
            .keyforms
            .iter()
            .all(|f| f.positions.iter().all(|x| x.is_finite())));
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        bb_out(&path, "bb_mirror.cmo3", &bytes);
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.model.same_data(&edited), "{path:?}");
        eprintln!(
            "{path:?}: mirrored {} to AngleX = {v}",
            back.model.art_meshes[i].info.id
        );
    }
}

/// Writes `bytes` as `<name>` into `$KEYFORM_BB_OUT` (for checks in the
/// reference editor), for hiyori only.
fn bb_out(path: &Path, name: &str, bytes: &[u8]) {
    if let Some(dir) = std::env::var_os("KEYFORM_BB_OUT") {
        if path.to_string_lossy().contains("hiyori_pro") {
            std::fs::write(Path::new(&dir).join(name), bytes).unwrap();
        }
    }
}

/// Extended interpolation (extended-interpolation.md §1–2): the generated
/// inserted keyforms equal the ones the reference stored in files made
/// with its Extended Interpolation dialog
/// (`test-data/blackbox/forms/ext_{ellipse,sns}.cmo3`, when present).
#[test]
fn extended_grid_matches_reference() {
    use keyform_eval::extended::extended_grid;
    use keyform_formats::cmo3::Cmo3Document;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/blackbox/forms");
    for name in ["ext_ellipse.cmo3", "ext_sns.cmo3"] {
        let Ok(data) = std::fs::read(dir.join(name)) else {
            continue;
        };
        let doc = Cmo3Document::read(&data).unwrap();
        let a = doc
            .model
            .art_meshes
            .iter()
            .find(|a| a.info.id == "ArtMesh")
            .unwrap();
        let g = extended_grid(&a.grid).expect("extended");
        assert_eq!(g.keyforms.len(), 13, "{name}");
        // The stored inserted forms, in key order.
        let xml = String::from_utf8(doc.xml.write()).unwrap();
        let i = xml.find("xs.n=\"normalExtendedKeyforms\"").unwrap();
        let j = i + xml[i..].find("</carray_list>").unwrap();
        let stored: Vec<Vec<f32>> = xml[i..j]
            .split("<float-array xs.n=\"positions\"")
            .skip(1)
            .map(|t| {
                let body = &t[t.find('>').unwrap() + 1..t.find("</float-array>").unwrap()];
                body.split_whitespace()
                    .map(|x| x.parse().unwrap())
                    .collect()
            })
            .collect();
        let inserted: Vec<&Vec<f32>> = g
            .keyforms
            .iter()
            .enumerate()
            .filter(|(k, _)| k % 6 != 0)
            .map(|(_, f)| &f.positions)
            .collect();
        assert_eq!(stored.len(), inserted.len(), "{name}");
        let mut worst = 0.0f32;
        for (s, o) in stored.iter().zip(&inserted) {
            for (x, y) in s.iter().zip(o.iter()) {
                worst = worst.max((x - y).abs());
            }
        }
        eprintln!("{name}: max difference {worst} px");
        assert!(worst < 0.01, "{name}: {worst}");
    }
}

/// Saving extended interpolation (extended-interpolation.md §1): setting
/// ELLIPSE (5 points) on the 3-key test model writes an extension whose
/// inserted forms equal the reference's saved file; switching a reference
/// file to SNS replaces its extension; both reload as edited.
#[test]
fn extended_interpolation_saves() {
    use keyform_edit::form_tools::set_extended;
    use keyform_edit::ObjectRef;
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::ExtendedInterpolationKind as K;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/blackbox/forms");
    let stored = |doc: &Cmo3Document| -> (usize, Vec<Vec<f32>>) {
        let xml = String::from_utf8(doc.xml.write()).unwrap();
        let n = xml.matches("<CExtendedInterpolationExtension").count();
        let Some(i) = xml.find("xs.n=\"normalExtendedKeyforms\"") else {
            return (n, Vec::new());
        };
        let j = i + xml[i..].find("</carray_list>").unwrap();
        let v = xml[i..j]
            .split("<float-array xs.n=\"positions\"")
            .skip(1)
            .map(|t| {
                let body = &t[t.find('>').unwrap() + 1..t.find("</float-array>").unwrap()];
                body.split_whitespace()
                    .map(|x| x.parse().unwrap())
                    .collect()
            })
            .collect();
        (n, v)
    };
    let close = |a: &[Vec<f32>], b: &[Vec<f32>]| {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(x, y)| x.iter().zip(y).all(|(p, q)| (p - q).abs() < 0.01))
    };
    let (Ok(keys), Ok(ell), Ok(sns)) = (
        std::fs::read(dir.join("ext_keys.cmo3")),
        std::fs::read(dir.join("ext_ellipse.cmo3")),
        std::fs::read(dir.join("ext_sns.cmo3")),
    ) else {
        return;
    };
    let reference_ell = stored(&Cmo3Document::read(&ell).unwrap()).1;
    let reference_sns = stored(&Cmo3Document::read(&sns).unwrap()).1;
    // Keyform sets ELLIPSE on the linear file.
    let mut doc = Cmo3Document::read(&keys).unwrap();
    let a = doc
        .model
        .art_meshes
        .iter()
        .position(|a| a.info.id == "ArtMesh")
        .unwrap();
    let p = doc
        .model
        .parameters
        .iter()
        .position(|x| x.id == "ParamAngleX")
        .unwrap();
    assert!(set_extended(&mut doc.model, ObjectRef::ArtMesh(a), p, K::Ellipse, 5, 1.0).unwrap());
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.model.same_data(&edited));
    let (n, forms) = stored(&back);
    assert_eq!(n, 1);
    assert!(close(&forms, &reference_ell), "ellipse forms differ");
    if let Some(d) = std::env::var_os("KEYFORM_BB_OUT") {
        std::fs::write(Path::new(&d).join("kf_ext_ellipse.cmo3"), &bytes).unwrap();
    }
    // The reference's ELLIPSE file switched to SNS by Keyform.
    let mut doc = Cmo3Document::read(&ell).unwrap();
    assert!(set_extended(
        &mut doc.model,
        ObjectRef::ArtMesh(a),
        p,
        K::SnsCurve,
        5,
        1.0
    )
    .unwrap());
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.model.same_data(&edited));
    let (n, forms) = stored(&back);
    assert_eq!(n, 1, "the old extension is replaced");
    assert!(close(&forms, &reference_sns), "sns forms differ");
    // Back to linear: the extension goes.
    let mut doc = back;
    set_extended(&mut doc.model, ObjectRef::ArtMesh(a), p, K::Linear, 5, 1.0).unwrap();
    let bytes = doc.save_bytes().unwrap();
    assert_eq!(stored(&Cmo3Document::read(&bytes).unwrap()).0, 0);
}

/// Replace PSD (psd-import.md §7) on a new model from the smaller sample
/// PSD: replaced by the same PSD, every layer pairs (score 1), every model
/// image is relinked (its layer map keeps the old PSD), the old PSD is
/// marked replaced and the group links both; placement and UVs follow the
/// new layers; the result saves and reloads.
#[test]
fn replace_psd_saves() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::psd::Psd;
    use keyform_formats::psd_import::{new_model_from_psd, ImportOptions};
    use keyform_formats::psd_replace::replace_psd;
    use std::sync::Arc;
    let Some(psd_path) = sample_files("psd")
        .into_iter()
        .min_by_key(|p| std::fs::metadata(p).map_or(0, |m| m.len()))
    else {
        return;
    };
    let data = std::fs::read(&psd_path).unwrap();
    let psd = Psd::parse(&data).unwrap();
    let o = ImportOptions::default();
    let (mut doc, _) = new_model_from_psd("blitty.psd", &psd, &o).unwrap();
    let bytes = doc.save_bytes().unwrap();
    let mut doc = Cmo3Document::read(&bytes).unwrap();
    // Displace every model image so the relink has to restore placement.
    let before = doc.model.clone();
    for mi in &mut doc.model.model_images {
        Arc::make_mut(mi).local_to_canvas[2] += 10.0;
    }
    let n_images = doc.model.model_images.len();
    let n_layers = psd.layers().len();
    let r = replace_psd(&mut doc.model, &[0], "blitty_v2.psd", &psd, &o).unwrap();
    assert_eq!(r.pairs.len(), n_layers);
    assert_eq!(r.relinked.len(), n_images);
    assert!(r.import.art_meshes.is_empty(), "nothing unpaired");
    let m = &doc.model;
    assert!(m.layered_images[0].replaced && !m.layered_images[1].replaced);
    assert_eq!(m.model_image_groups[0].linked.len(), 2);
    for (a, b) in m.model_images.iter().zip(&before.model_images) {
        assert_eq!(
            a.local_to_canvas, b.local_to_canvas,
            "placement from the new layer"
        );
        assert_eq!(a.other_sources.len(), 1);
        assert_eq!(a.source.as_ref().unwrap().0, m.layered_images[1].guid);
    }
    for (a, b) in m.art_meshes.iter().zip(&before.art_meshes) {
        for (x, y) in a.uvs.iter().zip(&b.uvs) {
            assert!((x - y).abs() < 1e-5);
        }
    }
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert!(back.model.same_data(&edited));
    if let Some(d) = std::env::var_os("KEYFORM_BB_OUT") {
        std::fs::write(Path::new(&d).join("kf_replaced.cmo3"), &bytes).unwrap();
    }
    // Re-save unchanged: byte-stable.
    let mut again = back;
    assert_eq!(again.save_bytes().unwrap(), bytes);
}

/// Replace PSD with an unrelated PSD (both samples): the unpaired layers
/// become new objects in "<name> (Corresponding layer not found)" at the
/// top of the root part; saves and reloads.
#[test]
fn replace_psd_unpaired_layers() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::psd::Psd;
    use keyform_formats::psd_import::{new_model_from_psd, ImportOptions};
    use keyform_formats::psd_replace::replace_psd;
    use keyform_model::PartChild;
    let mut files = sample_files("psd");
    if files.len() < 2 {
        return;
    }
    files.sort_by_key(|p| std::fs::metadata(p).map_or(0, |m| m.len()));
    let small = std::fs::read(&files[0]).unwrap();
    let large = std::fs::read(&files[1]).unwrap();
    let (a, b) = (Psd::parse(&small).unwrap(), Psd::parse(&large).unwrap());
    let o = ImportOptions::default();
    let (mut doc, _) = new_model_from_psd("a.psd", &a, &o).unwrap();
    let bytes = doc.save_bytes().unwrap();
    let mut doc = Cmo3Document::read(&bytes).unwrap();
    let r = replace_psd(&mut doc.model, &[0], "other.psd", &b, &o).unwrap();
    eprintln!(
        "paired {}, relinked {}, imported {} meshes (by id first: {})",
        r.pairs.len(),
        r.relinked.len(),
        r.import.art_meshes.len(),
        r.by_id
    );
    assert_eq!(
        r.pairs.len() + r.import.art_meshes.len(),
        b.layers().len() - unpaired_dupes(&r)
    );
    let m = &doc.model;
    let root = m.root_part.unwrap();
    let PartChild::Part(top) = m.parts[root].children[0] else {
        panic!("no part at the top")
    };
    assert_eq!(
        m.parts[top].info.name,
        "other.psd (Corresponding layer not found)"
    );
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert!(back.model.same_data(&edited));
}

/// New layers paired with more than one old layer count once.
fn unpaired_dupes(r: &keyform_formats::psd_replace::ReplaceReport) -> usize {
    let mut seen = std::collections::HashSet::new();
    r.pairs
        .iter()
        .filter(|(n, _)| !seen.insert(n.clone()))
        .count()
}

/// An alpha composition on an older document (CArtMeshSource < 5) upgrades
/// the type to version 5 with the field on every art mesh and its import
/// (xml-object-graph.md §5); the mesh then uses the "illegal" look on an
/// old target (blending.md §5).
#[test]
fn alpha_composition_upgrades_old_documents() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::AlphaComposition;
    use std::sync::Arc;
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let xml = String::from_utf8(doc.xml.write()).unwrap();
        if !xml.contains("version CArtMeshSource:4") {
            continue;
        }
        Arc::make_mut(&mut doc.model.art_meshes[0]).alpha_composition = AlphaComposition::Atop;
        let bytes = doc.save_bytes().unwrap();
        let back = Cmo3Document::read(&bytes).unwrap();
        assert_eq!(
            back.model.art_meshes[0].alpha_composition,
            AlphaComposition::Atop
        );
        let xml = String::from_utf8(back.xml.write()).unwrap();
        assert!(xml.contains("version CArtMeshSource:5"), "{path:?}");
        assert!(xml.contains("com.live2d.cubism.doc.model.drawable.AlphaComposition"));
        let meshes = xml.matches("<CArtMeshSource xs.id").count();
        assert_eq!(xml.matches("xs.n=\"alphaComposition\"").count(), meshes);
        if (1..5_030_000).contains(&back.model.target_version) {
            assert!(back.model.illegal_look(&back.model.art_meshes[0]));
        }
    }
}

/// Unused source images (psd-import.md §7.1): after Replace PSD the old
/// PSD is unused; removing it drops it from the layer maps and the group,
/// and the save removes it with its layers and images.
#[test]
fn remove_unused_source_images() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::psd::Psd;
    use keyform_formats::psd_import::{new_model_from_psd, ImportOptions};
    use keyform_formats::psd_replace::{remove_layered_images, replace_psd, unused_layered_images};
    let Some(psd_path) = sample_files("psd")
        .into_iter()
        .min_by_key(|p| std::fs::metadata(p).map_or(0, |m| m.len()))
    else {
        return;
    };
    let data = std::fs::read(&psd_path).unwrap();
    let psd = Psd::parse(&data).unwrap();
    let o = ImportOptions::default();
    let (mut doc, _) = new_model_from_psd("a.psd", &psd, &o).unwrap();
    let bytes = doc.save_bytes().unwrap();
    let mut doc = Cmo3Document::read(&bytes).unwrap();
    assert!(unused_layered_images(&doc.model).is_empty());
    replace_psd(&mut doc.model, &[0], "b.psd", &psd, &o).unwrap();
    let replaced = doc.save_bytes().unwrap();
    let mut doc = Cmo3Document::read(&replaced).unwrap();
    assert_eq!(unused_layered_images(&doc.model), vec![0]);
    remove_layered_images(&mut doc.model, &[0]);
    assert_eq!(doc.model.layered_images.len(), 1);
    assert!(doc
        .model
        .model_images
        .iter()
        .all(|m| m.other_sources.is_empty()));
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert!(back.model.same_data(&edited));
    let xml = String::from_utf8(back.xml.write()).unwrap();
    assert_eq!(xml.matches("<LayeredImageWrapper").count(), 1);
    assert_eq!(xml.matches("<CLayeredImage xs.id").count(), 1);
    eprintln!("{} -> {} bytes", replaced.len(), bytes.len());
    assert!(
        bytes.len() < replaced.len() * 3 / 4,
        "the old PSD's images went"
    );
}

/// Deform paths load from the samples (deform-paths.md §1): control points
/// with anchors, curves, target points with effects.
#[test]
fn deform_paths_load() {
    use keyform_formats::cmo3::Cmo3Document;
    for path in sample_files("cmo3") {
        let doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &doc.model;
        let paths: usize = m.art_meshes.iter().map(|a| a.paths.len()).sum();
        let points: usize = m
            .art_meshes
            .iter()
            .flat_map(|a| &a.paths)
            .map(|p| p.points.len())
            .sum();
        let targets: usize = m
            .art_meshes
            .iter()
            .flat_map(|a| &a.paths)
            .map(|p| p.targets.len())
            .sum();
        eprintln!("{path:?}: {paths} paths, {points} control points, {targets} target points");
        let names: Vec<String> = m
            .art_meshes
            .iter()
            .filter(|a| !a.paths.is_empty())
            .map(|a| {
                format!(
                    "{} (level {})",
                    a.info.display_name(),
                    a.paths[0].edit_level
                )
            })
            .collect();
        eprintln!("  on: {}", names.join(", "));
        for a in &m.art_meshes {
            for p in &a.paths {
                for c in &p.curves {
                    assert!(c.points.iter().all(|&k| k < p.points.len()));
                }
                for t in &p.targets {
                    assert!(t.effects.iter().all(|e| match &e.effector {
                        keyform_model::Effector::Point(k) => *k < p.points.len(),
                        keyform_model::Effector::Curve { curve, .. } => *curve < p.curves.len(),
                    }));
                }
            }
        }
    }
}

/// Curves rebuilt from the stored bind positions pass through the stored
/// curve effector positions (deform-paths.md §1, sns-spline.md §3), and
/// rebinding reproduces the stored effects.
#[test]
fn deform_paths_rebind() {
    use keyform_edit::deform_path::{bind_vertex, curves};
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::Effector;
    for path in sample_files("cmo3") {
        let doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let (mut n, mut bad, mut worst) = (0, 0, 0.0f32);
        let (mut nb, mut badb) = (0, 0);
        for a in &doc.model.art_meshes {
            for p in &a.paths {
                let pts: Vec<[f32; 2]> = p.points.iter().map(|c| c.pos_default).collect();
                let cs = curves(p, &pts);
                for t in &p.targets {
                    for e in &t.effects {
                        if let Effector::Curve {
                            curve,
                            t: ct,
                            distance,
                            pos,
                        } = &e.effector
                        {
                            let q = cs[*curve].point(*ct);
                            let d = (q[0] - pos[0]).hypot(q[1] - pos[1]);
                            n += 1;
                            // Files last saved by a 4.2 editor store 2-point
                            // segments as straight lines (Q-0026): a few px
                            // of tolerance there.
                            let cv = &p.curves[*curve];
                            let seg = ct.floor() as usize;
                            let corner = |k: usize| p.points[cv.points[k % cv.points.len()]].corner;
                            let two_point =
                                cv.points.len() == 2 || (corner(seg) && corner(seg + 1));
                            let tol = if two_point { 3.0 } else { 0.5 };
                            if !two_point {
                                worst = worst.max(d);
                            }
                            if d > tol {
                                bad += 1;
                                if bad <= 5 {
                                    let c = &p.curves[*curve];
                                    let corners: Vec<bool> =
                                        c.points.iter().map(|&k| p.points[k].corner).collect();
                                    eprintln!("  t={ct} pos={pos:?} got={q:?} open={} corners={corners:?}", c.open);
                                }
                            }
                            let _ = distance;
                        }
                    }
                    // Effects bound with a single Point effector at weight 1
                    // sit on that point: rebinding there must agree.
                    if let [e] = t.effects.as_slice() {
                        let Effector::Point(k) = e.effector else {
                            continue;
                        };
                        nb += 1;
                        let got = bind_vertex(p, &pts, &cs, pts[k]);
                        if !matches!(got.as_slice(), [g] if g.effector == Effector::Point(k)) {
                            badb += 1;
                        }
                    }
                }
            }
        }
        eprintln!(
            "{path:?}: curve effects {n}, off {bad}, worst {worst}; point checks {nb}, off {badb}"
        );
        // Rebind from the vertices' positions at the default parameters.
        let m = &doc.model;
        let values: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
        let frame = keyform_eval::Evaluator::new(m).evaluate(&values);
        let (mut tot, mut same, mut wdiff) = (0, 0, 0.0f32);
        for a in &m.art_meshes {
            for p in &a.paths {
                let pts: Vec<[f32; 2]> = p.points.iter().map(|c| c.pos_default).collect();
                let cs = curves(p, &pts);
                for t in &p.targets {
                    let Some(mi) = m.art_meshes.iter().position(|x| x.info.guid == t.mesh) else {
                        continue;
                    };
                    let Some(vi) = m.art_meshes[mi].vertex_of_uid(t.uid) else {
                        continue;
                    };
                    let pos = &frame.meshes[mi].positions;
                    let v = [pos[2 * vi], pos[2 * vi + 1]];
                    let got = bind_vertex(p, &pts, &cs, v);
                    tot += 1;
                    let key = |e: &keyform_model::Effect| match &e.effector {
                        Effector::Point(k) => (0, *k),
                        Effector::Curve { curve, .. } => (1, *curve),
                    };
                    let a: Vec<_> = t.effects.iter().map(key).collect();
                    let b: Vec<_> = got.iter().map(key).collect();
                    if a == b {
                        same += 1;
                        for (x, y) in t.effects.iter().zip(&got) {
                            wdiff = wdiff.max((x.weight - y.weight).abs());
                        }
                    } else if tot - same <= 3 {
                        eprintln!("  stored {:?}\n  got    {:?}", t.effects, got);
                    }
                }
            }
        }
        eprintln!("  rebind: {same}/{tot} same effectors, worst weight diff {wdiff}");
        assert_eq!(bad, 0, "{path:?}: curve points off");
        assert_eq!(badb, 0);
        assert_eq!(same, tot, "{path:?}: rebinding changed effectors");
        assert!(wdiff < 1e-4, "{path:?}: weights differ by {wdiff}");
    }
}

fn xml_debug(e: &keyform_formats::xml::Element) -> String {
    let doc = keyform_formats::xml::XmlDocument::new(e.clone());
    String::from_utf8_lossy(&doc.write())
        .chars()
        .take(1500)
        .collect()
}

/// Every target point's `positions` is its keyform's current position
/// array, and its keyform is one its mesh lists (deform-paths.md §1).
fn check_point_refs(xml: &keyform_formats::xml::XmlDocument) -> usize {
    use keyform_formats::xml::Element;
    let sh = xml
        .root
        .children
        .iter()
        .find(|c| c.name == "shared")
        .unwrap();
    let by_id = |id: &str| sh.children.iter().find(|c| c.attr("xs.id") == Some(id));
    let mut listed = std::collections::HashSet::new();
    let mut stack: Vec<&Element> = vec![&xml.root];
    while let Some(e) = stack.pop() {
        if e.field_name() == Some("keyforms") {
            listed.extend(e.children.iter().filter_map(|c| c.attr("xs.ref")));
        }
        stack.extend(e.children.iter());
    }
    let mut n = 0;
    for ext in sh
        .children
        .iter()
        .filter(|c| c.name == "CControllerExtension")
    {
        for t in &ext.child_field("targetPoints").unwrap().children {
            let mut pr = t.child_field("_point").unwrap();
            if let Some(r) = pr.attr("xs.ref") {
                pr = by_id(r).unwrap();
            }
            let Some(form) = pr.child_field("keyForm").and_then(|f| f.attr("xs.ref")) else {
                panic!("{:?}", xml_debug(pr));
            };
            assert!(listed.contains(form), "keyform {form} is not listed");
            let fe = by_id(form).unwrap();
            let fp = fe.child_field("positions").unwrap().attr("xs.ref");
            assert_eq!(pr.child_field("positions").unwrap().attr("xs.ref"), fp);
            n += 1;
        }
    }
    n
}

/// Deform paths save (deform-paths.md §1): edits, a keyform edit (the
/// target points follow the keyform's new array), deletion and a new path
/// reload equal.
#[test]
fn deform_paths_save() {
    use keyform_edit::deform_path::{anchor, anchored, bind};
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::{ControlCurve, ControlPoint, DeformPath};
    use std::sync::Arc;
    let roundtrip = |doc: &mut Cmo3Document| {
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap();
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        assert!(back.model.same_data(&edited), "{}", {
            let a: Vec<_> = edited
                .art_meshes
                .iter()
                .flat_map(|a| a.paths.clone())
                .collect();
            let b: Vec<_> = back
                .model
                .art_meshes
                .iter()
                .flat_map(|a| a.paths.clone())
                .collect();
            format!("paths equal: {}", a == b)
        });
        back
    };
    for path in sample_files("cmo3") {
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let Some(i) = doc
            .model
            .art_meshes
            .iter()
            .position(|a| !a.paths.is_empty())
        else {
            continue;
        };
        eprintln!("{path:?}: mesh {}", doc.model.art_meshes[i].info.id);
        check_point_refs(&doc.xml);
        // Edit fields.
        {
            let a = Arc::make_mut(&mut doc.model.art_meshes[i]);
            let p = &mut a.paths[0];
            if let Some(c) = p.curves.first_mut() {
                c.width = 123.0;
            }
            p.points[0].anchor.weight = [0.5, 0.25, 0.25];
            p.points[0].corner = !p.points[0].corner;
            p.max_bind = 2;
        }
        let mut doc = roundtrip(&mut doc);
        let n0 = check_point_refs(&doc.xml);
        // A keyform edit gives the keyform a new array.
        {
            let a = Arc::make_mut(&mut doc.model.art_meshes[i]);
            for f in &mut a.grid.keyforms {
                f.positions[0] += 1.0;
            }
        }
        let mut doc = roundtrip(&mut doc);
        assert_eq!(check_point_refs(&doc.xml), n0);
        // Delete the mesh's paths.
        let had = doc.xml.write().len();
        let removed: usize = doc.model.art_meshes[i]
            .paths
            .iter()
            .map(|p| p.targets.len())
            .sum();
        Arc::make_mut(&mut doc.model.art_meshes[i]).paths.clear();
        let mut doc = roundtrip(&mut doc);
        assert_eq!(check_point_refs(&doc.xml), n0 - removed);
        assert!(doc.xml.write().len() < had);
        // A new path on a mesh without one: two points on a curve.
        let values: Vec<f32> = doc.model.parameters.iter().map(|p| p.default).collect();
        let frame = keyform_eval::Evaluator::new(&doc.model).evaluate(&values);
        let j = doc
            .model
            .art_meshes
            .iter()
            .position(|a| a.paths.is_empty() && a.vertex_count() >= 3)
            .unwrap();
        let a = &doc.model.art_meshes[j];
        let pos = frame.meshes[j].positions.clone();
        let form = a.grid.guids.first().cloned().unwrap_or_default();
        let mut p = DeformPath::new(keyform_model::new_guid(), 2);
        let mut pts = Vec::new();
        for v in [0usize, 1] {
            let q = [pos[2 * v] + 1.0, pos[2 * v + 1] + 1.0];
            let an = anchor(&pos, &a.indices, q);
            let at = anchored(&an, &pos).unwrap();
            pts.push(at);
            p.points.push(ControlPoint {
                guid: keyform_model::new_guid(),
                curve: Some(0),
                corner: false,
                anchor: an,
                pos_default: at,
                recovery: at,
                total_effect: 0.0,
            });
        }
        p.curves
            .push(ControlCurve::new(keyform_model::new_guid(), vec![0, 1]));
        let verts: Vec<_> = (0..a.vertex_count())
            .map(|v| {
                let uid = a.point_uids.get(v).copied().unwrap_or(v as i64);
                (
                    a.info.guid.clone(),
                    uid,
                    v,
                    form.clone(),
                    [pos[2 * v], pos[2 * v + 1]],
                )
            })
            .collect();
        bind(&mut p, &pts, &verts);
        Arc::make_mut(&mut doc.model.art_meshes[j]).paths.push(p);
        let doc = roundtrip(&mut doc);
        assert_eq!(
            check_point_refs(&doc.xml),
            n0 - removed + doc.model.art_meshes[j].vertex_count()
        );
    }
}

/// Dragging every control point of a sample path by δ moves each vertex
/// bound with full weight by δ on the canvas (deform-paths.md §4), and a
/// mesh edit keeps the paths (§1 "Topology observers").
#[test]
fn deform_path_drag_and_mesh_edit() {
    use keyform_edit::deform_path::{point_positions, set_mesh_keep_paths, PathDrag};
    use keyform_edit::mesh::MeshGraph;
    use keyform_eval::Evaluator;
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::Effector;
    for path in sample_files("cmo3") {
        let doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m0 = doc.model;
        let values: Vec<f32> = m0.parameters.iter().map(|p| p.default).collect();
        let f0 = Evaluator::new(&m0).evaluate(&values);
        let mut tried = 0;
        for mi in 0..m0.art_meshes.len() {
            if m0.art_meshes[mi].paths.is_empty() {
                continue;
            }
            let Ok(drag) = PathDrag::begin(
                &m0,
                &f0,
                &values,
                mi,
                0,
                (0..m0.art_meshes[mi].paths[0].points.len()).collect(),
            ) else {
                continue;
            };
            let delta = [6.0, -4.0];
            let mut m = m0.clone();
            drag.apply(&mut m, delta).unwrap();
            let f1 = Evaluator::new(&m).evaluate(&values);
            let p = &m0.art_meshes[mi].paths[0];
            let mut checked = 0;
            for t in &p.targets {
                if !(t.effects.len() == 1 && (t.effects[0].weight - 1.0).abs() < 1e-3) {
                    continue;
                }
                let _ = matches!(t.effects[0].effector, Effector::Curve { .. });
                let k = m0
                    .art_meshes
                    .iter()
                    .position(|a| a.info.guid == t.mesh)
                    .unwrap();
                let v = m0.art_meshes[k].vertex_of_uid(t.uid).unwrap();
                let (a, b) = (&f0.meshes[k].positions, &f1.meshes[k].positions);
                let d = [b[2 * v] - a[2 * v], b[2 * v + 1] - a[2 * v + 1]];
                assert!(
                    (d[0] - delta[0]).abs() < 0.05 && (d[1] - delta[1]).abs() < 0.05,
                    "{path:?} mesh {mi} vertex {v}: moved {d:?}"
                );
                checked += 1;
            }
            assert!(checked > 0);
            // Release: the stored totals are recomputed.
            drag.finish(&mut m, &f1, delta);
            // Mesh edit with the same graph keeps the points where they
            // were and rebinds every vertex.
            let before = point_positions(&m0, &f0, mi, 0);
            let mut m = m0.clone();
            let g = MeshGraph::of(&m.art_meshes[mi]);
            set_mesh_keep_paths(&mut m, &values, mi, &g).unwrap();
            let f2 = Evaluator::new(&m).evaluate(&values);
            let after = point_positions(&m, &f2, mi, 0);
            for (x, y) in before.iter().zip(&after) {
                assert!(
                    (x[0] - y[0]).abs() < 0.01 && (x[1] - y[1]).abs() < 0.01,
                    "{x:?} {y:?}"
                );
            }
            let p = &m.art_meshes[mi].paths[0];
            let subs: usize = p
                .sub_meshes
                .iter()
                .filter_map(|g| m.art_meshes.iter().find(|a| &a.info.guid == g))
                .map(|a| a.vertex_count())
                .sum();
            assert_eq!(p.targets.len(), m.art_meshes[mi].vertex_count() + subs);
            tried += 1;
            if tried >= 3 {
                break;
            }
        }
        eprintln!("{path:?}: {tried} paths dragged");
    }
}

/// Multi-target paste pairing (form-tools.md §2): own copies pair with
/// themselves; a target several copies fit is ambiguous; the default
/// pairing hands the shared candidates out in order; an explicit pairing
/// pastes the chosen copy.
#[test]
fn paste_pairing() {
    use keyform_edit::form_tools::*;
    use keyform_edit::ObjectRef;
    use keyform_eval::Evaluator;
    use keyform_formats::cmo3::Cmo3Document;
    for path in sample_files("cmo3") {
        let doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = doc.model;
        let values: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
        let frame = Evaluator::new(&m).evaluate(&values);
        // Three meshes on a key with the same vertex count.
        let on_key =
            |i: usize| keyform_edit::current_cell(&m, ObjectRef::ArtMesh(i), &values).is_ok();
        let mut by_count: std::collections::HashMap<usize, Vec<usize>> = Default::default();
        for i in 0..m.art_meshes.len() {
            if on_key(i) {
                by_count
                    .entry(m.art_meshes[i].vertex_count())
                    .or_default()
                    .push(i);
            }
        }
        let Some(group) = by_count.values().find(|g| g.len() >= 4) else {
            continue;
        };
        let (a, b, c, d) = (group[0], group[1], group[2], group[3]);
        let copies: Vec<CopiedForm> = [a, b]
            .iter()
            .map(|&i| copy_form(&m, &frame, &values, ObjectRef::ArtMesh(i), None).unwrap())
            .collect();
        let own = [ObjectRef::ArtMesh(a), ObjectRef::ArtMesh(b)];
        assert!(!pairing_ambiguous(&m, &copies, &own));
        assert_eq!(
            default_pairing(&m, &copies, &own),
            vec![(own[0], Some(0)), (own[1], Some(1))]
        );
        let others = [ObjectRef::ArtMesh(c), ObjectRef::ArtMesh(d)];
        assert!(pairing_ambiguous(&m, &copies, &others));
        assert_eq!(
            default_pairing(&m, &copies, &others),
            vec![(others[0], Some(0)), (others[1], Some(1))]
        );
        // Paste b's form into c: c's vertices land on b's on the canvas.
        let mut t = m.clone();
        let n = paste_form_paired(
            &mut t,
            &values,
            &copies,
            &[(others[0], Some(1)), (others[1], None)],
            1.0,
            PasteMirror::default(),
        )
        .unwrap();
        assert_eq!(n, 1);
        let f = Evaluator::new(&t).evaluate(&values);
        let (pc, pb) = (
            f.positions_before_glue(c).unwrap(),
            frame.positions_before_glue(b).unwrap(),
        );
        let worst = pc
            .iter()
            .zip(pb)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 0.05, "{path:?}: {worst}");
        assert!(f.meshes[d].positions == frame.meshes[d].positions);
        eprintln!(
            "{path:?}: pairing ok (vertex count {})",
            m.art_meshes[a].vertex_count()
        );
    }
}

/// `.can3` documents load: scenes, tracks, model-track attributes and
/// curves (animation-model.md §1–3).
#[test]
fn can3_load() {
    use keyform_formats::can3::Can3Document;
    use keyform_model::animation::TrackKind;
    for path in sample_files("can3") {
        let doc = Can3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let a = &doc.animation;
        assert!(doc.warnings.is_empty(), "{path:?}: {:?}", doc.warnings);
        assert!(!a.scenes.is_empty());
        let (mut models, mut attrs, mut keys) = (0, 0, 0);
        for s in &a.scenes {
            assert!(s.fps > 0.0);
            for t in &s.tracks {
                if let TrackKind::Model(m) = &t.kind {
                    models += 1;
                    assert!(m.resource.is_some());
                    for at in m.params.iter().chain(&m.parts) {
                        attrs += 1;
                        keys += at.curve.keys.len();
                        assert!(
                            at.curve.keys.windows(2).all(|w| w[0].frame < w[1].frame),
                            "{}",
                            at.id
                        );
                        assert!(at.curve.fixed || at.curve.types.len() == at.curve.keys.len());
                    }
                }
            }
        }
        eprintln!(
            "{path:?}: {} scenes, {models} model tracks, {attrs} attributes, {keys} keys, resources {:?}",
            a.scenes.len(),
            a.resources.iter().map(|r| &r.name).collect::<Vec<_>>()
        );
        assert!(models > 0);
    }
}

/// Recomputing SMOOTH handles (animation-model.md §3.2) reproduces the
/// handles the reference stored in the sample `.can3` files.
#[test]
fn can3_smooth_handles() {
    use keyform_anim::scene::smooth_handles;
    use keyform_formats::can3::Can3Document;
    use keyform_model::animation::{CurveType, TrackKind};
    for path in sample_files("can3") {
        let doc = Can3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let (mut n, mut bad, mut worst) = (0, 0, 0.0f64);
        for s in &doc.animation.scenes {
            for t in &s.tracks {
                let TrackKind::Model(m) = &t.kind else {
                    continue;
                };
                for a in m.params.iter().chain(&m.parts) {
                    let c = &a.curve;
                    if c.keys.len() < 2 || c.types.iter().any(|t| *t != CurveType::Smooth) {
                        continue;
                    }
                    let mut r = c.clone();
                    smooth_handles(&mut r, doc.animation.smooth_legacy);
                    // The validity clamp (§3.3) runs after edits, so a
                    // stored handle is either one.
                    let mut rc = r.clone();
                    keyform_anim::scene::clamp_handles(&mut rc, a.range.0, a.range.1);
                    for (i, ((k, q), qc)) in c.keys.iter().zip(&r.keys).zip(&rc.keys).enumerate() {
                        let pairs = [
                            (i > 0).then_some((k.prev, q.prev, qc.prev)),
                            (i + 1 < c.keys.len()).then_some((k.next, q.next, qc.next)),
                        ];
                        for (x, y, yc) in pairs.into_iter().flatten() {
                            let (Some(x), Some(y), Some(yc)) = (x, y, yc) else {
                                continue;
                            };
                            n += 1;
                            let dist = |y: keyform_model::animation::Handle| {
                                (x.value - y.value)
                                    .abs()
                                    .max(f64::from((x.frame - y.frame).abs()))
                            };
                            let d = dist(y).min(dist(yc));
                            worst = worst.max(d);
                            if d > 1e-3 {
                                bad += 1;
                                if bad <= 8 {
                                    eprintln!(
                                        "  {} {:?} key {i} @{} v {}: stored {x:?} got {y:?}",
                                        a.id, a.range, k.frame, k.value
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        eprintln!("{path:?}: {n} handles, {bad} off, worst {worst}");
        assert_eq!(bad, 0, "{path:?}");
    }
}

#[test]
#[ignore]
fn can3_dump_scene() {
    use keyform_formats::can3::Can3Document;
    use keyform_model::animation::TrackKind;
    let path = std::env::var("CAN3").unwrap();
    let which: usize = std::env::var("SCENE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let doc = Can3Document::read(&std::fs::read(&path).unwrap()).unwrap();
    let s = &doc.animation.scenes[which];
    eprintln!(
        "scene {} duration {} fps {} work {}..{} start {} fades {} {} loop {} restricted {}",
        s.name,
        s.duration,
        s.fps,
        s.work_start,
        s.work_end,
        s.start_frame,
        s.fade_in_ms,
        s.fade_out_ms,
        s.loop_motion,
        s.bezier_restricted
    );
    for t in &s.tracks {
        eprintln!(
            "track {} start {} dur {} opacity keys {:?}",
            t.name,
            t.start,
            t.duration,
            t.opacity.as_ref().map(|a| a.curve.keys.len())
        );
        if let TrackKind::Model(m) = &t.kind {
            for a in m.params.iter().chain(&m.parts) {
                let k = &a.curve.keys;
                eprintln!(
                    "  {} keys {} first {:?} last {:?} base {} shy {} freeze {} fades {} {}",
                    a.id,
                    k.len(),
                    k.first().map(|k| (k.frame, k.value)),
                    k.last().map(|k| (k.frame, k.value)),
                    a.curve.base,
                    a.shy,
                    a.freeze,
                    a.fade_in_ms,
                    a.fade_out_ms
                );
            }
        }
    }
}

/// motion3.json export (motion3-json.md §2) of every sample scene equals
/// the motion the reference exported from it (runtime/motion/<scene>).
#[test]
fn can3_motion_export() {
    use keyform_anim::motion::Motion;
    use keyform_anim::motion_export::{export_motion, ExportOptions};
    use keyform_formats::can3::Can3Document;
    use keyform_model::animation::TrackKind;
    for path in sample_files("can3") {
        let dir = path.parent().unwrap().join("runtime/motion");
        let doc = Can3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let (mut compared, mut differ) = (0, 0);
        for s in &doc.animation.scenes {
            let ref_path = dir.join(format!("{}.motion3.json", s.name));
            let Ok(text) = std::fs::read_to_string(&ref_path) else {
                continue;
            };
            let reference = Motion::parse(&text).unwrap();
            let ti = s
                .tracks
                .iter()
                .position(|t| matches!(t.kind, TrackKind::Model(_)))
                .unwrap();
            let o = ExportOptions {
                legacy_smooth: doc.animation.smooth_legacy,
                ..Default::default()
            };
            let mine = export_motion(s, ti, &o, &|_| None, &|_| None).unwrap();
            // Through JSON, as written (rounding).
            let mine = Motion::parse(&mine.to_json().to_string()).unwrap();
            compared += 1;
            let mut notes = Vec::new();
            if (mine.duration - reference.duration).abs() > 1e-3 {
                notes.push(format!(
                    "duration {} vs {}",
                    mine.duration, reference.duration
                ));
            }
            let ids = |m: &Motion| m.curves.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
            if ids(&mine) != ids(&reference) {
                notes.push(format!(
                    "curves {:?}\n   vs {:?}",
                    ids(&mine),
                    ids(&reference)
                ));
            } else {
                for (a, b) in mine.curves.iter().zip(&reference.curves) {
                    if a != b {
                        let k = a
                            .segments
                            .iter()
                            .zip(&b.segments)
                            .position(|(x, y)| x != y)
                            .unwrap_or(0);
                        notes.push(format!(
                            "{}: start {:?} vs {:?}; segment {k}: {:?}\n   vs {:?}",
                            a.id,
                            a.start,
                            b.start,
                            a.segments.get(k),
                            b.segments.get(k)
                        ));
                        if a.segments.len() != b.segments.len() {
                            notes.push(format!(
                                "   segments {} vs {}",
                                a.segments.len(),
                                b.segments.len()
                            ));
                        }
                        break;
                    }
                }
            }
            if mine.user_data != reference.user_data {
                notes.push("user data".into());
            }
            // hiyori_free's runtime motions come from an older editor
            // (Q-0029): m07 from another revision of the scene, m03/m05 with
            // unclamped handles / unrounded anchors, m02/m04 with ParamCheek
            // not clamped to its −1…0 range. Fresh 5.3.04 exports of all
            // eight scenes match exactly (KEYFORM_MOTION_STRICT=1 with a
            // runtime/motion folder holding them).
            let known = path.ends_with("hiyori_free_t03.can3")
                && matches!(
                    s.name.as_str(),
                    "hiyori_m02" | "hiyori_m03" | "hiyori_m04" | "hiyori_m05" | "hiyori_m07"
                )
                && std::env::var_os("KEYFORM_MOTION_STRICT").is_none();
            if !notes.is_empty() && !known {
                differ += 1;
                eprintln!("{}: {}", s.name, notes.join("; "));
            }
        }
        eprintln!("{path:?}: {compared} scenes compared, {differ} differ");
        assert_eq!(differ, 0, "{path:?}");
    }
}

#[test]
#[ignore]
fn can3_debug_attr() {
    use keyform_anim::motion_export::{export_motion, ExportOptions};
    use keyform_formats::can3::Can3Document;
    use keyform_model::animation::TrackKind;
    let path = std::env::var("CAN3").unwrap();
    let which: usize = std::env::var("SCENE").unwrap().parse().unwrap();
    let id = std::env::var("ATTR").unwrap();
    let doc = Can3Document::read(&std::fs::read(&path).unwrap()).unwrap();
    let s = &doc.animation.scenes[which];
    for (ti, t) in s.tracks.iter().enumerate() {
        if let TrackKind::Model(m) = &t.kind {
            for a in m
                .params
                .iter()
                .chain(&m.parts)
                .filter(|a| a.id.ends_with(&id))
            {
                eprintln!(
                    "track {ti} {} range {:?} keys {:?} types {:?}",
                    a.id, a.range, a.curve.keys, a.curve.types
                );
            }
            let mo = export_motion(s, ti, &ExportOptions::default(), &|_| None, &|_| None).unwrap();
            for c in mo.curves.iter().filter(|c| c.id.ends_with(&id)) {
                eprintln!("export {:?} {:?}", c.start, c.segments);
            }
        }
    }
}

/// Edited `.can3` curves save in place and reload equal; an unchanged
/// save reloads equal too.
#[test]
fn can3_save_edits() {
    use keyform_anim::scene::{delete_key, move_key, refresh, set_key};
    use keyform_formats::can3::Can3Document;
    use keyform_model::animation::{CurveType, TrackKind};
    for path in sample_files("can3") {
        let data = std::fs::read(&path).unwrap();
        let mut doc = Can3Document::read(&data).unwrap();
        let same = doc.save_bytes().unwrap();
        assert_eq!(Can3Document::read(&same).unwrap().animation, doc.animation);
        eprintln!(
            "{path:?}: unchanged save {} bytes (was {}), identical: {}",
            same.len(),
            data.len(),
            same == data
        );
        // Edit the first two keyed parameter curves of every scene.
        let legacy = doc.animation.smooth_legacy;
        let mut edited = 0;
        for s in &mut doc.animation.scenes {
            for t in &mut s.tracks {
                let TrackKind::Model(m) = &mut t.kind else {
                    continue;
                };
                for a in m
                    .params
                    .iter_mut()
                    .filter(|a| a.curve.keys.len() >= 3)
                    .take(2)
                {
                    let c = &mut a.curve;
                    let last = c.keys.last().unwrap().frame;
                    set_key(c, last + 3, 0.5, CurveType::Linear);
                    let f1 = c.keys[1].frame;
                    if c.keys.iter().all(|k| k.frame != f1 + 1) && f1 + 1 < c.keys[2].frame {
                        assert!(move_key(c, 1, f1 + 1, c.keys[1].value + 0.25));
                    }
                    c.types[0] = CurveType::Step;
                    delete_key(c, 2);
                    let (lo, hi) = keyform_anim::scene::clamp_range(a);
                    refresh(&mut a.curve, lo, hi, legacy);
                    edited += 1;
                }
            }
        }
        let edited_anim = doc.animation.clone();
        let bytes = doc.save_bytes().unwrap();
        let back = Can3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        assert_eq!(back.animation, edited_anim);
        // Saving again changes nothing.
        let mut back = back;
        assert_eq!(
            Can3Document::read(&back.save_bytes().unwrap())
                .unwrap()
                .animation,
            edited_anim
        );
        eprintln!("  {edited} curves edited, saved and reloaded");
        if let Some(out) = std::env::var_os("KEYFORM_CAN3_OUT") {
            let name = path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .replace(".can3", "_kf.can3");
            std::fs::write(std::path::Path::new(&out).join(name), &bytes).unwrap();
        }
    }
}

/// Extended blend-shape sets save (extended-interpolation.md §4): two
/// blend-shape entries injected into a sample mesh, an SNS setting with 2
/// points saves only the inserted points (2 gaps incl. the base × 2) and
/// the settings map, and the document reloads and evaluates equal. A
/// LINEAR setting is stored too.
#[test]
fn extended_blend_shapes_save() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::xml::Element;
    use keyform_model::{BlendExtended, ExtendedInterpolationKind};
    use std::sync::Arc;
    let Some(path) = sample_files("cmo3")
        .into_iter()
        .find(|p| p.ends_with("hiyori_pro_t11.cmo3"))
    else {
        return;
    };
    let doc0 = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
    let m = doc0.model.clone();
    // A mesh with ≥ 3 distinct stored keyforms and a parameter it isn't bound to.
    let Some(i) = m.art_meshes.iter().position(|a| {
        let mut g = a.grid.guids.clone();
        g.dedup();
        g.len() >= 3 && a.blend_shapes.shapes.is_empty()
    }) else {
        return;
    };
    let a = &m.art_meshes[i];
    let bound: Vec<usize> = a.grid.bindings.iter().map(|b| b.parameter).collect();
    let p = (0..m.parameters.len())
        .find(|p| !bound.contains(p))
        .unwrap();
    let (pguid, f1, f2) = (
        m.parameters[p].guid.clone(),
        a.grid.guids[1].clone(),
        a.grid.guids[2].clone(),
    );
    // Inject <KeyFormMorphTarget> entries (key 0.5 → keyform 1, 1.0 → 2).
    let mut doc = doc0;
    let mut xml = doc.xml.clone();
    let sh_idx = xml
        .root
        .children
        .iter()
        .position(|c| c.name == "shared")
        .unwrap();
    let find_uuid = |root: &Element, tag: &str, uuid: &str| -> Option<String> {
        root.children[sh_idx]
            .children
            .iter()
            .find(|c| c.name == tag && c.attr("uuid") == Some(uuid))
            .and_then(|c| c.attr("xs.id"))
            .map(str::to_owned)
    };
    let pid = find_uuid(&xml.root, "CParameterGuid", &pguid).expect("parameter guid shared");
    let g1 = find_uuid(&xml.root, "CFormGuid", &f1).expect("form guid shared");
    let g2 = find_uuid(&xml.root, "CFormGuid", &f2).expect("form guid shared");
    let mesh_guid = a.info.guid.clone();
    let dg = find_uuid(&xml.root, "CDrawableGuid", &mesh_guid).unwrap();
    // The mesh element: the shared CArtMeshSource whose guid refs `dg`.
    let mesh = xml.root.children[sh_idx]
        .children
        .iter_mut()
        .find(|c| {
            c.name == "CArtMeshSource" && {
                let mut st = vec![&**c];
                let mut hit = false;
                while let Some(e) = st.pop() {
                    if e.field_name() == Some("guid")
                        && e.attr("xs.ref") == Some(dg.as_str())
                        && e.name == "CDrawableGuid"
                    {
                        hit = true;
                        break;
                    }
                    st.extend(e.children.iter());
                }
                hit
            }
        })
        .unwrap();
    let mut stack: Vec<&mut Element> = vec![mesh];
    let mut set = None;
    while let Some(e) = stack.pop() {
        if e.field_name() == Some("keyformMorphTargetSet") {
            set = Some(e);
            break;
        }
        stack.extend(e.children.iter_mut());
    }
    let set = set.unwrap();
    let list = set
        .children
        .iter_mut()
        .find(|c| c.field_name() == Some("_morphTargets"))
        .unwrap();
    for (k, g) in [("0.5", &g1), ("1.0", &g2)] {
        let mut t = Element::new("KeyFormMorphTarget");
        let mut kv = Element::new("f").with_attr("xs.n", "keyValue");
        kv.text = k.into();
        t.children = vec![
            Element::new("CParameterGuid")
                .with_attr("xs.n", "parameterGuid")
                .with_attr("xs.ref", &pid),
            kv,
            Element::new("CFormGuid")
                .with_attr("xs.n", "keyformGuid")
                .with_attr("xs.ref", g),
        ];
        list.children.push(t);
    }
    list.set_attr("count", list.children.len().to_string());
    doc.xml = xml;
    let bytes = doc.write().unwrap();
    let mut doc = Cmo3Document::read(&bytes).unwrap();
    assert_eq!(
        doc.model.art_meshes[i].blend_shapes.shapes.len(),
        2,
        "{:?}",
        doc.warnings
    );
    // An SNS setting with 2 points, and a LINEAR one on another parameter
    // with no shapes (dropped on rebuild, so not added here).
    // Through the Extended Interpolation panel's operation.
    use keyform_edit::form_tools::{extended_settings, set_extended};
    use keyform_edit::ObjectRef;
    assert_eq!(
        extended_settings(&doc.model, ObjectRef::ArtMesh(i), p),
        Some((ExtendedInterpolationKind::Linear, 1, 1.0, 3))
    );
    assert!(set_extended(
        &mut doc.model,
        ObjectRef::ArtMesh(i),
        p,
        ExtendedInterpolationKind::SnsCurve,
        2,
        1.0
    )
    .unwrap());
    assert_eq!(
        doc.model.art_meshes[i].blend_shapes.extended,
        vec![BlendExtended {
            parameter: p,
            kind: ExtendedInterpolationKind::SnsCurve,
            points: 2,
            scale: 1.0
        }]
    );
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.model.same_data(&edited));
    let text = String::from_utf8(back.xml.write()).unwrap();
    assert_eq!(text.matches("<ExtendedKeyFormMorphTarget>").count(), 4);
    assert_eq!(text.matches("<ExtendedInterpolationSetting").count(), 1);
    // Evaluation with the parameter at 0.25 (an inserted key) agrees.
    let mut values: Vec<f32> = edited.parameters.iter().map(|q| q.default).collect();
    values[p] = 0.25;
    let fa = keyform_eval::Evaluator::new(&edited).evaluate(&values);
    let fb = keyform_eval::Evaluator::new(&back.model).evaluate(&values);
    assert_eq!(fa.meshes[i].positions, fb.meshes[i].positions);
    // Switching the setting to LINEAR stores a LINEAR entry, no points.
    let mut doc = back;
    Arc::make_mut(&mut doc.model.art_meshes[i])
        .blend_shapes
        .extended[0]
        .kind = ExtendedInterpolationKind::Linear;
    let edited = doc.model.clone();
    let back = Cmo3Document::read(&doc.save_bytes().unwrap()).unwrap();
    assert!(back.model.same_data(&edited));
    let text = String::from_utf8(back.xml.write()).unwrap();
    assert_eq!(text.matches("<ExtendedKeyFormMorphTarget>").count(), 0);
    assert!(text.contains(r#"<ExtendedInterpolationType xs.n="type" v="LINEAR" />"#));
}

/// New animations, added / renamed / deleted scenes save and reload
/// (animation-model.md §1–2).
#[test]
fn can3_structure() {
    use keyform_formats::can3::Can3Document;
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::animation::{CurveType, TrackKind};
    let Some(cmo3) = sample_files("cmo3")
        .into_iter()
        .find(|p| p.ends_with("hiyori_pro_t11.cmo3"))
    else {
        return;
    };
    let model = Cmo3Document::read(&std::fs::read(&cmo3).unwrap())
        .unwrap()
        .model;
    // A new animation.
    // The stored file path lets the reference resolve the model relative
    // to the animation.
    let out_dir = std::env::var_os("KEYFORM_CAN3_OUT").map(std::path::PathBuf::from);
    let file = out_dir.as_ref().map_or_else(
        || "test.can3".into(),
        |d| {
            d.join("new_animation_kf.can3")
                .to_string_lossy()
                .into_owned()
        },
    );
    let mut doc = Can3Document::new_animation(
        "Test",
        &file,
        &model,
        &cmo3.to_string_lossy(),
        "Scene1",
        90,
        30.0,
    )
    .unwrap();
    assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
    let a = &doc.animation;
    assert_eq!(a.scenes.len(), 1);
    assert_eq!(a.resources.len(), 1);
    let s = &a.scenes[0];
    assert_eq!(
        (s.name.as_str(), s.duration, s.fps, s.work_end),
        ("Scene1", 90, 30.0, 89)
    );
    let TrackKind::Model(m) = &s.tracks[1].kind else {
        panic!("model track")
    };
    assert_eq!(s.tracks[1].parent, Some(0));
    assert_eq!(m.params.len(), model.parameters.len());
    assert_eq!(m.parts.len(), model.parts.len());
    assert_eq!(m.groups.len(), model.parameter_groups.len());
    assert_eq!(
        m.eye_blink.as_ref().unwrap().targets.len(),
        model.eye_blink_parameters.len()
    );
    assert!(m
        .params
        .iter()
        .all(|p| p.guid.is_some() && p.curve.keys.is_empty()));
    // New tracks (animation-model.md §5.1): opacity 0–1 base 1, 100 %,
    // xy at the scene centre, anchor at the model canvas centre.
    let op = s.tracks[1].opacity.as_ref().unwrap();
    assert_eq!((op.curve.base, op.range), (1.0, (0.0, 1.0)));
    let v = s.tracks[1].visual.as_ref().unwrap();
    assert_eq!((v.scale_x.curve.base, v.scale_y.curve.base), (100.0, 100.0));
    assert_eq!(
        v.xy.keys[0].1,
        [s.canvas.0 as f32 / 2.0, s.canvas.1 as f32 / 2.0]
    );
    assert_eq!(
        v.anchor.keys[0].1,
        [
            model.canvas.width as f32 / 2.0,
            model.canvas.height as f32 / 2.0
        ]
    );
    // Keys on the new scene save and reload.
    if let TrackKind::Model(m) = &mut doc.animation.scenes[0].tracks[1].kind {
        let c = &mut m.params[0].curve;
        keyform_anim::scene::set_key(c, 0, 0.0, CurveType::Smooth);
        keyform_anim::scene::set_key(c, 45, 10.0, CurveType::Smooth);
        let a = &m.params[0];
        let (lo, hi) = keyform_anim::scene::clamp_range(a);
        keyform_anim::scene::refresh(&mut m.params[0].curve, lo, hi, false);
    }
    let edited = doc.animation.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Can3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(back.animation, edited);
    if let Some(out) = std::env::var_os("KEYFORM_CAN3_OUT") {
        std::fs::write(
            std::path::Path::new(&out).join("new_animation_kf.can3"),
            &bytes,
        )
        .unwrap();
    }
    // A sound track (WAV next to the animation) as the lip-sync source.
    if let Some(out) = out_dir.as_ref() {
        let wav = out.join("tone_kf.wav");
        let rate = 44100u32;
        let samples: Vec<i16> = (0..rate)
            .map(|i| ((i as f32 * 0.05).sin() * 12000.0) as i16)
            .collect();
        let mut d = Vec::new();
        let n = (samples.len() * 2) as u32;
        d.extend_from_slice(b"RIFF");
        d.extend_from_slice(&(36 + n).to_le_bytes());
        d.extend_from_slice(b"WAVEfmt ");
        d.extend_from_slice(&16u32.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&rate.to_le_bytes());
        d.extend_from_slice(&(rate * 2).to_le_bytes());
        d.extend_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&16u16.to_le_bytes());
        d.extend_from_slice(b"data");
        d.extend_from_slice(&n.to_le_bytes());
        for x in &samples {
            d.extend_from_slice(&x.to_le_bytes());
        }
        std::fs::write(&wav, d).unwrap();
        let mut back = back;
        back.add_sound_track(0, &wav.to_string_lossy(), 1.0)
            .unwrap();
        let s = &back.animation.scenes[0];
        // §4.1: 100 % speed, dB volume fixed at 0, duration from the
        // sound (1 s × fps), no lip-sync link.
        let TrackKind::Sound(snd) = &s.tracks[2].kind else {
            panic!()
        };
        assert_eq!(snd.speed, 100.0);
        let v = snd.volume.as_ref().unwrap();
        assert!(v.curve.fixed && v.curve.base == 0.0 && v.range == (-48.0, 6.0));
        assert_eq!(s.tracks[2].duration, s.fps.floor() as i32);
        let TrackKind::Model(m) = &s.tracks[1].kind else {
            panic!()
        };
        assert!(m.lip_sync.as_ref().unwrap().sync_track.is_none());
        let edited = back.animation.clone();
        let bytes = back.save_bytes().unwrap();
        let again = Can3Document::read(&bytes).unwrap();
        assert!(again.warnings.is_empty(), "{:?}", again.warnings);
        assert_eq!(again.animation, edited);
        std::fs::write(out.join("new_sound_kf.can3"), &bytes).unwrap();
    }
    // Scenes added to a sample, renamed, one deleted.
    let Some(sample) = sample_files("can3")
        .into_iter()
        .find(|p| p.ends_with("hiyori_pro_t04.can3"))
    else {
        return;
    };
    let mut doc = Can3Document::read(&std::fs::read(&sample).unwrap()).unwrap();
    let n0 = doc.animation.scenes.len();
    let before = doc.xml.write().len();
    let k = doc.add_scene("Added", &model, 60, 30.0).unwrap();
    assert_eq!(k, n0);
    doc.rename_scene(0, "First").unwrap();
    doc.delete_scene(1).unwrap();
    let names: Vec<String> = doc
        .animation
        .scenes
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(names.len(), n0);
    assert_eq!(names[0], "First");
    assert_eq!(names.last().unwrap(), "Added");
    // A second model track, then deleting the first one.
    let last = doc.animation.scenes.len() - 1;
    doc.add_model_track(last, &model, "Second").unwrap();
    let s = &doc.animation.scenes[last];
    assert_eq!(s.tracks.len(), 3);
    assert_eq!(s.tracks[2].name, "Second");
    assert_eq!(s.tracks[2].parent, Some(0));
    doc.delete_track(last, 1).unwrap();
    assert_eq!(doc.animation.scenes[last].tracks.len(), 2);
    assert!(doc.delete_track(last, 0).is_err(), "root group");
    let edited = doc.animation.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Can3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(back.animation, edited);
    // The deleted scene's objects went (one scene of ~140 removed, one
    // smaller added).
    eprintln!("xml {} → {} bytes", before, back.xml.write().len());
    if let Some(out) = std::env::var_os("KEYFORM_CAN3_OUT") {
        std::fs::write(
            std::path::Path::new(&out).join("hiyori_scenes_kf.can3"),
            &bytes,
        )
        .unwrap();
    }
}

/// Scene settings and track events save in place and reach the motion3
/// export (dialogs.md §5, motion3-json.md §2 "UserData").
#[test]
fn can3_scene_settings_and_events() {
    use keyform_anim::motion_export::{export_motion, ExportOptions};
    use keyform_formats::can3::Can3Document;
    use keyform_model::animation::{CurveType, TrackKind};
    let Some(path) = sample_files("can3")
        .into_iter()
        .find(|p| p.ends_with("hiyori_pro_t04.can3"))
    else {
        return;
    };
    let mut doc = Can3Document::read(&std::fs::read(&path).unwrap()).unwrap();
    {
        let s = &mut doc.animation.scenes[0];
        s.name = "Renamed".into();
        s.fps = 60.0;
        s.work_start = 10;
        s.work_end = 100;
        s.fade_in_ms = 500;
        s.fade_out_ms = 250;
        s.bezier_restricted = true;
        s.loop_motion = true;
        s.default_part_curve = CurveType::Linear;
        let ti = s
            .tracks
            .iter()
            .position(|t| matches!(t.kind, TrackKind::Model(_)))
            .unwrap();
        s.tracks[ti].user_data = vec![(20, "wave".into()), (60, "smile".into())];
    }
    let edited = doc.animation.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Can3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(back.animation, edited);
    let s = &back.animation.scenes[0];
    let ti = s
        .tracks
        .iter()
        .position(|t| matches!(t.kind, TrackKind::Model(_)))
        .unwrap();
    let m = export_motion(s, ti, &ExportOptions::default(), &|_| None, &|_| None).unwrap();
    assert_eq!(m.fade_in, Some(0.5));
    assert!(m.beziers_restricted);
    // Events in the work area, times from its start: (20−10)/60, (60−10)/60.
    assert_eq!(m.user_data.len(), 2);
    // (2 decimals: the last curve processed is a PartOpacity one.)
    assert!((m.user_data[0].0 - 10.0 / 60.0).abs() < 0.01 && m.user_data[0].1 == "wave");
}

/// Keyform-written form keys and controllers, saved again by the reference
/// (`KEYFORM_BB_DIR` with form_kf / form_ref / ctrl_kf / ctrl_ref .can3):
/// they read back equal. spec: form-animation-and-controllers.md §3
#[test]
fn can3_reference_round_trip() {
    use keyform_formats::can3::Can3Document;
    use keyform_model::animation::TrackKind;
    let Some(dir) = std::env::var_os("KEYFORM_BB_DIR") else {
        return;
    };
    let dir = std::path::Path::new(&dir);
    let model = |name: &str| {
        let d = Can3Document::read(&std::fs::read(dir.join(name)).unwrap()).unwrap();
        assert!(d.warnings.is_empty(), "{name}: {:?}", d.warnings);
        d.animation.scenes[0]
            .tracks
            .iter()
            .find_map(|t| match &t.kind {
                TrackKind::Model(m) => Some((**m).clone()),
                _ => None,
            })
            .unwrap()
    };
    let (a, b) = (model("form_kf.can3"), model("form_ref.can3"));
    assert!(!a.form.global.is_empty() && !a.form.local.is_empty());
    assert_eq!(a.form, b.form);
    let (a, b) = (model("ctrl_kf.can3"), model("ctrl_ref.can3"));
    assert!(!a.controllers.controllers.is_empty());
    assert_eq!(a.controllers.controllers, b.controllers.controllers);
    for (p, q) in a.controllers.points.iter().zip(&b.controllers.points) {
        assert_eq!(
            (&p.guid, &p.mesh_id, p.tri, p.hash),
            (&q.guid, &q.mesh_id, q.tri, q.hash)
        );
        for k in 0..3 {
            assert!((p.bary[k] - q.bary[k]).abs() < 1e-9);
        }
    }
}

#[test]
#[ignore]
fn can3_dump_expression_scenes() {
    use keyform_formats::can3::Can3Document;
    use keyform_model::animation::TrackKind;
    let path = std::env::var("CAN3").unwrap();
    let doc = Can3Document::read(&std::fs::read(&path).unwrap()).unwrap();
    for s in &doc.animation.scenes {
        let m = s.tracks.iter().find_map(|t| match &t.kind {
            TrackKind::Model(m) => Some(m),
            _ => None,
        });
        let Some(m) = m else { continue };
        let keyed: Vec<String> = m
            .params
            .iter()
            .filter(|a| !a.curve.keys.is_empty())
            .map(|a| {
                format!(
                    "{}={:?}@{:?}",
                    a.id.trim_start_matches("live2dParam_"),
                    a.curve.keys.iter().map(|k| k.value).collect::<Vec<_>>(),
                    a.curve.keys.iter().map(|k| k.frame).collect::<Vec<_>>()
                )
            })
            .collect();
        eprintln!(
            "{} dur {} ws {} keyed {}: {}",
            s.name,
            s.duration,
            s.work_start,
            keyed.len(),
            keyed.join(" ")
        );
        eprintln!(
            "  fades {} {} export {}",
            s.fade_in_ms, s.fade_out_ms, s.export_motion
        );
    }
}

/// Expression scenes → exp3.json, compared with the reference-exported
/// runtime expressions (chitose: 7 scenes).
#[test]
fn can3_expression_export() {
    use keyform_anim::expression::{from_scene, scene_exp3_json, Expression};
    use keyform_formats::can3::Can3Document;
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::animation::TrackKind;
    let Some(can3) = sample_files("can3")
        .into_iter()
        .find(|p| p.ends_with("chitose_expressions_t01.can3"))
    else {
        return;
    };
    let Some(cmo3) = sample_files("cmo3")
        .into_iter()
        .find(|p| p.ends_with("chitose_t01.cmo3"))
    else {
        return;
    };
    let model = Cmo3Document::read(&std::fs::read(&cmo3).unwrap())
        .unwrap()
        .model;
    let dir = can3.parent().unwrap().join("runtime/expressions");
    let doc = Can3Document::read(&std::fs::read(&can3).unwrap()).unwrap();
    let mut compared = 0;
    for s in &doc.animation.scenes {
        let Ok(text) = std::fs::read_to_string(dir.join(format!("{}.exp3.json", s.name))) else {
            continue;
        };
        let reference = Expression::parse(&text).unwrap();
        let ti = s
            .tracks
            .iter()
            .position(|t| matches!(t.kind, TrackKind::Model(_)))
            .unwrap();
        let e = from_scene(s, ti, &|id| {
            model
                .parameters
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.default)
        })
        .unwrap();
        let mine = Expression::parse(&scene_exp3_json(&e).to_string()).unwrap();
        assert_eq!(mine, reference, "{}", s.name);
        assert!(!scene_exp3_json(&e).to_string().contains("Version"));
        compared += 1;
    }
    assert_eq!(compared, 7);
}

#[test]
#[ignore]
fn moc3_dump_colors() {
    use keyform_formats::moc3::Moc3Raw;
    let r = Moc3Raw::read(&std::fs::read(std::env::var("MOC3").unwrap()).unwrap()).unwrap();
    let ty = r.i32s("deformers.type");
    let si = r.i32s("deformers.specific_index");
    let wb = r.i32s("warps.color_begin");
    let rb = r.i32s("rotations.color_begin");
    let wkc = r.i32s("warps.keyform_count");
    let rkc = r.i32s("rotations.keyform_count");
    for d in 0..ty.len() {
        let (b, n) = if ty[d] == 0 {
            (wb[si[d] as usize], wkc[si[d] as usize])
        } else {
            (rb[si[d] as usize], rkc[si[d] as usize])
        };
        eprintln!(
            "deformer {d} type {} idx {} color_begin {b} keyforms {n}",
            ty[d], si[d]
        );
    }
    let ab = r.i32s("art_meshes.color_begin");
    eprintln!(
        "art meshes color_begin first {:?} last {:?}",
        ab.first(),
        ab.last()
    );
}

#[test]
#[ignore]
fn moc3_dump_blend_shapes() {
    use keyform_formats::moc3::{layout::SECTIONS, Moc3Raw, SectionData};
    let r = Moc3Raw::read(&std::fs::read(std::env::var("MOC3").unwrap()).unwrap()).unwrap();
    eprintln!("version {} counts {:?}", r.version, r.counts);
    for sec in SECTIONS.iter().filter(|s| s.since <= r.version) {
        if !(sec.name.starts_with("bs_")
            || sec.name.starts_with("parameters.bs")
            || sec.name == "parameters.type")
        {
            continue;
        }
        let d = &r.sections[sec.name];
        let txt = match d {
            SectionData::I32(v) => format!("{v:?}"),
            SectionData::F32(v) => format!("{v:?}"),
            other => format!("len {}", other.len()),
        };
        eprintln!("{} = {}", sec.name, &txt[..txt.len().min(300)]);
    }
    let keys = r.f32s("keys");
    eprintln!(
        "keys len {} tail {:?}",
        keys.len(),
        &keys[keys.len().saturating_sub(40)..]
    );
    let ak = r.i32s("art_meshes.keyform_count");
    let ab = r.i32s("art_meshes.keyform_begin");
    let ids = match &r.sections["art_meshes.id"] {
        SectionData::Ids(v) => v.clone(),
        _ => vec![],
    };
    if let Some(i) = ids.iter().position(|x| x == "ArtMesh49") {
        eprintln!("ArtMesh49 index {i} kf begin {} count {}", ab[i], ak[i]);
    }
    let op = r.f32s("art_mesh_keyforms.opacity");
    eprintln!(
        "art keyforms {} last 4 opacity {:?}",
        op.len(),
        &op[op.len() - 4..]
    );
    let dro = r.f32s("art_mesh_keyforms.draw_order");
    eprintln!("last 4 draw order {:?}", &dro[dro.len() - 4..]);
    if r.version >= 5 {
        let mc = r.i32s("art_mesh_keyforms.multiply_color");
        eprintln!("last 4 mul color idx {:?}", &mc[mc.len() - 4..]);
        let mr = r.f32s("multiply_colors.r");
        let sr = r.f32s("screen_colors.r");
        eprintln!(
            "mul pool len {} tail {:?} scr tail {:?}",
            mr.len(),
            &mr[mr.len() - 4..],
            &sr[sr.len() - 4..]
        );
    }
    let pb = r.i32s("art_mesh_keyforms.position_begin");
    eprintln!(
        "last 4 pos begin {:?} positions len {}",
        &pb[pb.len() - 4..],
        r.f32s("keyform_positions.xy").len()
    );
}

/// moc3 export of the reference-made Phase 5 test models (`KEYFORM_BB_P5`):
/// an offscreen part (v6) and a blend shape (v4.2, v5.0, v5.3) match the
/// reference's exports byte for byte. spec: moc3.md §8.4, §8.9
#[test]
fn moc3_export_matches_reference_p5() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::moc3::{export_moc3, ExportOptions};
    let Some(dir) = std::env::var_os("KEYFORM_BB_P5") else {
        return;
    };
    let dir = std::path::Path::new(&dir);
    for (model, version, reference) in [
        ("off.cmo3", 6u8, "off_runtime/off.moc3"),
        ("bs.cmo3", 4, "bs_v4/bs.moc3"),
        ("bs.cmo3", 5, "bs_v5/bs.moc3"),
        ("bs.cmo3", 6, "bs_runtime/bs.moc3"),
        // Blend shapes on a warp, a rotation deformer, a part and an
        // offscreen part (v4.2 drops the last three with a warning).
        ("bs2.cmo3", 4, "bs2_v4/bs2.moc3"),
        ("bs2.cmo3", 5, "bs2_v5/bs2.moc3"),
        ("bs2.cmo3", 6, "bs2_v6/bs2.moc3"),
        // bs2 plus a glue blend shape.
        ("bs3.cmo3", 5, "bs3_v5/bs3.moc3"),
        ("bs3.cmo3", 6, "bs3_v6/bs3.moc3"),
        // bs3 with a clip list on the offscreen part.
        ("clip.cmo3", 6, "clip_v6/clip.moc3"),
    ] {
        let doc = Cmo3Document::read(&std::fs::read(dir.join(model)).unwrap()).unwrap();
        let mut o = ExportOptions::for_model(&doc.model);
        o.version = version;
        let (raw, rep) = export_moc3(&doc.model, &o).unwrap();
        assert_eq!(
            rep.warnings.len(),
            usize::from(model == "bs2.cmo3" && version == 4),
            "{model} v{version}: {:?}",
            rep.warnings
        );
        let want = std::fs::read(dir.join(reference)).unwrap();
        assert!(
            raw.write().unwrap() == want,
            "{model} v{version} differs from {reference}"
        );
    }
}

#[test]
#[ignore]
fn can3_dump_track_opacity() {
    use keyform_formats::can3::Can3Document;
    let doc = Can3Document::read(&std::fs::read(std::env::var("CAN3").unwrap()).unwrap()).unwrap();
    for t in &doc.animation.scenes[0].tracks {
        if let Some(o) = &t.opacity {
            eprintln!(
                "{}: opacity base {} range {:?} keys {}",
                t.name,
                o.curve.base,
                o.range,
                o.curve.keys.len()
            );
        }
    }
}

/// Physics editing (physics-editor.md): an edited setting, a new one, a
/// deleted one and the FPS hint survive a save and reload.
#[test]
fn physics_edits_save() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::{PhysicsInput, PhysicsKind, PhysicsOutput, PhysicsSetting, PhysicsVertex};
    let only = std::env::var("KEYFORM_PHYSICS_SRC").unwrap_or_default();
    for path in sample_files("cmo3") {
        if !path.to_string_lossy().contains(&only) {
            continue;
        }
        let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
        let m = &mut doc.model;
        if m.physics.settings.len() < 2 || m.parameters.len() < 3 {
            continue;
        }
        let ph = &mut m.physics;
        ph.settings.remove(1);
        let s = &mut ph.settings[0];
        s.name = "Edited <&> group".into();
        s.vertices[1].radius = 7.5;
        s.vertices.push(PhysicsVertex {
            position: [0.0, 17.5],
            mobility: 0.95,
            delay: 1.0,
            acceleration: 1.0,
            radius: 10.0,
        });
        s.inputs.push(PhysicsInput {
            parameter: 2,
            weight: 25.0,
            kind: PhysicsKind::Angle,
            reflect: true,
        });
        s.position.maximum = 9.5;
        ph.settings.push(PhysicsSetting {
            guid: keyform_model::new_guid(),
            id: "PhysicsSetting99".into(),
            name: "New group".into(),
            enabled: true,
            inputs: vec![PhysicsInput {
                parameter: 0,
                weight: 100.0,
                kind: PhysicsKind::X,
                reflect: false,
            }],
            outputs: vec![PhysicsOutput {
                parameter: 1,
                vertex_index: 1,
                translation_scale: [0.0, 0.0],
                angle_scale: 1.25,
                weight: 100.0,
                kind: PhysicsKind::Angle,
                reflect: false,
            }],
            vertices: vec![
                PhysicsVertex {
                    position: [0.0, 0.0],
                    mobility: 1.0,
                    delay: 1.0,
                    acceleration: 1.0,
                    radius: 0.0,
                },
                PhysicsVertex {
                    position: [0.0, 10.0],
                    mobility: 0.95,
                    delay: 1.0,
                    acceleration: 1.0,
                    radius: 10.0,
                },
            ],
            position: Default::default(),
            angle: Default::default(),
        });
        ph.fps = Some(30.0);
        let edited = doc.model.clone();
        let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{path:?}: {:?}", back.warnings);
        assert_eq!(back.model.physics, edited.physics, "{path:?}");
        if let Some(out) = std::env::var_os("KEYFORM_PHYSICS_OUT") {
            std::fs::write(out, &bytes).unwrap();
        }
        eprintln!("{path:?}: physics edits saved");
        break;
    }
}

/// Runtime export of the reference-made motion-sync model (`KEYFORM_BB_P5`,
/// ms.cmo3): moc3, model3, motionsync3, cdi3 and physics3 equal the
/// reference's export byte for byte. spec: motion-sync.md §2
#[test]
fn runtime_export_with_motion_sync_matches_reference() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_formats::moc3::ExportOptions;
    use keyform_formats::runtime_json::{export_runtime, RuntimeExportOptions};
    let Some(dir) = std::env::var_os("KEYFORM_BB_P5") else {
        return;
    };
    let dir = std::path::Path::new(&dir);
    let doc = Cmo3Document::read(&std::fs::read(dir.join("ms.cmo3")).unwrap()).unwrap();
    assert_eq!(doc.model.motion_sync.len(), 1);
    assert_eq!(doc.model.motion_sync[0].version, 1);
    let mut o = ExportOptions::for_model(&doc.model);
    o.version = 6;
    let ro = RuntimeExportOptions {
        moc3: o,
        export_physics: true,
        export_motion_sync: true,
        extend_edge_color: doc.model.export.extend_edge_color,
        export_display_info: true,
        export_user_data: true,
    };
    let out = tempdir("ms_export");
    export_runtime(&doc.model, &ro, &out, "ms").unwrap();
    for f in [
        "moc3",
        "model3.json",
        "motionsync3.json",
        "cdi3.json",
        "physics3.json",
    ] {
        let want = std::fs::read(dir.join(format!("ms_rt/ms.{f}"))).unwrap();
        let got = std::fs::read(out.join(format!("ms.{f}"))).unwrap();
        assert!(want == got, "ms.{f} differs");
    }
}

/// Motion-sync edits (a changed setting, a new one) survive a save and
/// reload; an unchanged save keeps the document as it was.
#[test]
fn motion_sync_edits_save() {
    use keyform_formats::cmo3::Cmo3Document;
    let Some(dir) = std::env::var_os("KEYFORM_BB_P5") else {
        return;
    };
    let path = std::path::Path::new(&dir).join("ms.cmo3");
    let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
    let s = &mut doc.model.motion_sync;
    let mut extra = s[0].clone();
    s[0].name = "Edited <&>".into();
    s[0].blend = 0.25;
    s[0].smoothing = 40;
    s[0].cubism[1].damper = 0.05;
    s[0].audio[2].enabled = false;
    s[0].mappings[1].targets[1].1 = 0.8;
    extra.guid = keyform_model::new_guid();
    extra.id = "MotionSyncSetting2".into();
    extra.name = "Second".into();
    for c in &mut extra.cubism {
        c.guid = keyform_model::new_guid();
    }
    // Every audio element keeps its mapping: the reference drops a setting
    // whose mapping misses elements (observed; Q-0039).
    s.push(extra);
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(back.model.motion_sync, edited.motion_sync);
    if let Some(out) = std::env::var_os("KEYFORM_MS_OUT") {
        std::fs::write(out, &bytes).unwrap();
    }
}

/// A fresh, empty directory under the target temp folder for one test.
fn tempdir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("keyform-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Applying a model as a template to a copy of itself whose art meshes
/// lost their rig (root-level, one keyform) restores the deformation:
/// the meshes evaluate where the original's do at other parameter values,
/// and the result saves. spec: templates.md §3, §5
#[test]
fn template_restores_a_stripped_rig() {
    use keyform_edit::template::{apply, auto_match, mesh_infos, ApplyOptions, Layout};
    use keyform_formats::cmo3::Cmo3Document;
    use std::sync::Arc;
    let Some(path) = sample_files("cmo3")
        .into_iter()
        .find(|p| p.ends_with("hiyori_free_t08.cmo3"))
    else {
        return;
    };
    let tpl = Cmo3Document::read(&std::fs::read(&path).unwrap())
        .unwrap()
        .model;
    let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
    // Strip: every mesh at its default-state canvas shape, at the root.
    let vals: Vec<f32> = tpl.parameters.iter().map(|p| p.default).collect();
    let frame = keyform_eval::Evaluator::new(&tpl).evaluate(&vals);
    for (i, a) in doc.model.art_meshes.iter_mut().enumerate() {
        let a = Arc::make_mut(a);
        let mut f = a.grid.keyforms[0].clone();
        f.positions = frame.meshes[i].positions.clone();
        a.target = None;
        a.grid = keyform_model::KeyformGrid::single(f);
        a.grid.guids = vec![keyform_model::new_guid()];
        a.blend_shapes = Default::default();
    }
    let layout = Layout::default();
    let w = tpl.canvas.width as f32;
    let h = tpl.canvas.height as f32;
    let pairs: Vec<(usize, usize)> = auto_match(
        &mesh_infos(&doc.model),
        &mesh_infos(&tpl),
        &layout,
        w.hypot(h),
        true,
        &[],
    )
    .into_iter()
    .enumerate()
    .filter_map(|(d, t)| t.map(|t| (d, t)))
    .collect();
    assert_eq!(pairs.len(), tpl.art_meshes.len());
    let rep = apply(
        &mut doc.model,
        &tpl,
        &pairs,
        &layout,
        &ApplyOptions::default(),
    )
    .unwrap();
    assert_eq!(rep.meshes, pairs.len());
    assert_eq!(rep.parameters_added, 0, "all parameters reused by ID");
    // Compare at a few poses.
    let pick = |id: &str| tpl.parameters.iter().position(|p| p.id == id);
    for (id, v) in [
        ("ParamAngleX", 20.0),
        ("ParamAngleY", -15.0),
        ("ParamBodyAngleX", 8.0),
    ] {
        let Some(pi) = pick(id) else { continue };
        let mut a = vals.clone();
        a[pi] = v;
        let fa = keyform_eval::Evaluator::new(&tpl).evaluate(&a);
        let pd = doc
            .model
            .parameters
            .iter()
            .position(|p| p.id == id)
            .unwrap();
        let mut b: Vec<f32> = doc.model.parameters.iter().map(|p| p.default).collect();
        b[pd] = v;
        let fb = keyform_eval::Evaluator::new(&doc.model).evaluate(&b);
        let mut worst = 0.0f32;
        for (i, &(d, t)) in pairs.iter().enumerate() {
            let _ = i;
            for (x, y) in fa.meshes[t].positions.iter().zip(&fb.meshes[d].positions) {
                worst = worst.max((x - y).abs());
            }
        }
        assert!(worst < 0.05, "{id}: worst {worst} px");
    }
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.model.same_data(&doc.model));
}

/// A motion-sync setting added to a document from before the set existed
/// (CModelSource v13): the set is added, the schema marker raised, and the
/// setting reloads. spec: motion-sync.md §1 (Q-0039)
#[test]
fn motion_sync_in_older_document() {
    use keyform_formats::cmo3::Cmo3Document;
    let Some(dir) = std::env::var_os("KEYFORM_BB_P5") else {
        return;
    };
    let Some(old) = sample_files("cmo3")
        .into_iter()
        .find(|p| p.ends_with("hiyori_free_t08.cmo3"))
    else {
        return;
    };
    let ms =
        Cmo3Document::read(&std::fs::read(std::path::Path::new(&dir).join("ms.cmo3")).unwrap())
            .unwrap();
    let mut doc = Cmo3Document::read(&std::fs::read(&old).unwrap()).unwrap();
    assert!(doc.model.motion_sync.is_empty());
    let mut s = ms.model.motion_sync[0].clone();
    s.guid = keyform_model::new_guid();
    for c in &mut s.cubism {
        c.guid = keyform_model::new_guid();
    }
    doc.model.motion_sync.push(s);
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert_eq!(back.model.motion_sync, edited.motion_sync);
    if let Some(out) = std::env::var_os("KEYFORM_MS_OLD_OUT") {
        std::fs::write(out, &bytes).unwrap();
    }
}

/// Sway settings (`CAutoYureConfigExtension`) load from a reference-made
/// file, and edits save and reload. spec: generators.md §2 "Storage"
#[test]
fn sway_settings_load_and_save() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::DeformerKind;
    use std::sync::Arc;
    let Some(dir) = std::env::var_os("KEYFORM_BB_P5") else {
        return;
    };
    let mut doc =
        Cmo3Document::read(&std::fs::read(std::path::Path::new(&dir).join("sway.cmo3")).unwrap())
            .unwrap();
    let d = doc
        .model
        .deformers
        .iter()
        .position(|x| x.info.id == "Warp1")
        .unwrap();
    let DeformerKind::Warp(w) = &doc.model.deformers[d].kind else {
        panic!()
    };
    assert_eq!(w.sway.len(), 1);
    let c = w.sway[0];
    assert_eq!(doc.model.parameters[c.parameter].id, "ParamBodyAngleZ");
    assert_eq!(
        (c.left.scale_x, c.left.scale_y, c.left.expand, c.left.decay),
        (7.1, 5.2, 0.0, 3.0)
    );
    assert!(c.sync && !c.flip && c.root == 0);
    let DeformerKind::Warp(w) = &mut Arc::make_mut(&mut doc.model.deformers[d]).kind else {
        panic!()
    };
    w.sway[0].right.scale_x = 9.5;
    w.sway[0].sync = false;
    w.sway[0].root = 3;
    w.sway[0].flip = true;
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    let DeformerKind::Warp(a) = &back.model.deformers[d].kind else {
        panic!()
    };
    let DeformerKind::Warp(b) = &edited.deformers[d].kind else {
        panic!()
    };
    assert_eq!(a.sway, b.sway);
    if let Some(out) = std::env::var_os("KEYFORM_SWAY_OUT") {
        std::fs::write(out, &bytes).unwrap();
    }
}

/// Face deformers and face motion on a sample: warps are created in the
/// feature parts with their meshes, then Angle X / Y keyforms are written
/// and the model still saves. spec: generators.md §3.1–3.2
#[test]
fn face_generators_on_a_sample() {
    use keyform_edit::face::{generate_deformers, generate_motion, Feature};
    use keyform_formats::cmo3::Cmo3Document;
    let Some(path) = sample_files("cmo3")
        .into_iter()
        .find(|p| p.ends_with("hiyori_free_t08.cmo3"))
    else {
        return;
    };
    let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
    let m = &mut doc.model;
    let part = |id: &str| m.parts.iter().position(|p| p.info.id == id);
    let mut parts = [None; 11];
    parts[0] = part("PartFace");
    parts[1] = part("PartEye");
    parts[9] = part("PartMouth");
    parts[10] = part("PartNose");
    assert!(parts[0].is_some() && parts[1].is_some(), "sample part ids");
    let values: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
    let n_def = m.deformers.len();
    let warps = generate_deformers(m, &parts, (5, 5), &values).unwrap();
    assert_eq!(m.deformers.len(), n_def + warps.len());
    let (px, py) = (
        m.parameters
            .iter()
            .position(|p| p.id == "ParamAngleX")
            .unwrap(),
        m.parameters
            .iter()
            .position(|p| p.id == "ParamAngleY")
            .unwrap(),
    );
    // hiyori has one Eye part: use its warp for both eyes.
    let mut w = warps.clone();
    let eye = w.iter().find(|x| x.0 == Feature::EyeL).unwrap().1;
    w.push((Feature::EyeR, eye));
    let n = generate_motion(m, &w, px, py, [30.0, 30.0], &values).unwrap();
    assert!(n >= 8, "{n} keyforms");
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.model.same_data(&edited));
}

/// 3D rotation settings (`CRotate3DExtension`) on an art mesh and a warp
/// save and reload, with and without depth data. spec: generators.md §1
#[test]
fn rotate3d_settings_save() {
    use keyform_formats::cmo3::Cmo3Document;
    use keyform_model::{Rotate3dConfig, Rotate3dDepth};
    let Some(path) = sample_files("cmo3")
        .into_iter()
        .find(|p| p.ends_with("hiyori_free_t08.cmo3"))
    else {
        return;
    };
    let mut doc = Cmo3Document::read(&std::fs::read(&path).unwrap()).unwrap();
    let m = &mut doc.model;
    let px = m
        .parameters
        .iter()
        .position(|p| p.id == "ParamAngleX")
        .unwrap();
    let py = m
        .parameters
        .iter()
        .position(|p| p.id == "ParamAngleY")
        .unwrap();
    let mesh = m.art_meshes[0].info.guid.clone();
    let warp = m
        .deformers
        .iter()
        .find(|d| matches!(d.kind, keyform_model::DeformerKind::Warp(_)))
        .unwrap()
        .info
        .guid
        .clone();
    let form = m.art_meshes[0]
        .grid
        .guids
        .first()
        .cloned()
        .unwrap_or_default();
    m.rotate3d.push((
        mesh,
        Rotate3dConfig {
            parameters: [Some(px), Some(py)],
            ..Default::default()
        },
    ));
    m.rotate3d.push((
        warp,
        Rotate3dConfig {
            camera: [10.0, -20.0],
            centre: [5.0, 6.0],
            range: [25.0, 15.0],
            perspective: 0.4,
            method: 0,
            filter: 3,
            parameters: [Some(px), None],
            depth_enabled: true,
            depth: Some(Rotate3dDepth {
                buffer: vec![1.0, 2.5, -3.0],
                row: 1,
                column: 3,
                reference_form: form,
                ..Default::default()
            }),
            ..Default::default()
        },
    ));
    doc.model.rotate3d.sort_by(|a, b| a.0.cmp(&b.0));
    let edited = doc.model.clone();
    let bytes = doc.save_bytes().unwrap_or_else(|e| panic!("{e}"));
    if let Some(out) = std::env::var_os("KEYFORM_R3_OUT") {
        std::fs::write(out, &bytes).unwrap();
    }
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(back.model.rotate3d, edited.rotate3d);
}

/// An art path made in the reference loads (11 points, LCNS, div 20) and
/// evaluates to a stroke; an unchanged save keeps it. spec: art-path.md
#[test]
fn art_path_loads_and_strokes() {
    use keyform_formats::cmo3::Cmo3Document;
    let Some(dir) = std::env::var_os("KEYFORM_BB_P5") else {
        return;
    };
    let mut doc = Cmo3Document::read(
        &std::fs::read(std::path::Path::new(&dir).join("artpath.cmo3")).unwrap(),
    )
    .unwrap();
    assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
    let m = &doc.model;
    assert_eq!(m.art_paths.len(), 1);
    let a = &m.art_paths[0];
    assert_eq!(a.grid.keyforms[0].points.len(), 11);
    assert_eq!(
        (a.div_num, a.algorithm),
        (20, keyform_model::LineAlgorithm::Lcns)
    );
    assert!(a.target.is_some());
    let values: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
    let frame = keyform_eval::Evaluator::new(m).evaluate(&values);
    let st = &frame.art_paths[0];
    assert!(frame
        .items
        .contains(&keyform_eval::draw_order::DrawItem::ArtPath(0)));
    assert!(st.stroke.indices.len() > 100);
    // Canvas coordinates (not the warp's 0–1 local space).
    assert!(st.stroke.positions.iter().any(|v| *v > 10.0));
    // A small unrelated edit saves with the art path intact.
    let p0 = std::sync::Arc::make_mut(&mut doc.model.parameters[0]);
    p0.name.push('!');
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert_eq!(back.model.art_paths, doc.model.art_paths);
    // Name and visibility are written back; other edits are refused.
    let mut doc = back;
    {
        let a = std::sync::Arc::make_mut(&mut doc.model.art_paths[0]);
        a.info.visible = false;
        a.info.name = "Path renamed".into();
    }
    let back = Cmo3Document::read(&doc.save_bytes().unwrap()).unwrap();
    assert!(!back.model.art_paths[0].info.visible);
    assert_eq!(back.model.art_paths[0].info.name, "Path renamed");
    // Settings and points are written; a new art path is created; both
    // reload as saved.
    {
        let a = std::sync::Arc::make_mut(&mut doc.model.art_paths[0]);
        a.div_num = 5;
        a.algorithm = keyform_model::LineAlgorithm::Sns;
        let f = &mut a.grid.keyforms[0];
        f.points[0].color = [1.0, 0.0, 0.0, 1.0];
        f.points[0].width = 3.5;
        f.points.remove(1);
    }
    let root = doc.model.root_part;
    let k = keyform_edit::artpath::create(
        &mut doc.model,
        root,
        &[[100.0, 100.0], [300.0, 150.0], [500.0, 100.0]],
        keyform_edit::artpath::Defaults::default(),
    )
    .unwrap();
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(back.model.art_paths.len(), 2);
    assert_eq!(
        back.model.art_paths[0].grid.keyforms,
        doc.model.art_paths[0].grid.keyforms
    );
    assert_eq!(back.model.art_paths[0].div_num, 5);
    let nb = back
        .model
        .art_paths
        .iter()
        .find(|a| a.info.guid == doc.model.art_paths[k].info.guid)
        .unwrap();
    assert_eq!(nb.grid.keyforms, doc.model.art_paths[k].grid.keyforms);
    let root = back.model.root_part.unwrap();
    assert!(back.model.parts[root]
        .children
        .iter()
        .any(|c| matches!(c, keyform_model::PartChild::ArtPath(_))));
    // Deleting it saves too.
    let mut doc = back;
    let k = doc
        .model
        .art_paths
        .iter()
        .position(|a| a.info.id.starts_with("ArtPath") && a.grid.keyforms[0].points.len() == 3)
        .unwrap();
    keyform_edit::artpath::delete(&mut doc.model, k).unwrap();
    let back = Cmo3Document::read(&doc.save_bytes().unwrap()).unwrap();
    assert_eq!(back.model.art_paths.len(), 1);
    std::fs::write(
        std::env::temp_dir().join("keyform-artpath-edit.cmo3"),
        &bytes,
    )
    .unwrap();
}
