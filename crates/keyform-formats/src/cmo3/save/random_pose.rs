//! Keeps the random-pose settings in step with the parameter set: one
//! `isEnable` entry per parameter (new entries true, or false when every
//! existing entry is false; entries of deleted parameters removed) and one
//! `isExpand` = true per parameter group. A model without a setting gets
//! one. spec: 70-advanced/psd-import.md §4.2, 10-formats/cmo3-schema.md
//! (CRandomPoseSettingManager, CRandomPoseSetting)

use std::collections::HashMap;

use keyform_model::Model;

use super::element_at;
use super::grid::{
    field_path, find_shared, get_shared, scalar, set_count, share_in_place, shared, stub, uuid_of,
    Ctx,
};
use crate::error::{invalid, Result};
use crate::xml::Element;

/// Whether the parameter set or the group count changed.
fn needs_sync(old: &Model, new: &Model) -> bool {
    old.parameters.len() != new.parameters.len()
        || old
            .parameters
            .iter()
            .zip(&new.parameters)
            .any(|(a, b)| a.guid != b.guid || a.id != b.id)
        || old.parameter_groups.len() != new.parameter_groups.len()
}

/// Depth-first path of the first element matching `f`.
fn find_path(e: &Element, f: &dyn Fn(&Element) -> bool, path: &mut Vec<u32>) -> bool {
    if f(e) {
        return true;
    }
    for (k, c) in e.children.iter().enumerate() {
        path.push(k as u32);
        if find_path(c, f, path) {
            return true;
        }
        path.pop();
    }
    false
}

/// The shared `id` element of the object at `obj` (`CParameterId` or
/// `CParameterGroupId`) and its id string; an inline id element is moved
/// into `<shared>`.
fn id_ref(root: &mut Element, ctx: &mut Ctx, obj: &[u32]) -> Result<Option<(String, String)>> {
    let id_path = match field_path(root, obj, "id") {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let id = share_in_place(root, ctx, &id_path)?;
    let idstr = get_shared(root, &id)
        .and_then(|e| e.attr("idstr"))
        .unwrap_or_default()
        .to_owned();
    Ok(Some((id, idstr)))
}

fn param_ids(root: &mut Element, ctx: &mut Ctx, new: &Model) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for p in &new.parameters {
        let snapshot = root.clone();
        let guid = p.guid.clone();
        let mut path = Vec::new();
        let found = find_path(
            &snapshot,
            &|e| {
                e.name == "CParameterSource"
                    && e.attr("xs.ref").is_none()
                    && e.child_field("guid")
                        .and_then(|g| uuid_of(&snapshot, g))
                        .as_deref()
                        == Some(guid.as_str())
            },
            &mut path,
        );
        if !found {
            return invalid(format!("parameter {}: element not found", p.id));
        }
        match id_ref(root, ctx, &path)? {
            Some(x) => out.push(x),
            None => return invalid(format!("parameter {}: no id", p.id)),
        }
    }
    Ok(out)
}

fn group_ids(root: &mut Element, ctx: &mut Ctx) -> Result<Vec<(String, String)>> {
    let Some(sh) = shared(root) else {
        return Ok(Vec::new());
    };
    let si = root
        .children
        .iter()
        .position(|c| c.name == "shared")
        .unwrap_or(0) as u32;
    let groups: Vec<u32> = sh
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| c.name == "CParameterGroup" && !c.children.is_empty())
        .map(|(k, _)| k as u32)
        .collect();
    let mut out = Vec::new();
    for k in groups {
        if let Some(x) = id_ref(root, ctx, &[si, k])? {
            out.push(x);
        }
    }
    Ok(out)
}

