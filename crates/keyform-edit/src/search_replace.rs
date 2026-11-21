//! Edit ▸ Search and Replace (names and IDs). spec: 50-ui/dialogs.md §10;
//! the dialog's layout follows the reference (Condition with Name / ID /
//! case switches, type toggles, a Parent Part / Type / Name / ID table, and
//! "Replace the Name/ID for the selected items in the table").

use std::collections::HashSet;
use std::sync::Arc;

use keyform_model::{DeformerKind, Model};

use crate::keys::ObjectRef;

/// What a row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Part,
    ArtMesh,
    ArtPath,
    Warp,
    Rotation,
    Glue,
    Parameter,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Part,
        Kind::ArtMesh,
        Kind::ArtPath,
        Kind::Warp,
        Kind::Rotation,
        Kind::Glue,
        Kind::Parameter,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Part => "Part",
            Kind::ArtMesh => "ArtMesh",
            Kind::ArtPath => "ArtPath",
            Kind::Warp => "Warp Deformer",
            Kind::Rotation => "Rotation Deformer",
            Kind::Glue => "Glue",
            Kind::Parameter => "Parameter",
        }
    }
}

/// One row of the table.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub kind: Kind,
    pub index: usize,
    pub parent_part: String,
    pub name: String,
    pub id: String,
}

impl Item {
    /// The canvas object of the row (parameters have none).
    pub fn object(&self) -> Option<ObjectRef> {
        Some(match self.kind {
            Kind::Part => ObjectRef::Part(self.index),
            Kind::ArtMesh => ObjectRef::ArtMesh(self.index),
            Kind::Warp | Kind::Rotation => ObjectRef::Deformer(self.index),
            Kind::Glue => ObjectRef::Glue(self.index),
            Kind::ArtPath | Kind::Parameter => return None,
        })
    }
}

/// Every searchable object (in palette order) and then every parameter.
pub fn items(m: &Model) -> Vec<Item> {
    let part_name = |p: Option<usize>| {
        p.and_then(|p| {
            let x = m.parts.get(p)?;
            Some(if Some(p) == m.root_part {
                "Root Part".to_owned()
            } else {
                x.info.display_name().to_owned()
            })
        })
        .unwrap_or_default()
    };
    let mut out = Vec::new();
    for o in crate::clipboard::palette_order(m) {
        let (kind, index, info) = match o {
            ObjectRef::Part(i) => {
                if Some(i) == m.root_part {
                    continue;
                }
                (Kind::Part, i, &m.parts[i].info)
            }
            ObjectRef::ArtMesh(i) => (Kind::ArtMesh, i, &m.art_meshes[i].info),
            ObjectRef::Glue(i) => (Kind::Glue, i, &m.glues[i].info),
            ObjectRef::Deformer(i) => (
                match m.deformers[i].kind {
                    DeformerKind::Warp(_) => Kind::Warp,
                    DeformerKind::Rotation(_) => Kind::Rotation,
                },
                i,
                &m.deformers[i].info,
            ),
        };
        out.push(Item {
            kind,
            index,
            parent_part: part_name(info.parent_part),
            name: info.name.clone(),
            id: info.id.clone(),
        });
    }
    for (i, a) in m.art_paths.iter().enumerate() {
        out.push(Item {
            kind: Kind::ArtPath,
            index: i,
            parent_part: part_name(a.info.parent_part),
            name: a.info.name.clone(),
            id: a.info.id.clone(),
        });
    }
    for (i, p) in m.parameters.iter().enumerate() {
        out.push(Item {
            kind: Kind::Parameter,
            index: i,
            parent_part: String::new(),
            name: p.name.clone(),
            id: p.id.clone(),
        });
    }
    out
}

/// Whether `text` contains `cond` (case-sensitive or not).
fn contains(text: &str, cond: &str, case: bool) -> bool {
    if case {
        text.contains(cond)
    } else {
        text.to_lowercase().contains(&cond.to_lowercase())
    }
}

/// The Condition filter: the name and / or the ID contain `cond`.
pub fn matches(item: &Item, cond: &str, name: bool, id: bool, case: bool) -> bool {
    cond.is_empty()
        || (name && contains(&item.name, cond, case))
        || (id && contains(&item.id, cond, case))
}

