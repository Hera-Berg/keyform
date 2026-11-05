//! Editing the current keyform of an object: the keyform at the current
//! parameter values, which must sit on a key of every bound parameter.
//! spec: 35-editing/form-tools.md (intro), 50-ui/canvas-and-tools.md §3,
//! 30-algorithms/deformers.md §1.4, §2.2

use std::sync::Arc;

use keyform_eval::DeformerMap;
use keyform_model::{Binding, DeformerKind, Model, Parameter};

use crate::keys::ObjectRef;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FormError {
    #[error("There is no keyform to edit at the current parameter values.")]
    NotOnKey,
    #[error("no such object")]
    Missing,
    #[error("the point cannot be mapped into the parent deformer")]
    NotInvertible,
}

/// Grid cell of the keyform at `values`, if every binding is on a key
/// (within the parameter's snap epsilon; repeat values wrap).
fn cell_at(
    bindings: &[Binding],
    params: &[Arc<Parameter>],
    values: &[f32],
) -> Result<usize, FormError> {
    let mut cell = 0;
    let mut stride = 1;
    for b in bindings {
        let p = params.get(b.parameter).ok_or(FormError::Missing)?;
        let mut v = values.get(b.parameter).copied().unwrap_or(p.default);
        if p.repeat {
            let (lo, hi) = (p.min.min(p.max), p.max.max(p.min));
            if hi > lo {
                v = lo + (v - lo).rem_euclid(hi - lo);
            }
        }
        let k = b
            .keys
            .iter()
            .position(|k| (k - v).abs() < p.snap_epsilon)
            .ok_or(FormError::NotOnKey)?;
        cell += k * stride;
        stride *= b.keys.len();
    }
    Ok(cell)
}

/// The current keyform's cell of object `o`.
pub fn current_cell(m: &Model, o: ObjectRef, values: &[f32]) -> Result<usize, FormError> {
    let p = &m.parameters;
    match o {
        ObjectRef::Part(i) => cell_at(
            &m.parts.get(i).ok_or(FormError::Missing)?.grid.bindings,
            p,
            values,
        ),
        ObjectRef::ArtMesh(i) => cell_at(
            &m.art_meshes.get(i).ok_or(FormError::Missing)?.grid.bindings,
            p,
            values,
        ),
        ObjectRef::Glue(i) => cell_at(
            &m.glues.get(i).ok_or(FormError::Missing)?.grid.bindings,
            p,
            values,
        ),
        ObjectRef::Deformer(i) => match &m.deformers.get(i).ok_or(FormError::Missing)?.kind {
            DeformerKind::Warp(w) => cell_at(&w.grid.bindings, p, values),
            DeformerKind::Rotation(r) => cell_at(&r.grid.bindings, p, values),
        },
    }
}

/// The Inspector's keyform fields of an object (inspector-fields.md:
/// `ACDrawableForm`, `CPartForm`, `ACDeformerForm`, `CRotationDeformerForm`).
/// `draw_order` exists for art meshes and parts, `angle` for rotation
/// deformers; glue has none of these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormFields {
    pub draw_order: Option<f32>,
    pub opacity: f32,
    pub multiply: keyform_model::Color,
    pub screen: keyform_model::Color,
    pub angle: Option<f32>,
}

/// The fields of keyform `cell` of `o`.
pub fn form_fields(m: &Model, o: ObjectRef, cell: usize) -> Option<FormFields> {
    Some(match o {
        ObjectRef::ArtMesh(i) => {
            let f = m.art_meshes.get(i)?.grid.keyforms.get(cell)?;
            FormFields {
                draw_order: Some(f.draw_order),
                opacity: f.opacity,
                multiply: f.multiply,
                screen: f.screen,
                angle: None,
            }
        }
        ObjectRef::Part(i) => {
            let f = m.parts.get(i)?.grid.keyforms.get(cell)?;
            FormFields {
                draw_order: Some(f.draw_order),
                opacity: f.opacity,
                multiply: f.multiply,
                screen: f.screen,
                angle: None,
            }
        }
        ObjectRef::Deformer(i) => match &m.deformers.get(i)?.kind {
            DeformerKind::Warp(w) => {
                let f = w.grid.keyforms.get(cell)?;
                FormFields {
                    draw_order: None,
                    opacity: f.opacity,
                    multiply: f.multiply,
                    screen: f.screen,
                    angle: None,
                }
            }
            DeformerKind::Rotation(r) => {
                let f = r.grid.keyforms.get(cell)?;
                FormFields {
                    draw_order: None,
                    opacity: f.opacity,
                    multiply: f.multiply,
                    screen: f.screen,
                    angle: Some(f.angle),
                }
            }
        },
        ObjectRef::Glue(_) => return None,
    })
}

