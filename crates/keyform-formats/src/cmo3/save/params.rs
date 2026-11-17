//! Writing parameter additions, deletions and regrouping, and the lists
//! that follow the parameter set: parameter-group children, the eye-blink
//! and lip-sync effect groups, cursor-tracking settings and deleted
//! physics settings. Matching is by GUID. spec: 10-formats/cmo3-schema.md
//! (CParameterSource, CParameterGroup, TrackCursorSetting), palettes.md §3

use std::collections::HashMap;
use std::sync::Arc;

use keyform_model::{Model, ParamEntry, Parameter, ParameterKind, PhysicsSetting};

use super::grid::{
    add_shared, field_path, get_shared, scalar, set_count, share_in_place, shared, stub, uuid_of,
    Ctx,
};
use super::objects::guid_id;
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
            ctx.guid_ids.insert(p.guid.clone(), id);
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
        ctx.guid_ids.insert(p.guid.clone(), id.clone());
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
    if old.physics.as_stored() == new.physics.as_stored() {
        return Ok(());
    }
    // The document stores no gravity, wind or enable flag (physics.md §1).
    if old.physics.gravity != new.physics.gravity || old.physics.wind != new.physics.wind {
        return unsupported("physics gravity / wind (not stored in the model file)");
    }
    ctx.structural = true;
    let main = main_path(root)?;
    let set = field_path(root, &main, "physicsSettingsSourceSet")?;
    let list = field_path(root, &set, "_sourceCubismPhysics")?;
    let snapshot = element_at(root, Some(&list))?.clone();
    // Existing entries by setting uuid: (list child, its content).
    let mut by_uuid: HashMap<String, (Element, Element)> = HashMap::new();
    for c in &snapshot.children {
        let content = match c.attr("xs.ref") {
            Some(id) => get_shared(root, id).cloned(),
            None => Some(c.clone()),
        };
        let Some(content) = content else { continue };
        let uuid = content
            .child_field("guid")
            .cloned()
            .and_then(|g| uuid_of(root, &g));
        if let Some(u) = uuid {
            by_uuid.insert(u, (c.clone(), content));
        }
    }
    let mut children = Vec::new();
    for s in &new.physics.settings {
        let before = old.physics.settings.iter().find(|o| o.guid == s.guid);
        let (before, s) = (
            before.map(|b| PhysicsSetting {
                enabled: true,
                ..b.clone()
            }),
            &PhysicsSetting {
                enabled: true,
                ..s.clone()
            },
        );
        let before = before.as_ref();
        match (by_uuid.remove(&s.guid), before) {
            (Some((child, _)), Some(b)) if same_setting(old, b, new, s) => children.push(child),
            (Some((child, content)), _) => {
                if let Some(id) = child.attr("xs.ref") {
                    ctx.orphans.push(id.to_owned());
                }
                children.push(physics_setting(root, ctx, new, s, Some(&content))?);
            }
            (None, _) => children.push(physics_setting(root, ctx, new, s, None)?),
        }
    }
    for (child, _) in by_uuid.into_values() {
        if let Some(id) = child.attr("xs.ref") {
            ctx.orphans.push(id.to_owned());
        }
    }
    let l = element_at(root, Some(&list))?;
    l.children = children;
    set_count(l);
    // `settingFPS`: `<i>` or `<null/>` (both observed).
    if old.physics.fps != new.physics.fps {
        let e = element_at(root, Some(&set))?;
        let fps = match new.physics.fps {
            Some(f) => scalar("i", "settingFPS", format!("{}", f.round() as i32)),
            None => Element::new("null").with_attr("xs.n", "settingFPS"),
        };
        match e
            .children
            .iter()
            .position(|c| c.field_name() == Some("settingFPS"))
        {
            Some(i) => e.children[i] = fps,
            None => e.children.push(fps),
        }
    }
    Ok(())
}

/// Whether two settings are equal, comparing parameters by GUID.
fn same_setting(
    ma: &Model,
    a: &keyform_model::PhysicsSetting,
    mb: &Model,
    b: &keyform_model::PhysicsSetting,
) -> bool {
    let pg = |m: &Model, i: usize| m.parameters.get(i).map(|p| p.guid.clone());
    let strip = |s: &keyform_model::PhysicsSetting| {
        let mut s = s.clone();
        s.inputs.iter_mut().for_each(|x| x.parameter = 0);
        s.outputs.iter_mut().for_each(|x| x.parameter = 0);
        s
    };
    strip(a) == strip(b)
        && a.inputs
            .iter()
            .zip(&b.inputs)
            .all(|(x, y)| pg(ma, x.parameter) == pg(mb, y.parameter))
        && a.outputs
            .iter()
            .zip(&b.outputs)
            .all(|(x, y)| pg(ma, x.parameter) == pg(mb, y.parameter))
}

