//! Auto Generation of Face Motion: face deformers (§3.1, exact rules) and
//! face motion from Keyform's own predictor (§3.2: the reference's ML
//! models are not reproduced; Keyform rotates each feature as a 3-D
//! surface at a feature depth with [`crate::rotate3d`]).
//! spec: 70-advanced/generators.md §3

use keyform_eval::Evaluator;
use keyform_model::{Model, PartChild, PartIndex};

use crate::keys::ObjectRef;
use crate::objects::{self, NewDeformer};
use crate::rotate3d;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    Face,
    EyeL,
    EyeR,
    EyeballL,
    EyeballR,
    EyebrowL,
    EyebrowR,
    EarL,
    EarR,
    Mouth,
    Nose,
}

impl Feature {
    pub const ALL: [Feature; 11] = [
        Feature::Face,
        Feature::EyeL,
        Feature::EyeR,
        Feature::EyeballL,
        Feature::EyeballR,
        Feature::EyebrowL,
        Feature::EyebrowR,
        Feature::EarL,
        Feature::EarR,
        Feature::Mouth,
        Feature::Nose,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Feature::Face => "Face",
            Feature::EyeL => "Eye L",
            Feature::EyeR => "Eye R",
            Feature::EyeballL => "Eyeball L",
            Feature::EyeballR => "Eyeball R",
            Feature::EyebrowL => "Eyebrow L",
            Feature::EyebrowR => "Eyebrow R",
            Feature::EarL => "Ear L",
            Feature::EarR => "Ear R",
            Feature::Mouth => "Mouth",
            Feature::Nose => "Nose",
        }
    }

    /// Margin constants (k, c) of the warp rectangle (§3.1 table).
    pub fn margin(self) -> (f32, f32) {
        match self {
            Feature::Face => (1.1, 0.25),
            Feature::EyeL | Feature::EyeR => (1.13, 0.4),
            Feature::EyeballL | Feature::EyeballR => (0.98, 0.14),
            Feature::EyebrowL | Feature::EyebrowR => (0.87, 0.98),
            Feature::EarL | Feature::EarR => (0.85, 0.3),
            Feature::Mouth => (0.95, 0.43),
            Feature::Nose => (1.1, 0.14),
        }
    }

    /// Default Bezier divisions (§3.1).
    pub fn bezier(self) -> (u32, u32) {
        match self {
            Feature::Face => (2, 3),
            Feature::EyeL | Feature::EyeR => (3, 3),
            Feature::Mouth => (3, 2),
            _ => (2, 2),
        }
    }

    /// Depth of the feature in front of the face plane, as a fraction of
    /// the scale reference (Keyform's own predictor, not the reference's).
    pub fn depth(self) -> f32 {
        match self {
            Feature::Face => 0.0,
            Feature::EyeL | Feature::EyeR | Feature::EyeballL | Feature::EyeballR => 0.15,
            Feature::EyebrowL | Feature::EyebrowR => 0.2,
            Feature::EarL | Feature::EarR => -0.6,
            Feature::Mouth => 0.25,
            Feature::Nose => 0.45,
        }
    }
}

/// Margin factor m(x) = ((x/s)·k + c − x/s) / (x/s) / 2 (§3.1).
pub fn margin(x: f32, s: f32, k: f32, c: f32) -> f32 {
    let r = (x / s.max(1e-6)).max(1e-6);
    (r * k + c - r) / r / 2.0
}

/// All art meshes in part `p`, recursively.
pub fn meshes_in(m: &Model, p: PartIndex) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = vec![p];
    while let Some(q) = stack.pop() {
        let Some(part) = m.parts.get(q) else { continue };
        for c in &part.children {
            match *c {
                PartChild::ArtMesh(a) => out.push(a),
                PartChild::Part(x) => stack.push(x),
                _ => {}
            }
        }
    }
    out
}

/// Canvas bounding box (min, max) of meshes at `values`.
fn canvas_box(m: &Model, meshes: &[usize], values: &[f32]) -> Option<([f32; 2], [f32; 2])> {
    let frame = Evaluator::new(m).evaluate(values);
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for &a in meshes {
        for c in frame.meshes.get(a)?.positions.chunks_exact(2) {
            for k in 0..2 {
                lo[k] = lo[k].min(c[k]);
                hi[k] = hi[k].max(c[k]);
            }
        }
    }
    lo[0].is_finite().then_some((lo, hi))
}

