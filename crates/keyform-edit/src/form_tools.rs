//! Form tools: copy / paste / blend form, synthesize corners, flip, and
//! reverse keyforms on a parameter (batch reflect).
//! spec: 35-editing/form-tools.md §1–4, §5.2; 35-editing/keys.md §3

use std::sync::Arc;

use keyform_eval::{interpolate, Blend, DeformerMap, Evaluator, Frame};
use keyform_model::{BlendShapes, Color, DeformerKind, KeyformGrid, Model, ParamIndex, Parameter};

use crate::forms::{current_cell, parent_deformer, FormError};
use crate::keys::ObjectRef;

type V2 = [f32; 2];

// ------------------------------------------------------------- copy/paste

/// One copied form (§1).
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedForm {
    pub object: ObjectRef,
    /// The deformed (canvas) points: art mesh vertices (before glue), warp
    /// grid points, or a rotation deformer's origin. Parts have none.
    pub canvas: Vec<V2>,
    /// Per-point weight: the point selection weight, or 1 for every point.
    pub weights: Vec<f32>,
    /// Interpolated non-point properties.
    pub opacity: f32,
    pub multiply: Color,
    pub screen: Color,
    pub draw_order: Option<f32>,
    pub angle: Option<f32>,
    pub used_selection: bool,
}

fn kind(o: ObjectRef) -> u8 {
    match o {
        ObjectRef::ArtMesh(_) => 0,
        ObjectRef::Part(_) => 1,
        ObjectRef::Deformer(_) => 2,
        ObjectRef::Glue(_) => 3,
    }
}

fn pairs(v: &[f32]) -> Vec<V2> {
    v.chunks_exact(2).map(|p| [p[0], p[1]]).collect()
}

/// Copy form of `o` at the evaluated `frame` (values `values`).
/// `selection` = per-point selection weights, if a point selection exists.
/// Glue is not supported (`None`).
pub fn copy_form(
    m: &Model,
    frame: &Frame,
    values: &[f32],
    o: ObjectRef,
    selection: Option<&[f32]>,
) -> Option<CopiedForm> {
    let params = &m.parameters;
    let (canvas, opacity, multiply, screen, draw_order, angle) = match o {
        ObjectRef::ArtMesh(i) => {
            let a = m.art_meshes.get(i)?;
            let f = interpolate(params, values, &a.grid, &a.blend_shapes)?.form;
            (
                pairs(frame.positions_before_glue(i)?),
                f.opacity,
                f.multiply,
                f.screen,
                Some(f.draw_order),
                None,
            )
        }
        ObjectRef::Part(i) => {
            let p = m.parts.get(i)?;
            let f = interpolate(params, values, &p.grid, &p.blend_shapes)?.form;
            (
                Vec::new(),
                f.opacity,
                f.multiply,
                f.screen,
                Some(f.draw_order),
                None,
            )
        }
        ObjectRef::Deformer(i) => {
            let map = frame.deformers.get(i)?.map.as_ref()?;
            match (&m.deformers.get(i)?.kind, map) {
                (DeformerKind::Warp(w), DeformerMap::Warp(wm)) => {
                    let f = interpolate(params, values, &w.grid, &w.blend_shapes)?.form;
                    (
                        pairs(&wm.points),
                        f.opacity,
                        f.multiply,
                        f.screen,
                        None,
                        None,
                    )
                }
                (DeformerKind::Rotation(r), DeformerMap::Rotation(rm)) => {
                    let f = interpolate(params, values, &r.grid, &r.blend_shapes)?.form;
                    (
                        vec![rm.origin],
                        f.opacity,
                        f.multiply,
                        f.screen,
                        None,
                        Some(f.angle),
                    )
                }
                _ => return None,
            }
        }
        ObjectRef::Glue(_) => return None,
    };
    let n = canvas.len();
    let weights = match selection {
        Some(w) => (0..n).map(|k| w.get(k).copied().unwrap_or(0.0)).collect(),
        None => vec![1.0; n],
    };
    Some(CopiedForm {
        object: o,
        canvas,
        weights,
        opacity,
        multiply,
        screen,
        draw_order,
        angle,
        used_selection: selection.is_some(),
    })
}

/// Paste Form Special: mirror the copied canvas form first.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PasteMirror {
    /// Mirror x about `x_axis` (x' = 2a − x).
    pub horizontal: Option<f32>,
    /// Mirror y about `y_axis`.
    pub vertical: Option<f32>,
}

/// The copy that applies to target `t`: the same object, else (with
/// `multi`) one of the same kind with the same point count.
fn copy_for<'c>(
    m: &Model,
    copies: &'c [CopiedForm],
    t: ObjectRef,
    multi: bool,
) -> Option<&'c CopiedForm> {
    if let Some(c) = copies.iter().find(|c| c.object == t) {
        return Some(c);
    }
    if !multi {
        return None;
    }
    let n = point_count(m, t)?;
    let same: Vec<&CopiedForm> = copies
        .iter()
        .filter(|c| {
            kind(c.object) == kind(t) && c.canvas.len() == n && same_deformer_kind(m, c.object, t)
        })
        .collect();
    // Ambiguous matches need the multi-paste dialog; only a single match
    // is taken here.
    (same.len() == 1).then(|| same[0])
}

fn same_deformer_kind(m: &Model, a: ObjectRef, b: ObjectRef) -> bool {
    match (a, b) {
        (ObjectRef::Deformer(x), ObjectRef::Deformer(y)) => {
            matches!(
                (
                    m.deformers.get(x).map(|d| &d.kind),
                    m.deformers.get(y).map(|d| &d.kind)
                ),
                (Some(DeformerKind::Warp(_)), Some(DeformerKind::Warp(_)))
                    | (
                        Some(DeformerKind::Rotation(_)),
                        Some(DeformerKind::Rotation(_))
                    )
            )
        }
        _ => true,
    }
}

