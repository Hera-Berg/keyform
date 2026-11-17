//! Adding and removing keys on an object's keyform grid.
//! spec: 35-editing/keys.md §1–2, §4

use std::sync::Arc;

use keyform_eval::interp::Blend;
use keyform_eval::keys::insert_key;
use keyform_model::{
    new_guid, Binding, DeformerKind, KeyformGrid, Model, ParamIndex, Parameter, ParameterKind,
};

/// An object that has a keyform grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectRef {
    Part(usize),
    Deformer(usize),
    ArtMesh(usize),
    Glue(usize),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("no such object or parameter")]
    Missing,
    #[error("cannot add key: too close to an existing key")]
    TooClose,
    #[error("the object has no key at this value")]
    NoKey,
    #[error("the minimum and maximum keys of a repeat parameter cannot be removed")]
    RepeatEnds,
}

/// An operation on one grid, whatever the keyform type.
trait GridOp {
    type Out;
    fn run<F: Blend + Clone>(self, grid: &mut KeyformGrid<F>) -> Self::Out;
}

fn with_grid<O: GridOp>(m: &mut Model, o: ObjectRef, op: O) -> Option<O::Out> {
    Some(match o {
        ObjectRef::Part(i) => op.run(&mut Arc::make_mut(m.parts.get_mut(i)?).grid),
        ObjectRef::ArtMesh(i) => op.run(&mut Arc::make_mut(m.art_meshes.get_mut(i)?).grid),
        ObjectRef::Glue(i) => op.run(&mut Arc::make_mut(m.glues.get_mut(i)?).grid),
        ObjectRef::Deformer(i) => match &mut Arc::make_mut(m.deformers.get_mut(i)?).kind {
            DeformerKind::Warp(w) => op.run(&mut w.grid),
            DeformerKind::Rotation(r) => op.run(&mut r.grid),
        },
    })
}

/// Key values of `o` on parameter `p` (empty if unbound).
pub fn keys_of(m: &Model, o: ObjectRef, p: ParamIndex) -> Vec<f32> {
    fn find(b: &[Binding], p: ParamIndex) -> Vec<f32> {
        b.iter()
            .find(|b| b.parameter == p)
            .map(|b| b.keys.clone())
            .unwrap_or_default()
    }
    match o {
        ObjectRef::Part(i) => m.parts.get(i).map(|x| find(&x.grid.bindings, p)),
        ObjectRef::ArtMesh(i) => m.art_meshes.get(i).map(|x| find(&x.grid.bindings, p)),
        ObjectRef::Glue(i) => m.glues.get(i).map(|x| find(&x.grid.bindings, p)),
        ObjectRef::Deformer(i) => m.deformers.get(i).map(|d| match &d.kind {
            DeformerKind::Warp(w) => find(&w.grid.bindings, p),
            DeformerKind::Rotation(r) => find(&r.grid.bindings, p),
        }),
    }
    .unwrap_or_default()
}

/// Number of parameters `o` is bound to.
pub fn binding_count(m: &Model, o: ObjectRef) -> usize {
    match o {
        ObjectRef::Part(i) => m.parts.get(i).map(|x| x.grid.bindings.len()),
        ObjectRef::ArtMesh(i) => m.art_meshes.get(i).map(|x| x.grid.bindings.len()),
        ObjectRef::Glue(i) => m.glues.get(i).map(|x| x.grid.bindings.len()),
        ObjectRef::Deformer(i) => m.deformers.get(i).map(|d| match &d.kind {
            DeformerKind::Warp(w) => w.grid.bindings.len(),
            DeformerKind::Rotation(r) => r.grid.bindings.len(),
        }),
    }
    .unwrap_or(0)
}

/// Gives every untracked cell of a tracked grid its own identity, so that
/// edited grids never contain anonymous keyforms.
fn fill_guids<F>(grid: &mut KeyformGrid<F>) {
    if grid.guids.len() == grid.keyforms.len() {
        for g in grid.guids.iter_mut().filter(|g| g.is_empty()) {
            *g = new_guid();
        }
    }
}