/// Bounding box of meshes in deformer `parent`'s local space at `values`.
fn local_box(
    m: &Model,
    meshes: &[usize],
    parent: Option<usize>,
    values: &[f32],
) -> Option<([f32; 2], [f32; 2])> {
    let frame = Evaluator::new(m).evaluate(values);
    let map = parent.and_then(|p| frame.deformers.get(p).and_then(|x| x.map.clone()));
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for &a in meshes {
        for c in frame.meshes.get(a)?.positions.chunks_exact(2) {
            let q = match &map {
                Some(mp) => mp.inverse([c[0], c[1]])?,
                None => [c[0], c[1]],
            };
            for k in 0..2 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    lo[0].is_finite().then_some((lo, hi))
}

/// Creates one warp named `name` in part `part` under `parent`, sized to
/// `meshes` with feature margins, and re-parents `meshes` to it.
#[allow(clippy::too_many_arguments)]
fn warp_for(
    m: &mut Model,
    name: &str,
    part: PartIndex,
    parent: Option<usize>,
    meshes: &[usize],
    (k, c): (f32, f32),
    s: f32,
    divisions: (u32, u32),
    bezier: (u32, u32),
    values: &[f32],
    move_meshes: bool,
) -> Result<usize, String> {
    let (clo, chi) = canvas_box(m, meshes, values).ok_or("the feature has no art meshes")?;
    let (lo, hi) = local_box(m, meshes, parent, values)
        .ok_or("the feature cannot be mapped into its parent")?;
    let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
    let (mx, my) = (
        margin(chi[0] - clo[0], s, k, c),
        margin(chi[1] - clo[1], s, k, c),
    );
    let rect = (
        lo[0] - w * mx,
        lo[1] - h * my,
        w * (1.0 + 2.0 * mx),
        h * (1.0 + 2.0 * my),
    );
    let nd = NewDeformer {
        name: name.to_owned(),
        part: Some(part),
        parent,
        children: &[],
        values,
    };
    let d = objects::create_warp_deformer_in(m, &nd, divisions.0, divisions.1, bezier, Some(rect))?;
    if move_meshes {
        for &a in meshes {
            objects::change_parent(m, ObjectRef::ArtMesh(a), Some(d), values)?;
        }
    }
    Ok(d)
}

/// Generate Face Deformer (§3.1). `parts[i]` is the part of
/// `Feature::ALL[i]`; `divisions` the warp columns × rows (default 5 × 5).
/// Returns the deformer that carries each feature's meshes.
pub fn generate_deformers(
    m: &mut Model,
    parts: &[Option<PartIndex>; 11],
    divisions: (u32, u32),
    values: &[f32],
) -> Result<Vec<(Feature, usize)>, String> {
    let part = |f: Feature| parts[Feature::ALL.iter().position(|x| *x == f).unwrap_or(0)];
    let centre_x = |m: &Model, f: Feature| {
        let p = part(f)?;
        let (lo, hi) = canvas_box(m, &meshes_in(m, p), values)?;
        Some((lo[0] + hi[0]) / 2.0)
    };
    let s = match (centre_x(m, Feature::EyeL), centre_x(m, Feature::EyeR)) {
        (Some(a), Some(b)) if (a - b).abs() > 1e-3 => (a - b).abs(),
        _ => m.canvas.width as f32 / 10.0,
    };
    let mut out: Vec<(Feature, usize)> = Vec::new();
    let mut eye_warp = [None, None];
    for f in Feature::ALL {
        let Some(p) = part(f) else { continue };
        let meshes = meshes_in(m, p);
        if meshes.is_empty() {
            continue;
        }
        let name = format!("{} Warp", f.name());
        let parent = match f {
            Feature::EyeballL => eye_warp[0],
            Feature::EyeballR => eye_warp[1],
            _ => None,
        };
        let brow = matches!(f, Feature::EyebrowL | Feature::EyebrowR);
        let d = warp_for(
            m,
            &name,
            p,
            parent,
            &meshes,
            f.margin(),
            s,
            divisions,
            f.bezier(),
            values,
            !brow,
        )?;
        match f {
            Feature::EyeL => eye_warp[0] = Some(d),
            Feature::EyeR => eye_warp[1] = Some(d),
            _ => {}
        }
        if brow {
            // Warp → Position → Angle; the meshes go to the Angle deformer.
            let pos = warp_for(
                m,
                &format!("{} Position", f.name()),
                p,
                Some(d),
                &meshes,
                (0.83, 0.63),
                s,
                divisions,
                f.bezier(),
                values,
                false,
            )?;
            let ang = warp_for(
                m,
                &format!("{} Angle", f.name()),
                p,
                Some(pos),
                &meshes,
                (0.89, 0.23),
                s,
                divisions,
                f.bezier(),
                values,
                true,
            )?;
            out.push((f, ang));
        } else {
            out.push((f, d));
        }
    }
    Ok(out)
}

/// Generate Face Motion (§3.2) with Keyform's own predictor: each feature
/// deformer gets keys at min / default / max of `px` and `py` and keyforms
/// from rotating the feature as a plane at its feature depth about the
/// face centre (corners included).
pub fn generate_motion(
    m: &mut Model,
    warps: &[(Feature, usize)],
    px: usize,
    py: usize,
    max_angle: [f32; 2],
    values: &[f32],
) -> Result<usize, String> {
    let required = [Feature::Face, Feature::EyeL, Feature::EyeR, Feature::Mouth];
    for f in required {
        if !warps.iter().any(|w| w.0 == f) {
            return Err(format!("Please specify the {} deformer.", f.name()));
        }
    }
    // Face centre and scale from the Face deformer's canvas grid.
    let frame = Evaluator::new(m).evaluate(values);
    let face = warps
        .iter()
        .find(|w| w.0 == Feature::Face)
        .map(|w| w.1)
        .ok_or("no face")?;
    let Some(keyform_eval::DeformerMap::Warp(fw)) =
        frame.deformers.get(face).and_then(|x| x.map.clone())
    else {
        return Err("the Face deformer could not be evaluated".into());
    };
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for c in fw.points.chunks_exact(2) {
        for k in 0..2 {
            lo[k] = lo[k].min(c[k]);
            hi[k] = hi[k].max(c[k]);
        }
    }
    let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let s = (hi[0] - lo[0]).max(1.0);
    let mut n = 0;
    for &(f, d) in warps {
        for p in [px, py] {
            let q = m.parameters[p].clone();
            for v in [q.min, q.default, q.max] {
                match crate::keys::add_key(m, ObjectRef::Deformer(d), p, v) {
                    Ok(_) | Err(crate::keys::KeyError::TooClose) => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
        let mut st = rotate3d::Settings::for_canvas(m.canvas.width as f32, m.canvas.height as f32);
        st.centre = centre;
        st.camera = centre;
        st.max_angle = max_angle;
        st.default_depth = f.depth() * s;
        st.z_offset = -0.5 * s;
        n += rotate3d::generate(m, ObjectRef::Deformer(d), px, py, &st, None)?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margin_formula() {
        // x = s: m = (k + c − 1) / 2.
        assert!((margin(100.0, 100.0, 1.1, 0.25) - 0.175).abs() < 1e-6);
        assert!(
            (margin(50.0, 100.0, 0.98, 0.14) - ((0.5 * 0.98 + 0.14 - 0.5) / 0.5 / 2.0)).abs()
                < 1e-6
        );
    }
}

/// Directions of the face keys (Adjust Face Motion buttons ← → ↑ ↓).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    XMinus,
    XPlus,
    YPlus,
    YMinus,
}

/// A snapshot of a deformer's generated keyforms (cell → positions) and
/// its front cell, for re-adjusting without compounding.
#[derive(Clone, Debug, PartialEq)]
pub struct MotionSnapshot {
    pub deformer: usize,
    pub front: usize,
    /// (cell, X key sign, Y key sign, positions); signs −1 / 0 / +1.
    pub cells: Vec<(usize, i8, i8, Vec<f32>)>,
}

/// Snapshot of deformer `d`'s keyforms on (`px`, `py`) after generation.
pub fn snapshot(m: &Model, d: usize, px: usize, py: usize) -> Option<MotionSnapshot> {
    let keyform_model::DeformerKind::Warp(w) = &m.deformers.get(d)?.kind else {
        return None;
    };
    let bs = &w.grid.bindings;
    let (bx, by) = (
        bs.iter().position(|b| b.parameter == px)?,
        bs.iter().position(|b| b.parameter == py)?,
    );
    let (dx, dy) = (m.parameters[px].default, m.parameters[py].default);
    let n = w.grid.keyforms.len();
    let mut cells = Vec::new();
    let mut front = None;
    for c in 0..n {
        let mut rest = c;
        let mut keys = Vec::new();
        for b in bs {
            let k = b.keys.len().max(1);
            keys.push(b.keys.get(rest % k).copied().unwrap_or(0.0));
            rest /= k;
        }
        // Other bindings at their defaults only.
        if bs.iter().enumerate().any(|(j, b)| {
            j != bx && j != by && (keys[j] - m.parameters[b.parameter].default).abs() > 1e-4
        }) {
            continue;
        }
        let sign = |v: f32, d: f32| {
            if (v - d).abs() < 1e-4 {
                0
            } else if v > d {
                1
            } else {
                -1
            }
        };
        let (sx, sy) = (sign(keys[bx], dx), sign(keys[by], dy));
        if sx == 0 && sy == 0 {
            front = Some(c);
        }
        cells.push((c, sx, sy, w.grid.keyforms[c].positions.clone()));
    }
    Some(MotionSnapshot {
        deformer: d,
        front: front?,
        cells,
    })
}

/// Adjust Face Motion: rescales the snapshot's displacements from the
/// front keyform for the chosen directions. Movement (the mean
/// translation) is scaled by `movement`, the remaining shape change by
/// `deformation` (1.0 = as generated). Q-0044: movement scales
/// translation, deformation the shape change.
pub fn adjust(
    m: &mut Model,
    s: &MotionSnapshot,
    dirs: &[Direction],
    movement: f32,
    deformation: f32,
) {
    let Some(front) = s.cells.iter().find(|c| c.0 == s.front).map(|c| c.3.clone()) else {
        return;
    };
    let hit = |sx: i8, sy: i8| {
        dirs.iter().any(|d| match d {
            Direction::XMinus => sx < 0,
            Direction::XPlus => sx > 0,
            // Canvas y grows downward; a positive Y key looks up.
            Direction::YPlus => sy > 0,
            Direction::YMinus => sy < 0,
        })
    };
    let Some(def) = m.deformers.get_mut(s.deformer) else {
        return;
    };
    let keyform_model::DeformerKind::Warp(w) = &mut std::sync::Arc::make_mut(def).kind else {
        return;
    };
    for (c, sx, sy, pos) in &s.cells {
        if *c == s.front || !hit(*sx, *sy) {
            continue;
        }
        let n = (pos.len() / 2).max(1) as f32;
        let (mut mx, mut my) = (0.0, 0.0);
        for (p, f) in pos.chunks_exact(2).zip(front.chunks_exact(2)) {
            mx += (p[0] - f[0]) / n;
            my += (p[1] - f[1]) / n;
        }
        if let Some(k) = w.grid.keyforms.get_mut(*c) {
            k.positions = pos
                .chunks_exact(2)
                .zip(front.chunks_exact(2))
                .flat_map(|(p, f)| {
                    let (dx, dy) = (p[0] - f[0] - mx, p[1] - f[1] - my);
                    [
                        f[0] + mx * movement + dx * deformation,
                        f[1] + my * movement + dy * deformation,
                    ]
                })
                .collect();
        }
    }
}

#[cfg(test)]
mod adjust_tests {
    use super::*;

    #[test]
    fn movement_and_deformation_split() {
        // Front at (0,0),(2,0); a key moved by (1,0) and stretched by ±1.
        let front = vec![0.0, 0.0, 2.0, 0.0];
        let key = vec![0.0, 0.0, 4.0, 0.0]; // mean shift (1, 0), shape ±1
        let s = MotionSnapshot {
            deformer: 0,
            front: 0,
            cells: vec![(0, 0, 0, front.clone()), (1, 1, 0, key)],
        };
        let mut m = Model::default();
        let mut w = keyform_model::WarpForm::default();
        w.positions = front.clone();
        let mut grid = keyform_model::KeyformGrid::single(w.clone());
        grid.keyforms.push(w);
        m.deformers
            .push(std::sync::Arc::new(keyform_model::Deformer {
                info: keyform_model::ObjectInfo::new("W"),
                target: None,
                kind: keyform_model::DeformerKind::Warp(keyform_model::WarpDeformer {
                    cols: 1,
                    rows: 1,
                    quad: false,
                    grid,
                    blend_shapes: Default::default(),
                    bezier_nets: Vec::new(),
                    sway: Vec::new(),
                }),
            }));
        adjust(&mut m, &s, &[Direction::XPlus], 2.0, 0.0);
        let keyform_model::DeformerKind::Warp(w) = &m.deformers[0].kind else {
            panic!()
        };
        // Movement doubled (2, 0), no shape change.
        assert_eq!(w.grid.keyforms[1].positions, vec![2.0, 0.0, 4.0, 0.0]);
    }
}