fn point_count(m: &Model, o: ObjectRef) -> Option<usize> {
    Some(match o {
        ObjectRef::ArtMesh(i) => m.art_meshes.get(i)?.vertex_count(),
        ObjectRef::Part(_) => 0,
        ObjectRef::Deformer(i) => match &m.deformers.get(i)?.kind {
            DeformerKind::Warp(w) => ((w.cols + 1) * (w.rows + 1)) as usize,
            DeformerKind::Rotation(_) => 1,
        },
        ObjectRef::Glue(_) => return None,
    })
}

/// Depth of `o` in the deformer tree (parents first).
fn depth(m: &Model, o: ObjectRef) -> usize {
    let mut d = parent_deformer(m, o);
    let mut n = 0;
    while let Some(p) = d {
        n += 1;
        d = m.deformers.get(p).and_then(|x| x.target);
        if n > m.deformers.len() {
            break;
        }
    }
    n
}

/// Paste (w = 1) / Blend Form (w = rate / 100) into the current keyforms
/// of `targets` (§2). Copies are converted from canvas into each target's
/// current parent-local space; deformers go first, parents before
/// children (re-evaluated after each deformer so children see the new
/// parent). Returns how many targets were written.
pub fn paste_form(
    m: &mut Model,
    values: &[f32],
    copies: &[CopiedForm],
    targets: &[ObjectRef],
    w: f32,
    mirror: PasteMirror,
) -> Result<usize, FormError> {
    let multi = targets.len() > 1 || copies.len() > 1;
    let mut order: Vec<ObjectRef> = targets.to_vec();
    order.sort_by_key(|&o| (!matches!(o, ObjectRef::Deformer(_)), depth(m, o)));
    let mut n = 0;
    let mut frame = Evaluator::new(m).evaluate(values);
    for t in order {
        let Some(c) = copy_for(m, copies, t, multi).cloned() else {
            continue;
        };
        let Ok(cell) = current_cell(m, t, values) else {
            return Err(FormError::NotOnKey);
        };
        let parent = parent_deformer(m, t).and_then(|p| frame.deformers.get(p)?.map.clone());
        let to_local = |p: V2| -> Option<V2> {
            let mut q = p;
            if let Some(a) = mirror.horizontal {
                q[0] = 2.0 * a - q[0];
            }
            if let Some(a) = mirror.vertical {
                q[1] = 2.0 * a - q[1];
            }
            match &parent {
                None => Some(q),
                Some(map) => map.inverse(q),
            }
        };
        let local: Vec<V2> = c
            .canvas
            .iter()
            .map(|&p| to_local(p).ok_or(FormError::NotInvertible))
            .collect::<Result<_, _>>()?;
        let blend_points = |pos: &mut Vec<f32>| {
            for (k, q) in local.iter().enumerate() {
                let pw = w * c.weights.get(k).copied().unwrap_or(1.0);
                if let Some(p) = pos.get_mut(2 * k..2 * k + 2) {
                    p[0] += pw * (q[0] - p[0]);
                    p[1] += pw * (q[1] - p[1]);
                }
            }
        };
        let lerp = |a: &mut f32, b: f32| *a += w * (b - *a);
        let lerp_c = |a: &mut Color, b: &Color| {
            for i in 0..4 {
                a[i] += w * (b[i] - a[i]);
            }
        };
        let is_deformer = matches!(t, ObjectRef::Deformer(_));
        match t {
            ObjectRef::ArtMesh(i) => {
                let f = &mut Arc::make_mut(&mut m.art_meshes[i]).grid.keyforms[cell];
                blend_points(&mut f.positions);
                lerp(&mut f.opacity, c.opacity);
                lerp_c(&mut f.multiply, &c.multiply);
                lerp_c(&mut f.screen, &c.screen);
                if let Some(d) = c.draw_order {
                    lerp(&mut f.draw_order, d);
                    f.draw_order = f.draw_order.round().clamp(0.0, 1000.0);
                }
                f.opacity = f.opacity.clamp(0.0, 1.0);
            }
            ObjectRef::Part(i) => {
                let f = &mut Arc::make_mut(&mut m.parts[i]).grid.keyforms[cell];
                lerp(&mut f.opacity, c.opacity);
                lerp_c(&mut f.multiply, &c.multiply);
                lerp_c(&mut f.screen, &c.screen);
                if let Some(d) = c.draw_order {
                    lerp(&mut f.draw_order, d);
                    f.draw_order = f.draw_order.round().clamp(0.0, 1000.0);
                }
                f.opacity = f.opacity.clamp(0.0, 1.0);
            }
            ObjectRef::Deformer(i) => match &mut Arc::make_mut(&mut m.deformers[i]).kind {
                DeformerKind::Warp(wd) => {
                    let f = &mut wd.grid.keyforms[cell];
                    blend_points(&mut f.positions);
                    lerp(&mut f.opacity, c.opacity);
                    lerp_c(&mut f.multiply, &c.multiply);
                    lerp_c(&mut f.screen, &c.screen);
                    f.opacity = f.opacity.clamp(0.0, 1.0);
                }
                DeformerKind::Rotation(r) => {
                    let f = &mut r.grid.keyforms[cell];
                    let mut o = f.origin.to_vec();
                    blend_points(&mut o);
                    f.origin = [o[0], o[1]];
                    if let Some(a) = c.angle {
                        lerp(&mut f.angle, a);
                    }
                    lerp(&mut f.opacity, c.opacity);
                    lerp_c(&mut f.multiply, &c.multiply);
                    lerp_c(&mut f.screen, &c.screen);
                    f.opacity = f.opacity.clamp(0.0, 1.0);
                }
            },
            ObjectRef::Glue(_) => continue,
        }
        n += 1;
        if is_deformer {
            frame = Evaluator::new(m).evaluate(values);
        }
    }
    Ok(n)
}

