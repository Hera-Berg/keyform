#![allow(clippy::unwrap_used)]
//! Compares the cleaned atlas pages of a project with reference-exported PNGs.
use keyform_formats::cmo3::Cmo3Document;
use keyform_formats::image::RgbaImage;
use keyform_formats::texture_cleanup::{default_cleanup, extend_edge_color};

fn stats(name: &str, a: &RgbaImage, b: &RgbaImage) {
    let (mut diff_px, mut max, mut alpha_diff, mut transparent_rgb_diff) =
        (0usize, 0u8, 0usize, 0usize);
    for (p, q) in a.data.as_chunks::<4>().0.iter().zip(b.data.chunks_exact(4)) {
        if p != q {
            diff_px += 1;
            max = max.max(
                p.iter()
                    .zip(q)
                    .map(|(x, y)| x.abs_diff(*y))
                    .max()
                    .unwrap_or(0),
            );
            if p[3] != q[3] {
                alpha_diff += 1;
            }
            if p[3] == 0 && q[3] == 0 {
                transparent_rgb_diff += 1;
            }
        }
    }
    println!("{name}: {diff_px} px differ (alpha {alpha_diff}, rgb-under-zero-alpha {transparent_rgb_diff}), max channel Δ {max}");
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let doc = Cmo3Document::read(&std::fs::read(&a[0]).unwrap()).unwrap();
    for (i, path) in a[1..].iter().enumerate() {
        let reference = RgbaImage::decode_png(&std::fs::read(path).unwrap()).unwrap();
        let page = RgbaImage::decode_png(doc.model.textures[i].png.as_ref().unwrap()).unwrap();
        stats(&format!("page {i} raw     "), &page, &reference);
        stats(
            &format!("page {i} default "),
            &default_cleanup(&page),
            &reference,
        );
        stats(
            &format!("page {i} extend  "),
            &extend_edge_color(&page),
            &reference,
        );
    }
}
