//! [`Model`] → moc3. spec: 10-formats/moc3.md §8–9, 70-advanced/export-dialog.md

use std::collections::HashMap;

use keyform_eval::keys::insert_key;
use keyform_model::*;

use super::layout::c;
use super::{CanvasInfo, Moc3Raw, SectionData};
use crate::error::{invalid, FormatError, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct ExportOptions {
    /// Header version byte 1…6.
    pub version: u8,
    pub export_invisible_parts: bool,
    pub export_invisible_art_meshes: bool,
    pub export_sketch_parts: bool,
    /// Export origin in canvas pixels.
    pub origin: [f32; 2],
    pub pixels_per_unit: f32,
}

impl ExportOptions {
    /// Options from the model's stored export settings. spec: export-dialog.md §1
    pub fn for_model(m: &Model) -> Self {
        let e = &m.export;
        let (w, h) = (m.canvas.width as f32, m.canvas.height as f32);
        Self {
            version: e
                .moc_version
                .unwrap_or_else(|| version_for_target(m.target_version)),
            export_invisible_parts: e.export_invisible_parts,
            export_invisible_art_meshes: e.export_invisible_art_meshes,
            export_sketch_parts: e.export_sketch_parts,
            origin: [e.origin_normalized[0] * w, e.origin_normalized[1] * h],
            // Default: the shorter canvas side is one unit (moc3.md §5).
            pixels_per_unit: e.pixels_per_unit.unwrap_or(w.min(h)),
        }
    }
}

/// Default moc3 version for a model target version (cmo3.md §2, moc3.md §1).
pub fn version_for_target(target: i32) -> u8 {
    match target {
        3000 => 1,
        3030 => 2,
        400000 => 3,
        4020000 => 4,
        5000000 => 5,
        _ => 6,
    }
}

#[derive(Clone, Debug, Default)]
pub struct ExportReport {
    pub warnings: Vec<String>,
    /// Model texture indices in export order (`texture_00.png`, …).
    pub textures: Vec<usize>,
    /// Exported objects, as model indices in moc3 order.
    pub parts: Vec<PartIndex>,
    /// Every part except the root, hidden and sketch ones included, in the
    /// same tree order (cdi3.json lists these).
    pub all_parts: Vec<PartIndex>,
    pub deformers: Vec<DeformerIndex>,
    pub art_meshes: Vec<MeshIndex>,
    pub parameters: Vec<ParamIndex>,
    pub glues: Vec<GlueIndex>,
}

/// Parameter bindings, keyform bindings and the keys pool. spec §8.2
#[derive(Default)]
struct Pools {
    /// (model parameter, key bit patterns) in first-seen order.
    param_bindings: Vec<(ParamIndex, Vec<u32>)>,
    pb_lookup: HashMap<(ParamIndex, Vec<u32>), usize>,
    /// Keyform binding → list of first-seen parameter-binding ids.
    keyform_bindings: Vec<Vec<usize>>,
    kb_lookup: HashMap<Vec<usize>, usize>,
}

impl Pools {
    fn new() -> Self {
        let mut p = Self::default();
        // Keyform binding 0 is always the empty list.
        p.keyform_bindings.push(Vec::new());
        p.kb_lookup.insert(Vec::new(), 0);
        p
    }

    fn keyform_binding(&mut self, bindings: &[Binding]) -> i32 {
        let mut ids = Vec::with_capacity(bindings.len());
        for b in bindings {
            let key = (
                b.parameter,
                b.keys.iter().map(|k| k.to_bits()).collect::<Vec<_>>(),
            );
            let n = self.param_bindings.len();
            let id = *self.pb_lookup.entry(key.clone()).or_insert(n);
            if id == n {
                self.param_bindings.push(key);
            }
            ids.push(id);
        }
        let n = self.keyform_bindings.len();
        let kb = *self.kb_lookup.entry(ids.clone()).or_insert(n);
        if kb == n {
            self.keyform_bindings.push(ids);
        }
        kb as i32
    }
}

/// Appends `positions` to the pool, zero-padded so the next block starts on a
/// 16-float boundary (§8.3). Returns the block's float index.
/// Blend-shape bindings being built (moc3.md §8.4).
#[derive(Default)]
struct BsBuild {
    /// (export parameter index, keys, base key index).
    pb: Vec<(usize, Vec<f32>, i32)>,
    /// (parameter binding id, keyform begin, keyform count, constraint begin, constraint count).
    kb: Vec<[i32; 5]>,
    /// (export parameter index, (key, weight) list).
    constraints: Vec<(usize, Vec<(f32, f32)>)>,
    constraint_indices: Vec<i32>,
}

impl BsBuild {
    /// Adds an object's blend shapes: per blend-shape parameter (in export
    /// order) its key list with the base at 0, one delta keyform per key
    /// (pushed by `push`, which returns the keyform index) and its
    /// constraints. Returns the object's keyform-binding range.
    fn add<F: keyform_eval::Blend>(
        &mut self,
        m: &Model,
        param_pos: &HashMap<usize, usize>,
        grid: &KeyformGrid<F>,
        blend: &keyform_model::BlendShapes<F>,
        push: &mut dyn FnMut(&F) -> Result<i32>,
    ) -> Result<(i32, i32)> {
        let Some(base) = keyform_eval::interp::base_keyform(&m.parameters, grid) else {
            return invalid("blend shapes on an object without keyforms");
        };
        let mut params: Vec<usize> = blend.shapes.iter().map(|s| s.parameter).collect();
        params.sort_by_key(|p| param_pos.get(p).copied().unwrap_or(usize::MAX));
        params.dedup();
        let begin = count(self.kb.len())?;
        for p in params {
            let Some(&pe) = param_pos.get(&p) else {
                continue;
            };
            let max = m.parameters.get(p).map_or(1.0, |q| q.max);
            // Entries, then (0, base); duplicate keys: first wins; sorted.
            let mut curve: Vec<(f32, &F)> = Vec::new();
            for s in blend.shapes.iter().filter(|s| s.parameter == p) {
                let k = s.key.unwrap_or(max);
                if !curve.iter().any(|c| c.0 == k) {
                    curve.push((k, &s.form));
                }
            }
            if !curve.iter().any(|c| c.0 == 0.0) {
                curve.push((0.0, base));
            }
            curve.sort_by(|a, b| a.0.total_cmp(&b.0));
            let keys: Vec<f32> = curve.iter().map(|c| c.0).collect();
            let base_idx = count(keys.iter().position(|k| *k == 0.0).unwrap_or(0))?;
            let pb = match self.pb.iter().position(|(q, k, b)| {
                *q == pe
                    && k.iter()
                        .map(|x| x.to_bits())
                        .eq(keys.iter().map(|x| x.to_bits()))
                    && *b == base_idx
            }) {
                Some(i) => i,
                None => {
                    self.pb.push((pe, keys.clone(), base_idx));
                    self.pb.len() - 1
                }
            };
            let mut first = None;
            for (_, f) in &curve {
                let mut d = f.zeroed();
                d.add_scaled(f, 1.0);
                d.add_scaled(base, -1.0);
                let i = push(&d)?;
                first.get_or_insert(i);
            }
            // Constraints of this parameter, one per constraint parameter.
            let cbegin = count(self.constraint_indices.len())?;
            let mut cps: Vec<usize> = blend
                .constraints
                .iter()
                .filter(|c| c.blend_parameter == p)
                .map(|c| c.constraint_parameter)
                .collect();
            cps.dedup();
            for q in cps {
                let Some(&qe) = param_pos.get(&q) else {
                    continue;
                };
                let pts: Vec<(f32, f32)> = blend
                    .constraints
                    .iter()
                    .filter(|c| c.blend_parameter == p && c.constraint_parameter == q)
                    .map(|c| (c.value, c.weight))
                    .collect();
                let ci = match self
                    .constraints
                    .iter()
                    .position(|c| c.0 == qe && c.1 == pts)
                {
                    Some(i) => i,
                    None => {
                        self.constraints.push((qe, pts));
                        self.constraints.len() - 1
                    }
                };
                self.constraint_indices.push(count(ci)?);
            }
            let ccount = count(self.constraint_indices.len())? - cbegin;
            self.kb.push([
                count(pb)?,
                first.unwrap_or(0),
                count(curve.len())?,
                cbegin,
                ccount,
            ]);
        }
        Ok((begin, count(self.kb.len())? - begin))
    }
}

fn push_positions(pool: &mut Vec<f32>, positions: &[f32]) -> i32 {
    let begin = pool.len();
    pool.extend_from_slice(positions);
    pool.resize(begin + positions.len().div_ceil(16) * 16, 0.0);
    begin as i32
}

fn to_units(xy: &mut [f32], o: &ExportOptions) {
    for p in xy.as_chunks_mut::<2>().0 {
        // Multiply by the f32 reciprocal: bit-identical to the reference export.
        let s = 1.0 / o.pixels_per_unit;
        p[0] = (p[0] - o.origin[0]) * s;
        p[1] = (p[1] - o.origin[1]) * s;
    }
}

fn i32s(v: Vec<i32>) -> SectionData {
    SectionData::I32(v)
}
fn f32s(v: Vec<f32>) -> SectionData {
    SectionData::F32(v)
}
fn count(n: usize) -> Result<i32> {
    i32::try_from(n).map_err(|_| FormatError::LimitExceeded("moc3 count".into()))
}

/// Exports `m` as moc3 data (write it with [`Moc3Raw::write`]).
pub fn export_moc3(m: &Model, o: &ExportOptions) -> Result<(Moc3Raw, ExportReport)> {
    // §9 step 1: extended interpolation baked into plain keyforms.
    if let Some(b) = keyform_eval::extended::bake_extended(m) {
        return export_moc3(&b, o);
    }
    let v = o.version;
    if !(1..=super::LATEST_VERSION).contains(&v) {
        return Err(FormatError::Unsupported(format!("moc3 version {v}")));
    }
    if o.pixels_per_unit.is_nan()
        || o.pixels_per_unit <= 0.0
        || !o.origin.iter().all(|x| x.is_finite())
    {
        return invalid("bad export origin / pixels per unit");
    }
    let mut rep = ExportReport::default();
    let root = m.root_part;

    // ---- §9 step 6: which objects are exported.
    let np = m.parts.len();
    let mut part_ok = vec![false; np];
    {
        // Parents before children so a dropped part drops its subtree.
        let mut stack: Vec<(PartIndex, bool)> = root.map(|r| (r, true)).into_iter().collect();
        let mut seen = vec![false; np];
        while let Some((p, parent_ok)) = stack.pop() {
            if p >= np || seen[p] {
                continue;
            }
            seen[p] = true;
            let part = &m.parts[p];
            let ok = parent_ok
                && (Some(p) == root
                    || ((part.info.visible || o.export_invisible_parts)
                        && (!part.sketch || o.export_sketch_parts)));
            part_ok[p] = ok;
            for c in &part.children {
                if let PartChild::Part(q) = *c {
                    stack.push((q, ok));
                }
            }
        }
    }
    let in_exported_part = |pp: Option<PartIndex>| pp.is_none_or(|p| p < np && part_ok[p]);
    let mut mesh_ok = vec![false; m.art_meshes.len()];
    for (i, a) in m.art_meshes.iter().enumerate() {
        if !in_exported_part(a.info.parent_part)
            || !(a.info.visible || o.export_invisible_art_meshes)
        {
            continue;
        }
        if a.texture.is_none_or(|t| t >= m.textures.len()) {
            // spec §8.6: the reference fails the export here; Keyform drops the mesh and warns.
            rep.warnings.push(format!(
                "Not found texture atlas: {}",
                a.info.display_name()
            ));
            continue;
        }
        mesh_ok[i] = true;
    }

    // ---- §8.1 object order.
    let exported_part = |p: PartIndex| Some(p) != root && part_ok[p];
    // Children of every part, then recursively each child (deformers.md-style grouping).
    fn visit_parts(
        m: &Model,
        p: PartIndex,
        ok: &dyn Fn(PartIndex) -> bool,
        out: &mut Vec<PartIndex>,
        depth: usize,
    ) {
        if depth > m.parts.len() {
            return;
        }
        let kids: Vec<PartIndex> = m.parts[p]
            .children
            .iter()
            .filter_map(|c| match c {
                PartChild::Part(q) if *q < m.parts.len() && ok(*q) && !out.contains(q) => Some(*q),
                _ => None,
            })
            .collect();
        out.extend(&kids);
        for q in kids {
            visit_parts(m, q, ok, out, depth + 1);
        }
    }
    let mut part_order = Vec::new();
    if let Some(r) = root {
        visit_parts(m, r, &exported_part, &mut part_order, 0);
        visit_parts(m, r, &|p| Some(p) != root, &mut rep.all_parts, 0);
    }
    let part_index: HashMap<PartIndex, i32> = part_order
        .iter()
        .enumerate()
        .map(|(i, &p)| (p, i as i32))
        .collect();
    let export_parent_part = |mut pp: Option<PartIndex>| -> i32 {
        let mut steps = 0;
        while let Some(p) = pp {
            if let Some(&i) = part_index.get(&p) {
                return i;
            }
            steps += 1;
            if steps > np {
                break;
            }
            pp = m.parts.get(p).and_then(|x| x.info.parent_part);
        }
        -1
    };

    let nd = m.deformers.len();
    let mut children: Vec<Vec<DeformerIndex>> = vec![Vec::new(); nd];
    let mut def_order: Vec<DeformerIndex> = Vec::new();
    for (i, d) in m.deformers.iter().enumerate() {
        match d.target.filter(|&t| t < nd) {
            Some(t) => children[t].push(i),
            None => def_order.push(i),
        }
    }
    fn visit_def(
        d: DeformerIndex,
        ch: &[Vec<DeformerIndex>],
        out: &mut Vec<DeformerIndex>,
        depth: usize,
    ) {
        if depth > ch.len() {
            return;
        }
        out.extend(&ch[d]);
        for &k in &ch[d] {
            visit_def(k, ch, out, depth + 1);
        }
    }
    for r in def_order.clone() {
        visit_def(r, &children, &mut def_order, 0);
    }
    if def_order.len() != nd {
        return invalid("deformer hierarchy has a cycle");
    }
    let def_index: Vec<i32> = {
        let mut v = vec![-1; nd];
        for (i, &d) in def_order.iter().enumerate() {
            v[d] = i as i32;
        }
        v
    };
    let mesh_order: Vec<MeshIndex> = (0..m.art_meshes.len()).filter(|&i| mesh_ok[i]).collect();
    let mesh_index: HashMap<MeshIndex, i32> = mesh_order
        .iter()
        .enumerate()
        .map(|(i, &a)| (a, i as i32))
        .collect();

    // Parameters in Parameter-palette order.
    let mut param_order = Vec::new();
    fn walk(m: &Model, entries: &[ParamEntry], out: &mut Vec<ParamIndex>, depth: usize) {
        for e in entries {
            match *e {
                ParamEntry::Parameter(p) if p < m.parameters.len() && !out.contains(&p) => {
                    out.push(p)
                }
                ParamEntry::Group(g) if g < m.parameter_groups.len() && depth < 64 => {
                    walk(m, &m.parameter_groups[g].children, out, depth + 1)
                }
                _ => {}
            }
        }
    }
    walk(m, &m.root_parameter_entries, &mut param_order, 0);
    for p in 0..m.parameters.len() {
        if !param_order.contains(&p) {
            param_order.push(p);
        }
    }
    let param_rank: Vec<usize> = {
        let mut r = vec![0; m.parameters.len()];
        for (i, &p) in param_order.iter().enumerate() {
            r[p] = i;
        }
        r
    };

    // Glue in part-tree traversal order.
    let mut glue_order = Vec::new();
    fn walk_glue(m: &Model, p: PartIndex, out: &mut Vec<GlueIndex>, seen: &mut [bool]) {
        if seen[p] {
            return;
        }
        seen[p] = true;
        for c in &m.parts[p].children {
            match *c {
                PartChild::Glue(g) if g < m.glues.len() => out.push(g),
                PartChild::Part(q) if q < m.parts.len() => walk_glue(m, q, out, seen),
                _ => {}
            }
        }
    }
    if let Some(r) = root {
        walk_glue(m, r, &mut glue_order, &mut vec![false; np]);
    }
    for g in 0..m.glues.len() {
        if !glue_order.contains(&g) {
            glue_order.push(g);
        }
    }
    glue_order.retain(|&g| {
        let gl = &m.glues[g];
        gl.mesh_a.is_some_and(|a| mesh_ok.get(a) == Some(&true))
            && gl.mesh_b.is_some_and(|b| mesh_ok.get(b) == Some(&true))
    });

    let uses_blend_shapes = m.parts.iter().any(|p| !p.blend_shapes.is_empty())
        || m.art_meshes.iter().any(|a| !a.blend_shapes.is_empty())
        || m.glues.iter().any(|g| !g.blend_shapes.is_empty())
        || m.deformers.iter().any(|d| match &d.kind {
            DeformerKind::Warp(w) => !w.blend_shapes.is_empty(),
            DeformerKind::Rotation(r) => !r.blend_shapes.is_empty(),
        });
    if uses_blend_shapes {
        if v < 4 {
            rep.warnings.push(
                "blend shapes are not supported by this moc3 version and were dropped".into(),
            );
        } else if m.parts.iter().any(|p| !p.blend_shapes.is_empty())
            || m.glues.iter().any(|g| !g.blend_shapes.is_empty())
            || m.deformers
                .iter()
                .any(|d| matches!(&d.kind, DeformerKind::Rotation(r) if !r.blend_shapes.is_empty()))
        {
            // TODO(Q-0036): parts / rotation deformers / glue (v5.0) have no
            // reference export to compare against yet.
            return Err(FormatError::Unsupported(
                "moc3 export of blend shapes on parts, rotation deformers or glue is not implemented yet".into(),
            ));
        }
    }
    let param_pos: HashMap<usize, usize> = param_order
        .iter()
        .enumerate()
        .map(|(i, &p)| (p, i))
        .collect();
    let mut bs = BsBuild::default();
    let mut bs_warps: Vec<(i32, i32, i32)> = Vec::new();
    let mut bs_meshes: Vec<(i32, i32, i32)> = Vec::new();

    // §9 step 4: repeat parameters get keys at min and max.
    fn prep<F: keyform_eval::interp::Blend>(m: &Model, g: &KeyformGrid<F>) -> KeyformGrid<F> {
        let mut g = g.clone();
        for j in 0..g.bindings.len() {
            let Some(p) = m.parameters.get(g.bindings[j].parameter) else {
                continue;
            };
            if p.repeat {
                insert_key(&mut g, j, p, p.min);
                insert_key(&mut g, j, p, p.max);
            }
        }
        g
    }

    let mut raw = Moc3Raw::new(
        v,
        CanvasInfo {
            pixels_per_unit: o.pixels_per_unit,
            origin_x: o.origin[0],
            origin_y: o.origin[1],
            width: m.canvas.width as f32,
            height: m.canvas.height as f32,
            flags: 0,
        },
    );
    let mut pools = Pools::new();
    let mut positions: Vec<f32> = Vec::new();
    let mut mul_pool: Vec<Color> = Vec::new();
    let mut scr_pool: Vec<Color> = Vec::new();

    // Binding numbering follows the reference: art meshes, deformers, parts,
    // glue, each in export order (observed).
    for &ai in &mesh_order {
        pools.keyform_binding(&prep(m, &m.art_meshes[ai].grid).bindings);
    }
    for &d in &def_order {
        match &m.deformers[d].kind {
            DeformerKind::Warp(w) => pools.keyform_binding(&prep(m, &w.grid).bindings),
            DeformerKind::Rotation(r) => pools.keyform_binding(&prep(m, &r.grid).bindings),
        };
    }
    for &p in &part_order {
        if m.parts[p].draw_order_group {
            pools.keyform_binding(&prep(m, &m.parts[p].grid).bindings);
        }
    }
    for &g in &glue_order {
        pools.keyform_binding(&prep(m, &m.glues[g].grid).bindings);
    }

    // ---- Parts. §9 step 3: parts without draw-order group get one keyform
    // at their editor default order.
    let mut parts_kb = Vec::new();
    let mut parts_kf_begin = Vec::new();
    let mut parts_kf_count = Vec::new();
    let mut part_draw = Vec::new();
    let mut part_grids = Vec::new();
    for &p in &part_order {
        let part = &m.parts[p];
        let g = if part.draw_order_group {
            prep(m, &part.grid)
        } else {
            KeyformGrid::single(PartForm {
                draw_order: part.default_order as f32,
                ..PartForm::default()
            })
        };
        parts_kb.push(pools.keyform_binding(&g.bindings));
        parts_kf_begin.push(count(part_draw.len())?);
        parts_kf_count.push(count(g.keyforms.len())?);
        part_draw.extend(g.keyforms.iter().map(|k| k.draw_order));
        part_grids.push(g);
    }

    // ---- Deformers.
    let mut dkb = Vec::new();
    let (mut d_type, mut d_spec) = (Vec::new(), Vec::new());
    let mut w = WarpCols::default();
    let mut r = RotCols::default();
    let mut warp_color_begin = Vec::new();
    let mut rot_color_begin = Vec::new();
    let mut warp_kf_colors: Vec<(i32, i32)> = Vec::new();
    let mut rot_kf_colors: Vec<(i32, i32)> = Vec::new();
    // Colour pool order (observed, 5.3.04, v4–v6): offscreen keyforms, then
    // deformer keyforms in deformer-table order, then art meshes.
    // TODO(Q-0036): confirm the pool order rule.
    let mut off_colors: Vec<(i32, i32)> = Vec::new();
    let mut off_opacity: Vec<f32> = Vec::new();
    let mut part_off_kf: Vec<i32> = Vec::new();
    let mut part_off: Vec<i32> = Vec::new();
    let mut off_owner: Vec<i32> = Vec::new();
    let mut off_parts: Vec<PartIndex> = Vec::new();
    for (pi, &p) in part_order.iter().enumerate() {
        let off = v >= 6 && m.parts[p].offscreen;
        part_off.push(if off { count(off_owner.len())? } else { -1 });
        for k in &part_grids[pi].keyforms {
            if off {
                part_off_kf.push(count(off_opacity.len())?);
                off_opacity.push(k.opacity);
                off_colors.push((count(mul_pool.len())?, count(scr_pool.len())?));
                mul_pool.push(k.multiply);
                scr_pool.push(k.screen);
            } else {
                part_off_kf.push(-1);
            }
        }
        if off {
            off_owner.push(count(pi)?);
            off_parts.push(p);
        }
    }
    for &d in &def_order {
        let def = &m.deformers[d];
        match &def.kind {
            DeformerKind::Warp(wd) => {
                let g = prep(m, &wd.grid);
                let kb = pools.keyform_binding(&g.bindings);
                dkb.push(kb);
                d_type.push(0);
                d_spec.push(count(w.kb.len())?);
                w.kb.push(kb);
                w.begin.push(count(w.opacity.len())?);
                w.count.push(count(g.keyforms.len())?);
                w.vertex_count.push(count(wd.point_count())?);
                w.rows.push(wd.rows as i32);
                w.cols.push(wd.cols as i32);
                w.quad.push(i32::from(wd.quad));
                warp_color_begin.push(count(mul_pool.len())?);
                for k in &g.keyforms {
                    let mut pts = k.positions.clone();
                    if pts.len() != 2 * wd.point_count() {
                        return invalid(format!("{}: warp keyform size", def.info.id));
                    }
                    if def.target.is_none() {
                        to_units(&mut pts, o);
                    }
                    w.opacity.push(k.opacity);
                    w.pos_begin.push(push_positions(&mut positions, &pts));
                    warp_kf_colors.push((count(mul_pool.len())?, count(scr_pool.len())?));
                    mul_pool.push(k.multiply);
                    scr_pool.push(k.screen);
                }
            }
            DeformerKind::Rotation(rd) => {
                let g = prep(m, &rd.grid);
                let kb = pools.keyform_binding(&g.bindings);
                dkb.push(kb);
                d_type.push(1);
                d_spec.push(count(r.kb.len())?);
                r.kb.push(kb);
                r.begin.push(count(r.opacity.len())?);
                r.count.push(count(g.keyforms.len())?);
                r.base_angle.push(rd.base_angle);
                // spec: moc3.md §8.10 — the 1/ppu export wrap is removed with
                // change-parent scale compensation, which lands on the first
                // rotation deformer of every chain (warps pass total scale through).
                let mut first_rotation = true;
                let mut t = def.target;
                let mut steps = 0;
                while let Some(p) = t.filter(|&p| p < nd) {
                    if matches!(m.deformers[p].kind, DeformerKind::Rotation(_)) {
                        first_rotation = false;
                        break;
                    }
                    steps += 1;
                    if steps > nd {
                        break;
                    }
                    t = m.deformers[p].target;
                }
                rot_color_begin.push(count(mul_pool.len())?);
                for k in &g.keyforms {
                    let mut origin = k.origin;
                    if def.target.is_none() {
                        to_units(&mut origin, o);
                    }
                    r.opacity.push(k.opacity);
                    r.angle.push(k.angle);
                    r.ox.push(origin[0]);
                    r.oy.push(origin[1]);
                    r.scale.push(if first_rotation {
                        k.scale / o.pixels_per_unit
                    } else {
                        k.scale
                    });
                    r.rx.push(i32::from(k.reflect_x));
                    r.ry.push(i32::from(k.reflect_y));
                    rot_kf_colors.push((count(mul_pool.len())?, count(scr_pool.len())?));
                    mul_pool.push(k.multiply);
                    scr_pool.push(k.screen);
                }
            }
        }
    }

    // Warp blend-shape deltas follow all warp grid keyforms (§8.4).
    // TODO(Q-0036): unverified against a reference export with warp blend shapes.
    if v >= 4 {
        let mut wi = 0i32;
        for &d in &def_order {
            let def = &m.deformers[d];
            let DeformerKind::Warp(wd) = &def.kind else {
                continue;
            };
            let here = wi;
            wi += 1;
            if wd.blend_shapes.is_empty() {
                continue;
            }
            let g = prep(m, &wd.grid);
            let root = def.target.is_none();
            let (b, n) = bs.add(m, &param_pos, &g, &wd.blend_shapes, &mut |k: &WarpForm| {
                let i = count(w.opacity.len())?;
                let mut pts = k.positions.clone();
                if root {
                    let sc = 1.0 / o.pixels_per_unit;
                    pts.iter_mut().for_each(|x| *x *= sc);
                }
                w.opacity.push(k.opacity);
                w.pos_begin.push(push_positions(&mut positions, &pts));
                if v >= 5 {
                    warp_kf_colors.push((count(mul_pool.len())?, count(scr_pool.len())?));
                    mul_pool.push(k.multiply);
                    scr_pool.push(k.screen);
                }
                Ok(i)
            })?;
            bs_warps.push((here, b, n));
        }
    }

    // ---- Art meshes.
    let mut a = MeshCols::default();
    let mut uvs: Vec<f32> = Vec::new();
    let mut vidx: Vec<i16> = Vec::new();
    let mut clips: Vec<i32> = Vec::new();
    let mut tex_used: Vec<usize> = Vec::new();
    let mut mesh_draw_orders: Vec<Vec<f32>> = Vec::new();
    for &ai in &mesh_order {
        let am = &m.art_meshes[ai];
        let nv = am.vertex_count();
        if nv > i16::MAX as usize {
            return invalid(format!(
                "{}: {nv} vertices exceed the moc3 limit",
                am.info.id
            ));
        }
        let g = prep(m, &am.grid);
        let kb = pools.keyform_binding(&g.bindings);
        a.kb.push(kb);
        a.begin.push(count(a.opacity.len())?);
        a.count.push(count(g.keyforms.len())?);
        a.visible.push(i32::from(am.info.visible));
        a.parent_part.push(export_parent_part(am.info.parent_part));
        a.parent_def.push(
            am.target
                .map_or(-1, |t| def_index.get(t).copied().unwrap_or(-1)),
        );
        let t = am.texture.unwrap_or(0);
        if !tex_used.contains(&t) {
            tex_used.push(t);
        }
        a.texture.push(t);
        let mut flags = 0u8;
        let mut cc = am.color_composition;
        if v < 6
            && !matches!(
                cc,
                ColorComposition::Normal | ColorComposition::Add | ColorComposition::Multiply
            )
        {
            rep.warnings.push(format!(
                "{}: blend mode needs moc3 5.3; exported as Normal",
                am.info.id
            ));
            cc = ColorComposition::Normal;
        }
        flags |= match cc {
            ColorComposition::Add => 1,
            ColorComposition::Multiply => 2,
            _ => 0,
        };
        if !am.culling {
            flags |= 4;
        }
        let mut clip_list: Vec<i32> = Vec::new();
        for c in &am.clip {
            match mesh_index.get(c) {
                Some(&ci) => clip_list.push(ci),
                None => rep.warnings.push(format!(
                    "{}: clipping refers to an art mesh that was not exported",
                    am.info.id
                )),
            }
        }
        if am.invert_clip {
            if v >= 3 {
                flags |= 8;
            } else {
                rep.warnings.push(format!(
                    "{}: inverted mask needs moc3 4.0; mask dropped",
                    am.info.id
                ));
                clip_list.clear();
            }
        }
        a.flags.push(flags);
        a.vertex_count.push(count(nv)?);
        a.uv_begin.push(count(uvs.len())?);
        uvs.extend_from_slice(&am.uvs);
        a.index_begin.push(count(vidx.len())?);
        a.index_count.push(count(am.indices.len())?);
        vidx.extend(am.indices.iter().map(|&i| i as i16));
        a.clip_begin.push(count(clips.len())?);
        a.clip_count.push(count(clip_list.len())?);
        clips.extend(clip_list);
        a.blend_mode
            .push(i32::from(cc.moc3_code()) | (i32::from(am.alpha_composition.moc3_code()) << 8));
        a.color_begin.push(count(mul_pool.len())?);
        let mut dos = Vec::new();
        for k in &g.keyforms {
            let mut pos = k.positions.clone();
            if pos.len() != 2 * nv {
                return invalid(format!("{}: keyform size", am.info.id));
            }
            if am.target.is_none() {
                to_units(&mut pos, o);
            }
            a.opacity.push(k.opacity);
            a.draw_order.push(k.draw_order);
            dos.push(k.draw_order);
            a.pos_begin.push(push_positions(&mut positions, &pos));
            a.kf_colors
                .push((count(mul_pool.len())?, count(scr_pool.len())?));
            mul_pool.push(k.multiply);
            scr_pool.push(k.screen);
        }
        mesh_draw_orders.push(dos);
    }
    // Art mesh blend-shape deltas follow all art mesh grid keyforms (§8.4).
    let mut bs_mesh_draw: Vec<(usize, f32, f32)> = Vec::new();
    if v >= 4 {
        for (mi_out, &ai) in mesh_order.iter().enumerate() {
            let am = &m.art_meshes[ai];
            if am.blend_shapes.is_empty() {
                continue;
            }
            let g = prep(m, &am.grid);
            let root = am.target.is_none();
            let (mut lo, mut hi) = (0.0f32, 0.0f32);
            let (b, n) = bs.add(
                m,
                &param_pos,
                &g,
                &am.blend_shapes,
                &mut |k: &ArtMeshForm| {
                    let i = count(a.opacity.len())?;
                    let mut pts = k.positions.clone();
                    if root {
                        let sc = 1.0 / o.pixels_per_unit;
                        pts.iter_mut().for_each(|x| *x *= sc);
                    }
                    a.opacity.push(k.opacity);
                    a.draw_order.push(k.draw_order);
                    lo = lo.min(k.draw_order);
                    hi = hi.max(k.draw_order);
                    a.pos_begin.push(push_positions(&mut positions, &pts));
                    if v >= 5 {
                        a.kf_colors
                            .push((count(mul_pool.len())?, count(scr_pool.len())?));
                        mul_pool.push(k.multiply);
                        scr_pool.push(k.screen);
                    }
                    Ok(i)
                },
            )?;
            bs_meshes.push((count(mi_out)?, b, n));
            bs_mesh_draw.push((mi_out, lo, hi));
        }
    }
    // Texture index = position among the pages used, in texture-manager order (§8.6).
    tex_used.sort_unstable();
    let tex_pos: HashMap<usize, i32> = tex_used
        .iter()
        .enumerate()
        .map(|(i, &t)| (t, i as i32))
        .collect();
    let mesh_tex: Vec<i32> = a.texture.iter().map(|t| tex_pos[t]).collect();
    rep.textures = tex_used;

    // ---- Glue.
    let mut gl = GlueCols::default();
    let (mut pair_w, mut pair_v): (Vec<f32>, Vec<i16>) = (Vec::new(), Vec::new());
    let mut glue_int = Vec::new();
    for &gi in &glue_order {
        let g0 = &m.glues[gi];
        let g = prep(m, &g0.grid);
        gl.kb.push(pools.keyform_binding(&g.bindings));
        gl.begin.push(count(glue_int.len())?);
        gl.count.push(count(g.keyforms.len())?);
        glue_int.extend(g.keyforms.iter().map(|k| k.intensity));
        let (ma, mb) = (g0.mesh_a.unwrap_or(0), g0.mesh_b.unwrap_or(0));
        gl.a.push(mesh_index[&ma]);
        gl.b.push(mesh_index[&mb]);
        gl.pair_begin.push(count(pair_w.len())?);
        for (pair, wts) in g0.pairs.iter().zip(&g0.weights) {
            // Entries whose vertex did not resolve are skipped (§8.8).
            let (Some(va), Some(vb)) = *pair else {
                continue;
            };
            pair_v.push(va as i16);
            pair_w.push(wts[0]);
            pair_v.push(vb as i16);
            pair_w.push(wts[1]);
        }
        gl.pair_count
            .push(count(pair_w.len())? - gl.pair_begin.last().copied().unwrap_or(0));
    }

    // ---- Parameters and binding pools. Bindings sorted by parameter (stable).
    let mut pb_order: Vec<usize> = (0..pools.param_bindings.len()).collect();
    pb_order.sort_by_key(|&i| param_rank[pools.param_bindings[i].0]);
    let mut pb_final = vec![0i32; pb_order.len()];
    for (pos, &id) in pb_order.iter().enumerate() {
        pb_final[id] = pos as i32;
    }
    let mut keys: Vec<f32> = Vec::new();
    let (mut pb_kbegin, mut pb_kcount) = (Vec::new(), Vec::new());
    for &id in &pb_order {
        pb_kbegin.push(count(keys.len())?);
        let k = &pools.param_bindings[id].1;
        pb_kcount.push(count(k.len())?);
        keys.extend(k.iter().map(|b| f32::from_bits(*b)));
    }
    let mut pbi: Vec<i32> = Vec::new();
    let (mut kb_begin, mut kb_count) = (Vec::new(), Vec::new());
    for list in &pools.keyform_bindings {
        kb_begin.push(count(pbi.len())?);
        kb_count.push(count(list.len())?);
        pbi.extend(list.iter().map(|&id| pb_final[id]));
    }
    let mut p_cols = ParamCols::default();
    for &p in &param_order {
        let par = &m.parameters[p];
        if par.id.len() >= 64 {
            return invalid(format!("parameter ID {:?} is too long", par.id));
        }
        p_cols.ids.push(par.id.clone());
        p_cols.max.push(par.max);
        p_cols.min.push(par.min);
        p_cols.default.push(par.default);
        p_cols.repeat.push(i32::from(par.repeat));
        p_cols.decimals.push(par.decimal_places);
        let owned: Vec<usize> = pb_order
            .iter()
            .enumerate()
            .filter(|(_, &id)| pools.param_bindings[id].0 == p)
            .map(|(i, _)| i)
            .collect();
        match (owned.first(), owned.len()) {
            (Some(&b), n) => {
                p_cols.binding_begin.push(count(b)?);
                p_cols.binding_count.push(count(n)?);
            }
            (None, _) => {
                // Unused parameters store begin −1; a blend-shape parameter
                // with blend-shape bindings only stores 0 (observed).
                let pe = param_pos[&p];
                p_cols
                    .binding_begin
                    .push(if bs.pb.iter().any(|b| b.0 == pe) {
                        0
                    } else {
                        -1
                    });
                p_cols.binding_count.push(0);
            }
        }
        p_cols
            .kind
            .push(i32::from(par.kind == ParameterKind::BlendShape));
    }
    // Blend-shape parameter bindings: sorted by parameter (stable), their
    // keys after the normal binding keys (§8.4).
    let mut bs_order: Vec<usize> = (0..bs.pb.len()).collect();
    bs_order.sort_by_key(|&i| bs.pb[i].0);
    let mut bs_final = vec![0i32; bs.pb.len()];
    for (pos, &id) in bs_order.iter().enumerate() {
        bs_final[id] = count(pos)?;
    }
    let (mut bs_kbegin, mut bs_kcount, mut bs_base) = (Vec::new(), Vec::new(), Vec::new());
    for &id in &bs_order {
        bs_kbegin.push(count(keys.len())?);
        bs_kcount.push(count(bs.pb[id].1.len())?);
        bs_base.push(bs.pb[id].2);
        keys.extend_from_slice(&bs.pb[id].1);
    }
    // Parameter key lists follow the binding keys. The keys are written for
    // every version (observed in v3 files); the begin/count sections exist from v4.2.
    let mut key_list_begin = Vec::new();
    let mut key_list_count = Vec::new();
    {
        for &p in &param_order {
            let mut ks: Vec<f32> = pb_order
                .iter()
                .filter(|&&id| pools.param_bindings[id].0 == p)
                .flat_map(|&id| {
                    pools.param_bindings[id]
                        .1
                        .iter()
                        .map(|b| f32::from_bits(*b))
                })
                .collect();
            let pe = param_pos[&p];
            ks.extend(
                bs.pb
                    .iter()
                    .filter(|b| b.0 == pe)
                    .flat_map(|b| b.1.iter().copied()),
            );
            ks.sort_by(f32::total_cmp);
            ks.dedup();
            key_list_begin.push(count(keys.len())?);
            key_list_count.push(count(ks.len())?);
            keys.extend(ks);
        }
    }

    // ---- Draw-order groups. §8.7
    let groups = build_groups(
        m,
        root,
        &part_ok,
        &mesh_index,
        &part_index,
        &part_grids,
        &part_order,
        &mesh_order,
        &mesh_draw_orders,
        v >= 6,
    );

    // ---- Fill sections.
    let set = |raw: &mut Moc3Raw, name: &'static str, d: SectionData| raw.set(name, d);
    let n_parts = part_order.len();
    raw.counts[c::PARTS] = count(n_parts)?;
    set(&mut raw, "parts.runtime", SectionData::Runtime(n_parts));
    set(
        &mut raw,
        "parts.id",
        SectionData::Ids(
            part_order
                .iter()
                .map(|&p| m.parts[p].info.id.clone())
                .collect(),
        ),
    );
    set(&mut raw, "parts.keyform_binding", i32s(parts_kb));
    set(&mut raw, "parts.keyform_begin", i32s(parts_kf_begin));
    set(&mut raw, "parts.keyform_count", i32s(parts_kf_count));
    set(
        &mut raw,
        "parts.visible",
        i32s(
            part_order
                .iter()
                .map(|&p| i32::from(m.parts[p].info.visible))
                .collect(),
        ),
    );
    set(&mut raw, "parts.enabled", i32s(vec![1; n_parts]));
    set(
        &mut raw,
        "parts.parent_part",
        i32s(
            part_order
                .iter()
                .map(|&p| export_parent_part(m.parts[p].info.parent_part))
                .collect(),
        ),
    );
    raw.counts[c::PART_KEYFORMS] = count(part_draw.len())?;
    let n_part_kf = part_draw.len();
    set(&mut raw, "part_keyforms.draw_order", f32s(part_draw));

    raw.counts[c::DEFORMERS] = count(nd)?;
    set(&mut raw, "deformers.runtime", SectionData::Runtime(nd));
    set(
        &mut raw,
        "deformers.id",
        SectionData::Ids(
            def_order
                .iter()
                .map(|&d| m.deformers[d].info.id.clone())
                .collect(),
        ),
    );
    set(&mut raw, "deformers.keyform_binding", i32s(dkb));
    set(
        &mut raw,
        "deformers.visible",
        i32s(
            def_order
                .iter()
                .map(|&d| i32::from(m.deformers[d].info.visible))
                .collect(),
        ),
    );
    set(&mut raw, "deformers.enabled", i32s(vec![1; nd]));
    set(
        &mut raw,
        "deformers.parent_part",
        i32s(
            def_order
                .iter()
                .map(|&d| export_parent_part(m.deformers[d].info.parent_part))
                .collect(),
        ),
    );
    set(
        &mut raw,
        "deformers.parent_deformer",
        i32s(
            def_order
                .iter()
                .map(|&d| m.deformers[d].target.map_or(-1, |t| def_index[t]))
                .collect(),
        ),
    );
    set(&mut raw, "deformers.type", i32s(d_type));
    set(&mut raw, "deformers.specific_index", i32s(d_spec));

    raw.counts[c::WARPS] = count(w.kb.len())?;
    raw.counts[c::WARP_KEYFORMS] = count(w.opacity.len())?;
    let n_warp = w.kb.len();
    set(&mut raw, "warps.keyform_binding", i32s(w.kb));
    set(&mut raw, "warps.keyform_begin", i32s(w.begin));
    set(&mut raw, "warps.keyform_count", i32s(w.count));
    set(&mut raw, "warps.vertex_count", i32s(w.vertex_count));
    set(&mut raw, "warps.rows", i32s(w.rows));
    set(&mut raw, "warps.cols", i32s(w.cols));
    set(&mut raw, "warp_keyforms.opacity", f32s(w.opacity));
    set(&mut raw, "warp_keyforms.position_begin", i32s(w.pos_begin));
    if v >= 2 {
        set(&mut raw, "warps.quad", i32s(w.quad));
    } else if m
        .deformers
        .iter()
        .any(|d| matches!(&d.kind, DeformerKind::Warp(w) if w.quad))
    {
        rep.warnings
            .push("quad-transform warps need moc3 3.3; exported as triangle warps".into());
    }

    raw.counts[c::ROTATIONS] = count(r.kb.len())?;
    raw.counts[c::ROTATION_KEYFORMS] = count(r.opacity.len())?;
    let n_rot = r.kb.len();
    set(&mut raw, "rotations.keyform_binding", i32s(r.kb));
    set(&mut raw, "rotations.keyform_begin", i32s(r.begin));
    set(&mut raw, "rotations.keyform_count", i32s(r.count));
    set(&mut raw, "rotations.base_angle", f32s(r.base_angle));
    set(&mut raw, "rotation_keyforms.opacity", f32s(r.opacity));
    set(&mut raw, "rotation_keyforms.angle", f32s(r.angle));
    set(&mut raw, "rotation_keyforms.origin_x", f32s(r.ox));
    set(&mut raw, "rotation_keyforms.origin_y", f32s(r.oy));
    set(&mut raw, "rotation_keyforms.scale", f32s(r.scale));
    set(&mut raw, "rotation_keyforms.reflect_x", i32s(r.rx));
    set(&mut raw, "rotation_keyforms.reflect_y", i32s(r.ry));

    let n_mesh = mesh_order.len();
    raw.counts[c::ART_MESHES] = count(n_mesh)?;
    raw.counts[c::ART_MESH_KEYFORMS] = count(a.opacity.len())?;
    for n in [
        "art_meshes.runtime0",
        "art_meshes.runtime1",
        "art_meshes.runtime2",
        "art_meshes.runtime3",
    ] {
        set(&mut raw, n, SectionData::Runtime(n_mesh));
    }
    set(
        &mut raw,
        "art_meshes.id",
        SectionData::Ids(
            mesh_order
                .iter()
                .map(|&i| m.art_meshes[i].info.id.clone())
                .collect(),
        ),
    );
    set(&mut raw, "art_meshes.keyform_binding", i32s(a.kb));
    set(&mut raw, "art_meshes.keyform_begin", i32s(a.begin));
    set(&mut raw, "art_meshes.keyform_count", i32s(a.count));
    set(&mut raw, "art_meshes.visible", i32s(a.visible));
    set(&mut raw, "art_meshes.enabled", i32s(vec![1; n_mesh]));
    set(&mut raw, "art_meshes.parent_part", i32s(a.parent_part));
    set(&mut raw, "art_meshes.parent_deformer", i32s(a.parent_def));
    set(&mut raw, "art_meshes.texture", i32s(mesh_tex));
    set(&mut raw, "art_meshes.flags", SectionData::U8(a.flags));
    set(&mut raw, "art_meshes.vertex_count", i32s(a.vertex_count));
    set(&mut raw, "art_meshes.uv_begin", i32s(a.uv_begin));
    set(&mut raw, "art_meshes.index_begin", i32s(a.index_begin));
    set(&mut raw, "art_meshes.index_count", i32s(a.index_count));
    set(&mut raw, "art_meshes.clip_begin", i32s(a.clip_begin));
    set(&mut raw, "art_meshes.clip_count", i32s(a.clip_count));
    set(&mut raw, "art_mesh_keyforms.opacity", f32s(a.opacity));
    set(&mut raw, "art_mesh_keyforms.draw_order", f32s(a.draw_order));
    set(
        &mut raw,
        "art_mesh_keyforms.position_begin",
        i32s(a.pos_begin),
    );

    let n_params = param_order.len();
    raw.counts[c::PARAMETERS] = count(n_params)?;
    set(
        &mut raw,
        "parameters.runtime",
        SectionData::Runtime(n_params),
    );
    set(&mut raw, "parameters.id", SectionData::Ids(p_cols.ids));
    set(&mut raw, "parameters.max", f32s(p_cols.max));
    set(&mut raw, "parameters.min", f32s(p_cols.min));
    set(&mut raw, "parameters.default", f32s(p_cols.default));
    set(&mut raw, "parameters.repeat", i32s(p_cols.repeat));
    set(&mut raw, "parameters.decimal_places", i32s(p_cols.decimals));
    set(
        &mut raw,
        "parameters.binding_begin",
        i32s(p_cols.binding_begin),
    );
    set(
        &mut raw,
        "parameters.binding_count",
        i32s(p_cols.binding_count),
    );

    raw.counts[c::KEYFORM_POSITIONS] = count(positions.len())?;
    set(&mut raw, "keyform_positions.xy", f32s(positions));
    raw.counts[c::PARAM_BINDING_INDICES] = count(pbi.len())?;
    set(&mut raw, "param_binding_indices", i32s(pbi));
    raw.counts[c::KEYFORM_BINDINGS] = count(kb_begin.len())?;
    set(&mut raw, "keyform_bindings.begin", i32s(kb_begin));
    set(&mut raw, "keyform_bindings.count", i32s(kb_count));
    raw.counts[c::PARAM_BINDINGS] = count(pb_kbegin.len())?;
    set(&mut raw, "param_bindings.keys_begin", i32s(pb_kbegin));
    set(&mut raw, "param_bindings.keys_count", i32s(pb_kcount));
    raw.counts[c::KEYS] = count(keys.len())?;
    set(&mut raw, "keys", f32s(keys));
    raw.counts[c::UVS] = count(uvs.len())?;
    set(&mut raw, "uvs", f32s(uvs));
    raw.counts[c::VERTEX_INDICES] = count(vidx.len())?;
    set(&mut raw, "vertex_indices", SectionData::I16(vidx));
    raw.counts[c::CLIP_MASKS] = count(clips.len())?;
    let raw_clips = clips.clone();
    set(&mut raw, "clip_masks", i32s(clips));

    raw.counts[c::DRAW_ORDER_GROUPS] = count(groups.begin.len())?;
    raw.counts[c::DRAW_ORDER_GROUP_ITEMS] = count(groups.ty.len())?;
    set(&mut raw, "draw_order_groups.item_begin", i32s(groups.begin));
    set(&mut raw, "draw_order_groups.item_count", i32s(groups.count));
    set(
        &mut raw,
        "draw_order_groups.total_drawn",
        i32s(groups.total),
    );
    set(
        &mut raw,
        "draw_order_groups.max_draw_order",
        i32s(groups.max),
    );
    set(
        &mut raw,
        "draw_order_groups.min_draw_order",
        i32s(groups.min),
    );
    set(&mut raw, "draw_order_group_items.type", i32s(groups.ty));
    set(&mut raw, "draw_order_group_items.index", i32s(groups.index));
    set(
        &mut raw,
        "draw_order_group_items.own_group",
        i32s(groups.own),
    );

    let n_glue = gl.kb.len();
    raw.counts[c::GLUES] = count(n_glue)?;
    raw.counts[c::GLUE_PAIRS] = count(pair_w.len())?;
    raw.counts[c::GLUE_KEYFORMS] = count(glue_int.len())?;
    set(&mut raw, "glues.runtime", SectionData::Runtime(n_glue));
    set(
        &mut raw,
        "glues.id",
        SectionData::Ids(
            glue_order
                .iter()
                .map(|&g| m.glues[g].info.id.clone())
                .collect(),
        ),
    );
    set(&mut raw, "glues.keyform_binding", i32s(gl.kb));
    set(&mut raw, "glues.keyform_begin", i32s(gl.begin));
    set(&mut raw, "glues.keyform_count", i32s(gl.count));
    set(&mut raw, "glues.mesh_a", i32s(gl.a));
    set(&mut raw, "glues.mesh_b", i32s(gl.b));
    set(&mut raw, "glues.pair_begin", i32s(gl.pair_begin));
    set(&mut raw, "glues.pair_count", i32s(gl.pair_count));
    set(&mut raw, "glue_pairs.weight", f32s(pair_w));
    set(&mut raw, "glue_pairs.vertex", SectionData::I16(pair_v));
    set(&mut raw, "glue_keyforms.intensity", f32s(glue_int));

    if v >= 4 {
        set(
            &mut raw,
            "param_key_lists.runtime",
            SectionData::Runtime(n_params),
        );
        set(&mut raw, "param_key_lists.keys_begin", i32s(key_list_begin));
        set(&mut raw, "param_key_lists.keys_count", i32s(key_list_count));
        set(&mut raw, "warps.color_begin", i32s(warp_color_begin));
        set(&mut raw, "rotations.color_begin", i32s(rot_color_begin));
        set(&mut raw, "art_meshes.color_begin", i32s(a.color_begin));
        raw.counts[c::MULTIPLY_COLORS] = count(mul_pool.len())?;
        raw.counts[c::SCREEN_COLORS] = count(scr_pool.len())?;
        for (pre, pool) in [("multiply_colors", &mul_pool), ("screen_colors", &scr_pool)] {
            let names: [&'static str; 3] = if pre == "multiply_colors" {
                [
                    "multiply_colors.r",
                    "multiply_colors.g",
                    "multiply_colors.b",
                ]
            } else {
                ["screen_colors.r", "screen_colors.g", "screen_colors.b"]
            };
            for (ch, name) in names.into_iter().enumerate() {
                set(&mut raw, name, f32s(pool.iter().map(|c| c[ch]).collect()));
            }
        }
        set(&mut raw, "parameters.type", i32s(p_cols.kind));
        // Each blend-shape parameter owns a range of the sorted bindings.
        let (mut pbb, mut pbc) = (vec![0i32; n_params], vec![0i32; n_params]);
        for (pos, &id) in bs_order.iter().enumerate() {
            let pe = bs.pb[id].0;
            if pbc[pe] == 0 {
                pbb[pe] = count(pos)?;
            }
            pbc[pe] += 1;
        }
        set(&mut raw, "parameters.bs_binding_begin", i32s(pbb));
        set(&mut raw, "parameters.bs_binding_count", i32s(pbc));
        raw.counts[c::BS_PARAM_BINDINGS] = count(bs_order.len())?;
        set(&mut raw, "bs_param_bindings.keys_begin", i32s(bs_kbegin));
        set(&mut raw, "bs_param_bindings.keys_count", i32s(bs_kcount));
        set(&mut raw, "bs_param_bindings.base_key", i32s(bs_base));
        raw.counts[c::BS_KEYFORM_BINDINGS] = count(bs.kb.len())?;
        let col = |k: usize| i32s(bs.kb.iter().map(|x| x[k]).collect());
        set(
            &mut raw,
            "bs_keyform_bindings.param_binding",
            i32s(bs.kb.iter().map(|x| bs_final[x[0] as usize]).collect()),
        );
        set(&mut raw, "bs_keyform_bindings.keyform_begin", col(1));
        set(&mut raw, "bs_keyform_bindings.keyform_count", col(2));
        set(&mut raw, "bs_keyform_bindings.constraint_begin", col(3));
        set(&mut raw, "bs_keyform_bindings.constraint_count", col(4));
        let targets = |raw: &mut Moc3Raw,
                       idx: usize,
                       pre: [&'static str; 3],
                       t: &[(i32, i32, i32)]|
         -> Result<()> {
            raw.counts[idx] = count(t.len())?;
            raw.set(pre[0], i32s(t.iter().map(|x| x.0).collect()));
            raw.set(pre[1], i32s(t.iter().map(|x| x.1).collect()));
            raw.set(pre[2], i32s(t.iter().map(|x| x.2).collect()));
            Ok(())
        };
        targets(
            &mut raw,
            c::BS_WARPS,
            [
                "bs_warps.target",
                "bs_warps.binding_begin",
                "bs_warps.binding_count",
            ],
            &bs_warps,
        )?;
        targets(
            &mut raw,
            c::BS_ART_MESHES,
            [
                "bs_art_meshes.target",
                "bs_art_meshes.binding_begin",
                "bs_art_meshes.binding_count",
            ],
            &bs_meshes,
        )?;
        raw.counts[c::BS_CONSTRAINT_INDICES] = count(bs.constraint_indices.len())?;
        set(
            &mut raw,
            "bs_constraint_indices",
            i32s(bs.constraint_indices.clone()),
        );
        raw.counts[c::BS_CONSTRAINTS] = count(bs.constraints.len())?;
        let (mut cp, mut cb, mut cn, mut ck, mut cw) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (q, pts) in &bs.constraints {
            cp.push(count(*q)?);
            cb.push(count(ck.len())?);
            cn.push(count(pts.len())?);
            for (k, wt) in pts {
                ck.push(*k);
                cw.push(*wt);
            }
        }
        raw.counts[c::BS_CONSTRAINT_VALUES] = count(ck.len())?;
        set(&mut raw, "bs_constraints.parameter", i32s(cp));
        set(&mut raw, "bs_constraints.value_begin", i32s(cb));
        set(&mut raw, "bs_constraints.value_count", i32s(cn));
        set(&mut raw, "bs_constraint_values.key", f32s(ck));
        set(&mut raw, "bs_constraint_values.weight", f32s(cw));
    }
    if v >= 5 {
        let split = |v: &[(i32, i32)]| {
            (
                v.iter().map(|x| x.0).collect(),
                v.iter().map(|x| x.1).collect(),
            )
        };
        let (wm, ws) = split(&warp_kf_colors);
        let (rm, rs) = split(&rot_kf_colors);
        let (am_, as_) = split(&a.kf_colors);
        set(&mut raw, "warp_keyforms.multiply_color", i32s(wm));
        set(&mut raw, "warp_keyforms.screen_color", i32s(ws));
        set(&mut raw, "rotation_keyforms.multiply_color", i32s(rm));
        set(&mut raw, "rotation_keyforms.screen_color", i32s(rs));
        set(&mut raw, "art_mesh_keyforms.multiply_color", i32s(am_));
        set(&mut raw, "art_mesh_keyforms.screen_color", i32s(as_));
    }
    if v >= 6 {
        // Offscreen renderings (moc3.md §8.9): one per offscreen part.
        let mut flags = Vec::new();
        let mut blend = Vec::new();
        let (mut cb, mut cn) = (Vec::new(), Vec::new());
        let mut clips2: Vec<i32> = raw_clips.clone();
        for &p in &off_parts {
            let part = &m.parts[p];
            // Parts are never culled: double-sided (4); inverted mask (8).
            let mut f = 4u8;
            if part.invert_clip {
                f |= 8;
            }
            flags.push(f);
            blend.push(
                i32::from(part.color_composition.moc3_code())
                    | (i32::from(part.alpha_composition.moc3_code()) << 8),
            );
            // TODO(Q-0036): where part clip lists go in the clip pool.
            let list: Vec<i32> = part
                .clip
                .iter()
                .filter_map(|c| mesh_index.get(c).copied())
                .collect();
            // An empty list starts at 0 (observed).
            cb.push(if list.is_empty() {
                0
            } else {
                count(clips2.len())?
            });
            cn.push(count(list.len())?);
            clips2.extend(list);
        }
        if clips2.len() != raw_clips.len() {
            raw.counts[c::CLIP_MASKS] = count(clips2.len())?;
            set(&mut raw, "clip_masks", i32s(clips2));
        }
        let n_off = off_owner.len();
        raw.counts[c::OFFSCREENS] = count(n_off)?;
        raw.counts[c::OFFSCREEN_KEYFORMS] = count(off_opacity.len())?;
        set(&mut raw, "parts.offscreen", i32s(part_off));
        set(&mut raw, "art_meshes.blend_mode", i32s(a.blend_mode));
        set(&mut raw, "offscreens.runtime", SectionData::Runtime(n_off));
        set(&mut raw, "offscreens.owner", i32s(off_owner));
        set(&mut raw, "offscreens.flags", SectionData::U8(flags));
        set(&mut raw, "offscreens.blend_mode", i32s(blend));
        set(&mut raw, "offscreens.clip_begin", i32s(cb));
        set(&mut raw, "offscreens.clip_count", i32s(cn));
        set(
            &mut raw,
            "part_keyforms.offscreen_keyform",
            i32s(part_off_kf),
        );
        set(&mut raw, "offscreen_keyforms.opacity", f32s(off_opacity));
        set(
            &mut raw,
            "offscreen_keyforms.multiply_color",
            i32s(off_colors.iter().map(|c| c.0).collect()),
        );
        set(
            &mut raw,
            "offscreen_keyforms.screen_color",
            i32s(off_colors.iter().map(|c| c.1).collect()),
        );
        let _ = n_part_kf;
    }
    let _ = (n_warp, n_rot);

    rep.parts = part_order;
    rep.deformers = def_order;
    rep.art_meshes = mesh_order;
    rep.parameters = param_order;
    rep.glues = glue_order;
    Ok((raw, rep))
}

#[derive(Default)]
struct WarpCols {
    kb: Vec<i32>,
    begin: Vec<i32>,
    count: Vec<i32>,
    vertex_count: Vec<i32>,
    rows: Vec<i32>,
    cols: Vec<i32>,
    quad: Vec<i32>,
    opacity: Vec<f32>,
    pos_begin: Vec<i32>,
}

#[derive(Default)]
struct RotCols {
    kb: Vec<i32>,
    begin: Vec<i32>,
    count: Vec<i32>,
    base_angle: Vec<f32>,
    opacity: Vec<f32>,
    angle: Vec<f32>,
    ox: Vec<f32>,
    oy: Vec<f32>,
    scale: Vec<f32>,
    rx: Vec<i32>,
    ry: Vec<i32>,
}

#[derive(Default)]
struct MeshCols {
    kb: Vec<i32>,
    begin: Vec<i32>,
    count: Vec<i32>,
    visible: Vec<i32>,
    parent_part: Vec<i32>,
    parent_def: Vec<i32>,
    texture: Vec<usize>,
    flags: Vec<u8>,
    vertex_count: Vec<i32>,
    uv_begin: Vec<i32>,
    index_begin: Vec<i32>,
    index_count: Vec<i32>,
    clip_begin: Vec<i32>,
    clip_count: Vec<i32>,
    blend_mode: Vec<i32>,
    color_begin: Vec<i32>,
    opacity: Vec<f32>,
    draw_order: Vec<f32>,
    pos_begin: Vec<i32>,
    kf_colors: Vec<(i32, i32)>,
}

#[derive(Default)]
struct GlueCols {
    kb: Vec<i32>,
    begin: Vec<i32>,
    count: Vec<i32>,
    a: Vec<i32>,
    b: Vec<i32>,
    pair_begin: Vec<i32>,
    pair_count: Vec<i32>,
}

#[derive(Default)]
struct ParamCols {
    ids: Vec<String>,
    max: Vec<f32>,
    min: Vec<f32>,
    default: Vec<f32>,
    repeat: Vec<i32>,
    decimals: Vec<i32>,
    binding_begin: Vec<i32>,
    binding_count: Vec<i32>,
    kind: Vec<i32>,
}

#[derive(Default)]
struct Groups {
    begin: Vec<i32>,
    count: Vec<i32>,
    total: Vec<i32>,
    max: Vec<i32>,
    min: Vec<i32>,
    ty: Vec<i32>,
    index: Vec<i32>,
    own: Vec<i32>,
}

/// Draw-order groups: root group 0, then breadth-wise per parent; items are
/// direct members in reverse palette order. spec §8.7
#[allow(clippy::too_many_arguments)]
fn build_groups(
    m: &Model,
    root: Option<PartIndex>,
    part_ok: &[bool],
    mesh_index: &HashMap<MeshIndex, i32>,
    part_index: &HashMap<PartIndex, i32>,
    part_grids: &[KeyformGrid<PartForm>],
    part_order: &[PartIndex],
    _mesh_order: &[MeshIndex],
    mesh_draw_orders: &[Vec<f32>],
    offscreen: bool,
) -> Groups {
    enum Item {
        Mesh(i32),
        Group(PartIndex),
    }
    fn collect(
        m: &Model,
        p: PartIndex,
        part_ok: &[bool],
        mesh_index: &HashMap<MeshIndex, i32>,
        out: &mut Vec<Item>,
        depth: usize,
    ) {
        if depth > m.parts.len() {
            return;
        }
        for c in &m.parts[p].children {
            match *c {
                PartChild::ArtMesh(a) => {
                    if let Some(&i) = mesh_index.get(&a) {
                        out.push(Item::Mesh(i));
                    }
                }
                PartChild::Part(q) if q < m.parts.len() && part_ok[q] => {
                    if m.parts[q].draw_order_group {
                        out.push(Item::Group(q));
                    } else {
                        collect(m, q, part_ok, mesh_index, out, depth + 1);
                    }
                }
                _ => {}
            }
        }
    }
    let mut g = Groups::default();
    let Some(root) = root else { return g };
    // Group list: owner parts, root first, children appended breadth-wise.
    let mut owners = vec![root];
    let mut items_of: Vec<Vec<Item>> = Vec::new();
    let mut i = 0;
    while i < owners.len() {
        let mut items = Vec::new();
        collect(m, owners[i], part_ok, mesh_index, &mut items, 0);
        items.reverse();
        for it in &items {
            if let Item::Group(q) = it {
                owners.push(*q);
            }
        }
        items_of.push(items);
        i += 1;
    }
    let group_of: HashMap<PartIndex, usize> =
        owners.iter().enumerate().map(|(i, &p)| (p, i)).collect();
    let part_grid = |p: PartIndex| {
        part_order
            .iter()
            .position(|&q| q == p)
            .map(|i| &part_grids[i])
    };

    let mut totals = vec![0i32; owners.len()];
    for gi in (0..owners.len()).rev() {
        totals[gi] = items_of[gi]
            .iter()
            .map(|it| match it {
                Item::Mesh(_) => 1,
                // An offscreen part counts once more (moc3.md §8.7).
                Item::Group(q) => {
                    totals[group_of[q]] + i32::from(offscreen && m.parts[*q].offscreen)
                }
            })
            .sum();
    }
    for (gi, items) in items_of.iter().enumerate() {
        g.begin.push(g.ty.len() as i32);
        g.count.push(items.len() as i32);
        g.total.push(totals[gi]);
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for it in items {
            let dos: Vec<f32> = match it {
                Item::Mesh(mi) => mesh_draw_orders
                    .get(*mi as usize)
                    .cloned()
                    .unwrap_or_default(),
                Item::Group(q) => part_grid(*q)
                    .map(|g| g.keyforms.iter().map(|k| k.draw_order).collect())
                    .unwrap_or_default(),
            };
            for d in dos {
                lo = lo.min(d as i32);
                hi = hi.max(d as i32);
            }
            match it {
                Item::Mesh(mi) => {
                    g.ty.push(0);
                    g.index.push(*mi);
                    g.own.push(-1);
                }
                Item::Group(q) => {
                    g.ty.push(1);
                    g.index.push(part_index.get(q).copied().unwrap_or(-1));
                    g.own.push(group_of[q] as i32);
                }
            }
        }
        if lo > hi {
            (lo, hi) = (500, 500);
        }
        g.min.push(lo.clamp(0, 1000));
        g.max.push(hi.clamp(0, 1000));
    }
    g
}