// ------------------------------------------------------- synthesize corners

/// A corner of the (P1, P2) key square: top = P2 max, right = P1 max.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CornerSettings {
    pub p1: ParamIndex,
    pub p2: ParamIndex,
    /// Reference values: the centre (min+max)/2 instead of the defaults.
    pub centre: bool,
    pub corners: Vec<Corner>,
}

/// Parameters with ≥ 3 keys on some object (the dialog's choices).
pub fn corner_parameters(m: &Model) -> Vec<ParamIndex> {
    let mut out = Vec::new();
    let mut add = |b: &[keyform_model::Binding]| {
        for x in b {
            if x.keys.len() >= 3 && !out.contains(&x.parameter) {
                out.push(x.parameter);
            }
        }
    };
    m.art_meshes.iter().for_each(|a| add(&a.grid.bindings));
    m.parts.iter().for_each(|p| add(&p.grid.bindings));
    for d in &m.deformers {
        match &d.kind {
            DeformerKind::Warp(w) => add(&w.grid.bindings),
            DeformerKind::Rotation(r) => add(&r.grid.bindings),
        }
    }
    out.sort_unstable();
    out
}

fn cell_of<F>(grid: &KeyformGrid<F>, params: &[Arc<Parameter>], values: &[f32]) -> Option<usize> {
    let mut cell = 0;
    let mut stride = 1;
    for b in &grid.bindings {
        let p = params.get(b.parameter)?;
        let v = values.get(b.parameter).copied().unwrap_or(p.default);
        let k = b
            .keys
            .iter()
            .position(|k| (k - v).abs() < p.snap_epsilon.max(1e-6))?;
        cell += k * stride;
        stride *= b.keys.len();
    }
    Some(cell)
}

/// K(c1,c2) = F(c1,b2) + F(b1,c2) − F(b1,b2) on one grid. Returns whether
/// the grid qualified (both parameters bound with ≥ 3 keys).
fn corners_on<F: Blend>(
    params: &[Arc<Parameter>],
    values: &[f32],
    grid: &mut KeyformGrid<F>,
    s: &CornerSettings,
) -> bool {
    let keys = |p: ParamIndex| {
        grid.bindings
            .iter()
            .find(|b| b.parameter == p)
            .map(|b| b.keys.clone())
    };
    let (Some(k1), Some(k2)) = (keys(s.p1), keys(s.p2)) else {
        return false;
    };
    if k1.len() < 3 || k2.len() < 3 {
        return false;
    }
    let base = |p: ParamIndex| {
        let q = &params[p];
        if s.centre {
            (q.min + q.max) / 2.0
        } else {
            q.default
        }
    };
    let (b1, b2) = (base(s.p1), base(s.p2));
    let none = BlendShapes::default();
    let at = |a: f32, b: f32| {
        let mut v = values.to_vec();
        v[s.p1] = a;
        v[s.p2] = b;
        v
    };
    let f = |grid: &KeyformGrid<F>, a: f32, b: f32| {
        interpolate(params, &at(a, b), grid, &none).map(|x| x.form)
    };
    let mut out = Vec::new();
    for &c in &s.corners {
        let c1 = match c {
            Corner::TopLeft | Corner::BottomLeft => k1[0],
            Corner::TopRight | Corner::BottomRight => k1[k1.len() - 1],
        };
        let c2 = match c {
            Corner::TopLeft | Corner::TopRight => k2[k2.len() - 1],
            Corner::BottomLeft | Corner::BottomRight => k2[0],
        };
        let (Some(fa), Some(fb), Some(fc)) = (f(grid, c1, b2), f(grid, b1, c2), f(grid, b1, b2))
        else {
            continue;
        };
        let mut k = fa;
        k.add_scaled(&fb, 1.0);
        k.add_scaled(&fc, -1.0);
        k.clamp();
        if let Some(cell) = cell_of(grid, params, &at(c1, c2)) {
            out.push((cell, k));
        }
    }
    for (cell, k) in out {
        grid.keyforms[cell] = k;
    }
    true
}

