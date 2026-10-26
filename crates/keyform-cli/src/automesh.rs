//! `kf automesh`: automatic mesh generation on named art meshes
//! (spec: 70-advanced/auto-mesh.md), with an optional wireframe preview.

use std::path::Path;

use keyform_edit::automesh::{auto_mesh, presets, AlphaImage};
use keyform_formats::cmo3::Cmo3Document;
use keyform_formats::image::RgbaImage;
use keyform_model::Model;

type R = Result<(), Box<dyn std::error::Error>>;

/// `kf automesh <in.cmo3> <out.cmo3> [--preset NAME] [--preview DIR] ID...`
pub fn run(input: &Path, output: &Path, rest: &[&str]) -> R {
    let mut doc = Cmo3Document::read(&std::fs::read(input)?)?;
    let mut setting = None;
    let mut preview = None;
    let mut ids = Vec::new();
    let mut it = rest.iter();
    while let Some(&a) = it.next() {
        match a {
            "--preset" => {
                let name = it.next().ok_or("--preset needs a name")?;
                let p = presets()
                    .into_iter()
                    .find(|(n, _, _)| {
                        n.eq_ignore_ascii_case(name)
                            || n.to_lowercase().contains(&name.to_lowercase())
                    })
                    .ok_or_else(|| format!("unknown preset {name:?}"))?;
                setting = Some((p.1, p.2));
            }
            "--preview" => preview = Some(Path::new(it.next().ok_or("--preview needs a folder")?)),
            id => ids.push(id),
        }
    }
    if ids.is_empty() {
        return Err(keyform_edit::automesh::NO_SELECTION.into());
    }
    for id in ids {
        let m = &mut doc.model;
        let i = m
            .art_meshes
            .iter()
            .position(|a| a.info.id == id)
            .ok_or_else(|| format!("no art mesh {id:?}"))?;
        let img = model_image(m, i)?;
        let (s, d) = setting.unwrap_or((
            m.art_meshes[i].generator,
            keyform_model::DEFAULT_DENSITY_SCALE,
        ));
        let before = m.art_meshes[i].vertex_count();
        let t = match auto_mesh(m, i, &s, d, &alpha(&img)) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{id}: {e}");
                continue;
            }
        };
        let a = &m.art_meshes[i];
        println!(
            "{id}: {before} → {} vertices, {} triangles{}",
            a.vertex_count(),
            a.indices.len() / 3,
            if t.unresolved > 0 {
                format!(", {} crossing edges left out", t.unresolved)
            } else {
                String::new()
            }
        );
        if let Some(dir) = preview {
            std::fs::create_dir_all(dir)?;
            let out = dir.join(format!("{id}.png"));
            std::fs::write(&out, wireframe(m, i, &img)?.encode_png()?)?;
        }
    }
    doc.save_to(output)?;
    println!("wrote {output:?}");
    Ok(())
}

fn model_image(m: &Model, i: usize) -> Result<RgbaImage, Box<dyn std::error::Error>> {
    let a = &m.art_meshes[i];
    let png = a
        .model_image
        .and_then(|k| m.model_images.get(k))
        .and_then(|mi| mi.png.as_ref())
        .ok_or_else(|| format!("{}: no model image", a.info.id))?;
    Ok(RgbaImage::decode_png(png)?)
}

fn alpha(img: &RgbaImage) -> AlphaImage {
    AlphaImage {
        width: img.width,
        height: img.height,
        alpha: img.data.chunks_exact(4).map(|p| p[3]).collect(),
    }
}

/// The model image (2× size, on grey) with the mesh drawn over it.
fn wireframe(
    m: &Model,
    i: usize,
    img: &RgbaImage,
) -> Result<RgbaImage, Box<dyn std::error::Error>> {
    let a = &m.art_meshes[i];
    let mi = &m.model_images[a.model_image.ok_or("no model image")?];
    let t = mi.local_to_canvas;
    // Canvas → image pixels (inverse of the affine).
    let det = t[0] * t[4] - t[1] * t[3];
    let to_img = |p: [f32; 2]| {
        let (x, y) = (p[0] - t[2], p[1] - t[5]);
        [(t[4] * x - t[1] * y) / det, (-t[3] * x + t[0] * y) / det]
    };
    let k = 2usize;
    let pad = 40usize;
    let (w, h) = (
        img.width as usize * k + 2 * pad,
        img.height as usize * k + 2 * pad,
    );
    let mut out = RgbaImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (x as isize - pad as isize, y as isize - pad as isize);
            let mut c = [200u8, 200, 200, 255];
            if sx >= 0
                && sy >= 0
                && (sx as usize / k) < img.width as usize
                && (sy as usize / k) < img.height as usize
            {
                let j = 4 * ((sy as usize / k) * img.width as usize + sx as usize / k);
                let s = &img.data[j..j + 4];
                let al = u16::from(s[3]);
                for ch in 0..3 {
                    c[ch] = ((u16::from(s[ch]) * al + u16::from(c[ch]) * (255 - al)) / 255) as u8;
                }
            }
            out.data[4 * (y * w + x)..4 * (y * w + x) + 4].copy_from_slice(&c);
        }
    }
    let px = |p: [f32; 2]| {
        let q = to_img(p);
        [q[0] * k as f32 + pad as f32, q[1] * k as f32 + pad as f32]
    };
    let mut plot = |x: i64, y: i64, c: [u8; 4]| {
        if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
            let j = 4 * (y as usize * w + x as usize);
            out.data[j..j + 4].copy_from_slice(&c);
        }
    };
    let pt = |v: u32| px([a.positions[2 * v as usize], a.positions[2 * v as usize + 1]]);
    let mut line = |p: [f32; 2], q: [f32; 2], c: [u8; 4]| {
        let n = ((q[0] - p[0]).abs().max((q[1] - p[1]).abs()).ceil() as usize).max(1);
        for s in 0..=n {
            let u = s as f32 / n as f32;
            plot(
                (p[0] + (q[0] - p[0]) * u).round() as i64,
                (p[1] + (q[1] - p[1]) * u).round() as i64,
                c,
            );
        }
    };
    for tri in a.indices.chunks_exact(3) {
        for (p, q) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            line(pt(p), pt(q), [40, 90, 220, 255]);
        }
    }
    for (e, &pr) in a.mesh.edges.iter().zip(&a.mesh.edge_priority) {
        if pr != keyform_model::EDGE_AUTO_TRIANGULATION {
            line(pt(e[0]), pt(e[1]), [230, 40, 40, 255]);
        }
    }
    Ok(out)
}
