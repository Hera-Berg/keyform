//! Saves 3D rotation settings (`CRotate3DExtension`, declarative fields in
//! schema order) into the object's `_extensions`.
//! spec: 70-advanced/generators.md §1 "Storage", 10-formats/cmo3-schema.md

use keyform_model::{Model, Rotate3dConfig};

use super::element_at;
use super::grid::{field_path, scalar, set_count, stub, Ctx};
use super::objects::guid_id;
use super::unsupported;
use crate::error::Result;
use crate::graph::format_f32;
use crate::xml::Element;

fn el(tag: &str, name: &str) -> Element {
    Element::new(tag).with_attr("xs.n", name)
}

const METHODS: [&str; 3] = ["XY_ORDER", "YX_ORDER", "ARBITRARY_AXIS"];
const FILTERS: [&str; 4] = ["ALL_KEYFORMS", "X_DIRECTION", "Y_DIRECTION", "CORNERS"];

fn extension(
    root: &mut Element,
    ctx: &mut Ctx,
    owner_tag: &str,
    owner: &str,
    c: &Rotate3dConfig,
) -> Result<Element> {
    let f = |n: &str, v: f32| scalar("f", n, format_f32(v));
    let mut sup = el("ACExtension", "super");
    sup.children = vec![
        el("CExtensionGuid", "guid")
            .with_attr("uuid", keyform_model::new_guid())
            .with_attr("note", "(no debug info)"),
        stub(owner_tag, Some("_owner"), owner),
    ];
    let mut kids = vec![
        sup,
        f("cameraX", c.camera[0]),
        f("cameraY", c.camera[1]),
        f("centerX", c.centre[0]),
        f("centerY", c.centre[1]),
        f("rotationRangeX", c.range[0]),
        f("rotationRangeY", c.range[1]),
        scalar("b", "isSymmetryX", c.symmetry[0].to_string()),
        scalar("b", "isSymmetryY", c.symmetry[1].to_string()),
        f("zOffset", c.z_offset),
        f("perspective", c.perspective),
        el("RotationMethod", "rotationMethod")
            .with_attr("v", METHODS[usize::from(c.method.min(2))]),
    ];
    for (k, p) in c.parameters.iter().enumerate() {
        let name = format!("mappedParameterGuid{k}");
        match p.and_then(|i| ctx.params.get(i).cloned()) {
            Some(q) => {
                let id = guid_id(root, ctx, &q.guid, "CParameterGuid", &q.id)?;
                kids.push(stub("CParameterGuid", Some(&name), &id));
            }
            None => kids.push(el("null", &name)),
        }
    }
    kids.push(scalar(
        "b",
        "isDepthEstimationEnabled",
        c.depth_enabled.to_string(),
    ));
    match &c.depth {
        Some(d) => {
            let mut e = el("DepthEstimationData", "depthEstimationData");
            let mut buf =
                el("float-array", "depthBuffer").with_attr("count", d.buffer.len().to_string());
            buf.text = d
                .buffer
                .iter()
                .map(|v| format_f32(*v))
                .collect::<Vec<_>>()
                .join(" ");
            let form = if d.reference_form.is_empty() {
                el("null", "referenceFormGuid")
            } else {
                let id = guid_id(root, ctx, &d.reference_form, "CFormGuid", "(no debug info)")?;
                stub("CFormGuid", Some("referenceFormGuid"), &id)
            };
            e.children = vec![
                buf,
                scalar("i", "row", d.row.to_string()),
                scalar("i", "column", d.column.to_string()),
                f("cameraX", d.camera[0]),
                f("cameraY", d.camera[1]),
                f("centerX", d.centre[0]),
                f("centerY", d.centre[1]),
                f("rotationRangeX", d.range[0]),
                f("rotationRangeY", d.range[1]),
                f("zOffset", d.z_offset),
                f("perspective", d.perspective),
                el("RotationMethod", "rotationMethod")
                    .with_attr("v", METHODS[usize::from(d.method.min(2))]),
                form,
            ];
            kids.push(e);
        }
        None => kids.push(el("null", "depthEstimationData")),
    }
    kids.push(
        el("KeyformFilterKind", "keyformFilterKind")
            .with_attr("v", FILTERS[usize::from(c.filter.min(3))]),
    );
    let mut x = Element::new("CRotate3DExtension");
    x.children = kids;
    Ok(x)
}

pub(super) fn sync_rotate3d(
    root: &mut Element,
    ctx: &mut Ctx,
    old: &Model,
    new: &Model,
) -> Result<()> {
    if old.rotate3d == new.rotate3d {
        return Ok(());
    }
    let mut guids: Vec<&str> = new
        .rotate3d
        .iter()
        .chain(old.rotate3d.iter())
        .map(|x| x.0.as_str())
        .collect();
    guids.sort_unstable();
    guids.dedup();
    for g in guids {
        let a = old.rotate3d.iter().find(|x| x.0 == g).map(|x| &x.1);
        let b = new.rotate3d.iter().find(|x| x.0 == g).map(|x| &x.1);
        if a == b {
            continue;
        }
        let owner_tag = if new.art_meshes.iter().any(|x| x.info.guid == g) {
            "CArtMeshSource"
        } else if new.deformers.iter().any(|x| x.info.guid == g) {
            "CWarpDeformerSource"
        } else {
            continue; // the object was deleted
        };
        let Some(path) = ctx.object_paths.get(g).cloned() else {
            return unsupported("3D rotation settings on an object new in this save");
        };
        let Some(owner) = element_at(root, Some(&path))?
            .attr("xs.id")
            .map(str::to_owned)
        else {
            return unsupported("3D rotation settings on an unshared object");
        };
        let ext = match b {
            Some(c) => Some(extension(root, ctx, owner_tag, &owner, c)?),
            None => None,
        };
        let ep = field_path(root, &path, "_extensions")?;
        let l = element_at(root, Some(&ep))?;
        l.children.retain(|c| c.name != "CRotate3DExtension");
        l.children.extend(ext);
        set_count(l);
        ctx.structural = true;
    }
    Ok(())
}
