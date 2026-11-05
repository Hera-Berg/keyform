//! `kf atlas`: texture atlas pages (spec: 70-advanced/texture-atlas.md).

use std::path::Path;

use keyform_formats::atlas::LayoutSettings;
use keyform_formats::atlas_edit::{auto_layout, create_atlas, default_name, default_size, Insert};
use keyform_formats::cmo3::Cmo3Document;

type R = Result<(), Box<dyn std::error::Error>>;

/// `kf atlas <in.cmo3> <out.cmo3 | out.png> new [WxH] [visible|all|none]`
/// or `... layout <texture index> [--margin M] [--no-rotate] [--scale S] [--images]`
pub fn run(input: &Path, out: &Path, rest: &[&str]) -> R {
    let mut doc = Cmo3Document::read(&std::fs::read(input)?)?;
    let m = &mut doc.model;
    match rest {
        ["new", opts @ ..] => {
            let (mut w, mut h) = default_size(m);
            let mut insert = Insert::Visible;
            for o in opts {
                match *o {
                    "visible" => insert = Insert::Visible,
                    "all" => insert = Insert::All,
                    "none" => insert = Insert::None,
                    s => {
                        let (a, b) = s.split_once('x').ok_or(format!("bad option {s:?}"))?;
                        (w, h) = (a.parse()?, b.parse()?);
                    }
                }
            }
            let name = default_name(m);
            let t = create_atlas(m, &name, w, h, insert)?;
            let tex = &m.textures[t];
            println!("{name}: page {t}, {w}x{h}, {} images", tex.entries.len());
        }
        ["layout", t, opts @ ..] => {
            let t: usize = t.parse()?;
            let mut s = LayoutSettings::default();
            let mut mesh_bounds = true;
            let mut it = opts.iter();
            while let Some(o) = it.next() {
                match *o {
                    "--margin" => s.margin = it.next().ok_or("--margin needs a value")?.parse()?,
                    "--scale" => s.scale = it.next().ok_or("--scale needs a value")?.parse()?,
                    "--no-rotate" => s.rotate = false,
                    "--images" => mesh_bounds = false,
                    x => return Err(format!("unknown option {x:?}").into()),
                }
            }
            let missed = auto_layout(m, t, &s, mesh_bounds)?;
            println!("auto layout of page {t}: {missed} images did not fit");
        }
        _ => return Err("expected `new` or `layout`".into()),
    }
    if out
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        let t = doc.model.textures.len() - 1;
        let png = doc.model.textures[t].png.clone().ok_or("no page image")?;
        std::fs::write(out, png)?;
    } else {
        doc.save_to(out)?;
    }
    println!("wrote {out:?}");
    Ok(())
}
