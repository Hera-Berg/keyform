//! Structural object changes: new and deleted parts and deformers, parent
//! deformer and part changes, part child lists. Objects are matched by
//! GUID. New elements are cloned from an existing element of the same type
//! in the document (so they use the layout of the document's type
//! versions) and rewritten.
//! spec: 10-formats/cmo3-schema.md (CPartSource, C*DeformerSource,
//! ACParameterControllableSource, ACDeformerSource, ACForm),
//! 35-editing/change-parent.md §2 step 4 (coordinate type)

use std::collections::HashMap;

use keyform_model::{
    Deformer, DeformerKind, KeyformGrid, Model, Part, PartChild, NOT_INITIALIZED_GUID,
};

use super::grid::{
    add_shared, field_path, find_shared, get_shared, scalar, set_count, share_in_place, shared,
    shared_mut, stub, uuid_of, Ctx,
};
use super::{element_at, format_f32, holder_depth, set_scalar, unsupported, xml_text};
use crate::cmo3::{ElementPath, ObjectSource};
use crate::error::{invalid, Result};
use crate::xml::Element;

/// Index of `<shared>` among the root's children.
fn shared_index(root: &Element) -> Result<u32> {
    match root.children.iter().position(|c| c.name == "shared") {
        Some(i) => Ok(i as u32),
        None => invalid("document has no <shared>"),
    }
}

fn shared_path_of(root: &Element, id: &str) -> Result<ElementPath> {
    let si = shared_index(root)?;
    let k = shared(root)
        .and_then(|s| s.children.iter().position(|c| c.attr("xs.id") == Some(id)))
        .ok_or_else(|| crate::error::FormatError::Invalid(format!("ref id {id}: not found")))?;
    Ok(vec![si, k as u32])
}

/// Shared id of the GUID object with this uuid: an object's own GUID
/// (shared if inline), or an existing shared GUID, or a new one.
pub(super) fn guid_id(
    root: &mut Element,
    ctx: &mut Ctx,
    uuid: &str,
    tag: &str,
    note: &str,
) -> Result<String> {
    if let Some(id) = ctx.guid_ids.get(uuid) {
        return Ok(id.clone());
    }
    // An existing object's guid field.
    if let Some(path) = ctx.object_paths.get(uuid).cloned() {
        let f = field_path(root, &path, "guid")?;
        let id = share_in_place(root, ctx, &f)?;
        ctx.guid_ids.insert(uuid.to_owned(), id.clone());
        return Ok(id);
    }
    // Any shared GUID object with that uuid.
    if let Some(id) = shared(root).and_then(|s| {
        s.children
            .iter()
            .find(|c| c.name == tag && c.attr("uuid") == Some(uuid))
            .and_then(|c| c.attr("xs.id"))
            .map(str::to_owned)
    }) {
        ctx.guid_ids.insert(uuid.to_owned(), id.clone());
        return Ok(id);
    }
    let e = Element::new(tag)
        .with_attr("uuid", uuid)
        .with_attr("note", note);
    let id = add_shared(root, ctx, e)?;
    ctx.guid_ids.insert(uuid.to_owned(), id.clone());
    Ok(id)
}

/// Shared id of a `CoordType` named `name` ("Canvas" / "DeformerLocal").
fn coord_id(root: &mut Element, ctx: &mut Ctx, name: &str) -> Result<String> {
    if let Some(id) = shared(root).and_then(|s| {
        s.children
            .iter()
            .find(|c| {
                c.name == "CoordType" && c.child_field("coordName").is_some_and(|n| n.text == name)
            })
            .and_then(|c| c.attr("xs.id"))
            .map(str::to_owned)
    }) {
        return Ok(id);
    }
    let mut e = Element::new("CoordType");
    e.children.push(scalar("s", "coordName", name));
    add_shared(root, ctx, e)
}

fn target_uuid(m: &Model, t: Option<usize>) -> String {
    m.target_deformer_guid(t).to_owned()
}

fn part_uuid(m: &Model, p: Option<usize>) -> Option<String> {
    p.and_then(|p| m.parts.get(p)).map(|p| p.info.guid.clone())
}