struct AddKey<'a> {
    p: ParamIndex,
    param: &'a Parameter,
    v: f32,
}

impl GridOp for AddKey<'_> {
    type Out = Result<bool, KeyError>;
    fn run<F: Blend + Clone>(self, grid: &mut KeyformGrid<F>) -> Self::Out {
        fill_guids(grid);
        let Some(j) = grid.bindings.iter().position(|b| b.parameter == self.p) else {
            // Not bound yet: the existing keyforms become the single key's.
            grid.bindings.push(Binding::new(self.p, vec![self.v]));
            return Ok(true);
        };
        let keys = &grid.bindings[j].keys;
        if keys.contains(&self.v) {
            return Ok(false);
        }
        if keys
            .iter()
            .any(|k| (k - self.v).abs() < self.param.snap_epsilon)
        {
            return Err(KeyError::TooClose);
        }
        insert_key(grid, j, self.param, self.v)
            .map(|_| true)
            .ok_or(KeyError::Missing)
    }
}

/// Adds key `v` of parameter `p` to object `o` (keys.md §1). The new
/// keyforms equal the object's interpolated forms there, so nothing moves.
/// Returns whether anything changed.
pub fn add_key(m: &mut Model, o: ObjectRef, p: ParamIndex, v: f32) -> Result<bool, KeyError> {
    let param = (*m.parameters.get(p).ok_or(KeyError::Missing)?).clone();
    if param.kind == ParameterKind::BlendShape {
        return Ok(false);
    }
    let bound = !keys_of(m, o, p).is_empty();
    if param.repeat && !bound {
        // Repeat parameters always carry their min and max keys (§1, §4).
        let mut changed = false;
        for x in [param.min, param.max, v] {
            changed |= add_key_one(m, o, p, &param, x)?;
        }
        return Ok(changed);
    }
    add_key_one(m, o, p, &param, v)
}

fn add_key_one(
    m: &mut Model,
    o: ObjectRef,
    p: ParamIndex,
    param: &Parameter,
    v: f32,
) -> Result<bool, KeyError> {
    with_grid(m, o, AddKey { p, param, v }).ok_or(KeyError::Missing)?
}

struct RemoveKey {
    p: ParamIndex,
    k: usize,
}

impl GridOp for RemoveKey {
    type Out = bool;
    fn run<F: Blend + Clone>(self, grid: &mut KeyformGrid<F>) -> bool {
        let Some(j) = grid.bindings.iter().position(|b| b.parameter == self.p) else {
            return false;
        };
        let n = grid.bindings[j].keys.len();
        if n == 1 {
            // The binding goes; the keyforms stay (their index on it was 0).
            grid.bindings.remove(j);
            return true;
        }
        let dims: Vec<usize> = grid.bindings.iter().map(|b| b.keys.len()).collect();
        let stride: usize = dims[..j].iter().product();
        let track = grid.guids.len() == grid.keyforms.len();
        let mut forms = Vec::with_capacity(grid.keyforms.len() / n * (n - 1));
        let mut guids = Vec::new();
        for (cell, f) in grid.keyforms.iter().enumerate() {
            if (cell / stride) % n == self.k {
                continue;
            }
            forms.push(f.clone());
            if track {
                guids.push(grid.guids[cell].clone());
            }
        }
        grid.bindings[j].keys.remove(self.k);
        grid.keyforms = forms;
        if track {
            grid.guids = guids;
        }
        true
    }
}

