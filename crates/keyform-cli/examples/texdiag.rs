#![allow(clippy::unwrap_used)]
//! Diagnoses differences between Keyform's default cleanup of an atlas page and a reference PNG.
use std::collections::BTreeMap;

use keyform_formats::cmo3::Cmo3Document;
use keyform_formats::image::RgbaImage;
use keyform_formats::texture_cleanup::default_cleanup;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let doc = Cmo3Document::read(&std::fs::read(&a[0]).unwrap()).unwrap();
    let page: usize = a[2].parse().unwrap();
    let r = RgbaImage::decode_png(&std::fs::read(&a[1]).unwrap()).unwrap();
    let s = RgbaImage::decode_png(doc.model.textures[page].png.as_ref().unwrap()).unwrap();
    let o = default_cleanup(&s);
    let w = s.width as usize;
    let mut alpha_cases: BTreeMap<(u8, u8, u8), usize> = BTreeMap::new();
    let mut rgb_cases: BTreeMap<String, usize> = BTreeMap::new();
    let mut examples = Vec::new();
    let mut rgb_examples = Vec::new();
    for i in 0..(s.width * s.height) as usize {
        let (sp, op, rp) = (
            &s.data[4 * i..4 * i + 4],
            &o.data[4 * i..4 * i + 4],
            &r.data[4 * i..4 * i + 4],
        );
        if op == rp {
            continue;
        }
        if op[3] != rp[3] {
            *alpha_cases
                .entry((sp[3].min(9), op[3].min(9), rp[3].min(9)))
                .or_default() += 1;
            if examples.len() < 3 {
                examples.push(i);
            }
        } else {
            let big = op
                .iter()
                .zip(rp)
                .take(3)
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap()
                >= 9;
            if big && sp[3] == 0 && rgb_examples.len() < 2 && (i % w) > 3 && (i / w) > 3 {
                rgb_examples.push(i);
            }
            let k = format!(
                "a={} max|Δrgb|={}",
                if sp[3] == 0 {
                    "0".into()
                } else if sp[3] < 64 {
                    "<64".to_string()
                } else {
                    ">=64".into()
                },
                op.iter()
                    .zip(rp)
                    .take(3)
                    .map(|(x, y)| x.abs_diff(*y))
                    .max()
                    .unwrap()
                    .min(9)
            );
            *rgb_cases.entry(k).or_default() += 1;
        }
    }
    println!("alpha diffs (src a, ours a, ref a; clipped at 9): {alpha_cases:?}");
    println!("rgb-only diffs: {rgb_cases:?}");
    for i in rgb_examples {
        let (x, y) = (i % w, i / w);
        let px = |img: &RgbaImage, xx: usize, yy: usize| {
            let j = 4 * (yy * w + xx);
            format!(
                "{:02x}{:02x}{:02x}/{:3}",
                img.data[j],
                img.data[j + 1],
                img.data[j + 2],
                img.data[j + 3]
            )
        };
        println!(
            "rgb pixel ({x},{y}): ours {} ref {}",
            px(&o, x, y),
            px(&r, x, y)
        );
        println!("  source 5x5 (rrggbb/alpha):");
        for yy in y - 2..=y + 2 {
            println!(
                "    {}",
                (x - 2..=x + 2)
                    .map(|xx| px(&s, xx, yy))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
    for i in examples {
        let (x, y) = (i % w, i / w);
        println!("pixel ({x},{y}) 5x5 src alpha / ref alpha:");
        for yy in y.saturating_sub(2)..=y + 2 {
            let row = |img: &RgbaImage| {
                (x.saturating_sub(2)..=x + 2)
                    .map(|xx| format!("{:3}", img.data[4 * (yy * w + xx) + 3]))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            println!("  {}   |   {}", row(&s), row(&r));
        }
    }
}
