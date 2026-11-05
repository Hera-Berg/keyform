//! Texture atlas layout and page rendering: model images placed on
//! power-of-two pages. spec: 70-advanced/texture-atlas.md §3, §5, §6
//!
//! Affines are row-major 2×3 `[m00, m01, m02, m10, m11, m12]`.

use crate::image::RgbaImage;

pub type Affine = [f32; 6];

pub const IDENTITY: Affine = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

pub fn mul(a: Affine, b: Affine) -> Affine {
    [
        a[0] * b[0] + a[1] * b[3],
        a[0] * b[1] + a[1] * b[4],
        a[0] * b[2] + a[1] * b[5] + a[2],
        a[3] * b[0] + a[4] * b[3],
        a[3] * b[1] + a[4] * b[4],
        a[3] * b[2] + a[4] * b[5] + a[5],
    ]
}

pub fn inverse(a: Affine) -> Option<Affine> {
    let det = a[0] * a[4] - a[1] * a[3];
    if det.abs() < 1e-12 {
        return None;
    }
    let (i00, i01, i10, i11) = (a[4] / det, -a[1] / det, -a[3] / det, a[0] / det);
    Some([
        i00,
        i01,
        -(i00 * a[2] + i01 * a[5]),
        i10,
        i11,
        -(i10 * a[2] + i11 * a[5]),
    ])
}

pub fn apply(a: Affine, p: [f32; 2]) -> [f32; 2] {
    [
        a[0] * p[0] + a[1] * p[1] + a[2],
        a[3] * p[0] + a[4] * p[1] + a[5],
    ]
}

fn translate(x: f32, y: f32) -> Affine {
    [1.0, 0.0, x, 0.0, 1.0, y]
}

fn scale(sx: f32, sy: f32) -> Affine {
    [sx, 0.0, 0.0, 0.0, sy, 0.0]
}

/// Grid layout (initial placement): image-local → atlas per image.
/// spec: texture-atlas.md §3
pub fn grid_layout(sizes: &[(u32, u32)], w: u32, h: u32) -> Vec<Affine> {
    let n = sizes.len();
    if n == 0 {
        return Vec::new();
    }
    let k = (n as f64).sqrt().ceil() as usize;
    let (cw, ch) = (w as f32 / k as f32, h as f32 / k as f32);
    sizes
        .iter()
        .enumerate()
        .map(|(i, &(iw, ih))| {
            let (col, row) = ((i % k) as f32, (i / k) as f32);
            let (x0, y0) = (col * cw + 5.0, row * ch + 5.0);
            let (cw2, ch2) = ((cw - 10.0).max(1.0), (ch - 10.0).max(1.0));
            let (iw, ih) = (iw.max(1) as f32, ih.max(1) as f32);
            let s = (cw2 / iw).min(ch2 / ih);
            let (fx, fy) = (x0 + (cw2 - iw * s) / 2.0, y0 + (ch2 - ih * s) / 2.0);
            mul(translate(fx, fy), scale(s, s))
        })
        .collect()
}

/// One item of an auto layout.
#[derive(Clone, Copy, Debug)]
pub struct LayoutItem {
    /// Model image size (the sort key).
    pub size: (u32, u32),
    /// The packed source rectangle in image-local pixels (the image
    /// rectangle, or the bounds of the meshes using it).
    pub src: [f32; 4],
    pub scale: f32,
    /// Keeps its current placement.
    pub locked: bool,
}

