//! Automatic mesh generation: a mesh following the opaque pixels of an art
//! mesh's model image, with an outer ring, an inner ring and a hexagonal
//! interior fill per island. spec: 70-advanced/auto-mesh.md §2–3
//!
//! Exact point placement is Keyform's own (the spec asks for the same
//! parameters, meaning and overall look, not identical meshes). Choices made
//! here where the spec leaves room:
//! - the model image is reduced to the working size by area averaging;
//! - islands are 8-connected solid pixels after bridging 1 px gaps; an
//!   island smaller than min(800 px², a quarter of the largest) is noise;
//! - distance fields are exact Euclidean (separable row/column passes);
//! - the outer ring is the marching-squares iso-line of the island's
//!   outside distance at the outer margin (the largest loop);
//! - inner ring points are found by walking down the signed distance
//!   gradient from each outer point to the inner margin;
//! - the skeleton of a thin island is its distance-field ridge, sampled at
//!   the interior spacing.

use std::collections::HashMap;

use std::sync::Arc;

use keyform_model::{MeshGenerateSetting, Model, EDGE_NORMAL, POINT_AUTO};

use crate::mesh::{set_mesh, MeshGraph, Triangulated};

/// spec: auto-mesh.md §1
pub const NO_SELECTION: &str = "Please call this function with at least one ArtMesh selected.";
pub const GLUED: &str = "You cannot use Automatic Mesh Generation on ArtMesh with Glue enabled.";

/// The engine's "setting scale" σ (§3.2).
const SIGMA: f32 = 0.5;
/// Small islands below this area (working px²) are noise (§3.2).
const IGNORE_AREA: usize = 800;
/// Turning-angle corners (§3.4).
const CORNER_DEG: f32 = 30.0;
const AVERAGING: usize = 5;

/// An alpha raster (row-major, one byte per pixel).
#[derive(Clone, Debug, PartialEq)]
pub struct AlphaImage {
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}

/// Generated mesh in canvas pixels: points and ring edges.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Generated {
    pub points: Vec<[f32; 2]>,
    pub edges: Vec<[u32; 2]>,
    pub islands: usize,
}

/// A preset: label, setting and density scale.
pub type Preset = (&'static str, MeshGenerateSetting, f32);

/// Built-in presets (§2).
pub fn presets() -> [Preset; 3] {
    let p = |interval: i32, out: i32, inner: i32, min: i32, pts: i32| MeshGenerateSetting {
        outer_density: interval,
        inner_density: interval,
        margin: out,
        inner_margin: inner,
        min_margin: min,
        min_bounds_pt: pts,
        threshold_alpha: 0,
    };
    [
        ("Deformation (Little)", p(80, 14, 14, 5, 15), 1.0),
        ("Standard", p(50, 14, 14, 5, 10), 1.0),
        ("Deformation (Heavy)", p(25, 3, 2, 2, 5), 1.0),
    ]
}

/// A float field over the working image.
struct Field {
    w: usize,
    h: usize,
    v: Vec<f32>,
}

impl Field {
    fn at(&self, x: usize, y: usize) -> f32 {
        self.v[y * self.w + x]
    }

    /// Bilinear sample at working coordinates (pixel centres at +0.5).
    fn sample(&self, p: [f32; 2]) -> f32 {
        let x = (p[0] - 0.5).clamp(0.0, (self.w - 1) as f32);
        let y = (p[1] - 0.5).clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (u, t) = (x - x0 as f32, y - y0 as f32);
        let a = self.at(x0, y0) + (self.at(x1, y0) - self.at(x0, y0)) * u;
        let b = self.at(x0, y1) + (self.at(x1, y1) - self.at(x0, y1)) * u;
        a + (b - a) * t
    }

    fn gradient(&self, p: [f32; 2]) -> [f32; 2] {
        let e = 0.75;
        [
            self.sample([p[0] + e, p[1]]) - self.sample([p[0] - e, p[1]]),
            self.sample([p[0], p[1] + e]) - self.sample([p[0], p[1] - e]),
        ]
    }
}

/// 1-D squared distance transform (Felzenszwalb–Huttenlocher): the lower
/// envelope of the parabolas (q − p)² + f(p).
fn dt1(f: &[f64], out: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let cross = |q: usize, p: usize| {
        ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q as f64 - p as f64))
    };
    let mut k = 0;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        let mut s = cross(q, v[k]);
        while s <= z[k] {
            k -= 1;
            s = cross(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let d = q as f64 - v[k] as f64;
        *o = d * d + f[v[k]];
    }
}

