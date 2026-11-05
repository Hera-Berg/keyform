//! Writes an art mesh's topology: vertices, UVs, triangles and the
//! editable-mesh graph. spec: 10-formats/cmo3.md §4

use std::fmt::Display;

use keyform_model::ArtMesh;

use super::grid::{field_path, find_shared, set_count, stub, uuid_of, Ctx};
use super::{element_at, set_array, set_array_text, unsupported};
use crate::error::Result;
use crate::xml::Element;

/// Whether `o` and `n` differ in anything [`write_topology`] writes.
pub(super) fn topology_changed(o: &ArtMesh, n: &ArtMesh) -> bool {
    o.positions != n.positions
        || o.uvs != n.uvs
        || o.indices != n.indices
        || o.point_uids != n.point_uids
        || o.mesh != n.mesh
}

fn join<T: Display>(values: impl Iterator<Item = T>) -> String {
    values.map(|v| v.to_string()).collect::<Vec<_>>().join(" ")
}

/// Writes `n`'s vertices, UVs, triangles and graph into the art mesh at
/// `path`. The keyforms are written separately.
pub(super) fn write_topology(root: &mut Element, path: &[u32], n: &ArtMesh) -> Result<()> {
    let nv = n.vertex_count();
    if n.uvs.len() != 2 * nv
        || n.point_uids.len() != nv
        || n.indices.iter().any(|&i| i as usize >= nv)
        || n.grid.keyforms.iter().any(|f| f.positions.len() != 2 * nv)
        || n.blend_shapes
            .shapes
            .iter()
            .any(|s| s.form.positions.len() != 2 * nv)
    {
        return unsupported(format!("{}: inconsistent mesh arrays", n.info.id));
    }
    if nv > usize::from(u16::MAX) {
        return unsupported(format!("{}: more than 65535 vertices", n.info.id));
    }
    let Ok(uids) = n
        .point_uids
        .iter()
        .map(|&u| i32::try_from(u))
        .collect::<std::result::Result<Vec<i32>, _>>()
    else {
        return unsupported(format!("{}: vertex UID out of range", n.info.id));
    };
    let e = element_at(root, Some(path))?;
    set_array(e, "positions", "float-array", &n.positions);
    set_array(e, "uvs", "float-array", &n.uvs);
    set_array_text(
        e,
        "indices",
        "int-array",
        n.indices.len(),
        join(n.indices.iter()),
    );

    // The editable mesh lives in the CEditableMeshExtension (inline in the
    // extension list, or shared).
    let list = field_path(root, path, "_extensions")?;
    let items = element_at(root, Some(&list))?.children.clone();
    let mut ext_path = None;
    for (k, c) in items.iter().enumerate() {
        if c.name != "CEditableMeshExtension" {
            continue;
        }
        let mut p = list.clone();
        p.push(k as u32);
        ext_path = Some((p, c.attr("xs.ref").map(str::to_owned)));
        break;
    }
    let Some((p, shared_id)) = ext_path else {
        return unsupported(format!("{}: art mesh has no editable mesh", n.info.id));
    };
    fn ext_mut<'e>(root: &'e mut Element, p: &[u32], id: Option<&str>) -> Result<&'e mut Element> {
        match id {
            Some(id) => find_shared(root, id),
            None => element_at(root, Some(p)),
        }
    }
    let ext = ext_mut(root, &p, shared_id.as_deref())?;
    let Some(i) = ext
        .children
        .iter()
        .position(|c| c.field_name() == Some("editableMesh"))
    else {
        return unsupported(format!("{}: art mesh has no editable mesh", n.info.id));
    };
    let em = match ext.children[i].attr("xs.ref").map(str::to_owned) {
        Some(id) => find_shared(root, &id)?,
        None => &mut ext_mut(root, &p, shared_id.as_deref())?.children[i],
    };
    let m = &n.mesh;
    em.set_attr("nextPointUid", m.next_point_uid.to_string());
    em.set_attr("useDelaunayTriangulation", m.delaunay.to_string());
    set_array(em, "point", "float-array", &n.positions);
    let mut priority = m.point_priority.clone();
    priority.resize(nv, keyform_model::POINT_NORMAL);
    set_array_text(em, "pointPriority", "byte-array", nv, join(priority.iter()));
    // Edge indices are stored as signed shorts.
    let edges = m.edges.iter().flatten().map(|&i| i as u16 as i16);
    set_array_text(em, "edge", "short-array", 2 * m.edges.len(), join(edges));
    set_array_text(
        em,
        "edgePriority",
        "byte-array",
        m.edge_priority.len(),
        join(m.edge_priority.iter()),
    );
    set_array_text(em, "pointUid", "int-array", nv, join(uids.iter()));
    Ok(())
}