/// Sets reference field `name` of the object at `obj` to the shared `id`.
fn set_ref(root: &mut Element, obj: &[u32], name: &str, tag: &str, id: &str) -> Result<()> {
    let f = field_path(root, obj, name)?;
    *element_at(root, Some(&f))? = stub(tag, Some(name), id);
    Ok(())
}

/// The keyform elements of the object at `obj` (resolving shared ones).
fn form_paths(root: &mut Element, obj: &[u32]) -> Result<Vec<ElementPath>> {
    let list = field_path(root, obj, "keyforms")?;
    let n = element_at(root, Some(&list))?.children.len();
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let mut p = list.clone();
        p.push(k as u32);
        match element_at(root, Some(&p))?
            .attr("xs.ref")
            .map(str::to_owned)
        {
            Some(id) => out.push(shared_path_of(root, &id)?),
            None => out.push(p),
        }
    }
    Ok(out)
}

/// Points every keyform's `coordType` at Canvas (root) or DeformerLocal.
fn set_coord_types(
    root: &mut Element,
    ctx: &mut Ctx,
    obj: &[u32],
    root_target: bool,
) -> Result<()> {
    let id = coord_id(
        root,
        ctx,
        if root_target {
            "Canvas"
        } else {
            "DeformerLocal"
        },
    )?;
    for p in form_paths(root, obj)? {
        if let Ok(f) = field_path(root, &p, "coordType") {
            *element_at(root, Some(&f))? = stub("CoordType", Some("coordType"), &id);
        }
    }
    Ok(())
}

/// Writes a new parent deformer (`targetDeformerGuid` and coordinates).
pub(super) fn write_target(
    root: &mut Element,
    ctx: &mut Ctx,
    obj: &[u32],
    new: &Model,
    t: Option<usize>,
) -> Result<()> {
    let uuid = target_uuid(new, t);
    let note = if t.is_none() { "ROOT" } else { "" };
    let id = guid_id(root, ctx, &uuid, "CDeformerGuid", note)?;
    set_ref(root, obj, "targetDeformerGuid", "CDeformerGuid", &id)?;
    set_coord_types(root, ctx, obj, t.is_none())
}

/// Writes a new parent part (`parentGuid`).
pub(super) fn write_parent_part(
    root: &mut Element,
    ctx: &mut Ctx,
    obj: &[u32],
    new: &Model,
    p: Option<usize>,
) -> Result<()> {
    let Some(uuid) = part_uuid(new, p) else {
        return unsupported("an object without a part");
    };
    let id = guid_id(root, ctx, &uuid, "CPartGuid", "")?;
    set_ref(root, obj, "parentGuid", "CPartGuid", &id)
}

/// Rewrites a part's `_childGuids` from the model; entries Keyform does
/// not model (`PartChild::Other`) keep their place.
pub(super) fn write_children(
    root: &mut Element,
    ctx: &mut Ctx,
    obj: &[u32],
    new: &Model,
    part: &Part,
) -> Result<()> {
    let list = field_path(root, obj, "_childGuids")?;
    let old: Vec<Element> = element_at(root, Some(&list))?.children.clone();
    // Keep the unknown entries in order.
    let known = [
        "CPartGuid",
        "CDeformerGuid",
        "CDrawableGuid",
        "CAffecterGuid",
    ];
    let others: Vec<Element> = old
        .into_iter()
        .filter(|e| {
            let u = uuid_of(root, e);
            !known.contains(&e.name.as_str())
                || u.is_none_or(|u| !new_has_guid(new, &u) && !ctx.old_has_guid(&u))
        })
        .collect();
    let mut others = others.into_iter();
    let mut out = Vec::new();
    for c in &part.children {
        let (tag, uuid) = match *c {
            PartChild::Part(i) => ("CPartGuid", new.parts.get(i).map(|x| x.info.guid.clone())),
            PartChild::Deformer(i) => (
                "CDeformerGuid",
                new.deformers.get(i).map(|x| x.info.guid.clone()),
            ),
            PartChild::ArtMesh(i) => (
                "CDrawableGuid",
                new.art_meshes.get(i).map(|x| x.info.guid.clone()),
            ),
            PartChild::Glue(i) => (
                "CAffecterGuid",
                new.glues.get(i).map(|x| x.info.guid.clone()),
            ),
            PartChild::Other => {
                if let Some(e) = others.next() {
                    out.push(e);
                }
                continue;
            }
        };
        let Some(uuid) = uuid else { continue };
        let id = guid_id(root, ctx, &uuid, tag, "")?;
        out.push(stub(tag, None, &id));
    }
    let l = element_at(root, Some(&list))?;
    l.children = out;
    set_count(l);
    Ok(())
}

