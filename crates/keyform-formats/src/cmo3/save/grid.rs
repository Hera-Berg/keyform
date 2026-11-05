//! Rewriting an object's keyform grid after keys were added or removed.
//!
//! The grid's `KeyformGridSource` gets new `keyformsOnGrid` and
//! `keyformBindings` lists; existing bindings (matched by parameter) and
//! keyform elements (matched by form GUID) are reused, new ones are created
//! (`<shared>` objects get fresh `#N` ids at the end of `<shared>`), and
//! objects that may have become unreferenced are collected for removal.
//! Grid entries are written in cell order, first binding fastest, as the
//! reference does. spec: 10-formats/cmo3-schema.md (KeyformGridSource,
//! KeyformBindingSource, KeyformOnGrid, KeyOnParameter),
//! 35-editing/keys.md §1, xml-object-graph.md §4

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use keyform_model::{ExtendedInterpolationKind, KeyformGrid, Parameter};

use super::{element_at, holder_depth, unsupported, Fields, FormFields};
use crate::cmo3::{ElementPath, ObjectSource};
use crate::error::{invalid, Result};
use crate::graph::format_f32;
use crate::xml::Element;

/// State shared by every object written in one sync.
pub(super) struct Ctx<'a> {
    /// The new model's parameters.
    pub params: &'a [Arc<Parameter>],
    /// The saved model's parameters and their elements.
    pub old_params: &'a [Arc<Parameter>],
    pub param_paths: &'a [Option<ElementPath>],
    /// GUID → shared id of its GUID object (filled on demand and by new
    /// objects).
    pub guid_ids: HashMap<String, String>,
    /// Element of every object of the saved model, by GUID.
    pub object_paths: HashMap<String, ElementPath>,
    pub old_model: &'a keyform_model::Model,
    pub new_model: &'a keyform_model::Model,
    pub(super) next_id: u64,
    pub(super) next_idx: u64,
    /// Objects deleted by this save: (GUID, set list, tag, element id,
    /// set-list position).
    pub(super) deleted: Vec<(String, String, String, String, usize)>,
    /// Shared ids that may have lost their last reference.
    pub orphans: Vec<String>,
    /// Whether any element was added or removed (sources must be reloaded).
    pub structural: bool,
    /// New texture-manager elements and archive entries of this save.
    pub(crate) tex: super::textures::TexState,
}

impl Ctx<'_> {
    /// Whether the saved model has an object with this GUID.
    pub fn old_has_guid(&self, u: &str) -> bool {
        self.object_paths.contains_key(u)
    }
}

impl<'a> Ctx<'a> {
    pub fn new(
        root: &Element,
        old_model: &'a keyform_model::Model,
        new_model: &'a keyform_model::Model,
        sources: &'a crate::cmo3::SourceMap,
    ) -> Self {
        let (params, old_params, param_paths) = (
            &new_model.parameters[..],
            &old_model.parameters[..],
            &sources.parameters[..],
        );
        let mut object_paths = HashMap::new();
        let mut add = |guid: &str, src: Option<&crate::cmo3::ObjectSource>| {
            if let Some(p) = src.and_then(|s| s.element.clone()) {
                object_paths.insert(guid.to_owned(), p);
            }
        };
        for (i, x) in old_model.parts.iter().enumerate() {
            add(&x.info.guid, sources.parts.get(i));
        }
        for (i, x) in old_model.deformers.iter().enumerate() {
            add(&x.info.guid, sources.deformers.get(i));
        }
        for (i, x) in old_model.art_meshes.iter().enumerate() {
            add(&x.info.guid, sources.art_meshes.get(i));
        }
        for (i, x) in old_model.glues.iter().enumerate() {
            add(&x.info.guid, sources.glues.get(i));
        }
        let (mut id, mut idx) = (0, 0);
        if let Some(sh) = shared(root) {
            for e in &sh.children {
                let num =
                    |a: Option<&str>| a.and_then(|v| v.trim_start_matches('#').parse::<u64>().ok());
                id = id.max(num(e.attr("xs.id")).map_or(0, |n| n + 1));
                idx = idx.max(num(e.attr("xs.idx")).map_or(0, |n| n + 1));
            }
        }
        Self {
            params,
            old_params,
            param_paths,
            guid_ids: HashMap::new(),
            object_paths,
            old_model,
            new_model,
            next_id: id,
            next_idx: idx,
            deleted: Vec::new(),
            tex: Default::default(),
            orphans: Vec::new(),
            structural: false,
        }
    }
}

