//! Deform paths (`CControllerExtension`) in the save, and the references
//! of their target points to keyform position arrays.
//! spec: 35-editing/deform-paths.md §1, 10-formats/cmo3-schema.md

use std::collections::{HashMap, HashSet};

use keyform_model::{ArtMesh, DeformPath, Effector};

use super::grid::{
    add_shared, field_path, find_shared, form_guid_field_pub, get_shared, scalar, set_count,
    share_in_place, shared, stub, uuid_of, Ctx,
};
use super::objects::{coord_id, form_paths, guid_id, refs_in, shared_index, shared_path_of};
use super::{element_at, format_f32, unsupported};
use crate::error::Result;
use crate::xml::Element;

const EXT: &str = "CControllerExtension";
const OBSERVER: &str = "CTopologyObserverExtension";

/// The element of list item `item` (resolving a reference).
fn resolved<'e>(root: &'e Element, item: &'e Element) -> &'e Element {
    match item.attr("xs.ref") {
        Some(id) => get_shared(root, id).unwrap_or(item),
        None => item,
    }
}

/// The extension GUID (`super.guid`) of an extension element.
fn ext_guid(e: &Element) -> Option<String> {
    let sup = e
        .children
        .iter()
        .find(|c| c.field_name() == Some("super"))?;
    sup.children
        .iter()
        .find(|c| c.field_name() == Some("guid"))
        .and_then(|g| g.attr("uuid"))
        .map(str::to_owned)
}

fn vec2(name: &str, v: [f32; 2]) -> Element {
    let mut e = Element::new("GVector2").with_attr("xs.n", name);
    e.children = vec![
        scalar("f", "x", format_f32(v[0])),
        scalar("f", "y", format_f32(v[1])),
    ];
    e
}

fn list(tag: &str, name: &str, items: Vec<Element>) -> Element {
    let mut l = Element::new(tag).with_attr("xs.n", name);
    l.children = items;
    set_count(&mut l);
    l
}

/// Shared ids a target point of `mesh` (by GUID) bound to keyform `form`
/// refers to: (mesh source, drawable GUID, keyform, its positions, its
/// coordinate type).
#[derive(Clone)]
struct MeshRefs {
    source: String,
    guid: String,
    form: String,
    positions: String,
    coord: String,
}

/// The shared id of field `name` of the shared object `id`, sharing it in
/// place when inline.
fn field_ref(root: &mut Element, ctx: &mut Ctx, id: &str, name: &str) -> Result<String> {
    let p = shared_path_of(root, id)?;
    let f = field_path(root, &p, name)?;
    share_in_place(root, ctx, &f)
}

fn mesh_refs(
    root: &mut Element,
    ctx: &mut Ctx,
    cache: &mut HashMap<(String, String), MeshRefs>,
    mesh_path: Option<&[u32]>,
    mesh: &ArtMesh,
    form: &str,
) -> Result<MeshRefs> {
    let key = (mesh.info.guid.clone(), form.to_owned());
    if let Some(r) = cache.get(&key) {
        return Ok(r.clone());
    }
    let Some(path) = mesh_path
        .map(<[u32]>::to_vec)
        .or_else(|| ctx.object_paths.get(&mesh.info.guid).cloned())
    else {
        return unsupported(format!("deform path: sub mesh {} is new", mesh.info.id));
    };
    let source = match element_at(root, Some(&path))?.attr("xs.id") {
        Some(id) => id.to_owned(),
        None => share_in_place(root, ctx, &path)?,
    };
    let guid = guid_id(
        root,
        ctx,
        &mesh.info.guid,
        "CDrawableGuid",
        mesh.info.display_name(),
    )?;
    // The keyform with that GUID, else the first.
    let forms = form_paths(root, &shared_path_of(root, &source)?)?;
    let mut pick = None;
    for (k, fp) in forms.iter().enumerate() {
        let e = element_at(root, Some(fp))?;
        let g = form_guid_field_pub(e).cloned();
        let u = g.and_then(|g| uuid_of(root, &g));
        if u.as_deref() == Some(form) || (pick.is_none() && k == 0) {
            pick = Some(fp.clone());
            if u.as_deref() == Some(form) {
                break;
            }
        }
    }
    let Some(fp) = pick else {
        return unsupported(format!("deform path: {} has no keyform", mesh.info.id));
    };
    let fid = match element_at(root, Some(&fp))?.attr("xs.id") {
        Some(id) => id.to_owned(),
        None => share_in_place(root, ctx, &fp)?,
    };
    let positions = field_ref(root, ctx, &fid, "positions")?;
    let coord = field_ref(root, ctx, &fid, "coordType")?;
    let r = MeshRefs {
        source,
        guid,
        form: fid,
        positions,
        coord,
    };
    cache.insert(key, r.clone());
    Ok(r)
}

