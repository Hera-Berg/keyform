//! `kf` — Keyform command-line tools.

use std::path::Path;
use std::process::ExitCode;

use keyform_formats::cmo3::Cmo3Document;
use keyform_model::{DeformerKind, Model};

const USAGE: &str = "usage:
  kf info <file.cmo3>             summary of a model project
  kf roundtrip <in.cmo3> <out>    load and save a project unchanged
  kf xml <in.cmo3|can3> <out.xml> extract the document's main.xml
  kf motions <in.can3> <outdir>   export the scenes as motion3.json
  kf set <in.cmo3> <out.cmo3> KIND:ID.FIELD=VALUE ...
                                   edit fields and save (see edit.rs)
  kf moc3info <file.moc3>          summary of a runtime model
  kf export <in.cmo3> <out.moc3|out-dir> [version]
                                   export for runtime: a .moc3 path writes only the
                                   moc3, a folder gets the full file set
  kf diffmoc <a.moc3> <b.moc3>     compare two moc3 files section by section
  kf mocsec <file.moc3> <section>  print one section
  kf automesh <in.cmo3> <out.cmo3> [--preset NAME] [--preview DIR] MESH_ID ...
                                   automatic mesh generation (auto-mesh.md); the preview
                                   draws each new mesh over its model image
  kf psd <in.cmo3> <file.psd> <out.cmo3|out.png> [--register] [--legacy]
                                   PSD import (psd-import.md §6); a .png output renders
                                   the result instead of saving
  kf atlas <in.cmo3> <out.cmo3|out.png> new [WxH] [visible|all|none]
  kf atlas <in.cmo3> <out.cmo3|out.png> layout <page> [--margin M] [--scale S] [--no-rotate] [--images]
                                   texture atlas pages (texture-atlas.md); a .png output
                                   writes the last page image
  kf new <out.cmo3> [WxH]          a blank model (File > New > Model; default 1000x2000)
  kf newanim <model.cmo3> <out.can3>
                                   a new animation file on the model (one 120-frame scene)
  kf newpsd <file.psd> <out.cmo3|out.png>
                                   a new model from a PSD (psd-import.md §4)
  kf render <model> <out.png> [--size N | --scale S] [--ss N] [--bg] [--gpu] [ID=VALUE ...]
                                   render a .cmo3/.moc3 at parameter values
  kf gpudiff <model> <diff.png> [render options]
                                   compare the CPU and GPU renderers
  kf sweep <model> <out.png> <ID> [steps] [--size N]
                                   contact sheet sweeping one parameter min→max
  kf compare <a> <b> [samples]     evaluate two models (.cmo3/.moc3) at the same
                                   parameter values and report differences";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["info", path] => info(Path::new(path)),
        ["roundtrip", input, output] => roundtrip(Path::new(input), Path::new(output)),
        ["set", input, output, edits @ ..] => {
            edit::set_cmd(Path::new(input), Path::new(output), edits)
        }
        ["xml", input, output] => xml_dump(Path::new(input), Path::new(output)),
        ["motions", input, outdir] => export_motions(Path::new(input), Path::new(outdir)),
        ["moc3info", path] => load_any(Path::new(path)).map(|m| print_summary(&m)),
        ["export", input, output] => export(Path::new(input), Path::new(output), None),
        ["export", input, output, v] => match v.parse() {
            Ok(v) => export(Path::new(input), Path::new(output), Some(v)),
            Err(_) => Err("version must be 1…6".into()),
        },
        ["diffmoc", a, b] => diffmoc(Path::new(a), Path::new(b)),
        ["mocsec", a, name] => mocsec(Path::new(a), name),
        ["automesh", input, output, rest @ ..] => {
            automesh::run(Path::new(input), Path::new(output), rest)
        }
        ["atlas", input, output, rest @ ..] => {
            atlas::run(Path::new(input), Path::new(output), rest)
        }
        ["new", out, rest @ ..] => psd::new_blank(Path::new(out), rest),
        ["newanim", model, out] => new_animation(Path::new(model), Path::new(out)),
        ["newpsd", psd, out, rest @ ..] => psd::new_model(Path::new(psd), Path::new(out), rest),
        ["psd", input, psd, out, rest @ ..] => {
            psd::run(Path::new(input), Path::new(psd), Path::new(out), rest)
        }
        ["render", model, out, rest @ ..] => {
            render::render_cmd(Path::new(model), Path::new(out), rest)
        }
        ["gpudiff", model, out, rest @ ..] => {
            render::gpudiff_cmd(Path::new(model), Path::new(out), rest)
        }
        ["sweep", model, out, id, rest @ ..] => {
            render::sweep_cmd(Path::new(model), Path::new(out), id, rest)
        }
        ["compare", a, b] => compare::run(Path::new(a), Path::new(b), 50),
        ["compare", a, b, n] => match n.parse() {
            Ok(n) => compare::run(Path::new(a), Path::new(b), n),
            Err(_) => Err("samples must be a number".into()),
        },
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