pub(super) fn shared(root: &Element) -> Option<&Element> {
    root.children.iter().find(|c| c.name == "shared")
}

pub(super) fn shared_mut(root: &mut Element) -> Result<&mut Element> {
    match root.children.iter_mut().find(|c| c.name == "shared") {
        Some(s) => Ok(s),
        None => invalid("document has no <shared> section"),
    }
}

pub(super) fn find_shared<'e>(root: &'e mut Element, id: &str) -> Result<&'e mut Element> {
    match shared_mut(root)?
        .children
        .iter_mut()
        .find(|c| c.attr("xs.id") == Some(id))
    {
        Some(e) => Ok(e),
        None => invalid(format!("ref id {id}: object not found")),
    }
}

pub(super) fn get_shared<'e>(root: &'e Element, id: &str) -> Option<&'e Element> {
    shared(root)?
        .children
        .iter()
        .find(|c| c.attr("xs.id") == Some(id))
}

/// Puts `e` into `<shared>` under a fresh id and returns the id.
pub(super) fn add_shared(root: &mut Element, ctx: &mut Ctx, mut e: Element) -> Result<String> {
    let id = format!("#{}", ctx.next_id);
    ctx.next_id += 1;
    e.attrs
        .retain(|(k, _)| k != "xs.n" && k != "xs.id" && k != "xs.idx");
    e.attrs.push(("xs.id".into(), id.clone()));
    e.attrs.push(("xs.idx".into(), ctx.next_idx.to_string()));
    ctx.next_idx += 1;
    shared_mut(root)?.children.push(e);
    ctx.structural = true;
    Ok(id)
}

/// A reference stub `<tag xs.n=name xs.ref=id/>` (no `xs.n` for list items).
pub(super) fn stub(tag: &str, name: Option<&str>, id: &str) -> Element {
    let mut e = Element::new(tag);
    if let Some(n) = name {
        e.attrs.push(("xs.n".into(), n.into()));
    }
    e.attrs.push(("xs.ref".into(), id.into()));
    e
}

/// Makes `e` (an element in place) a reference to a shared object, moving
/// its content into `<shared>` if it is inline. Returns the shared id.
pub(super) fn share_in_place(root: &mut Element, ctx: &mut Ctx, path: &[u32]) -> Result<String> {
    let e = element_at(root, Some(path))?;
    if let Some(id) = e.attr("xs.ref") {
        return Ok(id.to_owned());
    }
    let content = e.clone();
    let id = add_shared(root, ctx, content.clone())?;
    *element_at(root, Some(path))? = stub(&content.name, content.field_name(), &id);
    Ok(id)
}

/// Path of field `name` of the object at `obj` (own element or `super` chain).
pub(super) fn field_path(root: &mut Element, obj: &[u32], name: &str) -> Result<ElementPath> {
    let mut path = obj.to_vec();
    let e = element_at(root, Some(obj))?;
    let Some(depth) = holder_depth(e, name) else {
        return invalid(format!("object has no field {name:?}"));
    };
    let mut cur: &Element = e;
    for _ in 0..depth {
        let i = cur
            .children
            .iter()
            .position(|c| c.field_name() == Some("super"))
            .unwrap_or(0);
        path.push(i as u32);
        cur = &cur.children[i];
    }
    match cur
        .children
        .iter()
        .position(|c| c.field_name() == Some(name))
    {
        Some(i) => {
            path.push(i as u32);
            Ok(path)
        }
        None => invalid(format!(
            "field {name:?} is an attribute, expected an element"
        )),
    }
}

/// UUID of a GUID-valued element (inline `uuid`, or the shared object's).
pub(super) fn uuid_of(root: &Element, e: &Element) -> Option<String> {
    match e.attr("xs.ref") {
        Some(id) => get_shared(root, id)?.attr("uuid").map(str::to_owned),
        None => e.attr("uuid").map(str::to_owned),
    }
}