/// Fills the shared `CControllerExtension` `ext` with path `p` of mesh
/// `mesh` (shared id `me`). Old points and curves become orphans.
#[allow(clippy::too_many_arguments)]
fn write_body(
    root: &mut Element,
    ctx: &mut Ctx,
    cache: &mut HashMap<(String, String), MeshRefs>,
    mesh_path: &[u32],
    me: &str,
    ext: &str,
    mesh: &ArtMesh,
    p: &DeformPath,
) -> Result<()> {
    let new = ctx.new_model;
    {
        let old = find_shared(root, ext)?.clone();
        for c in &old.children {
            refs_in(c, &mut ctx.orphans);
        }
    }
    // Objects first (points and curves refer to each other).
    let pids: Vec<String> = (0..p.points.len())
        .map(|_| add_shared(root, ctx, Element::new("CControllerPoint")))
        .collect::<Result<_>>()?;
    let cids: Vec<String> = (0..p.curves.len())
        .map(|_| add_shared(root, ctx, Element::new("CControllerCurve")))
        .collect::<Result<_>>()?;
    let mut curve_guids = Vec::new();
    for c in &p.curves {
        let g = if c.guid.is_empty() {
            keyform_model::new_guid()
        } else {
            c.guid.clone()
        };
        curve_guids.push(guid_id(
            root,
            ctx,
            &g,
            "CControllerCurveGuid",
            "(no debug info)",
        )?);
    }
    let point_coord = {
        let name = if mesh.target.is_some() {
            "DeformerLocal"
        } else {
            "Canvas"
        };
        coord_id(root, ctx, name)?
    };
    for (k, cp) in p.points.iter().enumerate() {
        let mut e = Element::new("CControllerPoint");
        let assigned = match cp.curve.and_then(|c| cids.get(c)) {
            Some(id) => stub("CControllerCurve", Some("assignedCurve"), id),
            None => Element::new("null").with_attr("xs.n", "assignedCurve"),
        };
        let mut tri = Element::new("PointInTriangle").with_attr("xs.n", "pointInTriangle");
        for i in 0..3 {
            tri.children.push(scalar(
                "i",
                &format!("ptIndex{}", i + 1),
                cp.anchor.index[i].to_string(),
            ));
            tri.children.push(scalar(
                "f",
                &format!("weight{}", i + 1),
                format_f32(cp.anchor.weight[i]),
            ));
        }
        let g = if cp.guid.is_empty() {
            keyform_model::new_guid()
        } else {
            cp.guid.clone()
        };
        e.children = vec![
            assigned,
            scalar("b", "isCorner", cp.corner.to_string()),
            stub("CoordType", Some("coordType"), &point_coord),
            tri,
            vec2("pointOnCanvasForRecovery", cp.recovery),
            scalar(
                "f",
                "totalEffectToPointInTriangle",
                format_f32(cp.total_effect),
            ),
            stub(EXT, Some("_owner"), ext),
            Element::new("CControllerPointGuid")
                .with_attr("xs.n", "ctrlPtId")
                .with_attr("uuid", g)
                .with_attr("note", "(no debug info)"),
            vec2("posOnLocalOfDefaultKeyform", cp.pos_default),
        ];
        let slot = find_shared(root, &pids[k])?;
        slot.children = e.children;
    }
    for (k, c) in p.curves.iter().enumerate() {
        let pts = c
            .points
            .iter()
            .filter_map(|&i| pids.get(i))
            .map(|id| stub("CControllerPoint", None, id))
            .collect();
        let children = vec![
            scalar("f", "lineWidth", format_f32(c.width)),
            scalar("f", "lineHardnessPercent", format_f32(c.hardness)),
            scalar("b", "isOpen", c.open.to_string()),
            list("carray_list", "_curvePoints", pts),
            stub("CControllerCurveGuid", Some("curveId"), &curve_guids[k]),
        ];
        find_shared(root, &cids[k])?.children = children;
    }
    // Target points.
    let mut targets = Vec::with_capacity(p.targets.len());
    for t in &p.targets {
        let Some(m) = new.art_meshes.iter().find(|a| a.info.guid == t.mesh) else {
            continue;
        };
        let own = m.info.guid == mesh.info.guid;
        let r = mesh_refs(root, ctx, cache, own.then_some(mesh_path), m, &t.form)?;
        let mut pr = Element::new("MeshPointRef").with_attr("xs.n", "_point");
        pr.children = vec![
            scalar("l", "pointUid", t.uid.to_string()),
            stub("CoordType", Some("coordType"), &r.coord),
            stub("CArtMeshForm", Some("keyForm"), &r.form),
            stub("float-array", Some("positions"), &r.positions),
            scalar("i", "step", "0"),
            stub("CArtMeshSource", Some("_artMeshSource"), &r.source),
            scalar("i", "_index", t.index.to_string()),
            stub("CDrawableGuid", Some("artMeshGuid"), &r.guid),
        ];
        let mut effects = Vec::with_capacity(t.effects.len());
        for e in &t.effects {
            let ep = match &e.effector {
                Effector::Point(k) => match pids.get(*k) {
                    Some(id) => stub("CControllerPoint", Some("effectorPt"), id),
                    None => continue,
                },
                Effector::Curve {
                    curve,
                    t,
                    distance,
                    pos,
                } => {
                    let Some(g) = curve_guids.get(*curve) else {
                        continue;
                    };
                    let mut c = Element::new("PointOnCurve").with_attr("xs.n", "effectorPt");
                    c.children = vec![
                        stub("CControllerCurveGuid", Some("curveId"), g),
                        scalar("f", "totalT", format_f32(*t)),
                        scalar("f", "distance", format_f32(*distance)),
                        vec2("_posOnLocal", *pos),
                    ];
                    c
                }
            };
            let mut ef = Element::new("Effect");
            ef.children = vec![ep, scalar("f", "weight", format_f32(e.weight))];
            effects.push(ef);
        }
        let mut tp = Element::new("TargetPoint");
        tp.children = vec![pr, list("carray_list", "effects", effects)];
        targets.push(tp);
    }
    let mut subs = Vec::new();
    for g in &p.sub_meshes {
        let name = new
            .art_meshes
            .iter()
            .find(|a| &a.info.guid == g)
            .map_or("", |a| a.info.display_name())
            .to_owned();
        subs.push(stub(
            "CDrawableGuid",
            None,
            &guid_id(root, ctx, g, "CDrawableGuid", &name)?,
        ));
    }
    let eg = if p.guid.is_empty() {
        keyform_model::new_guid()
    } else {
        p.guid.clone()
    };
    let mut sup = Element::new("ACExtension").with_attr("xs.n", "super");
    sup.children = vec![
        Element::new("CExtensionGuid")
            .with_attr("xs.n", "guid")
            .with_attr("uuid", eg)
            .with_attr("note", "(no debug info)"),
        stub("CArtMeshSource", Some("_owner"), me),
    ];
    let children = vec![
        sup,
        scalar("i", "editLevel", p.edit_level.to_string()),
        scalar("i", "maxBindCount", p.max_bind.to_string()),
        Element::new("BindMethod")
            .with_attr("xs.n", "bindMethod")
            .with_attr("v", p.bind_method.name()),
        list(
            "carray_list",
            "controlPoints",
            pids.iter()
                .map(|id| stub("CControllerPoint", None, id))
                .collect(),
        ),
        list(
            "carray_list",
            "controlCurves",
            cids.iter()
                .map(|id| stub("CControllerCurve", None, id))
                .collect(),
        ),
        list("carray_list", "targetPoints", targets),
        list("array_list", "subArtMeshGuids", subs),
    ];
    find_shared(root, ext)?.children = children;
    ctx.structural = true;
    Ok(())
}

