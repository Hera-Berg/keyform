//! Keyform interpolation and blend shapes.
//!
//! spec: 30-algorithms/keyform-interpolation.md, 30-algorithms/blend-shapes.md

use std::borrow::Borrow;

use keyform_model::*;

/// Up to two (key index, weight) pairs selected on one binding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub keys: [(usize, f32); 2],
    pub len: usize,
}

impl Selection {
    fn one(k: usize) -> Self {
        Self {
            keys: [(k, 1.0), (0, 0.0)],
            len: 1,
        }
    }
    fn two(a: usize, b: usize, t: f32) -> Self {
        Self {
            keys: [(a, 1.0 - t), (b, t)],
            len: 2,
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (usize, f32)> + '_ {
        self.keys[..self.len].iter().copied()
    }
}

/// Per-binding key selection. spec: keyform-interpolation.md §1
pub fn select_keys(param: &Parameter, keys: &[f32], v: f32) -> Selection {
    if keys.is_empty() {
        return Selection::one(0);
    }
    let es = param.snap_epsilon;
    let ei = param.interp_epsilon();
    if !param.repeat {
        // §1.1: largest i with K[i] < v + εₛ.
        let Some(i) = keys.iter().rposition(|&k| k < v + es) else {
            return Selection::one(0);
        };
        if i + 1 < keys.len() && v >= keys[i] + ei {
            let t = (v - keys[i]) / (keys[i + 1] - keys[i]);
            return Selection::two(i, i + 1, t);
        }
        return Selection::one(i);
    }

    // §1.2: periodic extension over the keys inside [min, max].
    let (lo, hi) = (param.min.min(param.max), param.max.max(param.min));
    let in_range: Vec<usize> = (0..keys.len())
        .filter(|&i| keys[i] >= lo && keys[i] <= hi)
        .collect();
    let m = in_range.len() as i64;
    let period = hi - lo;
    if m == 0 || period <= 0.0 {
        let mut p = param.clone();
        p.repeat = false;
        return select_keys(&p, keys, v);
    }
    let value = |j: i64| -> f32 {
        let w = j.div_euclid(m);
        keys[in_range[j.rem_euclid(m) as usize]] + w as f32 * period
    };
    let key_of = |j: i64| in_range[j.rem_euclid(m) as usize];
    // Start a period below v and walk up to the largest j with value(j) < v + εₛ.
    let w0 = ((v - keys[in_range[0]]) / period).floor() as i64 - 1;
    let mut j = w0 * m;
    let limit = j + 4 * m;
    while j + 1 < limit && value(j + 1) < v + es {
        j += 1;
    }
    let (a, b) = (value(j), value(j + 1));
    if v >= a + ei && b > a {
        let t = (v - a) / (b - a);
        Selection::two(key_of(j), key_of(j + 1), t)
    } else {
        Selection::one(key_of(j))
    }
}

/// "Outside" test for one binding. spec: keyform-interpolation.md §2
pub fn is_outside(param: &Parameter, keys: &[f32], v: f32) -> bool {
    match (param.repeat, keys.first(), keys.last()) {
        (false, Some(&first), Some(&last)) => {
            let es = param.snap_epsilon;
            !(first - es < v && v < last + es)
        }
        _ => false,
    }
}

/// Field-wise blending rules. spec: keyform-interpolation.md §4
pub trait Blend: Clone {
    /// A zeroed form shaped like `self`.
    fn zeroed(&self) -> Self;
    /// `self += w · other`.
    fn add_scaled(&mut self, other: &Self, w: f32);
    /// Draw-order truncation after accumulation.
    fn finish(&mut self) {}
    /// Clamp to valid ranges (object-model.md §7), applied after blend shapes.
    fn clamp(&mut self) {}
}

fn add_color(a: &mut Color, b: &Color, w: f32) {
    for i in 0..4 {
        a[i] += w * b[i];
    }
}

fn add_positions(a: &mut [f32], b: &[f32], w: f32) {
    for (x, y) in a.iter_mut().zip(b) {
        *x += w * y;
    }
}

/// ⌊d + 0.001⌋ toward zero, sign-aware.
pub fn truncate_draw_order(d: f32) -> f32 {
    (d + 0.001 * d.signum()).trunc()
}

fn clamp_color(c: &mut Color) {
    c.iter_mut().for_each(|x| *x = x.clamp(0.0, 1.0));
}

impl Blend for PartForm {
    fn zeroed(&self) -> Self {
        Self {
            draw_order: 0.0,
            opacity: 0.0,
            multiply: [0.0; 4],
            screen: [0.0; 4],
        }
    }
    fn add_scaled(&mut self, o: &Self, w: f32) {
        self.draw_order += w * o.draw_order;
        self.opacity += w * o.opacity;
        add_color(&mut self.multiply, &o.multiply, w);
        add_color(&mut self.screen, &o.screen, w);
    }
    fn finish(&mut self) {
        self.draw_order = truncate_draw_order(self.draw_order);
    }
    fn clamp(&mut self) {
        self.draw_order = self.draw_order.clamp(0.0, 1000.0);
        self.opacity = self.opacity.clamp(0.0, 1.0);
        clamp_color(&mut self.multiply);
        clamp_color(&mut self.screen);
    }
}

impl Blend for ArtMeshForm {
    fn zeroed(&self) -> Self {
        Self {
            positions: vec![0.0; self.positions.len()],
            draw_order: 0.0,
            opacity: 0.0,
            multiply: [0.0; 4],
            screen: [0.0; 4],
        }
    }
    fn add_scaled(&mut self, o: &Self, w: f32) {
        add_positions(&mut self.positions, &o.positions, w);
        self.draw_order += w * o.draw_order;
        self.opacity += w * o.opacity;
        add_color(&mut self.multiply, &o.multiply, w);
        add_color(&mut self.screen, &o.screen, w);
    }
    fn finish(&mut self) {
        self.draw_order = truncate_draw_order(self.draw_order);
    }
    fn clamp(&mut self) {
        self.draw_order = self.draw_order.clamp(0.0, 1000.0);
        self.opacity = self.opacity.clamp(0.0, 1.0);
        clamp_color(&mut self.multiply);
        clamp_color(&mut self.screen);
    }
}

impl Blend for WarpForm {
    fn zeroed(&self) -> Self {
        Self {
            positions: vec![0.0; self.positions.len()],
            opacity: 0.0,
            multiply: [0.0; 4],
            screen: [0.0; 4],
        }
    }
    fn add_scaled(&mut self, o: &Self, w: f32) {
        add_positions(&mut self.positions, &o.positions, w);
        self.opacity += w * o.opacity;
        add_color(&mut self.multiply, &o.multiply, w);
        add_color(&mut self.screen, &o.screen, w);
    }
    fn clamp(&mut self) {
        self.opacity = self.opacity.clamp(0.0, 1.0);
        clamp_color(&mut self.multiply);
        clamp_color(&mut self.screen);
    }
}

impl Blend for RotationForm {
    fn zeroed(&self) -> Self {
        Self {
            angle: 0.0,
            origin: [0.0; 2],
            scale: 0.0,
            reflect_x: false,
            reflect_y: false,
            opacity: 0.0,
            multiply: [0.0; 4],
            screen: [0.0; 4],
        }
    }
    fn add_scaled(&mut self, o: &Self, w: f32) {
        // Angles add linearly, no shortest-arc logic (§4).
        self.angle += w * o.angle;
        self.origin[0] += w * o.origin[0];
        self.origin[1] += w * o.origin[1];
        self.scale += w * o.scale;
        if w > 0.0 {
            self.reflect_x |= o.reflect_x;
            self.reflect_y |= o.reflect_y;
        }
        self.opacity += w * o.opacity;
        add_color(&mut self.multiply, &o.multiply, w);
        add_color(&mut self.screen, &o.screen, w);
    }
    fn clamp(&mut self) {
        self.angle = self.angle.clamp(-3600.0, 3600.0);
        self.scale = self.scale.clamp(0.0001, 100.0);
        self.opacity = self.opacity.clamp(0.0, 1.0);
        clamp_color(&mut self.multiply);
        clamp_color(&mut self.screen);
    }
}

impl Blend for GlueForm {
    fn zeroed(&self) -> Self {
        Self { intensity: 0.0 }
    }
    fn add_scaled(&mut self, o: &Self, w: f32) {
        self.intensity += w * o.intensity;
    }
    fn clamp(&mut self) {
        self.intensity = self.intensity.clamp(0.0, 1.0);
    }
}

/// Result of interpolating one object.
pub struct Interpolated<F> {
    pub form: F,
    pub outside: bool,
}

/// Interpolates `grid` at parameter values `values`, then applies blend
/// shapes. spec: keyform-interpolation.md §3–5, blend-shapes.md §2
pub fn interpolate<F: Blend>(
    params: &[impl Borrow<Parameter>],
    values: &[f32],
    grid: &KeyformGrid<F>,
    blend: &BlendShapes<F>,
) -> Option<Interpolated<F>> {
    let first = grid.keyforms.first()?;
    let mut outside = false;
    let mut sels = Vec::with_capacity(grid.bindings.len());
    for b in &grid.bindings {
        let p: &Parameter = params.get(b.parameter)?.borrow();
        let v = values.get(b.parameter).copied().unwrap_or(p.default);
        outside |= is_outside(p, &b.keys, v);
        sels.push(select_keys(p, &b.keys, v));
    }

    let mut form = if grid.bindings.is_empty() {
        first.clone()
    } else {
        let mut acc = first.zeroed();
        let mut idx = vec![0usize; sels.len()];
        let mut pick = vec![0usize; sels.len()];
        // Odometer over the tensor product of selections.
        'outer: loop {
            let mut w = 1.0f32;
            for (j, s) in sels.iter().enumerate() {
                let (k, wk) = s.keys[pick[j]];
                idx[j] = k;
                w *= wk;
            }
            if w != 0.0 {
                let cell = grid.cell_index(&idx)?;
                acc.add_scaled(grid.keyforms.get(cell)?, w);
            }
            for j in 0..sels.len() {
                pick[j] += 1;
                if pick[j] < sels[j].len {
                    continue 'outer;
                }
                pick[j] = 0;
            }
            break;
        }
        acc.finish();
        acc
    };