/// Removes key `v` of parameter `p` from object `o` (keys.md §2).
pub fn remove_key(m: &mut Model, o: ObjectRef, p: ParamIndex, v: f32) -> Result<bool, KeyError> {
    let param = m.parameters.get(p).ok_or(KeyError::Missing)?;
    let keys = keys_of(m, o, p);
    let Some(k) = keys
        .iter()
        .position(|key| (key - v).abs() < param.snap_epsilon)
    else {
        return Err(KeyError::NoKey);
    };
    if param.repeat && keys.len() > 1 && (keys[k] == param.min || keys[k] == param.max) {
        return Err(KeyError::RepeatEnds);
    }
    with_grid(m, o, RemoveKey { p, k }).ok_or(KeyError::Missing)
}

/// "Delete all keys": unbinds `p` from `o`, keeping the shape at the
/// parameter's current value `v` (keys.md §2).
pub fn delete_all_keys(
    m: &mut Model,
    o: ObjectRef,
    p: ParamIndex,
    v: f32,
) -> Result<bool, KeyError> {
    let param = (*m.parameters.get(p).ok_or(KeyError::Missing)?).clone();
    if keys_of(m, o, p).is_empty() {
        return Ok(false);
    }
    let v = if param.repeat {
        let (lo, hi) = (param.min.min(param.max), param.max.max(param.min));
        if hi > lo {
            lo + (v - lo).rem_euclid(hi - lo)
        } else {
            v
        }
    } else {
        v
    };
    let keys = keys_of(m, o, p);
    let keep = match keys.iter().position(|k| (k - v).abs() < param.snap_epsilon) {
        Some(i) => keys[i],
        None => {
            add_key_one(m, o, p, &param, v)?;
            v
        }
    };
    // Remove the others (repeat ends included: the binding goes anyway).
    loop {
        let keys = keys_of(m, o, p);
        let Some(k) = keys.iter().position(|&k| k != keep) else {
            break;
        };
        with_grid(m, o, RemoveKey { p, k }).ok_or(KeyError::Missing)?;
    }
    if let Some(k) = keys_of(m, o, p).iter().position(|&k| k == keep) {
        with_grid(m, o, RemoveKey { p, k }).ok_or(KeyError::Missing)?;
    }
    Ok(true)
}

/// An operation on an object's grid together with its blend shapes.
trait BlendOp {
    type Out;
    fn run<F: Blend + Clone>(
        self,
        params: &[std::sync::Arc<keyform_model::Parameter>],
        grid: &mut KeyformGrid<F>,
        blend: &mut keyform_model::BlendShapes<F>,
    ) -> Self::Out;
}

fn with_blend<O: BlendOp>(m: &mut Model, o: ObjectRef, op: O) -> Option<O::Out> {
    let params = m.parameters.clone();
    Some(match o {
        ObjectRef::Part(i) => {
            let x = Arc::make_mut(m.parts.get_mut(i)?);
            op.run(&params, &mut x.grid, &mut x.blend_shapes)
        }
        ObjectRef::ArtMesh(i) => {
            let x = Arc::make_mut(m.art_meshes.get_mut(i)?);
            op.run(&params, &mut x.grid, &mut x.blend_shapes)
        }
        ObjectRef::Glue(i) => {
            let x = Arc::make_mut(m.glues.get_mut(i)?);
            op.run(&params, &mut x.grid, &mut x.blend_shapes)
        }
        ObjectRef::Deformer(i) => match &mut Arc::make_mut(m.deformers.get_mut(i)?).kind {
            DeformerKind::Warp(w) => op.run(&params, &mut w.grid, &mut w.blend_shapes),
            DeformerKind::Rotation(r) => op.run(&params, &mut r.grid, &mut r.blend_shapes),
        },
    })
}

/// The key of a blend-shape entry (unset = the parameter's max).
fn entry_key(
    params: &[std::sync::Arc<keyform_model::Parameter>],
    p: ParamIndex,
    key: Option<f32>,
) -> f32 {
    key.unwrap_or_else(|| params.get(p).map_or(1.0, |q| q.max))
}

struct AddBlend {
    p: ParamIndex,
    key: f32,
}

