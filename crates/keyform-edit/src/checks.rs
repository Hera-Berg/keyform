//! Model checks shown to the user.
//!
//! "Issues on Mask Settings": problems with art meshes used as clipping
//! masks. spec: 50-ui/dialogs.md §21

use std::collections::{HashMap, HashSet};

use keyform_model::{Binding, DeformerKind, Model, ObjectInfo, ParamIndex};

use crate::keys::ObjectRef;

/// A binding whose keys don't reach its parameter's minimum and/or maximum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenBinding {
    pub parameter: ParamIndex,
    /// No key at (or below) the minimum.
    pub min: bool,
    /// No key at (or above) the maximum.
    pub max: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaskIssues {
    /// Objects whose clip list names a deleted drawable.
    pub deleted: Vec<ObjectRef>,
    /// Hidden masks, their hidden ancestor parts and guide-image ancestors.
    pub hidden: Vec<ObjectRef>,
    /// Masks and their ancestors that can go outside, with the bindings.
    pub outside: Vec<(ObjectRef, Vec<OpenBinding>)>,
}

/// Fixed texts of the dialog (dialogs.md §21).
pub const MASK_HINT: &str = "Issues on mask settings found (Click for details)";
pub const MASK_TITLE: &str = "Issues on Mask Settings";
pub const MASK_INTRO: [&str; 2] = [
    "A problem was found with the settings of an ArtMesh used as a mask.",
    "Exporting in this state may cause the following issues in applications using the SDK.",
];
pub const MASK_CONSEQUENCES: [&str; 3] = [
    "Application terminates due to an error",
    "Runtime files cannot be loaded",
    "Model is shown differently from the editor",
];
pub const MASK_MANUAL: &str = "Please refer to the \"Official Manual\" for details.";

/// One list of the dialog: its message and its lines.
pub struct IssueList {
    pub message: &'static str,
    /// "<ID> (<kind>)", with the bindings indented below it.
    pub items: Vec<(String, Vec<String>)>,
}

fn label(m: &Model, o: ObjectRef) -> String {
    let (id, kind) = match o {
        ObjectRef::ArtMesh(i) => (m.art_meshes.get(i).map(|x| &x.info.id), "ArtMesh"),
        ObjectRef::Deformer(i) => (m.deformers.get(i).map(|x| &x.info.id), "Deformer"),
        ObjectRef::Part(i) => (m.parts.get(i).map(|x| &x.info.id), "Part"),
        ObjectRef::Glue(i) => (m.glues.get(i).map(|x| &x.info.id), "Glue"),
    };
    format!("{} ({kind})", id.map_or("?", |s| s.as_str()))
}

impl MaskIssues {
    pub fn is_empty(&self) -> bool {
        self.deleted.is_empty() && self.hidden.is_empty() && self.outside.is_empty()
    }

    /// The non-empty lists with their texts, as the dialog shows them.
    pub fn describe(&self, m: &Model) -> Vec<IssueList> {
        let mut out = Vec::new();
        if !self.deleted.is_empty() {
            out.push(IssueList {
                message: "Referencing a deleted mask. Fix the clipping field.",
                items: self
                    .deleted
                    .iter()
                    .map(|&o| (label(m, o), Vec::new()))
                    .collect(),
            });
        }
        if !self.hidden.is_empty() {
            out.push(IssueList {
                message: "Check the visibility state and guide image settings of the ArtMesh used as a mask.",
                items: self.hidden.iter().map(|&o| (label(m, o), Vec::new())).collect(),
            });
        }
        if !self.outside.is_empty() {
            out.push(IssueList {
                message: "Add keyforms to the maximum and minimum values of the target parameter.",
                items: self
                    .outside
                    .iter()
                    .map(|(o, bindings)| {
                        let lines = bindings
                            .iter()
                            .map(|b| {
                                let name = m
                                    .parameters
                                    .get(b.parameter)
                                    .map_or("?", |p| p.name.as_str());
                                format!(
                                    "{name} / {}{}",
                                    if b.min { "[Minimum Value]" } else { "" },
                                    if b.max { "[Maximum Value]" } else { "" }
                                )
                            })
                            .collect();
                        (label(m, *o), lines)
                    })
                    .collect(),
            });
        }
        out
    }
}

fn push_unique<T: PartialEq>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) {
        v.push(x);
    }
}

struct Ctx<'a> {
    m: &'a Model,
    hier: HashMap<ObjectRef, bool>,
}

