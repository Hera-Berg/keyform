//! Texture page preprocessing applied when exporting for runtime. Both paths
//! work on 8-bit straight RGBA and never change a pixel with alpha ≥ 64.
//!
//! spec: 10-formats/runtime-json.md §1, §1.1–1.2

use crate::image::RgbaImage;

/// Bleed offsets, in the exact order the reference draws them (69 draws,
/// some repeated, never (0, 0)). spec: runtime-json.md §1.2 step 1
#[rustfmt::skip]
const BLEED_OFFSETS: [(i32, i32); 69] = [
    (-3, -3), (-2, -3), (-1, -3), (0, -3), (1, -3), (2, -3), (3, -3), (4, -3), (4, -2), (4, -1),
    (4, 0), (4, 1), (4, 2), (4, 3), (4, 4), (3, 4), (2, 4), (1, 4), (0, 4), (-1, 4), (-2, 4),
    (-3, 4), (-4, 4), (-4, 3), (-4, 2), (-4, 1), (-4, 0), (-4, -1), (-4, -2), (-4, -3),
    (-3, -3), (-2, -3), (-1, -3), (0, -3), (1, -3), (2, -3), (3, -3), (3, -2), (3, -1), (3, 0),
    (3, 1), (3, 2), (3, 3), (2, 3), (1, 3), (0, 3), (-1, 3), (-2, 3), (-3, 3), (-3, 2), (-3, 1),
    (-3, 0), (-3, -1), (-3, -2), (-2, -2), (-1, -2), (0, -2), (1, -2), (2, -2), (2, -1), (2, 0),
    (2, 1), (2, 2), (1, 2), (0, 2), (-1, 2), (-2, 2), (-2, 1), (-2, 0),
];

/// round(a·b/255) as the reference compositor computes it.
fn mul(a: u32, b: u32) -> u32 {
    if a == 0 || b == 0 {
        return 0;
    }
    let inc = a * 65793;
    ((u64::from(b) * u64::from(inc) + (1 << 23)) >> 24) as u32
}

/// v·255/a for v < a, as the reference compositor computes it; 255 otherwise.
fn div(v: u32, a: u32) -> u32 {
    if v >= a {
        return 255;
    }
    let inc = ((255u64 << 24) + u64::from(a / 2)) / u64::from(a);
    ((u64::from(v) * inc + (1 << 23)) >> 24) as u32
}

/// Integer source-over of straight RGBA `s` onto `d`. spec §1.2 step 1
fn source_over(s: &[u8], d: &mut [u8]) {
    let sa = u32::from(s[3]);
    if sa == 0 {
        return;
    }
    if sa == 255 {
        d.copy_from_slice(s);
        return;
    }
    let da = u32::from(d[3]);
    let f = mul(255 - sa, da);
    let a = sa + f;
    for c in 0..3 {
        let mut v = mul(sa, u32::from(s[c])) + mul(f, u32::from(d[c]));
        if a > 0 && a < 255 {
            v = div(v, a);
        }
        d[c] = v.min(255) as u8;
    }
    d[3] = a.min(255) as u8;
}

