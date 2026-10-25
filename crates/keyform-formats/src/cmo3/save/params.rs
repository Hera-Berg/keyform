//! Writing parameter additions, deletions and regrouping, and the lists
//! that follow the parameter set: parameter-group children, the eye-blink
//! and lip-sync effect groups, cursor-tracking settings and deleted
//! physics settings. Matching is by GUID. spec: 10-formats/cmo3-schema.md
//! (CParameterSource, CParameterGroup, TrackCursorSetting), palettes.md §3

use std::collections::HashMap;
use std::sync::Arc;

use keyform_model::{Model, ParamEntry, Parameter, ParameterKind};

use super::grid::{
    add_shared, field_path, get_shared, scalar, set_count, share_in_place, shared, stub, uuid_of,
    Ctx,
};
use super::{element_at, format_f32, unsupported, write_parameter};
use crate::cmo3::SourceMap;
use crate::error::{invalid, Result};
use crate::xml::Element;

/// Path of the document's root object (`<main>`'s first child).
fn main_path(root: &Element) -> Result<Vec<u32>> {
    match root.children.iter().position(|c| c.name == "main") {
        Some(i) => Ok(vec![i as u32, 0]),
        None => invalid("document has no <main>"),
    }
}

/// The element of a field of the root object, following a reference.
fn root_field_id_or_path(root: &mut Element, name: &str) -> Result<FieldRef> {
    let main = main_path(root)?;
    let path = field_path(root, &main, name)?;
    let e = element_at(root, Some(&path))?;
    Ok(match e.attr("xs.ref") {
        Some(id) => FieldRef::Shared(id.to_owned()),
        None => FieldRef::Path(path),
    })
}

enum FieldRef {
    Shared(String),
    Path(Vec<u32>),
}

fn resolve<'e>(root: &'e mut Element, r: &FieldRef) -> Result<&'e mut Element> {
    match r {
        FieldRef::Shared(id) => super::grid::find_shared(root, id),
        FieldRef::Path(p) => element_at(root, Some(p)),
    }
}

fn child_mut<'e>(e: &'e mut Element, name: &str) -> Result<&'e mut Element> {
    match e.children.iter_mut().find(|c| c.field_name() == Some(name)) {
        Some(c) => Ok(c),
        None => invalid(format!("missing field {name:?}")),
    }
}

fn kind_name(k: ParameterKind) -> &'static str {
    match k {
        ParameterKind::Normal => "NORMAL",
        ParameterKind::BlendShape => "MORPH_TARGET",
    }
}

/// Default cursor-tracking mapping of a new parameter (viewer.md §3.6).
fn track_kind(id: &str) -> &'static str {
    match id {
        "ParamAngleX" | "ParamEyeBallX" | "ParamBodyAngleX" | "PARAM_ANGLE_X"
        | "PARAM_EYE_BALL_X" | "PARAM_BODY_ANGLE_X" => "X",
        "ParamAngleY" | "ParamEyeBallY" | "PARAM_ANGLE_Y" | "PARAM_EYE_BALL_Y" => "Y",
        _ => "NONE",
    }
}

/// Shared ids of the groups' GUIDs: the root group, then each model group.
struct Groups {
    root: FieldRef,
    root_guid: String,
    /// Model group index → (element, guid shared id).
    groups: Vec<(FieldRef, String)>,
}

fn find_groups(root: &mut Element, ctx: &mut Ctx, m: &Model) -> Result<Groups> {
    let root_group = root_field_id_or_path(root, "rootParameterGroup")?;
    let root_guid = {
        let path = match &root_group {
            FieldRef::Shared(id) => shared_path(root, id)?,
            FieldRef::Path(p) => p.clone(),
        };
        let g = field_path(root, &path, "guid")?;
        share_in_place(root, ctx, &g)?
    };
    // The set's group elements by GUID.
    let set = root_field_id_or_path(root, "parameterGroupSet")?;
    let set_path = match &set {
        FieldRef::Shared(id) => shared_path(root, id)?,
        FieldRef::Path(p) => p.clone(),
    };
    let list = field_path(root, &set_path, "_groups")?;
    let items = element_at(root, Some(&list))?.children.len();
    let mut by_uuid: HashMap<String, FieldRef> = HashMap::new();
    for k in 0..items {
        let mut item = list.clone();
        item.push(k as u32);
        let r = match element_at(root, Some(&item))?.attr("xs.ref") {
            Some(id) => FieldRef::Shared(id.to_owned()),
            None => FieldRef::Path(item),
        };
        let path = match &r {
            FieldRef::Shared(id) => shared_path(root, id)?,
            FieldRef::Path(p) => p.clone(),
        };
        let g = field_path(root, &path, "guid")?;
        let e = element_at(root, Some(&g))?.clone();
        if let Some(u) = uuid_of(root, &e) {
            by_uuid.insert(u, r);
        }
    }
    let mut groups = Vec::new();
    for g in &m.parameter_groups {
        let Some(r) = by_uuid.remove(&g.guid) else {
            return unsupported(format!("parameter group {}: new groups", g.id));
        };
        let path = match &r {
            FieldRef::Shared(id) => shared_path(root, id)?,
            FieldRef::Path(p) => p.clone(),
        };
        let gp = field_path(root, &path, "guid")?;
        let id = share_in_place(root, ctx, &gp)?;
        groups.push((r, id));
    }
    Ok(Groups {
        root: root_group,
        root_guid,
        groups,
    })
}