impl Ctx<'_> {
    fn info(&self, o: ObjectRef) -> Option<&ObjectInfo> {
        Some(match o {
            ObjectRef::Part(i) => &self.m.parts.get(i)?.info,
            ObjectRef::Deformer(i) => &self.m.deformers.get(i)?.info,
            ObjectRef::ArtMesh(i) => &self.m.art_meshes.get(i)?.info,
            ObjectRef::Glue(i) => &self.m.glues.get(i)?.info,
        })
    }

    /// Ancestor parts, nearest first.
    fn ancestors(&self, o: ObjectRef) -> Vec<usize> {
        let mut out = Vec::new();
        let mut cur = self.info(o).and_then(|i| i.parent_part);
        while let Some(p) = cur {
            if out.contains(&p) || out.len() > self.m.parts.len() {
                break;
            }
            out.push(p);
            cur = self.m.parts.get(p).and_then(|x| x.info.parent_part);
        }
        out
    }

    fn visible_in_hierarchy(&self, o: ObjectRef) -> bool {
        self.info(o).is_some_and(|i| i.visible)
            && self
                .ancestors(o)
                .iter()
                .all(|&p| self.m.parts[p].info.visible)
    }

    fn bindings(&self, o: ObjectRef) -> &[Binding] {
        let m = self.m;
        match o {
            ObjectRef::Part(i) => m.parts.get(i).map(|x| &x.grid.bindings[..]),
            ObjectRef::ArtMesh(i) => m.art_meshes.get(i).map(|x| &x.grid.bindings[..]),
            ObjectRef::Glue(i) => m.glues.get(i).map(|x| &x.grid.bindings[..]),
            ObjectRef::Deformer(i) => m.deformers.get(i).map(|d| match &d.kind {
                DeformerKind::Warp(w) => &w.grid.bindings[..],
                DeformerKind::Rotation(r) => &r.grid.bindings[..],
            }),
        }
        .unwrap_or(&[])
    }

    /// Bindings that can go outside: not repeat, and no keys or the keys
    /// don't reach min or max.
    fn open_bindings(&self, o: ObjectRef) -> Vec<OpenBinding> {
        let mut out = Vec::new();
        for b in self.bindings(o) {
            let Some(p) = self.m.parameters.get(b.parameter) else {
                continue;
            };
            if p.repeat {
                continue;
            }
            let lo = b.keys.iter().copied().reduce(f32::min);
            let hi = b.keys.iter().copied().reduce(f32::max);
            let min = lo.is_none_or(|k| k > p.min);
            let max = hi.is_none_or(|k| k < p.max);
            if min || max {
                out.push(OpenBinding {
                    parameter: b.parameter,
                    min,
                    max,
                });
            }
        }
        out
    }

    fn up(&self, o: ObjectRef) -> Vec<ObjectRef> {
        let mut out = Vec::new();
        let target = match o {
            ObjectRef::ArtMesh(i) => self.m.art_meshes.get(i).and_then(|x| x.target),
            ObjectRef::Deformer(i) => self.m.deformers.get(i).and_then(|x| x.target),
            _ => None,
        };
        out.extend(target.map(ObjectRef::Deformer));
        out.extend(
            self.info(o)
                .and_then(|i| i.parent_part)
                .map(ObjectRef::Part),
        );
        out
    }

    /// Can go outside itself, or through its parent part or target deformer.
    fn can_go_outside_in_hierarchy(&mut self, o: ObjectRef, depth: usize) -> bool {
        if let Some(&v) = self.hier.get(&o) {
            return v;
        }
        // Provisional value guards against cycles in malformed input.
        self.hier.insert(o, false);
        let v = !self.open_bindings(o).is_empty()
            || (depth < 256
                && self
                    .up(o)
                    .into_iter()
                    .any(|p| self.can_go_outside_in_hierarchy(p, depth + 1)));
        self.hier.insert(o, v);
        v
    }
}

