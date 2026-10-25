#![allow(clippy::unwrap_used)]
//! The GPU renderer must match the CPU reference renderer on a synthetic
//! model exercising every colour/alpha composition, clipping (normal and
//! inverted), culling and overlapping semi-transparent layers. Skipped when
//! no GPU adapter is available.
#![cfg(feature = "gpu")]

use keyform_eval::Evaluator;
use keyform_model::*;
use keyform_render::gpu::GpuRenderer;
use keyform_render::{CpuRenderer, RenderOptions, RgbaImage, View};

/// A 64×64 texture: left half a colour gradient, right half semi-transparent.
fn texture_png() -> Vec<u8> {
    let mut img = RgbaImage::new(64, 64);
    for y in 0..64u32 {
        for x in 0..64u32 {
            let i = 4 * (y * 64 + x) as usize;
            let a = if x < 32 { 255 } else { 96 + (y * 2) as u8 };
            img.data[i..i + 4].copy_from_slice(&[
                (x * 4) as u8,
                (y * 4) as u8,
                200 - (x + y) as u8,
                a,
            ]);
        }
    }
    img.encode_png().unwrap()
}

/// An axis-aligned quad mesh at (x, y) of size s, optionally mirrored
/// (reversed winding, so culling hides it).
fn quad(id: &str, x: f32, y: f32, s: f32, draw_order: f32, mirrored: bool) -> ArtMesh {
    let mut positions = vec![x, y, x + s, y, x + s, y + s, x, y + s];
    if mirrored {
        positions = vec![x + s, y, x, y, x, y + s, x + s, y + s];
    }
    ArtMesh {
        info: ObjectInfo::new(id),
        target: None,
        positions: positions.clone(),
        uvs: vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
        // Positive signed area in y-down canvas space (front-facing).
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: Some(0),
        clip: vec![],
        invert_clip: false,
        culling: false,
        color_composition: ColorComposition::Normal,
        alpha_composition: AlphaComposition::Over,
        user_data: String::new(),
        point_uids: vec![],
        grid: KeyformGrid::single(ArtMeshForm {
            positions,
            draw_order,
            opacity: 0.8,
            multiply: [1.0, 0.9, 0.8, 1.0],
            screen: [0.1, 0.0, 0.05, 1.0],
        }),
        blend_shapes: BlendShapes::default(),
    }
}

fn scene() -> Model {
    let mut m = Model {
        canvas: Canvas {
            width: 512,
            height: 512,
        },
        ..Model::default()
    };
    m.textures.push(std::sync::Arc::new(Texture {
        guid: String::new(),
        name: "t".into(),
        width: 64,
        height: 64,
        png: Some(texture_png()),
        locked: true,
    }));
    // Background row strips for every blend test cell.
    m.art_meshes.push(std::sync::Arc::new(quad(
        "bg", 0.0, 0.0, 512.0, 100.0, false,
    )));
    let modes = ColorComposition::NAMES;
    let alphas = [
        AlphaComposition::Over,
        AlphaComposition::Atop,
        AlphaComposition::Out,
        AlphaComposition::Conjoint,
        AlphaComposition::Disjoint,
    ];
    for (i, (name, cc)) in modes.iter().enumerate() {
        let (cx, cy) = ((i % 6) as f32 * 80.0 + 10.0, (i / 6) as f32 * 80.0 + 10.0);
        let mut q = quad(name, cx, cy, 60.0, 500.0, false);
        q.color_composition = *cc;
        q.alpha_composition = alphas[i % alphas.len()];
        m.art_meshes.push(std::sync::Arc::new(q));
        // A second overlapping mesh of the same mode: stresses dst reads.
        let mut q2 = quad(
            &format!("{name}2"),
            cx + 20.0,
            cy + 20.0,
            40.0,
            501.0,
            false,
        );
        q2.color_composition = *cc;
        m.art_meshes.push(std::sync::Arc::new(q2));
    }
    // Clipping: a mask source and two clipped meshes (normal / inverted).
    let src = m.art_meshes.len();
    m.art_meshes.push(std::sync::Arc::new(quad(
        "mask", 30.0, 270.0, 120.0, 400.0, false,
    )));
    let mut c1 = quad("clipped", 10.0, 250.0, 100.0, 600.0, false);
    c1.clip = vec![src];
    m.art_meshes.push(std::sync::Arc::new(c1));
    let mut c2 = quad("inverted", 90.0, 300.0, 100.0, 610.0, false);
    c2.clip = vec![src];
    c2.invert_clip = true;
    m.art_meshes.push(std::sync::Arc::new(c2));
    // Culling: a mirrored quad with culling on is hidden; off, it is drawn.
    let mut culled = quad("culled", 250.0, 300.0, 80.0, 700.0, true);
    culled.culling = true;
    m.art_meshes.push(std::sync::Arc::new(culled));
    m.art_meshes.push(std::sync::Arc::new(quad(
        "double_sided",
        350.0,
        300.0,
        80.0,
        700.0,
        true,
    )));

    let mut root = Part::new("__RootPart__");
    root.children = (0..m.art_meshes.len())
        .rev()
        .map(PartChild::ArtMesh)
        .collect();
    m.parts.push(std::sync::Arc::new(root));
    m.root_part = Some(0);
    for a in &mut m.art_meshes {
        std::sync::Arc::make_mut(a).info.parent_part = Some(0);
    }
    m
}

