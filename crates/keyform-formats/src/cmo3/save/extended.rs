//! Writes the extended-interpolation extension of an object whose
//! generated grid changed: the old `CExtendedInterpolationExtension` goes,
//! a new one holds the generated grid (an extended binding source per
//! extended binding, original keys reusing their keyform GUIDs, inserted
//! keyforms as new forms). Structure as saved by the reference 5.3.04.
//! spec: 30-algorithms/extended-interpolation.md §1, 10-formats/cmo3-schema.md
//! (CExtendedInterpolationExtension, ExtendedKeyFormMorphTargetSet)

use std::collections::HashMap;

use keyform_eval::extended::{extended_grid, inserted_keys, is_extended, Points};
use keyform_eval::Blend;
use keyform_model::{BlendShapes, KeyformGrid};

use super::grid::{
    add_shared, field_path, get_shared, scalar, set_count, share_in_place, stub, uuid_of, Ctx,
};
use super::objects::guid_id;
use super::unsupported;
use super::{element_at, Fields, FormFields};
use crate::cmo3::ObjectSource;
use crate::error::{invalid, Result};
use crate::xml::Element;

const EXT: &str = "CExtendedInterpolationExtension";

fn list(name: &str, children: Vec<Element>) -> Element {
    let mut l = Element::new("array_list").with_attr("xs.n", name);
    l.children = children;
    set_count(&mut l);
    l
}

fn clist(name: &str, children: Vec<Element>) -> Element {
    let mut l = Element::new("carray_list").with_attr("xs.n", name);
    l.children = children;
    set_count(&mut l);
    l
}

fn floats(name: &str, v: &[f32]) -> Element {
    list(
        name,
        v.iter()
            .map(|x| {
                let mut f = Element::new("f");
                f.text = super::format_f32(*x);
                f
            })
            .collect(),
    )
}

/// Ids referenced (`xs.ref`) anywhere inside `e`.
fn refs(e: &Element, out: &mut Vec<String>) {
    if let Some(r) = e.attr("xs.ref") {
        out.push(r.to_owned());
    }
    for c in &e.children {
        refs(c, out);
    }
}

