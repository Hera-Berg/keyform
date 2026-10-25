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
use interp::interpolate;

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

/// Precomputed per-model data for repeated evaluation.
pub struct Evaluator<'m> {
    model: &'m Model,
    /// Deformers, parents before children.
    deformer_order: Vec<DeformerIndex>,
    /// Parent part per part, cycle-safe.
    part_parents: Vec<Option<PartIndex>>,
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
            .map(|p| {
                let r = interpolate(params, &values, &p.grid, &p.blend_shapes);
                PartState {
                    outside: r.as_ref().is_none_or(|r| r.outside),
                    form: r.map(|r| r.form).unwrap_or_default(),
                    visible: p.info.visible,
                }
            })
            .collect();
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
                DeformerKind::Warp(w) => interpolate(params, &values, &w.grid, &w.blend_shapes)
                    .map(|r| {
                        let mut points = r.form.positions;
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
                    interpolate(params, &values, &rd.grid, &rd.blend_shapes).map(|r| {
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
            .map(|a| {
                let r = interpolate(params, &values, &a.grid, &a.blend_shapes);
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
                    opacity: f.opacity.clamp(0.0, 1.0) * vis * d_op * part_op,
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
        for g in &m.glues {
            let Some(r) = interpolate(params, &values, &g.grid, &g.blend_shapes) else {
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
        }
    }
}