/// Writes the deform paths of art mesh `n` (element at `mesh_path`) whose
/// saved version had `old`: changed paths are rewritten, new ones added
/// (with a topology observer), deleted ones removed. spec: deform-paths.md §1
pub(super) fn write_paths(
    root: &mut Element,
    ctx: &mut Ctx,
    mesh_path: &[u32],
    old: &[DeformPath],
    n: &ArtMesh,
) -> Result<()> {
    let me = match element_at(root, Some(mesh_path))?.attr("xs.id") {
        Some(id) => id.to_owned(),
        None => share_in_place(root, ctx, mesh_path)?,
    };
    let mesh_path = shared_path_of(root, &me)?;
    let lp = field_path(root, &mesh_path, "_extensions")?;
    // Make every controller extension in the list shared (points refer to
    // it by id).
    let n_items = element_at(root, Some(&lp))?.children.len();
    for k in 0..n_items {
        let mut ip = lp.clone();
        ip.push(k as u32);
        let e = element_at(root, Some(&ip))?;
        if e.name == EXT && e.attr("xs.ref").is_none() {
            share_in_place(root, ctx, &ip)?;
        }
    }
    // GUID → shared id of the existing controller extensions.
    let existing: HashMap<String, String> = {
        let l = element_at(root, Some(&lp))?.clone();
        l.children
            .iter()
            .filter(|c| c.name == EXT)
            .filter_map(|c| {
                let id = c.attr("xs.ref")?.to_owned();
                Some((ext_guid(resolved(root, c))?, id))
            })
            .collect()
    };
    let mut cache = HashMap::new();
    let keep: HashSet<&str> = n.paths.iter().map(|p| p.guid.as_str()).collect();
    // Deleted paths.
    let mut gone = Vec::new();
    for o in old {
        if keep.contains(o.guid.as_str()) {
            continue;
        }
        if let Some(id) = existing.get(&o.guid) {
            gone.push(id.clone());
        }
    }
    if !gone.is_empty() {
        unlink(root, ctx, &lp, &gone)?;
    }
    for p in &n.paths {
        let before = old.iter().find(|o| o.guid == p.guid);
        if before == Some(p) {
            continue;
        }
        let ext = match existing.get(&p.guid) {
            Some(id) => id.clone(),
            None => {
                let id = add_shared(root, ctx, Element::new(EXT))?;
                let l = element_at(root, Some(&lp))?;
                l.children.push(stub(EXT, None, &id));
                set_count(l);
                observe(root, ctx, &lp, &me, &id)?;
                id
            }
        };
        write_body(root, ctx, &mut cache, &mesh_path, &me, &ext, n, p)?;
        // Sub meshes observe the path too.
        for g in &p.sub_meshes {
            let Some(sp) = ctx.object_paths.get(g).cloned() else {
                continue;
            };
            let sid = match element_at(root, Some(&sp))?.attr("xs.id") {
                Some(id) => id.to_owned(),
                None => continue,
            };
            let slp = field_path(root, &sp, "_extensions")?;
            if !observes(root, &slp, &ext) {
                observe(root, ctx, &slp, &sid, &ext)?;
            }
        }
    }
    Ok(())
}

