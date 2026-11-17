//! Keyform model evaluation: parameter values → deformed geometry, final
//! appearance and render order. Pure CPU and deterministic.
//!
//! spec: 30-algorithms/evaluation-pipeline.md

pub mod deform;
pub mod draw_order;
pub mod interp;
pub mod keys;

use keyform_model::*;

pub use deform::{DeformerMap, RotationMap, WarpMap};
pub use interp::{interpolate, Blend, Interpolated};
pub mod extended;

/// Evaluated state of one part.
#[derive(Clone, Debug, PartialEq)]
pub struct PartState {
    pub form: PartForm,
    /// Outside its keys, or under an outside part.
    pub outside: bool,
    /// Own and ancestor `isVisible` flags.
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeformerState {
    /// Local→canvas map; `None` when the deformer could not be evaluated.
    pub map: Option<DeformerMap>,
    pub outside: bool,
    pub total_scale: f32,
    pub total_opacity: f32,
    pub total_multiply: [f32; 3],
    pub total_screen: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshState {
    /// Final canvas positions (after deformers and glue), x,y interleaved.
    pub positions: Vec<f32>,
    pub draw_order: f32,
    /// Interpolated (own) opacity.
    pub form_opacity: f32,
    /// Final opacity α (appearance.md §1 step 5).
    pub opacity: f32,
    pub multiply: [f32; 3],
    pub screen: [f32; 3],
    pub outside: bool,
    /// Hidden by a visibility flag on itself or an ancestor part.
    pub hidden: bool,
}

impl MeshState {
    /// Whether the mesh is drawn (evaluation-pipeline.md §4); clip sources
    /// are still drawn into masks even when this is false.
    pub fn is_drawn(&self) -> bool {
        !self.outside && !self.hidden && self.opacity > 0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub parts: Vec<PartState>,
    pub deformers: Vec<DeformerState>,
    pub meshes: Vec<MeshState>,
    /// Art meshes back to front.
    pub render_order: Vec<MeshIndex>,
    /// Deformed positions before glue of the meshes glue moved (the glue
    /// weight brushes measure these). spec: glue.md "Weight brushes"
    pub pre_glue: std::collections::HashMap<MeshIndex, Vec<f32>>,
}

impl Frame {
    /// Canvas positions of mesh `i` before glue (its final positions when
    /// glue didn't touch it).
    pub fn positions_before_glue(&self, i: MeshIndex) -> Option<&[f32]> {
        match self.pre_glue.get(&i) {
            Some(p) => Some(p),
            None => self.meshes.get(i).map(|m| &m.positions[..]),
        }
    }
}

/// Options that are not part of the model.
#[derive(Clone, Copy, Debug)]
pub struct EvalOptions {
    /// Global glue enable factor (Glue On/Off). spec: glue.md
    pub glue: f32,
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self { glue: 1.0 }
    }
}

/// Runtime values that replace evaluated ones (viewer playback).
#[derive(Clone, Debug, PartialEq)]
pub struct Overrides {
    /// Per part (model index): opacity replacing the interpolated one.
    pub part_opacity: Vec<Option<f32>>,
    /// Multiplies every drawable's opacity (motion `Model/Opacity`).
    pub model_opacity: f32,
    /// Added to an art mesh's interpolated local positions before its
    /// deformer (form animation, Adjustment Mode 2), by art mesh index.
    /// spec: 60-animation/form-animation-and-controllers.md §1 step 3
    pub local_mesh: std::collections::HashMap<usize, Vec<f32>>,
    /// The same for a warp deformer's grid points, by deformer index.
    pub local_warp: std::collections::HashMap<usize, Vec<f32>>,
}

impl Default for Overrides {
    fn default() -> Self {
        Self {
            part_opacity: Vec::new(),
            model_opacity: 1.0,
            local_mesh: Default::default(),
            local_warp: Default::default(),
        }
    }
}

fn add_offsets(p: &mut [f32], d: Option<&Vec<f32>>) {
    if let Some(d) = d.filter(|d| d.len() == p.len()) {
        for (x, dx) in p.iter_mut().zip(d) {
            *x += dx;
        }
    }
}

/// Precomputed per-model data for repeated evaluation.
pub struct Evaluator<'m> {
    model: &'m Model,
    /// Deformers, parents before children.
    deformer_order: Vec<DeformerIndex>,
    /// Parent part per part, cycle-safe.
    part_parents: Vec<Option<PartIndex>>,
    /// Generated extended-interpolation grids, by object index
    /// (extended-interpolation.md §1; used instead of the source grid).
    ext: ExtendedGrids,
}

#[derive(Default)]
struct ExtendedGrids {
    parts: std::collections::HashMap<usize, keyform_model::KeyformGrid<PartForm>>,
    meshes:
        std::collections::HashMap<usize, keyform_model::KeyformGrid<keyform_model::ArtMeshForm>>,
    warps: std::collections::HashMap<usize, keyform_model::KeyformGrid<keyform_model::WarpForm>>,
    rotations:
        std::collections::HashMap<usize, keyform_model::KeyformGrid<keyform_model::RotationForm>>,
    glues: std::collections::HashMap<usize, keyform_model::KeyformGrid<keyform_model::GlueForm>>,
    /// Extended blend-shape sets (extended-interpolation.md §4).
    parts_b: std::collections::HashMap<usize, keyform_model::BlendShapes<PartForm>>,
    meshes_b:
        std::collections::HashMap<usize, keyform_model::BlendShapes<keyform_model::ArtMeshForm>>,
    warps_b: std::collections::HashMap<usize, keyform_model::BlendShapes<keyform_model::WarpForm>>,
    rotations_b:
        std::collections::HashMap<usize, keyform_model::BlendShapes<keyform_model::RotationForm>>,
    glues_b: std::collections::HashMap<usize, keyform_model::BlendShapes<keyform_model::GlueForm>>,
}

impl ExtendedGrids {
    fn of(m: &Model) -> Self {
        use extended::{extended_blend, extended_grid};
        let mut e = Self::default();
        let ps = &m.parameters;
        for (i, p) in m.parts.iter().enumerate() {
            if let Some(g) = extended_grid(&p.grid) {
                e.parts.insert(i, g);
            }
            if let Some(b) = extended_blend(ps, &p.grid, &p.blend_shapes) {
                e.parts_b.insert(i, b);
            }
        }
        for (i, a) in m.art_meshes.iter().enumerate() {
            if let Some(g) = extended_grid(&a.grid) {
                e.meshes.insert(i, g);
            }
            if let Some(b) = extended_blend(ps, &a.grid, &a.blend_shapes) {
                e.meshes_b.insert(i, b);
            }
        }
        for (i, d) in m.deformers.iter().enumerate() {
            match &d.kind {
                DeformerKind::Warp(w) => {
                    if let Some(g) = extended_grid(&w.grid) {
                        e.warps.insert(i, g);
                    }
                    if let Some(b) = extended_blend(ps, &w.grid, &w.blend_shapes) {
                        e.warps_b.insert(i, b);
                    }
                }
                DeformerKind::Rotation(r) => {
                    if let Some(g) = extended_grid(&r.grid) {
                        e.rotations.insert(i, g);
                    }
                    if let Some(b) = extended_blend(ps, &r.grid, &r.blend_shapes) {
                        e.rotations_b.insert(i, b);
                    }
                }
            }
        }
        for (i, g) in m.glues.iter().enumerate() {
            if let Some(x) = extended_grid(&g.grid) {
                e.glues.insert(i, x);
            }
            if let Some(b) = extended_blend(ps, &g.grid, &g.blend_shapes) {
                e.glues_b.insert(i, b);
            }
        }
        e
    }
}

impl<'m> Evaluator<'m> {
    pub fn new(model: &'m Model) -> Self {
        let n = model.deformers.len();
        let mut order = Vec::with_capacity(n);
        let mut state = vec![0u8; n]; // 0 new, 1 visiting, 2 done
        for start in 0..n {
            let mut chain = Vec::new();
            let mut cur = Some(start);
            while let Some(d) = cur {
                if state[d] != 0 {
                    break;
                }
                state[d] = 1;
                chain.push(d);
                cur = model.deformers[d].target.filter(|&t| t < n);
            }
            for &d in chain.iter().rev() {
                state[d] = 2;
                order.push(d);
            }
        }
        let part_parents = (0..model.parts.len())
            .map(|p| {
                model.parts[p]
                    .info
                    .parent_part
                    .filter(|&q| q < model.parts.len() && q != p)
            })
            .collect();
        Self {
            model,
            deformer_order: order,
            part_parents,
            ext: ExtendedGrids::of(model),
        }
    }

