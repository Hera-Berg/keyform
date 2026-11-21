//! Edit ▸ Cut / Copy / Paste of objects (art meshes and deformers) and
//! Edit ▸ Copy / Paste Parameter Value.
//!
//! spec: 50-ui/menus.md (Edit) names the commands; their behaviour was
//! observed in the reference editor (5.3.04, black box, 2026-11-21):
//! - a pasted object keeps its name, part, parent deformer, keyforms and
//!   draw order; it gets a fresh ID (the copied ID without trailing digits,
//!   plus the smallest free number from 2: BrowR → BrowR2, Warp0 … Warp5 →
//!   Warp6) and fresh GUIDs;
//! - it goes into the Parts palette directly above the selected object
//!   (in that object's part), or at the top of the root part when nothing
//!   is selected; the pasted objects become the selection;
//! - a deformer is copied alone (its children stay with the original);
//! - Cut is Copy then Delete; a pasted ID that is free again is reused;
//! - Copy Parameter Value copies every parameter's current value, Paste
//!   Parameter Value sets them all back.
//!
//! TODO(Q-0050): observed behaviour, not in the spec; parts, glue and art
//! paths aren't copied, and pasting into another document is refused.

use std::sync::Arc;

use keyform_model::{
    new_guid, ArtMesh, Deformer, DeformerIndex, Guid, Model, PartChild, PartIndex,
};

use crate::keys::ObjectRef;

/// One copied object, with what it refers to by identity rather than by
/// index (indices change when objects are deleted between copy and paste).
#[derive(Clone, Debug)]
pub enum CopiedObject {
    ArtMesh {
        mesh: Arc<ArtMesh>,
        parent: Option<Guid>,
        clips: Vec<Guid>,
    },
    Deformer {
        deformer: Arc<Deformer>,
        parent: Option<Guid>,
    },
}

/// What Edit ▸ Copy put on the clipboard.
#[derive(Clone, Debug)]
pub struct ObjectClipboard {
    /// The model the objects came from (paste is refused elsewhere).
    pub model: Guid,
    /// Parameter IDs at copy time, to re-map keyform bindings.
    pub parameters: Vec<String>,
    pub objects: Vec<CopiedObject>,
}

impl ObjectClipboard {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

/// Copies the art meshes and deformers among `objects` (others are
/// skipped). Returns `None` when there is nothing to copy.
pub fn copy_objects(m: &Model, objects: &[ObjectRef]) -> Option<ObjectClipboard> {
    let guid_of = |d: Option<DeformerIndex>| {
        d.and_then(|d| m.deformers.get(d))
            .map(|x| x.info.guid.clone())
    };
    let mut out = Vec::new();
    for &o in objects {
        match o {
            ObjectRef::ArtMesh(i) => {
                if let Some(a) = m.art_meshes.get(i) {
                    out.push(CopiedObject::ArtMesh {
                        mesh: a.clone(),
                        parent: guid_of(a.target),
                        clips: a
                            .clip
                            .iter()
                            .filter_map(|&c| m.art_meshes.get(c))
                            .map(|c| c.info.guid.clone())
                            .collect(),
                    });
                }
            }
            ObjectRef::Deformer(i) => {
                if let Some(d) = m.deformers.get(i) {
                    out.push(CopiedObject::Deformer {
                        deformer: d.clone(),
                        parent: guid_of(d.target),
                    });
                }
            }
            ObjectRef::Part(_) | ObjectRef::Glue(_) => {}
        }
    }
    (!out.is_empty()).then(|| ObjectClipboard {
        model: m.guid.clone(),
        parameters: m.parameters.iter().map(|p| p.id.clone()).collect(),
        objects: out,
    })
}

/// The ID for a copy of `id`: `id` without trailing digits plus the
/// smallest free number from 2.
pub fn copy_id<'a>(id: &str, taken: impl Iterator<Item = &'a str>) -> String {
    let base = id.trim_end_matches(|c: char| c.is_ascii_digit());
    let base = if base.is_empty() { id } else { base };
    let used: std::collections::HashSet<&str> = taken.collect();
    (2u64..)
        .map(|n| format!("{base}{n}"))
        .find(|c| !used.contains(c.as_str()))
        .unwrap_or_else(|| format!("{base}2"))
}