/// The `guid` field element of a keyform element (for sibling modules).
pub(super) fn form_guid_field_pub(form: &mut Element) -> Option<&mut Element> {
    form_guid_field(form)
}

/// The `guid` field element of a keyform element (on its `super` chain).
fn form_guid_field(form: &mut Element) -> Option<&mut Element> {
    let mut cur = form;
    for _ in 0..super::MAX_SUPER_DEPTH {
        if let Some(i) = cur
            .children
            .iter()
            .position(|c| c.field_name() == Some("guid"))
        {
            return cur.children.get_mut(i);
        }
        let i = cur
            .children
            .iter()
            .position(|c| c.field_name() == Some("super"))?;
        cur = &mut cur.children[i];
    }
    None
}

pub(super) fn set_count(list: &mut Element) {
    list.set_attr("count", list.children.len().to_string());
}

fn float_list(name: &str, values: &[f32]) -> Element {
    let mut l = Element::new("array_list").with_attr("xs.n", name);
    for v in values {
        let mut f = Element::new("f");
        f.text = format_f32(*v);
        l.children.push(f);
    }
    set_count(&mut l);
    l
}

pub(super) fn scalar(tag: &str, name: &str, text: impl Into<String>) -> Element {
    let mut e = Element::new(tag).with_attr("xs.n", name);
    e.text = text.into();
    e
}

pub(super) fn extended_name(k: ExtendedInterpolationKind) -> &'static str {
    match k {
        ExtendedInterpolationKind::Linear => "LINEAR",
        ExtendedInterpolationKind::Ellipse => "ELLIPSE",
        ExtendedInterpolationKind::SnsCurve => "SNS_CURVE",
    }
}

/// Shared id of a parameter's GUID object (sharing it if it is inline).
pub(super) fn param_guid_id(root: &mut Element, ctx: &mut Ctx, p: usize) -> Result<String> {
    let Some(guid) = ctx.params.get(p).map(|x| x.guid.clone()) else {
        return invalid("binding to an unknown parameter");
    };
    if let Some(id) = ctx.guid_ids.get(&guid) {
        return Ok(id.clone());
    }
    let Some(old) = ctx.old_params.iter().position(|x| x.guid == guid) else {
        return invalid("parameter has no element");
    };
    let Some(Some(path)) = ctx.param_paths.get(old).cloned() else {
        return invalid("parameter has no element");
    };
    let field = field_path(root, &path, "guid")?;
    let id = share_in_place(root, ctx, &field)?;
    ctx.guid_ids.insert(guid, id.clone());
    Ok(id)
}