/// Euclidean distance to the nearest pixel where `feature` is true.
fn distance(w: usize, h: usize, feature: impl Fn(usize) -> bool) -> Field {
    const BIG: f64 = 1e20;
    let mut g: Vec<f64> = (0..w * h)
        .map(|i| if feature(i) { 0.0 } else { BIG })
        .collect();
    let n = w.max(h);
    let (mut f, mut out, mut v, mut z) = (
        vec![0.0; n],
        vec![0.0; n],
        vec![0usize; n],
        vec![0.0f64; n + 1],
    );
    for x in 0..w {
        for y in 0..h {
            f[y] = g[y * w + x];
        }
        dt1(&f[..h], &mut out[..h], &mut v, &mut z);
        for y in 0..h {
            g[y * w + x] = out[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&g[y * w..y * w + w]);
        dt1(&f[..w], &mut out[..w], &mut v, &mut z);
        g[y * w..y * w + w].copy_from_slice(&out[..w]);
    }
    let v = g.iter().map(|&d| d.min(1e12).sqrt() as f32).collect();
    Field { w, h, v }
}

/// 8-connected labels of `solid` after bridging gaps of up to 1 px.
fn islands(w: usize, h: usize, solid: &[bool]) -> Vec<Vec<usize>> {
    // Bridging: label on the mask dilated by one pixel, keep solid pixels.
    let mut grown = solid.to_vec();
    for y in 0..h {
        for x in 0..w {
            if solid[y * w + x] {
                continue;
            }
            let near = (y.saturating_sub(1)..=(y + 1).min(h - 1))
                .any(|yy| (x.saturating_sub(1)..=(x + 1).min(w - 1)).any(|xx| solid[yy * w + xx]));
            // Only gaps: the pixel must touch solid on two opposite sides.
            let opposite = |dx: isize, dy: isize| {
                let a = (x as isize + dx, y as isize + dy);
                let b = (x as isize - dx, y as isize - dy);
                let ok = |p: (isize, isize)| {
                    p.0 >= 0
                        && p.1 >= 0
                        && (p.0 as usize) < w
                        && (p.1 as usize) < h
                        && solid[p.1 as usize * w + p.0 as usize]
                };
                ok(a) && ok(b)
            };
            grown[y * w + x] =
                near && (opposite(1, 0) || opposite(0, 1) || opposite(1, 1) || opposite(1, -1));
        }
    }
    let mut label = vec![usize::MAX; w * h];
    let mut out: Vec<Vec<usize>> = Vec::new();
    for start in 0..w * h {
        if !grown[start] || label[start] != usize::MAX {
            continue;
        }
        let id = out.len();
        let mut pixels = Vec::new();
        let mut stack = vec![start];
        label[start] = id;
        while let Some(i) = stack.pop() {
            if solid[i] {
                pixels.push(i);
            }
            let (x, y) = (i % w, i / w);
            for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let j = yy * w + xx;
                    if grown[j] && label[j] == usize::MAX {
                        label[j] = id;
                        stack.push(j);
                    }
                }
            }
        }
        out.push(pixels);
    }
    out.retain(|p| !p.is_empty());
    out.sort_by_key(|p| std::cmp::Reverse(p.len()));
    out
}

/// A marching-squares cell edge: (x, y, 0 = horizontal / 1 = vertical).
type EdgeKey = (usize, usize, u8);