/// Runs the mask checks of dialogs.md §21.
pub fn mask_issues(m: &Model) -> MaskIssues {
    let mut c = Ctx {
        m,
        hier: HashMap::new(),
    };
    let mut issues = MaskIssues::default();

    // The mask set (in order of first use).
    let mut masks: Vec<usize> = Vec::new();
    for (i, a) in m.art_meshes.iter().enumerate() {
        if c.visible_in_hierarchy(ObjectRef::ArtMesh(i)) {
            for &k in &a.clip {
                push_unique(&mut masks, k);
            }
        }
    }
    for (i, p) in m.parts.iter().enumerate() {
        if p.offscreen && c.visible_in_hierarchy(ObjectRef::Part(i)) {
            for &k in &p.clip {
                push_unique(&mut masks, k);
            }
        }
    }

    // 1. Deleted masks.
    for (i, a) in m.art_meshes.iter().enumerate() {
        if a.clip_missing {
            push_unique(&mut issues.deleted, ObjectRef::ArtMesh(i));
        }
    }
    for (i, p) in m.parts.iter().enumerate() {
        if p.offscreen && p.clip_missing {
            push_unique(&mut issues.deleted, ObjectRef::Part(i));
        }
    }

    // 2. Hidden or guide-image masks.
    for &k in &masks {
        let o = ObjectRef::ArtMesh(k);
        let ancestors = c.ancestors(o);
        if !c.visible_in_hierarchy(o) {
            if c.info(o).is_some_and(|i| !i.visible) {
                push_unique(&mut issues.hidden, o);
            }
            for &p in &ancestors {
                if !m.parts[p].info.visible {
                    push_unique(&mut issues.hidden, ObjectRef::Part(p));
                }
            }
        }
        for &p in &ancestors {
            if m.parts[p].sketch {
                push_unique(&mut issues.hidden, ObjectRef::Part(p));
            }
        }
    }

    // 3. Masks that can go outside, walking up.
    let mut seen: HashSet<ObjectRef> = HashSet::new();
    let mut stack: Vec<ObjectRef> = masks.iter().rev().map(|&k| ObjectRef::ArtMesh(k)).collect();
    while let Some(o) = stack.pop() {
        if !seen.insert(o) {
            continue;
        }
        if c.can_go_outside_in_hierarchy(o, 0) {
            let open = c.open_bindings(o);
            if !open.is_empty() {
                issues.outside.push((o, open));
            }
            for p in c.up(o).into_iter().rev() {
                stack.push(p);
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::*;
    use std::sync::Arc;

    fn mesh(id: &str, clip: Vec<usize>, keys: Option<Vec<f32>>) -> ArtMesh {
        let mut grid = KeyformGrid::single(ArtMeshForm::default());
        if let Some(k) = keys {
            grid.bindings.push(Binding::new(0, k.clone()));
            grid.keyforms = vec![ArtMeshForm::default(); k.len()];
        }
        ArtMesh {
            info: ObjectInfo::new(id),
            target: None,
            positions: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
            texture: None,
            clip,
            invert_clip: false,
            clip_missing: false,
            culling: false,
            color_composition: Default::default(),
            alpha_composition: Default::default(),
            user_data: String::new(),
            point_uids: Vec::new(),
            mesh: EditableMesh::default(),
            grid,
            blend_shapes: Default::default(),
        }
    }

    #[test]
    fn detects_open_keys_hidden_masks_and_deleted_references() {
        let mut m = Model {
            parameters: vec![Arc::new(Parameter::new("ParamAngleX", -30.0, 40.0, 0.0))],
            ..Model::default()
        };
        // 0 = mask (keys −30…30, max 40 not reached), 1 = user of the mask.
        m.art_meshes
            .push(Arc::new(mesh("Mask", vec![], Some(vec![-30.0, 30.0]))));
        m.art_meshes.push(Arc::new(mesh("Clipped", vec![0], None)));
        let i = mask_issues(&m);
        assert_eq!(
            i.outside,
            vec![(
                ObjectRef::ArtMesh(0),
                vec![OpenBinding {
                    parameter: 0,
                    min: false,
                    max: true
                }]
            )]
        );
        assert!(i.hidden.is_empty() && i.deleted.is_empty());

        // Keys cover the range: no issue.
        Arc::make_mut(&mut m.art_meshes[0]).grid.bindings[0].keys = vec![-30.0, 40.0];
        assert!(mask_issues(&m).is_empty());

        // A hidden mask, and a deleted reference elsewhere.
        Arc::make_mut(&mut m.art_meshes[0]).info.visible = false;
        Arc::make_mut(&mut m.art_meshes[1]).clip_missing = true;
        let i = mask_issues(&m);
        assert_eq!(i.hidden, vec![ObjectRef::ArtMesh(0)]);
        assert_eq!(i.deleted, vec![ObjectRef::ArtMesh(1)]);
    }
}