/// Rewrites the grid of the object at `src` from `old` to `new`.
pub(super) fn rebuild_grid<F: FormFields>(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    what: &str,
    old: &KeyformGrid<F>,
    new: &KeyformGrid<F>,
) -> Result<()> {
    let tracked = |g: &KeyformGrid<F>| g.guids.len() == g.keyforms.len();
    if !tracked(old) || !tracked(new) || new.guids.iter().any(String::is_empty) {
        return unsupported(format!("{what}: keyforms without identities"));
    }
    let Some(obj) = src.element.as_deref() else {
        return invalid(format!("{what}: no element"));
    };
    ctx.structural = true;

    // Existing keyform elements by form GUID.
    let list_path = field_path(root, obj, "keyforms")?;
    let old_list = element_at(root, Some(&list_path))?.clone();
    // (uuid, content, shared id when the list holds a reference stub)
    let mut existing: Vec<(String, Element, Option<String>)> = Vec::new();
    for c in &old_list.children {
        let shared_id = c.attr("xs.ref").map(str::to_owned);
        let mut e = match &shared_id {
            Some(id) => match get_shared(root, id) {
                Some(e) => e.clone(),
                None => return invalid(format!("ref id {id}: object not found")),
            },
            None => c.clone(),
        };
        let uuid = form_guid_field(&mut e).and_then(|g| uuid_of(root, g));
        existing.push((uuid.unwrap_or_default(), e, shared_id));
    }
    let old_form = |uuid: &str| -> Option<&F> {
        let i = old.guids.iter().position(|g| g == uuid)?;
        old.keyforms.get(i)
    };
    // Template for new keyforms: the middle one (keys.md §1 step 3).
    let mid = old.keyforms.len() / 2;
    let template = old
        .guids
        .get(mid)
        .and_then(|g| existing.iter().rfind(|(u, _, _)| u == g))
        .or_else(|| existing.first())
        .map(|(_, e, _)| {
            let mut e = e.clone();
            e.attrs.retain(|(k, _)| k != "xs.id" && k != "xs.idx");
            e
        });
    let template_form = old.keyforms.get(mid).or_else(|| old.keyforms.first());
    let (Some(template), Some(template_form)) = (template, template_form) else {
        return invalid(format!("{what}: no keyform to copy"));
    };

    // The grid source; shared as soon as bindings refer to it.
    let gs_field = field_path(root, obj, "keyformGridSource")?;
    let gs_id = if new.bindings.is_empty() {
        element_at(root, Some(&gs_field))?
            .attr("xs.ref")
            .map(str::to_owned)
    } else {
        Some(share_in_place(root, ctx, &gs_field)?)
    };

    // Bindings: reuse by parameter, create the missing ones.
    let gs_snapshot = match &gs_id {
        Some(id) => get_shared(root, id).cloned(),
        None => Some(element_at(root, Some(&gs_field))?.clone()),
    };
    let Some(gs_snapshot) = gs_snapshot else {
        return invalid(format!("{what}: grid source missing"));
    };
    let mut old_bindings: HashMap<String, String> = HashMap::new();
    if let Some(l) = gs_snapshot.child_field("keyformBindings") {
        for b in &l.children {
            let Some(id) = b.attr("xs.ref") else { continue };
            let pg = get_shared(root, id)
                .and_then(|e| e.child_field("parameterGuid"))
                .and_then(|g| uuid_of(root, g));
            if let Some(pg) = pg {
                old_bindings.insert(pg, id.to_owned());
            }
        }
    }
    let mut binding_ids = Vec::new();
    for b in &new.bindings {
        let Some(param) = ctx.params.get(b.parameter).cloned() else {
            return invalid(format!("{what}: binding to unknown parameter"));
        };
        let id = match old_bindings.remove(&param.guid) {
            Some(id) => id,
            None => {
                let pg = param_guid_id(root, ctx, b.parameter)?;
                let Some(gs) = gs_id.as_deref() else {
                    return invalid(format!("{what}: grid source not shared"));
                };
                let mut e = Element::new("KeyformBindingSource");
                e.children = vec![
                    stub("KeyformGridSource", Some("_gridSource"), gs),
                    stub("CParameterGuid", Some("parameterGuid"), &pg),
                    float_list("keys", &[]),
                    Element::new("InterpolationType")
                        .with_attr("xs.n", "interpolationType")
                        .with_attr("v", "LINEAR"),
                    Element::new("ExtendedInterpolationType")
                        .with_attr("xs.n", "extendedInterpolationType")
                        .with_attr("v", "LINEAR"),
                    scalar("i", "insertPointCount", "1"),
                    scalar("f", "extendedInterpolationScale", "1.0"),
                    scalar("s", "description", param.id.clone()),
                ];
                add_shared(root, ctx, e)?
            }
        };
        // Keys and interpolation settings.
        let e = find_shared(root, &id)?;
        match e
            .children
            .iter_mut()
            .find(|c| c.field_name() == Some("keys"))
        {
            Some(k) => *k = float_list("keys", &b.keys),
            None => e.children.push(float_list("keys", &b.keys)),
        }
        let mut f = Fields { e };
        f.enumv(
            "interpolationType",
            "InterpolationType",
            "",
            &b.interpolation,
        );
        f.enumv(
            "extendedInterpolationType",
            "ExtendedInterpolationType",
            "",
            extended_name(b.extended),
        );
        f.i32("insertPointCount", i32::MIN, b.insert_point_count);
        f.f32("extendedInterpolationScale", f32::NAN, b.extended_scale);
        binding_ids.push(id);
    }
    ctx.orphans.extend(old_bindings.into_values());

    // Keyform elements in cell order (each stored keyform once), then the
    // elements the grid never used (blend-shape keyforms) in their order.
    let mut seen = HashSet::new();
    let mut form_ids: HashMap<String, String> = HashMap::new();
    let mut children = Vec::new();
    for (cell, uuid) in new.guids.iter().enumerate() {
        if !seen.insert(uuid.clone()) {
            continue;
        }
        let new_form = &new.keyforms[cell];
        let found = existing.iter().rfind(|(u, _, _)| u == uuid);
        let shared_form = found.and_then(|(_, _, id)| id.clone());
        let (mut e, base) = match found {
            Some((_, e, _)) => (e.clone(), old_form(uuid).unwrap_or(template_form)),
            None => (template.clone(), template_form),
        };
        F::write(&mut Fields { e: &mut e }, base, new_form)?;
        let Some(g) = form_guid_field(&mut e) else {
            return invalid(format!("{what}: keyform without guid field"));
        };
        let id = match (found.is_some(), g.attr("xs.ref")) {
            (true, Some(id)) => id.to_owned(),
            (true, None) => {
                // Inline GUID: the grid will refer to it, so share it.
                let content = g.clone();
                let id = add_shared(root, ctx, content.clone())?;
                *g = stub("CFormGuid", Some("guid"), &id);
                id
            }
            (false, _) => {
                let guid = Element::new("CFormGuid")
                    .with_attr("uuid", uuid.clone())
                    .with_attr("note", "(no debug info)");
                let id = add_shared(root, ctx, guid)?;
                *g = stub("CFormGuid", Some("guid"), &id);
                id
            }
        };
        form_ids.insert(uuid.clone(), id);
        match shared_form {
            // A shared keyform: its content is edited where it lives.
            Some(sid) => {
                let tag = e.name.clone();
                *find_shared(root, &sid)? = e;
                children.push(stub(&tag, None, &sid));
            }
            None => children.push(e),
        }
    }
    for (i, (uuid, e, sid)) in existing.iter().enumerate() {
        if old.guids.contains(uuid) {
            if !seen.contains(uuid) {
                // A dropped grid keyform: it and its GUID may now be unused.
                let mut e = e.clone();
                if let Some(id) = form_guid_field(&mut e).and_then(|g| g.attr("xs.ref")) {
                    ctx.orphans.push(id.to_owned());
                }
                ctx.orphans.extend(sid.clone());
            }
        } else {
            children.push(old_list.children[i].clone());
        }
    }
    let list = element_at(root, Some(&list_path))?;
    list.children = children;
    set_count(list);

    // Grid entries.
    let dims: Vec<usize> = new.bindings.iter().map(|b| b.keys.len()).collect();
    let mut on_grid = Element::new("array_list").with_attr("xs.n", "keyformsOnGrid");
    for (cell, uuid) in new.guids.iter().enumerate() {
        let mut keys = Element::new("array_list").with_attr("xs.n", "_keyOnParameterList");
        let mut rest = cell;
        for (j, n) in dims.iter().enumerate() {
            let mut kop = Element::new("KeyOnParameter");
            kop.children = vec![
                stub("KeyformBindingSource", Some("binding"), &binding_ids[j]),
                scalar("i", "keyIndex", (rest % n).to_string()),
            ];
            rest /= n;
            keys.children.push(kop);
        }
        set_count(&mut keys);
        let mut access = Element::new("KeyformGridAccessKey").with_attr("xs.n", "accessKey");
        access.children.push(keys);
        let mut kog = Element::new("KeyformOnGrid");
        kog.children = vec![
            access,
            stub("CFormGuid", Some("keyformGuid"), &form_ids[uuid]),
        ];
        on_grid.children.push(kog);
    }
    set_count(&mut on_grid);
    let mut bindings = Element::new("array_list").with_attr("xs.n", "keyformBindings");
    bindings.children = binding_ids
        .iter()
        .map(|id| stub("KeyformBindingSource", None, id))
        .collect();
    set_count(&mut bindings);
    let gs = match &gs_id {
        Some(id) => find_shared(root, id)?,
        None => element_at(root, Some(&gs_field))?,
    };
    for new_child in [on_grid, bindings] {
        let name = new_child.field_name().map(str::to_owned);
        match gs
            .children
            .iter_mut()
            .find(|c| c.field_name().map(str::to_owned) == name)
        {
            Some(c) => *c = new_child,
            None => gs.children.push(new_child),
        }
    }
    Ok(())
}

