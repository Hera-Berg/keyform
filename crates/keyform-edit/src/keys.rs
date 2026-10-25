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

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::{ArtMesh, ArtMeshForm, ObjectInfo};

    fn model() -> Model {
        let form = |x: f32| ArtMeshForm {
            positions: vec![x, 0.0],
            ..ArtMeshForm::default()
        };
        let mut a = ArtMesh {
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
            grid: KeyformGrid::single(form(0.0)),
            blend_shapes: Default::default(),
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
