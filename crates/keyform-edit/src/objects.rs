//! Creating and deleting model objects.
//!
//! Parameters: spec 50-ui/palettes.md §3 ("Create new parameter",
//! "Delete parameter": related physics settings are deleted too),
//! 35-editing/keys.md §2 (unbinding keeps the shape at a value).
//! Deformers: 35-editing/change-parent.md, 50-ui/dialogs.md §6–7.

use std::sync::Arc;

use keyform_eval::deform::inherited_rotation;
use keyform_eval::{DeformerMap, Evaluator, Frame};
use keyform_model::{
    new_guid, Binding, Deformer, DeformerIndex, DeformerKind, KeyformGrid, Model, ObjectInfo,
    ParamEntry, ParamIndex, Parameter, Part, PartChild, PartIndex, RotationDeformer, RotationForm,
    WarpDeformer, WarpForm, DEFAULT_BEZIER_NETS,
};

use crate::keys::{delete_all_keys, ObjectRef};

/// Adds `p` (given a new GUID if it has none) at the end of `group`'s
/// children, or of the top level. Returns its index.
pub fn add_parameter(m: &mut Model, mut p: Parameter, group: Option<usize>) -> ParamIndex {
    if p.guid.is_empty() {
        p.guid = new_guid();
    }
    let i = m.parameters.len();
    p.group = group.filter(|&g| g < m.parameter_groups.len());
    match p.group {
        Some(g) => Arc::make_mut(&mut m.parameter_groups[g])
            .children
            .push(ParamEntry::Parameter(i)),
        None => m.root_parameter_entries.push(ParamEntry::Parameter(i)),
    }
    m.parameters.push(Arc::new(p));
    i
}

/// Every object of the model, for walking grids.
fn all_objects(m: &Model) -> Vec<ObjectRef> {
    (0..m.parts.len())
        .map(ObjectRef::Part)
        .chain((0..m.deformers.len()).map(ObjectRef::Deformer))
        .chain((0..m.art_meshes.len()).map(ObjectRef::ArtMesh))
        .chain((0..m.glues.len()).map(ObjectRef::Glue))
        .collect()
}

/// Renumbers parameter references with `f` (old index → new index).
fn remap_parameters(m: &mut Model, f: &dyn Fn(ParamIndex) -> ParamIndex) {
    macro_rules! grid {
        ($g:expr, $b:expr) => {{
            for b in &mut $g.bindings {
                b.parameter = f(b.parameter);
            }
            for s in &mut $b.shapes {
                s.parameter = f(s.parameter);
            }
            for c in &mut $b.constraints {
                c.blend_parameter = f(c.blend_parameter);
                c.constraint_parameter = f(c.constraint_parameter);
            }
        }};
    }
    for p in &mut m.parts {
        let p = Arc::make_mut(p);
        grid!(p.grid, p.blend_shapes);
    }
    for a in &mut m.art_meshes {
        let a = Arc::make_mut(a);
        grid!(a.grid, a.blend_shapes);
    }
    for g in &mut m.glues {
        let g = Arc::make_mut(g);
        grid!(g.grid, g.blend_shapes);
    }
    for d in &mut m.deformers {
        match &mut Arc::make_mut(d).kind {
            DeformerKind::Warp(w) => grid!(w.grid, w.blend_shapes),
            DeformerKind::Rotation(r) => grid!(r.grid, r.blend_shapes),
        }
    }
    let entry = |e: &mut ParamEntry| {
        if let ParamEntry::Parameter(p) = e {
            *p = f(*p);
        }
    };
    m.root_parameter_entries.iter_mut().for_each(entry);
    for g in &mut m.parameter_groups {
        Arc::make_mut(g).children.iter_mut().for_each(entry);
    }
    for p in m
        .eye_blink_parameters
        .iter_mut()
        .chain(&mut m.lip_sync_parameters)
    {
        *p = f(*p);
    }
    for s in &mut m.physics.settings {
        for i in &mut s.inputs {
            i.parameter = f(i.parameter);
        }
        for o in &mut s.outputs {
            o.parameter = f(o.parameter);
        }
    }
}