impl BlendOp for AddBlend {
    type Out = Result<bool, KeyError>;
    fn run<F: Blend + Clone>(
        self,
        params: &[std::sync::Arc<keyform_model::Parameter>],
        grid: &mut KeyformGrid<F>,
        blend: &mut keyform_model::BlendShapes<F>,
    ) -> Self::Out {
        let eps = params.get(self.p).map_or(1e-3, |q| q.snap_epsilon);
        if self.key.abs() < eps {
            // 0 is the implicit base key (blend-shapes.md §3).
            return Err(KeyError::TooClose);
        }
        if blend.shapes.iter().any(|s| {
            s.parameter == self.p && (entry_key(params, self.p, s.key) - self.key).abs() < eps
        }) {
            return Err(KeyError::TooClose);
        }
        let base = keyform_eval::interp::base_keyform(params, grid)
            .ok_or(KeyError::Missing)?
            .clone();
        let max = params.get(self.p).map_or(1.0, |q| q.max);
        blend.shapes.push(keyform_model::BlendShape {
            parameter: self.p,
            // An entry at the max is stored unset, as the reference does.
            key: ((self.key - max).abs() >= eps).then_some(self.key),
            form: base,
        });
        Ok(true)
    }
}

struct RemoveBlend {
    p: ParamIndex,
    key: Option<f32>,
}

impl BlendOp for RemoveBlend {
    type Out = bool;
    fn run<F: Blend + Clone>(
        self,
        params: &[std::sync::Arc<keyform_model::Parameter>],
        _grid: &mut KeyformGrid<F>,
        blend: &mut keyform_model::BlendShapes<F>,
    ) -> Self::Out {
        let eps = params.get(self.p).map_or(1e-3, |q| q.snap_epsilon);
        let n = blend.shapes.len();
        blend.shapes.retain(|s| {
            s.parameter != self.p
                || self
                    .key
                    .is_some_and(|k| (entry_key(params, self.p, s.key) - k).abs() >= eps)
        });
        if self.key.is_none() {
            blend.constraints.retain(|c| c.blend_parameter != self.p);
        }
        blend.shapes.len() != n
    }
}

/// Adds a blend-shape entry of parameter `p` at key `key` to object `o`,
/// starting as the object's base keyform (so nothing moves).
/// spec: 30-algorithms/blend-shapes.md §1, §3
// TODO(Q-0037): the reference's rules for adding blend-shape keys.
pub fn add_blend_shape(
    m: &mut Model,
    o: ObjectRef,
    p: ParamIndex,
    key: f32,
) -> Result<bool, KeyError> {
    if m.parameters
        .get(p)
        .is_none_or(|q| q.kind != ParameterKind::BlendShape)
    {
        return Err(KeyError::Missing);
    }
    with_blend(m, o, AddBlend { p, key }).ok_or(KeyError::Missing)?
}

/// Removes `o`'s blend-shape entry of `p` at `key`, or all of them (and
/// their constraints) with `None`.
pub fn remove_blend_shape(m: &mut Model, o: ObjectRef, p: ParamIndex, key: Option<f32>) -> bool {
    with_blend(m, o, RemoveBlend { p, key }).unwrap_or(false)
}

/// Blend-shape key values of `o` on `p` (unset keys as the max).
pub fn blend_keys_of(m: &Model, o: ObjectRef, p: ParamIndex) -> Vec<f32> {
    let params = &m.parameters;
    let keys = |shapes: Vec<(ParamIndex, Option<f32>)>| -> Vec<f32> {
        let mut v: Vec<f32> = shapes
            .into_iter()
            .filter(|s| s.0 == p)
            .map(|s| entry_key(params, p, s.1))
            .collect();
        v.sort_by(f32::total_cmp);
        v
    };
    macro_rules! of {
        ($b:expr) => {
            keys($b.shapes.iter().map(|s| (s.parameter, s.key)).collect())
        };
    }
    match o {
        ObjectRef::Part(i) => m.parts.get(i).map(|x| of!(x.blend_shapes)),
        ObjectRef::ArtMesh(i) => m.art_meshes.get(i).map(|x| of!(x.blend_shapes)),
        ObjectRef::Glue(i) => m.glues.get(i).map(|x| of!(x.blend_shapes)),
        ObjectRef::Deformer(i) => m.deformers.get(i).map(|d| match &d.kind {
            DeformerKind::Warp(w) => of!(w.blend_shapes),
            DeformerKind::Rotation(r) => of!(r.blend_shapes),
        }),
    }
    .unwrap_or_default()
}