/// Synthesize corners (§3) on `objects` (other parameters at `values`).
/// Returns how many objects were processed.
pub fn synthesize_corners(
    m: &mut Model,
    values: &[f32],
    objects: &[ObjectRef],
    s: &CornerSettings,
) -> usize {
    if s.p1 == s.p2 || s.p1 >= m.parameters.len() || s.p2 >= m.parameters.len() {
        return 0;
    }
    let params = m.parameters.clone();
    let mut n = 0;
    for &o in objects {
        let done = match o {
            ObjectRef::ArtMesh(i) if i < m.art_meshes.len() => {
                let mut g = m.art_meshes[i].grid.clone();
                let ok = corners_on(&params, values, &mut g, s);
                if ok && g != m.art_meshes[i].grid {
                    Arc::make_mut(&mut m.art_meshes[i]).grid = g;
                }
                ok
            }
            ObjectRef::Part(i) if i < m.parts.len() => {
                let mut g = m.parts[i].grid.clone();
                let ok = corners_on(&params, values, &mut g, s);
                if ok && g != m.parts[i].grid {
                    Arc::make_mut(&mut m.parts[i]).grid = g;
                }
                ok
            }
            ObjectRef::Deformer(i) if i < m.deformers.len() => match &m.deformers[i].kind {
                DeformerKind::Warp(w) => {
                    let mut g = w.grid.clone();
                    let ok = corners_on(&params, values, &mut g, s);
                    if ok && g != w.grid {
                        if let DeformerKind::Warp(w) = &mut Arc::make_mut(&mut m.deformers[i]).kind
                        {
                            w.grid = g;
                        }
                    }
                    ok
                }
                DeformerKind::Rotation(r) => {
                    let mut g = r.grid.clone();
                    let ok = corners_on(&params, values, &mut g, s);
                    if ok && g != r.grid {
                        if let DeformerKind::Rotation(r) =
                            &mut Arc::make_mut(&mut m.deformers[i]).kind
                        {
                            r.grid = g;
                        }
                    }
                    ok
                }
            },
            _ => false,
        };
        n += usize::from(done);
    }
    n
}

// ------------------------------------------------------------------- flip

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlipSettings {
    pub horizontal: bool,
    pub vertical: bool,
    /// Every keyform instead of the current one ("target all").
    pub all_keyforms: bool,
    /// "Flip rotation deformer position".
    pub flip_rotation_position: bool,
    /// "Cancel auto-adjusted culling".
    pub keep_culling: bool,
    /// "Parameters to reflect": keyforms reversed afterwards.
    pub reflect: Vec<ParamIndex>,
}

fn mirror(p: &mut [f32], pivot: V2, h: bool, v: bool) {
    for q in p.chunks_exact_mut(2) {
        if h {
            q[0] = 2.0 * pivot[0] - q[0];
        }
        if v {
            q[1] = 2.0 * pivot[1] - q[1];
        }
    }
}

fn flip_angle(a: f32, h: bool, v: bool) -> f32 {
    let mut a = a;
    if h {
        a = -a;
    }
    if v {
        a = if a > 0.0 { 180.0 - a } else { -180.0 - a };
    }
    a
}

/// The cells a flip writes: all, or the current one (if on a key).
fn cells(m: &Model, o: ObjectRef, values: &[f32], all: bool, count: usize) -> Vec<usize> {
    if all {
        (0..count).collect()
    } else {
        current_cell(m, o, values).ok().into_iter().collect()
    }
}

fn children_of(m: &Model, d: usize) -> Vec<ObjectRef> {
    let mut out: Vec<ObjectRef> = (0..m.art_meshes.len())
        .filter(|&i| m.art_meshes[i].target == Some(d))
        .map(ObjectRef::ArtMesh)
        .collect();
    out.extend(
        (0..m.deformers.len())
            .filter(|&i| m.deformers[i].target == Some(d))
            .map(ObjectRef::Deformer),
    );
    out
}

/// Fix-ups below a flipped deformer `d` so the whole subtree is mirrored.
fn fix_children(m: &mut Model, d: usize, values: &[f32], s: &FlipSettings, guard: usize) {
    if guard > m.deformers.len() {
        return;
    }
    let parent_is_rotation = matches!(m.deformers[d].kind, DeformerKind::Rotation(_));
    for c in children_of(m, d) {
        match c {
            ObjectRef::ArtMesh(i) => {
                let n = m.art_meshes[i].grid.keyforms.len();
                let cs = cells(m, c, values, s.all_keyforms, n);
                let a = Arc::make_mut(&mut m.art_meshes[i]);
                if parent_is_rotation {
                    for k in cs {
                        mirror(&mut a.grid.keyforms[k].positions, [0.0, 0.0], true, false);
                    }
                }
                if !s.keep_culling {
                    a.culling = false;
                }
            }
            ObjectRef::Deformer(i) => {
                let n = match &m.deformers[i].kind {
                    DeformerKind::Warp(w) => w.grid.keyforms.len(),
                    DeformerKind::Rotation(r) => r.grid.keyforms.len(),
                };
                let cs = cells(m, c, values, s.all_keyforms, n);
                match &mut Arc::make_mut(&mut m.deformers[i]).kind {
                    DeformerKind::Warp(w) => {
                        if parent_is_rotation {
                            for k in cs {
                                mirror(&mut w.grid.keyforms[k].positions, [0.0, 0.0], true, false);
                            }
                        }
                    }
                    DeformerKind::Rotation(r) => {
                        r.base_angle = -r.base_angle;
                        for k in cs {
                            let f = &mut r.grid.keyforms[k];
                            f.angle = -f.angle;
                            if parent_is_rotation {
                                f.origin[0] = -f.origin[0];
                            }
                        }
                    }
                }
                fix_children(m, i, values, s, guard + 1);
            }
            _ => {}
        }
    }
}

