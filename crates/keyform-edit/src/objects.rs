//! Creating and deleting model objects.
//!
//! Parameters: spec 50-ui/palettes.md §3 ("Create new parameter",
//! "Delete parameter": related physics settings are deleted too),
//! 35-editing/keys.md §2 (unbinding keeps the shape at a value).

use std::sync::Arc;

use keyform_model::{new_guid, DeformerKind, Model, ParamEntry, ParamIndex, Parameter};

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
