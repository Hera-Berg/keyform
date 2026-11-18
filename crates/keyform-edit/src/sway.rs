//! Auto Generation of Sway Motion: left / right sway keyforms of a warp
//! deformer on a parameter with 3 keys.
//! spec: 70-advanced/generators.md §2 (§2.1 keys, §2.2 offsets, §2.3 assembly)

use std::sync::Arc;

use keyform_eval::Evaluator;
use keyform_model::{DeformerKind, Model, ParameterKind};

use crate::keys::ObjectRef;

/// The fixed (root) edge of the deformer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Pivot {
    #[default]
    Top,
    Right,
    Bottom,
    Left,
}

/// One side's settings (§2 table).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwaySide {
    /// Sway level (horizontal) %, 0 … 25.
    pub sx: f32,
    /// Sway level (vertical) %, −5 … 20.
    pub sy: f32,
    /// Decay exponent, 1 … 8.
    pub softness: f32,
    /// Enlarge / shrink, −4 … 4.
    pub enlarge: f32,
}

impl Default for SwaySide {
    fn default() -> Self {
        Self {
            sx: 8.0,
            sy: 6.0,
            softness: 3.0,
            enlarge: 0.0,
        }
    }
}

impl SwaySide {
    pub const SX: (f32, f32) = (0.0, 25.0);
    pub const SY: (f32, f32) = (-5.0, 20.0);
    pub const SOFTNESS: (f32, f32) = (1.0, 8.0);
    pub const ENLARGE: (f32, f32) = (-4.0, 4.0);
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SwaySettings {
    pub left: SwaySide,
    pub right: SwaySide,
    pub pivot: Pivot,
    pub flip: bool,
}

type P = [f32; 2];

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(a: P) -> f32 {
    a[0].hypot(a[1])
}

/// Grid offsets of one side (§2.2), indexed like the grid points
/// (i·(cols+1) + j). `base` are the base keyform's canvas points.
pub fn side_offsets(
    base: &[P],
    rows: usize,
    cols: usize,
    s: &SwaySide,
    pivot: Pivot,
    flip: bool,
    left: bool,
) -> Vec<P> {
    let at = |i: usize, j: usize| base[i * (cols + 1) + j];
    let (tl, tr, bl, br) = (at(0, 0), at(0, cols), at(rows, 0), at(rows, cols));
    let (mut n_r, mut n_c) = (rows + 1, cols + 1);
    let (mut w, mut h) = (len(sub(tr, tl)), len(sub(br, tr)));
    if matches!(pivot, Pivot::Left | Pivot::Right) {
        std::mem::swap(&mut n_r, &mut n_c);
        std::mem::swap(&mut w, &mut h);
    }
    // 2.
    let a = (h * s.sx / 100.0).max(5.0);
    let b = -h * s.sy / 100.0;
    let rho2 = a * a + b * b;
    // 3.
    let last = (n_c - 1).max(1) as f32;
    let mut xy: Vec<P> = (0..n_c)
        .map(|k| {
            let tau = k as f32 / last;
            let y = -0.15 * b + tau.powf(1.3) * (b + 0.15 * b);
            [(rho2 - y * y).max(0.0).sqrt(), y]
        })
        .collect();
    // 4. Monotone x.
    for k in (1..n_c).rev() {
        xy[k - 1][0] = xy[k - 1][0].min(xy[k][0] + w / last);
    }
    // 5. Enlarge / shrink.
    let w2 = if s.enlarge >= 0.0 {
        (1.0 + s.enlarge / 10.0) * w
    } else {
        w / (1.0 + s.enlarge.abs() / 10.0)
    };
    for (k, p) in xy.iter_mut().enumerate() {
        p[0] += (-1.0 + 2.0 * k as f32 / last) * (w2 - w) / 2.0;
    }
    // 6. Rotate to the root edge; flip.
    let f = flip ^ left;
    let edge = match pivot {
        Pivot::Top => sub(tr, tl),
        Pivot::Right => sub(br, tr),
        Pivot::Bottom => sub(bl, br),
        Pivot::Left => sub(tl, bl),
    };
    let mut th = edge[1].atan2(edge[0]);
    if f {
        th = -th;
    }
    let (sn, cs) = th.sin_cos();
    for p in &mut xy {
        *p = [p[0] * cs - p[1] * sn, p[0] * sn + p[1] * cs];
    }
    if f {
        xy.reverse();
        for p in &mut xy {
            p[0] = -p[0];
        }
    }
    // 7.–8.
    let mut out = vec![[0.0; 2]; base.len()];
    let r_last = (n_r - 1).max(1) as f32;
    for r in 0..n_r {
        let wr = (r as f32 / r_last).powf(s.softness);
        for (k, p) in xy.iter().enumerate() {
            let (i, j) = match pivot {
                Pivot::Top => (r, k),
                Pivot::Bottom => (rows - r, cols - k),
                Pivot::Right => (k, cols - r),
                Pivot::Left => (rows - k, r),
            };
            out[i * (cols + 1) + j] = [p[0] * wr, p[1] * wr];
        }
    }
    out
}

/// Target canvas points from the base points and one side's offsets
/// (§2.3 steps 1–3).
pub fn assemble(base: &[P], rows: usize, cols: usize, offsets: &[P]) -> Vec<P> {
    let n_c = cols + 1;
    let at = |i: usize, j: usize| base[i * n_c + j];
    let (tl, tr, br) = (at(0, 0), at(0, cols), at(rows, cols));
    let (u, v) = (sub(tr, tl), sub(br, tr));
    let mut off = offsets.to_vec();
    if u[0] * v[1] - u[1] * v[0] < 0.0 {
        // Mirrored grid: each row reversed and negated.
        for i in 0..=rows {
            let row: Vec<P> = (0..n_c).map(|j| offsets[i * n_c + n_c - 1 - j]).collect();
            for (j, p) in row.into_iter().enumerate() {
                off[i * n_c + j] = [-p[0], -p[1]];
            }
        }
    }
    let mut pts: Vec<P> = base
        .iter()
        .zip(&off)
        .map(|(b, o)| [b[0] + o[0], b[1] + o[1]])
        .collect();
    // 3. Smooth kinks along rows (y onto the line), then columns (x).
    let side = |a: P, b: P, p: P| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let smooth = |pts: &mut Vec<P>, lines: Vec<Vec<usize>>, rows_pass: bool| {
        let src = pts.clone();
        for line in lines {
            for w in line.windows(4) {
                let [p0, p1, p2, p3] = [src[w[0]], src[w[1]], src[w[2]], src[w[3]]];
                if side(p0, p3, p1) * side(p0, p3, p2) < 0.0 {
                    for (idx, p) in [(w[1], p1), (w[2], p2)] {
                        if rows_pass {
                            let dx = p3[0] - p0[0];
                            if dx.abs() > 1e-9 {
                                pts[idx][1] = p0[1] + (p[0] - p0[0]) / dx * (p3[1] - p0[1]);
                            }
                        } else {
                            let dy = p3[1] - p0[1];
                            if dy.abs() > 1e-9 {
                                pts[idx][0] = p0[0] + (p[1] - p0[1]) / dy * (p3[0] - p0[0]);
                            }
                        }
                    }
                }
            }
        }
    };
    let row_lines = (0..=rows)
        .map(|i| (0..n_c).map(|j| i * n_c + j).collect())
        .collect();
    smooth(&mut pts, row_lines, true);
    let col_lines = (0..n_c)
        .map(|j| (0..=rows).map(|i| i * n_c + j).collect())
        .collect();
    smooth(&mut pts, col_lines, false);
    pts
}

/// Writes the sway keyforms of warp deformer `d` on parameter `p` (§2.1):
/// the parameter's 3 keys k₀ < k₁ < k₂ with the other parameters at their
/// defaults; k₁ is the base, left writes k₀, right writes k₂.
pub fn generate(m: &mut Model, d: usize, p: usize, s: &SwaySettings) -> Result<(), String> {
    let param = m.parameters.get(p).ok_or("No such parameter.")?.clone();
    if param.kind == ParameterKind::BlendShape {
        // TODO(Q-0040): sway on blend-shape keys.
        return Err("Sway on blend-shape parameters is not supported yet.".into());
    }
    let def = m.deformers.get(d).ok_or("No such deformer.")?.clone();
    let DeformerKind::Warp(w) = &def.kind else {
        return Err("Sway motion needs a warp deformer.".into());
    };
    let keys = crate::keys::keys_of(m, ObjectRef::Deformer(d), p);
    if keys.len() != 3 {
        return Err("Would you like to delete the existing keys and add 3 keys?".into());
    }
    let (rows, cols) = (w.rows as usize, w.cols as usize);
    let mut values: Vec<f32> = m.parameters.iter().map(|q| q.default).collect();
    let cell = |m: &Model, values: &[f32]| {
        crate::forms::current_cell(m, ObjectRef::Deformer(d), values).map_err(|e| e.to_string())
    };
    values[p] = keys[1];
    let base_cell = cell(m, &values)?;
    let frame = Evaluator::new(m).evaluate(&values);
    let parent = def
        .target
        .and_then(|t| frame.deformers.get(t))
        .and_then(|x| x.map.clone());
    let to_canvas = |q: P| parent.as_ref().map_or(q, |pm| pm.map(q));
    let base: Vec<P> = w.grid.keyforms[base_cell]
        .positions
        .chunks_exact(2)
        .map(|c| to_canvas([c[0], c[1]]))
        .collect();
    let mut writes = Vec::new();
    for (k, side, left) in [(keys[0], &s.left, true), (keys[2], &s.right, false)] {
        values[p] = k;
        let c = cell(m, &values)?;
        if c == base_cell {
            return Err("The three keys must have distinct keyforms.".into());
        }
        let off = side_offsets(&base, rows, cols, side, s.pivot, s.flip, left);
        let pts = assemble(&base, rows, cols, &off);
        let mut local = Vec::with_capacity(pts.len() * 2);
        for q in pts {
            let l = match &parent {
                Some(pm) => pm
                    .inverse(q)
                    .ok_or("the point cannot be mapped into the parent deformer")?,
                None => q,
            };
            local.extend_from_slice(&l);
        }
        writes.push((c, local));
    }
    let DeformerKind::Warp(w) = &mut Arc::make_mut(&mut m.deformers[d]).kind else {
        unreachable!()
    };
    for (c, local) in writes {
        w.grid.keyforms[c].positions = local;
    }
    Ok(())
}

impl SwaySettings {
    /// From a stored setting (`AutoYureConfig`).
    pub fn from_config(c: &keyform_model::SwayConfig) -> Self {
        let side = |s: &keyform_model::SwaySideConfig| SwaySide {
            sx: s.scale_x,
            sy: s.scale_y,
            softness: s.decay as f32,
            enlarge: s.expand,
        };
        Self {
            left: side(&c.left),
            right: side(&c.right),
            pivot: match c.root {
                1 => Pivot::Right,
                2 => Pivot::Bottom,
                3 => Pivot::Left,
                _ => Pivot::Top,
            },
            flip: c.flip,
        }
    }