/// Writes the fields into keyform `cell` of `o` (draw order rounded to an
/// integer in 0…1000, opacity clamped to 0…1). Returns whether anything
/// changed.
pub fn set_form_fields(
    m: &mut Model,
    o: ObjectRef,
    cell: usize,
    v: &FormFields,
) -> Result<bool, FormError> {
    if form_fields(m, o, cell).ok_or(FormError::Missing)? == *v {
        return Ok(false);
    }
    let order = v.draw_order.map(|d| d.round().clamp(0.0, 1000.0));
    let opacity = v.opacity.clamp(0.0, 1.0);
    macro_rules! common {
        ($f:expr) => {{
            $f.opacity = opacity;
            $f.multiply = v.multiply;
            $f.screen = v.screen;
        }};
    }
    match o {
        ObjectRef::ArtMesh(i) => {
            let f = &mut Arc::make_mut(&mut m.art_meshes[i]).grid.keyforms[cell];
            common!(f);
            if let Some(d) = order {
                f.draw_order = d;
            }
        }
        ObjectRef::Part(i) => {
            let f = &mut Arc::make_mut(&mut m.parts[i]).grid.keyforms[cell];
            common!(f);
            if let Some(d) = order {
                f.draw_order = d;
            }
        }
        ObjectRef::Deformer(i) => match &mut Arc::make_mut(&mut m.deformers[i]).kind {
            DeformerKind::Warp(w) => common!(w.grid.keyforms[cell]),
            DeformerKind::Rotation(r) => {
                let f = &mut r.grid.keyforms[cell];
                common!(f);
                if let Some(a) = v.angle {
                    f.angle = a;
                }
            }
        },
        ObjectRef::Glue(_) => return Err(FormError::Missing),
    }
    Ok(true)
}

/// "Bulk Set Draw Order": every keyform of `o` gets draw order `d` (blend
/// shape targets are left alone). Returns whether anything changed.
/// spec: 50-ui/palettes.md §4
pub fn set_draw_order_all(m: &mut Model, o: ObjectRef, d: f32) -> bool {
    let d = d.round().clamp(0.0, 1000.0);
    match o {
        ObjectRef::ArtMesh(i) => {
            let Some(a) = m.art_meshes.get(i) else {
                return false;
            };
            if a.grid.keyforms.iter().all(|f| f.draw_order == d) {
                return false;
            }
            for f in &mut Arc::make_mut(&mut m.art_meshes[i]).grid.keyforms {
                f.draw_order = d;
            }
            true
        }
        ObjectRef::Part(i) => {
            let Some(p) = m.parts.get(i) else {
                return false;
            };
            if p.grid.keyforms.iter().all(|f| f.draw_order == d) {
                return false;
            }
            for f in &mut Arc::make_mut(&mut m.parts[i]).grid.keyforms {
                f.draw_order = d;
            }
            true
        }
        _ => false,
    }
}

/// The deformer whose map places `o` on the canvas (`None` = canvas root).
pub fn parent_deformer(m: &Model, o: ObjectRef) -> Option<usize> {
    match o {
        ObjectRef::ArtMesh(i) => m.art_meshes.get(i)?.target,
        ObjectRef::Deformer(i) => m.deformers.get(i)?.target,
        ObjectRef::Part(_) | ObjectRef::Glue(_) => None,
    }
}

/// Moves local points (x,y interleaved) by `delta` in canvas space through
/// `parent` (local → canvas). Only the points in `which` move (all if None).
fn move_points(
    points: &mut [f32],
    parent: Option<&DeformerMap>,
    delta: [f32; 2],
    which: Option<&[(usize, f32)]>,
) -> Result<(), FormError> {
    let n = points.len() / 2;
    let mut moved = points.to_vec();
    let mut each = |i: usize, w: f32| -> Result<(), FormError> {
        let d = [delta[0] * w, delta[1] * w];
        let p = [points[2 * i], points[2 * i + 1]];
        let q = match parent {
            None => [p[0] + d[0], p[1] + d[1]],
            Some(map) => {
                let c = map.map(p);
                map.inverse([c[0] + d[0], c[1] + d[1]])
                    .ok_or(FormError::NotInvertible)?
            }
        };
        moved[2 * i] = q[0];
        moved[2 * i + 1] = q[1];
        Ok(())
    };
    match which {
        Some(list) => {
            for &(i, w) in list.iter().filter(|(i, w)| *i < n && *w != 0.0) {
                each(i, w)?;
            }
        }
        None => {
            for i in 0..n {
                each(i, 1.0)?;
            }
        }
    }
    points.copy_from_slice(&moved);
    Ok(())
}