/// Adds `ext` to the observers of the mesh's topology observer (creating
/// one when the mesh has none).
fn observe(root: &mut Element, ctx: &mut Ctx, lp: &[u32], me: &str, ext: &str) -> Result<()> {
    let items = element_at(root, Some(lp))?.children.clone();
    if let Some(k) = items
        .iter()
        .position(|c| resolved(root, c).name == OBSERVER)
    {
        let mut ip = lp.to_vec();
        ip.push(k as u32);
        let id = share_in_place(root, ctx, &ip)?;
        let op = shared_path_of(root, &id)?;
        let f = field_path(root, &op, "observers")?;
        let l = element_at(root, Some(&f))?;
        l.children.push(stub(EXT, None, ext));
        set_count(l);
        return Ok(());
    }
    let mut sup = Element::new("ACExtension").with_attr("xs.n", "super");
    sup.children = vec![
        Element::new("CExtensionGuid")
            .with_attr("xs.n", "guid")
            .with_attr("uuid", keyform_model::new_guid())
            .with_attr("note", "(no debug info)"),
        stub("CArtMeshSource", Some("_owner"), me),
    ];
    let mut obs = Element::new(OBSERVER);
    obs.children = vec![
        sup,
        list("carray_list", "observers", vec![stub(EXT, None, ext)]),
    ];
    let l = element_at(root, Some(lp))?;
    let at = l
        .children
        .iter()
        .position(|c| c.name == EXT)
        .unwrap_or(l.children.len());
    l.children.insert(at, obs);
    set_count(l);
    ctx.structural = true;
    Ok(())
}