/// Flip horizontally / vertically (§4) the selected objects' current (or
/// all) keyforms, in each object's parent-local space.
pub fn flip(m: &mut Model, values: &[f32], objects: &[ObjectRef], s: &FlipSettings) -> usize {
    if !s.horizontal && !s.vertical {
        return 0;
    }
    let (h, v) = (s.horizontal, s.vertical);
    let canvas = [m.canvas.width as f32 / 2.0, m.canvas.height as f32 / 2.0];
    // Top-level: skip objects below another selected deformer.
    let selected_deformers: Vec<usize> = objects
        .iter()
        .filter_map(|o| match o {
            ObjectRef::Deformer(d) => Some(*d),
            _ => None,
        })
        .collect();
    let below_selected = |m: &Model, o: ObjectRef| {
        let mut d = parent_deformer(m, o);
        let mut n = 0;
        while let Some(p) = d {
            if selected_deformers.contains(&p) {
                return true;
            }
            d = m.deformers.get(p).and_then(|x| x.target);
            n += 1;
            if n > m.deformers.len() {
                break;
            }
        }
        false
    };
    let mut done = 0;
    let mut flipped = Vec::new();
    for &o in objects {
        if below_selected(m, o) {
            continue;
        }
        let pivot = match parent_deformer(m, o).and_then(|p| m.deformers.get(p)) {
            None => canvas,
            Some(p) => match p.kind {
                DeformerKind::Warp(_) => [0.5, 0.5],
                DeformerKind::Rotation(_) => [0.0, 0.0],
            },
        };
        match o {
            ObjectRef::ArtMesh(i) if i < m.art_meshes.len() => {
                let n = m.art_meshes[i].grid.keyforms.len();
                let cs = cells(m, o, values, s.all_keyforms, n);
                if cs.is_empty() {
                    continue;
                }
                let a = Arc::make_mut(&mut m.art_meshes[i]);
                for k in cs {
                    mirror(&mut a.grid.keyforms[k].positions, pivot, h, v);
                }
                if !s.keep_culling {
                    a.culling = false;
                }
            }
            ObjectRef::Deformer(i) if i < m.deformers.len() => {
                let n = match &m.deformers[i].kind {
                    DeformerKind::Warp(w) => w.grid.keyforms.len(),
                    DeformerKind::Rotation(r) => r.grid.keyforms.len(),
                };
                let cs = cells(m, o, values, s.all_keyforms, n);
                if cs.is_empty() {
                    continue;
                }
                match &mut Arc::make_mut(&mut m.deformers[i]).kind {
                    DeformerKind::Warp(w) => {
                        for k in cs {
                            mirror(&mut w.grid.keyforms[k].positions, pivot, h, v);
                        }
                    }
                    DeformerKind::Rotation(r) => {
                        r.base_angle = flip_angle(r.base_angle, h, v);
                        for k in cs {
                            let f = &mut r.grid.keyforms[k];
                            f.angle = flip_angle(f.angle, h, v);
                            if s.flip_rotation_position {
                                let mut p = f.origin;
                                mirror(&mut p, pivot, h, v);
                                f.origin = p;
                            }
                        }
                    }
                }
                fix_children(m, i, values, s, 0);
            }
            _ => continue,
        }
        flipped.push(o);
        done += 1;
    }
    for &p in &s.reflect {
        for &o in &flipped {
            reverse_keys(m, o, p);
            if let ObjectRef::Deformer(d) = o {
                for c in descendants(m, d) {
                    reverse_keys(m, c, p);
                }
            }
        }
    }
    done
}

fn descendants(m: &Model, d: usize) -> Vec<ObjectRef> {
    let mut out = Vec::new();
    let mut stack = vec![d];
    let mut guard = 0;
    while let Some(x) = stack.pop() {
        for c in children_of(m, x) {
            if let ObjectRef::Deformer(i) = c {
                stack.push(i);
            }
            out.push(c);
        }
        guard += 1;
        if guard > m.deformers.len() + 1 {
            break;
        }
    }
    out
}

// ------------------------------------------------------- motion mirroring

/// Motion Mirroring settings (§5.1): flips and the axis value (canvas
/// pixels) for each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MirrorSettings {
    pub horizontal: bool,
    pub vertical: bool,
    /// x of the vertical mirror line (horizontal flip).
    pub axis_x: f32,
    /// y of the horizontal mirror line (vertical flip).
    pub axis_y: f32,
}

/// Messages of the §5.1 pre-checks.
pub const MIRROR_NO_OBJECTS: &str = "No objects selected for motion mirroring.";
pub const MIRROR_REPEAT: &str = "Parameter is set to repeat";
pub const MIRROR_SAME_VALUE: &str = "Unable to generate keyforms for mirroring.";
pub const MIRROR_NOT_LINKED: &str = "The selected parameters are not linked to the object";
pub const MIRROR_NO_KEYFORM: &str = "There are no keyforms at this parameter value.";
pub const MIRROR_NO_DEFAULT: &str = "There are no default keyforms.";
pub const MIRROR_NO_FLIP: &str = "Unable to execute mirroring process.";
pub const MIRROR_PARENT_WARNING: &str = "The parent deformer with common parameters is not selected. Parent deformer not mirrored; child objects may behave unexpectedly. If the result is different than expected, the process can be reversed with Undo.";

fn bound_to(m: &Model, o: ObjectRef, p: ParamIndex) -> bool {
    let b = |g: &[keyform_model::Binding]| g.iter().any(|x| x.parameter == p);
    match o {
        ObjectRef::ArtMesh(i) => m.art_meshes.get(i).is_some_and(|a| b(&a.grid.bindings)),
        ObjectRef::Part(i) => m.parts.get(i).is_some_and(|a| b(&a.grid.bindings)),
        ObjectRef::Glue(i) => m.glues.get(i).is_some_and(|a| b(&a.grid.bindings)),
        ObjectRef::Deformer(i) => m.deformers.get(i).is_some_and(|d| match &d.kind {
            DeformerKind::Warp(w) => b(&w.grid.bindings),
            DeformerKind::Rotation(r) => b(&r.grid.bindings),
        }),
    }
}

