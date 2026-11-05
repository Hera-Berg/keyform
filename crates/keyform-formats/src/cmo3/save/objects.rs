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
/// Deformer prototypes, in the layout of the newest sample projects.
const WARP_PROTOTYPE: &str = r##"<CWarpDeformerSource xs.id="#PROTO">
<ACDeformerSource xs.n="super">
<ACParameterControllableSource xs.n="super">
<s xs.n="localName">Deformer</s>
<b xs.n="isVisible">true</b>
<b xs.n="isLocked">false</b>
<null xs.n="parentGuid" />
<KeyformGridSource xs.n="keyformGridSource" />
<KeyFormMorphTargetSet xs.n="keyformMorphTargetSet">
<carray_list xs.n="_morphTargets" count="0" />
<MorphTargetBlendWeightConstraintSet xs.n="blendWeightConstraintSet">
<carray_list xs.n="_constraints" count="0" />
</MorphTargetBlendWeightConstraintSet>
</KeyFormMorphTargetSet>
<carray_list xs.n="_extensions" count="0" />
<null xs.n="internalColor_direct_argb" />
<CLabelColor xs.n="labelColor" customizedColorInt="-1">
<CLabelColorType xs.n="labelType" v="UNDEFINED" />
</CLabelColor>
</ACParameterControllableSource>
<CDeformerId xs.n="id" idstr="Deformer" />
<null xs.n="guid" />
<null xs.n="targetDeformerGuid" />
</ACDeformerSource>
<i xs.n="col">5</i>
<i xs.n="row">5</i>
<b xs.n="isQuadTransform">false</b>
<carray_list xs.n="keyforms" count="1">
<CWarpDeformerForm>
<ACDeformerForm xs.n="super">
<ACForm xs.n="super">
<null xs.n="guid" />
<b xs.n="isAnimatedForm">false</b>
<b xs.n="isLocalAnimatedForm">false</b>
<CWarpDeformerSource xs.n="_source" xs.ref="#PROTO" />
<null xs.n="name" />
<s xs.n="notes" />
</ACForm>
<f xs.n="opacity">1.0</f>
<CFloatColor xs.n="multiplyColor" red="1.0" green="1.0" blue="1.0" alpha="1.0" />
<CFloatColor xs.n="screenColor" red="0.0" green="0.0" blue="0.0" alpha="1.0" />
<null xs.n="coordType" />
</ACDeformerForm>
<float-array xs.n="positions" count="0" />
</CWarpDeformerForm>
</carray_list>
</CWarpDeformerSource>"##;

const ROTATION_PROTOTYPE: &str = r##"<CRotationDeformerSource xs.id="#PROTO">
<ACDeformerSource xs.n="super">
<ACParameterControllableSource xs.n="super">
<s xs.n="localName">Deformer</s>
<b xs.n="isVisible">true</b>
<b xs.n="isLocked">false</b>
<null xs.n="parentGuid" />
<KeyformGridSource xs.n="keyformGridSource" />
<KeyFormMorphTargetSet xs.n="keyformMorphTargetSet">
<carray_list xs.n="_morphTargets" count="0" />
<MorphTargetBlendWeightConstraintSet xs.n="blendWeightConstraintSet">
<carray_list xs.n="_constraints" count="0" />
</MorphTargetBlendWeightConstraintSet>
</KeyFormMorphTargetSet>
<carray_list xs.n="_extensions" count="0" />
<null xs.n="internalColor_direct_argb" />
<CLabelColor xs.n="labelColor" customizedColorInt="-1">
<CLabelColorType xs.n="labelType" v="UNDEFINED" />
</CLabelColor>
</ACParameterControllableSource>
<CDeformerId xs.n="id" idstr="Deformer" />
<null xs.n="guid" />
<null xs.n="targetDeformerGuid" />
</ACDeformerSource>
<b xs.n="useBoneUi_testImpl">true</b>
<carray_list xs.n="keyforms" count="1">
<CRotationDeformerForm angle="0.0" originX="0.0" originY="0.0" scale="1.0" isReflectX="false" isReflectY="false">
<ACDeformerForm xs.n="super">
<ACForm xs.n="super">
<null xs.n="guid" />
<b xs.n="isAnimatedForm">false</b>
<b xs.n="isLocalAnimatedForm">false</b>
<CRotationDeformerSource xs.n="_source" xs.ref="#PROTO" />
<null xs.n="name" />
<s xs.n="notes" />
</ACForm>
<f xs.n="opacity">1.0</f>
<CFloatColor xs.n="multiplyColor" red="1.0" green="1.0" blue="1.0" alpha="1.0" />
<CFloatColor xs.n="screenColor" red="0.0" green="0.0" blue="0.0" alpha="1.0" />
<null xs.n="coordType" />
</ACDeformerForm>
</CRotationDeformerForm>
</carray_list>
<f xs.n="handleLengthOnCanvas">50.0</f>
<f xs.n="circleRadiusOnCanvas">100.0</f>
<f xs.n="baseAngle">0.0</f>
</CRotationDeformerSource>"##;