/// The blend-shape entry an edit at `values` goes to: a blend-shape
/// parameter whose value is on one of `o`'s entry keys (other than 0).
/// Returns (parameter, entry index in the object's list).
// TODO(Q-0037): the reference's rule when several are on keys.
pub fn blend_target(m: &Model, o: ObjectRef, values: &[f32]) -> Option<(ParamIndex, usize)> {
    let params = &m.parameters;
    let find = |shapes: Vec<(ParamIndex, Option<f32>)>| {
        shapes.iter().enumerate().find_map(|(i, (p, key))| {
            let q = params.get(*p)?;
            let v = values.get(*p).copied().unwrap_or(q.default);
            let k = entry_key(params, *p, *key);
            (k.abs() >= q.snap_epsilon && (v - k).abs() < q.snap_epsilon).then_some((*p, i))
        })
    };
    macro_rules! of {
        ($b:expr) => {
            find($b.shapes.iter().map(|s| (s.parameter, s.key)).collect())
        };
    }
    match o {
        ObjectRef::Part(i) => m.parts.get(i).and_then(|x| of!(x.blend_shapes)),
        ObjectRef::ArtMesh(i) => m.art_meshes.get(i).and_then(|x| of!(x.blend_shapes)),
        ObjectRef::Glue(i) => m.glues.get(i).and_then(|x| of!(x.blend_shapes)),
        ObjectRef::Deformer(i) => m.deformers.get(i).and_then(|d| match &d.kind {
            DeformerKind::Warp(w) => of!(w.blend_shapes),
            DeformerKind::Rotation(r) => of!(r.blend_shapes),
        }),
    }
}

struct SwapBlend {
    cell: usize,
    entry: usize,
}

impl BlendOp for SwapBlend {
    type Out = bool;
    fn run<F: Blend + Clone>(
        self,
        _params: &[std::sync::Arc<keyform_model::Parameter>],
        grid: &mut KeyformGrid<F>,
        blend: &mut keyform_model::BlendShapes<F>,
    ) -> Self::Out {
        match (
            grid.keyforms.get_mut(self.cell),
            blend.shapes.get_mut(self.entry),
        ) {
            (Some(k), Some(s)) => {
                std::mem::swap(k, &mut s.form);
                true
            }
            _ => false,
        }
    }
}