fn new_has_guid(m: &Model, u: &str) -> bool {
    m.parts.iter().any(|x| x.info.guid == u)
        || m.deformers.iter().any(|x| x.info.guid == u)
        || m.art_meshes.iter().any(|x| x.info.guid == u)
        || m.glues.iter().any(|x| x.info.guid == u)
}

/// Replaces every `xs.ref="from"` in `e` by `to`.
fn retarget(e: &mut Element, from: &str, to: &str) {
    if e.attr("xs.ref") == Some(from) {
        e.set_attr("xs.ref", to.to_owned());
    }
    for c in &mut e.children {
        retarget(c, from, to);
    }
}

/// Every `xs.ref` inside `e`.
pub(super) fn refs_in(e: &Element, out: &mut Vec<String>) {
    if let Some(r) = e.attr("xs.ref") {
        out.push(r.to_owned());
    }
    for c in &e.children {
        refs_in(c, out);
    }
}

fn clear_list(e: &mut Element, name: &str) {
    let Some(d) = holder_depth(e, name) else {
        return;
    };
    let h = super::level_mut(e, d);
    if let Some(l) = h.children.iter_mut().find(|c| c.field_name() == Some(name)) {
        l.children.clear();
        set_count(l);
    }
}

/// Sets field `name` only where the template has it.
fn set_if_present(e: &mut Element, name: &str, tag: &str, text: String) {
    if holder_depth(e, name).is_some() {
        set_scalar(e, name, tag, text);
    }
}

/// A one-entry grid source pointing at form GUID `fg`.
fn single_grid_source(fg: &str) -> Element {
    let mut keys = Element::new("array_list").with_attr("xs.n", "_keyOnParameterList");
    set_count(&mut keys);
    let mut access = Element::new("KeyformGridAccessKey").with_attr("xs.n", "accessKey");
    access.children.push(keys);
    let mut kog = Element::new("KeyformOnGrid");
    kog.children = vec![access, stub("CFormGuid", Some("keyformGuid"), fg)];
    let mut on_grid = Element::new("array_list").with_attr("xs.n", "keyformsOnGrid");
    on_grid.children.push(kog);
    set_count(&mut on_grid);
    let mut bindings = Element::new("array_list").with_attr("xs.n", "keyformBindings");
    set_count(&mut bindings);
    let mut gs = Element::new("KeyformGridSource").with_attr("xs.n", "keyformGridSource");
    gs.children = vec![on_grid, bindings];
    gs
}

fn color_attrs(c: &[f32; 4]) -> Vec<(&'static str, String)> {
    ["red", "green", "blue", "alpha"]
        .iter()
        .zip(c)
        .map(|(k, v)| (*k, format_f32(*v)))
        .collect()
}

/// The first shared element with this tag, as a template.
fn template(root: &Element, tag: &str) -> Result<(String, Element)> {
    shared(root)
        .and_then(|s| s.children.iter().find(|c| c.name == tag))
        .and_then(|e| Some((e.attr("xs.id")?.to_owned(), e.clone())))
        .ok_or_else(|| {
            crate::error::FormatError::Unsupported(format!(
                "saving this change is not supported yet: the first {tag} of a project"
            ))
        })
}