/// Default cleanup ("color leakage prevention" off): bleed, speck removal,
/// median, low-alpha blend. spec: runtime-json.md §1.2
pub fn default_cleanup(src: &RgbaImage) -> RgbaImage {
    let (w, h) = (src.width as usize, src.height as usize);
    let s = &src.data;

    // 1. Bleed canvas: transparent white, page drawn at 69 offsets.
    let mut b = vec![255u8; w * h * 4];
    b.iter_mut().skip(3).step_by(4).for_each(|a| *a = 0);
    for &(dx, dy) in &BLEED_OFFSETS {
        for y in 0..h {
            let ty = y as i64 + i64::from(dy);
            if ty < 0 || ty >= h as i64 {
                continue;
            }
            for x in 0..w {
                let tx = x as i64 + i64::from(dx);
                if tx < 0 || tx >= w as i64 {
                    continue;
                }
                let si = 4 * (y * w + x);
                let di = 4 * (ty as usize * w + tx as usize);
                source_over(&s[si..si + 4], &mut b[di..di + 4]);
            }
        }
    }

    // 2. Speck removal → P. The reference's neighbour lookup derives the row
    // as index / H (not / W); identical for square pages, reproduced for others.
    let mut p = s.clone();
    for y in 0..h {
        for x in 0..w {
            let i = 4 * (y * w + x);
            let a = s[i + 3];
            let speck = a > 0 && a < 5 && {
                let row = (y * w + x) / h;
                let mut max = 0u8;
                for ny in row.saturating_sub(1)..=(row + 1).min(h - 1) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                        if (nx, ny) != (x, row) {
                            max = max.max(s[4 * (ny * w + nx) + 3]);
                        }
                    }
                }
                max > 50
            };
            if a == 0 || speck {
                p[i..i + 3].copy_from_slice(&b[i..i + 3]);
                p[i + 3] = 0;
            }
        }
    }

    // 3. Median → Q over a 6-pixel window (columns x−1, x) with 9-entry
    // arrays whose last 3 entries carry stale values between pixels.
    let mut q = p.clone();
    let mut arr = [[0u8; 9]; 4]; // R, G, B, A
    if w >= 3 && h >= 3 {
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let mut k = 0;
                let (mut amax, mut amin) = (0u8, 255u8);
                for yy in [y - 1, y, y + 1] {
                    for xx in [x - 1, x] {
                        let i = 4 * (yy * w + xx);
                        for c in 0..4 {
                            arr[c][k] = p[i + c];
                        }
                        amax = amax.max(p[i + 3]);
                        amin = amin.min(p[i + 3]);
                        k += 1;
                    }
                }
                if amax != 0 && amin != 255 {
                    let i = 4 * (y * w + x);
                    for c in 0..4 {
                        arr[c].sort_unstable();
                        q[i + c] = arr[c][4];
                    }
                }
            }
        }
    }

    // 4. Final blend.
    let mut out = p.clone();
    for i in (0..w * h * 4).step_by(4) {
        let a = p[i + 3];
        if a == 0 {
            out[i..i + 3].copy_from_slice(&q[i..i + 3]);
        } else if a < 64 {
            let t = f32::from(a) / 64.0;
            for c in 0..3 {
                let v = t * f32::from(p[i + c]) + (1.0 - t) * f32::from(q[i + c]) + 0.5;
                out[i + c] = v as u8;
            }
        }
    }
    RgbaImage {
        width: src.width,
        height: src.height,
        data: out,
    }
}

/// "Extend edge color" ("Apply color leakage prevention to texture"):
/// colours of solid pixels spread into pixels with alpha ≤ 8 with growing
/// reach; alpha never changes. spec: runtime-json.md §1.1
pub fn extend_edge_color(src: &RgbaImage) -> RgbaImage {
    let (w, h) = (src.width as usize, src.height as usize);
    let mut out = src.clone();
    if w < 2 || h < 2 {
        return out;
    }
    let total = w * h;
    let strips = if total > 1 << 26 {
        total.div_ceil(1 << 26).next_power_of_two()
    } else {
        1
    };
    if h < strips {
        return out;
    }
    for k in 0..strips {
        let (y0, y1) = (k * h / strips, (k + 1) * h / strips);
        extend_strip(&mut out.data[4 * y0 * w..4 * y1 * w], w, y1 - y0);
    }
    out
}

/// One directional scan in an iteration of §1.1 step 3. `idx` yields
/// (pixel index, position along the scan line) in visiting order.
#[allow(clippy::too_many_arguments)]
fn scan(
    idx: impl Iterator<Item = (usize, usize)>,
    px: &mut [u8],
    label: &mut [u32],
    dist: &mut [usize],
    m: u32,
    reach: usize,
    filled: &mut bool,
) {
    let mut source: Option<(usize, usize)> = None;
    for (p, pos) in idx {
        let l = label[p];
        if (1..m).contains(&l) {
            source = Some((p, pos));
        } else if let Some((sp, spos)) = source {
            let d = pos.abs_diff(spos);
            if d <= reach && d < dist[p] {
                dist[p] = d;
                let (a, b) = (4 * sp, 4 * p);
                let rgb = [px[a], px[a + 1], px[a + 2]];
                px[b..b + 3].copy_from_slice(&rgb);
                label[p] = m;
                *filled = true;
            }
        }
    }
}

