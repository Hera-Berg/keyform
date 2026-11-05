//! Extended interpolation: the generated (denser) keyform grid of an
//! object whose bindings use ELLIPSE or SNS_CURVE.
//! spec: 30-algorithms/extended-interpolation.md §1–3,
//! 30-algorithms/sns-spline.md §1

use keyform_model::{
    ArtMeshForm, Binding, ExtendedInterpolationKind, GlueForm, KeyformGrid, PartForm, RotationForm,
    WarpForm,
};

use crate::interp::Blend;

type V2 = [f32; 2];

/// Forms whose point positions follow the curve (everything with a
/// position: mesh vertices, warp grid points, rotation origin).
pub trait Points {
    fn points(&self) -> Vec<V2>;
    fn set_point(&mut self, k: usize, p: V2);
}

impl Points for ArtMeshForm {
    fn points(&self) -> Vec<V2> {
        self.positions
            .chunks_exact(2)
            .map(|p| [p[0], p[1]])
            .collect()
    }
    fn set_point(&mut self, k: usize, p: V2) {
        if let Some(q) = self.positions.get_mut(2 * k..2 * k + 2) {
            q.copy_from_slice(&p);
        }
    }
}

impl Points for WarpForm {
    fn points(&self) -> Vec<V2> {
        self.positions
            .chunks_exact(2)
            .map(|p| [p[0], p[1]])
            .collect()
    }
    fn set_point(&mut self, k: usize, p: V2) {
        if let Some(q) = self.positions.get_mut(2 * k..2 * k + 2) {
            q.copy_from_slice(&p);
        }
    }
}

impl Points for RotationForm {
    fn points(&self) -> Vec<V2> {
        vec![self.origin]
    }
    fn set_point(&mut self, k: usize, p: V2) {
        if k == 0 {
            self.origin = p;
        }
    }
}

impl Points for PartForm {
    fn points(&self) -> Vec<V2> {
        Vec::new()
    }
    fn set_point(&mut self, _: usize, _: V2) {}
}

impl Points for GlueForm {
    fn points(&self) -> Vec<V2> {
        Vec::new()
    }
    fn set_point(&mut self, _: usize, _: V2) {}
}

/// Whether binding `b` is extended: a non-linear type, ≥ 3 keys and (for
/// ELLIPSE) legal (§3).
pub fn is_extended(b: &Binding) -> bool {
    if b.extended == ExtendedInterpolationKind::Linear || b.keys.len() < 3 {
        return false;
    }
    b.extended != ExtendedInterpolationKind::Ellipse || ellipse_legal(&b.keys, b.extended_scale)
}

/// ELLIPSE is illegal if adjacent keys differ by more than 175° after
/// scaling, or keys two apart by more than 270°. spec: §3
pub fn ellipse_legal(keys: &[f32], scale: f32) -> bool {
    keys.windows(2)
        .all(|w| ((w[1] - w[0]) * scale).abs() <= 175.0)
        && keys
            .windows(3)
            .all(|w| ((w[2] - w[0]) * scale).abs() <= 270.0)
}

/// The key list with N evenly spaced keys inserted between each pair, and
/// for every new key its (segment, τ); τ = 0 marks an original key.
pub fn inserted_keys(keys: &[f32], n: usize) -> Vec<(f32, usize, f32)> {
    let mut out = Vec::new();
    for i in 0..keys.len() {
        out.push((keys[i], i, 0.0));
        if i + 1 < keys.len() {
            for j in 1..=n {
                let t = j as f32 / (n + 1) as f32;
                out.push((keys[i] + t * (keys[i + 1] - keys[i]), i, t));
            }
        }
    }
    out
}

// ------------------------------------------------------------ SNS spline

fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn len(a: V2) -> f32 {
    a[0].hypot(a[1])
}
fn scale(a: V2, s: f32) -> V2 {
    [a[0] * s, a[1] * s]
}
fn normalize(a: V2) -> V2 {
    let l = len(a);
    if l > 0.0 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0]
    }
}