/// Keeps only the first keyform of the clone and points it at the new
/// object and form GUID; returns it for the caller to fill.
fn first_form(e: &mut Element, owner: &str, fg: &str) -> Result<()> {
    let Some(d) = holder_depth(e, "keyforms") else {
        return invalid("template has no keyforms");
    };
    let h = super::level_mut(e, d);
    let Some(l) = h
        .children
        .iter_mut()
        .find(|c| c.field_name() == Some("keyforms"))
    else {
        return invalid("template has no keyforms");
    };
    l.children.truncate(1);
    set_count(l);
    if let Some(f) = l.children.first_mut() {
        f.attrs.retain(|(k, _)| k != "xs.ref");
        if let Some(g) = super::grid::form_guid_field_pub(f) {
            *g = stub("CFormGuid", Some("guid"), fg);
        }
        let _ = owner;
    }
    Ok(())
}

fn first_form_mut(e: &mut Element) -> Option<&mut Element> {
    let d = holder_depth(e, "keyforms")?;
    let h = super::level_mut(e, d);
    h.children
        .iter_mut()
        .find(|c| c.field_name() == Some("keyforms"))?
        .children
        .first_mut()
}

/// Fills the object fields shared by every kind (ACParameterControllableSource).
fn common_fields(
    root: &mut Element,
    ctx: &mut Ctx,
    e: &mut Element,
    info: &keyform_model::ObjectInfo,
    new: &Model,
    fg: &str,
) -> Result<()> {
    set_scalar(e, "localName", "s", xml_text(&info.name));
    set_scalar(e, "isVisible", "b", info.visible.to_string());
    set_scalar(e, "isLocked", "b", info.locked.to_string());
    if let Some(uuid) = part_uuid(new, info.parent_part) {
        let id = guid_id(root, ctx, &uuid, "CPartGuid", "")?;
        if let Some(d) = holder_depth(e, "parentGuid") {
            let h = super::level_mut(e, d);
            if let Some(c) = h
                .children
                .iter_mut()
                .find(|c| c.field_name() == Some("parentGuid"))
            {
                *c = stub("CPartGuid", Some("parentGuid"), &id);
            }
        }
    }
    if let Some(d) = holder_depth(e, "keyformGridSource") {
        let h = super::level_mut(e, d);
        if let Some(c) = h
            .children
            .iter_mut()
            .find(|c| c.field_name() == Some("keyformGridSource"))
        {
            *c = single_grid_source(fg);
        }
    }
    clear_list(e, "_morphTargets");
    clear_list(e, "_constraints");
    clear_list(e, "_extensions");
    Ok(())
}

/// Adds the new object element to its set list and the cache.
fn register(root: &mut Element, ctx: &mut Ctx, set: &str, tag: &str, id: &str) -> Result<()> {
    let main = vec![
        root.children
            .iter()
            .position(|c| c.name == "main")
            .ok_or_else(|| crate::error::FormatError::Invalid("no <main>".into()))? as u32,
        0,
    ];
    let set_path = field_path(root, &main, set)?;
    let set_path = match element_at(root, Some(&set_path))?
        .attr("xs.ref")
        .map(str::to_owned)
    {
        Some(r) => shared_path_of(root, &r)?,
        None => set_path,
    };
    let list = field_path(root, &set_path, "_sources")?;
    let l = element_at(root, Some(&list))?;
    l.children.push(stub(tag, None, id));
    set_count(l);
    let _ = ctx;
    Ok(())
}