/// Swaps grid keyform `cell` with blend-shape entry `entry` of `o`: edit
/// functions written for grid keyforms then edit the entry (swap, edit,
/// swap back).
pub fn swap_blend_form(m: &mut Model, o: ObjectRef, cell: usize, entry: usize) -> bool {
    with_blend(m, o, SwapBlend { cell, entry }).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::{ArtMesh, ArtMeshForm, EditableMesh, ObjectInfo};

    fn model() -> Model {
        let form = |x: f32| ArtMeshForm {
            positions: vec![x, 0.0],
            ..ArtMeshForm::default()
        };
        let mut a = ArtMesh {
            clip_missing: false,
            info: ObjectInfo::new("A"),
            target: None,
            positions: vec![0.0, 0.0],
            uvs: vec![0.0, 0.0],
            indices: Vec::new(),
            texture: None,
            clip: Vec::new(),
            invert_clip: false,
            culling: false,
            color_composition: Default::default(),
            alpha_composition: Default::default(),
            user_data: String::new(),
            point_uids: vec![0],
            mesh: EditableMesh::default(),
            model_image: None,
            generator: Default::default(),
            grid: KeyformGrid::single(form(0.0)),
            blend_shapes: Default::default(),
            paths: Vec::new(),
        };
        a.grid = KeyformGrid {
            bindings: vec![Binding::new(0, vec![-10.0, 10.0])],
            keyforms: vec![form(-10.0), form(10.0)],
            guids: vec!["g0".into(), "g1".into()],
        };
        let mut rep = Parameter::new("R", -180.0, 180.0, 0.0);
        rep.repeat = true;
        Model {
            parameters: vec![
                Arc::new(Parameter::new("X", -10.0, 10.0, 0.0)),
                Arc::new(Parameter::new("Y", 0.0, 1.0, 0.0)),
                Arc::new(rep),
            ],
            art_meshes: vec![Arc::new(a)],
            ..Model::default()
        }
    }

    fn xs(m: &Model) -> Vec<f32> {
        m.art_meshes[0]
            .grid
            .keyforms
            .iter()
            .map(|f| f.positions[0])
            .collect()
    }

    #[test]
    fn add_key_interpolates_and_keeps_identities() {
        let mut m = model();
        let a = ObjectRef::ArtMesh(0);
        assert_eq!(add_key(&mut m, a, 0, 5.0), Ok(true));
        assert_eq!(keys_of(&m, a, 0), vec![-10.0, 5.0, 10.0]);
        assert_eq!(xs(&m), vec![-10.0, 5.0, 10.0]);
        let g = &m.art_meshes[0].grid.guids;
        assert_eq!((g[0].as_str(), g[2].as_str()), ("g0", "g1"));
        assert_eq!(g[1].len(), 36, "new keyform gets a new GUID");
        assert_eq!(add_key(&mut m, a, 0, 5.0), Ok(false));
        assert_eq!(add_key(&mut m, a, 0, 5.0005), Err(KeyError::TooClose));
        // A second parameter: binding with one key, nothing else changes.
        assert_eq!(add_key(&mut m, a, 1, 0.0), Ok(true));
        assert_eq!(m.art_meshes[0].grid.bindings.len(), 2);
        assert_eq!(xs(&m), vec![-10.0, 5.0, 10.0]);
        // Its second key duplicates the row of forms.
        assert_eq!(add_key(&mut m, a, 1, 1.0), Ok(true));
        assert_eq!(xs(&m), vec![-10.0, 5.0, 10.0, -10.0, 5.0, 10.0]);
        assert!(m.art_meshes[0].grid.is_consistent());
    }

    #[test]
    fn repeat_parameter_gets_min_and_max() {
        let mut m = model();
        let a = ObjectRef::ArtMesh(0);
        add_key(&mut m, a, 2, 30.0).unwrap();
        assert_eq!(keys_of(&m, a, 2), vec![-180.0, 30.0, 180.0]);
        assert_eq!(remove_key(&mut m, a, 2, 180.0), Err(KeyError::RepeatEnds));
        assert_eq!(remove_key(&mut m, a, 2, 30.0), Ok(true));
    }

    #[test]
    fn remove_and_delete_all() {
        let mut m = model();
        let a = ObjectRef::ArtMesh(0);
        add_key(&mut m, a, 0, 0.0).unwrap();
        assert_eq!(remove_key(&mut m, a, 0, 3.0), Err(KeyError::NoKey));
        assert_eq!(remove_key(&mut m, a, 0, -10.0), Ok(true));
        assert_eq!(xs(&m), vec![0.0, 10.0]);
        assert_eq!(m.art_meshes[0].grid.guids[1], "g1");
        // Delete all keys at 5: the shape there is kept, the binding goes.
        assert_eq!(delete_all_keys(&mut m, a, 0, 5.0), Ok(true));
        assert!(m.art_meshes[0].grid.bindings.is_empty());
        assert_eq!(xs(&m), vec![5.0]);
        assert_eq!(delete_all_keys(&mut m, a, 0, 5.0), Ok(false));
    }
}
