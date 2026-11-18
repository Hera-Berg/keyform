//! Model templates: automatic art mesh matching between a template and a
//! destination model. spec: 70-advanced/templates.md §2 (layout
//! transform), §3 (matching, dominant colour)

use std::sync::Arc;

type P = [f32; 2];

/// The template transform (§2): destination = template · scale + offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub scale: f32,
    pub offset: P,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset: [0.0, 0.0],
        }
    }
}

impl Layout {
    pub fn apply(&self, p: P) -> P {
        [
            p[0] * self.scale + self.offset[0],
            p[1] * self.scale + self.offset[1],
        ]
    }

    pub fn inverse(&self, p: P) -> P {
        let s = if self.scale.abs() < 1e-12 {
            1.0
        } else {
            self.scale
        };
        [(p[0] - self.offset[0]) / s, (p[1] - self.offset[1]) / s]
    }
}

/// What matching needs to know about one art mesh.
#[derive(Clone)]
pub struct MeshInfo {
    pub name: String,
    /// Base (default-state) canvas points.
    pub points: Vec<P>,
    /// Draw-order rank among the model's meshes, divided by their count.
    pub rank: f32,
    /// Dominant colour (RGB 0–255), if the mesh has an image.
    pub color: Option<[f32; 3]>,
    /// For a destination mesh under a warp deformer: whether a canvas
    /// point falls inside that warp grid.
    pub inside_parent: Option<Arc<dyn Fn(P) -> bool + Send + Sync>>,
}

/// Dominant colour of sampled RGBA pixels (§3): pixels with alpha ≤ 10
/// are skipped; each pixel joins every group whose mean is within 40 on
/// every channel (updating its mean), or starts a new group; the largest
/// group's mean wins.
pub fn dominant_color(pixels: impl IntoIterator<Item = [u8; 4]>) -> Option<[f32; 3]> {
    // (sum r, g, b, count)
    let mut groups: Vec<([f64; 3], u32)> = Vec::new();
    for px in pixels {
        if px[3] <= 10 {
            continue;
        }
        let c = [f64::from(px[0]), f64::from(px[1]), f64::from(px[2])];
        let mut joined = false;
        for (sum, n) in &mut groups {
            let mean = sum.map(|s| s / f64::from(*n));
            if (0..3).all(|k| (mean[k] - c[k]).abs() <= 40.0) {
                for k in 0..3 {
                    sum[k] += c[k];
                }
                *n += 1;
                joined = true;
            }
        }
        if !joined {
            groups.push((c, 1));
        }
    }
    // Stable: the first of equally large groups.
    let mut best: Option<&([f64; 3], u32)> = None;
    for g in &groups {
        if best.is_none_or(|b| g.1 > b.1) {
            best = Some(g);
        }
    }
    best.map(|(s, n)| s.map(|x| (x / f64::from(*n)) as f32))
}

/// About `n` evenly spaced sample indices out of `len`.
pub fn sample_indices(len: usize, n: usize) -> impl Iterator<Item = usize> {
    let step = (len / n.max(1)).max(1);
    (0..len).step_by(step)
}