/// Deletes parameter `p`: objects keep their shape at its default value
/// and lose the binding (and blend shapes on it), physics settings using
/// it are deleted, it leaves its group, and later parameters move down.
pub fn remove_parameter(m: &mut Model, p: ParamIndex) -> bool {
    let Some(param) = m.parameters.get(p).cloned() else {
        return false;
    };
    for o in all_objects(m) {
        let _ = delete_all_keys(m, o, p, param.default);
    }
    macro_rules! drop_shapes {
        ($b:expr) => {{
            $b.shapes.retain(|s| s.parameter != p);
            $b.constraints
                .retain(|c| c.blend_parameter != p && c.constraint_parameter != p);
        }};
    }
    for x in &mut m.parts {
        drop_shapes!(Arc::make_mut(x).blend_shapes);
    }
    for x in &mut m.art_meshes {
        drop_shapes!(Arc::make_mut(x).blend_shapes);
    }
    for x in &mut m.glues {
        drop_shapes!(Arc::make_mut(x).blend_shapes);
    }
    for d in &mut m.deformers {
        match &mut Arc::make_mut(d).kind {
            DeformerKind::Warp(w) => drop_shapes!(w.blend_shapes),
            DeformerKind::Rotation(r) => drop_shapes!(r.blend_shapes),
        }
    }
    m.physics.settings.retain(|s| {
        !s.inputs.iter().any(|i| i.parameter == p) && !s.outputs.iter().any(|o| o.parameter == p)
    });
    m.eye_blink_parameters.retain(|&x| x != p);
    m.lip_sync_parameters.retain(|&x| x != p);
    let not_p = |e: &ParamEntry| *e != ParamEntry::Parameter(p);
    m.root_parameter_entries.retain(not_p);
    for g in &mut m.parameter_groups {
        if g.children.contains(&ParamEntry::Parameter(p)) {
            Arc::make_mut(g).children.retain(not_p);
        }
    }
    m.parameters.remove(p);
    remap_parameters(m, &|i| if i > p { i - 1 } else { i });
    true
}

// ---------------------------------------------------------------- parents

type V2 = [f32; 2];

/// Parameter values with the keys of `cell` substituted on `bindings`.
fn values_at(bindings: &[Binding], cell: usize, base: &[f32]) -> Vec<f32> {
    let mut v = base.to_vec();
    let mut rest = cell;
    for b in bindings {
        let n = b.keys.len().max(1);
        if let (Some(slot), Some(&k)) = (v.get_mut(b.parameter), b.keys.get(rest % n)) {
            *slot = k;
        }
        rest /= n;
    }
    v
}

fn map_of(frame: &Frame, d: Option<DeformerIndex>) -> Result<Option<DeformerMap>, String> {
    match d {
        None => Ok(None),
        Some(d) => frame
            .deformers
            .get(d)
            .and_then(|s| s.map.clone())
            .map(Some)
            .ok_or_else(|| "the parent deformer could not be evaluated".to_owned()),
    }
}

/// local (old parent) → canvas → local (new parent).
fn convert(p: V2, old: Option<&DeformerMap>, new: Option<&DeformerMap>) -> Result<V2, String> {
    let c = old.map_or(p, |m| m.map(p));
    match new {
        None => Ok(c),
        Some(m) => m
            .inverse(c)
            .ok_or_else(|| "a point cannot be expressed in the new parent deformer".to_owned()),
    }
}

fn convert_points(
    pts: &mut [f32],
    old: Option<&DeformerMap>,
    new: Option<&DeformerMap>,
) -> Result<(), String> {
    for q in pts.chunks_exact_mut(2) {
        let r = convert([q[0], q[1]], old, new)?;
        q[0] = r[0];
        q[1] = r[1];
    }
    Ok(())
}

fn convert_rotation(
    f: &mut RotationForm,
    old: Option<&DeformerMap>,
    new: Option<&DeformerMap>,
) -> Result<(), String> {
    let inh_old = old.map_or(0.0, |m| inherited_rotation(m, f.origin));
    let origin = convert(f.origin, old, new)?;
    let inh_new = new.map_or(0.0, |m| inherited_rotation(m, origin));
    f.origin = origin;
    f.angle += inh_old - inh_new;
    Ok(())
}