mod atlas;
mod automesh;
mod compare;
mod edit;
mod psd;
mod render;

type R = Result<(), Box<dyn std::error::Error>>;

/// Loads a `.cmo3`, `.moc3` or `.model3.json` (with its textures) as a model.
fn load_any(path: &Path) -> Result<Model, Box<dyn std::error::Error>> {
    if path.to_string_lossy().ends_with(".model3.json") {
        let m3 = keyform_formats::runtime_json::Model3::parse(&std::fs::read_to_string(path)?)?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut m = load_any(&dir.join(&m3.moc))?;
        for (i, t) in m3.textures.iter().enumerate() {
            let png = std::fs::read(dir.join(t))?;
            let (w, h) = keyform_formats::image::RgbaImage::decode_png(&png)
                .map(|i| (i.width, i.height))
                .unwrap_or((0, 0));
            m.textures.push(std::sync::Arc::new(keyform_model::Texture {
                sampling: Default::default(),
                guid: String::new(),
                name: format!("texture_{i:02}"),
                width: w,
                height: h,
                png: Some(png),
                locked: true,
                entries: Vec::new(),
            }));
        }
        return Ok(m);
    }
    let data = std::fs::read(path)?;
    if data.starts_with(b"MOC3") {
        let raw = keyform_formats::moc3::Moc3Raw::read(&data)?;
        Ok(keyform_formats::moc3::moc3_to_model(&raw)?)
    } else {
        Ok(Cmo3Document::read(&data)?.model)
    }
}

fn load(path: &Path) -> Result<Cmo3Document, Box<dyn std::error::Error>> {
    let data = std::fs::read(path)?;
    Ok(Cmo3Document::read(&data)?)
}

fn info(path: &Path) -> R {
    let doc = load(path)?;
    print_summary(&doc.model);
    for w in &doc.warnings {
        println!("warning: {w}");
    }
    Ok(())
}

fn print_summary(m: &Model) {
    let warps = m
        .deformers
        .iter()
        .filter(|d| matches!(d.kind, DeformerKind::Warp(_)))
        .count();
    println!(
        "model {:?} canvas {}×{} target {}",
        m.name, m.canvas.width, m.canvas.height, m.target_version
    );
    println!(
        "  {} parameters, {} groups, {} parts, {} deformers ({} warp, {} rotation), {} art meshes, {} glues, {} textures",
        m.parameters.len(),
        m.parameter_groups.len(),
        m.parts.len(),
        m.deformers.len(),
        warps,
        m.deformers.len() - warps,
        m.art_meshes.len(),
        m.glues.len(),
        m.textures.len(),
    );
    let verts: usize = m.art_meshes.iter().map(|a| a.vertex_count()).sum();
    let tris: usize = m.art_meshes.iter().map(|a| a.indices.len() / 3).sum();
    println!(
        "  {verts} vertices, {tris} triangles; export {:?}",
        m.export
    );
    for t in &m.textures {
        println!(
            "  texture {:?} {}×{} png {} bytes",
            t.name,
            t.width,
            t.height,
            t.png.as_ref().map_or(0, Vec::len)
        );
    }
}

fn roundtrip(input: &Path, output: &Path) -> R {
    let doc = load(input)?;
    std::fs::write(output, doc.write()?)?;
    let back = Cmo3Document::read(&std::fs::read(output)?)?;
    if back.model != doc.model {
        return Err("re-read model differs".into());
    }
    println!("wrote {output:?}; model identical after re-read");
    Ok(())
}