/// Writes changed automatic-mesh settings into the art mesh's
/// `CMeshGeneratorExtension` (auto-mesh.md §2). Older files' unused
/// `polygonDensity` is replaced by the two current fields.
pub(super) fn write_generator(root: &mut Element, path: &[u32], n: &ArtMesh) -> Result<()> {
    let list = field_path(root, path, "_extensions")?;
    let items = element_at(root, Some(&list))?.children.clone();
    let Some((k, c)) = items
        .iter()
        .enumerate()
        .find(|(_, c)| c.name == "CMeshGeneratorExtension")
    else {
        return unsupported(format!("{}: art mesh has no mesh generator", n.info.id));
    };
    let ext = match c.attr("xs.ref") {
        Some(id) => find_shared(root, id)?,
        None => {
            let mut p = list.clone();
            p.push(k as u32);
            element_at(root, Some(&p))?
        }
    };
    let Some(i) = ext
        .children
        .iter()
        .position(|c| c.field_name() == Some("meshGenerateSetting"))
    else {
        return unsupported(format!("{}: art mesh has no mesh generator", n.info.id));
    };
    if ext.children[i].attr("xs.ref").is_some() {
        return unsupported(format!("{}: shared mesh generator settings", n.info.id));
    }
    let e = &mut ext.children[i];
    let g = &n.generator;
    // Fields in schema order.
    let fields = [
        ("polygonOuterDensity", g.outer_density),
        ("polygonInnerDensity", g.inner_density),
        ("polygonMargin", g.margin),
        ("polygonInnerMargin", g.inner_margin),
        ("polygonMinMargin", g.min_margin),
        ("polygonMinBoundsPt", g.min_bounds_pt),
        ("thresholdAlpha", g.threshold_alpha),
    ];
    let mut children: Vec<Element> = fields
        .iter()
        .map(|(name, v)| {
            let mut c = Element::new("i").with_attr("xs.n", *name);
            c.text = v.to_string();
            c
        })
        .collect();
    // Keep anything else the element holds.
    let known = |c: &Element| {
        c.field_name()
            .is_some_and(|f| f == "polygonDensity" || fields.iter().any(|(n, _)| *n == f))
    };
    children.extend(e.children.iter().filter(|c| !known(c)).cloned());
    e.children = children;
    Ok(())
}

/// GUIDs of `a`'s clip sources in `m`.
pub(super) fn clip_guids(m: &keyform_model::Model, a: &ArtMesh) -> Vec<String> {
    a.clip
        .iter()
        .filter_map(|&k| m.art_meshes.get(k))
        .map(|x| x.info.guid.clone())
        .collect()
}

/// Rewrites `clipGuidList` (spec: 40-rendering/clipping.md, 50-ui/dialogs.md
/// §21). While the mesh still references a deleted mask (`clip_missing`),
/// the unresolved entries stay, and masks deleted in this edit are added
/// as such, like the reference does.
pub(super) fn write_clip_list(
    root: &mut Element,
    ctx: &mut Ctx,
    path: &[u32],
    o: &ArtMesh,
    n: &ArtMesh,
) -> Result<()> {
    let (old, new) = (ctx.old_model, ctx.new_model);
    let live = |u: &str| new.art_meshes.iter().any(|a| a.info.guid == u);
    let was = |u: &str| old.art_meshes.iter().any(|a| a.info.guid == u);
    let list = field_path(root, path, "clipGuidList")?;
    let existing = element_at(root, Some(&list))?.children.clone();
    let mut entries = Vec::new();
    for g in clip_guids(new, n) {
        let note = new
            .art_meshes
            .iter()
            .find(|a| a.info.guid == g)
            .map_or(String::new(), |a| a.info.display_name().to_owned());
        let id = super::objects::guid_id(root, ctx, &g, "CDrawableGuid", &note)?;
        entries.push(stub("CDrawableGuid", None, &id));
    }
    if n.clip_missing {
        // Entries that were already unresolved.
        for e in &existing {
            if uuid_of(root, e).is_none_or(|u| !was(&u) && !live(&u)) {
                entries.push(e.clone());
            }
        }
        // Masks deleted now.
        for g in clip_guids(old, o) {
            if !live(&g) {
                let id = super::objects::guid_id(root, ctx, &g, "CDrawableGuid", "")?;
                entries.push(stub("CDrawableGuid", None, &id));
            }
        }
    }
    let l = element_at(root, Some(&list))?;
    l.children = entries;
    set_count(l);
    ctx.structural = true;
    Ok(())
}