/// A built-in element for a kind the document has no example of (a new
/// model), in the layout the reference editor writes for new objects
/// (observed in a new model it saved). Every reference in it is replaced
/// when the object is filled in.
fn prototype(tag: &str) -> Option<Element> {
    let xml = match tag {
        "CArtMeshSource" => {
            r##"<CArtMeshSource xs.id="#PROTO">
<ACDrawableSource xs.n="super">
<ACParameterControllableSource xs.n="super">
<s xs.n="localName">ArtMesh</s>
<b xs.n="isVisible">true</b>
<b xs.n="isLocked">false</b>
<null xs.n="parentGuid" />
<KeyformGridSource xs.n="keyformGridSource" />
<KeyFormMorphTargetSet xs.n="keyformMorphTargetSet">
<carray_list xs.n="_morphTargets" count="0" />
<MorphTargetBlendWeightConstraintSet xs.n="blendWeightConstraintSet">
<carray_list xs.n="_constraints" count="0" />
</MorphTargetBlendWeightConstraintSet>
</KeyFormMorphTargetSet>
<carray_list xs.n="_extensions" count="0" />
<null xs.n="internalColor_direct_argb" />
<CLabelColor xs.n="labelColor" customizedColorInt="-1">
<CLabelColorType xs.n="labelType" v="UNDEFINED" />
</CLabelColor>
</ACParameterControllableSource>
<CDrawableId xs.n="id" idstr="ArtMesh" />
<null xs.n="guid" />
<null xs.n="targetDeformerGuid" />
<carray_list xs.n="clipGuidList" count="0" />
<b xs.n="invertClippingMask">false</b>
<null xs.n="icon32" />
<null xs.n="icon16" />
</ACDrawableSource>
<int-array xs.n="indices" count="0" />
<carray_list xs.n="keyforms" count="1">
<CArtMeshForm>
<ACDrawableForm xs.n="super">
<ACForm xs.n="super">
<null xs.n="guid" />
<b xs.n="isAnimatedForm">false</b>
<b xs.n="isLocalAnimatedForm">false</b>
<CArtMeshSource xs.n="_source" xs.ref="#PROTO" />
<null xs.n="name" />
<s xs.n="notes" />
</ACForm>
<i xs.n="drawOrder">500</i>
<f xs.n="opacity">1.0</f>
<CFloatColor xs.n="multiplyColor" red="1.0" green="1.0" blue="1.0" alpha="1.0" />
<CFloatColor xs.n="screenColor" red="0.0" green="0.0" blue="0.0" alpha="1.0" />
<null xs.n="coordType" />
</ACDrawableForm>
<float-array xs.n="positions" count="0" />
</CArtMeshForm>
</carray_list>
<float-array xs.n="positions" count="0" />
<float-array xs.n="uvs" count="0" />
<null xs.n="texture" />
<ColorComposition xs.n="colorComposition" v="NORMAL" />
<b xs.n="culling">false</b>
<TextureState xs.n="textureState" v="MODEL_IMAGE" />
<s xs.n="userData" />
<AlphaComposition xs.n="alphaComposition" v="OVER" />
</CArtMeshSource>"##
        }
        "CWarpDeformerSource" => WARP_PROTOTYPE,
        "CRotationDeformerSource" => ROTATION_PROTOTYPE,
        _ => return None,
    };
    crate::xml::XmlDocument::parse(xml.as_bytes())
        .ok()
        .map(|d| d.root)
}