/// Auto layout settings. spec: texture-atlas.md §5
#[derive(Clone, Copy, Debug)]
pub struct LayoutSettings {
    /// Gap between images in page pixels (default 3).
    pub margin: f32,
    pub rotate: bool,
    /// 0 = fit the page; > 0 = fixed scale.
    pub scale: f32,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            margin: 3.0,
            rotate: true,
            scale: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct R {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl R {
    fn intersects(&self, o: &R) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

/// Auto Layout: image-local → atlas for each placed item (`None` when it
/// is locked or doesn't fit). spec: texture-atlas.md §5
pub fn auto_layout(
    items: &[LayoutItem],
    w: u32,
    h: u32,
    s: &LayoutSettings,
) -> Vec<Option<Affine>> {
    let (pw, ph) = (w.max(1) as f32, h.max(1) as f32);
    let mut out = vec![None; items.len()];
    if items.is_empty() {
        return out;
    }
    // 1. Sort by descending area × item scale (stable).
    let mut order: Vec<usize> = (0..items.len()).collect();
    let area = |i: usize| {
        let (a, b) = items[i].size;
        f64::from(a) * f64::from(b) * f64::from(items[i].scale)
    };
    order.sort_by(|&a, &b| area(b).total_cmp(&area(a)));
    // 2.–3.
    let (max_w, max_h) = if s.scale > 0.0 {
        (pw / s.scale, ph / s.scale)
    } else {
        (f32::INFINITY, f32::INFINITY)
    };
    let total: f64 = (0..items.len()).map(area).sum();
    let l = 2.0 * total.sqrt();
    let mw = (f64::from(s.margin) * l / f64::from(pw)) as i64 as f32;
    let mh = (f64::from(s.margin) * l / f64::from(ph)) as i64 as f32;
    // 4. Coarse occupancy bitmap, 8 × 8 units per cell.
    let side = (((4.0 * total.sqrt()) as i64) / 8).max(1) as usize;
    let mut bitmap = vec![false; side * side];
    let mark = |bitmap: &mut [bool], r: &R| {
        let (x0, y0) = ((r.x / 8.0) as usize, (r.y / 8.0) as usize);
        let (x1, y1) = (((r.x + r.w) / 8.0) as usize, ((r.y + r.h) / 8.0) as usize);
        for y in y0..=y1.min(side - 1) {
            for x in x0..=x1.min(side - 1) {
                bitmap[y * side + x] = true;
            }
        }
    };
    let occupied = |bitmap: &[bool], x: f32, y: f32| {
        let (cx, cy) = ((x / 8.0) as usize, (y / 8.0) as usize);
        cx < side && cy < side && bitmap[cy * side + cx]
    };
    let d = 8.0f32;
    let mut used = R {
        x: 0.0,
        y: 0.0,
        w: 8.0,
        h: 8.0,
    };
    let mut placed: Vec<(usize, R, bool)> = Vec::new();
    for &i in &order {
        let it = items[i];
        if it.locked {
            continue;
        }
        let (sw, sh) = (it.src[2] * it.scale, it.src[3] * it.scale);
        let mut r = R {
            x: d,
            y: d,
            w: sw + 2.0 * mw,
            h: sh + 2.0 * mh,
        };
        let mut rotated = false;
        let mut done = false;
        // 5a. Fill holes.
        let mut y = d;
        'scan: while y < used.h - r.h {
            let mut x = d;
            while x < used.w - r.w {
                if !occupied(&bitmap, x, y) {
                    let c = R { x, y, ..r };
                    if placed.iter().all(|(_, p, _)| !c.intersects(p)) {
                        r = c;
                        done = true;
                        break 'scan;
                    }
                }
                x += 8.0;
            }
            y += 8.0;
        }
        // 5b. Grow.
        if !done {
            if used.w / used.h > pw / ph {
                if s.rotate && r.w * 1.3 < r.h {
                    (r.w, r.h) = (r.h, r.w);
                    rotated = true;
                }
                r.x = d;
                r.y = used.h;
                if used.h + r.h <= max_h && r.x + r.w <= max_w {
                    used.h += r.h;
                    used.w = used.w.max(r.x + r.w);
                    done = true;
                }
            } else {
                if s.rotate && r.h * 1.3 < r.w {
                    (r.w, r.h) = (r.h, r.w);
                    rotated = true;
                }
                r.x = used.w;
                r.y = d;
                if used.w + r.w <= max_w && r.y + r.h <= max_h {
                    used.w += r.w;
                    used.h = used.h.max(r.y + r.h);
                    done = true;
                }
            }
        }
        if !done {
            continue;
        }
        mark(&mut bitmap, &r);
        used = R {
            x: 0.0,
            y: 0.0,
            w: used.w.max(r.x + r.w),
            h: used.h.max(r.y + r.h),
        };
        placed.push((i, r, rotated));
    }
    // 6.–7.
    used.w += 8.0;
    used.h += 8.0;
    let fin = if s.scale > 0.0 {
        s.scale
    } else if used.w / used.h > pw / ph {
        (pw / used.w).min(1.0)
    } else {
        (ph / used.h).min(1.0)
    };
    // 8. image-local → atlas = Scale(S) · placement.
    for (i, r, rotated) in placed {
        let it = items[i];
        let mut t = mul(translate(mw, mh), translate(r.x, r.y));
        if rotated {
            let rot = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0];
            t = mul(t, mul(translate(it.src[3] * it.scale, 0.0), rot));
        }
        t = mul(t, scale(it.scale, it.scale));
        t = mul(t, translate(-it.src[0], -it.src[1]));
        let a = mul(scale(fin, fin), t);
        if (a[0] * a[4] - a[1] * a[3]).abs() > 1e-9 {
            out[i] = Some(a);
        }
    }
    out
}

/// Box-reduces an image by an integer factor `d`.
fn reduce(img: &RgbaImage, d: u32) -> RgbaImage {
    let (w, h) = (img.width.div_ceil(d), img.height.div_ceil(d));
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            let mut n = 0;
            for yy in y * d..((y + 1) * d).min(img.height) {
                for xx in x * d..((x + 1) * d).min(img.width) {
                    let j = 4 * (yy * img.width + xx) as usize;
                    for c in 0..4 {
                        acc[c] += u32::from(img.data[j + c]);
                    }
                    n += 1;
                }
            }
            let o = 4 * (y * w + x) as usize;
            for c in 0..4 {
                out.data[o + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    out
}

/// Pads with transparent pixels and dilates the colour 3 px into
/// transparent pixels; returns (colour with alpha forced opaque, alpha).
fn prepare(img: &RgbaImage, pad: u32) -> (Vec<[f32; 3]>, Vec<f32>, u32, u32) {
    let (w, h) = (img.width + 2 * pad, img.height + 2 * pad);
    let mut col = vec![[0.0f32; 3]; (w * h) as usize];
    let mut alpha = vec![0.0f32; (w * h) as usize];
    let mut has = vec![false; (w * h) as usize];
    for y in 0..img.height {
        for x in 0..img.width {
            let j = 4 * (y * img.width + x) as usize;
            let o = ((y + pad) * w + x + pad) as usize;
            let a = img.data[j + 3];
            alpha[o] = f32::from(a) / 255.0;
            if a > 0 {
                col[o] = [
                    f32::from(img.data[j]),
                    f32::from(img.data[j + 1]),
                    f32::from(img.data[j + 2]),
                ];
                has[o] = true;
            }
        }
    }
    // Colour bleed: three rings of nearest-neighbour averaging.
    for _ in 0..3 {
        let prev = has.clone();
        let src = col.clone();
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let o = (y * i64::from(w) + x) as usize;
                if prev[o] {
                    continue;
                }
                let (mut acc, mut n) = ([0.0f32; 3], 0.0);
                for (dx, dy) in [
                    (-1, 0),
                    (1, 0),
                    (0, -1),
                    (0, 1),
                    (-1, -1),
                    (1, -1),
                    (-1, 1),
                    (1, 1),
                ] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= i64::from(w) || ny >= i64::from(h) {
                        continue;
                    }
                    let k = (ny * i64::from(w) + nx) as usize;
                    if prev[k] {
                        for c in 0..3 {
                            acc[c] += src[k][c];
                        }
                        n += 1.0;
                    }
                }
                if n > 0.0 {
                    col[o] = acc.map(|v| v / n);
                    has[o] = true;
                }
            }
        }
    }
    (col, alpha, w, h)
}

