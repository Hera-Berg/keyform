//! Regression tests against reference-made Phase 5 test files (made in the
//! reference editor 5.3.04, FREE mode, from the hiyori_free sample; local
//! only, `KEYFORM_BB_P5` = their folder):
//! - `off.cmo3`: PartHairFront with offscreen drawing at 50 % opacity;
//! - `bs.cmo3`: blend-shape parameter ParamBsTest moving ArtMesh49;
//! - `nest_ref.can3`: hiyori_m02 nested in hiyori_m01 as a scene track
//!   (work area, moved by (100, 100));
//!
//! each with the reference's image exports.
#![cfg(test)]

use std::path::{Path, PathBuf};

use keyform_formats::image::RgbaImage;

fn dir() -> Option<PathBuf> {
    std::env::var_os("KEYFORM_BB_P5").map(PathBuf::from)
}

/// cmp.py's metric: per pixel the largest premultiplied channel / alpha
/// difference. Returns (mean, fraction > 32).
fn diff(a: &RgbaImage, b: &RgbaImage) -> (f64, f64) {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let (mut sum, mut big) = (0u64, 0u64);
    for (p, q) in a.data.chunks_exact(4).zip(b.data.chunks_exact(4)) {
        let pm = |x: &[u8], i: usize| i32::from(x[i]) * i32::from(x[3]) / 255;
        let mut d = (i32::from(p[3]) - i32::from(q[3])).abs();
        for i in 0..3 {
            d = d.max((pm(p, i) - pm(q, i)).abs());
        }
        sum += d as u64;
        big += u64::from(d > 32);
    }
    let n = (a.data.len() / 4) as f64;
    (sum as f64 / n, big as f64 / n)
}

fn png(p: &Path) -> RgbaImage {
    RgbaImage::decode_png(&std::fs::read(p).unwrap()).unwrap()
}

/// A model rendered like the reference's 25 % image export.
fn render_model(path: &Path, set: &[(&str, f32)]) -> RgbaImage {
    let m = keyform_formats::cmo3::Cmo3Document::read(&std::fs::read(path).unwrap())
        .unwrap()
        .model;
    let mut v: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
    for (id, x) in set {
        let i = m.parameters.iter().position(|p| &p.id == id).unwrap();
        v[i] = *x;
    }
    let f = keyform_eval::Evaluator::new(&m).evaluate(&v);
    let view = keyform_render::View::for_export(&m, 0.25).unwrap();
    keyform_render::CpuRenderer::new(&m)
        .render(
            &f,
            &keyform_render::RenderOptions {
                view,
                background: None,
                supersample: 2,
                msaa: true,
            },
        )
        .unwrap()
}

#[test]
fn offscreen_part_matches_reference() {
    let Some(d) = dir() else { return };
    let m = keyform_formats::cmo3::Cmo3Document::read(&std::fs::read(d.join("off.cmo3")).unwrap())
        .unwrap()
        .model;
    let p = m
        .parts
        .iter()
        .find(|p| p.info.id == "PartHairFront")
        .unwrap();
    assert!(p.offscreen);
    let (mean, big) = diff(
        &render_model(&d.join("off.cmo3"), &[]),
        &png(&d.join("off_ref25.png")),
    );
    assert!(mean < 0.25 && big < 0.0005, "mean {mean}, >32 {big}");
}

#[test]
fn blend_shape_matches_reference() {
    let Some(d) = dir() else { return };
    for (v, f) in [(1.0, "bs_ref_1.png"), (0.5, "bs_ref_05.png")] {
        let (mean, big) = diff(
            &render_model(&d.join("bs.cmo3"), &[("ParamBsTest", v)]),
            &png(&d.join(f)),
        );
        assert!(mean < 0.25 && big < 0.0005, "{f}: mean {mean}, >32 {big}");
    }
}

#[test]
fn nested_scene_matches_reference() {
    let Some(d) = dir() else { return };
    let mut log = crate::log::Log::default();
    let doc =
        crate::document::Document::open_animation(&d.join("nest_ref.can3"), &mut log).unwrap();
    for f in [0u32, 10, 20, 30] {
        let kf = crate::export_media::render_frame(&doc, f64::from(f), 50.0, false).unwrap();
        let (mean, big) = diff(&kf, &png(&d.join(format!("nest_ref/nest_{f:03}.png"))));
        assert!(
            mean < 1.5 && big < 0.005,
            "frame {f}: mean {mean}, >32 {big}"
        );
    }
}