/// The pre-checks (§5.1). `Ok(true)` when the parent-deformer warning
/// applies.
pub fn mirror_check(
    m: &Model,
    values: &[f32],
    objects: &[ObjectRef],
    p: ParamIndex,
) -> Result<bool, String> {
    let objects: Vec<ObjectRef> = objects
        .iter()
        .copied()
        .filter(|o| !matches!(o, ObjectRef::Glue(_)))
        .collect();
    if objects.is_empty() {
        return Err(MIRROR_NO_OBJECTS.into());
    }
    let q = m
        .parameters
        .get(p)
        .ok_or("None of parameters is selected")?;
    if q.repeat {
        return Err(MIRROR_REPEAT.into());
    }
    let v = values.get(p).copied().unwrap_or(q.default);
    let target = 2.0 * q.default - v;
    if (target - v).abs() < q.snap_epsilon.max(1e-6) {
        return Err(MIRROR_SAME_VALUE.into());
    }
    for &o in &objects {
        if !bound_to(m, o, p) {
            return Err(MIRROR_NOT_LINKED.into());
        }
        if current_cell(m, o, values).is_err() {
            return Err(MIRROR_NO_KEYFORM.into());
        }
    }
    let warn = objects.iter().any(|&o| {
        let mut d = parent_deformer(m, o);
        let mut n = 0;
        while let Some(x) = d {
            if bound_to(m, ObjectRef::Deformer(x), p) && !objects.contains(&ObjectRef::Deformer(x))
            {
                return true;
            }
            d = m.deformers.get(x).and_then(|y| y.target);
            n += 1;
            if n > m.deformers.len() {
                break;
            }
        }
        false
    });
    Ok(warn)
}

fn reflect(q: V2, s: &MirrorSettings) -> V2 {
    [
        if s.horizontal {
            2.0 * s.axis_x - q[0]
        } else {
            q[0]
        },
        if s.vertical {
            2.0 * s.axis_y - q[1]
        } else {
            q[1]
        },
    ]
}

/// The canvas points of `o` in `frame` (art mesh vertices before glue,
/// warp grid, rotation origin).
fn canvas_points(frame: &Frame, o: ObjectRef) -> Option<Vec<V2>> {
    Some(match o {
        ObjectRef::ArtMesh(i) => pairs(frame.positions_before_glue(i)?),
        ObjectRef::Deformer(i) => match frame.deformers.get(i)?.map.as_ref()? {
            DeformerMap::Warp(w) => pairs(&w.points),
            DeformerMap::Rotation(r) => vec![r.origin],
        },
        _ => return None,
    })
}

/// Motion Mirroring (§5.1) of `objects` on parameter `p` from its current
/// value v to 2d − v. Returns the destination value.
pub fn motion_mirror(
    m: &mut Model,
    values: &[f32],
    objects: &[ObjectRef],
    p: ParamIndex,
    s: &MirrorSettings,
) -> Result<f32, String> {
    if !s.horizontal && !s.vertical {
        return Err(MIRROR_NO_FLIP.into());
    }
    mirror_check(m, values, objects, p)?;
    let q = m.parameters[p].clone();
    let v = values[p];
    let target = 2.0 * q.default - v;
    let at = |x: f32| {
        let mut w = values.to_vec();
        w[p] = x;
        w
    };
    let (vals_src, vals_def, vals_dst) = (at(v), at(q.default), at(target));
    let all_defaults: Vec<f32> = m.parameters.iter().map(|x| x.default).collect();
    let mut order: Vec<ObjectRef> = objects
        .iter()
        .copied()
        .filter(|o| matches!(o, ObjectRef::ArtMesh(_) | ObjectRef::Deformer(_)))
        .collect();
    order.sort_by_key(|&o| (depth(m, o), !matches!(o, ObjectRef::Deformer(_))));
    for o in order {
        if current_cell(m, o, &vals_def).is_err() {
            return Err(MIRROR_NO_DEFAULT.into());
        }
        // Destination keyform: add a key at v′ if needed (it starts as the
        // interpolated form). Cells are taken after that (it renumbers).
        if current_cell(m, o, &vals_dst).is_err() {
            crate::keys::add_key(m, o, p, target).map_err(|e| e.to_string())?;
        }
        let dst_cell = current_cell(m, o, &vals_dst).map_err(|e| e.to_string())?;
        let src_cell = current_cell(m, o, &vals_src).map_err(|_| MIRROR_NO_KEYFORM.to_owned())?;
        let def_cell = current_cell(m, o, &vals_def).ok();
        let ev = Evaluator::new(m);
        let f_src = ev.evaluate(&vals_src);
        let f_dst = ev.evaluate(&vals_dst);
        let src = canvas_points(&f_src, o).ok_or("no evaluated form")?;
        let dest: Vec<V2> = match o {
            ObjectRef::ArtMesh(i) => {
                let f_all = ev.evaluate(&all_defaults);
                let d = canvas_points(&f_all, o).ok_or("no evaluated form")?;
                let flat: Vec<f32> = d.iter().flat_map(|p| [p[0], p[1]]).collect();
                let src_flat: Vec<f32> = src.iter().flat_map(|p| [p[0], p[1]]).collect();
                let remap = crate::mesh::Remap::new(&flat, &m.art_meshes[i].indices);
                d.iter()
                    .map(|&pt| {
                        let r = reflect(pt, s);
                        let mapped = remap.apply(remap.locate(r), &src_flat);
                        reflect(mapped, s)
                    })
                    .collect()
            }
            ObjectRef::Deformer(i) => {
                let f_def = ev.evaluate(&vals_def);
                let def = canvas_points(&f_def, o).ok_or("no evaluated form")?;
                let delta: Vec<V2> = def
                    .iter()
                    .zip(&src)
                    .map(|(a, b)| [a[0] - b[0], a[1] - b[1]])
                    .collect();
                // Warp: Δ re-indexed to the mirrored grid point.
                let idx = |k: usize| -> usize {
                    match &m.deformers[i].kind {
                        DeformerKind::Warp(w) => {
                            let (c, r) = (w.cols as usize + 1, w.rows as usize + 1);
                            let (x, y) = (k % c, k / c);
                            let x = if s.horizontal { c - 1 - x } else { x };
                            let y = if s.vertical { r - 1 - y } else { y };
                            y * c + x
                        }
                        DeformerKind::Rotation(_) => k,
                    }
                };
                def.iter()
                    .enumerate()
                    .map(|(k, d)| {
                        let dl = delta.get(idx(k)).copied().unwrap_or([0.0, 0.0]);
                        let (dx, dy) = match (s.horizontal, s.vertical) {
                            (true, false) => (dl[0], -dl[1]),
                            (false, true) => (-dl[0], dl[1]),
                            _ => (dl[0], dl[1]),
                        };
                        [d[0] + dx, d[1] + dy]
                    })
                    .collect()
            }
            _ => continue,
        };
        // Canvas → the parent's local space at v′.
        let parent = parent_deformer(m, o).and_then(|x| f_dst.deformers.get(x)?.map.clone());
        let local: Vec<f32> = dest
            .iter()
            .map(|&c| match &parent {
                None => Some(c),
                Some(map) => map.inverse(c),
            })
            .collect::<Option<Vec<V2>>>()
            .ok_or("the point cannot be mapped into the parent deformer")?
            .into_iter()
            .flat_map(|p| [p[0], p[1]])
            .collect();
        match o {
            ObjectRef::ArtMesh(i) => {
                let a = Arc::make_mut(&mut m.art_meshes[i]);
                let (order, opacity) = {
                    let f = &a.grid.keyforms[src_cell];
                    (f.draw_order, f.opacity)
                };
                let f = &mut a.grid.keyforms[dst_cell];
                f.positions = local;
                f.draw_order = order;
                f.opacity = opacity;
            }
            ObjectRef::Deformer(i) => match &mut Arc::make_mut(&mut m.deformers[i]).kind {
                DeformerKind::Warp(w) => {
                    let opacity = w.grid.keyforms[src_cell].opacity;
                    let f = &mut w.grid.keyforms[dst_cell];
                    f.positions = local;
                    f.opacity = opacity;
                }
                DeformerKind::Rotation(r) => {
                    let (sa, op) = (
                        r.grid.keyforms[src_cell].angle,
                        r.grid.keyforms[src_cell].opacity,
                    );
                    let da = def_cell.map_or(sa, |c| r.grid.keyforms[c].angle);
                    let f = &mut r.grid.keyforms[dst_cell];
                    f.origin = [local[0], local[1]];
                    f.angle = da + (da - sa);
                    f.opacity = op;
                }
            },
            _ => {}
        }
    }
    Ok(target)
}