/// Clones a template object, rewrites it as `info`/kind and stores it.
/// Returns the new element's path.
#[allow(clippy::too_many_arguments)]
fn new_object(
    root: &mut Element,
    ctx: &mut Ctx,
    new: &Model,
    tag: &str,
    set: &str,
    info: &keyform_model::ObjectInfo,
    id_tag: &str,
    fill: &dyn Fn(&mut Element, &mut Ctx, &mut Element) -> Result<()>,
    first_guid: &str,
) -> Result<ElementPath> {
    let (tid, mut e) = template(root, tag)?;
    e.attrs.retain(|(k, _)| k != "xs.id" && k != "xs.idx");
    // The new object's own GUID object, and its first form GUID.
    let gtag = match id_tag {
        "CPartId" => "CPartGuid",
        _ => "CDeformerGuid",
    };
    let gid = guid_id(root, ctx, &info.guid, gtag, &info.id)?;
    let fg = add_shared(
        root,
        ctx,
        Element::new("CFormGuid")
            .with_attr("uuid", first_guid)
            .with_attr("note", "(no debug info)"),
    )?;
    common_fields(root, ctx, &mut e, info, new, &fg)?;
    // guid and id fields.
    if let Some(d) = holder_depth(&e, "guid") {
        let h = super::level_mut(&mut e, d);
        if let Some(c) = h
            .children
            .iter_mut()
            .find(|c| c.field_name() == Some("guid"))
        {
            *c = stub(&c.name.clone(), Some("guid"), &gid);
        }
    }
    super::set_field_attrs(&mut e, "id", id_tag, &[("idstr", xml_text(&info.id))]);
    first_form(&mut e, &tid, &fg)?;
    fill(root, ctx, &mut e)?;
    let id = add_shared(root, ctx, e)?;
    // Self-references of the template become references to the new object.
    let si = shared_index(root)?;
    let k = shared(root)
        .and_then(|s| {
            s.children
                .iter()
                .position(|c| c.attr("xs.id") == Some(id.as_str()))
        })
        .unwrap_or(0);
    retarget(&mut shared_mut(root)?.children[k], &tid, &id);
    register(root, ctx, set, tag, &id)?;
    Ok(vec![si, k as u32])
}

/// Creates the element of a new part.
pub(super) fn create_part(
    root: &mut Element,
    ctx: &mut Ctx,
    new: &Model,
    p: &Part,
) -> Result<ElementPath> {
    let fg = p.grid.guids.first().cloned().unwrap_or_default();
    let f0 = p.grid.keyforms.first().cloned().unwrap_or_default();
    let not_init = guid_id(
        root,
        ctx,
        NOT_INITIALIZED_GUID,
        "CDeformerGuid",
        "NOT INITIALIZED",
    )?;
    let path = new_object(
        root,
        ctx,
        new,
        "CPartSource",
        "partSourceSet",
        &p.info,
        "CPartId",
        &|_root, _ctx, e| {
            set_scalar(
                e,
                "enableDrawOrderGroup",
                "b",
                p.draw_order_group.to_string(),
            );
            set_scalar(
                e,
                "defaultOrder_forEditor",
                "i",
                p.default_order.to_string(),
            );
            set_scalar(e, "isSketch", "b", p.sketch.to_string());
            clear_list(e, "_childGuids");
            set_if_present(e, "useOffscreen", "b", p.offscreen.to_string());
            clear_list(e, "clipGuidList");
            set_if_present(e, "invertClippingMask", "b", p.invert_clip.to_string());
            if holder_depth(e, "colorComposition").is_some() {
                super::set_field_attrs(
                    e,
                    "colorComposition",
                    "ColorComposition",
                    &[("v", p.color_composition.name().into())],
                );
            }
            if holder_depth(e, "alphaComposition").is_some() {
                super::set_field_attrs(
                    e,
                    "alphaComposition",
                    "AlphaComposition",
                    &[("v", p.alpha_composition.name().into())],
                );
            }
            if let Some(d) = holder_depth(e, "targetDeformerGuid") {
                let h = super::level_mut(e, d);
                if let Some(c) = h
                    .children
                    .iter_mut()
                    .find(|c| c.field_name() == Some("targetDeformerGuid"))
                {
                    *c = stub("CDeformerGuid", Some("targetDeformerGuid"), &not_init);
                }
            }
            if let Some(f) = first_form_mut(e) {
                set_scalar(
                    f,
                    "drawOrder",
                    "i",
                    (f0.draw_order.round() as i32).to_string(),
                );
                set_scalar(f, "opacity", "f", format_f32(f0.opacity));
                super::set_field_attrs(
                    f,
                    "multiplyColor",
                    "CFloatColor",
                    &color_attrs(&f0.multiply),
                );
                super::set_field_attrs(f, "screenColor", "CFloatColor", &color_attrs(&f0.screen));
            }
            Ok(())
        },
        &fg,
    )?;
    write_children(root, ctx, &path, new, p)?;
    finish_grid(root, ctx, &path, &p.info.id, &p.grid, &p.blend_shapes)?;
    Ok(path)
}