/// Removes those of `ids` that nothing refers to any more.
/// Removes those of `ids` that nothing refers to any more, repeating
/// while removals free further candidates (a dead binding releases the
/// parameter GUID it named).
pub(super) fn remove_orphans(root: &mut Element, ids: &[String]) -> Result<Vec<Element>> {
    let mut removed = Vec::new();
    let mut left: Vec<String> = ids.to_vec();
    left.sort();
    left.dedup();
    while !left.is_empty() {
        // References from inside a candidate don't keep anything alive
        // (a deleted object's forms refer back to it).
        let cand: HashSet<&str> = left.iter().map(String::as_str).collect();
        let mut refs: HashSet<String> = HashSet::new();
        let mut stack = vec![&*root];
        while let Some(e) = stack.pop() {
            if e.attr("xs.id").is_some_and(|id| cand.contains(id)) {
                continue;
            }
            if let Some(r) = e.attr("xs.ref") {
                refs.insert(r.to_owned());
            }
            stack.extend(e.children.iter());
        }
        let dead: HashSet<String> = left
            .iter()
            .filter(|id| !refs.contains(*id))
            .cloned()
            .collect();
        if dead.is_empty() {
            break;
        }
        let sh = shared_mut(root)?;
        let (gone, keep): (Vec<Element>, Vec<Element>) = std::mem::take(&mut sh.children)
            .into_iter()
            .partition(|c| c.attr("xs.id").is_some_and(|id| dead.contains(id)));
        sh.children = keep;
        removed.extend(gone);
        left.retain(|id| !dead.contains(id));
    }
    Ok(removed)
}