// --------------------------------------------------------- reverse keys

fn reverse_grid<F>(g: &mut KeyformGrid<F>, p: ParamIndex) -> bool {
    let Some(j) = g.bindings.iter().position(|b| b.parameter == p) else {
        return false;
    };
    let keys = g.bindings[j].keys.clone();
    let n = keys.len();
    if n < 2 {
        return false;
    }
    let (lo, hi) = (keys[0], keys[n - 1]);
    g.bindings[j].keys = (0..n).map(|i| lo + hi - keys[n - 1 - i]).collect();
    let stride: usize = g.bindings[..j].iter().map(|b| b.keys.len()).product();
    let cells = g.keyforms.len();
    let src: Vec<usize> = (0..cells)
        .map(|c| {
            let i = (c / stride) % n;
            c - i * stride + (n - 1 - i) * stride
        })
        .collect();
    let mut forms: Vec<Option<F>> = std::mem::take(&mut g.keyforms)
        .into_iter()
        .map(Some)
        .collect();
    g.keyforms = src
        .iter()
        .map(|&s| forms[s].take())
        .collect::<Option<Vec<F>>>()
        .unwrap_or_default();
    if g.guids.len() == cells {
        let guids = std::mem::take(&mut g.guids);
        g.guids = src.iter().map(|&s| guids[s].clone()).collect();
    }
    true
}

/// Reverse keyforms on parameter `p` for object `o` (keys.md §3): key
/// values mirrored inside [min, max], keyforms permuted along that axis.
pub fn reverse_keys(m: &mut Model, o: ObjectRef, p: ParamIndex) -> bool {
    let bound = |g: &[keyform_model::Binding]| g.iter().any(|b| b.parameter == p);
    match o {
        ObjectRef::ArtMesh(i) if m.art_meshes.get(i).is_some_and(|a| bound(&a.grid.bindings)) => {
            reverse_grid(&mut Arc::make_mut(&mut m.art_meshes[i]).grid, p)
        }
        ObjectRef::Part(i) if m.parts.get(i).is_some_and(|a| bound(&a.grid.bindings)) => {
            reverse_grid(&mut Arc::make_mut(&mut m.parts[i]).grid, p)
        }
        ObjectRef::Glue(i) if m.glues.get(i).is_some_and(|a| bound(&a.grid.bindings)) => {
            reverse_grid(&mut Arc::make_mut(&mut m.glues[i]).grid, p)
        }
        ObjectRef::Deformer(i) if i < m.deformers.len() => {
            let has = match &m.deformers[i].kind {
                DeformerKind::Warp(w) => bound(&w.grid.bindings),
                DeformerKind::Rotation(r) => bound(&r.grid.bindings),
            };
            if !has {
                return false;
            }
            match &mut Arc::make_mut(&mut m.deformers[i]).kind {
                DeformerKind::Warp(w) => reverse_grid(&mut w.grid, p),
                DeformerKind::Rotation(r) => reverse_grid(&mut r.grid, p),
            }
        }
        _ => false,
    }
}