/// Existing (idstr → bool) entries of a key/value list pair.
fn entries(
    root: &Element,
    setting: &Element,
    keys: &str,
    values: &str,
    flag: &str,
) -> HashMap<String, bool> {
    let list = |n: &str| {
        setting
            .child_field(n)
            .map(|l| l.children.clone())
            .unwrap_or_default()
    };
    let (k, v) = (list(keys), list(values));
    k.iter()
        .zip(&v)
        .filter_map(|(k, v)| {
            let idstr = match k.attr("xs.ref") {
                Some(r) => get_shared(root, r)?.attr("idstr")?.to_owned(),
                None => k.attr("idstr")?.to_owned(),
            };
            let on = v.child_field(flag).map(|b| b.text.trim() == "true")?;
            Some((idstr, on))
        })
        .collect()
}

fn lists(
    name: &str,
    tag: &str,
    data: &str,
    flag: &str,
    ids: &[(String, String)],
    value: &dyn Fn(&str) -> bool,
) -> [Element; 2] {
    let mut keys = Element::new("array_list").with_attr("xs.n", format!("{name}.keys"));
    keys.children = ids.iter().map(|(id, _)| stub(tag, None, id)).collect();
    set_count(&mut keys);
    let mut values = Element::new("array_list").with_attr("xs.n", format!("{name}.values"));
    values.children = ids
        .iter()
        .map(|(_, s)| {
            let mut d = Element::new(data);
            d.children = vec![scalar("b", flag, value(s).to_string())];
            d
        })
        .collect();
    set_count(&mut values);
    [keys, values]
}

pub(super) fn sync_random_pose(
    root: &mut Element,
    ctx: &mut Ctx,
    old: &Model,
    new: &Model,
) -> Result<()> {
    if !needs_sync(old, new) {
        return Ok(());
    }
    let main = match root.children.iter().position(|c| c.name == "main") {
        Some(i) => vec![i as u32, 0],
        None => return invalid("document has no <main>"),
    };
    let Ok(mgr) = field_path(root, &main, "randomPoseSetting") else {
        // Older documents without random poses.
        return Ok(());
    };
    if element_at(root, Some(&mgr))?.attr("xs.ref").is_some() {
        return Ok(());
    }
    let list = field_path(root, &mgr, "_settings")?;
    let params = param_ids(root, ctx, new)?;
    let groups = group_ids(root, ctx)?;
    ctx.structural = true;
    if element_at(root, Some(&list))?.children.is_empty() {
        element_at(root, Some(&list))?
            .children
            .push(Element::new("CRandomPoseSetting"));
    }
    let n = element_at(root, Some(&list))?.children.len();
    for k in 0..n {
        let mut p = list.clone();
        p.push(k as u32);
        let snapshot = root.clone();
        let setting = element_at(root, Some(&p))?.clone();
        let setting = match setting.attr("xs.ref") {
            Some(r) => match get_shared(&snapshot, r) {
                Some(e) => e.clone(),
                None => continue,
            },
            None => setting,
        };
        let pe = entries(
            &snapshot,
            &setting,
            "parameters.keys",
            "parameters.values",
            "isEnable",
        );
        let ge = entries(
            &snapshot,
            &setting,
            "groups.keys",
            "groups.values",
            "isExpand",
        );
        let all_false = !pe.is_empty() && pe.values().all(|v| !v);
        let [pk, pv] = lists(
            "parameters",
            "CParameterId",
            "CRandomPoseParamData",
            "isEnable",
            &params,
            &|s| pe.get(s).copied().unwrap_or(!all_false),
        );
        let [gk, gv] = lists(
            "groups",
            "CParameterGroupId",
            "CRandomPoseGroupData",
            "isExpand",
            &groups,
            &|s| ge.get(s).copied().unwrap_or(true),
        );
        let body = vec![pk, pv, gk, gv];
        match element_at(root, Some(&p))?
            .attr("xs.ref")
            .map(str::to_owned)
        {
            Some(r) => find_shared(root, &r)?.children = body,
            None => element_at(root, Some(&p))?.children = body,
        }
    }
    set_count(element_at(root, Some(&list))?);
    Ok(())
}