    /// To a stored setting for `parameter`.
    pub fn to_config(&self, parameter: usize, sync: bool) -> keyform_model::SwayConfig {
        let side = |s: &SwaySide| keyform_model::SwaySideConfig {
            scale_x: s.sx,
            scale_y: s.sy,
            expand: s.enlarge,
            decay: f64::from(s.softness),
        };
        keyform_model::SwayConfig {
            parameter,
            left: side(&self.left),
            right: side(&self.right),
            sync,
            root: self.pivot as u8,
            flip: self.flip,
        }
    }
}

/// Stores `c` on warp deformer `d` (replacing the setting of its parameter).
pub fn store(m: &mut Model, d: usize, c: keyform_model::SwayConfig) {
    if let Some(def) = m.deformers.get_mut(d) {
        if let DeformerKind::Warp(w) = &mut Arc::make_mut(def).kind {
            match w.sway.iter_mut().find(|x| x.parameter == c.parameter) {
                Some(x) => *x = c,
                None => w.sway.push(c),
            }
        }
    }
}

/// The stored setting of warp `d` for parameter `p`.
pub fn stored(m: &Model, d: usize, p: usize) -> Option<keyform_model::SwayConfig> {
    match &m.deformers.get(d)?.kind {
        DeformerKind::Warp(w) => w.sway.iter().find(|x| x.parameter == p).copied(),
        DeformerKind::Rotation(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: usize, cols: usize, w: f32, h: f32) -> Vec<P> {
        let mut v = Vec::new();
        for i in 0..=rows {
            for j in 0..=cols {
                v.push([j as f32 * w / cols as f32, i as f32 * h / rows as f32]);
            }
        }
        v
    }

    #[test]
    fn top_pivot_offsets() {
        let (rows, cols) = (4, 4);
        let base = grid(rows, cols, 100.0, 200.0);
        let s = SwaySide::default();
        let off = side_offsets(&base, rows, cols, &s, Pivot::Top, false, false);
        // Root row does not move.
        for j in 0..=cols {
            assert_eq!(off[j], [0.0, 0.0]);
        }
        // Tip row: A = 16, B = −12, ρ = 20; k = 0: y = 1.8, x = √(400 − 3.24).
        let tip = &off[rows * (cols + 1)..];
        assert!((tip[0][1] - 1.8).abs() < 1e-4, "{:?}", tip[0]);
        assert!((tip[0][0] - (400.0f32 - 3.24).sqrt()).abs() < 1e-3);
        // k = last: y = B = −12, x = 16.
        assert!((tip[cols][1] + 12.0).abs() < 1e-4);
        assert!((tip[cols][0] - 16.0).abs() < 1e-3);
        // Softness 3: the middle row moves (1/2)³ as much.
        let mid = off[2 * (cols + 1) + cols];
        assert!((mid[1] + 12.0 / 8.0).abs() < 1e-4);
        // Left side mirrors x (flip XOR left), reversed.
        let l = side_offsets(&base, rows, cols, &s, Pivot::Top, false, true);
        let ltip = &l[rows * (cols + 1)..];
        assert!((ltip[0][0] + 16.0).abs() < 1e-3 && (ltip[0][1] + 12.0).abs() < 1e-4);
        // Assembly keeps the root row.
        let pts = assemble(&base, rows, cols, &off);
        assert_eq!(pts[0], base[0]);
    }
}