/// Prints the mask check, which the reference shows on export (dialogs.md §21).
fn print_mask_issues(m: &Model) {
    let issues = keyform_edit::mask_issues(m);
    for list in issues.describe(m) {
        println!("mask issue: {}", list.message);
        for (item, lines) in list.items {
            println!("  {item}");
            for l in lines {
                println!("    {l}");
            }
        }
    }
}

fn export(input: &Path, output: &Path, version: Option<u8>) -> R {
    use keyform_formats::moc3::{export_moc3, ExportOptions};
    let doc = load(input)?;
    print_mask_issues(&doc.model);
    let mut opts = ExportOptions::for_model(&doc.model);
    if let Some(v) = version {
        opts.version = v;
    }
    if !output.to_string_lossy().ends_with(".moc3") {
        use keyform_formats::runtime_json::{export_runtime, RuntimeExportOptions};
        std::fs::create_dir_all(output)?;
        let name = input
            .file_stem()
            .map_or("model".into(), |s| s.to_string_lossy().into_owned());
        let ro = RuntimeExportOptions {
            moc3: opts,
            export_physics: true,
            export_motion_sync: true,
            extend_edge_color: doc.model.export.extend_edge_color,
            export_display_info: true,
            export_user_data: true,
        };
        let (files, rep) = export_runtime(&doc.model, &ro, output, &name)?;
        for w in &rep.warnings {
            println!("warning: {w}");
        }
        for f in files {
            println!("wrote {f:?}");
        }
        return Ok(());
    }
    let (raw, rep) = export_moc3(&doc.model, &opts)?;
    std::fs::write(output, raw.write()?)?;
    for w in &rep.warnings {
        println!("warning: {w}");
    }
    println!(
        "wrote {output:?}: moc3 v{}, {} parts, {} deformers, {} art meshes, {} parameters, {} glues, textures {:?}",
        raw.version,
        rep.parts.len(),
        rep.deformers.len(),
        rep.art_meshes.len(),
        rep.parameters.len(),
        rep.glues.len(),
        rep.textures
    );
    Ok(())
}

/// Prints one section of a moc3 file (debug aid).
fn mocsec(a: &Path, name: &str) -> R {
    use keyform_formats::moc3::{Moc3Raw, SectionData};
    let r = Moc3Raw::read(&std::fs::read(a)?)?;
    match r.sections.get(name) {
        Some(SectionData::I32(v)) => println!("{v:?}"),
        Some(SectionData::F32(v)) => println!("{v:?}"),
        Some(SectionData::I16(v)) => println!("{v:?}"),
        Some(SectionData::U8(v)) => println!("{v:?}"),
        Some(SectionData::Ids(v)) => println!("{v:?}"),
        _ => println!("no such section"),
    }
    Ok(())
}