fn target_of(m: &Model, o: ObjectRef) -> Option<DeformerIndex> {
    match o {
        ObjectRef::ArtMesh(i) => m.art_meshes.get(i).and_then(|a| a.target),
        ObjectRef::Deformer(i) => m.deformers.get(i).and_then(|d| d.target),
        _ => None,
    }
}

/// Moves `o` (art mesh or deformer) under `new_parent`, keeping its exact
/// on-canvas shape in every keyform. spec: 35-editing/change-parent.md
pub fn change_parent(
    m: &mut Model,
    o: ObjectRef,
    new_parent: Option<DeformerIndex>,
    values: &[f32],
) -> Result<bool, String> {
    let old_parent = target_of(m, o);
    if old_parent == new_parent {
        return Ok(false);
    }
    // §1: not under itself or a descendant.
    if let ObjectRef::Deformer(me) = o {
        let mut cur = new_parent;
        let mut guard = 0;
        while let Some(d) = cur {
            if d == me {
                return Err("An object cannot be moved under itself.".into());
            }
            cur = m.deformers.get(d).and_then(|x| x.target);
            guard += 1;
            if guard > m.deformers.len() {
                break;
            }
        }
    }
    let ev = Evaluator::new(m);
    let pair = |vals: &[f32]| -> Result<(Option<DeformerMap>, Option<DeformerMap>), String> {
        let f = ev.evaluate(vals);
        Ok((map_of(&f, old_parent)?, map_of(&f, new_parent)?))
    };
    // §2: every grid keyform and blend-shape keyform.
    macro_rules! convert_grid {
        ($grid:expr, $blend:expr, $conv:expr) => {{
            let mut forms = $grid.keyforms.clone();
            for (cell, f) in forms.iter_mut().enumerate() {
                let (a, b) = pair(&values_at(&$grid.bindings, cell, values))?;
                $conv(f, a.as_ref(), b.as_ref())?;
            }
            let mut shapes = $blend.shapes.clone();
            for s in &mut shapes {
                let mut v = values.to_vec();
                if let (Some(slot), Some(p)) =
                    (v.get_mut(s.parameter), m.parameters.get(s.parameter))
                {
                    *slot = s.key.unwrap_or(p.max);
                }
                let (a, b) = pair(&v)?;
                $conv(&mut s.form, a.as_ref(), b.as_ref())?;
            }
            (forms, shapes)
        }};
    }
    let frame = ev.evaluate(values);
    let total = |d: Option<DeformerIndex>| {
        d.and_then(|d| frame.deformers.get(d))
            .map_or(1.0, |s| s.total_scale)
    };
    let r = total(old_parent) / total(new_parent).max(1e-12);
    match o {
        ObjectRef::ArtMesh(i) => {
            let a = m.art_meshes.get(i).ok_or("no such object")?.clone();
            let (forms, shapes) = convert_grid!(
                a.grid,
                a.blend_shapes,
                |f: &mut keyform_model::ArtMeshForm, x, y| convert_points(&mut f.positions, x, y)
            );
            let a = Arc::make_mut(&mut m.art_meshes[i]);
            a.grid.keyforms = forms;
            a.blend_shapes.shapes = shapes;
            a.target = new_parent;
        }
        ObjectRef::Deformer(i) => {
            let d = m.deformers.get(i).ok_or("no such object")?.clone();
            match &d.kind {
                DeformerKind::Warp(w) => {
                    let (forms, shapes) =
                        convert_grid!(w.grid, w.blend_shapes, |f: &mut WarpForm, x, y| {
                            convert_points(&mut f.positions, x, y)
                        });
                    let dm = Arc::make_mut(&mut m.deformers[i]);
                    if let DeformerKind::Warp(w) = &mut dm.kind {
                        w.grid.keyforms = forms;
                        w.blend_shapes.shapes = shapes;
                    }
                    dm.target = new_parent;
                    // §3: first-descendant rotation deformers keep their size.
                    if (r - 1.0).abs() > 1e-6 {
                        for k in first_rotation_descendants(m, i) {
                            if let DeformerKind::Rotation(rd) =
                                &mut Arc::make_mut(&mut m.deformers[k]).kind
                            {
                                rd.grid.keyforms.iter_mut().for_each(|f| f.scale *= r);
                                rd.blend_shapes
                                    .shapes
                                    .iter_mut()
                                    .for_each(|s| s.form.scale *= r);
                            }
                        }
                    }
                }
                DeformerKind::Rotation(rd) => {
                    let (mut forms, mut shapes) =
                        convert_grid!(rd.grid, rd.blend_shapes, convert_rotation);
                    if (r - 1.0).abs() > 1e-6 {
                        forms.iter_mut().for_each(|f| f.scale *= r);
                        shapes.iter_mut().for_each(|s| s.form.scale *= r);
                    }
                    let dm = Arc::make_mut(&mut m.deformers[i]);
                    if let DeformerKind::Rotation(rd) = &mut dm.kind {
                        rd.grid.keyforms = forms;
                        rd.blend_shapes.shapes = shapes;
                    }
                    dm.target = new_parent;
                }
            }
        }
        _ => return Err("Only art meshes and deformers have parent deformers.".into()),
    }
    Ok(true)
}