/// Pastes `clip` into `m`: above `above` in its part, or at the top of the
/// root part. Returns the new objects in clipboard order.
pub fn paste_objects(
    m: &mut Model,
    clip: &ObjectClipboard,
    above: Option<ObjectRef>,
) -> Result<Vec<ObjectRef>, String> {
    if clip.model != m.guid {
        return Err("Objects can only be pasted into the model they were copied from.".into());
    }
    let remap = param_remap(m, &clip.parameters)?;
    let (part, before) = destination(m, above);
    let deformer_by_guid = |m: &Model, g: &Option<Guid>| -> Result<Option<DeformerIndex>, String> {
        match g {
            None => Ok(None),
            Some(g) => m
                .deformers
                .iter()
                .position(|d| d.info.guid == *g)
                .map(Some)
                .ok_or_else(|| "The parent deformer of the copied object no longer exists.".into()),
        }
    };
    let mut made = Vec::new();
    for c in &clip.objects {
        let (obj, child) = match c {
            CopiedObject::ArtMesh {
                mesh,
                parent,
                clips,
            } => {
                let target = deformer_by_guid(m, parent)?;
                let mut a = (**mesh).clone();
                if a.texture.is_some_and(|t| t >= m.textures.len())
                    || a.model_image.is_some_and(|t| t >= m.model_images.len())
                {
                    return Err("The copied ArtMesh's image no longer exists.".into());
                }
                a.info.id = copy_id(&a.info.id, m.art_meshes.iter().map(|x| x.info.id.as_str()));
                a.info.guid = new_guid();
                a.info.parent_part = part;
                a.target = target;
                a.clip = clips
                    .iter()
                    .filter_map(|g| m.art_meshes.iter().position(|x| x.info.guid == *g))
                    .collect();
                remap_grid(&mut a.grid.bindings, &remap)?;
                if remap.is_some() && !a.blend_shapes.shapes.is_empty() {
                    return Err(PARAMS_CHANGED.into());
                }
                a.grid.guids = a.grid.guids.iter().map(|_| new_guid()).collect();
                let i = m.art_meshes.len();
                m.art_meshes.push(Arc::new(a));
                (ObjectRef::ArtMesh(i), PartChild::ArtMesh(i))
            }
            CopiedObject::Deformer { deformer, parent } => {
                let target = deformer_by_guid(m, parent)?;
                let mut d = (**deformer).clone();
                d.info.id = copy_id(&d.info.id, m.deformers.iter().map(|x| x.info.id.as_str()));
                d.info.guid = new_guid();
                d.info.parent_part = part;
                d.target = target;
                match &mut d.kind {
                    keyform_model::DeformerKind::Warp(w) => {
                        remap_grid(&mut w.grid.bindings, &remap)?;
                        if remap.is_some() && !w.blend_shapes.shapes.is_empty() {
                            return Err(PARAMS_CHANGED.into());
                        }
                        w.grid.guids = w.grid.guids.iter().map(|_| new_guid()).collect();
                    }
                    keyform_model::DeformerKind::Rotation(r) => {
                        remap_grid(&mut r.grid.bindings, &remap)?;
                        if remap.is_some() && !r.blend_shapes.shapes.is_empty() {
                            return Err(PARAMS_CHANGED.into());
                        }
                        r.grid.guids = r.grid.guids.iter().map(|_| new_guid()).collect();
                    }
                }
                let i = m.deformers.len();
                m.deformers.push(Arc::new(d));
                (ObjectRef::Deformer(i), PartChild::Deformer(i))
            }
        };
        if let Some(p) = part {
            let p = Arc::make_mut(&mut m.parts[p]);
            let at = before
                .and_then(|b| p.children.iter().position(|c| *c == b))
                .unwrap_or(0);
            p.children.insert(at, child);
        }
        made.push(obj);
    }
    Ok(made)
}

