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
