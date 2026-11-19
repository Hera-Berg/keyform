#![allow(clippy::unwrap_used)]
//! Art paths draw as vertex-coloured strips in draw order, the same on the
//! CPU and the GPU. spec: 70-advanced/art-path.md §3

use keyform_eval::Evaluator;
use keyform_model::*;
use keyform_render::{CpuRenderer, RenderOptions, RgbaImage, View};

fn model() -> Model {
    let mut m = Model {
        canvas: Canvas {
            width: 64,
            height: 64,
        },
        ..Model::default()
    };
    let pt = |x: f32, y: f32, c: Color| ArtPathPoint {
        position: [x, y],
        width: 8.0,
        opacity: 1.0,
        color: c,
        corner: false,
    };
    let form = ArtPathForm {
        points: vec![
            pt(8.0, 32.0, [1.0, 0.0, 0.0, 1.0]),
            pt(32.0, 32.0, [1.0, 0.0, 0.0, 1.0]),
            pt(56.0, 32.0, [0.0, 0.0, 1.0, 1.0]),
        ],
        draw_order: 500.0,
        opacity: 1.0,
        multiply: [1.0; 4],
        screen: [0.0, 0.0, 0.0, 1.0],
    };
    m.art_paths.push(std::sync::Arc::new(ArtPath {
        info: ObjectInfo::new("ArtPath"),
        target: None,
        grid: KeyformGrid::single(form),
        blend_shapes: BlendShapes::default(),
        div_num: 8,
        algorithm: LineAlgorithm::Lcns,
        alignment: LineAlignment::Center,
        closed: false,
        clip: Vec::new(),
        invert_clip: false,
        brush: String::new(),
        user_data: String::new(),
    }));
    m
}

fn opts() -> RenderOptions {
    RenderOptions {
        view: View {
            width: 64,
            height: 64,
            origin: [0.0, 0.0],
            scale: 1.0,
            rotation: 0.0,
            mirror: false,
        },
        background: None,
        supersample: 1,
        msaa: false,
    }
}

fn px(img: &RgbaImage, x: usize, y: usize) -> [u8; 4] {
    let i = 4 * (y * 64 + x);
    [
        img.data[i],
        img.data[i + 1],
        img.data[i + 2],
        img.data[i + 3],
    ]
}

#[test]
fn stroke_draws_on_cpu_and_gpu() {
    let m = model();
    let f = Evaluator::new(&m).evaluate(&[]);
    assert!(f.art_paths[0].is_drawn());
    let c = CpuRenderer::new(&m).render(&f, &opts()).unwrap();
    // Inside the 8-px line: red at the start, blue at the end; empty off it.
    assert_eq!(px(&c, 12, 32)[3], 255);
    assert!(px(&c, 12, 32)[0] > 200 && px(&c, 12, 32)[2] < 30);
    assert!(px(&c, 54, 33)[2] > 200);
    assert_eq!(px(&c, 30, 20)[3], 0);
    assert_eq!(px(&c, 30, 37)[3], 0);
    let Ok(gpu) = keyform_render::gpu::GpuRenderer::new(&m) else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let g = gpu.render(&m, &f, &opts()).unwrap();
    for (x, y) in [(12, 32), (32, 31), (54, 33), (30, 20), (44, 32)] {
        let (a, b) = (px(&g, x, y), px(&c, x, y));
        for k in 0..4 {
            assert!(
                (i32::from(a[k]) - i32::from(b[k])).abs() <= 3,
                "({x},{y}): {a:?} vs {b:?}"
            );
        }
    }
}

/// An alias draws the path again with its own multiply colour, above the
/// original by draw order. spec: 70-advanced/alias.md
#[test]
fn alias_redraws_with_own_appearance() {
    let mut m = model();
    m.aliases.push(std::sync::Arc::new(Alias {
        info: ObjectInfo::new("Alias"),
        target: None,
        grid: KeyformGrid::single(AliasForm {
            draw_order: 600.0,
            opacity: 1.0,
            multiply: [0.0, 1.0, 1.0, 1.0],
            screen: [0.0, 0.0, 0.0, 1.0],
        }),
        blend_shapes: BlendShapes::default(),
        reference: AliasRef::ArtPath(0),
        offscreen: false,
        clip: Vec::new(),
        invert_clip: false,
        color_composition: ColorComposition::Normal,
        alpha_composition: AlphaComposition::Over,
    }));
    let f = Evaluator::new(&m).evaluate(&[]);
    use keyform_eval::draw_order::DrawItem;
    assert_eq!(
        f.items,
        vec![DrawItem::ArtPath(0), DrawItem::AliasArtPath(0, 0)]
    );
    let c = CpuRenderer::new(&m).render(&f, &opts()).unwrap();
    assert_eq!(px(&c, 12, 32), [0, 0, 0, 255], "red × (0, 1, 1) on top");
    // A loop of aliases draws nothing.
    let mut m2 = m.clone();
    std::sync::Arc::make_mut(&mut m2.aliases[0]).reference = AliasRef::Alias(0);
    let f2 = Evaluator::new(&m2).evaluate(&[]);
    assert_eq!(f2.items, vec![DrawItem::ArtPath(0)]);
    let Ok(gpu) = keyform_render::gpu::GpuRenderer::new(&m) else {
        return;
    };
    let g = gpu.render(&m, &f, &opts()).unwrap();
    assert_eq!(px(&g, 12, 32), [0, 0, 0, 255]);
}

/// The clip list masks the stroke like an art mesh's (Q-0046).
#[test]
fn clip_list_masks_the_stroke() {
    let mut m = model();
    let mut img = RgbaImage::new(4, 4);
    img.data.fill(255);
    m.textures.push(std::sync::Arc::new(Texture {
        sampling: Default::default(),
        guid: String::new(),
        name: "t".into(),
        width: 4,
        height: 4,
        png: Some(img.encode_png().unwrap()),
        locked: true,
        entries: Vec::new(),
    }));
    // Mask: the left half of the canvas.
    let positions = vec![0.0, 0.0, 32.0, 0.0, 32.0, 64.0, 0.0, 64.0];
    m.art_meshes.push(std::sync::Arc::new(ArtMesh {
        clip_missing: false,
        info: ObjectInfo::new("Mask"),
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
            draw_order: 100.0,
            opacity: 0.0,
            multiply: [1.0; 4],
            screen: [0.0, 0.0, 0.0, 1.0],
        }),
        blend_shapes: BlendShapes::default(),
        paths: Vec::new(),
    }));
    std::sync::Arc::make_mut(&mut m.art_paths[0]).clip = vec![0];
    let f = Evaluator::new(&m).evaluate(&[]);
    let c = CpuRenderer::new(&m).render(&f, &opts()).unwrap();
    assert_eq!(px(&c, 12, 32)[3], 255, "inside the mask");
    assert_eq!(px(&c, 50, 32)[3], 0, "outside the mask");
    let Ok(gpu) = keyform_render::gpu::GpuRenderer::new(&m) else {
        return;
    };
    let g = gpu.render(&m, &f, &opts()).unwrap();
    assert!(px(&g, 12, 32)[3] > 250);
    assert_eq!(px(&g, 50, 32)[3], 0);
}