/// Replaces every occurrence of `before` in `text` with `after`.
pub fn replace_in(text: &str, before: &str, after: &str, case: bool) -> String {
    if before.is_empty() {
        return text.to_owned();
    }
    if case {
        return text.replace(before, after);
    }
    let lower = text.to_lowercase();
    let pat = before.to_lowercase();
    let mut out = String::new();
    let mut i = 0;
    while let Some(k) = lower[i..].find(&pat) {
        out.push_str(&text[i..i + k]);
        out.push_str(after);
        i += k + pat.len();
    }
    out.push_str(&text[i..]);
    out
}

/// Replaces `before` with `after` in the names (or IDs) of `rows`. All or
/// nothing: an empty name, an invalid ID or a duplicate ID refuses the
/// whole replacement. Returns how many changed ("%d are replaced").
pub fn replace(
    m: &mut Model,
    rows: &[Item],
    before: &str,
    after: &str,
    target_id: bool,
    case: bool,
) -> Result<usize, String> {
    let mut changes = Vec::new();
    for r in rows {
        let old = if target_id { &r.id } else { &r.name };
        let new = replace_in(old, before, after, case);
        if new == *old {
            continue;
        }
        if target_id {
            if !crate::valid_id(&new) {
                return Err(format!("Invalid ID: {new}"));
            }
        } else if new.trim().is_empty() {
            return Err(format!("The name of {} would be empty.", r.id));
        }
        changes.push((r.kind, r.index, new));
    }
    if target_id {
        // IDs must stay unique within their kind (deformers share one list).
        let group = |k: Kind| match k {
            Kind::Warp | Kind::Rotation => Kind::Warp,
            k => k,
        };
        let all = items(m);
        for kind in Kind::ALL {
            let mut ids: HashSet<String> = HashSet::new();
            for it in all.iter().filter(|it| group(it.kind) == group(kind)) {
                let id = changes
                    .iter()
                    .find(|(k, i, _)| *k == it.kind && *i == it.index)
                    .map_or(it.id.clone(), |c| c.2.clone());
                if !ids.insert(id.clone()) {
                    return Err(format!("ID: \"{id}\" is duplicated."));
                }
            }
        }
    }
    for (kind, i, new) in &changes {
        let info = match kind {
            Kind::Part => &mut Arc::make_mut(&mut m.parts[*i]).info,
            Kind::ArtMesh => &mut Arc::make_mut(&mut m.art_meshes[*i]).info,
            Kind::ArtPath => &mut Arc::make_mut(&mut m.art_paths[*i]).info,
            Kind::Warp | Kind::Rotation => &mut Arc::make_mut(&mut m.deformers[*i]).info,
            Kind::Glue => &mut Arc::make_mut(&mut m.glues[*i]).info,
            Kind::Parameter => {
                let p = Arc::make_mut(&mut m.parameters[*i]);
                if target_id {
                    p.id = new.clone();
                } else {
                    p.name = new.clone();
                }
                continue;
            }
        };
        if target_id {
            info.id = new.clone();
        } else {
            info.name = new.clone();
        }
    }
    Ok(changes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_text() {
        assert_eq!(replace_in("BrowR", "Brow", "Eyebrow", true), "EyebrowR");
        assert_eq!(replace_in("browR_brow", "BROW", "X", false), "XR_X");
        assert_eq!(replace_in("BrowR", "BROW", "X", true), "BrowR");
    }

    #[test]
    fn replace_checks_ids_and_names() {
        let mut m = Model {
            parameters: vec![
                Arc::new(keyform_model::Parameter::new("ParamA", 0.0, 1.0, 0.0)),
                Arc::new(keyform_model::Parameter::new("ParamB", 0.0, 1.0, 0.0)),
            ],
            ..Model::default()
        };
        let rows = items(&m);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            replace(&mut m, &rows, "B", "A", true, true),
            Err("ID: \"ParamA\" is duplicated.".into())
        );
        assert!(
            replace(&mut m, &rows, "Param", "1", true, true).is_err(),
            "invalid ID"
        );
        assert_eq!(replace(&mut m, &rows, "Param", "P_", true, true), Ok(2));
        assert_eq!(m.parameters[1].id, "P_B");
        Arc::make_mut(&mut m.parameters[0]).name = "Eye".into();
        let rows = items(&m);
        assert!(
            replace(&mut m, &rows[..1], "Eye", "", false, true).is_err(),
            "an empty name is refused"
        );
        assert_eq!(
            replace(&mut m, &rows[..1], "Eye", "Ear", false, true),
            Ok(1)
        );
        assert_eq!(m.parameters[0].name, "Ear");
    }
}