/// Closed iso-lines `field = level` (marching squares), in working
/// coordinates. The field must exceed `level` on the grid border.
fn contours(f: &Field, level: f32) -> Vec<Vec<[f32; 2]>> {
    let (w, h) = (f.w, f.h);
    // Edge keys: (x, y, 0) horizontal (x,y)–(x+1,y); (x, y, 1) vertical.
    let point = |k: (usize, usize, u8)| -> [f32; 2] {
        let (x, y, d) = k;
        let (x1, y1) = if d == 0 { (x + 1, y) } else { (x, y + 1) };
        let (a, b) = (f.at(x, y), f.at(x1, y1));
        let t = if (b - a).abs() > 1e-12 {
            ((level - a) / (b - a)).clamp(0.0, 1.0)
        } else {
            0.5
        };
        [
            x as f32 + 0.5 + (x1 as f32 - x as f32) * t,
            y as f32 + 0.5 + (y1 as f32 - y as f32) * t,
        ]
    };
    let mut next: HashMap<(usize, usize, u8), (usize, usize, u8)> = HashMap::new();
    for y in 0..h - 1 {
        for x in 0..w - 1 {
            let inside = |xx: usize, yy: usize| f.at(xx, yy) < level;
            let c = [
                inside(x, y),
                inside(x + 1, y),
                inside(x + 1, y + 1),
                inside(x, y + 1),
            ];
            let (top, right, bottom, left) = ((x, y, 0), (x + 1, y, 1), (x, y + 1, 0), (x, y, 1));
            let case = usize::from(c[0])
                | usize::from(c[1]) << 1
                | usize::from(c[2]) << 2
                | usize::from(c[3]) << 3;
            // Segments oriented with the inside on the left.
            let centre_in =
                (f.at(x, y) + f.at(x + 1, y) + f.at(x + 1, y + 1) + f.at(x, y + 1)) / 4.0 < level;
            let segs: &[(EdgeKey, EdgeKey)] = match case {
                0 | 15 => &[],
                1 => &[(left, top)],
                2 => &[(top, right)],
                3 => &[(left, right)],
                4 => &[(right, bottom)],
                5 if centre_in => &[(left, bottom), (right, top)],
                5 => &[(left, top), (right, bottom)],
                6 => &[(top, bottom)],
                7 => &[(left, bottom)],
                8 => &[(bottom, left)],
                9 => &[(bottom, top)],
                10 if centre_in => &[(top, left), (bottom, right)],
                10 => &[(top, right), (bottom, left)],
                11 => &[(bottom, right)],
                12 => &[(right, left)],
                13 => &[(right, top)],
                14 => &[(top, left)],
                _ => &[],
            };
            for &(a, b) in segs {
                next.insert(a, b);
            }
        }
    }
    let mut loops = Vec::new();
    // Start each loop at its smallest edge, not in hash order, so the same
    // image always gives the same outline (and the same mesh).
    let mut keys: Vec<_> = next.keys().copied().collect();
    keys.sort_unstable();
    let mut done = std::collections::HashSet::new();
    for k0 in keys {
        if done.contains(&k0) {
            continue;
        }
        let mut lp = Vec::new();
        let mut k = k0;
        let mut closed = false;
        while done.insert(k) {
            lp.push(point(k));
            match next.get(&k) {
                Some(&n) => k = n,
                None => break,
            }
            if k == k0 {
                closed = true;
                break;
            }
        }
        if closed && lp.len() >= 3 {
            loops.push(lp);
        }
    }
    loops
}

fn area(p: &[[f32; 2]]) -> f32 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        / 2.0
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Resamples a closed polyline at unit arc-length steps.
fn uniform(lp: &[[f32; 2]], step: f32) -> Vec<[f32; 2]> {
    let n = lp.len();
    let mut out = vec![lp[0]];
    let mut carry = 0.0;
    for i in 0..n {
        let (a, b) = (lp[i], lp[(i + 1) % n]);
        let len = dist(a, b);
        let mut t = step - carry;
        while t <= len {
            let u = t / len;
            out.push([a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u]);
            t += step;
        }
        carry = len - (t - step);
    }
    if out.len() > 1 && dist(out[0], out[out.len() - 1]) < step * 0.5 {
        out.pop();
    }
    out
}