/// Moves object `o` (or the points `which` of it) by `delta` canvas pixels
/// in its keyform `cell`. `parent` is the parent deformer's current map.
/// Art meshes move their vertices, warp deformers their grid points,
/// rotation deformers their origin; parts and glue have no geometry.
/// Returns whether the model changed.
pub fn translate(
    m: &mut Model,
    o: ObjectRef,
    cell: usize,
    parent: Option<&DeformerMap>,
    delta: [f32; 2],
    which: Option<&[usize]>,
) -> Result<bool, FormError> {
    let list: Option<Vec<(usize, f32)>> = which.map(|w| w.iter().map(|&i| (i, 1.0)).collect());
    translate_weighted(m, o, cell, parent, delta, list.as_deref())
}

/// Like [`translate`], each listed point moving by its weight × `delta`
/// (soft selection; canvas-and-tools.md §4 "Soft Selection").
pub fn translate_weighted(
    m: &mut Model,
    o: ObjectRef,
    cell: usize,
    parent: Option<&DeformerMap>,
    delta: [f32; 2],
    which: Option<&[(usize, f32)]>,
) -> Result<bool, FormError> {
    if delta == [0.0, 0.0] {
        return Ok(false);
    }
    match o {
        ObjectRef::ArtMesh(i) => {
            let a = Arc::make_mut(m.art_meshes.get_mut(i).ok_or(FormError::Missing)?);
            let f = a.grid.keyforms.get_mut(cell).ok_or(FormError::Missing)?;
            move_points(&mut f.positions, parent, delta, which)?;
        }
        ObjectRef::Deformer(i) => {
            let d = Arc::make_mut(m.deformers.get_mut(i).ok_or(FormError::Missing)?);
            match &mut d.kind {
                DeformerKind::Warp(w) => {
                    let f = w.grid.keyforms.get_mut(cell).ok_or(FormError::Missing)?;
                    move_points(&mut f.positions, parent, delta, which)?;
                }
                DeformerKind::Rotation(r) => {
                    let f = r.grid.keyforms.get_mut(cell).ok_or(FormError::Missing)?;
                    let w = which
                        .and_then(|l| l.iter().find(|x| x.0 == 0))
                        .map_or(1.0, |x| x.1);
                    move_points(&mut f.origin, parent, delta, Some(&[(0, w)]))?;
                }
            }
        }
        ObjectRef::Part(_) | ObjectRef::Glue(_) => return Ok(false),
    }
    Ok(true)
}

/// Moves points of a warp deformer's Bezier net (edit levels 2 and 3) by
/// `delta` canvas pixels and regenerates the keyform's grid through the
/// net, keeping finer detail as residuals. `which` indexes
/// [`crate::warp_net::Net::points`]. spec: warp-deformer-editing.md §1–3
pub fn translate_net(
    m: &mut Model,
    o: ObjectRef,
    cell: usize,
    parent: Option<&DeformerMap>,
    delta: [f32; 2],
    which: &[usize],
    level: u8,
) -> Result<bool, FormError> {
    use crate::warp_net::{Grid, Located, Net};
    let ObjectRef::Deformer(i) = o else {
        return Ok(false);
    };
    let d = Arc::make_mut(m.deformers.get_mut(i).ok_or(FormError::Missing)?);
    let DeformerKind::Warp(w) = &mut d.kind else {
        return Ok(false);
    };
    let (nc, nr) = w.net_division(level);
    let (cols, rows) = (w.cols as usize, w.rows as usize);
    let f = w.grid.keyforms.get_mut(cell).ok_or(FormError::Missing)?;
    let g = Grid {
        cols,
        rows,
        points: &f.positions,
    };
    let net = Net::from_grid(&g, nc as usize, nr as usize);
    let loc = Located::new(&net, &g);
    let pts = net.points();
    let mut edited = net.clone();
    let mut moved = Vec::new();
    for &idx in which {
        let Some(&(k, dir)) = pts.get(idx) else {
            continue;
        };
        let p = net.point(k, dir);
        let q = match parent {
            None => [p[0] + delta[0], p[1] + delta[1]],
            Some(map) => {
                let c = map.map(p);
                map.inverse([c[0] + delta[0], c[1] + delta[1]])
                    .ok_or(FormError::NotInvertible)?
            }
        };
        moved.push((k, dir, q));
    }
    // Handles first: a moved anchor then carries its (already moved) handles.
    moved.sort_by_key(|m| m.1.is_none());
    for (k, dir, q) in moved {
        edited.set_point(k, dir, q);
    }
    f.positions = loc.regenerate(&edited);
    Ok(true)
}