    if !blend.is_empty() {
        apply_blend_shapes(params, values, grid, blend, &mut form);
        form.finish();
        form.clamp();
    }
    Some(Interpolated { form, outside })
}

/// The grid keyform at the key nearest each bound parameter's default.
/// spec: blend-shapes.md §1
pub fn base_keyform<'g, F>(
    params: &[impl Borrow<Parameter>],
    grid: &'g KeyformGrid<F>,
) -> Option<&'g F> {
    let idx: Vec<usize> = grid
        .bindings
        .iter()
        .map(|b| {
            let d = params.get(b.parameter).map_or(0.0, |p| p.borrow().default);
            let mut best = 0;
            for (i, k) in b.keys.iter().enumerate() {
                if (k - d).abs() < (b.keys[best] - d).abs() {
                    best = i;
                }
            }
            best
        })
        .collect();
    grid.keyforms.get(grid.cell_index(&idx)?)
}

fn apply_blend_shapes<F: Blend>(
    params: &[impl Borrow<Parameter>],
    values: &[f32],
    grid: &KeyformGrid<F>,
    blend: &BlendShapes<F>,
    form: &mut F,
) {
    let Some(base) = base_keyform(params, grid) else {
        return;
    };
    // Blend-shape parameters with entries, in model parameter order.
    let mut bs_params: Vec<ParamIndex> = blend.shapes.iter().map(|s| s.parameter).collect();
    bs_params.sort_unstable();
    bs_params.dedup();
    let value_of = |p: ParamIndex| {
        values
            .get(p)
            .copied()
            .or_else(|| params.get(p).map(|q| q.borrow().default))
            .unwrap_or(0.0)
    };
    for p in bs_params {
        let Some(param) = params.get(p).map(Borrow::<Parameter>::borrow) else {
            continue;
        };
        // 1. Shape curve: entries plus (0, base), sorted by key.
        let mut curve: Vec<(f32, &F)> = blend
            .shapes
            .iter()
            .filter(|s| s.parameter == p)
            .map(|s| (s.key.unwrap_or(param.max), &s.form))
            .collect();
        curve.push((0.0, base));
        curve.sort_by(|a, b| a.0.total_cmp(&b.0));
        if curve.len() < 2 {
            continue;
        }
        // 2. Shape at the current value, no extrapolation.
        let v = value_of(p);
        let lower = curve.iter().rposition(|(k, _)| *k <= v);
        let upper = curve.iter().position(|(k, _)| *k > v);
        let shape = match (lower, upper) {
            (Some(l), Some(u)) => {
                let (kl, fl) = curve[l];
                let (ku, fu) = curve[u];
                let t = (v - kl) / (ku - kl);
                let mut s = fl.zeroed();
                s.add_scaled(fl, 1.0 - t);
                s.add_scaled(fu, t);
                s
            }
            (Some(i), None) | (None, Some(i)) => curve[i].1.clone(),
            (None, None) => continue,
        };
        // 3. Constraint weight: minimum over constraint parameters.
        let c = constraint_weight(params, &value_of, blend, p);
        // 4. F += c·(S − base).
        if c != 0.0 {
            form.add_scaled(&shape, c);
            form.add_scaled(base, -c);
        }
    }
}

