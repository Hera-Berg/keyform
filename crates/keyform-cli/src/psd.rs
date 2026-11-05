//! `kf psd`: PSD import into a project (spec: 70-advanced/psd-import.md
//! §6): every layer as a new art mesh, or `--register` only.

use std::path::Path;

use keyform_formats::cmo3::Cmo3Document;
use keyform_formats::psd::Psd;
use keyform_formats::psd_import::{add_psd_layers, register_psd, ImportOptions};

type R = Result<(), Box<dyn std::error::Error>>;

/// `kf psd <in.cmo3> <file.psd> <out.cmo3 | out.png> [--register] [--legacy] [render options]`
pub fn run(input: &Path, psd_path: &Path, out: &Path, rest: &[&str]) -> R {
    let mut doc = Cmo3Document::read(&std::fs::read(input)?)?;
    let data = std::fs::read(psd_path)?;
    let psd = Psd::parse(&data)?;
    let name = psd_path
        .file_name()
        .map_or("PSD".into(), |n| n.to_string_lossy().to_string());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let modified = std::fs::metadata(psd_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as i64);
    let o = ImportOptions {
        legacy_blend: rest.contains(&"--legacy"),
        path: psd_path.to_string_lossy().to_string(),
        modified_ms: modified,
        imported_ms: now,
        ..ImportOptions::default()
    };
    if rest.contains(&"--register") {
        register_psd(&mut doc.model, &name, &psd, &o)?;
        println!("{name}: registered without art meshes");
    } else {
        let r = add_psd_layers(&mut doc.model, &name, &psd, &o)?;
        println!(
            "{name}: {} art meshes, {} parts, atlas {:?}",
            r.art_meshes.len(),
            r.parts.len(),
            r.atlas.map(|t| &doc.model.textures[t].name)
        );
        for w in r.warnings {
            eprintln!("warning: {w}");
        }
    }
    let render_args: Vec<&str> = rest
        .iter()
        .copied()
        .filter(|a| *a != "--register" && *a != "--legacy")
        .collect();
    if out
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        return crate::render::render_model(&doc.model, out, &render_args);
    }
    doc.save_to(out)?;
    println!("wrote {out:?}");
    Ok(())
}
