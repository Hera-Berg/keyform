//! `kf psd`: PSD import into a project (spec: 70-advanced/psd-import.md
//! §6): every layer as a new art mesh, or `--register` only.

use std::path::Path;

use keyform_formats::cmo3::Cmo3Document;
use keyform_formats::psd::Psd;
use keyform_formats::psd_import::{
    add_psd_layers, new_model_from_psd, register_psd, ImportOptions,
};

type R = Result<(), Box<dyn std::error::Error>>;

/// `kf psd <in.cmo3> <file.psd> <out.cmo3 | out.png> [--register | --replace NAME] [--legacy] [render options]`
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
    let replace = rest
        .iter()
        .position(|a| *a == "--replace")
        .and_then(|i| rest.get(i + 1))
        .copied();
    if let Some(old) = replace {
        let k = doc
            .model
            .layered_images
            .iter()
            .position(|l| l.name == old)
            .ok_or_else(|| format!("no source image named {old:?}"))?;
        let r = keyform_formats::psd_replace::replace_psd(&mut doc.model, &[k], &name, &psd, &o)?;
        println!(
            "{name} replaces {old}: {} pairs (by {} first), {} model images relinked, {} new art meshes",
            r.pairs.len(),
            if r.by_id { "ID" } else { "name" },
            r.relinked.len(),
            r.import.art_meshes.len()
        );
    } else if rest.contains(&"--register") {
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
        .filter(|a| {
            *a != "--register" && *a != "--legacy" && *a != "--replace" && Some(*a) != replace
        })
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

/// `kf newpsd <file.psd> <out.cmo3 | out.png> [render options]`: a new
/// model from a PSD (psd-import.md §4).
pub fn new_model(psd_path: &Path, out: &Path, rest: &[&str]) -> R {
    let data = std::fs::read(psd_path)?;
    let psd = Psd::parse(&data)?;
    let name = psd_path
        .file_name()
        .map_or("PSD".into(), |n| n.to_string_lossy().to_string());
    let o = ImportOptions {
        path: psd_path.to_string_lossy().to_string(),
        ..ImportOptions::default()
    };
    let (mut doc, r) = new_model_from_psd(&name, &psd, &o)?;
    println!(
        "{name}: new model, {} parameters, {} art meshes, {} parts",
        doc.model.parameters.len(),
        r.art_meshes.len(),
        r.parts.len()
    );
    for w in r.warnings {
        eprintln!("warning: {w}");
    }
    if out
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        return crate::render::render_model(&doc.model, out, rest);
    }
    doc.save_to(out)?;
    println!("wrote {out:?}");
    Ok(())
}

/// `kf new <out.cmo3> [WxH]`: a blank model (dialogs.md §1).
pub fn new_blank(out: &Path, rest: &[&str]) -> R {
    let (w, h) = match rest {
        [] => (1000, 2000),
        [s] => {
            let (a, b) = s.split_once('x').ok_or("size must be WxH")?;
            (a.parse()?, b.parse()?)
        }
        _ => return Err("usage: kf new <out.cmo3> [WxH]".into()),
    };
    let stem = out
        .file_stem()
        .map_or("Untitled Model".into(), |s| s.to_string_lossy().to_string());
    let mut doc = keyform_formats::psd_import::new_blank_model(&stem, w, h)?;
    doc.save_to(out)?;
    println!("wrote {out:?} ({w}x{h})");
    Ok(())
}