/// Changes a rotation deformer's keyform angle by `delta` degrees; with
/// `snap`, the result is rounded to a multiple of `snap` degrees
/// (canvas-and-tools.md §3: Shift snaps, default 45°).
pub fn rotate(
    m: &mut Model,
    o: ObjectRef,
    cell: usize,
    delta: f32,
    snap: Option<f32>,
) -> Result<bool, FormError> {
    let ObjectRef::Deformer(i) = o else {
        return Ok(false);
    };
    let d = Arc::make_mut(m.deformers.get_mut(i).ok_or(FormError::Missing)?);
    let DeformerKind::Rotation(r) = &mut d.kind else {
        return Ok(false);
    };
    let f = r.grid.keyforms.get_mut(cell).ok_or(FormError::Missing)?;
    let mut a = f.angle + delta;
    if let Some(s) = snap.filter(|s| *s > 0.0) {
        a = (a / s).round() * s;
    }
    f.angle = a;
    Ok(true)
}

/// Multiplies a rotation deformer's keyform scale by `factor` (> 0).
pub fn scale(m: &mut Model, o: ObjectRef, cell: usize, factor: f32) -> Result<bool, FormError> {
    let ObjectRef::Deformer(i) = o else {
        return Ok(false);
    };
    if !(factor.is_finite() && factor > 0.0) {
        return Ok(false);
    }
    let d = Arc::make_mut(m.deformers.get_mut(i).ok_or(FormError::Missing)?);
    let DeformerKind::Rotation(r) = &mut d.kind else {
        return Ok(false);
    };
    let f = r.grid.keyforms.get_mut(cell).ok_or(FormError::Missing)?;
    f.scale *= factor;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_eval::{RotationMap, WarpMap};
    use keyform_model::{ArtMesh, ArtMeshForm, EditableMesh, KeyformGrid, ObjectInfo};

    fn mesh_model() -> Model {
        let a = ArtMesh {
            clip_missing: false,
            info: ObjectInfo::new("A"),
            target: None,
            positions: vec![0.0, 0.0, 1.0, 0.0],
            uvs: vec![0.0; 4],
            indices: Vec::new(),
            texture: None,
            clip: Vec::new(),
            invert_clip: false,
            culling: false,
            color_composition: Default::default(),
            alpha_composition: Default::default(),
            user_data: String::new(),
            point_uids: vec![0, 1],
            mesh: EditableMesh::default(),
            model_image: None,
            generator: Default::default(),
            grid: KeyformGrid {
                bindings: vec![Binding::new(0, vec![-1.0, 1.0])],
                keyforms: vec![
                    ArtMeshForm {
                        positions: vec![0.0, 0.0, 1.0, 0.0],
                        ..Default::default()
                    },
                    ArtMeshForm {
                        positions: vec![0.5, 0.5, 1.0, 0.5],
                        ..Default::default()
                    },
                ],
                guids: Vec::new(),
            },
            blend_shapes: Default::default(),
        };
        Model {
            parameters: vec![Arc::new(Parameter::new("X", -1.0, 1.0, 0.0))],
            art_meshes: vec![Arc::new(a)],
            ..Model::default()
        }
    }

    #[test]
    fn needs_a_key_and_moves_only_that_keyform() {
        let mut m = mesh_model();
        let o = ObjectRef::ArtMesh(0);
        assert_eq!(current_cell(&m, o, &[0.0]), Err(FormError::NotOnKey));
        let cell = current_cell(&m, o, &[1.0]).unwrap();
        assert_eq!(cell, 1);
        // Under a rotation by 90°, scale 2: canvas +x is local −y / 2.
        let rot = DeformerMap::Rotation(RotationMap {
            origin: [10.0, 10.0],
            angle: 90.0,
            scale: 2.0,
            reflect_x: false,
            reflect_y: false,
        });
        assert!(translate(&mut m, o, cell, Some(&rot), [4.0, 0.0], Some(&[1])).unwrap());
        let f = &m.art_meshes[0].grid.keyforms;
        assert_eq!(
            f[0].positions,
            vec![0.0, 0.0, 1.0, 0.0],
            "other keyform untouched"
        );
        assert_eq!(
            &f[1].positions[..2],
            &[0.5, 0.5],
            "unselected point untouched"
        );
        assert!((f[1].positions[2] - 1.0).abs() < 1e-5);
        assert!(
            (f[1].positions[3] - (0.5 - 2.0)).abs() < 1e-5,
            "{:?}",
            f[1].positions
        );
    }

    #[test]
    fn moves_through_a_warp() {
        let mut m = mesh_model();
        let o = ObjectRef::ArtMesh(0);
        let w = DeformerMap::Warp(WarpMap {
            cols: 1,
            rows: 1,
            quad: false,
            points: vec![0.0, 0.0, 100.0, 0.0, 0.0, 100.0, 100.0, 100.0],
        });
        translate(&mut m, o, 1, Some(&w), [10.0, -20.0], None).unwrap();
        let p = &m.art_meshes[0].grid.keyforms[1].positions;
        assert!(
            (p[0] - 0.6).abs() < 1e-5 && (p[1] - 0.3).abs() < 1e-5,
            "{p:?}"
        );
    }
}