/// Path of a shared object (child of `<shared>`).
fn shared_path(root: &Element, id: &str) -> Result<Vec<u32>> {
    let si = root
        .children
        .iter()
        .position(|c| c.name == "shared")
        .ok_or_else(|| crate::error::FormatError::Invalid("no <shared>".into()))?;
    let k = shared(root)
        .and_then(|s| s.children.iter().position(|c| c.attr("xs.id") == Some(id)))
        .ok_or_else(|| crate::error::FormatError::Invalid(format!("ref id {id}: not found")))?;
    Ok(vec![si as u32, k as u32])
}

/// A new `CParameterSource`, fields in the reference's order.
fn parameter_element(p: &Parameter, guid_id: &str, group_guid_id: &str) -> Element {
    let mut e = Element::new("CParameterSource");
    e.children = vec![
        scalar("i", "decimalPlaces", p.decimal_places.to_string()),
        stub("CParameterGuid", Some("guid"), guid_id),
        scalar("f", "snapEpsilon", format_f32(p.snap_epsilon)),
        scalar("f", "minValue", format_f32(p.min)),
        scalar("f", "maxValue", format_f32(p.max)),
        scalar("f", "defaultValue", format_f32(p.default)),
        scalar("b", "isRepeat", p.repeat.to_string()),
        Element::new("CParameterId")
            .with_attr("xs.n", "id")
            .with_attr("idstr", p.id.clone()),
        Element::new("Type")
            .with_attr("xs.n", "paramType")
            .with_attr("v", kind_name(p.kind)),
        scalar("s", "name", super::xml_text(&p.name)),
        scalar("s", "description", super::xml_text(&p.description)),
        scalar("b", "combined", p.combined.to_string()),
        stub(
            "CParameterGroupGuid",
            Some("parentGroupGuid"),
            group_guid_id,
        ),
    ];
    e
}