/// Places the ring points on a closed contour (§3.4 steps 2–4).
fn place_ring(contour: &[[f32; 2]], h: f32, min_points: usize) -> Vec<[f32; 2]> {
    let c = uniform(contour, 1.0);
    let n = c.len();
    if n < 3 {
        return c;
    }
    let length = n as f32;
    let even = |count: usize, from: usize| -> Vec<[f32; 2]> {
        (0..count).map(|k| c[(from + k * n / count) % n]).collect()
    };
    // Turning angle with a ±k window, smoothed [¼, ½, ¼] AVERAGING times.
    let k = 3.min(n / 3).max(1);
    let mut turn: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b, d) = (c[(i + n - k) % n], c[i], c[(i + k) % n]);
            let (u, v) = ([b[0] - a[0], b[1] - a[1]], [d[0] - b[0], d[1] - b[1]]);
            (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1])
        })
        .collect();
    for _ in 0..AVERAGING {
        turn = (0..n)
            .map(|i| 0.25 * turn[(i + n - 1) % n] + 0.5 * turn[i] + 0.25 * turn[(i + 1) % n])
            .collect();
    }
    let threshold = CORNER_DEG.to_radians();
    let mut corners: Vec<usize> = (0..n)
        .filter(|&i| {
            let t = turn[i].abs();
            t > threshold && t >= turn[(i + n - 1) % n].abs() && t > turn[(i + 1) % n].abs()
        })
        .collect();
    // Keep the strongest of corners closer than h / 2.
    corners.sort_by(|&a, &b| turn[b].abs().total_cmp(&turn[a].abs()));
    let mut kept: Vec<usize> = Vec::new();
    for i in corners {
        let gap = |j: usize| {
            let d = i.abs_diff(j);
            d.min(n - d) as f32
        };
        if kept.iter().all(|&j| gap(j) >= h * 0.5) {
            kept.push(i);
        }
    }
    let count = min_points.max((length / h).round() as usize).max(3);
    if kept.len() < 2 {
        return even(count, 0);
    }
    kept.sort_unstable();
    let mut out = Vec::new();
    for (q, &i) in kept.iter().enumerate() {
        let j = kept[(q + 1) % kept.len()];
        let arc = (j + n - i) % n;
        let arc = if arc == 0 { n } else { arc };
        out.push(c[i]);
        let segments = ((arc as f32 / h * 1.2).floor() as usize).max(1);
        for s in 1..segments {
            out.push(c[(i + s * arc / segments) % n]);
        }
    }
    if out.len() < min_points {
        return even(count, kept[0]);
    }
    out
}

/// Spacing fixes of the inner ring (§3.5): insert points across gaps over
/// 0.9 h, drop points closer than 0.2 h to the previous one.
fn tidy_ring(ring: Vec<[f32; 2]>, h: f32) -> Vec<[f32; 2]> {
    let n = ring.len();
    let mut out: Vec<[f32; 2]> = Vec::new();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        out.push(a);
        let d = dist(a, b);
        if d > 0.9 * h {
            let extra = (d / (0.9 * h) - 0.3).floor().max(0.0) as usize;
            for e in 1..=extra {
                let u = e as f32 / (extra + 1) as f32;
                out.push([a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u]);
            }
        }
    }
    let mut tidy: Vec<[f32; 2]> = Vec::new();
    for p in out {
        if tidy.last().is_none_or(|&q| dist(p, q) >= 0.2 * h) {
            tidy.push(p);
        }
    }
    while tidy.len() > 3 && dist(tidy[0], tidy[tidy.len() - 1]) < 0.2 * h {
        tidy.pop();
    }
    tidy
}