/// Rewrites the extension of the object at `src` when its generated grid
/// changed between `og` and `ng`.
pub(super) fn sync_extension<F: FormFields + Blend + Points>(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    what: &str,
    (og, ob): (&KeyformGrid<F>, &BlendShapes<F>),
    (ng, nb): (&KeyformGrid<F>, &BlendShapes<F>),
) -> Result<()> {
    let (oe, ne) = (extended_grid(og), extended_grid(ng));
    // Blend-shape settings (§4): the extension also exists for them.
    let same_settings = {
        let og_ = |i: usize| ctx.old_params.get(i).map(|p| p.guid.as_str());
        let ng_ = |i: usize| ctx.params.get(i).map(|p| p.guid.as_str());
        ob.extended.len() == nb.extended.len()
            && ob.extended.iter().zip(&nb.extended).all(|(x, y)| {
                og_(x.parameter) == ng_(y.parameter)
                    && (x.kind, x.points, x.scale) == (y.kind, y.points, y.scale)
            })
    };
    let blend_same = same_settings && super::grid::same_blend(ctx, ob, nb, true);
    if oe.is_none() && ne.is_none() && ob.extended.is_empty() && nb.extended.is_empty() {
        return Ok(());
    }
    if oe == ne && og.guids == ng.guids && blend_same {
        return Ok(());
    }
    let Some(obj) = src.element.clone() else {
        return invalid(format!("{what}: no element"));
    };
    ctx.structural = true;
    // Drop the old extension (its own bindings and form GUIDs may become
    // orphans).
    let ext_list = field_path(root, &obj, "_extensions")?;
    let items = element_at(root, Some(&ext_list))?.children.clone();
    let mut keep = Vec::new();
    for c in items {
        if c.name == EXT {
            let body = match c.attr("xs.ref") {
                Some(id) => {
                    ctx.orphans.push(id.to_owned());
                    get_shared(root, id).cloned().unwrap_or_else(|| c.clone())
                }
                None => c.clone(),
            };
            refs(&body, &mut ctx.orphans);
        } else {
            keep.push(c);
        }
    }
    {
        let l = element_at(root, Some(&ext_list))?;
        l.children = keep;
        set_count(l);
    }
    // Enabled for an extended grid or blend-shape settings; with no
    // extended grid binding the grid source is a plain copy (§4).
    let g = match ne {
        Some(g) => g,
        None if !nb.extended.is_empty() => {
            let mut g = ng.clone();
            g.guids = Vec::new();
            g
        }
        None => return Ok(()),
    };
    if ng.guids.len() != ng.keyforms.len() {
        return unsupported(format!("{what}: keyforms without identities"));
    }

    // The object's own element id (the extension's owner).
    let Some(owner) = element_at(root, Some(&obj))?
        .attr("xs.id")
        .map(str::to_owned)
    else {
        return unsupported(format!(
            "{what}: an inline object with extended interpolation"
        ));
    };
    let owner_tag = element_at(root, Some(&obj))?.name.clone();
    // Grid source (shared) and its bindings by parameter GUID.
    let gs_field = field_path(root, &obj, "keyformGridSource")?;
    let gs = share_in_place(root, ctx, &gs_field)?;
    let mut main_bindings: HashMap<String, String> = HashMap::new();
    if let Some(l) = get_shared(root, &gs)
        .and_then(|e| e.child_field("keyformBindings"))
        .cloned()
    {
        for b in &l.children {
            let Some(id) = b.attr("xs.ref") else { continue };
            let pg = get_shared(root, id)
                .and_then(|e| e.child_field("parameterGuid"))
                .and_then(|x| uuid_of(root, x));
            if let Some(pg) = pg {
                main_bindings.insert(pg, id.to_owned());
            }
        }
    }
    // Binding ids of the extended grid: a new source per extended binding.
    let mut binding_ids = Vec::new();
    let mut axes: Vec<Option<Vec<(usize, f32)>>> = Vec::new();
    for b in &ng.bindings {
        let Some(param) = ctx.params.get(b.parameter).cloned() else {
            return invalid(format!("{what}: binding to unknown parameter"));
        };
        let Some(main) = main_bindings.get(&param.guid).cloned() else {
            return invalid(format!("{what}: binding source not found"));
        };
        if !is_extended(b) {
            binding_ids.push(main);
            axes.push(None);
            continue;
        }
        let n = b.insert_point_count.max(1) as usize;
        let ins = inserted_keys(&b.keys, n);
        let pg = get_shared(root, &main)
            .and_then(|e| e.child_field("parameterGuid"))
            .and_then(|x| x.attr("xs.ref"))
            .map(str::to_owned);
        let Some(pg) = pg else {
            return invalid(format!("{what}: binding without parameter"));
        };
        let mut e = Element::new("KeyformBindingSource");
        e.children = vec![
            stub("KeyformGridSource", Some("_gridSource"), &gs),
            stub("CParameterGuid", Some("parameterGuid"), &pg),
            floats("keys", &ins.iter().map(|k| k.0).collect::<Vec<_>>()),
            Element::new("InterpolationType")
                .with_attr("xs.n", "interpolationType")
                .with_attr("v", b.interpolation.clone()),
            Element::new("ExtendedInterpolationType")
                .with_attr("xs.n", "extendedInterpolationType")
                .with_attr("v", super::grid::extended_name(b.extended)),
            scalar("i", "insertPointCount", b.insert_point_count.to_string()),
            scalar(
                "f",
                "extendedInterpolationScale",
                super::format_f32(b.extended_scale),
            ),
            scalar("s", "description", param.id.clone()),
        ];
        binding_ids.push(add_shared(root, ctx, e)?);
        axes.push(Some(ins.iter().map(|k| (k.1, k.2)).collect()));
    }

    // Template keyform element: the object's first grid keyform.
    let list_path = field_path(root, &obj, "keyforms")?;
    let first = element_at(root, Some(&list_path))?
        .children
        .first()
        .cloned();
    let template = match first {
        Some(c) => match c.attr("xs.ref") {
            Some(id) => get_shared(root, id).cloned(),
            None => Some(c),
        },
        None => None,
    };
    let Some(mut template) = template else {
        return invalid(format!("{what}: no keyform to copy"));
    };
    template
        .attrs
        .retain(|(k, _)| k != "xs.id" && k != "xs.idx");
    let tuuid =
        super::grid::form_guid_field_pub(&mut template.clone()).and_then(|x| uuid_of(root, x));
    let base = tuuid
        .and_then(|u| ng.guids.iter().position(|g| *g == u))
        .and_then(|i| ng.keyforms.get(i))
        .unwrap_or(&ng.keyforms[0]);

    // Cells: original keys reuse their keyform GUIDs; inserted ones are new.
    let dims: Vec<usize> = g.bindings.iter().map(|b| b.keys.len()).collect();
    let src_dims: Vec<usize> = ng.bindings.iter().map(|b| b.keys.len()).collect();
    let mut on_grid = Vec::new();
    let mut forms = Vec::new();
    for (cell, f) in g.keyforms.iter().enumerate() {
        let mut rest = cell;
        let mut idx = Vec::new();
        let mut original = Some(0usize);
        let mut stride = 1;
        for (j, &n) in dims.iter().enumerate() {
            let k = rest % n;
            rest /= n;
            idx.push(k);
            let orig_k = match &axes[j] {
                None => Some(k),
                Some(a) => (a[k].1 == 0.0).then_some(a[k].0),
            };
            original = match (original, orig_k) {
                (Some(o), Some(ok)) => Some(o + ok * stride),
                _ => None,
            };
            stride *= src_dims[j];
        }
        let guid_ref = match original {
            Some(oc) => guid_id(root, ctx, &ng.guids[oc], "CFormGuid", "")?,
            None => {
                let id = add_shared(
                    root,
                    ctx,
                    Element::new("CFormGuid")
                        .with_attr("uuid", keyform_model::new_guid())
                        .with_attr("note", "(no debug info)"),
                )?;
                let mut e = template.clone();
                F::write(&mut Fields { e: &mut e }, base, f)?;
                if let Some(gf) = super::grid::form_guid_field_pub(&mut e) {
                    *gf = stub("CFormGuid", Some("guid"), &id);
                }
                forms.push(e);
                id
            }
        };
        let kops: Vec<Element> = idx
            .iter()
            .enumerate()
            .map(|(j, k)| {
                let mut kop = Element::new("KeyOnParameter");
                kop.children = vec![
                    stub("KeyformBindingSource", Some("binding"), &binding_ids[j]),
                    scalar("i", "keyIndex", k.to_string()),
                ];
                kop
            })
            .collect();
        let mut access = Element::new("KeyformGridAccessKey").with_attr("xs.n", "accessKey");
        access.children.push(list("_keyOnParameterList", kops));
        let mut kog = Element::new("KeyformOnGrid");
        kog.children = vec![access, stub("CFormGuid", Some("keyformGuid"), &guid_ref)];
        on_grid.push(kog);
    }
    let mut grid_src =
        Element::new("KeyformGridSource").with_attr("xs.n", "extendedKeyformGridSource");
    grid_src.children = vec![
        list("keyformsOnGrid", on_grid),
        list(
            "keyformBindings",
            binding_ids
                .iter()
                .map(|id| stub("KeyformBindingSource", None, id))
                .collect(),
        ),
    ];
    // Extended blend-shape sets (§4): only the inserted points, their
    // forms in `morphTargetExtendedKeyforms` in the same order, and the
    // settings map (entry form, LINEAR included).
    let mut targets = Vec::new();
    let mut morph_forms = Vec::new();
    let mut entries = Vec::new();
    for x in &nb.extended {
        let Some(param) = ctx.params.get(x.parameter).cloned() else {
            continue;
        };
        // Entries for parameters without blend shapes any more go.
        if !nb.shapes.iter().any(|s| s.parameter == x.parameter) {
            continue;
        }
        let pg = guid_id(root, ctx, &param.guid, "CParameterGuid", &param.id)?;
        for (k, f) in keyform_eval::extended::blend_inserted(ctx.params, ng, nb, x) {
            let fid = add_shared(
                root,
                ctx,
                Element::new("CFormGuid")
                    .with_attr("uuid", keyform_model::new_guid())
                    .with_attr("note", "(no debug info)"),
            )?;
            let mut e = template.clone();
            F::write(&mut Fields { e: &mut e }, base, &f)?;
            if let Some(gf) = super::grid::form_guid_field_pub(&mut e) {
                *gf = stub("CFormGuid", Some("guid"), &fid);
            }
            morph_forms.push(e);
            let mut t = Element::new("ExtendedKeyFormMorphTarget");
            t.children = vec![
                stub("CFormGuid", Some("keyformGuid"), &fid),
                stub("CParameterGuid", Some("parameterGuid"), &pg),
                scalar("f", "keyValue", super::format_f32(k)),
            ];
            targets.push(t);
        }
        let mut setting = Element::new("ExtendedInterpolationSetting").with_attr("xs.n", "value");
        setting.children = vec![
            Element::new("ExtendedInterpolationType")
                .with_attr("xs.n", "type")
                .with_attr("v", super::grid::extended_name(x.kind)),
            scalar("i", "insertPointCount", x.points.to_string()),
            scalar(
                "f",
                "extendedInterpolationScale",
                super::format_f32(x.scale),
            ),
        ];
        let mut entry = Element::new("entry");
        entry.children = vec![stub("CParameterGuid", Some("key"), &pg), setting];
        entries.push(entry);
    }
    let mut settings = Element::new("hash_map")
        .with_attr("xs.n", "extendedInterpolationSettings")
        .with_attr("count", entries.len().to_string());
    if entries.is_empty() {
        settings.set_attr("keyType", "string");
    }
    settings.children = entries;
    let mut morph = Element::new("ExtendedKeyFormMorphTargetSet")
        .with_attr("xs.n", "_extendedKeyformMorphTargetSet");
    morph.children = vec![clist("_morphTargets", targets), settings];
    let mut sup = Element::new("ACExtension").with_attr("xs.n", "super");
    sup.children = vec![
        Element::new("CExtensionGuid")
            .with_attr("xs.n", "guid")
            .with_attr("uuid", keyform_model::new_guid())
            .with_attr("note", "(no debug info)"),
        stub(&owner_tag, Some("_owner"), &owner),
    ];
    let mut ext = Element::new(EXT);
    ext.children = vec![
        sup,
        grid_src,
        clist("normalExtendedKeyforms", forms),
        morph,
        clist("morphTargetExtendedKeyforms", morph_forms),
    ];
    let l = element_at(root, Some(&ext_list))?;
    l.children.push(ext);
    set_count(l);
    Ok(())
}