fn extend_strip(px: &mut [u8], w: usize, h: usize) {
    const T: u8 = 8;
    // 1. Alpha-0 pixels start grey.
    for p in px.chunks_exact_mut(4) {
        if p[3] == 0 {
            p[..3].copy_from_slice(&[128, 128, 128]);
        }
    }
    // 2. Labels: 1 = solid; m = i + 2 for pixels filled in iteration i.
    let mut label: Vec<u32> = px.chunks_exact(4).map(|p| u32::from(p[3] > T)).collect();
    let n = ((w.max(h) as f64).ln() / 1.5f64.ln()).floor() as u32;
    let mut dist = vec![usize::MAX; w * h];
    let mut d = 1.0f64;
    for i in 0..n {
        let reach = d.floor() as usize;
        let m = i + 2;
        dist.iter_mut().for_each(|v| *v = usize::MAX);
        let mut filled = false;
        for y in 0..h {
            scan(
                (0..w).map(|x| (y * w + x, x)),
                px,
                &mut label,
                &mut dist,
                m,
                reach,
                &mut filled,
            );
            scan(
                (0..w).rev().map(|x| (y * w + x, x)),
                px,
                &mut label,
                &mut dist,
                m,
                reach,
                &mut filled,
            );
        }
        for x in 0..w {
            scan(
                (0..h).map(|y| (y * w + x, y)),
                px,
                &mut label,
                &mut dist,
                m,
                reach,
                &mut filled,
            );
            scan(
                (0..h).rev().map(|y| (y * w + x, y)),
                px,
                &mut label,
                &mut dist,
                m,
                reach,
                &mut filled,
            );
        }
        if !filled {
            break;
        }
        d *= 1.5;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_arithmetic_matches_rounded_division() {
        for a in 0..=255u32 {
            for b in 0..=255u32 {
                let m = mul(a, b);
                let exact = f64::from(a * b) / 255.0;
                assert!(
                    (f64::from(m) - exact).abs() <= 0.5 + 1e-9,
                    "mul({a},{b})={m}"
                );
            }
        }
        assert_eq!(div(10, 5), 255);
        assert_eq!(div(0, 7), 0);
        assert_eq!(div(64, 128), 128);
    }

    #[test]
    fn extend_edge_color_keeps_alpha_and_spreads_colour() {
        let mut img = RgbaImage::new(8, 2);
        img.data[0..4].copy_from_slice(&[200, 10, 20, 255]);
        img.data[4 * 3 + 3] = 5; // faint pixel: recoloured, alpha kept
        let out = extend_edge_color(&img);
        for (a, b) in img.data.chunks_exact(4).zip(out.data.chunks_exact(4)) {
            assert_eq!(a[3], b[3]);
        }
        assert_eq!(&out.data[4..7], &[200, 10, 20]);
        assert_eq!(&out.data[12..16], &[200, 10, 20, 5]);
    }

    #[test]
    fn default_cleanup_never_touches_solid_pixels() {
        let mut img = RgbaImage::new(6, 6);
        for (i, p) in img.data.chunks_exact_mut(4).enumerate() {
            let a = if i % 3 == 0 { 255 } else { (i * 11 % 64) as u8 };
            p.copy_from_slice(&[(i * 7) as u8, 100, 50, a]);
        }
        let out = default_cleanup(&img);
        for (a, b) in img.data.chunks_exact(4).zip(out.data.chunks_exact(4)) {
            if a[3] >= 64 {
                assert_eq!(a, b);
            }
        }
    }
}