/// Creates the element of a new deformer.
pub(super) fn create_deformer(
    root: &mut Element,
    ctx: &mut Ctx,
    new: &Model,
    d: &Deformer,
) -> Result<ElementPath> {
    let target = target_uuid(new, d.target);
    let tid = guid_id(
        root,
        ctx,
        &target,
        "CDeformerGuid",
        if d.target.is_none() { "ROOT" } else { "" },
    )?;
    let coord = coord_id(
        root,
        ctx,
        if d.target.is_none() {
            "Canvas"
        } else {
            "DeformerLocal"
        },
    )?;
    let set_target = |e: &mut Element| {
        if let Some(dp) = holder_depth(e, "targetDeformerGuid") {
            let h = super::level_mut(e, dp);
            if let Some(c) = h
                .children
                .iter_mut()
                .find(|c| c.field_name() == Some("targetDeformerGuid"))
            {
                *c = stub("CDeformerGuid", Some("targetDeformerGuid"), &tid);
            }
        }
    };
    let deformer_form = |f: &mut Element, opacity: f32, mul: &[f32; 4], scr: &[f32; 4]| {
        set_scalar(f, "opacity", "f", format_f32(opacity));
        super::set_field_attrs(f, "multiplyColor", "CFloatColor", &color_attrs(mul));
        super::set_field_attrs(f, "screenColor", "CFloatColor", &color_attrs(scr));
        if let Some(dp) = holder_depth(f, "coordType") {
            let h = super::level_mut(f, dp);
            if let Some(c) = h
                .children
                .iter_mut()
                .find(|c| c.field_name() == Some("coordType"))
            {
                *c = stub("CoordType", Some("coordType"), &coord);
            }
        }
    };
    match &d.kind {
        DeformerKind::Rotation(r) => {
            let fg = r.grid.guids.first().cloned().unwrap_or_default();
            let f0 = r.grid.keyforms.first().cloned().unwrap_or_default();
            let path = new_object(
                root,
                ctx,
                new,
                "CRotationDeformerSource",
                "deformerSourceSet",
                &d.info,
                "CDeformerId",
                &|_root, _ctx, e| {
                    set_target(e);
                    set_scalar(e, "baseAngle", "f", format_f32(r.base_angle));
                    set_scalar(e, "handleLengthOnCanvas", "f", format_f32(r.handle_length));
                    set_scalar(e, "circleRadiusOnCanvas", "f", format_f32(r.circle_radius));
                    if let Some(f) = first_form_mut(e) {
                        for (k, v) in [
                            ("angle", format_f32(f0.angle)),
                            ("originX", format_f32(f0.origin[0])),
                            ("originY", format_f32(f0.origin[1])),
                            ("scale", format_f32(f0.scale)),
                            ("isReflectX", f0.reflect_x.to_string()),
                            ("isReflectY", f0.reflect_y.to_string()),
                        ] {
                            set_scalar(f, k, "f", v);
                        }
                        deformer_form(f, f0.opacity, &f0.multiply, &f0.screen);
                    }
                    Ok(())
                },
                &fg,
            )?;
            finish_grid(root, ctx, &path, &d.info.id, &r.grid, &r.blend_shapes)?;
            Ok(path)
        }
        DeformerKind::Warp(w) => {
            let fg = w.grid.guids.first().cloned().unwrap_or_default();
            let f0 = w.grid.keyforms.first().cloned().unwrap_or_default();
            let path = new_object(
                root,
                ctx,
                new,
                "CWarpDeformerSource",
                "deformerSourceSet",
                &d.info,
                "CDeformerId",
                &|_root, _ctx, e| {
                    set_target(e);
                    set_scalar(e, "col", "i", w.cols.to_string());
                    set_scalar(e, "row", "i", w.rows.to_string());
                    set_scalar(e, "isQuadTransform", "b", w.quad.to_string());
                    if let Some(f) = first_form_mut(e) {
                        super::set_array(f, "positions", "float-array", &f0.positions);
                        deformer_form(f, f0.opacity, &f0.multiply, &f0.screen);
                    }
                    Ok(())
                },
                &fg,
            )?;
            // Bezier extensions (warp-deformer-editing.md): one per level.
            let owner = match element_at(root, Some(&path))?.attr("xs.id") {
                Some(i) => i.to_owned(),
                None => return invalid("new deformer has no id"),
            };
            let mut exts = Vec::new();
            for &(level, c, r) in w.bezier_nets.iter().chain(
                keyform_model::DEFAULT_BEZIER_NETS
                    .iter()
                    .filter(|n| !w.bezier_nets.iter().any(|m| m.0 == n.0)),
            ) {
                let mut sup = Element::new("ACExtension").with_attr("xs.n", "super");
                sup.children = vec![
                    Element::new("CExtensionGuid")
                        .with_attr("xs.n", "guid")
                        .with_attr("uuid", keyform_model::new_guid())
                        .with_attr("note", "(no debug info)"),
                    stub("CWarpDeformerSource", Some("_owner"), &owner),
                ];
                let mut x = Element::new("CWarpDeformerBezierExtension");
                x.children = vec![
                    sup,
                    scalar("i", "editLevel", level.to_string()),
                    scalar("i", "bezierCol", c.to_string()),
                    scalar("i", "bezierRow", r.to_string()),
                ];
                exts.push(x);
            }
            let ep = field_path(root, &path, "_extensions")?;
            let l = element_at(root, Some(&ep))?;
            l.children = exts;
            set_count(l);
            finish_grid(root, ctx, &path, &d.info.id, &w.grid, &w.blend_shapes)?;
            Ok(path)
        }
    }
}