#[test]
fn gpu_matches_cpu_on_all_compositions() {
    let m = scene();
    let g = match GpuRenderer::new(&m) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipping: no GPU adapter ({e})");
            return;
        }
    };
    let frame = Evaluator::new(&m).evaluate(&[]);
    for (ss, bg, rot) in [
        (1, None, 0.0),
        (2, Some([0.2, 0.3, 0.4, 1.0]), 0.0),
        (1, Some([1.0; 4]), 27.0),
    ] {
        let mut view = View::fit_canvas(&m, 512);
        view.rotation = rot;
        view.mirror = rot != 0.0;
        let opts = RenderOptions {
            view,
            background: bg,
            supersample: ss,
        };
        let cpu = CpuRenderer::new(&m).render(&frame, &opts).unwrap();
        let gpu = g.render(&m, &frame, &opts).unwrap();
        let pm = |c: &[u8]| [0, 1, 2].map(|i| i32::from(c[i]) * i32::from(c[3]) / 255);
        let mut bad = Vec::new();
        for (i, (p, q)) in cpu
            .data
            .chunks_exact(4)
            .zip(gpu.data.chunks_exact(4))
            .enumerate()
        {
            let d = pm(p)
                .iter()
                .zip(pm(q))
                .map(|(x, y)| (x - y).abs())
                .max()
                .unwrap()
                .max((i32::from(p[3]) - i32::from(q[3])).abs());
            if d > 3 {
                bad.push((i % 512, i / 512, p.to_vec(), q.to_vec()));
            }
        }
        eprintln!("ss={ss}: {} pixels differ by > 3", bad.len());
        if let Some(dir) = std::env::var_os("KF_TEST_OUT") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::write(
                dir.join(format!("scene_cpu_ss{ss}.png")),
                cpu.encode_png().unwrap(),
            )
            .unwrap();
            std::fs::write(
                dir.join(format!("scene_gpu_ss{ss}.png")),
                gpu.encode_png().unwrap(),
            )
            .unwrap();
        }
        // Rasterization differences on triangle edges are allowed to touch a
        // few pixels, never whole regions.
        assert!(
            bad.len() < 20,
            "ss={ss}: {} pixels differ by > 3 on {} (first: {:?})",
            bad.len(),
            g.adapter_info,
            &bad[..bad.len().min(5)]
        );
        if rot != 0.0 {
            // Culling under a mirrored view: the GPU must hide the same
            // triangles as the CPU renderer (checked by the pixel comparison
            // above) — and the culled quad must not appear at its position.
            let c = view.to_px([290.0, 340.0]);
            let mut hidden_m = scene();
            hidden_m
                .art_meshes
                .iter_mut()
                .filter(|a| a.info.id == "culled")
                .for_each(|a| std::sync::Arc::make_mut(a).info.visible = false);
            let fr = Evaluator::new(&hidden_m).evaluate(&[]);
            let hidden = GpuRenderer::new(&hidden_m)
                .unwrap()
                .render(&hidden_m, &fr, &opts)
                .unwrap();
            let at = |img: &RgbaImage| {
                img.data[4 * (c[1] as usize * 512 + c[0] as usize)..][..4].to_vec()
            };
            assert_eq!(at(&gpu), at(&hidden), "culled mesh drawn in mirrored view");
            continue;
        }
        // Culling: the culled (mirrored) quad must look exactly like the
        // scene without it; with culling off it must show.
        let at = |img: &RgbaImage, x: usize, y: usize| img.data[4 * (y * 512 + x)..][..4].to_vec();
        let variant = |f: &dyn Fn(&mut ArtMesh)| {
            let mut m2 = scene();
            m2.art_meshes
                .iter_mut()
                .filter(|a| a.info.id == "culled")
                .for_each(|a| f(std::sync::Arc::make_mut(a)));
            let fr = Evaluator::new(&m2).evaluate(&[]);
            GpuRenderer::new(&m2)
                .unwrap()
                .render(&m2, &fr, &opts)
                .unwrap()
        };
        let hidden = variant(&|a| a.info.visible = false);
        let shown = variant(&|a| a.culling = false);
        assert_eq!(
            at(&gpu, 290, 340),
            at(&hidden, 290, 340),
            "culled mesh drawn"
        );
        assert_ne!(
            at(&shown, 290, 340),
            at(&hidden, 290, 340),
            "mirrored mesh missing without culling"
        );
    }
}