/// "Add a number when pasting objects": each pasted object's name gets the
/// smallest free number from 2 among the names of its kind.
pub fn number_names(m: &mut Model, made: &[ObjectRef]) {
    for &o in made {
        match o {
            ObjectRef::ArtMesh(i) => {
                let name = m.art_meshes[i].info.display_name().to_owned();
                let n = copy_id(&name, m.art_meshes.iter().map(|x| x.info.display_name()));
                Arc::make_mut(&mut m.art_meshes[i]).info.name = n;
            }
            ObjectRef::Deformer(i) => {
                let name = m.deformers[i].info.display_name().to_owned();
                let n = copy_id(&name, m.deformers.iter().map(|x| x.info.display_name()));
                Arc::make_mut(&mut m.deformers[i]).info.name = n;
            }
            _ => {}
        }
    }
}

const PARAMS_CHANGED: &str =
    "The parameters changed since the objects were copied (blend shapes can't be re-mapped).";

/// Where pasted objects go: the part of `above` (before it), or the top of
/// the root part.
fn destination(m: &Model, above: Option<ObjectRef>) -> (Option<PartIndex>, Option<PartChild>) {
    let found = above.and_then(|o| {
        let (part, child) = match o {
            ObjectRef::ArtMesh(i) => (m.art_meshes.get(i)?.info.parent_part, PartChild::ArtMesh(i)),
            ObjectRef::Deformer(i) => {
                (m.deformers.get(i)?.info.parent_part, PartChild::Deformer(i))
            }
            ObjectRef::Glue(i) => (m.glues.get(i)?.info.parent_part, PartChild::Glue(i)),
            ObjectRef::Part(i) => (m.parts.get(i)?.info.parent_part, PartChild::Part(i)),
        };
        Some((part?, child))
    });
    match found {
        Some((p, c)) => (Some(p), Some(c)),
        None => (m.root_part, None),
    }
}

/// Old → new parameter index when the parameter list changed since copying.
fn param_remap(m: &Model, copied: &[String]) -> Result<Option<Vec<Option<usize>>>, String> {
    if copied.len() == m.parameters.len()
        && copied.iter().zip(&m.parameters).all(|(a, b)| *a == b.id)
    {
        return Ok(None);
    }
    Ok(Some(
        copied
            .iter()
            .map(|id| m.parameters.iter().position(|p| p.id == *id))
            .collect(),
    ))
}

fn remap_grid(
    bindings: &mut [keyform_model::grid::Binding],
    remap: &Option<Vec<Option<usize>>>,
) -> Result<(), String> {
    let Some(r) = remap else { return Ok(()) };
    for b in bindings {
        b.parameter = r
            .get(b.parameter)
            .copied()
            .flatten()
            .ok_or("A parameter of the copied object no longer exists.")?;
    }
    Ok(())
}

/// Edit ▸ Copy Parameter Value: every parameter's value, by ID.
pub fn copy_parameter_values(m: &Model, values: &[f32]) -> Vec<(String, f32)> {
    m.parameters
        .iter()
        .zip(values)
        .map(|(p, &v)| (p.id.clone(), v))
        .collect()
}