/// Removes the controller extensions `ids` from the mesh's extension list
/// and from its topology observer.
fn unlink(root: &mut Element, ctx: &mut Ctx, lp: &[u32], ids: &[String]) -> Result<()> {
    let dead =
        |c: &Element| c.name == EXT && c.attr("xs.ref").is_some_and(|r| ids.iter().any(|i| i == r));
    let items = element_at(root, Some(lp))?.children.clone();
    if let Some(k) = items
        .iter()
        .position(|c| resolved(root, c).name == OBSERVER)
    {
        let mut ip = lp.to_vec();
        ip.push(k as u32);
        let id = share_in_place(root, ctx, &ip)?;
        let op = shared_path_of(root, &id)?;
        let f = field_path(root, &op, "observers")?;
        let l = element_at(root, Some(&f))?;
        l.children.retain(|c| !dead(c));
        set_count(l);
    }
    let l = element_at(root, Some(lp))?;
    l.children.retain(|c| !dead(c));
    set_count(l);
    drop_observer_refs(root, ids);
    ctx.orphans.extend(ids.iter().cloned());
    for id in ids {
        let e = find_shared(root, id)?.clone();
        refs_in(&e, &mut ctx.orphans);
    }
    ctx.structural = true;
    Ok(())
}

/// Removes references to `ids` from every `observers` list (sub meshes
/// observe the paths that drive them).
fn drop_observer_refs(root: &mut Element, ids: &[String]) {
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if e.field_name() == Some("observers") {
            let before = e.children.len();
            e.children
                .retain(|c| !c.attr("xs.ref").is_some_and(|r| ids.iter().any(|i| i == r)));
            if e.children.len() != before {
                set_count(e);
            }
            continue;
        }
        stack.extend(e.children.iter_mut());
    }
}

/// Whether the observer of the mesh whose extension list is at `lp`
/// already lists `ext`.
fn observes(root: &Element, lp: &[u32], ext: &str) -> bool {
    let mut cur = root;
    for &i in lp {
        match cur.children.get(i as usize) {
            Some(c) => cur = c,
            None => return false,
        }
    }
    cur.children.iter().any(|c| {
        let o = resolved(root, c);
        o.name == OBSERVER
            && o.child_field("observers")
                .is_some_and(|l| l.children.iter().any(|x| x.attr("xs.ref") == Some(ext)))
    })
}