/// Whether segments ab and cd cross (proper intersection).
fn crosses(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> bool {
    let o = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let (d1, d2) = (o(a, b, c), o(a, b, d));
    let (d3, d4) = (o(c, d, a), o(c, d, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// Drops inner-ring points until no inner segment crosses the outer ring
/// or another inner segment (crossing ring edges can't both be kept by
/// the triangulation).
fn untangle(mut inner: Vec<[f32; 2]>, outer: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let ring_seg = |r: &[[f32; 2]], i: usize| (r[i], r[(i + 1) % r.len()]);
    for _ in 0..inner.len() {
        let n = inner.len();
        if n < 3 {
            break;
        }
        let bad = (0..n).find(|&i| {
            let (a, b) = ring_seg(&inner, i);
            (0..outer.len()).any(|j| {
                let (c, d) = ring_seg(outer, j);
                crosses(a, b, c, d)
            }) || (0..n).any(|j| {
                let (c, d) = ring_seg(&inner, j);
                j != i && crosses(a, b, c, d)
            })
        });
        let Some(i) = bad else { break };
        // Remove the segment's end nearer the outer ring.
        let j = (i + 1) % n;
        let k = if polyline_dist(inner[i], outer) < polyline_dist(inner[j], outer) {
            i
        } else {
            j
        };
        inner.remove(k);
    }
    inner
}

fn in_polygon(p: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

fn polyline_dist(p: [f32; 2], poly: &[[f32; 2]]) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 {
                (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            dist(p, [a[0] + t * dx, a[1] + t * dy])
        })
        .fold(f32::INFINITY, f32::min)
}

/// Generates the mesh for a model image (`image_to_canvas` maps its pixels
/// to the canvas, row-major 2×3; `density_scale` is `polygonDensityScale`).
/// spec: auto-mesh.md §3
pub fn generate(
    img: &AlphaImage,
    image_to_canvas: [f32; 6],
    s: &MeshGenerateSetting,
    density_scale: f32,
) -> Generated {
    let (iw, ih) = (img.width as usize, img.height as usize);
    if iw == 0 || ih == 0 || img.alpha.len() < iw * ih {
        return Generated::default();
    }
    // §3.1 working image.
    let long = iw.max(ih);
    let mut lw = (long as f32 * 0.5) as usize;
    while lw > 256 {
        lw = (lw as f32 * 0.8) as usize;
    }
    let lw = lw.max(20);
    let scale = lw as f32 / long as f32;
    let (sw, sh) = (
        ((iw as f32 * scale) as usize).max(1),
        ((ih as f32 * scale) as usize).max(1),
    );
    // Padding: the spec's ceil(2·margin·s), and room for the offset ring.
    let outer_max = s.margin as f32 * SIGMA * density_scale.max(1.0);
    let pad = ((2.0 * s.margin as f32 * scale).ceil() as usize).max(outer_max.ceil() as usize + 3);
    let (w, h) = (sw + 2 * pad, sh + 2 * pad);
    // Area-averaged alpha.
    let mut alpha = vec![0.0f32; w * h];
    for y in 0..sh {
        for x in 0..sw {
            let (x0, x1) = (
                (x as f32 / scale) as usize,
                (((x + 1) as f32 / scale) as usize)
                    .max((x as f32 / scale) as usize + 1)
                    .min(iw),
            );
            let (y0, y1) = (
                (y as f32 / scale) as usize,
                (((y + 1) as f32 / scale) as usize)
                    .max((y as f32 / scale) as usize + 1)
                    .min(ih),
            );
            let mut acc = 0.0;
            for yy in y0..y1 {
                for xx in x0..x1 {
                    acc += f32::from(img.alpha[yy * iw + xx]);
                }
            }
            alpha[(y + pad) * w + x + pad] = acc / ((x1 - x0) * (y1 - y0)).max(1) as f32;
        }
    }
    let solid: Vec<bool> = alpha
        .iter()
        .map(|&a| a > s.threshold_alpha as f32)
        .collect();

    // §3.3 islands.
    let mut found = islands(w, h, &solid);
    if found.is_empty() {
        return Generated::default();
    }
    let largest = found[0].len();
    let noise = IGNORE_AREA.min(largest / 4);
    found.retain(|p| p.len() >= noise.max(1));
    let outer_density = 1.0 / (s.outer_density as f32).powi(2) / SIGMA.powi(2);
    let inner_density = 1.0 / (s.inner_density as f32).powi(2) / SIGMA.powi(2);
    let min_margin_setting = (s.min_margin as f32).clamp(0.0, (s.margin - 2).max(0) as f32);

    let to_canvas = |p: [f32; 2]| -> [f32; 2] {
        let q = [(p[0] - pad as f32) / scale, (p[1] - pad as f32) / scale];
        let m = image_to_canvas;
        [
            m[0] * q[0] + m[1] * q[1] + m[2],
            m[3] * q[0] + m[4] * q[1] + m[5],
        ]
    };
    let mut out = Generated::default();
    for pixels in &found {
        let mut mine = vec![false; w * h];
        for &i in pixels {
            mine[i] = true;
        }
        // Size factor f (§3.3 step 5).
        let a = ((pixels.len() as f32).sqrt() / (largest as f32).sqrt()).max(0.1);
        let t = (a.ln() / 0.1f32.ln()).clamp(0.0, 1.0);
        let f = density_scale.max(1e-3).powf(t);
        let interior_spacing = 1.0 / (inner_density * f * f).sqrt();
        let outer_margin = ((s.margin as f32 * SIGMA / f) as i32) as f32;
        let min_margin = min_margin_setting * SIGMA / f;
        let inner_margin = s.inner_margin as f32 * SIGMA / (f * f);
        let h_ring = 1.0 / outer_density.sqrt();

        let outside = distance(w, h, |i| mine[i]);
        let inside = distance(w, h, |i| !mine[i]);
        let signed = Field {
            w,
            h,
            v: outside
                .v
                .iter()
                .zip(&inside.v)
                .map(|(o, i)| o - i)
                .collect(),
        };
        // §3.4 outer ring: the offset contour (the largest loop).
        let level = outer_margin.max(0.5);
        let Some(contour) = contours(&outside, level)
            .into_iter()
            .max_by(|a, b| area(a).abs().total_cmp(&area(b).abs()))
        else {
            continue;
        };
        let outer = place_ring(&contour, h_ring, s.min_bounds_pt.max(3) as usize);
        if outer.len() < 3 {
            continue;
        }
        let thickest = inside.v.iter().copied().fold(0.0f32, f32::max);
        let mut inner: Vec<[f32; 2]> = Vec::new();
        let mut interior: Vec<[f32; 2]> = Vec::new();
        if thickest >= 2.0 * inner_margin.max(0.5) {
            // §3.5 inner ring: walk down the signed distance to −inner margin.
            for &p in &outer {
                let mut q = p;
                for _ in 0..((outer_margin + inner_margin + 8.0) * 4.0) as usize {
                    if signed.sample(q) <= -inner_margin {
                        break;
                    }
                    let g = signed.gradient(q);
                    let l = g[0].hypot(g[1]);
                    if l < 1e-6 {
                        break;
                    }
                    q = [q[0] - 0.5 * g[0] / l, q[1] - 0.5 * g[1] / l];
                }
                if signed.sample(q) <= -inner_margin + 0.75 {
                    inner.push(q);
                }
            }
            inner = untangle(tidy_ring(inner, h_ring), &outer);
            if inner.len() < 3 {
                inner.clear();
            }
        }
        if inner.is_empty() {
            // Skeleton mode: the inside distance ridge at interior spacing.
            let mut ridge: Vec<(usize, f32)> = pixels
                .iter()
                .copied()
                .filter(|&i| {
                    let (x, y) = (i % w, i / w);
                    let d = inside.v[i];
                    d >= 1.0
                        && (y.saturating_sub(1)..=(y + 1).min(h - 1)).all(|yy| {
                            (x.saturating_sub(1)..=(x + 1).min(w - 1))
                                .all(|xx| inside.at(xx, yy) <= d)
                        })
                })
                .map(|i| (i, inside.v[i]))
                .collect();
            ridge.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (i, _) in ridge {
                let p = [(i % w) as f32 + 0.5, (i / w) as f32 + 0.5];
                if interior
                    .iter()
                    .all(|&q| dist(p, q) >= interior_spacing * 0.9)
                    && polyline_dist(p, &outer) >= min_margin
                {
                    interior.push(p);
                }
            }
        } else {
            // §3.6 interior: hexagonal lattice inside the inner ring.
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in &inner {
                (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
            }
            let row = interior_spacing * 3f32.sqrt() / 2.0;
            let mut r = 0;
            let mut y = y0 + row / 2.0;
            while y < y1 {
                let shift = if r % 2 == 1 {
                    interior_spacing / 2.0
                } else {
                    0.0
                };
                let mut x = x0 + shift + interior_spacing / 2.0;
                while x < x1 {
                    let p = [x, y];
                    if in_polygon(p, &inner)
                        && polyline_dist(p, &inner) >= min_margin.max(interior_spacing * 0.35)
                        && inside.sample(p) >= min_margin
                    {
                        interior.push(p);
                    }
                    x += interior_spacing;
                }
                y += row;
                r += 1;
            }
        }
        // §3.7 output.
        let ring = |pts: &[[f32; 2]], out: &mut Generated| {
            let base = out.points.len() as u32;
            let n = pts.len() as u32;
            out.points.extend(pts.iter().map(|&p| to_canvas(p)));
            for k in 0..n {
                out.edges.push([base + k, base + (k + 1) % n]);
            }
        };
        ring(&outer, &mut out);
        if !inner.is_empty() {
            ring(&inner, &mut out);
        }
        out.points.extend(interior.iter().map(|&p| to_canvas(p)));
        out.islands += 1;
    }
    out
}

/// The editable graph of a generated mesh replacing `base` (an art mesh's
/// graph): AUTO points with fresh UIDs, NORMAL ring edges, Delaunay
/// triangulation (§3.7).
pub fn generated_graph(base: &MeshGraph, g: &Generated) -> MeshGraph {
    let mut graph = base.clone();
    let n = graph.len();
    graph.delete_points(&(0..n).collect());
    graph.mesh.delaunay = true;
    for &p in &g.points {
        let k = graph.add_point(p);
        graph.mesh.point_priority[k] = POINT_AUTO;
    }
    for e in &g.edges {
        graph.add_edge(e[0], e[1], EDGE_NORMAL);
    }
    graph
}

/// Whether art mesh `i` is a glue target (auto-mesh.md §1).
pub fn is_glued(m: &Model, i: usize) -> bool {
    m.glues
        .iter()
        .any(|g| g.mesh_a == Some(i) || g.mesh_b == Some(i))
}

/// Replaces art mesh `i`'s mesh with one generated from `img` (its model
/// image) and stores `s` as the mesh's settings. Keyforms and UVs are
/// remapped (auto-mesh.md §4). Returns the triangulation report.
pub fn auto_mesh(
    m: &mut Model,
    i: usize,
    s: &MeshGenerateSetting,
    density_scale: f32,
    img: &AlphaImage,
) -> Result<Triangulated, String> {
    let a = m.art_meshes.get(i).ok_or("no such art mesh")?;
    if is_glued(m, i) {
        return Err(GLUED.into());
    }
    let mi = a
        .model_image
        .and_then(|k| m.model_images.get(k))
        .ok_or_else(|| format!("{}: the ArtMesh has no model image.", a.info.display_name()))?;
    let g = generate(img, mi.local_to_canvas, s, density_scale);
    if g.points.len() < 3 {
        return Err(format!(
            "{}: no opaque pixels to build a mesh from.",
            a.info.display_name()
        ));
    }
    let graph = generated_graph(&MeshGraph::of(a), &g);
    let t = set_mesh(m, i, &graph).map_err(|e| e.to_string())?;
    Arc::make_mut(&mut m.art_meshes[i]).generator = *s;
    Ok(t)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn disc(size: u32, r: f32) -> AlphaImage {
        let c = size as f32 / 2.0;
        let alpha = (0..size * size)
            .map(|i| {
                let (x, y) = ((i % size) as f32 + 0.5, (i / size) as f32 + 0.5);
                if (x - c).hypot(y - c) <= r {
                    255
                } else {
                    0
                }
            })
            .collect();
        AlphaImage {
            width: size,
            height: size,
            alpha,
        }
    }

    const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

    #[test]
    fn disc_gets_two_rings_and_a_fill() {
        let img = disc(400, 150.0);
        let (_, s, d) = presets()[1];
        let g = generate(&img, IDENTITY, &s, d);
        assert_eq!(g.islands, 1);
        // Outer ring points sit outside the disc, by about the margin
        // (14 working px ≈ 7 / 0.5 scale … in image px: margin·σ/scale).
        let ring: Vec<[f32; 2]> = g
            .edges
            .iter()
            .take_while(|e| e[1] != 0)
            .map(|e| g.points[e[0] as usize])
            .collect();
        assert!(ring.len() >= 10, "{}", ring.len());
        for p in &ring {
            let r = (p[0] - 200.0).hypot(p[1] - 200.0);
            assert!(r > 150.0 && r < 200.0, "outer point at r = {r}");
        }
        // Some points inside the disc that aren't on a ring.
        let interior = g.points.len() - g.edges.len();
        assert!(interior >= 5, "interior {interior}");
        for p in &g.points[g.edges.len()..] {
            assert!((p[0] - 200.0).hypot(p[1] - 200.0) < 150.0);
        }
        // Coarser preset → fewer points.
        let little = generate(&img, IDENTITY, &presets()[0].1, 1.0);
        let heavy = generate(&img, IDENTITY, &presets()[2].1, 1.0);
        assert!(little.points.len() < g.points.len());
        assert!(heavy.points.len() > g.points.len());
    }

    /// Found by the e2e session: the same layer gave a different mesh on
    /// each run because contour loops started in hash-map order.
    #[test]
    fn same_image_same_mesh() {
        let mut img = disc(300, 110.0);
        // An irregular shape, so where a loop starts changes the outline.
        for i in 0..300 * 300usize {
            let (x, y) = ((i % 300) as f32, (i / 300) as f32);
            if (x - 210.0).hypot(y - 90.0) < 45.0
                || (x > 60.0 && x < 140.0 && y > 200.0 && y < 270.0)
            {
                img.alpha[i] = 255;
            }
        }
        let first = generate(&img, IDENTITY, &presets()[1].1, 1.0);
        for _ in 0..8 {
            let g = generate(&img, IDENTITY, &presets()[1].1, 1.0);
            assert_eq!(g.points, first.points);
            assert_eq!(g.edges, first.edges);
        }
    }

    #[test]
    fn two_islands_and_noise() {
        let mut img = disc(300, 0.0);
        let mut put = |cx: f32, cy: f32, r: f32| {
            for i in 0..300 * 300usize {
                let (x, y) = ((i % 300) as f32 + 0.5, (i / 300) as f32 + 0.5);
                if (x - cx).hypot(y - cy) <= r {
                    img.alpha[i] = 255;
                }
            }
        };
        put(80.0, 150.0, 50.0);
        put(220.0, 150.0, 40.0);
        put(150.0, 20.0, 2.0); // noise
        let g = generate(&img, IDENTITY, &presets()[1].1, 1.0);
        assert_eq!(g.islands, 2);
    }

    #[test]
    fn thin_stroke_uses_the_skeleton() {
        // A 6 px wide horizontal bar: no room for an inner ring.
        let (w, h) = (300u32, 60u32);
        let alpha = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if (27..33).contains(&y) && (20..280).contains(&x) {
                    255
                } else {
                    0
                }
            })
            .collect();
        let img = AlphaImage {
            width: w,
            height: h,
            alpha,
        };
        let g = generate(&img, IDENTITY, &presets()[1].1, 1.0);
        assert_eq!(g.islands, 1);
        let ring_points = g.edges.len();
        assert!(g.points.len() > ring_points, "skeleton points");
        for p in &g.points[ring_points..] {
            assert!((p[1] - 30.0).abs() < 4.0, "{p:?}");
        }
    }

    #[test]
    fn empty_and_degenerate_images() {
        let s = MeshGenerateSetting::default();
        assert_eq!(generate(&disc(50, 0.0), IDENTITY, &s, 2.0).islands, 0);
        let bad = AlphaImage {
            width: 10,
            height: 10,
            alpha: vec![255; 5],
        };
        assert_eq!(generate(&bad, IDENTITY, &s, 2.0), Generated::default());
        // One opaque pixel.
        let mut one = disc(64, 0.0);
        one.alpha[32 * 64 + 32] = 255;
        let g = generate(&one, IDENTITY, &s, 2.0);
        assert!(g.points.len() >= 3 || g.islands == 0);
    }
}