/// A new object was written with one keyform; richer grids are rebuilt.
fn finish_grid<F: super::FormFields>(
    root: &mut Element,
    ctx: &mut Ctx,
    path: &ElementPath,
    what: &str,
    grid: &KeyformGrid<F>,
    blend: &keyform_model::BlendShapes<F>,
) -> Result<()> {
    if !blend.is_empty() {
        return unsupported(format!("{what}: blend shapes on a new object"));
    }
    if grid.bindings.is_empty() && grid.keyforms.len() == 1 {
        return Ok(());
    }
    let first = KeyformGrid {
        bindings: Vec::new(),
        keyforms: grid.keyforms[..1].to_vec(),
        guids: grid.guids[..1].to_vec(),
    };
    let src = ObjectSource {
        element: Some(path.clone()),
        cells: Vec::new(),
        blend: Vec::new(),
    };
    super::grid::rebuild_grid(root, ctx, &src, what, &first, grid)
}

/// Deletes an object element: its set-list stub goes, and the element and
/// everything it referred to become orphan candidates.
pub(super) fn delete_object(
    root: &mut Element,
    ctx: &mut Ctx,
    path: &[u32],
    set: &str,
) -> Result<()> {
    let e = element_at(root, Some(path))?.clone();
    let Some(id) = e.attr("xs.id").map(str::to_owned) else {
        return unsupported("deleting an object that is not shared");
    };
    let main = vec![
        root.children
            .iter()
            .position(|c| c.name == "main")
            .ok_or_else(|| crate::error::FormatError::Invalid("no <main>".into()))? as u32,
        0,
    ];
    let set_path = field_path(root, &main, set)?;
    let set_path = match element_at(root, Some(&set_path))?
        .attr("xs.ref")
        .map(str::to_owned)
    {
        Some(r) => shared_path_of(root, &r)?,
        None => set_path,
    };
    let list = field_path(root, &set_path, "_sources")?;
    let l = element_at(root, Some(&list))?;
    l.children.retain(|c| c.attr("xs.ref") != Some(id.as_str()));
    set_count(l);
    ctx.orphans.push(id);
    refs_in(&e, &mut ctx.orphans);
    ctx.structural = true;
    let _ = (get_shared, find_shared);
    Ok(())
}

/// GUID → index maps of a model's parts and deformers.
pub(super) fn index_by_guid<T>(
    list: &[std::sync::Arc<T>],
    guid: impl Fn(&T) -> &str,
) -> HashMap<String, usize> {
    list.iter()
        .enumerate()
        .map(|(i, x)| (guid(x).to_owned(), i))
        .collect()
}