/// Points every target point's `keyForm` at a keyform its mesh still has,
/// and its `positions` at that keyform's current position array (a keyform
/// edit gives the keyform a new array). spec: deform-paths.md §1
pub(super) fn relink_targets(root: &mut Element, ctx: &mut Ctx) -> Result<()> {
    let Some(sh) = shared(root) else {
        return Ok(());
    };
    if !sh.children.iter().any(|c| c.name == EXT) {
        return Ok(());
    }
    // Keyforms some object still lists.
    let mut live: HashSet<String> = HashSet::new();
    let mut first_form: HashMap<String, String> = HashMap::new();
    for c in &sh.children {
        if c.name != "CArtMeshSource" {
            continue;
        }
        let mut stack = vec![c];
        while let Some(e) = stack.pop() {
            if e.field_name() == Some("keyforms") {
                for (k, f) in e.children.iter().enumerate() {
                    if let Some(r) = f.attr("xs.ref") {
                        live.insert(r.to_owned());
                        if let (0, Some(id)) = (k, c.attr("xs.id")) {
                            first_form.insert(id.to_owned(), r.to_owned());
                        }
                    }
                }
                continue;
            }
            stack.extend(e.children.iter());
        }
    }
    // Every MeshPointRef definition: inline in a path's target points, or
    // a shared one (paths copied from one another share them).
    let si = shared_index(root)?;
    let mut locs: Vec<Vec<u32>> = Vec::new();
    if let Some(sh) = shared(root) {
        for (k, c) in sh.children.iter().enumerate() {
            if c.name == "MeshPointRef" {
                locs.push(vec![si, k as u32]);
            }
            if c.name != EXT {
                continue;
            }
            let Some(tl) = c
                .children
                .iter()
                .position(|c| c.field_name() == Some("targetPoints"))
            else {
                continue;
            };
            for (ti, t) in c.children[tl].children.iter().enumerate() {
                if let Some(pi) = t
                    .children
                    .iter()
                    .position(|x| x.field_name() == Some("_point") && x.attr("xs.ref").is_none())
                {
                    locs.push(vec![si, k as u32, tl as u32, ti as u32, pi as u32]);
                }
            }
        }
    }
    // Keyform → its positions array (shared in place when inline).
    let mut positions: HashMap<String, String> = HashMap::new();
    for loc in locs {
        let pr = element_at(root, Some(&loc))?.clone();
        let field = |n: &str| pr.children.iter().find(|c| c.field_name() == Some(n));
        let Some(form) = field("keyForm").and_then(|f| f.attr("xs.ref")) else {
            continue;
        };
        let form = if live.contains(form) {
            form.to_owned()
        } else {
            match field("_artMeshSource")
                .and_then(|s| s.attr("xs.ref"))
                .and_then(|s| first_form.get(s))
            {
                Some(f) => f.clone(),
                None => continue,
            }
        };
        let pos = match positions.get(&form) {
            Some(p) => p.clone(),
            None => {
                let p = field_ref(root, ctx, &form, "positions")?;
                positions.insert(form.clone(), p.clone());
                p
            }
        };
        let old_pos = field("positions")
            .and_then(|f| f.attr("xs.ref"))
            .map(str::to_owned);
        let old_form = field("keyForm").and_then(|f| f.attr("xs.ref"));
        if old_pos.as_deref() == Some(pos.as_str()) && old_form == Some(form.as_str()) {
            continue;
        }
        let e = element_at(root, Some(&loc))?;
        for c in e.children.iter_mut() {
            match c.field_name() {
                Some("keyForm") => *c = stub("CArtMeshForm", Some("keyForm"), &form),
                Some("positions") => *c = stub("float-array", Some("positions"), &pos),
                _ => {}
            }
        }
        if let Some(o) = old_pos {
            ctx.orphans.push(o);
        }
    }
    Ok(())
}
