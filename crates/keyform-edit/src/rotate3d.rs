//! Apply 3D Rotation Expression: keyforms of an angle-parameter pair from a
//! front keyform, treating the object as a 3-D surface.
//! spec: 70-advanced/generators.md §1

use std::sync::Arc;

use keyform_eval::Evaluator;
use keyform_model::{DeformerKind, Model, ParamIndex};

use crate::keys::ObjectRef;

type P2 = [f32; 2];
type P3 = [f32; 3];

/// Zmin / Zmax of the perspective formula as multiples of the canvas
/// width (§1: "fixed multiples"; the reference's values aren't given).
// TODO(Q-0044): the reference's multiples.
pub const ZMIN_K: f32 = 1.0;
pub const ZMAX_K: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Order {
    /// "From X-direction to −Y-direction".
    #[default]
    Xy,
    Yx,
    /// "XY-directions simultaneously": one combined axis.
    Arbitrary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Update {
    #[default]
    All,
    X,
    Y,
    Corners,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Camera position on the canvas.
    pub camera: P2,
    /// Centre of rotation on the canvas.
    pub centre: P2,
    pub z_offset: f32,
    /// Perspective strength 0–100; `None` = orthographic.
    pub perspective: Option<f32>,
    /// Range of angle (degrees) for X and Y.
    pub max_angle: P2,
    pub order: Order,
    /// Default depth (canvas pixels) when not estimated.
    pub default_depth: f32,
    pub update: Update,
}

impl Settings {
    /// Defaults for a canvas of `w` × `h`: camera and centre at the middle.
    pub fn for_canvas(w: f32, h: f32) -> Self {
        Self {
            camera: [w / 2.0, h / 2.0],
            centre: [w / 2.0, h / 2.0],
            z_offset: 0.0,
            perspective: Some(50.0),
            max_angle: [30.0, 30.0],
            order: Order::Xy,
            default_depth: w * 0.05,
            update: Update::All,
        }
    }
}

/// Camera distance d for perspective strength p ∈ [0, 1] (§1).
pub fn camera_distance(width: f32, p: f32) -> f32 {
    let (zmin, zmax) = (ZMIN_K * width, ZMAX_K * width);
    zmax / (p * (zmax - zmin) / zmin + 1.0)
}

/// Un-projects canvas point `q` at depth `z`: Q = (C + (1 − z/d)(q − C), z).
pub fn unproject(q: P2, z: f32, c: P2, d: Option<f32>) -> P3 {
    let f = d.map_or(1.0, |d| 1.0 - z / d);
    [c[0] + f * (q[0] - c[0]), c[1] + f * (q[1] - c[1]), z]
}

/// Projects: q = C + (x − C, y − C)·d/(d − z).
pub fn project(p: P3, c: P2, d: Option<f32>) -> P2 {
    let f = d.map_or(1.0, |d| d / (d - p[2]).max(1e-3));
    [c[0] + (p[0] - c[0]) * f, c[1] + (p[1] - c[1]) * f]
}

fn rot_y_axis(p: P3, o: P3, a: f32) -> P3 {
    // About the vertical axis: positive angles move the near side right.
    let (s, c) = a.to_radians().sin_cos();
    let (x, z) = (p[0] - o[0], p[2] - o[2]);
    [o[0] + x * c + z * s, p[1], o[2] - x * s + z * c]
}

fn rot_x_axis(p: P3, o: P3, a: f32) -> P3 {
    // About the horizontal axis: positive angles move the near side up.
    let (s, c) = a.to_radians().sin_cos();
    let (y, z) = (p[1] - o[1], p[2] - o[2]);
    [p[0], o[1] + y * c - z * s, o[2] + y * s + z * c]
}

/// Rotates `p` about `o` by the X-direction angle `ax` (vertical axis) and
/// the Y-direction angle `ay` (horizontal axis) in the chosen order.
// TODO(Q-0044): sign conventions of the angle parameters.
pub fn rotate(p: P3, o: P3, ax: f32, ay: f32, order: Order) -> P3 {
    match order {
        Order::Xy => rot_x_axis(rot_y_axis(p, o, ax), o, ay),
        Order::Yx => rot_y_axis(rot_x_axis(p, o, ay), o, ax),
        Order::Arbitrary => {
            // One rotation about the axis (ay, ax, 0) by |(ax, ay)|.
            let ang = ax.hypot(ay);
            if ang < 1e-6 {
                return p;
            }
            let k = [ay / ang, ax / ang, 0.0];
            let v = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
            let (s, c) = ang.to_radians().sin_cos();
            let cross = [
                k[1] * v[2] - k[2] * v[1],
                k[2] * v[0] - k[0] * v[2],
                k[0] * v[1] - k[1] * v[0],
            ];
            let dot = k[0] * v[0] + k[1] * v[1] + k[2] * v[2];
            let r: Vec<f32> = (0..3)
                .map(|i| v[i] * c + cross[i] * s + k[i] * dot * (1.0 - c))
                .collect();
            [o[0] + r[0], o[1] + r[1], o[2] + r[2]]
        }
    }
}

/// Depth estimation (§1): the z minimising the distance between the
/// rotated front point and the side point (golden-section search, a 1-D
/// stand-in for Nelder–Mead).
pub fn estimate_depth(
    front: P2,
    side: P2,
    ax: f32,
    ay: f32,
    s: &Settings,
    d: Option<f32>,
    range: f32,
) -> f32 {
    let o = [s.centre[0], s.centre[1], s.z_offset];
    let err = |z: f32| {
        let q = project(
            rotate(unproject(front, z, s.camera, d), o, ax, ay, s.order),
            s.camera,
            d,
        );
        (q[0] - side[0]).hypot(q[1] - side[1])
    };
    let (mut a, mut b) = (-range, range);
    let g = 0.618_034_f32;
    for _ in 0..60 {
        let (x1, x2) = (b - g * (b - a), a + g * (b - a));
        if err(x1) < err(x2) {
            b = x2;
        } else {
            a = x1;
        }
    }
    (a + b) / 2.0
}

fn positions(m: &Model, o: ObjectRef, cell: usize) -> Option<Vec<f32>> {
    match o {
        ObjectRef::ArtMesh(i) => m
            .art_meshes
            .get(i)?
            .grid
            .keyforms
            .get(cell)
            .map(|k| k.positions.clone()),
        ObjectRef::Deformer(i) => match &m.deformers.get(i)?.kind {
            DeformerKind::Warp(w) => w.grid.keyforms.get(cell).map(|k| k.positions.clone()),
            DeformerKind::Rotation(_) => None,
        },
        _ => None,
    }
}

fn set_positions(m: &mut Model, o: ObjectRef, cell: usize, pts: Vec<f32>) {
    match o {
        ObjectRef::ArtMesh(i) => {
            if let Some(k) = Arc::make_mut(&mut m.art_meshes[i])
                .grid
                .keyforms
                .get_mut(cell)
            {
                k.positions = pts;
            }
        }
        ObjectRef::Deformer(i) => {
            if let DeformerKind::Warp(w) = &mut Arc::make_mut(&mut m.deformers[i]).kind {
                if let Some(k) = w.grid.keyforms.get_mut(cell) {
                    k.positions = pts;
                }
            }
        }
        _ => {}
    }
}

fn parent(m: &Model, o: ObjectRef) -> Option<usize> {
    match o {
        ObjectRef::ArtMesh(i) => m.art_meshes.get(i)?.target,
        ObjectRef::Deformer(i) => m.deformers.get(i)?.target,
        _ => None,
    }
}

/// The object's bindings (parameter, keys).
fn bindings(m: &Model, o: ObjectRef) -> Vec<(ParamIndex, Vec<f32>)> {
    let f =
        |b: &[keyform_model::Binding]| b.iter().map(|x| (x.parameter, x.keys.clone())).collect();
    match o {
        ObjectRef::ArtMesh(i) => m.art_meshes.get(i).map(|a| f(&a.grid.bindings)),
        ObjectRef::Deformer(i) => m.deformers.get(i).and_then(|d| match &d.kind {
            DeformerKind::Warp(w) => Some(f(&w.grid.bindings)),
            DeformerKind::Rotation(_) => None,
        }),
        _ => None,
    }
    .unwrap_or_default()
}

/// Generates the keyforms of `o` (art mesh or warp deformer) on parameters
/// `px` (X direction) and `py` (Y direction). Each must have keys at min,
/// default and max; other bindings are taken at their defaults. `side` is
/// a user-drawn keyform cell for depth estimation. Returns the number of
/// keyforms written.
pub fn generate(
    m: &mut Model,
    o: ObjectRef,
    px: ParamIndex,
    py: ParamIndex,
    s: &Settings,
    side: Option<usize>,
) -> Result<usize, String> {
    let bs = bindings(m, o);
    let (Some(bx), Some(by)) = (
        bs.iter().position(|b| b.0 == px),
        bs.iter().position(|b| b.0 == py),
    ) else {
        return Err("Would you like to create a keyform by adding 3 keys?".into());
    };
    let defaults: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
    let snap =
        |p: ParamIndex, v: f32| (v - defaults[p]).abs() < m.parameters[p].snap_epsilon.max(1e-4);
    if !bs[bx].1.iter().any(|&k| snap(px, k)) || !bs[by].1.iter().any(|&k| snap(py, k)) {
        return Err("The default value does not have a key…".into());
    }
    // Cells, decoded (first binding fastest).
    let n: usize = bs.iter().map(|b| b.1.len().max(1)).product();
    let decode = |cell: usize| -> Vec<f32> {
        let mut rest = cell;
        bs.iter()
            .map(|(_, keys)| {
                let k = keys.len().max(1);
                let i = rest % k;
                rest /= k;
                keys.get(i).copied().unwrap_or(0.0)
            })
            .collect()
    };
    let others_default = |vals: &[f32]| {
        bs.iter()
            .enumerate()
            .all(|(j, b)| j == bx || j == by || snap(b.0, vals[j]))
    };
    let front_cell = (0..n)
        .find(|&c| {
            let v = decode(c);
            others_default(&v) && snap(px, v[bx]) && snap(py, v[by])
        })
        .ok_or("The default value does not have a key…")?;
    let width = m.canvas.width as f32;
    let d = s
        .perspective
        .map(|p| camera_distance(width, (p / 100.0).clamp(0.0, 1.0)));
    let values_of = |vals: &[f32]| {
        let mut v = defaults.clone();
        for (j, b) in bs.iter().enumerate() {
            v[b.0] = vals[j];
        }
        v
    };
    // Front points on the canvas.
    let par = parent(m, o);
    let map_at = |m: &Model, vals: &[f32]| {
        par.and_then(|p| {
            Evaluator::new(m)
                .evaluate(&values_of(vals))
                .deformers
                .get(p)
                .and_then(|x| x.map.clone())
        })
    };
    let front_vals = decode(front_cell);
    let pm = map_at(m, &front_vals);
    let to_canvas =
        |pm: &Option<keyform_eval::DeformerMap>, q: P2| pm.as_ref().map_or(q, |p| p.map(q));
    let front: Vec<P2> = positions(m, o, front_cell)
        .ok_or("no keyform")?
        .chunks_exact(2)
        .map(|c| to_canvas(&pm, [c[0], c[1]]))
        .collect();
    let angle = |p: ParamIndex, k: f32| {
        let q = &m.parameters[p];
        let max = if k >= 0.0 { q.max } else { -q.min };
        let a = if p == px {
            s.max_angle[0]
        } else {
            s.max_angle[1]
        };
        if max.abs() < 1e-6 {
            0.0
        } else {
            (k - q.default) / max * a
        }
    };
    // Depths: estimated from the side keyform, else the default.
    let depths: Vec<f32> = match side {
        Some(sc) if sc != front_cell => {
            let sv = decode(sc);
            if !others_default(&sv) {
                return Err("Cannot create keyform. Please select keys from parameters other than the X and Y direction".into());
            }
            let spm = map_at(m, &sv);
            let side_pts: Vec<P2> = positions(m, o, sc)
                .ok_or("no keyform")?
                .chunks_exact(2)
                .map(|c| to_canvas(&spm, [c[0], c[1]]))
                .collect();
            let (ax, ay) = (angle(px, sv[bx]), angle(py, sv[by]));
            front
                .iter()
                .zip(&side_pts)
                .map(|(f, sp)| estimate_depth(*f, *sp, ax, ay, s, d, width))
                .collect()
        }
        Some(_) => {
            return Err("Depth estimation requires a keyform different than the front side…".into())
        }
        None => vec![s.default_depth + s.z_offset; front.len()],
    };
    let o3 = [s.centre[0], s.centre[1], s.z_offset];
    let mut writes = Vec::new();
    for c in 0..n {
        let v = decode(c);
        if c == front_cell || !others_default(&v) || Some(c) == side {
            continue;
        }
        let (on_x, on_y) = (!snap(px, v[bx]), !snap(py, v[by]));
        let wanted = match s.update {
            Update::All => true,
            Update::X => on_x && !on_y,
            Update::Y => on_y && !on_x,
            Update::Corners => on_x && on_y,
        };
        if !wanted {
            continue;
        }
        let (ax, ay) = (angle(px, v[bx]), angle(py, v[by]));
        let pm = map_at(m, &v);
        let mut local = Vec::with_capacity(front.len() * 2);
        for (q, z) in front.iter().zip(&depths) {
            let r = project(
                rotate(unproject(*q, *z, s.camera, d), o3, ax, ay, s.order),
                s.camera,
                d,
            );
            let l = match &pm {
                Some(p) => p
                    .inverse(r)
                    .ok_or("the point cannot be mapped into the parent deformer")?,
                None => r,
            };
            local.extend_from_slice(&l);
        }
        writes.push((c, local));
    }
    let count = writes.len();
    for (c, pts) in writes {
        set_positions(m, o, c, pts);
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_round_trip_and_rotation() {
        let c = [500.0, 500.0];
        let d = Some(camera_distance(1000.0, 0.5));
        let q = [620.0, 410.0];
        let p = unproject(q, 40.0, c, d);
        let back = project(p, c, d);
        assert!((back[0] - q[0]).abs() < 1e-3 && (back[1] - q[1]).abs() < 1e-3);
        // p = 0 → the flattest perspective (d = Zmax).
        assert_eq!(camera_distance(1000.0, 0.0), ZMAX_K * 1000.0);
        // A point in front of the centre moves right for a positive X angle.
        let o = [500.0, 500.0, 0.0];
        let r = rotate([500.0, 500.0, 100.0], o, 30.0, 0.0, Order::Xy);
        assert!(r[0] > 500.0 && (r[0] - 550.0).abs() < 1e-2);
        // Combined axis with one angle equals the single rotation.
        let a = rotate([520.0, 470.0, 60.0], o, 0.0, 20.0, Order::Arbitrary);
        let b = rotate([520.0, 470.0, 60.0], o, 0.0, 20.0, Order::Xy);
        assert!(
            (a[1] - b[1]).abs() < 1e-3 && (a[2] - b[2]).abs() < 1e-3,
            "{a:?} {b:?}"
        );
        // Depth estimation recovers a known depth.
        let s = Settings::for_canvas(1000.0, 1000.0);
        let z = 80.0;
        let side = project(
            rotate(
                unproject(q, z, s.camera, d),
                [500.0, 500.0, 0.0],
                25.0,
                0.0,
                s.order,
            ),
            s.camera,
            d,
        );
        let est = estimate_depth(q, side, 25.0, 0.0, &s, d, 1000.0);
        assert!((est - z).abs() < 0.5, "{est}");
    }
}