/// Writes the parameter changes from `old` to `new`.
pub(super) fn sync_parameters(
    root: &mut Element,
    ctx: &mut Ctx,
    s: &SourceMap,
    old: &Model,
    new: &Model,
) -> Result<()> {
    let old_by_guid: HashMap<&str, usize> = old
        .parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (p.guid.as_str(), i))
        .collect();
    let new_guids: Vec<&str> = new.parameters.iter().map(|p| p.guid.as_str()).collect();
    let removed: Vec<usize> = (0..old.parameters.len())
        .filter(|&i| !new_guids.contains(&old.parameters[i].guid.as_str()))
        .collect();
    let added: Vec<usize> = (0..new.parameters.len())
        .filter(|&i| !old_by_guid.contains_key(new.parameters[i].guid.as_str()))
        .collect();
    let lists_changed = !removed.is_empty()
        || !added.is_empty()
        || old.root_parameter_entries.len() != new.root_parameter_entries.len()
        || old
            .parameter_groups
            .iter()
            .zip(&new.parameter_groups)
            .any(|(a, b)| a.children.len() != b.children.len())
        || old
            .parameters
            .iter()
            .zip(&new.parameters)
            .any(|(a, b)| a.guid != b.guid)
        || old.root_parameter_entries != new.root_parameter_entries
        || old
            .parameter_groups
            .iter()
            .zip(&new.parameter_groups)
            .any(|(a, b)| a.children != b.children);
    let effects_changed = lists_changed
        || old.eye_blink_parameters != new.eye_blink_parameters
        || old.lip_sync_parameters != new.lip_sync_parameters;

    // 1. Field edits of kept parameters (group moves handled with the lists).
    for n in &new.parameters {
        let Some(&oi) = old_by_guid.get(n.guid.as_str()) else {
            continue;
        };
        let o = &old.parameters[oi];
        if Arc::ptr_eq(o, n) {
            continue;
        }
        let path = s.parameters.get(oi).cloned().flatten();
        let fields = Parameter {
            group: o.group,
            ..(**n).clone()
        };
        if **o != fields {
            write_parameter(element_at(root, path.as_deref())?, o, &fields)?;
        }
    }
    if !lists_changed && !effects_changed {
        return Ok(());
    }
    ctx.structural = true;
    let groups = find_groups(root, ctx, new)?;

    // Parameter GUID shared ids of the kept parameters (shared if inline).
    let mut guid_ids: HashMap<String, String> = HashMap::new();
    for (oi, p) in old.parameters.iter().enumerate() {
        if let Some(Some(path)) = s.parameters.get(oi) {
            let g = field_path(root, path, "guid")?;
            let id = share_in_place(root, ctx, &g)?;
            guid_ids.insert(p.guid.clone(), id.clone());
            ctx.param_guid_ids.insert(p.guid.clone(), id);
        }
    }

    // 2. Group of each kept parameter that moved.
    for (oi, o) in old.parameters.iter().enumerate() {
        let Some(&ni) = new_guids.iter().position(|g| *g == o.guid).as_ref() else {
            continue;
        };
        let n = &new.parameters[ni];
        if o.group != n.group {
            let gid = match n.group {
                Some(g) => groups.groups[g].1.clone(),
                None => groups.root_guid.clone(),
            };
            if let Some(Some(path)) = s.parameters.get(oi) {
                let f = field_path(root, path, "parentGroupGuid")?;
                *element_at(root, Some(&f))? =
                    stub("CParameterGroupGuid", Some("parentGroupGuid"), &gid);
            }
        }
    }

    // 3. Deleted parameters: elements and tracking entries.
    let sources_list = {
        let set = root_field_id_or_path(root, "parameterSourceSet")?;
        let set_path = match &set {
            FieldRef::Shared(id) => shared_path(root, id)?,
            FieldRef::Path(p) => p.clone(),
        };
        field_path(root, &set_path, "_sources")?
    };
    let mut gone_ids = Vec::new();
    let mut doomed: Vec<u32> = Vec::new();
    for &oi in &removed {
        let Some(Some(path)) = s.parameters.get(oi) else {
            return invalid("deleted parameter has no element");
        };
        if path[..path.len() - 1] != sources_list[..] {
            return unsupported("parameter outside the parameter list");
        }
        doomed.push(*path.last().unwrap_or(&0));
        if let Some(id) = guid_ids.get(&old.parameters[oi].guid) {
            gone_ids.push(id.clone());
        }
    }
    doomed.sort_unstable_by(|a, b| b.cmp(a));
    {
        let list = element_at(root, Some(&sources_list))?;
        for k in doomed {
            if (k as usize) < list.children.len() {
                list.children.remove(k as usize);
            }
        }
        set_count(list);
    }
    ctx.orphans.extend(gone_ids.iter().cloned());

    // 4. New parameters.
    let mut new_elements = Vec::new();
    for &ni in &added {
        let p = &new.parameters[ni];
        let guid = Element::new("CParameterGuid")
            .with_attr("uuid", p.guid.clone())
            .with_attr("note", p.id.clone());
        let id = add_shared(root, ctx, guid)?;
        guid_ids.insert(p.guid.clone(), id.clone());
        ctx.param_guid_ids.insert(p.guid.clone(), id.clone());
        let gid = match p.group {
            Some(g) => groups.groups[g].1.clone(),
            None => groups.root_guid.clone(),
        };
        new_elements.push(parameter_element(p, &id, &gid));
    }
    {
        let list = element_at(root, Some(&sources_list))?;
        list.children.extend(new_elements);
        set_count(list);
    }

    // 5. Group child lists, rewritten from the model.
    let entry = |e: &ParamEntry| -> Result<Element> {
        Ok(match *e {
            ParamEntry::Parameter(p) => {
                let g = &new.parameters[p].guid;
                match guid_ids.get(g) {
                    Some(id) => stub("CParameterGuid", None, id),
                    None => return invalid("parameter without GUID object"),
                }
            }
            ParamEntry::Group(g) => stub("CParameterGroupGuid", None, &groups.groups[g].1),
        })
    };
    let root_children: Vec<Element> = new
        .root_parameter_entries
        .iter()
        .map(entry)
        .collect::<Result<_>>()?;
    let mut group_children = Vec::new();
    for g in &new.parameter_groups {
        group_children.push(g.children.iter().map(entry).collect::<Result<Vec<_>>>()?);
    }
    {
        let e = resolve(root, &groups.root)?;
        let l = child_mut(e, "_childGuids")?;
        l.children = root_children;
        set_count(l);
    }
    for (k, children) in group_children.into_iter().enumerate() {
        let e = resolve(root, &groups.groups[k].0)?;
        let l = child_mut(e, "_childGuids")?;
        l.children = children;
        set_count(l);
    }

    // 6. Eye-blink / lip-sync effect groups.
    if effects_changed {
        let lists = [
            ("EYE_BLINK", &new.eye_blink_parameters),
            ("LIPSYNC", &new.lip_sync_parameters),
        ];
        let main = main_path(root)?;
        if let Ok(info) = field_path(root, &main, "modelInfo") {
            let effects = field_path(root, &info, "_effectParameterGroups")?;
            let map = field_path(root, &effects, "_parameterGroups")?;
            let n = element_at(root, Some(&map))?.children.len();
            for k in 0..n {
                let mut entry_path = map.clone();
                entry_path.push(k as u32);
                let e = element_at(root, Some(&entry_path))?;
                let key = e
                    .child_field("key")
                    .and_then(|c| c.attr("v"))
                    .unwrap_or_default()
                    .to_owned();
                let Some((_, ids)) = lists.iter().find(|(k, _)| *k == key) else {
                    continue;
                };
                let stubs: Vec<Element> = ids
                    .iter()
                    .filter_map(|&p| new.parameters.get(p))
                    .filter_map(|p| guid_ids.get(&p.guid))
                    .map(|id| stub("CParameterGuid", None, id))
                    .collect();
                let e = element_at(root, Some(&entry_path))?;
                let v = child_mut(e, "value")?;
                v.children = stubs;
                set_count(v);
            }
        }
    }

    // 7. Cursor-tracking settings: one per parameter.
    let main = main_path(root)?;
    if let Ok(viewer) = field_path(root, &main, "modelViewerSetting") {
        if let Ok(list) = field_path(root, &viewer, "trackCursorSettings") {
            let l = element_at(root, Some(&list))?;
            l.children.retain(|c| {
                c.child_field("guid")
                    .and_then(|g| g.attr("xs.ref"))
                    .is_none_or(|r| !gone_ids.iter().any(|x| x == r))
            });
            for &ni in &added {
                let p = &new.parameters[ni];
                let mut t = Element::new("TrackCursorSetting");
                t.children = vec![
                    stub("CParameterGuid", Some("guid"), &guid_ids[&p.guid]),
                    Element::new("TrackCursorSettingItemType")
                        .with_attr("xs.n", "value")
                        .with_attr("v", track_kind(&p.id)),
                    scalar("f", "weight", "1.0"),
                ];
                l.children.push(t);
            }
            set_count(l);
        }
    }
    let _ = get_shared;
    Ok(())
}