/// A `CPhysicsSettingsSource` element in the layout the reference writes
/// (inline, fields name, guid, id, inputs, outputs, vertices, normalisation;
/// observed in hiyori_free_t08.cmo3). Data GUIDs and the inputs' unused
/// scales are kept from `template` by position.
/// spec: 10-formats/cmo3-schema.md `CPhysicsSettingsSource`, physics.md §1
fn physics_setting(
    root: &mut Element,
    ctx: &mut Ctx,
    m: &Model,
    s: &keyform_model::PhysicsSetting,
    template: Option<&Element>,
) -> Result<Element> {
    use keyform_model::PhysicsKind;
    let f = |v: f32| crate::graph::format_f32(v);
    let el = |tag: &str, name: &str| Element::new(tag).with_attr("xs.n", name);
    let vec2 = |name: &str, v: [f32; 2]| {
        let mut e = Element::new("GVector2").with_attr("xs.n", name);
        e.children = vec![scalar("f", "x", f(v[0])), scalar("f", "y", f(v[1]))];
        e
    };
    let kind = |k: PhysicsKind| {
        el("CPhysicsSourceType", "type").with_attr(
            "v",
            match k {
                PhysicsKind::X => "SRC_TO_X",
                PhysicsKind::Y => "SRC_TO_Y",
                PhysicsKind::Angle => "SRC_TO_G_ANGLE",
            },
        )
    };
    let old_items = |list: &str| -> Vec<Element> {
        template
            .and_then(|t| t.child_field(list))
            .map(|l| l.children.clone())
            .unwrap_or_default()
    };
    let data_guid = |old: Option<&Element>| {
        let uuid = old
            .and_then(|o| o.child_field("guid"))
            .and_then(|g| g.attr("uuid"))
            .map(str::to_owned)
            .unwrap_or_else(keyform_model::new_guid);
        el("CPhysicsDataGuid", "guid")
            .with_attr("uuid", uuid)
            .with_attr("note", "(no debug info)")
    };
    let param_ref = |root: &mut Element, ctx: &mut Ctx, i: usize, name: &str| -> Result<Element> {
        let Some(p) = m.parameters.get(i) else {
            return invalid("physics refers to a missing parameter");
        };
        let id = guid_id(root, ctx, &p.guid, "CParameterGuid", &p.id)?;
        Ok(stub("CParameterGuid", Some(name), &id))
    };
    let list = |name: &str, items: Vec<Element>| {
        let mut l = el("carray_list", name);
        l.children = items;
        set_count(&mut l);
        l
    };
    let old_in = old_items("inputs");
    let mut inputs = Vec::new();
    for (k, x) in s.inputs.iter().enumerate() {
        let o = old_in.get(k);
        let mut e = Element::new("CPhysicsInput");
        let angle = o
            .and_then(|o| o.child_field("angleScale"))
            .cloned()
            .unwrap_or_else(|| scalar("f", "angleScale", "0.0"));
        let trans = o
            .and_then(|o| o.child_field("translationScale"))
            .cloned()
            .unwrap_or_else(|| vec2("translationScale", [0.0, 0.0]));
        e.children = vec![
            data_guid(o),
            param_ref(root, ctx, x.parameter, "source")?,
            angle,
            trans,
            scalar("f", "weight", f(x.weight)),
            kind(x.kind),
            scalar("b", "isReverse", x.reflect.to_string()),
        ];
        inputs.push(e);
    }
    let old_out = old_items("outputs");
    let mut outputs = Vec::new();
    for (k, x) in s.outputs.iter().enumerate() {
        let mut e = Element::new("CPhysicsOutput");
        e.children = vec![
            data_guid(old_out.get(k)),
            param_ref(root, ctx, x.parameter, "destination")?,
            scalar("i", "vertexIndex", x.vertex_index.to_string()),
            vec2("translationScale", x.translation_scale),
            scalar("f", "angleScale", f(x.angle_scale)),
            scalar("f", "weight", f(x.weight)),
            kind(x.kind),
            scalar("b", "isReverse", x.reflect.to_string()),
        ];
        outputs.push(e);
    }
    let old_v = old_items("vertices");
    let mut vertices = Vec::new();
    for (k, v) in s.vertices.iter().enumerate() {
        let mut e = Element::new("CPhysicsVertex");
        e.children = vec![
            data_guid(old_v.get(k)),
            vec2("position", v.position),
            scalar("f", "mobility", f(v.mobility)),
            scalar("f", "delay", f(v.delay)),
            scalar("f", "acceleration", f(v.acceleration)),
            scalar("f", "radius", f(v.radius)),
        ];
        vertices.push(e);
    }
    let mut e = Element::new("CPhysicsSettingsSource");
    let mut name = el("s", "name");
    name.text = super::xml_text(&s.name);
    e.children = vec![
        name,
        el("CPhysicsSettingsGuid", "guid")
            .with_attr("uuid", s.guid.clone())
            .with_attr("note", "(no debug info)"),
        el("CPhysicsSettingId", "id").with_attr("idstr", s.id.clone()),
        list("inputs", inputs),
        list("outputs", outputs),
        list("vertices", vertices),
        scalar("f", "normalizedPositionValueMax", f(s.position.maximum)),
        scalar("f", "normalizedPositionValueMin", f(s.position.minimum)),
        scalar("f", "normalizedPositionDefaultValue", f(s.position.default)),
        scalar("f", "normalizedAngleValueMax", f(s.angle.maximum)),
        scalar("f", "normalizedAngleValueMin", f(s.angle.minimum)),
        scalar("f", "normalizedAngleDefaultValue", f(s.angle.default)),
    ];
    Ok(e)
}