/// Renders an atlas page: each image drawn with its image-local → atlas
/// transform, source-over in order, in the high-quality mode (power-of-two
/// pre-reduction, 2 px padding, colour bleed, separate alpha resampling).
/// spec: texture-atlas.md §6
/// Images come from an iterator so they can be decoded one at a time.
pub fn render_page(
    images: impl IntoIterator<Item = (RgbaImage, Affine)>,
    w: u32,
    h: u32,
) -> RgbaImage {
    let mut page = vec![[0.0f32; 4]; (w * h) as usize];
    for (img, t) in images {
        let img = &img;
        if img.width == 0 || img.height == 0 {
            continue;
        }
        // Pre-reduce when strongly downscaled.
        let sx = t[0].hypot(t[3]);
        let sy = t[1].hypot(t[4]);
        let smin = sx.min(sy);
        let mut d = 1u32;
        while smin < 0.45 / d as f32 && d < 64 {
            d *= 2;
        }
        let reduced;
        let (src, t) = if d > 1 {
            reduced = reduce(img, d);
            (&reduced, mul(t, scale(d as f32, d as f32)))
        } else {
            (img, t)
        };
        let pad = 2u32;
        let (col, alpha, pw, ph) = prepare(src, pad);
        // Page pixel → padded source pixel.
        let Some(inv) = inverse(mul(t, translate(-(pad as f32), -(pad as f32)))) else {
            continue;
        };
        // The destination box of the transformed source.
        let corners = [
            apply(t, [-(pad as f32), -(pad as f32)]),
            apply(t, [(src.width + pad) as f32, -(pad as f32)]),
            apply(t, [(src.width + pad) as f32, (src.height + pad) as f32]),
            apply(t, [-(pad as f32), (src.height + pad) as f32]),
        ];
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in corners {
            (x0, y0, x1, y1) = (x0.min(c[0]), y0.min(c[1]), x1.max(c[0]), y1.max(c[1]));
        }
        let (x0, y0) = (x0.floor().max(0.0) as u32, y0.floor().max(0.0) as u32);
        let (x1, y1) = (
            (x1.ceil().max(0.0) as u32).min(w),
            (y1.ceil().max(0.0) as u32).min(h),
        );
        let sample = |px: f32, py: f32| -> ([f32; 3], f32) {
            // Bilinear at pixel centres.
            let (fx, fy) = (px - 0.5, py - 0.5);
            let (ix, iy) = (fx.floor(), fy.floor());
            let (u, v) = (fx - ix, fy - iy);
            let get = |x: f32, y: f32| -> ([f32; 3], f32) {
                if x < 0.0 || y < 0.0 || x >= pw as f32 || y >= ph as f32 {
                    return ([0.0; 3], 0.0);
                }
                let k = (y as u32 * pw + x as u32) as usize;
                (col[k], alpha[k])
            };
            let (c00, a00) = get(ix, iy);
            let (c10, a10) = get(ix + 1.0, iy);
            let (c01, a01) = get(ix, iy + 1.0);
            let (c11, a11) = get(ix + 1.0, iy + 1.0);
            let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
            let mut c = [0.0; 3];
            for k in 0..3 {
                c[k] = lerp(lerp(c00[k], c10[k], u), lerp(c01[k], c11[k], u), v);
            }
            (c, lerp(lerp(a00, a10, u), lerp(a01, a11, u), v))
        };
        for y in y0..y1 {
            for x in x0..x1 {
                let q = apply(inv, [x as f32 + 0.5, y as f32 + 0.5]);
                let (c, a) = sample(q[0], q[1]);
                if a <= 0.0 {
                    continue;
                }
                // Source-over in straight alpha.
                let o = &mut page[(y * w + x) as usize];
                let out_a = a + o[3] * (1.0 - a);
                for k in 0..3 {
                    o[k] = (c[k] * a + o[k] * o[3] * (1.0 - a)) / out_a.max(1e-6);
                }
                o[3] = out_a;
            }
        }
    }
    let mut out = RgbaImage::new(w, h);
    for (p, o) in out.data.chunks_exact_mut(4).zip(&page) {
        p.copy_from_slice(&[
            o[0].round().clamp(0.0, 255.0) as u8,
            o[1].round().clamp(0.0, 255.0) as u8,
            o[2].round().clamp(0.0, 255.0) as u8,
            (o[3] * 255.0).round().clamp(0.0, 255.0) as u8,
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn grid_cells_fit_and_centre() {
        let t = grid_layout(&[(100, 50), (10, 10), (30, 60)], 400, 400);
        // k = 2: cells of 200 × 200, shrunk to 190 × 190.
        assert_eq!(t[0], [1.9, 0.0, 5.0, 0.0, 1.9, 52.5]);
        let p = apply(t[1], [10.0, 10.0]);
        assert!((p[0] - 395.0).abs() < 1e-3 && (p[1] - 195.0).abs() < 1e-3);
        assert!(apply(t[2], [0.0, 0.0])[1] >= 205.0);
    }

    fn item(w: u32, h: u32) -> LayoutItem {
        LayoutItem {
            size: (w, h),
            src: [0.0, 0.0, w as f32, h as f32],
            scale: 1.0,
            locked: false,
        }
    }

    #[test]
    fn auto_layout_packs_without_overlap() {
        let items: Vec<LayoutItem> = [
            (300, 200),
            (120, 400),
            (80, 80),
            (200, 60),
            (50, 50),
            (64, 32),
        ]
        .iter()
        .map(|&(w, h)| item(w, h))
        .collect();
        let t = auto_layout(&items, 512, 512, &LayoutSettings::default());
        let rects: Vec<[f32; 4]> = items
            .iter()
            .zip(&t)
            .map(|(it, a)| {
                let a = a.unwrap();
                let c = [
                    apply(a, [0.0, 0.0]),
                    apply(a, [it.src[2], 0.0]),
                    apply(a, [0.0, it.src[3]]),
                    apply(a, [it.src[2], it.src[3]]),
                ];
                let xs = c.map(|p| p[0]);
                let ys = c.map(|p| p[1]);
                [
                    xs.iter().copied().fold(f32::MAX, f32::min),
                    ys.iter().copied().fold(f32::MAX, f32::min),
                    xs.iter().copied().fold(f32::MIN, f32::max),
                    ys.iter().copied().fold(f32::MIN, f32::max),
                ]
            })
            .collect();
        for (i, a) in rects.iter().enumerate() {
            // Inside the page.
            assert!(
                a[0] >= -0.01 && a[1] >= -0.01 && a[2] <= 512.01 && a[3] <= 512.01,
                "{a:?}"
            );
            for b in &rects[i + 1..] {
                let overlap = a[0] < b[2] - 0.01
                    && b[0] < a[2] - 0.01
                    && a[1] < b[3] - 0.01
                    && b[1] < a[3] - 0.01;
                assert!(!overlap, "{a:?} {b:?}");
            }
        }
        // Small sets are never enlarged.
        let t = auto_layout(&[item(10, 10)], 1024, 1024, &LayoutSettings::default());
        assert_eq!(t[0].unwrap()[0], 1.0);
    }

    #[test]
    fn render_page_places_pixels() {
        let mut img = RgbaImage::new(4, 4);
        for p in img.data.chunks_exact_mut(4) {
            p.copy_from_slice(&[255, 0, 0, 255]);
        }
        let page = render_page([(img, translate(10.0, 20.0))], 32, 32);
        let px =
            |x: u32, y: u32| &page.data[4 * (y * 32 + x) as usize..4 * (y * 32 + x) as usize + 4];
        assert_eq!(px(11, 21), &[255, 0, 0, 255]);
        assert_eq!(px(0, 0)[3], 0);
        // The bled colour keeps edges red, not dark.
        let edge = px(14, 21);
        assert!(
            edge[3] < 255 && (edge[0] == 255 || edge[3] == 0),
            "{edge:?}"
        );
    }
}