/// Edit ▸ Paste Parameter Value: sets every parameter found by ID (clamped
/// to its range); returns how many were set.
pub fn paste_parameter_values(m: &Model, values: &mut [f32], copied: &[(String, f32)]) -> usize {
    let mut n = 0;
    for (id, v) in copied {
        if let Some(i) = m.parameters.iter().position(|p| p.id == *id) {
            if let Some(slot) = values.get_mut(i) {
                let p = &m.parameters[i];
                *slot = v.clamp(p.min.min(p.max), p.max.max(p.min));
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::{
        ArtMeshForm, Binding, EditableMesh, KeyformGrid, ObjectInfo, Parameter, Part, PartForm,
    };

    /// Root part with meshes A, B (in that order) bound to parameter X.
    fn model() -> Model {
        let mesh = |id: &str| ArtMesh {
            clip_missing: false,
            info: ObjectInfo {
                guid: new_guid(),
                name: format!("{id} name"),
                parent_part: Some(0),
                ..ObjectInfo::new(id)
            },
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
                keyforms: vec![ArtMeshForm::default(), ArtMeshForm::default()],
                guids: vec![new_guid(), new_guid()],
            },
            blend_shapes: Default::default(),
            paths: Vec::new(),
        };
        let mut part = Part::new("Root");
        part.grid = KeyformGrid::single(PartForm::default());
        part.children = vec![PartChild::ArtMesh(0), PartChild::ArtMesh(1)];
        Model {
            guid: new_guid(),
            parameters: vec![
                Arc::new(Parameter::new("X", -1.0, 1.0, 0.0)),
                Arc::new(Parameter::new("Y", 0.0, 10.0, 0.0)),
            ],
            art_meshes: vec![Arc::new(mesh("BrowR")), Arc::new(mesh("BrowL"))],
            parts: vec![Arc::new(part)],
            root_part: Some(0),
            ..Model::default()
        }
    }

    #[test]
    fn paste_goes_above_the_selection_or_to_the_top() {
        let mut m = model();
        let clip = copy_objects(&m, &[ObjectRef::ArtMesh(0)]).unwrap();
        // Above the selected BrowL.
        let made = paste_objects(&mut m, &clip, Some(ObjectRef::ArtMesh(1))).unwrap();
        assert_eq!(made, vec![ObjectRef::ArtMesh(2)]);
        let a = &m.art_meshes[2];
        assert_eq!(
            (a.info.id.as_str(), a.info.name.as_str()),
            ("BrowR2", "BrowR name")
        );
        assert_ne!(a.info.guid, m.art_meshes[0].info.guid);
        assert_eq!(a.grid.keyforms.len(), 2, "keyforms come along");
        assert!(a
            .grid
            .guids
            .iter()
            .all(|g| !m.art_meshes[0].grid.guids.contains(g)));
        assert_eq!(
            m.parts[0].children,
            vec![
                PartChild::ArtMesh(0),
                PartChild::ArtMesh(2),
                PartChild::ArtMesh(1)
            ]
        );
        // Nothing selected: top of the root part, next free ID.
        paste_objects(&mut m, &clip, None).unwrap();
        assert_eq!(m.parts[0].children[0], PartChild::ArtMesh(3));
        assert_eq!(m.art_meshes[3].info.id, "BrowR3");
    }

    #[test]
    fn paste_follows_parameter_changes_and_stays_in_its_model() {
        let mut m = model();
        let clip = copy_objects(&m, &[ObjectRef::ArtMesh(0)]).unwrap();
        // A parameter inserted before X: the binding follows X.
        m.parameters
            .insert(0, Arc::new(Parameter::new("Z", 0.0, 1.0, 0.0)));
        let made = paste_objects(&mut m, &clip, None).unwrap();
        let ObjectRef::ArtMesh(i) = made[0] else {
            panic!()
        };
        assert_eq!(m.art_meshes[i].grid.bindings[0].parameter, 1);
        let mut other = model();
        assert!(paste_objects(&mut other, &clip, None).is_err());
    }

    #[test]
    fn parameter_values_round_trip_by_id() {
        let m = model();
        let copied = copy_parameter_values(&m, &[0.5, 7.0]);
        let mut values = vec![-1.0, 0.0];
        assert_eq!(paste_parameter_values(&m, &mut values, &copied), 2);
        assert_eq!(values, vec![0.5, 7.0]);
        let mut values = vec![0.0, 0.0];
        paste_parameter_values(&m, &mut values, &[("Y".into(), 99.0)]);
        assert_eq!(values, vec![0.0, 10.0], "clamped to the range");
    }

    #[test]
    fn copy_ids_follow_the_reference() {
        let ids = ["BrowR", "BrowL"];
        assert_eq!(copy_id("BrowR", ids.iter().copied()), "BrowR2");
        let ids = ["BrowR", "BrowR2"];
        assert_eq!(copy_id("BrowR", ids.iter().copied()), "BrowR3");
        assert_eq!(copy_id("BrowR2", ids.iter().copied()), "BrowR3");
        let warps = ["Warp0", "Warp1", "Warp2", "Warp3", "Warp4", "Warp5"];
        assert_eq!(copy_id("Warp0", warps.iter().copied()), "Warp6");
        assert_eq!(copy_id("7", [].into_iter()), "72");
    }
}