/// Deleted physics settings (by GUID); other physics edits are not
/// written yet.
pub(super) fn sync_physics(
    root: &mut Element,
    ctx: &mut Ctx,
    old: &Model,
    new: &Model,
) -> Result<()> {
    if old.physics == new.physics {
        return Ok(());
    }
    let kept: Vec<&str> = new
        .physics
        .settings
        .iter()
        .map(|s| s.guid.as_str())
        .collect();
    let survivors: Vec<_> = old
        .physics
        .settings
        .iter()
        .filter(|s| kept.contains(&s.guid.as_str()))
        .collect();
    let same_rest = survivors.len() == new.physics.settings.len()
        && survivors.iter().zip(&new.physics.settings).all(|(a, b)| {
            // Parameter indices may have shifted: compare by GUID.
            let pg = |m: &Model, i: usize| m.parameters.get(i).map(|p| p.guid.clone());
            a.guid == b.guid
                && a.inputs.len() == b.inputs.len()
                && a.outputs.len() == b.outputs.len()
                && a.inputs
                    .iter()
                    .zip(&b.inputs)
                    .all(|(x, y)| pg(old, x.parameter) == pg(new, y.parameter))
                && a.outputs
                    .iter()
                    .zip(&b.outputs)
                    .all(|(x, y)| pg(old, x.parameter) == pg(new, y.parameter))
        })
        && old.physics.fps == new.physics.fps;
    if !same_rest {
        return unsupported("physics settings");
    }
    ctx.structural = true;
    let main = main_path(root)?;
    let set = field_path(root, &main, "physicsSettingsSourceSet")?;
    let list = field_path(root, &set, "_sourceCubismPhysics")?;
    let snapshot = element_at(root, Some(&list))?.clone();
    let mut keep = Vec::new();
    for c in &snapshot.children {
        let content = match c.attr("xs.ref") {
            Some(id) => get_shared(root, id).cloned(),
            None => Some(c.clone()),
        };
        let uuid = content
            .as_ref()
            .and_then(|e| e.child_field("guid").cloned())
            .and_then(|g| uuid_of(root, &g));
        if uuid.is_none_or(|u| kept.contains(&u.as_str())) {
            keep.push(c.clone());
        } else if let Some(id) = c.attr("xs.ref") {
            ctx.orphans.push(id.to_owned());
        }
    }
    let l = element_at(root, Some(&list))?;
    l.children = keep;
    set_count(l);
    Ok(())
}