/// Bindings equal up to parameter renumbering (compared by GUID).
pub(super) fn same_bindings(
    ctx: &Ctx,
    a: &[keyform_model::Binding],
    b: &[keyform_model::Binding],
) -> bool {
    let og = |i: usize| ctx.old_params.get(i).map(|p| p.guid.as_str());
    let ng = |i: usize| ctx.params.get(i).map(|p| p.guid.as_str());
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            og(x.parameter) == ng(y.parameter)
                && keyform_model::Binding {
                    parameter: 0,
                    ..x.clone()
                } == keyform_model::Binding {
                    parameter: 0,
                    ..y.clone()
                }
        })
}

/// Blend shapes equal up to parameter renumbering; `forms` also compares
/// the shape forms.
pub(super) fn same_blend<F: PartialEq>(
    ctx: &Ctx,
    a: &keyform_model::BlendShapes<F>,
    b: &keyform_model::BlendShapes<F>,
    forms: bool,
) -> bool {
    let og = |i: usize| ctx.old_params.get(i).map(|p| p.guid.as_str());
    let ng = |i: usize| ctx.params.get(i).map(|p| p.guid.as_str());
    a.shapes.len() == b.shapes.len()
        && a.constraints.len() == b.constraints.len()
        && a.shapes.iter().zip(&b.shapes).all(|(x, y)| {
            og(x.parameter) == ng(y.parameter) && x.key == y.key && (!forms || x.form == y.form)
        })
        && a.constraints.iter().zip(&b.constraints).all(|(x, y)| {
            og(x.blend_parameter) == ng(y.blend_parameter)
                && og(x.constraint_parameter) == ng(y.constraint_parameter)
                && x.value == y.value
                && x.weight == y.weight
        })
}

/// `<?import …?>` names of the types a rebuilt grid may contain.
pub(super) const GRID_IMPORTS: [&str; 9] = [
    "com.live2d.cubism.doc.model.interpolator.InterpolationType",
    "com.live2d.cubism.doc.model.interpolator.KeyOnParameter",
    "com.live2d.cubism.doc.model.interpolator.KeyformBindingSource",
    "com.live2d.cubism.doc.model.interpolator.KeyformGridAccessKey",
    "com.live2d.cubism.doc.model.interpolator.KeyformGridSource",
    "com.live2d.cubism.doc.model.interpolator.KeyformOnGrid",
    "com.live2d.cubism.doc.model.interpolator.extendedInterpolation.ExtendedInterpolationType",
    "com.live2d.type.CFormGuid",
    "com.live2d.type.CParameterGuid",
];