    pub fn model(&self) -> &'m Model {
        self.model
    }

    /// Default value of every parameter.
    pub fn default_values(&self) -> Vec<f32> {
        self.model.parameters.iter().map(|p| p.default).collect()
    }

    /// Ancestor parts of an object whose parent part is `p`, nearest first.
    fn ancestors(&self, mut p: Option<PartIndex>) -> impl Iterator<Item = PartIndex> + '_ {
        let mut steps = 0;
        std::iter::from_fn(move || {
            let cur = p?;
            steps += 1;
            if steps > self.part_parents.len() {
                return None;
            }
            p = self.part_parents[cur];
            Some(cur)
        })
    }

    pub fn evaluate(&self, values: &[f32]) -> Frame {
        self.evaluate_with(values, EvalOptions::default())
    }

    pub fn evaluate_with(&self, values: &[f32], opts: EvalOptions) -> Frame {
        self.evaluate_full(values, opts, &Overrides::default())
    }

    /// Evaluates with runtime overrides (motion PartOpacity curves, pose,
    /// model opacity). spec: 60-animation/motion3-json.md §3 steps 4–5
    pub fn evaluate_full(&self, values: &[f32], opts: EvalOptions, ov: &Overrides) -> Frame {
        let m = self.model;
        let params = &m.parameters;
        // Clamp non-repeating parameters (keyform-interpolation.md notation).
        let values: Vec<f32> = params
            .iter()
            .enumerate()
            .map(|(i, p)| p.clamp(values.get(i).copied().unwrap_or(p.default)))
            .collect();

        // Stage 2: parts.
        let mut parts: Vec<PartState> = m
            .parts
            .iter()
            .enumerate()
            .map(|(pi, p)| {
                let grid = self.ext.parts.get(&pi).unwrap_or(&p.grid);
                let blend = self.ext.parts_b.get(&pi).unwrap_or(&p.blend_shapes);
                let r = interpolate(params, &values, grid, blend);
                PartState {
                    outside: r.as_ref().is_none_or(|r| r.outside),
                    form: r.map(|r| r.form).unwrap_or_default(),
                    visible: p.info.visible,
                }
            })
            .collect();
        for (part, o) in parts.iter_mut().zip(&ov.part_opacity) {
            if let Some(o) = o {
                part.form.opacity = o.clamp(0.0, 1.0);
            }
        }
        let own_outside: Vec<bool> = parts.iter().map(|p| p.outside).collect();
        for (i, p) in m.parts.iter().enumerate() {
            let anc: Vec<PartIndex> = self.ancestors(p.info.parent_part).collect();
            parts[i].outside |= anc.iter().any(|&a| own_outside[a]);
            parts[i].visible &= anc.iter().all(|&a| m.parts[a].info.visible);
        }
        let part_hidden_or_outside = |parent: Option<PartIndex>| -> (bool, bool) {
            match parent {
                Some(q) if q < parts.len() => (!parts[q].visible, parts[q].outside),
                _ => (false, false),
            }
        };

        // Stage 3: deformers, parents first.
        let mut deformers: Vec<DeformerState> = vec![
            DeformerState {
                map: None,
                outside: true,
                total_scale: 1.0,
                total_opacity: 1.0,
                total_multiply: [1.0; 3],
                total_screen: [0.0; 3],
            };
            m.deformers.len()
        ];
        for &di in &self.deformer_order {
            let d = &m.deformers[di];
            let parent = d.target.map(|t| deformers[t].clone());
            let parent_map = parent.as_ref().and_then(|p| p.map.as_ref());
            let parent_outside = parent.as_ref().is_some_and(|p| p.outside);
            let (_, part_out) = part_hidden_or_outside(d.info.parent_part);
            let (pscale, popacity, pmul, pscr) = match &parent {
                Some(p) => (
                    p.total_scale,
                    p.total_opacity,
                    p.total_multiply,
                    p.total_screen,
                ),
                None => (1.0, 1.0, [1.0; 3], [0.0; 3]),
            };
            let st = match &d.kind {
                DeformerKind::Warp(w) => interpolate(
                    params,
                    &values,
                    self.ext.warps.get(&di).unwrap_or(&w.grid),
                    self.ext.warps_b.get(&di).unwrap_or(&w.blend_shapes),
                )
                .map(|r| {
                    let mut points = r.form.positions;
                    add_offsets(&mut points, ov.local_warp.get(&di));
                    if let Some(pm) = parent_map {
                        for xy in points.as_chunks_mut::<2>().0 {
                            let q = pm.map([xy[0], xy[1]]);
                            xy[0] = q[0];
                            xy[1] = q[1];
                        }
                    }
                    let ok = w.cols >= 1 && w.rows >= 1 && points.len() == 2 * w.point_count();
                    (
                        ok.then_some({
                            DeformerMap::Warp(WarpMap {
                                cols: w.cols as usize,
                                rows: w.rows as usize,
                                quad: w.quad,
                                points,
                            })
                        }),
                        r.outside,
                        pscale,
                        r.form.opacity,
                        r.form.multiply,
                        r.form.screen,
                    )
                }),
                DeformerKind::Rotation(rd) => {
                    let grid = self.ext.rotations.get(&di).unwrap_or(&rd.grid);
                    let blend = self.ext.rotations_b.get(&di).unwrap_or(&rd.blend_shapes);
                    interpolate(params, &values, grid, blend).map(|r| {
                        let f = &r.form;
                        // §3.2: origin through the parent, inherited turn.
                        let (origin, delta) = match parent_map {
                            Some(pm) => {
                                (pm.map(f.origin), deform::inherited_rotation(pm, f.origin))
                            }
                            None => (f.origin, 0.0),
                        };
                        let scale = f.scale * pscale;
                        (
                            Some(DeformerMap::Rotation(RotationMap {
                                origin,
                                angle: f.angle + delta + rd.base_angle,
                                scale,
                                reflect_x: f.reflect_x,
                                reflect_y: f.reflect_y,
                            })),
                            r.outside,
                            scale,
                            f.opacity,
                            f.multiply,
                            f.screen,
                        )
                    })
                }
            };
            let Some((map, outside, scale, opacity, mul, scr)) = st else {
                continue;
            };
            // spec: deformers.md §4 accumulated totals.
            deformers[di] = DeformerState {
                outside: outside
                    || parent_outside
                    || part_out
                    || map.is_none()
                    || (d.target.is_some() && parent_map.is_none()),
                map,
                total_scale: scale,
                total_opacity: opacity * popacity,
                total_multiply: [mul[0] * pmul[0], mul[1] * pmul[1], mul[2] * pmul[2]],
                total_screen: [
                    1.0 - (1.0 - scr[0]) * (1.0 - pscr[0]),
                    1.0 - (1.0 - scr[1]) * (1.0 - pscr[1]),
                    1.0 - (1.0 - scr[2]) * (1.0 - pscr[2]),
                ],
            };
        }

        // Stage 4: art meshes.
        let mut meshes: Vec<MeshState> = m
            .art_meshes
            .iter()
            .enumerate()
            .map(|(ai, a)| {
                let grid = self.ext.meshes.get(&ai).unwrap_or(&a.grid);
                let blend = self.ext.meshes_b.get(&ai).unwrap_or(&a.blend_shapes);
                let r = interpolate(params, &values, grid, blend);
                let (hidden_parent, outside_parent) = part_hidden_or_outside(a.info.parent_part);
                let target = a.target.and_then(|t| deformers.get(t));
                let Some(mut r) = r else {
                    return MeshState {
                        positions: a.positions.clone(),
                        draw_order: 500.0,
                        form_opacity: 0.0,
                        opacity: 0.0,
                        multiply: [1.0; 3],
                        screen: [0.0; 3],
                        outside: true,
                        hidden: true,
                    };
                };
                let mut positions = std::mem::take(&mut r.form.positions);
                add_offsets(&mut positions, ov.local_mesh.get(&ai));
                let mut outside = r.outside || outside_parent;
                if let Some(t) = target {
                    match &t.map {
                        Some(map) => {
                            for xy in positions.as_chunks_mut::<2>().0 {
                                let q = map.map([xy[0], xy[1]]);
                                xy[0] = q[0];
                                xy[1] = q[1];
                            }
                        }
                        None => outside = true,
                    }
                    outside |= t.outside;
                }
                // spec: 40-rendering/appearance.md §1
                let (d_op, d_mul, d_scr) = target.map_or((1.0, [1.0; 3], [0.0; 3]), |t| {
                    (t.total_opacity, t.total_multiply, t.total_screen)
                });
                let mut part_op = 1.0;
                for p in self.ancestors(a.info.parent_part) {
                    part_op *= parts[p].form.opacity;
                    if m.parts[p].offscreen {
                        break;
                    }
                }
                let vis = if a.info.visible { 1.0 } else { 0.0 };
                let f = &r.form;
                let mo = f.multiply;
                let so = f.screen;
                MeshState {
                    positions,
                    draw_order: f.draw_order,
                    form_opacity: f.opacity,
                    opacity: f.opacity.clamp(0.0, 1.0)
                        * vis
                        * d_op
                        * part_op
                        * ov.model_opacity.clamp(0.0, 1.0),
                    multiply: [
                        (mo[0] * d_mul[0]).clamp(0.0, 1.0),
                        (mo[1] * d_mul[1]).clamp(0.0, 1.0),
                        (mo[2] * d_mul[2]).clamp(0.0, 1.0),
                    ],
                    screen: [
                        (so[0] + d_scr[0] - so[0] * d_scr[0]).clamp(0.0, 1.0),
                        (so[1] + d_scr[1] - so[1] * d_scr[1]).clamp(0.0, 1.0),
                        (so[2] + d_scr[2] - so[2] * d_scr[2]).clamp(0.0, 1.0),
                    ],
                    outside,
                    hidden: !a.info.visible || hidden_parent,
                }
            })
            .collect();

        // Stage 7: glue, in model order, each reading the current forms.
        let mut pre_glue = std::collections::HashMap::new();
        for (gi, g) in m.glues.iter().enumerate() {
            let grid = self.ext.glues.get(&gi).unwrap_or(&g.grid);
            let blend = self.ext.glues_b.get(&gi).unwrap_or(&g.blend_shapes);
            let Some(r) = interpolate(params, &values, grid, blend) else {
                continue;
            };
            let i = r.form.intensity.clamp(0.0, 1.0) * opts.glue;
            if i < 1e-6 {
                continue;
            }
            let (Some(a), Some(b)) = (g.mesh_a, g.mesh_b) else {
                continue;
            };
            if a >= meshes.len() || b >= meshes.len() {
                continue;
            }
            let pa = meshes[a].positions.clone();
            let pb = meshes[b].positions.clone();
            pre_glue.entry(a).or_insert_with(|| pa.clone());
            pre_glue.entry(b).or_insert_with(|| pb.clone());
            for (pair, w) in g.pairs.iter().zip(&g.weights) {
                let (Some(va), Some(vb)) = *pair else {
                    continue;
                };
                if 2 * va + 1 >= pa.len() || 2 * vb + 1 >= pb.len() {
                    continue;
                }
                let (ax, ay, bx, by) = (pa[2 * va], pa[2 * va + 1], pb[2 * vb], pb[2 * vb + 1]);
                meshes[a].positions[2 * va] = ax + (bx - ax) * w[0] * i;
                meshes[a].positions[2 * va + 1] = ay + (by - ay) * w[0] * i;
                meshes[b].positions[2 * vb] = bx + (ax - bx) * w[1] * i;
                meshes[b].positions[2 * vb + 1] = by + (ay - by) * w[1] * i;
            }
        }

        let render_order = draw_order::render_order(m, &parts, &meshes);
        Frame {
            parts,
            deformers,
            meshes,
            render_order,
            pre_glue,
        }
    }
}
