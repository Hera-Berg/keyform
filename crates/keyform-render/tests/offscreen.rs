#![allow(clippy::unwrap_used)]
//! Offscreen parts: the subtree is drawn into its own buffer and composited
//! once with the part's opacity, so overlapping meshes inside it don't
//! show through each other. spec: 40-rendering/offscreen.md

use keyform_eval::Evaluator;
use keyform_model::*;
use keyform_render::{CpuRenderer, RenderOptions, RgbaImage, View};

fn white_png() -> Vec<u8> {
    let mut img = RgbaImage::new(4, 4);
    img.data.fill(255);
    img.encode_png().unwrap()
}

fn quad(id: &str, x: f32, y: f32, s: f32, part: usize) -> ArtMesh {
    let positions = vec![x, y, x + s, y, x + s, y + s, x, y + s];
    let mut info = ObjectInfo::new(id);
    info.parent_part = Some(part);
    ArtMesh {
        clip_missing: false,
        info,
        target: None,
        positions: positions.clone(),
        uvs: vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: Some(0),
        clip: vec![],
        invert_clip: false,
        culling: false,
        color_composition: ColorComposition::Normal,
        alpha_composition: AlphaComposition::Over,
        user_data: String::new(),
        point_uids: vec![],
        mesh: EditableMesh::default(),
        model_image: None,
        generator: Default::default(),
        grid: KeyformGrid::single(ArtMeshForm {
            positions,
            draw_order: 500.0,
            opacity: 1.0,
            multiply: [1.0, 1.0, 1.0, 1.0],
            screen: [0.0, 0.0, 0.0, 1.0],
        }),
        blend_shapes: BlendShapes::default(),
        paths: Vec::new(),
    }
}

fn render(offscreen: bool) -> RgbaImage {
    let mut m = Model {
        canvas: Canvas {
            width: 64,
            height: 64,
        },
        ..Model::default()
    };
    m.textures.push(std::sync::Arc::new(Texture {
        sampling: Default::default(),
        guid: String::new(),
        name: "t".into(),
        width: 4,
        height: 4,
        png: Some(white_png()),
        locked: true,
        entries: Vec::new(),
    }));
    let mut part = Part::new("off");
    part.offscreen = offscreen;
    part.grid = KeyformGrid::single(PartForm {
        opacity: 0.5,
        ..Default::default()
    });
    m.parts.push(std::sync::Arc::new(part));
    m.art_meshes
        .push(std::sync::Arc::new(quad("a", 8.0, 8.0, 32.0, 0)));
    m.art_meshes
        .push(std::sync::Arc::new(quad("b", 24.0, 24.0, 32.0, 0)));
    let f = Evaluator::new(&m).evaluate(&[]);
    let view = View {
        width: 64,
        height: 64,
        origin: [0.0, 0.0],
        scale: 1.0,
        rotation: 0.0,
        mirror: false,
    };
    CpuRenderer::new(&m)
        .render(
            &f,
            &RenderOptions {
                view,
                background: None,
                supersample: 1,
                msaa: false,
            },
        )
        .unwrap()
}

#[test]
fn offscreen_part_composites_once() {
    let alpha = |img: &RgbaImage, x: usize, y: usize| img.data[4 * (y * 64 + x) + 3];
    let inline = render(false);
    let off = render(true);
    // Inline: each mesh at 0.5, the overlap shows 0.75.
    assert_eq!(alpha(&inline, 12, 12), 128);
    assert!(alpha(&inline, 30, 30) > 180, "{}", alpha(&inline, 30, 30));
    // Offscreen: the whole part at 0.5, overlap included.
    assert_eq!(alpha(&off, 12, 12), 128);
    assert_eq!(alpha(&off, 30, 30), 128);
    assert_eq!(alpha(&off, 2, 2), 0);
}

/// The GPU renderer composites offscreen parts like the CPU one.
#[cfg(feature = "gpu")]
#[test]
fn offscreen_gpu_matches_cpu() {
    use keyform_render::gpu::GpuRenderer;
    let mut m = Model {
        canvas: Canvas {
            width: 64,
            height: 64,
        },
        ..Model::default()
    };
    m.textures.push(std::sync::Arc::new(Texture {
        sampling: Default::default(),
        guid: String::new(),
        name: "t".into(),
        width: 4,
        height: 4,
        png: Some(white_png()),
        locked: true,
        entries: Vec::new(),
    }));
    let mut part = Part::new("off");
    part.offscreen = true;
    part.grid = KeyformGrid::single(PartForm {
        opacity: 0.5,
        ..Default::default()
    });
    m.parts.push(std::sync::Arc::new(part));
    m.art_meshes
        .push(std::sync::Arc::new(quad("a", 8.0, 8.0, 32.0, 0)));
    m.art_meshes
        .push(std::sync::Arc::new(quad("b", 24.0, 24.0, 32.0, 0)));
    let f = Evaluator::new(&m).evaluate(&[]);
    let view = View {
        width: 64,
        height: 64,
        origin: [0.0, 0.0],
        scale: 1.0,
        rotation: 0.0,
        mirror: false,
    };
    let opts = RenderOptions {
        view,
        background: None,
        supersample: 1,
        msaa: false,
    };
    let Ok(gpu) = GpuRenderer::new(&m) else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let g = gpu.render(&m, &f, &opts).unwrap();
    let c = CpuRenderer::new(&m).render(&f, &opts).unwrap();
    let a = |img: &RgbaImage, x: usize, y: usize| img.data[4 * (y * 64 + x) + 3];
    for (x, y) in [(12, 12), (30, 30), (50, 50), (2, 2)] {
        assert!(
            (i32::from(a(&g, x, y)) - i32::from(a(&c, x, y))).abs() <= 1,
            "({x},{y}): {} vs {}",
            a(&g, x, y),
            a(&c, x, y)
        );
    }
}