fn bounds(pts: &[P]) -> [f32; 4] {
    let mut b = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for p in pts {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b
}

fn dist(a: P, b: P) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn nearest_sum(from: &[P], to: &[P]) -> f32 {
    from.iter()
        .map(|p| {
            to.iter()
                .map(|q| dist(*p, *q))
                .fold(f32::INFINITY, f32::min)
        })
        .filter(|d| d.is_finite())
        .sum()
}

/// eval1 of destination `d` against transformed template points `tp`
/// (§3 step 2).
fn eval1(d: &MeshInfo, t: &MeshInfo, tp: &[P], g: f32) -> f32 {
    let c = match (d.color, t.color) {
        (Some(a), Some(b)) => {
            let e = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
            if e < 20.0 {
                0.1
            } else if e < 40.0 {
                0.5
            } else {
                1.0
            }
        }
        _ => 1.0,
    };
    let o = if (d.rank - t.rank).abs() < 0.2 {
        0.5
    } else {
        1.0
    };
    let df = match &d.inside_parent {
        Some(inside) if !tp.is_empty() => {
            let mut f = tp.iter().filter(|p| inside(**p)).count() as f32 / tp.len() as f32;
            if f >= 0.8 {
                f = 1.0;
            }
            1.0 / f.max(0.1)
        }
        _ => 1.0,
    };
    let (bd, bt) = (bounds(&d.points), bounds(tp));
    let corner = (bd[0] - bt[0]).hypot(bd[1] - bt[1]);
    let geo = (corner * corner * (bd[2] - bt[2]).abs() * (bd[3] - bt[3]).abs()).powf(0.25);
    df * c * o * geo / g
}

/// Automatic matching (§3): for each destination mesh, the template mesh
/// index or `None`. `manual` holds pre-connected pairs (destination,
/// template or `None` for "no pair"); with `names`, equal names pair first.
/// `diagonal` is the destination canvas diagonal G.
// The facial-part recogniser of the reference is a tuned asset and is not
// reproduced (§3 step 1); Keyform has none.
pub fn auto_match(
    dst: &[MeshInfo],
    tpl: &[MeshInfo],
    layout: &Layout,
    diagonal: f32,
    names: bool,
    manual: &[(usize, Option<usize>)],
) -> Vec<Option<usize>> {
    let g = diagonal.max(1e-6);
    let tps: Vec<Vec<P>> = tpl
        .iter()
        .map(|t| t.points.iter().map(|p| layout.apply(*p)).collect())
        .collect();
    dst.iter()
        .enumerate()
        .map(|(di, d)| {
            if let Some(&(_, t)) = manual.iter().find(|m| m.0 == di) {
                return t;
            }
            if names {
                if let Some(t) = tpl.iter().position(|t| t.name == d.name) {
                    return Some(t);
                }
            }
            if d.points.is_empty() {
                return None;
            }
            let mut cand: Vec<(f32, usize)> = tpl
                .iter()
                .enumerate()
                .filter(|(ti, _)| !tps[*ti].is_empty())
                .map(|(ti, t)| (eval1(d, t, &tps[ti], g), ti))
                .filter(|c| c.0.is_finite())
                .collect();
            cand.sort_by(|a, b| a.0.total_cmp(&b.0));
            cand.truncate(10);
            let best = cand.first()?;
            if best.0 > 0.01 {
                // One-sided chamfer distance, destination → template.
                return cand
                    .iter()
                    .map(|&(_, ti)| {
                        (
                            nearest_sum(&d.points, &tps[ti]) / d.points.len() as f32 / g,
                            ti,
                        )
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|x| x.1);
            }
            let mut pick: Option<(f32, usize)> = None;
            let mut prev: Option<f32> = None;
            for &(e1, ti) in &cand {
                if prev.is_some_and(|p| e1 >= 20.0 * p) {
                    break;
                }
                prev = Some(e1);
                let tp = &tps[ti];
                let e2 = (nearest_sum(&d.points, tp) + nearest_sum(tp, &d.points))
                    / (d.points.len() + tp.len()) as f32
                    / g;
                let s = e1 * e2;
                if pick.is_none_or(|p| s < p.0) {
                    pick = Some((s, ti));
                }
            }
            pick.map(|p| p.1)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(name: &str, x: f32, y: f32, w: f32, rank: f32, color: [f32; 3]) -> MeshInfo {
        let mut points = Vec::new();
        for i in 0..5 {
            for j in 0..5 {
                points.push([x + w * j as f32 / 4.0, y + w * i as f32 / 4.0]);
            }
        }
        MeshInfo {
            name: name.into(),
            points,
            rank,
            color: Some(color),
            inside_parent: None,
        }
    }

    #[test]
    fn dominant_color_groups() {
        let px = [
            [200, 10, 10, 255],
            [205, 12, 8, 255],
            [10, 10, 200, 255],
            [0, 0, 0, 0],
        ];
        let c = dominant_color(px).unwrap();
        assert!((c[0] - 202.5).abs() < 1e-3 && (c[2] - 9.0).abs() < 1e-3);
        assert!(dominant_color([[1, 2, 3, 5]]).is_none());
    }

    #[test]
    fn matches_by_geometry_under_a_layout() {
        let tpl = vec![
            mesh("face", 0.0, 0.0, 100.0, 0.1, [250.0, 220.0, 200.0]),
            mesh("eye", 20.0, 30.0, 15.0, 0.5, [40.0, 40.0, 90.0]),
            mesh("mouth", 40.0, 70.0, 20.0, 0.6, [200.0, 80.0, 80.0]),
        ];
        // Destination: the same layout at scale 2, offset (10, 20), with
        // different names and slight colour noise.
        let layout = Layout {
            scale: 2.0,
            offset: [10.0, 20.0],
        };
        let dst: Vec<MeshInfo> = [(2, "m"), (0, "f"), (1, "e")]
            .iter()
            .map(|&(k, n)| {
                let t = &tpl[k];
                MeshInfo {
                    name: n.into(),
                    points: t.points.iter().map(|p| layout.apply(*p)).collect(),
                    rank: t.rank,
                    color: t.color.map(|c| [c[0] + 5.0, c[1], c[2]]),
                    inside_parent: None,
                }
            })
            .collect();
        let m = auto_match(&dst, &tpl, &layout, 500.0, true, &[]);
        assert_eq!(m, vec![Some(2), Some(0), Some(1)]);
        // A manual "no pair" wins.
        let m = auto_match(&dst, &tpl, &layout, 500.0, false, &[(1, None)]);
        assert_eq!(m, vec![Some(2), None, Some(1)]);
        assert_eq!(layout.inverse(layout.apply([3.0, 4.0])), [3.0, 4.0]);
    }
}

// ---------------------------------------------------------------- Apply

use std::collections::HashMap;

use keyform_eval::Evaluator;
use keyform_model::{new_guid, Binding, BlendShapes, DeformerKind, KeyformGrid, Model, PartChild};

use crate::mesh::Remap;

/// Draw order handling (§4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DrawOrderMode {
    /// Copy the template keyforms' draw order.
    #[default]
    AdjustToTemplate,
    /// Keep each destination mesh's current draw order.
    DontChange,
}

/// Apply settings (§4); meshes are not regenerated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ApplyOptions {
    pub draw_order: DrawOrderMode,
    pub mask: bool,
    pub culling: bool,
    pub color_blend: bool,
    pub alpha_blend: bool,
}

impl Default for ApplyOptions {
    fn default() -> Self {
        Self {
            draw_order: DrawOrderMode::AdjustToTemplate,
            mask: true,
            culling: true,
            color_blend: true,
            alpha_blend: true,
        }
    }
}

/// What an application copied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ApplyReport {
    pub parameters_added: usize,
    pub parameters_reused: usize,
    pub deformers: usize,
    pub parts: usize,
    pub meshes: usize,
    pub physics: usize,
}

fn ancestors_of_deformer(m: &Model, mut d: Option<usize>, out: &mut Vec<usize>) {
    let mut guard = 0;
    while let Some(i) = d {
        if !out.contains(&i) {
            out.push(i);
        }
        d = m.deformers.get(i).and_then(|x| x.target);
        guard += 1;
        if guard > m.deformers.len() {
            break;
        }
    }
}

fn remap_bindings(b: &[Binding], map: &HashMap<usize, usize>) -> Vec<Binding> {
    b.iter()
        .map(|x| Binding {
            parameter: map.get(&x.parameter).copied().unwrap_or(x.parameter),
            ..x.clone()
        })
        .collect()
}

fn remap_blend<F: Clone>(b: &BlendShapes<F>, map: &HashMap<usize, usize>) -> BlendShapes<F> {
    let mut out = b.clone();
    for s in &mut out.shapes {
        s.parameter = map.get(&s.parameter).copied().unwrap_or(s.parameter);
    }
    for c in &mut out.constraints {
        c.blend_parameter = map
            .get(&c.blend_parameter)
            .copied()
            .unwrap_or(c.blend_parameter);
        c.constraint_parameter = map
            .get(&c.constraint_parameter)
            .copied()
            .unwrap_or(c.constraint_parameter);
    }
    for e in &mut out.extended {
        e.parameter = map.get(&e.parameter).copied().unwrap_or(e.parameter);
    }
    out
}

fn fresh_guids<F>(g: &mut KeyformGrid<F>) {
    g.guids = (0..g.keyforms.len()).map(|_| new_guid()).collect();
}

/// Applies template `tpl` to `dst` for the art mesh `pairs` (destination
/// mesh, template mesh) under `layout` (§5). Parameters with an ID the
/// destination already has are reused; other copied elements get new
/// GUIDs and free IDs.
// TODO(Q-0041): merge settings (§4) and several §5 details are provisional.
pub fn apply(
    dst: &mut Model,
    tpl: &Model,
    pairs: &[(usize, usize)],
    layout: &Layout,
    o: &ApplyOptions,
) -> Result<ApplyReport, String> {
    let mut rep = ApplyReport::default();
    if pairs
        .iter()
        .any(|&(d, t)| d >= dst.art_meshes.len() || t >= tpl.art_meshes.len())
    {
        return Err("no such art mesh".into());
    }
    // 1. Needed template deformers and parts (paired meshes and ancestors).
    let mut defs = Vec::new();
    for &(_, t) in pairs {
        ancestors_of_deformer(tpl, tpl.art_meshes[t].target, &mut defs);
    }
    // Parents first.
    let depth = |d: usize| {
        let mut n = 0;
        let mut c = tpl.deformers[d].target;
        while let Some(i) = c {
            n += 1;
            c = tpl.deformers[i].target;
            if n > tpl.deformers.len() {
                break;
            }
        }
        n
    };
    defs.sort_by_key(|&d| (depth(d), d));
    let mut parts = Vec::new();
    let part_up = |p: Option<usize>, parts: &mut Vec<usize>| {
        let mut c = p;
        while let Some(i) = c {
            if Some(i) == tpl.root_part || parts.contains(&i) {
                break;
            }
            parts.push(i);
            c = tpl.parts[i].info.parent_part;
        }
    };
    for &d in &defs {
        part_up(tpl.deformers[d].info.parent_part, &mut parts);
    }
    for &(_, t) in pairs {
        part_up(tpl.art_meshes[t].info.parent_part, &mut parts);
    }
    // Parents first: by depth in the part tree.
    let pdepth = |p: usize| {
        let mut n = 0;
        let mut c = tpl.parts[p].info.parent_part;
        while let Some(i) = c {
            n += 1;
            c = tpl.parts[i].info.parent_part;
            if n > tpl.parts.len() {
                break;
            }
        }
        n
    };
    parts.sort_by_key(|&p| (pdepth(p), p));
    // Needed parameters.
    let mut params: Vec<usize> = Vec::new();
    let mut need = |b: &[Binding]| {
        for x in b {
            if !params.contains(&x.parameter) {
                params.push(x.parameter);
            }
        }
    };
    for &d in &defs {
        match &tpl.deformers[d].kind {
            DeformerKind::Warp(w) => need(&w.grid.bindings),
            DeformerKind::Rotation(r) => need(&r.grid.bindings),
        }
    }
    for &(_, t) in pairs {
        let a = &tpl.art_meshes[t];
        need(&a.grid.bindings);
        let bs: Vec<Binding> = a
            .blend_shapes
            .shapes
            .iter()
            .map(|s| Binding::new(s.parameter, Vec::new()))
            .chain(
                a.blend_shapes
                    .constraints
                    .iter()
                    .map(|c| Binding::new(c.constraint_parameter, Vec::new())),
            )
            .collect();
        need(&bs);
    }
    params.sort_unstable();
    // Parameter map: reuse same-ID parameters, add the others.
    let mut pmap: HashMap<usize, usize> = HashMap::new();
    for &p in &params {
        let tp = &tpl.parameters[p];
        if let Some(i) = dst.parameters.iter().position(|q| q.id == tp.id) {
            pmap.insert(p, i);
            rep.parameters_reused += 1;
        } else {
            let mut np = (**tp).clone();
            np.guid = new_guid();
            let group = tp
                .group
                .and_then(|g| tpl.parameter_groups.get(g))
                .and_then(|g| dst.parameter_groups.iter().position(|x| x.id == g.id));
            pmap.insert(p, crate::objects::add_parameter(dst, np, group));
            rep.parameters_added += 1;
        }
    }
    // Parts.
    let mut part_map: HashMap<usize, usize> = HashMap::new();
    if let (Some(a), Some(b)) = (tpl.root_part, dst.root_part) {
        part_map.insert(a, b);
    }
    for &p in &parts {
        let src = &tpl.parts[p];
        let parent = src.info.parent_part.and_then(|q| part_map.get(&q).copied());
        let i = crate::objects::create_part(dst, src.info.display_name(), parent);
        if dst.parts.iter().all(|x| x.info.id != src.info.id) {
            Arc::make_mut(&mut dst.parts[i]).info.id = src.info.id.clone();
        }
        part_map.insert(p, i);
        rep.parts += 1;
    }
    let map_part =
        |p: Option<usize>, dst: &Model| p.and_then(|q| part_map.get(&q).copied()).or(dst.root_part);
    // 1.–2. Deformers; top-level ones are transformed by the layout.
    let mut dmap: HashMap<usize, usize> = HashMap::new();
    for &d in &defs {
        let mut nd = (*tpl.deformers[d]).clone();
        nd.info.guid = new_guid();
        if dst.deformers.iter().any(|x| x.info.id == nd.info.id) {
            nd.info.id = crate::objects::unique_id(
                &nd.info.id,
                dst.deformers.iter().map(|x| x.info.id.as_str()),
            );
        }
        nd.target = nd.target.and_then(|t| dmap.get(&t).copied());
        let top = tpl.deformers[d].target.is_none();
        nd.info.parent_part = map_part(nd.info.parent_part, dst);
        match &mut nd.kind {
            DeformerKind::Warp(w) => {
                w.grid.bindings = remap_bindings(&w.grid.bindings, &pmap);
                w.blend_shapes = remap_blend(&w.blend_shapes, &pmap);
                if top {
                    for k in &mut w.grid.keyforms {
                        for c in k.positions.chunks_exact_mut(2) {
                            let q = layout.apply([c[0], c[1]]);
                            c.copy_from_slice(&q);
                        }
                    }
                }
                fresh_guids(&mut w.grid);
            }
            DeformerKind::Rotation(r) => {
                r.grid.bindings = remap_bindings(&r.grid.bindings, &pmap);
                r.blend_shapes = remap_blend(&r.blend_shapes, &pmap);
                if top {
                    // Children are in pixel units: scale with the layout.
                    for k in &mut r.grid.keyforms {
                        k.origin = layout.apply(k.origin);
                        k.scale *= layout.scale;
                    }
                }
                fresh_guids(&mut r.grid);
            }
        }
        let i = dst.deformers.len();
        let part = nd.info.parent_part;
        dst.deformers.push(Arc::new(nd));
        if let Some(p) = part {
            crate::objects::add_to_part(dst, p, PartChild::Deformer(i), None);
        }
        dmap.insert(d, i);
        rep.deformers += 1;
    }
    // 3. Meshes: T's current (default-state) canvas mesh → each keyform.
    let tvals: Vec<f32> = tpl.parameters.iter().map(|p| p.default).collect();
    let tframe = Evaluator::new(tpl).evaluate(&tvals);
    let dvals: Vec<f32> = dst.parameters.iter().map(|p| p.default).collect();
    let dframe = Evaluator::new(dst).evaluate(&dvals);
    for &(di, ti) in pairs {
        let t = tpl.art_meshes[ti].clone();
        let Some(tcanvas) = tframe.meshes.get(ti).map(|m| m.positions.clone()) else {
            continue;
        };
        let Some(dcanvas) = dframe.meshes.get(di).map(|m| m.positions.clone()) else {
            continue;
        };
        let remap = Remap::new(&tcanvas, &t.indices);
        let locs: Vec<_> = dcanvas
            .chunks_exact(2)
            .map(|c| remap.locate(layout.inverse([c[0], c[1]])))
            .collect();
        let root_level = t.target.is_none();
        let map_positions = |src: &[f32]| -> Vec<f32> {
            let mut out = Vec::with_capacity(locs.len() * 2);
            for l in &locs {
                let mut q = remap.apply(*l, src);
                if root_level {
                    // TODO(Q-0041): root-level template meshes follow the layout.
                    q = layout.apply(q);
                }
                out.extend_from_slice(&q);
            }
            out
        };
        let old_part = dst.art_meshes[di].info.parent_part;
        let new_part = map_part(t.info.parent_part, dst);
        let keep_order = dst.art_meshes[di]
            .grid
            .keyforms
            .first()
            .map(|k| k.draw_order);
        {
            let d = Arc::make_mut(&mut dst.art_meshes[di]);
            d.target = t.target.and_then(|x| dmap.get(&x).copied());
            let mut g = t.grid.clone();
            g.bindings = remap_bindings(&g.bindings, &pmap);
            for k in &mut g.keyforms {
                k.positions = map_positions(&k.positions);
                if o.draw_order == DrawOrderMode::DontChange {
                    if let Some(v) = keep_order {
                        k.draw_order = v;
                    }
                }
            }
            fresh_guids(&mut g);
            d.grid = g;
            let mut b = remap_blend(&t.blend_shapes, &pmap);
            for s in &mut b.shapes {
                s.form.positions = map_positions(&s.form.positions);
            }
            d.blend_shapes = b;
            // 4. Basic settings.
            if o.culling {
                d.culling = t.culling;
            }
            if o.color_blend {
                d.color_composition = t.color_composition;
            }
            if o.alpha_blend {
                d.alpha_composition = t.alpha_composition;
            }
        }
        if new_part != old_part {
            if let Some(p) = new_part {
                crate::objects::move_to_part(dst, &[crate::keys::ObjectRef::ArtMesh(di)], p)?;
            }
        }
        rep.meshes += 1;
    }
    if o.mask {
        // Clip lists remapped to the paired destination meshes.
        let first_dst = |t: usize| pairs.iter().find(|p| p.1 == t).map(|p| p.0);
        for &(di, ti) in pairs {
            let clip: Vec<usize> = tpl.art_meshes[ti]
                .clip
                .iter()
                .filter_map(|&c| first_dst(c))
                .collect();
            let d = Arc::make_mut(&mut dst.art_meshes[di]);
            d.clip = clip;
            d.invert_clip = tpl.art_meshes[ti].invert_clip;
        }
    }
    // 1. Physics settings whose parameters were all copied or exist.
    for s in &tpl.physics.settings {
        let ok = s
            .inputs
            .iter()
            .map(|x| x.parameter)
            .chain(s.outputs.iter().map(|x| x.parameter))
            .all(|p| {
                pmap.contains_key(&p)
                    || tpl
                        .parameters
                        .get(p)
                        .is_some_and(|q| dst.parameters.iter().any(|r| r.id == q.id))
            });
        if !ok || dst.physics.settings.iter().any(|x| x.name == s.name) {
            continue;
        }
        let find = |p: usize| {
            pmap.get(&p).copied().or_else(|| {
                let id = &tpl.parameters[p].id;
                dst.parameters.iter().position(|r| &r.id == id)
            })
        };
        let mut ns = s.clone();
        ns.guid = new_guid();
        ns.id = keyform_physics_like_next_id(dst);
        for x in &mut ns.inputs {
            x.parameter = find(x.parameter).unwrap_or(x.parameter);
        }
        for x in &mut ns.outputs {
            x.parameter = find(x.parameter).unwrap_or(x.parameter);
        }
        dst.physics.settings.push(ns);
        rep.physics += 1;
    }
    // 6. Template-only helpers (`@` IDs) go.
    let helpers: Vec<usize> = dmap
        .values()
        .copied()
        .filter(|&i| dst.deformers[i].info.id.starts_with('@'))
        .collect();
    let mut helpers = helpers;
    helpers.sort_unstable_by(|a, b| b.cmp(a));
    for d in helpers {
        crate::objects::delete_deformer(dst, d, &dvals_for(dst))?;
    }
    let mut hp: Vec<usize> = pmap
        .iter()
        .filter(|(&t, _)| tpl.parameters[t].id.starts_with('@'))
        .map(|(_, &d)| d)
        .collect();
    hp.sort_unstable_by(|a, b| b.cmp(a));
    for p in hp {
        crate::objects::remove_parameter(dst, p);
    }
    Ok(rep)
}

fn dvals_for(m: &Model) -> Vec<f32> {
    m.parameters.iter().map(|p| p.default).collect()
}

/// The next free `PhysicsSetting<n>` id.
fn keyform_physics_like_next_id(m: &Model) -> String {
    let used = |n: usize| {
        m.physics
            .settings
            .iter()
            .any(|s| s.id == format!("PhysicsSetting{n}"))
    };
    format!("PhysicsSetting{}", (1..).find(|&n| !used(n)).unwrap_or(1))
}

/// Matching inputs for every art mesh of `m` at its default state (canvas
/// points, draw-order rank, dominant colour from the atlas, warp parent
/// test). spec: §3
pub fn mesh_infos(m: &Model) -> Vec<MeshInfo> {
    let vals: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
    let frame = Evaluator::new(m).evaluate(&vals);
    let n = m.art_meshes.len().max(1);
    let mut order: Vec<usize> = (0..m.art_meshes.len()).collect();
    let dorder = |i: usize| frame.meshes.get(i).map_or(0.0, |x| x.draw_order);
    order.sort_by(|a, b| dorder(*a).total_cmp(&dorder(*b)));
    let mut rank = vec![0.0f32; m.art_meshes.len()];
    for (r, &i) in order.iter().enumerate() {
        rank[i] = r as f32 / n as f32;
    }
    m.art_meshes
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let points = frame
                .meshes
                .get(i)
                .map(|x| x.positions.chunks_exact(2).map(|c| [c[0], c[1]]).collect())
                .unwrap_or_default();
            let inside_parent: Option<Arc<dyn Fn(P) -> bool + Send + Sync>> = match a.target {
                Some(t) if matches!(m.deformers[t].kind, DeformerKind::Warp(_)) => frame
                    .deformers
                    .get(t)
                    .and_then(|s| s.map.clone())
                    .map(|map| {
                        Arc::new(move |p: P| {
                            map.inverse(p).is_some_and(|l| {
                                (0.0..=1.0).contains(&l[0]) && (0.0..=1.0).contains(&l[1])
                            })
                        }) as Arc<dyn Fn(P) -> bool + Send + Sync>
                    }),
                _ => None,
            };
            MeshInfo {
                name: a.info.display_name().to_owned(),
                points,
                rank: rank[i],
                color: None,
                inside_parent,
            }
        })
        .collect()
}