/// Batch Reflect (§5.2): reverse keyforms on each parameter for every
/// object bound to it. Returns how many grids changed.
pub fn batch_reflect(m: &mut Model, params: &[ParamIndex]) -> usize {
    let mut n = 0;
    for &p in params {
        let objects: Vec<ObjectRef> = (0..m.art_meshes.len())
            .map(ObjectRef::ArtMesh)
            .chain((0..m.parts.len()).map(ObjectRef::Part))
            .chain((0..m.deformers.len()).map(ObjectRef::Deformer))
            .chain((0..m.glues.len()).map(ObjectRef::Glue))
            .collect();
        for o in objects {
            n += usize::from(reverse_keys(m, o, p));
        }
    }
    n
}

/// Every deformer and art mesh (synthesize corners' "all objects").
pub fn all_objects(m: &Model) -> Vec<ObjectRef> {
    let mut out: Vec<ObjectRef> = (0..m.deformers.len()).map(ObjectRef::Deformer).collect();
    out.extend((0..m.art_meshes.len()).map(ObjectRef::ArtMesh));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::Binding;

    #[test]
    fn reverse_grid_two_axes() {
        // Bindings: p0 keys [0, 1] (fastest), p1 keys [-1, 0, 2].
        let mut g = KeyformGrid {
            bindings: vec![
                Binding::new(0, vec![0.0, 1.0]),
                Binding::new(1, vec![-1.0, 0.0, 2.0]),
            ],
            keyforms: (0..6).collect::<Vec<i32>>(),
            guids: Vec::new(),
        };
        assert!(reverse_grid(&mut g, 1));
        // k' = min + max − k(n−1−i) = 1 − [2, 0, −1] = [−1, 1, 2].
        assert_eq!(g.bindings[1].keys, vec![-1.0, 1.0, 2.0]);
        assert_eq!(g.keyforms, vec![4, 5, 2, 3, 0, 1]);
        assert!(reverse_grid(&mut g, 0));
        assert_eq!(g.keyforms, vec![5, 4, 3, 2, 1, 0]);
    }

    fn toy() -> Model {
        use keyform_model::{ArtMesh, ArtMeshForm, EditableMesh, ObjectInfo};
        let base = vec![40.0, 0.0, 60.0, 0.0, 50.0, 10.0];
        let shifted: Vec<f32> = base
            .iter()
            .enumerate()
            .map(|(k, v)| if k % 2 == 0 { v + 5.0 } else { *v })
            .collect();
        let form = |p: &Vec<f32>| ArtMeshForm {
            positions: p.clone(),
            ..Default::default()
        };
        let a = ArtMesh {
            clip_missing: false,
            info: ObjectInfo::new("A"),
            target: None,
            positions: base.clone(),
            uvs: vec![0.0; 6],
            indices: vec![0, 1, 2],
            texture: None,
            clip: Vec::new(),
            invert_clip: false,
            culling: false,
            color_composition: Default::default(),
            alpha_composition: Default::default(),
            user_data: String::new(),
            point_uids: vec![0, 1, 2],
            mesh: EditableMesh::default(),
            model_image: None,
            generator: Default::default(),
            grid: KeyformGrid {
                bindings: vec![Binding::new(0, vec![0.0, 1.0])],
                keyforms: vec![form(&base), form(&shifted)],
                guids: Vec::new(),
            },
            blend_shapes: Default::default(),
        };
        Model {
            parameters: vec![Arc::new(Parameter::new("ParamAngleX", -1.0, 1.0, 0.0))],
            art_meshes: vec![Arc::new(a)],
            ..Model::default()
        }
    }

    #[test]
    fn motion_mirror_toy() {
        let mut m = toy();
        let o = ObjectRef::ArtMesh(0);
        let s = MirrorSettings {
            horizontal: true,
            vertical: false,
            axis_x: 50.0,
            axis_y: 0.0,
        };
        assert_eq!(
            motion_mirror(&mut m, &[0.0], &[o], 0, &s),
            Err(MIRROR_SAME_VALUE.into())
        );
        assert_eq!(
            motion_mirror(&mut m, &[1.0], &[], 0, &s),
            Err(MIRROR_NO_OBJECTS.into())
        );
        assert_eq!(motion_mirror(&mut m, &[1.0], &[o], 0, &s), Ok(-1.0));
        let g = &m.art_meshes[0].grid;
        assert_eq!(g.bindings[0].keys, vec![-1.0, 0.0, 1.0]);
        let got = &g.keyforms[0].positions;
        let want = [35.0, 0.0, 55.0, 0.0, 45.0, 10.0];
        for (a, b) in got.iter().zip(want) {
            assert!((a - b).abs() < 1e-3, "{got:?}");
        }
        let none = MirrorSettings {
            horizontal: false,
            ..s
        };
        assert_eq!(
            motion_mirror(&mut m, &[1.0], &[o], 0, &none),
            Err(MIRROR_NO_FLIP.into())
        );
    }

    #[test]
    fn flip_angles() {
        assert_eq!(flip_angle(30.0, true, false), -30.0);
        assert_eq!(flip_angle(30.0, false, true), 150.0);
        assert_eq!(flip_angle(-30.0, false, true), -150.0);
    }
}