fn constraint_weight<F>(
    _params: &[impl Borrow<Parameter>],
    value_of: &dyn Fn(ParamIndex) -> f32,
    blend: &BlendShapes<F>,
    p: ParamIndex,
) -> f32 {
    let mut qs: Vec<ParamIndex> = blend
        .constraints
        .iter()
        .filter(|c| c.blend_parameter == p)
        .map(|c| c.constraint_parameter)
        .collect();
    if qs.is_empty() {
        return 1.0;
    }
    qs.sort_unstable();
    qs.dedup();
    let mut c = f32::INFINITY;
    for q in qs {
        let mut pts: Vec<(f32, f32)> = blend
            .constraints
            .iter()
            .filter(|k| k.blend_parameter == p && k.constraint_parameter == q)
            .map(|k| (k.value, k.weight))
            .collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        c = c.min(piecewise_linear(&pts, value_of(q)));
    }
    c
}

/// Piecewise-linear interpolation through sorted points, clamped at the ends.
pub fn piecewise_linear(pts: &[(f32, f32)], x: f32) -> f32 {
    match pts {
        [] => 1.0,
        [(_, y)] => *y,
        _ => {
            if x <= pts[0].0 {
                return pts[0].1;
            }
            for w in pts.windows(2) {
                let ((x0, y0), (x1, y1)) = (w[0], w[1]);
                if x <= x1 {
                    if x1 <= x0 {
                        return y1;
                    }
                    return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
                }
            }
            pts[pts.len() - 1].1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(min: f32, max: f32) -> Parameter {
        Parameter::new("P", min, max, 0.0)
    }

    fn sel(p: &Parameter, keys: &[f32], v: f32) -> Vec<(usize, f32)> {
        select_keys(p, keys, v).iter().collect()
    }

    // spec: keyform-interpolation.md §7
    #[test]
    fn spec_test_vectors() {
        let p = param(-30.0, 30.0);
        let k = [-30.0, 0.0, 30.0];
        assert_eq!(sel(&p, &k, 15.0), vec![(1, 0.5), (2, 0.5)]);
        assert_eq!(sel(&p, &k, 0.0012), vec![(1, 1.0)]);
        let s = sel(&p, &k, 0.0016);
        assert_eq!(s.len(), 2);
        assert!((s[1].1 - 0.0016 / 30.0).abs() < 1e-9);
        assert_eq!(sel(&p, &k, 30.0), vec![(2, 1.0)]);
        assert!(!is_outside(&p, &k, 30.0));

        let p2 = param(-1.0, 1.0);
        assert_eq!(sel(&p2, &[0.0, 1.0], -0.5), vec![(0, 1.0)]);
        assert!(is_outside(&p2, &[0.0, 1.0], -0.5));
    }

    #[test]
    fn repeat_parameter_wraps() {
        let mut p = param(-180.0, 180.0);
        p.repeat = true;
        let k = [-180.0, 0.0, 180.0];
        // v = 190: between the 180 keyform (as virtual key) and 0 (as 360), t = 10/180.
        let s = sel(&p, &k, 190.0);
        assert_eq!(s.len(), 2);
        assert!(s[0].0 == 2 || s[0].0 == 0, "{s:?}"); // coincident ±180 keys: either
        assert_eq!(s[1].0, 1);
        assert!((s[1].1 - 10.0 / 180.0).abs() < 1e-5, "{s:?}");
        assert!(!is_outside(&p, &k, 190.0));
        // Exactly on a key.
        assert_eq!(sel(&p, &k, 0.0), vec![(1, 1.0)]);
        // Far periods.
        let s = sel(&p, &k, 90.0 + 720.0);
        assert!((s[1].1 - 0.5).abs() < 1e-3, "{s:?}");
    }

    #[test]
    fn draw_order_truncation() {
        assert_eq!(truncate_draw_order(500.9999), 501.0);
        assert_eq!(truncate_draw_order(500.998), 500.0);
        assert_eq!(truncate_draw_order(-3.9995), -4.0);
    }

    fn mesh_form(x: f32, d: f32) -> ArtMeshForm {
        ArtMeshForm {
            positions: vec![x, 0.0],
            draw_order: d,
            ..ArtMeshForm::default()
        }
    }

    #[test]
    fn two_binding_tensor_product() {
        let params = vec![param(0.0, 1.0), param(0.0, 1.0)];
        let grid = KeyformGrid {
            bindings: vec![
                Binding::new(0, vec![0.0, 1.0]),
                Binding::new(1, vec![0.0, 1.0]),
            ],
            keyforms: vec![
                mesh_form(0.0, 0.0),
                mesh_form(10.0, 0.0),
                mesh_form(20.0, 0.0),
                mesh_form(30.0, 0.0),
            ],
        };
        let r = interpolate(&params, &[0.5, 0.25], &grid, &BlendShapes::default()).unwrap();
        // (0·.5·.75 + 10·.5·.75 + 20·.5·.25 + 30·.5·.25)
        assert!((r.form.positions[0] - 10.0).abs() < 1e-5);
        assert!(!r.outside);
    }

    #[test]
    fn blend_shape_adds_delta_with_constraint() {
        let mut bs = param(0.0, 1.0);
        bs.kind = ParameterKind::BlendShape;
        let params = vec![param(-1.0, 1.0), bs, param(0.0, 1.0)];
        let grid = KeyformGrid {
            bindings: vec![Binding::new(0, vec![-1.0, 1.0])],
            keyforms: vec![mesh_form(-10.0, 500.0), mesh_form(10.0, 500.0)],
        };
        // Base = keyform nearest default 0.0 → first key (tie keeps first).
        let blend = BlendShapes {
            shapes: vec![BlendShape {
                parameter: 1,
                key: Some(1.0),
                form: mesh_form(-6.0, 520.0),
            }],
            constraints: vec![
                BlendConstraint {
                    blend_parameter: 1,
                    constraint_parameter: 2,
                    value: 0.0,
                    weight: 1.0,
                },
                BlendConstraint {
                    blend_parameter: 1,
                    constraint_parameter: 2,
                    value: 1.0,
                    weight: 0.0,
                },
            ],
        };
        let r = interpolate(&params, &[1.0, 0.5, 0.0], &grid, &blend).unwrap();
        // F = 10 + 1·0.5·(−6 − (−10)) = 12; draw order 500 + 10 = 510.
        assert!(
            (r.form.positions[0] - 12.0).abs() < 1e-5,
            "{:?}",
            r.form.positions
        );
        assert_eq!(r.form.draw_order, 510.0);
        let r = interpolate(&params, &[1.0, 0.5, 0.5], &grid, &blend).unwrap();
        assert!((r.form.positions[0] - 11.0).abs() < 1e-5);
    }

    #[test]
    fn piecewise() {
        let pts = [(0.0, 1.0), (1.0, 0.0)];
        assert_eq!(piecewise_linear(&pts, -1.0), 1.0);
        assert_eq!(piecewise_linear(&pts, 0.25), 0.75);
        assert_eq!(piecewise_linear(&pts, 9.0), 0.0);
    }
}