/// Rotation deformers reached from `d` without passing another rotation
/// deformer (change-parent.md §3).
fn first_rotation_descendants(m: &Model, d: DeformerIndex) -> Vec<DeformerIndex> {
    let mut out = Vec::new();
    let mut stack = vec![d];
    let mut seen = vec![false; m.deformers.len()];
    while let Some(p) = stack.pop() {
        for (k, x) in m.deformers.iter().enumerate() {
            if x.target == Some(p) && !std::mem::replace(&mut seen[k], true) {
                match x.kind {
                    DeformerKind::Rotation(_) => out.push(k),
                    DeformerKind::Warp(_) => stack.push(k),
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- creation

/// The first `prefix` + number not used as an ID by `taken`.
pub fn unique_id<'a>(prefix: &str, taken: impl Iterator<Item = &'a str>) -> String {
    let used: Vec<&str> = taken.collect();
    let next = used
        .iter()
        .filter_map(|id| id.strip_prefix(prefix)?.parse::<u64>().ok())
        .max()
        .map_or(0, |n| n + 1);
    format!("{prefix}{next}")
}

/// Where a new deformer goes and which objects it adopts.
pub struct NewDeformer<'a> {
    pub name: String,
    /// "Add To": the destination part (root part if `None`).
    pub part: Option<PartIndex>,
    /// Parent deformer (`None` = canvas root).
    pub parent: Option<DeformerIndex>,
    /// Objects re-parented to the new deformer ("Set as parent of selected
    /// object"); they must share `parent`.
    pub children: &'a [ObjectRef],
    /// Current parameter values.
    pub values: &'a [f32],
}

/// Canvas points of an object at `frame`.
fn canvas_points(m: &Model, frame: &Frame, o: ObjectRef) -> Vec<V2> {
    match o {
        ObjectRef::ArtMesh(i) => frame
            .meshes
            .get(i)
            .map(|s| s.positions.chunks_exact(2).map(|p| [p[0], p[1]]).collect())
            .unwrap_or_default(),
        ObjectRef::Deformer(i) => match frame.deformers.get(i).and_then(|s| s.map.as_ref()) {
            Some(DeformerMap::Warp(w)) => w.points.chunks_exact(2).map(|p| [p[0], p[1]]).collect(),
            Some(DeformerMap::Rotation(r)) => vec![r.origin],
            None => Vec::new(),
        },
        _ => {
            let _ = m;
            Vec::new()
        }
    }
}

/// Bounding box (min, max) of the children's points in the parent's space.
fn children_box(m: &Model, nd: &NewDeformer) -> Result<Option<(V2, V2)>, String> {
    if nd.children.is_empty() {
        return Ok(None);
    }
    for &c in nd.children {
        if target_of(m, c) != nd.parent {
            return Err(
                "Please create after selecting objects with the same parent deformer.".into(),
            );
        }
    }
    let frame = Evaluator::new(m).evaluate(nd.values);
    let parent = map_of(&frame, nd.parent)?;
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for &c in nd.children {
        for p in canvas_points(m, &frame, c) {
            let q = convert(p, None, parent.as_ref())?;
            for k in 0..2 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    Ok((lo[0].is_finite()).then_some((lo, hi)))
}

fn parent_kind(m: &Model, d: Option<DeformerIndex>) -> Option<bool> {
    d.and_then(|d| m.deformers.get(d))
        .map(|x| matches!(x.kind, DeformerKind::Warp(_)))
}

/// Inserts a new object into a part's child list: after the first adopted
/// child if it is in that part, else at the end.
fn add_to_part(m: &mut Model, part: PartIndex, child: PartChild, near: Option<PartChild>) {
    let Some(p) = m.parts.get_mut(part) else {
        return;
    };
    let p = Arc::make_mut(p);
    match near.and_then(|n| p.children.iter().position(|c| *c == n)) {
        Some(k) => p.children.insert(k, child),
        None => p.children.push(child),
    }
}

fn as_child(o: ObjectRef) -> PartChild {
    match o {
        ObjectRef::Part(i) => PartChild::Part(i),
        ObjectRef::Deformer(i) => PartChild::Deformer(i),
        ObjectRef::ArtMesh(i) => PartChild::ArtMesh(i),
        ObjectRef::Glue(i) => PartChild::Glue(i),
    }
}

fn finish_deformer(
    m: &mut Model,
    nd: &NewDeformer,
    kind: DeformerKind,
    prefix: &str,
) -> Result<DeformerIndex, String> {
    let id = unique_id(prefix, m.deformers.iter().map(|d| d.info.id.as_str()));
    let part = nd.part.or(m.root_part);
    let mut info = ObjectInfo::new(&id);
    info.guid = new_guid();
    info.name = nd.name.clone();
    info.parent_part = part;
    let d = m.deformers.len();
    m.deformers.push(Arc::new(Deformer {
        info,
        target: nd.parent,
        kind,
    }));
    if let Some(p) = part {
        add_to_part(
            m,
            p,
            PartChild::Deformer(d),
            nd.children.first().map(|&c| as_child(c)),
        );
    }
    for &c in nd.children {
        change_parent(m, c, Some(d), nd.values)?;
    }
    Ok(d)
}

/// Creates a rotation deformer. spec: 50-ui/dialogs.md §7
pub fn create_rotation_deformer(m: &mut Model, nd: &NewDeformer) -> Result<DeformerIndex, String> {
    let (w, h) = (m.canvas.width as f32, m.canvas.height as f32);
    let origin = match children_box(m, nd)? {
        Some((lo, hi)) => [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0],
        None => match parent_kind(m, nd.parent) {
            None => [w / 2.0, h / 2.0],
            Some(true) => [0.5, 0.5],
            Some(false) => [0.0, 0.0],
        },
    };
    let form = RotationForm {
        origin,
        ..RotationForm::default()
    };
    let kind = DeformerKind::Rotation(RotationDeformer {
        handle_length: w / 10.0,
        circle_radius: w / 30.0,
        base_angle: 0.0,
        grid: KeyformGrid {
            guids: vec![new_guid()],
            ..KeyformGrid::single(form)
        },
        blend_shapes: Default::default(),
    });
    finish_deformer(m, nd, kind, "Rotation")
}

/// Creates a warp deformer of `cols`×`rows` cells with a level-2 Bezier net
/// of `bezier` cells. spec: 50-ui/dialogs.md §6
pub fn create_warp_deformer(
    m: &mut Model,
    nd: &NewDeformer,
    cols: u32,
    rows: u32,
    bezier: (u32, u32),
) -> Result<DeformerIndex, String> {
    let (cw, ch) = (m.canvas.width as f32, m.canvas.height as f32);
    let warp_parent = parent_kind(m, nd.parent);
    // (x, y, width, height) in the parent's local space.
    let rect = match children_box(m, nd)? {
        Some((lo, hi)) => {
            let (mut x, mut y, mut rw, mut rh) = (lo[0], lo[1], hi[0] - lo[0], hi[1] - lo[1]);
            if rw < 0.002 && rh < 0.002 {
                let (dy, e) = if warp_parent == Some(true) {
                    (0.1, 0.3)
                } else {
                    (30.0, 100.0)
                };
                y += dy;
                x -= e;
                y -= e;
                rw += 2.0 * e;
                rh += 2.0 * e;
            } else {
                rw = rw.max(0.002);
                rh = rh.max(0.002);
                x -= rw * 0.05;
                y -= rh * 0.05;
                rw *= 1.1;
                rh *= 1.1;
            }
            (x, y, rw, rh)
        }
        None => match warp_parent {
            None => (cw * 0.05, ch * 0.05, cw * 0.9, ch * 0.9),
            Some(true) => (0.05, 0.05, 0.9, 0.9),
            Some(false) => {
                let (x, y, rw, rh) = (-cw / 4.0, ch / 4.0, cw / 2.0, ch / 2.0);
                (x + 0.05 * rw, y + 0.05 * rh, rw * 0.9, rh * 0.9)
            }
        },
    };
    let (cols, rows) = (cols.clamp(1, 100), rows.clamp(1, 100));
    let mut positions = Vec::with_capacity(2 * ((cols + 1) * (rows + 1)) as usize);
    for j in 0..=rows {
        for i in 0..=cols {
            positions.push(rect.0 + rect.2 * i as f32 / cols as f32);
            positions.push(rect.1 + rect.3 * j as f32 / rows as f32);
        }
    }
    let kind = DeformerKind::Warp(WarpDeformer {
        bezier_nets: vec![
            (2, bezier.0.max(1), bezier.1.max(1)),
            DEFAULT_BEZIER_NETS[1],
        ],
        cols,
        rows,
        quad: m.target_version >= 3030,
        grid: KeyformGrid {
            guids: vec![new_guid()],
            ..KeyformGrid::single(WarpForm {
                positions,
                ..WarpForm::default()
            })
        },
        blend_shapes: Default::default(),
    });
    finish_deformer(m, nd, kind, "Warp")
}

/// Creates an empty part under `parent` (the root part if `None`).
pub fn create_part(m: &mut Model, name: &str, parent: Option<PartIndex>) -> PartIndex {
    let id = unique_id("Part", m.parts.iter().map(|p| p.info.id.as_str()));
    let mut part = Part::new(&id);
    part.info.guid = new_guid();
    part.info.name = name.to_owned();
    part.grid.guids = vec![new_guid()];
    let parent = parent.or(m.root_part);
    part.info.parent_part = parent;
    let i = m.parts.len();
    m.parts.push(Arc::new(part));
    if let Some(p) = parent {
        add_to_part(m, p, PartChild::Part(i), None);
    }
    i
}

// ---------------------------------------------------------------- deletion

/// Renumbers deformer references after removing deformer `d`.
fn remove_deformer_index(m: &mut Model, d: DeformerIndex) {
    let f = |i: DeformerIndex| if i > d { i - 1 } else { i };
    for x in &mut m.deformers {
        if x.target.is_some_and(|t| t >= d) {
            let x = Arc::make_mut(x);
            x.target = x.target.map(f);
        }
    }
    for a in &mut m.art_meshes {
        if a.target.is_some_and(|t| t >= d) {
            let a = Arc::make_mut(a);
            a.target = a.target.map(f);
        }
    }
    for p in &mut m.parts {
        if p.children
            .iter()
            .any(|c| matches!(c, PartChild::Deformer(i) if *i >= d))
        {
            let p = Arc::make_mut(p);
            p.children.retain(|c| *c != PartChild::Deformer(d));
            for c in &mut p.children {
                if let PartChild::Deformer(i) = c {
                    *i = f(*i);
                }
            }
        }
    }
}

/// Deletes deformer `d`; its children move to its parent deformer with
/// form conversion (change-parent.md "Used by … deformer deletion").
pub fn delete_deformer(m: &mut Model, d: DeformerIndex, values: &[f32]) -> Result<(), String> {
    let parent = m.deformers.get(d).ok_or("no such deformer")?.target;
    let children: Vec<ObjectRef> = (0..m.deformers.len())
        .filter(|&k| m.deformers[k].target == Some(d))
        .map(ObjectRef::Deformer)
        .chain(
            (0..m.art_meshes.len())
                .filter(|&k| m.art_meshes[k].target == Some(d))
                .map(ObjectRef::ArtMesh),
        )
        .collect();
    for c in children {
        change_parent(m, c, parent, values)?;
    }
    remove_deformer_index(m, d);
    m.deformers.remove(d);
    Ok(())
}

/// Deletes an empty part (not the root part).
pub fn delete_part(m: &mut Model, p: PartIndex) -> Result<(), String> {
    let part = m.parts.get(p).ok_or("no such part")?;
    if Some(p) == m.root_part {
        return Err("The root part cannot be deleted.".into());
    }
    if part.children.iter().any(|c| !matches!(c, PartChild::Other)) {
        return Err("Only empty parts can be deleted.".into());
    }
    let f = |i: PartIndex| if i > p { i - 1 } else { i };
    for x in &mut m.parts {
        let x = Arc::make_mut(x);
        x.children.retain(|c| *c != PartChild::Part(p));
        for c in &mut x.children {
            if let PartChild::Part(i) = c {
                *i = f(*i);
            }
        }
        x.info.parent_part = x.info.parent_part.map(f);
    }
    macro_rules! reparent {
        ($list:expr) => {
            for x in &mut $list {
                if x.info.parent_part.is_some_and(|q| q > p) {
                    let x = Arc::make_mut(x);
                    x.info.parent_part = x.info.parent_part.map(f);
                }
            }
        };
    }
    reparent!(m.deformers);
    reparent!(m.art_meshes);
    reparent!(m.glues);
    m.root_part = m.root_part.map(f);
    m.parts.remove(p);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{add_key, keys_of};
    use keyform_model::*;

    fn model() -> Model {
        let mut m = Model::default();
        add_parameter(&mut m, Parameter::new("A", -1.0, 1.0, 0.0), None);
        add_parameter(&mut m, Parameter::new("B", 0.0, 1.0, 0.0), None);
        add_parameter(&mut m, Parameter::new("C", 0.0, 10.0, 5.0), None);
        let mut part = Part::new("P");
        part.grid = KeyformGrid::single(PartForm::default());
        m.parts.push(Arc::new(part));
        m
    }

    #[test]
    fn remove_parameter_unbinds_and_renumbers() {
        let mut m = model();
        assert!(m.parameters.iter().all(|p| p.guid.len() == 36));
        let o = ObjectRef::Part(0);
        add_key(&mut m, o, 0, -1.0).unwrap();
        add_key(&mut m, o, 0, 1.0).unwrap();
        add_key(&mut m, o, 2, 10.0).unwrap();
        // Different opacity at A = 1 so unbinding has a shape to keep.
        Arc::make_mut(&mut m.parts[0]).grid.keyforms[1].opacity = 0.5;
        m.eye_blink_parameters = vec![0, 2];
        assert!(remove_parameter(&mut m, 0));
        assert_eq!(m.parameters.len(), 2);
        assert_eq!(m.parameters[1].id, "C");
        assert_eq!(keys_of(&m, o, 1), vec![10.0], "C's binding renumbered");
        assert_eq!(m.parts[0].grid.bindings.len(), 1);
        // The shape at A's default (0) was kept: opacity halfway.
        assert!((m.parts[0].grid.keyforms[0].opacity - 0.75).abs() < 1e-6);
        assert_eq!(m.eye_blink_parameters, vec![1]);
        assert_eq!(
            m.root_parameter_entries,
            vec![ParamEntry::Parameter(0), ParamEntry::Parameter(1)]
        );
    }
}