fn template(root: &Element, tag: &str) -> Result<(String, Element)> {
    shared(root)
        .and_then(|s| s.children.iter().find(|c| c.name == tag))
        .and_then(|e| Some((e.attr("xs.id")?.to_owned(), e.clone())))
        .or_else(|| prototype(tag).map(|e| ("#PROTO".to_owned(), e)))
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
pub(super) fn register(
    root: &mut Element,
    ctx: &mut Ctx,
    set: &str,
    tag: &str,
    id: &str,
) -> Result<()> {
    let _ = ctx;
    register_at(root, set, tag, id, usize::MAX)
}

/// Adds a stub for object `id` to set list `set` at position `pos`
/// (clamped to the end).
pub(super) fn register_at(
    root: &mut Element,
    set: &str,
    tag: &str,
    id: &str,
    pos: usize,
) -> Result<()> {
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
    let at = pos.min(l.children.len());
    l.children.insert(at, stub(tag, None, id));
    set_count(l);
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
        "CDrawableId" => "CDrawableGuid",
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
            *c = stub(gtag, Some("guid"), &gid);
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

/// Classes a new glue element uses (files without glue lack them).
pub(super) const GLUE_IMPORTS: [&str; 8] = [
    "com.live2d.cubism.doc.model.affecter.ACAffecterForm",
    "com.live2d.cubism.doc.model.affecter.ACAffecterSource",
    "com.live2d.cubism.doc.model.affecter.glue.CGlueForm",
    "com.live2d.cubism.doc.model.affecter.glue.CGlueSource",
    "com.live2d.cubism.doc.model.id.CAffecterId",
    "com.live2d.graphics3d.type.GVector2",
    "com.live2d.type.CAffecterGuid",
    "com.live2d.type.CDrawableGuid",
];

/// Creates the element of a new art mesh (cloned from an existing one for
/// the document's layout, then rewritten): mesh, UVs, keyform, texture
/// (a new atlas page of this save), editable mesh, texture input and mesh
/// generator extensions, icons. spec: psd-import.md §5.3,
/// texture-atlas.md §4, cmo3.md §4
pub(super) fn create_art_mesh(
    root: &mut Element,
    ctx: &mut Ctx,
    archive: &crate::caff::CaffArchive,
    new: &Model,
    a: &keyform_model::ArtMesh,
) -> Result<ElementPath> {
    use super::textures::{affine_el, icon};
    let Some(mi) = a.model_image.and_then(|k| new.model_images.get(k)) else {
        return unsupported(format!("art mesh {}: no model image", a.info.id));
    };
    // Atlas mode: the atlas page's texture; model-image mode: a texture of
    // its own on the model image (texture-atlas.md §4).
    let atlas = match a.texture {
        Some(t) => {
            let Some((_, gtex)) = ctx.tex.atlases.get(&t).cloned() else {
                return unsupported(format!("art mesh {}: a texture made earlier", a.info.id));
            };
            let Some(entry) = new.textures[t]
                .entries
                .iter()
                .find(|e| Some(e.model_image) == a.model_image)
            else {
                return unsupported(format!(
                    "art mesh {}: its image is not on its atlas",
                    a.info.id
                ));
            };
            Some((t, gtex, *entry))
        }
        None => None,
    };
    let gtex = match &atlas {
        Some((_, g, _)) => g.clone(),
        None => {
            let Some((_, res)) = mi
                .source
                .as_ref()
                .and_then(|(_, layer)| ctx.tex.layers.get(layer).cloned())
            else {
                return unsupported(format!(
                    "art mesh {}: a model image made earlier",
                    a.info.id
                ));
            };
            let size = mi
                .png
                .as_deref()
                .and_then(crate::image::png_size)
                .unwrap_or((1, 1));
            super::textures::model_image_texture(root, ctx, &a.info.name, &res, size)?
        }
    };
    let fg = a.grid.guids.first().cloned().unwrap_or_default();
    let f0 = a.grid.keyforms.first().cloned().unwrap_or_default();
    let img = mi
        .png
        .as_deref()
        .map(crate::image::RgbaImage::decode_png)
        .transpose()?
        .unwrap_or_else(|| crate::image::RgbaImage::new(1, 1));
    let icon32 = icon(archive, &mut ctx.tex, "icon32", &img, 32)?;
    let icon16 = icon(archive, &mut ctx.tex, "icon16", &img, 16)?;
    let path = new_object(
        root,
        ctx,
        new,
        "CArtMeshSource",
        "drawableSourceSet",
        &a.info,
        "CDrawableId",
        &|_root, _ctx, e| {
            super::set_array(e, "positions", "float-array", &a.positions);
            super::set_array(e, "uvs", "float-array", &a.uvs);
            super::set_array_text(
                e,
                "indices",
                "int-array",
                a.indices.len(),
                a.indices
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            clear_list(e, "clipGuidList");
            set_scalar(e, "invertClippingMask", "b", a.invert_clip.to_string());
            set_scalar(e, "culling", "b", a.culling.to_string());
            set_scalar(e, "userData", "s", xml_text(&a.user_data));
            super::set_field_attrs(
                e,
                "colorComposition",
                "ColorComposition",
                &[("v", a.color_composition.name().into())],
            );
            if holder_depth(e, "alphaComposition").is_some() {
                super::set_field_attrs(
                    e,
                    "alphaComposition",
                    "AlphaComposition",
                    &[("v", a.alpha_composition.name().into())],
                );
            }
            let state = if atlas.is_some() {
                "TEXTURE_ATLAS"
            } else {
                "MODEL_IMAGE"
            };
            super::set_field_attrs(e, "textureState", "TextureState", &[("v", state.into())]);
            for (name, ic) in [("icon32", &icon32), ("icon16", &icon16)] {
                if let Some(d) = holder_depth(e, name) {
                    let h = super::level_mut(e, d);
                    if let Some(c) = h.children.iter_mut().find(|c| c.field_name() == Some(name)) {
                        *c = (*ic).clone();
                    }
                }
            }
            if let Some(f) = first_form_mut(e) {
                super::set_array(f, "positions", "float-array", &f0.positions);
                set_scalar(
                    f,
                    "drawOrder",
                    "i",
                    (f0.draw_order.round() as i32).to_string(),
                );
                set_scalar(f, "opacity", "f", format_f32(f0.opacity));
                let col = |c: &keyform_model::Color| {
                    ["red", "green", "blue", "alpha"]
                        .iter()
                        .zip(c)
                        .map(|(k, v)| (*k, format_f32(*v)))
                        .collect::<Vec<_>>()
                };
                super::set_field_attrs(f, "multiplyColor", "CFloatColor", &col(&f0.multiply));
                super::set_field_attrs(f, "screenColor", "CFloatColor", &col(&f0.screen));
            }
            Ok(())
        },
        &fg,
    )?;
    let me = element_at(root, Some(&path))?
        .attr("xs.id")
        .map(str::to_owned)
        .unwrap_or_default();
    // The texture.
    {
        let e = element_at(root, Some(&path))?;
        if let Some(d) = holder_depth(e, "texture") {
            let h = super::level_mut(e, d);
            if let Some(c) = h
                .children
                .iter_mut()
                .find(|c| c.field_name() == Some("texture"))
            {
                *c = stub("GTexture2D", Some("texture"), &gtex);
            }
        }
    }
    write_target(root, ctx, &path, new, a.target)?;
    // Clip sources.
    let mut clips = Vec::new();
    for &k in &a.clip {
        if let Some(c) = new.art_meshes.get(k) {
            let id = guid_id(
                root,
                ctx,
                &c.info.guid,
                "CDrawableGuid",
                c.info.display_name(),
            )?;
            clips.push(stub("CDrawableGuid", None, &id));
        }
    }
    if !clips.is_empty() {
        let cl = field_path(root, &path, "clipGuidList")?;
        let l = element_at(root, Some(&cl))?;
        l.children = clips;
        set_count(l);
    }
    // Extensions.
    let basic = coord_id(root, ctx, "Basic Coord")?;
    let ext_guid = || {
        Element::new("CExtensionGuid")
            .with_attr("xs.n", "guid")
            .with_attr("uuid", keyform_model::new_guid())
            .with_attr("note", "(no debug info)")
    };
    let sup = |guid: Element| {
        let mut e = Element::new("ACExtension").with_attr("xs.n", "super");
        e.children = vec![guid, stub("CArtMeshSource", Some("_owner"), &me)];
        e
    };
    let m = &a.mesh;
    let join = |v: Vec<String>| v.join(" ");
    let arr = |tag: &str, name: &str, n: usize, text: String| {
        let mut e = Element::new(tag)
            .with_attr("xs.n", name)
            .with_attr("count", n.to_string());
        e.text = text;
        e
    };
    let mut prio = m.point_priority.clone();
    prio.resize(a.vertex_count(), keyform_model::POINT_NORMAL);
    let mut gem = Element::new("GEditableMesh2")
        .with_attr("xs.n", "editableMesh")
        .with_attr("nextPointUid", m.next_point_uid.to_string())
        .with_attr("useDelaunayTriangulation", m.delaunay.to_string());
    gem.children = vec![
        arr(
            "float-array",
            "point",
            a.positions.len(),
            join(a.positions.iter().map(|v| format_f32(*v)).collect()),
        ),
        arr(
            "byte-array",
            "pointPriority",
            prio.len(),
            join(prio.iter().map(|v| v.to_string()).collect()),
        ),
        arr(
            "short-array",
            "edge",
            2 * m.edges.len(),
            join(
                m.edges
                    .iter()
                    .flatten()
                    .map(|&v| (v as u16 as i16).to_string())
                    .collect(),
            ),
        ),
        arr(
            "byte-array",
            "edgePriority",
            m.edge_priority.len(),
            join(m.edge_priority.iter().map(|v| v.to_string()).collect()),
        ),
        arr(
            "int-array",
            "pointUid",
            a.point_uids.len(),
            join(a.point_uids.iter().map(|v| v.to_string()).collect()),
        ),
        Element::new("GEditableMeshGuid")
            .with_attr("xs.n", "meshGuid")
            .with_attr("uuid", keyform_model::new_guid())
            .with_attr("note", "(no debug info)"),
        stub("CoordType", Some("coordType"), &basic),
    ];
    let mut editable = Element::new("CEditableMeshExtension");
    editable.children = vec![sup(ext_guid()), gem, scalar("b", "isLocked", "false")];
    // Texture inputs: the model image, and the atlas region (current).
    let ext = add_shared(root, ctx, Element::new("CTextureInputExtension"))?;
    let input_super = |ext: &str| {
        let mut e = Element::new("ACTextureInput").with_attr("xs.n", "super");
        e.children = vec![
            affine_el("optionalTransformOnCanvas", [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
            stub("CTextureInputExtension", Some("_owner"), ext),
        ];
        e
    };
    let mg = guid_id(root, ctx, &mi.guid, "CModelImageGuid", "(no debug info)")?;
    let mut mi_input = Element::new("CTextureInput_ModelImage");
    mi_input.children = vec![
        input_super(&ext),
        stub("CModelImageGuid", Some("_modelImageGuid"), &mg),
    ];
    let mut inputs = Element::new("carray_list").with_attr("xs.n", "_textureInputs");
    let current = match &atlas {
        Some((t, _, entry)) => {
            let tg = guid_id(
                root,
                ctx,
                &new.textures[*t].guid,
                "CTextureAtlasGuid",
                "(no debug info)",
            )?;
            let mut region = Element::new("CTextureInput_TextureAtlasRegion");
            region.children = vec![
                input_super(&ext),
                stub("CTextureAtlasGuid", Some("textureAtlasGuid"), &tg),
                affine_el("inputImageLocalToCanvasTransform", entry.atlas_to_canvas),
            ];
            let region = add_shared(root, ctx, region)?;
            inputs.children = vec![
                mi_input,
                stub("CTextureInput_TextureAtlasRegion", None, &region),
            ];
            stub(
                "CTextureInput_TextureAtlasRegion",
                Some("currentTextureInputData"),
                &region,
            )
        }
        None => {
            let input = add_shared(root, ctx, mi_input)?;
            inputs.children = vec![stub("CTextureInput_ModelImage", None, &input)];
            stub(
                "CTextureInput_ModelImage",
                Some("currentTextureInputData"),
                &input,
            )
        }
    };
    set_count(&mut inputs);
    find_shared(root, &ext)?.children = vec![sup(ext_guid()), inputs, current];
    let g = &a.generator;
    let mut setting = Element::new("MeshGenerateSetting").with_attr("xs.n", "meshGenerateSetting");
    setting.children = [
        ("polygonOuterDensity", g.outer_density),
        ("polygonInnerDensity", g.inner_density),
        ("polygonMargin", g.margin),
        ("polygonInnerMargin", g.inner_margin),
        ("polygonMinMargin", g.min_margin),
        ("polygonMinBoundsPt", g.min_bounds_pt),
        ("thresholdAlpha", g.threshold_alpha),
    ]
    .iter()
    .map(|(n, v)| scalar("i", n, v.to_string()))
    .collect();
    let mut generator = Element::new("CMeshGeneratorExtension");
    generator.children = vec![sup(ext_guid()), setting];
    let ep = field_path(root, &path, "_extensions")?;
    let l = element_at(root, Some(&ep))?;
    l.children = vec![
        editable,
        stub("CTextureInputExtension", None, &ext),
        generator,
    ];
    set_count(l);
    finish_grid(root, ctx, &path, &a.info.id, &a.grid, &a.blend_shapes)?;
    Ok(path)
}

/// Vertex UIDs of glue `g`'s pairs in `m`, interleaved (uidA, uidB).
pub(super) fn glue_uids(m: &Model, g: &keyform_model::Glue) -> Option<Vec<i64>> {
    let uid = |mesh: Option<usize>, v: Option<usize>| -> Option<i64> {
        let a = m.art_meshes.get(mesh?)?;
        let v = v?;
        Some(a.point_uids.get(v).copied().unwrap_or(v as i64))
    };
    g.pairs
        .iter()
        .flat_map(|&(a, b)| [uid(g.mesh_a, a), uid(g.mesh_b, b)])
        .collect()
}

fn text_array(tag: &str, name: &str, values: &[String]) -> Element {
    let mut e = Element::new(tag)
        .with_attr("xs.n", name)
        .with_attr("count", values.len().to_string());
    e.text = values.join(" ");
    e
}

/// Writes glue `g`'s pairs and weights (`bindVertexUids`, `weights`).
pub(super) fn write_glue_pairs(
    root: &mut Element,
    path: &[u32],
    new: &Model,
    g: &keyform_model::Glue,
) -> Result<()> {
    let Some(uids) = glue_uids(new, g) else {
        return unsupported(format!("{}: a glue pair has an unknown vertex", g.info.id));
    };
    let weights: Vec<f32> = g.weights.iter().flatten().copied().collect();
    let e = element_at(root, Some(path))?;
    super::set_array(e, "weights", "float-array", &weights);
    super::set_array_text(
        e,
        "bindVertexUids",
        "long-array",
        uids.len(),
        uids.iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(" "),
    );
    Ok(())
}

/// Builds the element of a new glue (`CGlueSource`, spec: cmo3-schema.md;
/// tab position per glue.md "New glue object").
pub(super) fn create_glue(
    root: &mut Element,
    ctx: &mut Ctx,
    new: &Model,
    g: &keyform_model::Glue,
) -> Result<ElementPath> {
    let mesh_guid = |k: Option<usize>| -> Result<(String, String)> {
        let a = k
            .and_then(|k| new.art_meshes.get(k))
            .ok_or_else(|| crate::error::FormatError::Invalid("glue without its meshes".into()))?;
        Ok((a.info.guid.clone(), a.info.id.clone()))
    };
    let (ga, na) = mesh_guid(g.mesh_a)?;
    let (gb, nb) = mesh_guid(g.mesh_b)?;
    let Some(part) = part_uuid(new, g.info.parent_part) else {
        return unsupported("a glue without a part");
    };
    let Some(uids) = glue_uids(new, g) else {
        return unsupported(format!("{}: a glue pair has an unknown vertex", g.info.id));
    };
    let gid = guid_id(root, ctx, &g.info.guid, "CAffecterGuid", &g.info.id)?;
    let aid = guid_id(root, ctx, &ga, "CDrawableGuid", &na)?;
    let bid = guid_id(root, ctx, &gb, "CDrawableGuid", &nb)?;
    let pid = guid_id(root, ctx, &part, "CPartGuid", "")?;
    let tid = guid_id(root, ctx, &target_uuid(new, None), "CDeformerGuid", "ROOT")?;
    let fg_uuid = g
        .grid
        .guids
        .first()
        .cloned()
        .unwrap_or_else(keyform_model::new_guid);
    let fg = add_shared(
        root,
        ctx,
        Element::new("CFormGuid")
            .with_attr("uuid", fg_uuid)
            .with_attr("note", "(no debug info)"),
    )?;
    let me = "#KEYFORM-NEW-GLUE";
    let f0 = g.grid.keyforms.first().cloned().unwrap_or_default();
    // Tab: y at the first pair's midpoint, x at the canvas edge on that side.
    let mid = g.pairs.first().and_then(|&(a, b)| {
        let p = |k: Option<usize>, v: Option<usize>| -> Option<[f32; 2]> {
            let m = new.art_meshes.get(k?)?;
            let v = v?;
            Some([*m.positions.get(2 * v)?, *m.positions.get(2 * v + 1)?])
        };
        let (pa, pb) = (p(g.mesh_a, a)?, p(g.mesh_b, b)?);
        Some([(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0])
    });
    let w = new.canvas.width as f32;
    let tab = mid.map_or([-100.0, 0.0], |c| {
        [if c[0] > w / 2.0 { w } else { -100.0 }, c[1]]
    });

    let empty = |tag: &str, name: &str| {
        let mut l = Element::new(tag).with_attr("xs.n", name);
        set_count(&mut l);
        l
    };
    let mut morph =
        Element::new("KeyFormMorphTargetSet").with_attr("xs.n", "keyformMorphTargetSet");
    let mut cons = Element::new("MorphTargetBlendWeightConstraintSet")
        .with_attr("xs.n", "blendWeightConstraintSet");
    cons.children.push(empty("carray_list", "_constraints"));
    morph.children = vec![empty("carray_list", "_morphTargets"), cons];
    let mut pcs = Element::new("ACParameterControllableSource").with_attr("xs.n", "super");
    pcs.children = vec![
        scalar("s", "localName", xml_text(&g.info.name)),
        scalar("b", "isVisible", g.info.visible.to_string()),
        scalar("b", "isLocked", g.info.locked.to_string()),
        stub("CPartGuid", Some("parentGuid"), &pid),
        single_grid_source(&fg),
        morph,
        empty("carray_list", "_extensions"),
        Element::new("null").with_attr("xs.n", "internalColor_direct_argb"),
        Element::new("null").with_attr("xs.n", "internalColor_indirect_argb"),
    ];
    let mut aff = Element::new("ACAffecterSource").with_attr("xs.n", "super");
    aff.children = vec![
        pcs,
        stub("CAffecterGuid", Some("guid"), &gid),
        Element::new("CAffecterId")
            .with_attr("xs.n", "id")
            .with_attr("idstr", xml_text(&g.info.id)),
        stub("CDeformerGuid", Some("targetDeformerGuid"), &tid),
        scalar("i", "editVersion", "0"),
    ];
    // The keyform.
    let mut acform = Element::new("ACForm").with_attr("xs.n", "super");
    acform.children = vec![
        stub("CFormGuid", Some("guid"), &fg),
        scalar("b", "isAnimatedForm", "false"),
        scalar("b", "isLocalAnimatedForm", "false"),
        stub("CGlueSource", Some("_source"), me),
        Element::new("null").with_attr("xs.n", "name"),
        scalar("s", "notes", ""),
    ];
    let mut affform = Element::new("ACAffecterForm").with_attr("xs.n", "super");
    affform.children = vec![acform];
    let mut form = Element::new("CGlueForm");
    form.children = vec![affform, scalar("f", "intensity", format_f32(f0.intensity))];
    let mut forms = Element::new("carray_list").with_attr("xs.n", "keyforms");
    forms.children.push(form);
    set_count(&mut forms);
    let mut tabv = Element::new("GVector2").with_attr("xs.n", "tabPosOnCanvas");
    tabv.children = vec![
        scalar("f", "x", format_f32(tab[0])),
        scalar("f", "y", format_f32(tab[1])),
    ];
    let weights: Vec<String> = g.weights.iter().flatten().map(|w| format_f32(*w)).collect();
    let uid_text: Vec<String> = uids.iter().map(|u| u.to_string()).collect();
    let mut e = Element::new("CGlueSource");
    e.children = vec![
        aff,
        text_array("float-array", "weights", &weights),
        forms,
        stub("CDrawableGuid", Some("targetArtMeshA_guid"), &aid),
        stub("CDrawableGuid", Some("targetArtMeshB_guid"), &bid),
        text_array("long-array", "bindVertexUids", &uid_text),
        tabv,
    ];
    let id = add_shared(root, ctx, e)?;
    let si = shared_index(root)?;
    let k = shared(root)
        .and_then(|s| {
            s.children
                .iter()
                .position(|c| c.attr("xs.id") == Some(id.as_str()))
        })
        .unwrap_or(0);
    retarget(&mut shared_mut(root)?.children[k], me, &id);
    register(root, ctx, "affecterSourceSet", "CGlueSource", &id)?;
    let path = vec![si, k as u32];
    finish_grid(root, ctx, &path, &g.info.id, &g.grid, &g.blend_shapes)?;
    Ok(path)
}

/// Deletes an object element: its set-list stub goes, and the element and
/// everything it referred to become orphan candidates.
pub(super) fn delete_object(
    root: &mut Element,
    ctx: &mut Ctx,
    guid: &str,
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
    let pos = l
        .children
        .iter()
        .position(|c| c.attr("xs.ref") == Some(id.as_str()))
        .unwrap_or(l.children.len());
    l.children.retain(|c| c.attr("xs.ref") != Some(id.as_str()));
    set_count(l);
    ctx.deleted.push((
        guid.to_owned(),
        set.to_owned(),
        e.name.clone(),
        id.clone(),
        pos,
    ));
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