/// Open SNS spline through `p` (sns-spline.md §1): velocities after the
/// three lagged smoothing passes.
fn sns_velocities(p: &[V2]) -> (Vec<V2>, Vec<f32>) {
    let n = p.len();
    let d: Vec<f32> = (0..n.saturating_sub(1))
        .map(|i| len(sub(p[i + 1], p[i])))
        .collect();
    let mut v = vec![[0.0f32; 2]; n];
    if n < 2 {
        return (v, d);
    }
    let start = |v: &[V2], i: usize| -> V2 {
        if d[i] <= 0.0 {
            return [0.0, 0.0];
        }
        let a = scale(sub(p[i + 1], p[i]), 3.0 / d[i]);
        scale(sub(a, v[i + 1]), 0.5)
    };
    let end = |v: &[V2], i: usize| -> V2 {
        if d[i - 1] <= 0.0 {
            return [0.0, 0.0];
        }
        let a = scale(sub(p[i], p[i - 1]), 3.0 / d[i - 1]);
        scale(sub(a, v[i - 1]), 0.5)
    };
    for i in 1..n - 1 {
        v[i] = normalize(sub(p[i + 1], p[i - 1]));
    }
    v[0] = start(&v, 0);
    v[n - 1] = end(&v, n - 1);
    for _ in 0..3 {
        let mut carry = start(&v, 0);
        let mut new = carry;
        for i in 1..n - 1 {
            let s = d[i - 1] + d[i];
            if s > 0.0 {
                let a = scale(start(&v, i), d[i - 1]);
                let b = scale(end(&v, i), d[i]);
                new = scale([a[0] + b[0], a[1] + b[1]], 1.0 / s);
            }
            v[i - 1] = carry;
            carry = new;
        }
        v[n - 1] = end(&v, n - 1);
        if n >= 2 {
            v[n - 2] = carry;
        }
    }
    (v, d)
}

/// Point on segment `i` at `t` of the open SNS spline through `p`.
pub fn sns_point(p: &[V2], i: usize, t: f32) -> V2 {
    let n = p.len();
    if n < 2 {
        return p.first().copied().unwrap_or([0.0, 0.0]);
    }
    if i >= n - 1 {
        return p[n - 1];
    }
    let (v, d) = sns_velocities(p);
    let (t0, t1) = (scale(v[i], d[i]), scale(v[i + 1], d[i]));
    let (t2, t3) = (t * t, t * t * t);
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h10 = t3 - 2.0 * t2 + t;
    let h11 = t3 - t2;
    [
        h00 * p[i][0] + h01 * p[i + 1][0] + h10 * t0[0] + h11 * t1[0],
        h00 * p[i][1] + h01 * p[i + 1][1] + h10 * t0[1] + h11 * t1[1],
    ]
}

// --------------------------------------------------------------- ellipse

/// E(θ) through (θa, Pa), (θb, Pb), (θc, Pc) (angles in degrees).
fn ellipse_point(th: [f32; 3], p: [V2; 3], theta: f32) -> Option<V2> {
    let r = |x: f32| (x as f64).to_radians();
    let (a, b, c, t) = (r(th[0]), r(th[1]), r(th[2]), r(theta));
    let s = |x: f64| x.sin();
    let d = s(a - b) + s(b - c) + s(c - a);
    if d.abs() < 1e-12 {
        return None;
    }
    let wa = (s(b - c) + s(t - b) + s(c - t)) / d;
    let wc = (s(a - b) + s(t - a) + s(b - t)) / d;
    let wb = 1.0 - wa - wc;
    let f = |k: usize| wa * f64::from(p[0][k]) + wb * f64::from(p[1][k]) + wc * f64::from(p[2][k]);
    Some([f(0) as f32, f(1) as f32])
}