fn diffmoc(a: &Path, b: &Path) -> R {
    use keyform_formats::moc3::{layout::SECTIONS, Moc3Raw, SectionData};
    let ra = Moc3Raw::read(&std::fs::read(a)?)?;
    let rb = Moc3Raw::read(&std::fs::read(b)?)?;
    if ra.version != rb.version {
        println!("version {} vs {}", ra.version, rb.version);
    }
    if ra.counts != rb.counts {
        println!("counts differ:\n  {:?}\n  {:?}", ra.counts, rb.counts);
    }
    if ra.canvas != rb.canvas {
        println!("canvas {:?} vs {:?}", ra.canvas, rb.canvas);
    }
    let mut same = 0;
    for s in SECTIONS
        .iter()
        .filter(|s| s.since <= ra.version.min(rb.version))
    {
        let (x, y) = (ra.sections.get(s.name), rb.sections.get(s.name));
        if x == y {
            same += 1;
            continue;
        }
        let show = |d: Option<&SectionData>, i: usize| -> String {
            match d {
                Some(SectionData::Ids(v)) => v.get(i).cloned().unwrap_or_default(),
                Some(SectionData::I32(v)) => v.get(i).map(|x| x.to_string()).unwrap_or_default(),
                Some(SectionData::F32(v)) => v.get(i).map(|x| format!("{x:e}")).unwrap_or_default(),
                Some(SectionData::I16(v)) => v.get(i).map(|x| x.to_string()).unwrap_or_default(),
                Some(SectionData::U8(v)) => v.get(i).map(|x| x.to_string()).unwrap_or_default(),
                _ => String::new(),
            }
        };
        let (la, lb) = (x.map_or(0, |d| d.len()), y.map_or(0, |d| d.len()));
        let mut diffs = 0;
        let mut first = None;
        let mut max_f = 0.0f32;
        for i in 0..la.max(lb) {
            let (p, q) = (show(x, i), show(y, i));
            if p != q {
                diffs += 1;
                first.get_or_insert((i, p.clone(), q.clone()));
                if let (Ok(p), Ok(q)) = (p.parse::<f32>(), q.parse::<f32>()) {
                    max_f = max_f.max((p - q).abs());
                }
            }
        }
        let (i, p, q) = first.unwrap_or_default();
        println!(
            "{:<44} len {la}/{lb}, {diffs} differ (first #{i}: {p} vs {q}){}",
            s.name,
            if max_f > 0.0 {
                format!(", max |Δ| {max_f:e}")
            } else {
                String::new()
            }
        );
    }
    println!("{same} sections identical");
    Ok(())
}

/// Writes the main XML of any CAFF document (`.cmo3`, `.can3`).
fn xml_dump(input: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let data = std::fs::read(input)?;
    let archive = keyform_formats::caff::CaffArchive::read(&data)?;
    let entry = archive.main_xml().ok_or("no main_xml entry")?;
    let doc = keyform_formats::xml::XmlDocument::parse(&entry.data)?;
    std::fs::write(output, doc.write())?;
    Ok(())
}

/// Exports every scene of a `.can3` marked for motion export as
/// `<scene>.motion3.json` (its first model track). spec: motion3-json.md §2
fn export_motions(input: &Path, outdir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use keyform_anim::motion_export::{export_motion, ExportOptions};
    use keyform_model::animation::TrackKind;
    let doc = keyform_formats::can3::Can3Document::read(&std::fs::read(input)?)?;
    for w in &doc.warnings {
        eprintln!("warning: {w}");
    }
    std::fs::create_dir_all(outdir)?;
    let o = ExportOptions {
        legacy_smooth: doc.animation.smooth_legacy,
        ..Default::default()
    };
    for s in doc.animation.scenes.iter().filter(|s| s.export_motion) {
        let Some(ti) = s
            .tracks
            .iter()
            .position(|t| matches!(t.kind, TrackKind::Model(_)))
        else {
            eprintln!("{}: no model track", s.name);
            continue;
        };
        let Some(m) = export_motion(s, ti, &o, &|_| None, &|_| None) else {
            continue;
        };
        let out = outdir.join(format!("{}.motion3.json", s.name));
        std::fs::write(&out, keyform_anim::json::to_string(&m.to_json()))?;
        println!("{}", out.display());
    }
    Ok(())
}

/// `kf newanim`: a new animation file linked to `model` (File > New >
/// Animation), one scene of 120 frames at 30 fps.
fn new_animation(model: &Path, out: &Path) -> R {
    let doc = keyform_formats::cmo3::Cmo3Document::read(
        &std::fs::read(model).map_err(|e| format!("{}: {e}", model.display()))?,
    )
    .map_err(|e| format!("{}: {e}", model.display()))?;
    let stem = out
        .file_stem()
        .map_or_else(|| "Animation".into(), |s| s.to_string_lossy().into_owned());
    let abs = std::fs::canonicalize(model).unwrap_or_else(|_| model.to_path_buf());
    let mut a = keyform_formats::can3::Can3Document::new_animation(
        &stem,
        &out.to_string_lossy(),
        &doc.model,
        &abs.to_string_lossy(),
        "Scene1",
        120,
        30.0,
    )
    .map_err(|e| e.to_string())?;
    let bytes = a.save_bytes().map_err(|e| e.to_string())?;
    std::fs::write(out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("wrote {}", out.display());
    Ok(())
}