/// Ellipse point on segment `i` at fraction `t` (§2): forward estimate
/// from (i, i+1, i+2) weighted τ, backward from (i−1, i, i+1) weighted 1−τ.
pub fn ellipse_segment(keys: &[f32], scale_deg: f32, p: &[V2], i: usize, t: f32) -> V2 {
    let th: Vec<f32> = keys.iter().map(|k| k * scale_deg).collect();
    let theta = th[i] + t * (th[i + 1] - th[i]);
    let fwd = (i + 2 < p.len())
        .then(|| {
            ellipse_point(
                [th[i], th[i + 1], th[i + 2]],
                [p[i], p[i + 1], p[i + 2]],
                theta,
            )
        })
        .flatten();
    let bwd = (i >= 1)
        .then(|| {
            ellipse_point(
                [th[i - 1], th[i], th[i + 1]],
                [p[i - 1], p[i], p[i + 1]],
                theta,
            )
        })
        .flatten();
    match (fwd, bwd) {
        (Some(f), Some(b)) => [t * f[0] + (1.0 - t) * b[0], t * f[1] + (1.0 - t) * b[1]],
        (Some(f), None) => f,
        (None, Some(b)) => b,
        (None, None) => {
            let (a, b) = (p[i], p[i + 1]);
            [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
        }
    }
}

// ----------------------------------------------------------- generation

/// Expands binding `j` of `grid` (extended, ≥ 3 keys).
fn expand_axis<F: Blend + Points>(grid: &KeyformGrid<F>, j: usize) -> KeyformGrid<F> {
    let b = &grid.bindings[j];
    let m = b.keys.len();
    let n = b.insert_point_count.max(1) as usize;
    let new_keys = inserted_keys(&b.keys, n);
    let stride: usize = grid.bindings[..j].iter().map(|x| x.keys.len()).product();
    let outer: usize = grid.bindings[j + 1..]
        .iter()
        .map(|x| x.keys.len())
        .product();
    let nk = new_keys.len();
    let mut forms: Vec<Option<F>> = vec![None; stride * nk * outer];
    for o in 0..outer {
        for s in 0..stride {
            let at = |k: usize| o * stride * m + k * stride + s;
            let line: Vec<&F> = (0..m).map(|k| &grid.keyforms[at(k)]).collect();
            let pts: Vec<Vec<V2>> = line.iter().map(|f| f.points()).collect();
            for (q, &(_, seg, t)) in new_keys.iter().enumerate() {
                let f = if t == 0.0 {
                    line[seg].clone()
                } else {
                    // All fields: the linear blend.
                    let mut f = line[seg].zeroed();
                    f.add_scaled(line[seg], 1.0 - t);
                    f.add_scaled(line[seg + 1], t);
                    // Point positions: the curve through the line's points.
                    let count = pts[0].len();
                    for k in 0..count {
                        let curve: Vec<V2> = pts
                            .iter()
                            .map(|p| p.get(k).copied().unwrap_or([0.0, 0.0]))
                            .collect();
                        let v = match b.extended {
                            ExtendedInterpolationKind::SnsCurve => sns_point(&curve, seg, t),
                            ExtendedInterpolationKind::Ellipse => {
                                ellipse_segment(&b.keys, b.extended_scale, &curve, seg, t)
                            }
                            ExtendedInterpolationKind::Linear => continue,
                        };
                        f.set_point(k, v);
                    }
                    f
                };
                forms[o * stride * nk + q * stride + s] = Some(f);
            }
        }
    }
    let mut bindings = grid.bindings.clone();
    bindings[j].keys = new_keys.iter().map(|k| k.0).collect();
    KeyformGrid {
        bindings,
        keyforms: forms
            .into_iter()
            .map(|f| f.unwrap_or_else(|| grid.keyforms[0].clone()))
            .collect(),
        guids: Vec::new(),
    }
}

/// The extended keyform grid of `grid`, or `None` when no binding is
/// extended. Bindings are expanded in order. spec: §1–2
pub fn extended_grid<F: Blend + Points>(grid: &KeyformGrid<F>) -> Option<KeyformGrid<F>> {
    if !grid.bindings.iter().any(is_extended) || grid.keyforms.len() != grid.cell_count() {
        return None;
    }
    let mut g = grid.clone();
    for j in 0..g.bindings.len() {
        if is_extended(&g.bindings[j]) {
            g = expand_axis(&g, j);
        }
    }
    g.guids = Vec::new();
    Some(g)
}

fn baked<F: Blend + Points>(g: &KeyformGrid<F>) -> Option<KeyformGrid<F>> {
    let mut x = extended_grid(g)?;
    for b in &mut x.bindings {
        b.extended = ExtendedInterpolationKind::Linear;
    }
    Some(x)
}

/// A copy of `m` with every extended grid baked into plain keyforms (for
/// the moc3 export, moc3.md §9 step 1), or `None` when nothing is extended.
pub fn bake_extended(m: &keyform_model::Model) -> Option<keyform_model::Model> {
    use keyform_model::DeformerKind;
    use std::sync::Arc;
    let mut out = m.clone();
    let mut any = false;
    for p in &mut out.parts {
        if let Some(g) = baked(&p.grid) {
            Arc::make_mut(p).grid = g;
            any = true;
        }
    }
    for a in &mut out.art_meshes {
        if let Some(g) = baked(&a.grid) {
            Arc::make_mut(a).grid = g;
            any = true;
        }
    }
    for d in &mut out.deformers {
        let g = match &d.kind {
            DeformerKind::Warp(w) => baked(&w.grid).map(|g| (Some(g), None)),
            DeformerKind::Rotation(r) => baked(&r.grid).map(|g| (None, Some(g))),
        };
        if let Some((w, r)) = g {
            match &mut Arc::make_mut(d).kind {
                DeformerKind::Warp(x) => x.grid = w.unwrap_or_else(|| x.grid.clone()),
                DeformerKind::Rotation(x) => x.grid = r.unwrap_or_else(|| x.grid.clone()),
            }
            any = true;
        }
    }
    for g in &mut out.glues {
        if let Some(x) = baked(&g.grid) {
            Arc::make_mut(g).grid = x;
            any = true;
        }
    }
    any.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserted_key_list() {
        let k = inserted_keys(&[-30.0, 0.0, 30.0], 5);
        let v: Vec<f32> = k.iter().map(|x| x.0).collect();
        assert_eq!(
            v,
            vec![-30.0, -25.0, -20.0, -15.0, -10.0, -5.0, 0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0]
        );
        assert_eq!(k[7], (5.0, 1, 1.0 / 6.0));
    }

    #[test]
    fn ellipse_on_a_circle() {
        // Points on the unit circle at 0°, 90°, 180°: E(45°) is on it too.
        let p = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]];
        let e = ellipse_point([0.0, 90.0, 180.0], p, 45.0).unwrap();
        let r = std::f32::consts::FRAC_1_SQRT_2;
        assert!((e[0] - r).abs() < 1e-5 && (e[1] - r).abs() < 1e-5, "{e:?}");
        assert!(ellipse_legal(&[-30.0, 0.0, 30.0], 1.0));
        assert!(!ellipse_legal(&[0.0, 1.0, 2.0], 180.0));
    }

    #[test]
    fn sns_passes_through_points() {
        let p = [[0.0, 0.0], [1.0, 1.0], [2.0, 0.0], [3.0, 1.0]];
        for i in 0..3 {
            let a = sns_point(&p, i, 0.0);
            let b = sns_point(&p, i, 1.0);
            assert!((a[0] - p[i][0]).abs() < 1e-6 && (b[1] - p[i + 1][1]).abs() < 1e-6);
        }
    }
}
