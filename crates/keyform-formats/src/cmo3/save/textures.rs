//! Writes new texture-manager data: layered images (imported PSDs) with
//! their layer trees and pixels, model-image groups and model images with
//! their input filter graphs, and atlas pages. Existing entries are kept as
//! they are. spec: 70-advanced/psd-import.md §5.1, §8,
//! 70-advanced/texture-atlas.md §1, 10-formats/cmo3-schema.md

use std::collections::{BTreeSet, HashMap};

use keyform_model::{ImageLayer, LayerCommon, LayerEntry, Model};

use super::grid::{
    add_shared, field_path, find_shared, get_shared, scalar, set_count, shared, shared_mut, stub,
    uuid_of, Ctx,
};
use super::objects::guid_id;
use super::{element_at, format_f32, unsupported, xml_text};
use crate::caff::{CaffArchive, CaffEntry, Compression};
use crate::error::{invalid, Result};
use crate::image::RgbaImage;
use crate::xml::Element;

/// Ids of what this save wrote, for the art meshes that use it.
#[derive(Default)]
pub(crate) struct TexState {
    /// New archive entries (path, PNG).
    pub entries: Vec<(String, Vec<u8>)>,
    /// Classes used by new elements (for `<?import?>`).
    pub imports: BTreeSet<&'static str>,
    /// Layer GUID → (CLayer id, CImageResource id).
    pub layers: HashMap<String, (String, String)>,
    /// Texture index → (CTextureAtlas id, GTexture2D id).
    pub atlases: HashMap<usize, (String, String)>,
    /// Archive paths of image resources this save dropped.
    pub dropped: Vec<String>,
}

/// Classes used by the texture-manager elements written here.
pub(super) const TEXTURE_IMPORTS: &[&str] = &[
    "com.live2d.cubism.doc.model.texture.LayeredImageWrapper",
    "com.live2d.cubism.doc.resources.CLayeredImage",
    "com.live2d.cubism.doc.resources.CLayerGroup",
    "com.live2d.cubism.doc.resources.ACLayerGroup",
    "com.live2d.cubism.doc.resources.ACLayerEntry",
    "com.live2d.cubism.doc.resources.CLayer",
    "com.live2d.cubism.doc.resources.ACImageLayer",
    "com.live2d.cubism.doc.resources.CLayerIdentifier",
    "com.live2d.cubism.doc.resources.LayerSet",
    "com.live2d.type.CLayerGuid",
    "com.live2d.type.CLayeredImageGuid",
    "com.live2d.type.CRect",
    "com.live2d.type.CImageIcon",
    "com.live2d.graphics.CWritableImage",
    "com.live2d.graphics.CImageResource",
    "com.live2d.cubism.doc.model.texture.modelImage.CModelImageGroup",
    "com.live2d.cubism.doc.model.texture.modelImage.CModelImage",
    "com.live2d.type.CModelImageGuid",
    "com.live2d.cubism.doc.model.extension.textureInput.inputFilter.ModelImageFilterSet",
    "com.live2d.graphics.filter.FilterSet",
    "com.live2d.graphics.filter.filterInstance.FilterInstance",
    "com.live2d.graphics.filter.id.FilterInstanceId",
    "com.live2d.graphics.filter.id.FilterValueId",
    "com.live2d.graphics.filter.FilterValue",
    "com.live2d.type.StaticFilterDefGuid",
    "com.live2d.graphics.filter.concreteConnector.EnvValueConnector",
    "com.live2d.graphics.filter.AValueConnector",
    "com.live2d.graphics.filter.concreteConnector.FilterOutputValueConnector",
    "com.live2d.graphics.filter.FilterSet$EnvConnection",
    "com.live2d.cubism.doc.model.extension.textureInput.inputFilter.ModelImageFilterEnv",
    "com.live2d.graphics.filter.FilterEnv",
    "com.live2d.graphics.filter.FilterEnv$EnvValueSet",
    "com.live2d.cubism.doc.model.extension.textureInput.inputFilter.CLayerSelectorMap",
    "com.live2d.cubism.doc.model.extension.textureInput.inputFilter.CLayerInputData",
    "com.live2d.type.CAffine",
    "com.live2d.graphics.cachedImage.CCachedImageManager",
    "com.live2d.graphics.cachedImage.CachedImageType",
    "com.live2d.graphics.cachedImage.CCachedImage",
    "com.live2d.type.CSize",
    "com.live2d.cubism.doc.model.texture.textureAtlas.CTextureAtlas",
    "com.live2d.type.CTextureAtlasGuid",
    "com.live2d.cubism.doc.model.texture.textureAtlas.CTextureAtlas$ModelImageEntry",
    "com.live2d.graphics3d.component.GTransform2",
    "com.live2d.graphics3d.type.GVector2",
    "com.live2d.graphics3d.texture.GTexture2D",
    "com.live2d.graphics3d.texture.GTexture",
    "com.live2d.graphics3d.texture.WrapMode",
    "com.live2d.graphics3d.texture.GTexture$FilterMode",
    "com.live2d.graphics3d.texture.MinFilter",
    "com.live2d.graphics3d.texture.MagFilter",
    "com.live2d.type.GTextureGuid",
    "com.live2d.graphics3d.texture.Anisotropy",
    "com.live2d.graphics.psd.blend.ACBlend",
];

fn el(tag: &str, name: &str) -> Element {
    Element::new(tag).with_attr("xs.n", name)
}

fn list(tag: &str, name: &str, children: Vec<Element>) -> Element {
    let mut l = el(tag, name);
    l.children = children;
    set_count(&mut l);
    l
}

fn with(mut e: Element, children: Vec<Element>) -> Element {
    e.children = children;
    e
}

fn null(name: &str) -> Element {
    el("null", name)
}

pub(super) fn affine_el(name: &str, a: [f32; 6]) -> Element {
    let mut e = el("CAffine", name);
    for (k, n) in ["m00", "m01", "m02", "m10", "m11", "m12"]
        .iter()
        .enumerate()
    {
        e.set_attr(n, format_f32(a[k]));
    }
    e
}

fn long(name: &str, v: i64) -> Element {
    scalar("l", name, v.to_string())
}

fn file(name: &str, path: &str) -> Element {
    el("file", name).with_attr("path", path)
}

/// A PNG archive entry under a unique `base_N.png` name.
fn add_png(archive: &CaffArchive, tex: &mut TexState, base: &str, png: Vec<u8>) -> String {
    let mut path = archive.unique_path(&format!("{base}.png"));
    // Also unique among this save's new entries.
    let mut n = 0;
    while tex.entries.iter().any(|(p, _)| *p == path) {
        path = archive.unique_path(&format!("{base}_k{n}.png"));
        n += 1;
    }
    tex.entries.push((path.clone(), png));
    path
}

/// A shared `CImageResource` holding `png` (w × h). Returns its id.
fn image_resource(
    root: &mut Element,
    ctx: &mut Ctx,
    archive: &CaffArchive,
    png: Vec<u8>,
    (w, h): (u32, u32),
) -> Result<String> {
    let len = png.len();
    let tex = &mut ctx.tex;
    let path = add_png(archive, tex, "imageFileBuf", png);
    let e = with(
        Element::new("CImageResource")
            .with_attr("width", w.to_string())
            .with_attr("height", h.to_string())
            .with_attr("type", "INT_ARGB")
            .with_attr("imageFileBuf_size", len.to_string())
            .with_attr("previewFileBuf_size", "0"),
        vec![file("imageFileBuf", &path)],
    );
    add_shared(root, ctx, e)
}

/// A square thumbnail (the image fitted and centred) for icons.
pub(super) fn thumbnail(img: &RgbaImage, size: u32) -> RgbaImage {
    let mut out = RgbaImage::new(size, size);
    if img.width == 0 || img.height == 0 {
        return out;
    }
    let s = (size as f32 / img.width as f32).min(size as f32 / img.height as f32);
    let (w, h) = (img.width as f32 * s, img.height as f32 * s);
    let (ox, oy) = ((size as f32 - w) / 2.0, (size as f32 - h) / 2.0);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = ((x as f32 + 0.5 - ox) / s, (y as f32 + 0.5 - oy) / s);
            if fx < 0.0 || fy < 0.0 || fx >= img.width as f32 || fy >= img.height as f32 {
                continue;
            }
            // Box average over the source footprint.
            let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
            let r = ((1.0 / s).ceil() as u32).max(1);
            let mut acc = [0u32; 4];
            let mut n = 0;
            for yy in y0..(y0 + r).min(img.height) {
                for xx in x0..(x0 + r).min(img.width) {
                    let j = 4 * (yy * img.width + xx) as usize;
                    for c in 0..4 {
                        acc[c] += u32::from(img.data[j + c]);
                    }
                    n += 1;
                }
            }
            let o = 4 * (y * size + x) as usize;
            for c in 0..4 {
                out.data[o + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    out
}

/// A `CImageIcon` field with a `size`² thumbnail of `img`.
pub(super) fn icon(
    archive: &CaffArchive,
    tex: &mut TexState,
    name: &str,
    img: &RgbaImage,
    size: u32,
) -> Result<Element> {
    let png = thumbnail(img, size).encode_png()?;
    let path = add_png(archive, tex, "image", png);
    Ok(with(
        el("CImageIcon", name),
        vec![with(
            el("CWritableImage", "image")
                .with_attr("width", size.to_string())
                .with_attr("height", size.to_string())
                .with_attr("type", "INT_ARGB"),
            vec![file("image", &path)],
        )],
    ))
}

/// Shared instance of a PSD blend class (`CBlend_Normal`, …), found or made.
fn blend_id(root: &mut Element, ctx: &mut Ctx, key: &[u8; 4]) -> Result<String> {
    let class = crate::cmo3::PSD_BLEND_CLASSES
        .iter()
        .find(|(k, _)| *k == key)
        .map_or("NotSupported", |(_, c)| *c);
    let tag = format!("CBlend_{class}");
    if let Some(id) = shared(root).and_then(|s| {
        s.children
            .iter()
            .find(|c| c.name == tag)
            .and_then(|c| c.attr("xs.id"))
            .map(str::to_owned)
    }) {
        return Ok(id);
    }
    ctx.tex.imports.insert(match class {
        "Normal" => "com.live2d.graphics.psd.blend.CBlend_Normal",
        "Multiply" => "com.live2d.graphics.psd.blend.CBlend_Multiply",
        "Screen" => "com.live2d.graphics.psd.blend.CBlend_Screen",
        "LinearDodge" => "com.live2d.graphics.psd.blend.CBlend_LinearDodge",
        "Darken" => "com.live2d.graphics.psd.blend.CBlend_Darken",
        "Lighten" => "com.live2d.graphics.psd.blend.CBlend_Lighten",
        "ColorBurn" => "com.live2d.graphics.psd.blend.CBlend_ColorBurn",
        "ColorDodge" => "com.live2d.graphics.psd.blend.CBlend_ColorDodge",
        "LinearBurn" => "com.live2d.graphics.psd.blend.CBlend_LinearBurn",
        "Overray" => "com.live2d.graphics.psd.blend.CBlend_Overray",
        "SoftLight" => "com.live2d.graphics.psd.blend.CBlend_SoftLight",
        "HardLight" => "com.live2d.graphics.psd.blend.CBlend_HardLight",
        "LinearLight" => "com.live2d.graphics.psd.blend.CBlend_LinearLight",
        "VividLight" => "com.live2d.graphics.psd.blend.CBlend_VividLight",
        "PinLight" => "com.live2d.graphics.psd.blend.CBlend_PinLight",
        "HardMix" => "com.live2d.graphics.psd.blend.CBlend_HardMix",
        "Difference" => "com.live2d.graphics.psd.blend.CBlend_Difference",
        "Exclusion" => "com.live2d.graphics.psd.blend.CBlend_Exclusion",
        "Subtract" => "com.live2d.graphics.psd.blend.CBlend_Subtract",
        "Divide" => "com.live2d.graphics.psd.blend.CBlend_Divide",
        "Hue" => "com.live2d.graphics.psd.blend.CBlend_Hue",
        "Saturation" => "com.live2d.graphics.psd.blend.CBlend_Saturation",
        "Color" => "com.live2d.graphics.psd.blend.CBlend_Color",
        "Luminosity" => "com.live2d.graphics.psd.blend.CBlend_Luminosity",
        "Dissolve" => "com.live2d.graphics.psd.blend.CBlend_Dissolve",
        "DarkerColor" => "com.live2d.graphics.psd.blend.CBlend_DarkerColor",
        "LighterColor" => "com.live2d.graphics.psd.blend.CBlend_LighterColor",
        "PassThrough" => "com.live2d.graphics.psd.blend.CBlend_PassThrough",
        _ => "com.live2d.graphics.psd.blend.CBlend_NotSupported",
    });
    add_shared(
        root,
        ctx,
        with(
            Element::new(&tag),
            vec![Element::new("ACBlend").with_attr("xs.n", "super")],
        ),
    )
}

/// The `ACLayerEntry` super element shared by groups and layers.
fn layer_entry(
    root: &mut Element,
    ctx: &mut Ctx,
    c: &LayerCommon,
    parent: Option<&str>,
    image: &str,
    options: Element,
) -> Result<Element> {
    let blend = blend_id(root, ctx, &c.blend)?;
    Ok(with(
        el("ACLayerEntry", "super"),
        vec![
            scalar("s", "name", xml_text(&c.name)),
            scalar("s", "memo", ""),
            scalar("b", "isVisible", c.visible.to_string()),
            scalar("b", "isClipping", c.clipping.to_string()),
            stub(&blend_tag(root, &blend), Some("blend"), &blend),
            el("CLayerGuid", "guid")
                .with_attr("uuid", c.guid.clone())
                .with_attr("note", "(no debug info)"),
            match parent {
                Some(p) => stub("CLayerGroup", Some("group"), p),
                None => null("group"),
            },
            scalar("i", "opacity255", c.opacity.to_string()),
            options,
            stub("CLayeredImage", Some("_layeredImage"), image),
        ],
    ))
}

fn blend_tag(root: &Element, id: &str) -> String {
    shared(root)
        .and_then(|s| s.children.iter().find(|c| c.attr("xs.id") == Some(id)))
        .map_or("CBlend_Normal".to_owned(), |c| c.name.clone())
}

fn identifier(c: &LayerCommon) -> Element {
    let value = c
        .layer_id
        .as_deref()
        .and_then(|s| u32::from_str_radix(&s.replace('-', ""), 16).ok());
    with(
        el("CLayerIdentifier", "layerIdentifier"),
        vec![
            scalar("s", "layerName", xml_text(&c.name)),
            match &c.layer_id {
                Some(id) => scalar("s", "layerId", id.clone()),
                None => null("layerId"),
            },
            scalar(
                "i",
                "layerIdValue_testImpl",
                value.map_or(-1, i64::from).to_string(),
            ),
        ],
    )
}

/// Writes one layer tree level; returns the entries' ids in Layers-panel
/// order (pre-order) for the layer set, and the children's stubs.
fn write_entries(
    root: &mut Element,
    ctx: &mut Ctx,
    archive: &CaffArchive,
    entries: &[LayerEntry],
    parent: &str,
    image: &str,
    flat: &mut Vec<Element>,
) -> Result<Vec<Element>> {
    let mut stubs = Vec::new();
    for e in entries {
        match e {
            LayerEntry::Group(g) => {
                let opts = with(
                    el("hash_map", "_optionOfIOption")
                        .with_attr("count", "0")
                        .with_attr("keyType", "string"),
                    vec![],
                );
                let entry = layer_entry(root, ctx, &g.common, Some(parent), image, opts)?;
                // Reserve the id first: the children refer to their group.
                let id = add_shared(root, ctx, Element::new("CLayerGroup"))?;
                flat.push(stub("CLayerGroup", None, &id));
                let kids = write_entries(root, ctx, archive, &g.children, &id, image, flat)?;
                let body = vec![
                    with(
                        el("ACLayerGroup", "super"),
                        vec![entry, list("carray_list", "_children", kids)],
                    ),
                    identifier(&g.common),
                ];
                let e = find_shared(root, &id)?;
                e.children = body;
                stubs.push(stub("CLayerGroup", None, &id));
            }
            LayerEntry::Layer(l) => {
                let id = write_layer(root, ctx, archive, l, parent, image)?;
                flat.push(stub("CLayer", None, &id));
                stubs.push(stub("CLayer", None, &id));
            }
        }
    }
    Ok(stubs)
}

fn write_layer(
    root: &mut Element,
    ctx: &mut Ctx,
    archive: &CaffArchive,
    l: &ImageLayer,
    parent: &str,
    image: &str,
) -> Result<String> {
    let img = RgbaImage::decode_png(&l.png)?;
    let res = image_resource(
        root,
        ctx,
        archive,
        (*l.png).clone(),
        (img.width, img.height),
    )?;
    let opts = add_shared(
        root,
        ctx,
        Element::new("hash_map")
            .with_attr("count", "0")
            .with_attr("keyType", "string"),
    )?;
    let entry = layer_entry(
        root,
        ctx,
        &l.common,
        Some(parent),
        image,
        stub("hash_map", Some("_optionOfIOption"), &opts),
    )?;
    let icon16 = icon(archive, &mut ctx.tex, "icon16", &img, 16)?;
    let icon64 = icon(archive, &mut ctx.tex, "icon64", &img, 64)?;
    let b = l.bounds;
    let e = with(
        Element::new("CLayer"),
        vec![
            with(el("ACImageLayer", "super"), vec![entry]),
            stub("CImageResource", Some("imageResource"), &res),
            with(
                el("CRect", "boundsOnImageDoc"),
                vec![
                    scalar("i", "x", b[0].to_string()),
                    scalar("i", "y", b[1].to_string()),
                    scalar("i", "width", b[2].to_string()),
                    scalar("i", "height", b[3].to_string()),
                ],
            ),
            identifier(&l.common),
            icon16,
            icon64,
            with(
                el("linked_map", "layerInfo")
                    .with_attr("count", "0")
                    .with_attr("keyType", "string"),
                vec![],
            ),
            stub("hash_map", Some("_optionOfIOption"), &opts),
        ],
    );
    let id = add_shared(root, ctx, e)?;
    ctx.tex
        .layers
        .insert(l.common.guid.clone(), (id.clone(), res));
    Ok(id)
}

/// Field `name` of the texture manager (inline in the model source).
fn manager_list(root: &mut Element, name: &str) -> Result<Vec<u32>> {
    let main = match root.children.iter().position(|c| c.name == "main") {
        Some(i) => vec![i as u32, 0],
        None => return invalid("document has no <main>"),
    };
    let tm = field_path(root, &main, "textureManager")?;
    if element_at(root, Some(&tm))?.attr("xs.ref").is_some() {
        return unsupported("a shared texture manager");
    }
    field_path(root, &tm, name)
}

fn push_to(root: &mut Element, path: &[u32], e: Element) -> Result<()> {
    let l = element_at(root, Some(path))?;
    l.children.push(e);
    set_count(l);
    Ok(())
}

/// Writes the texture-manager objects of `new` that `old` doesn't have.
/// Existing atlas pages that changed are rewritten; changing or removing
/// other existing objects is not supported yet.
pub(super) fn sync_textures(
    root: &mut Element,
    ctx: &mut Ctx,
    archive: &CaffArchive,
    old: &Model,
    new: &Model,
) -> Result<()> {
    let prefix = |a: usize, b: usize, what: &str| -> Result<()> {
        if b < a {
            return unsupported(format!("removing {what}"));
        }
        Ok(())
    };
    prefix(
        old.layered_images.len(),
        new.layered_images.len(),
        "layered images",
    )?;
    prefix(
        old.model_image_groups.len(),
        new.model_image_groups.len(),
        "model image groups",
    )?;
    prefix(
        old.model_images.len(),
        new.model_images.len(),
        "model images",
    )?;
    prefix(old.textures.len(), new.textures.len(), "texture atlases")?;
    let same = |a: bool, what: &str| -> Result<()> {
        if !a {
            return unsupported(format!("changing {what}"));
        }
        Ok(())
    };
    same(
        new.layered_images[..old.layered_images.len()] == old.layered_images[..],
        "layered images",
    )?;
    same(
        new.model_image_groups[..old.model_image_groups.len()] == old.model_image_groups[..],
        "model image groups",
    )?;
    same(
        new.model_images[..old.model_images.len()] == old.model_images[..],
        "model images",
    )?;
    // Existing atlas pages that changed are rewritten in place.
    for t in 0..old.textures.len() {
        let (o, n) = (&old.textures[t], &new.textures[t]);
        if o.guid != n.guid {
            return unsupported("reordering texture atlases");
        }
        if o != n {
            rewrite_atlas(root, ctx, archive, new, t)?;
        }
    }
    if new.layered_images.len() == old.layered_images.len()
        && new.model_images.len() == old.model_images.len()
        && new.textures.len() == old.textures.len()
    {
        return Ok(());
    }
    ctx.structural = true;
    ctx.tex.imports.extend(TEXTURE_IMPORTS);
    // Layered images.
    let raw = manager_list(root, "_rawImages")?;
    for li in &new.layered_images[old.layered_images.len()..] {
        let Some(tree) = &li.tree else {
            return invalid("a new layered image without its layers");
        };
        let gid = guid_id(root, ctx, &li.guid, "CLayeredImageGuid", "(no debug info)")?;
        let image = add_shared(root, ctx, Element::new("CLayeredImage"))?;
        let rootc = LayerCommon {
            guid: keyform_model::new_guid(),
            name: "root".into(),
            visible: true,
            clipping: false,
            opacity: 255,
            blend: *b"norm",
            layer_id: None,
        };
        let opts = with(
            el("hash_map", "_optionOfIOption")
                .with_attr("count", "0")
                .with_attr("keyType", "string"),
            vec![],
        );
        let entry = layer_entry(root, ctx, &rootc, None, &image, opts)?;
        let rid = add_shared(root, ctx, Element::new("CLayerGroup"))?;
        let mut flat = vec![stub("CLayerGroup", None, &rid)];
        let kids = write_entries(root, ctx, archive, tree, &rid, &image, &mut flat)?;
        *find_shared(root, &rid)? = with(
            find_shared(root, &rid)?.clone(),
            vec![
                with(
                    el("ACLayerGroup", "super"),
                    vec![entry, list("carray_list", "_children", kids)],
                ),
                null("layerIdentifier"),
            ],
        );
        let path_el = {
            let mut f = el("file", "psdFile");
            f.text = xml_text(&li.path);
            f
        };
        let body = vec![
            scalar("s", "name", xml_text(&li.name)),
            scalar("s", "memo", ""),
            scalar("i", "width", li.width.to_string()),
            scalar("i", "height", li.height.to_string()),
            path_el,
            scalar("s", "description", ""),
            stub("CLayeredImageGuid", Some("guid"), &gid),
            null("psdBytes"),
            long("psdFileLastModified", li.modified_ms),
            stub("CLayerGroup", Some("_rootLayer"), &rid),
            with(
                el("LayerSet", "layerSet"),
                vec![
                    stub("CLayeredImage", Some("_layeredImage"), &image),
                    list("carray_list", "_layerEntryList", flat),
                ],
            ),
        ];
        find_shared(root, &image)?.children = body;
        let wrapper = with(
            Element::new("LayeredImageWrapper"),
            vec![
                stub("CLayeredImage", Some("image"), &image),
                long("importedTimeMSec", li.imported_ms),
                long("lastModifiedTimeMSec", li.modified_ms),
                scalar("b", "isReplaced", li.replaced.to_string()),
            ],
        );
        push_to(root, &raw, wrapper)?;
    }
    // Model image groups (new ones), then the new model images inline in
    // their group's list.
    let groups_path = manager_list(root, "_modelImageGroups")?;
    let mut group_ids: HashMap<usize, String> = HashMap::new();
    for (k, g) in new
        .model_image_groups
        .iter()
        .enumerate()
        .skip(old.model_image_groups.len())
    {
        let mut linked = Vec::new();
        for l in &g.linked {
            linked.push(stub(
                "CLayeredImageGuid",
                None,
                &guid_id(root, ctx, l, "CLayeredImageGuid", "(no debug info)")?,
            ));
        }
        let e = with(
            Element::new("CModelImageGroup"),
            vec![
                scalar("s", "memo", ""),
                scalar("s", "groupName", xml_text(&g.name)),
                list("carray_list", "_linkedRawImageGuids", linked),
                list("carray_list", "_modelImages", Vec::new()),
            ],
        );
        let id = add_shared(root, ctx, e)?;
        push_to(root, &groups_path, stub("CModelImageGroup", None, &id))?;
        group_ids.insert(k, id);
    }
    // Existing groups that get new model images (groups have no GUID; the
    // model keeps the list order).
    for k in old.model_images.len()..new.model_images.len() {
        let Some(g) = new.model_images[k].group else {
            continue;
        };
        if g >= old.model_image_groups.len() || group_ids.contains_key(&g) {
            continue;
        }
        let mut p = groups_path.clone();
        p.push(g as u32);
        if element_at(root, Some(&p)).is_err() {
            return invalid(format!("model image group {g}: element not found"));
        }
        let id = super::grid::share_in_place(root, ctx, &p)?;
        group_ids.insert(g, id);
    }
    for k in old.model_images.len()..new.model_images.len() {
        write_model_image(root, ctx, archive, new, k, &group_ids)?;
    }
    // Atlas pages.
    let atlases = manager_list(root, "_textureAtlases")?;
    for t in old.textures.len()..new.textures.len() {
        let (aid, gid) = write_atlas(root, ctx, archive, new, t)?;
        push_to(root, &atlases, stub("CTextureAtlas", None, &aid))?;
        ctx.tex.atlases.insert(t, (aid, gid));
    }
    Ok(())
}

/// A shared `FilterValueId`/`FilterValue`/`StaticFilterDefGuid` found by
/// its identity, or made.
fn shared_by(
    root: &mut Element,
    ctx: &mut Ctx,
    tag: &str,
    matches: &dyn Fn(&Element) -> bool,
    make: Element,
) -> Result<String> {
    if let Some(id) = shared(root).and_then(|s| {
        s.children
            .iter()
            .find(|c| c.name == tag && matches(c))
            .and_then(|c| c.attr("xs.id"))
            .map(str::to_owned)
    }) {
        return Ok(id);
    }
    add_shared(root, ctx, make)
}

fn value_id(root: &mut Element, ctx: &mut Ctx, name: &str) -> Result<String> {
    shared_by(
        root,
        ctx,
        "FilterValueId",
        &|c| c.attr("idstr") == Some(name),
        Element::new("FilterValueId").with_attr("idstr", name),
    )
}

/// The shared `FilterValue` for value id `vid` (label `label`), whose `id`
/// field refers to `vid` (or carries `inline` as its own id).
fn filter_value(
    root: &mut Element,
    ctx: &mut Ctx,
    vid: Option<&str>,
    inline: &str,
    label: &str,
) -> Result<String> {
    let target = vid.map(str::to_owned);
    let inline_s = inline.to_owned();
    let id_field = match vid {
        Some(v) => stub("FilterValueId", Some("id"), v),
        None => el("FilterValueId", "id").with_attr("idstr", inline),
    };
    shared_by(
        root,
        ctx,
        "FilterValue",
        &move |c| {
            c.child_field("id").is_some_and(|f| match &target {
                Some(t) => f.attr("xs.ref") == Some(t.as_str()),
                None => f.attr("idstr") == Some(inline_s.as_str()),
            })
        },
        with(
            Element::new("FilterValue"),
            vec![
                scalar("s", "name", xml_text(label)),
                id_field,
                null("defaultValueInitializer"),
            ],
        ),
    )
}

fn filter_def(root: &mut Element, ctx: &mut Ctx, uuid: &str) -> Result<String> {
    shared_by(
        root,
        ctx,
        "StaticFilterDefGuid",
        &|c| c.attr("uuid") == Some(uuid),
        Element::new("StaticFilterDefGuid")
            .with_attr("uuid", uuid)
            .with_attr("note", "(no debug info)"),
    )
}

/// The model image's input filter graph: a layer selector feeding an
/// image-layer filter, wired to the environment values.
/// spec: psd-import.md §5.1
fn filter_set(root: &mut Element, ctx: &mut Ctx) -> Result<String> {
    let v_in_map = value_id(root, ctx, "mi_input_layerInputData")?;
    let v_sel_map = value_id(root, ctx, "ilf_inputLayerData")?;
    let v_cur = value_id(root, ctx, "mi_currentImageGuid")?;
    let v_sel_cur = value_id(root, ctx, "ilf_currentImageGuid")?;
    let v_sel_out = value_id(root, ctx, "ilf_outputLayerData")?;
    let v_layer_in = value_id(root, ctx, "ilf_inputLayer")?;
    let v_out_img = value_id(root, ctx, "mi_output_image")?;
    let v_out_tr = value_id(root, ctx, "mi_output_transform")?;
    let fv_sel_out = filter_value(root, ctx, Some(&v_sel_out), "", "Selected layer")?;
    let fv_env_in = filter_value(root, ctx, Some(&v_in_map), "", "Input layers")?;
    let fv_sel_in = filter_value(root, ctx, Some(&v_sel_map), "", "Input layer candidates")?;
    let fv_env_cur = filter_value(root, ctx, Some(&v_cur), "", "Current GUID")?;
    let fv_sel_cur = filter_value(
        root,
        ctx,
        Some(&v_sel_cur),
        "",
        "GUID of the selected source image",
    )?;
    let fv_env_img = filter_value(root, ctx, Some(&v_out_img), "", "Output image")?;
    let fv_img_res = filter_value(
        root,
        ctx,
        None,
        "ilf_outputImageRes",
        "Output image (resource)",
    )?;
    let fv_env_tr = filter_value(root, ctx, Some(&v_out_tr), "", "LayerToCanvas transform")?;
    let fv_tr = filter_value(
        root,
        ctx,
        None,
        "ilf_outputTransform",
        "LayerToCanvas transform",
    )?;
    let def_sel = filter_def(root, ctx, "5e9fe1ea-0ec3-4d68-a5fa-018fc7abe301")?;
    let def_layer = filter_def(root, ctx, "4083cd1f-40ba-4eda-8400-379019d55ed8")?;
    // Per set: the set itself, two instances, their ids and a connector.
    let set = add_shared(root, ctx, Element::new("ModelImageFilterSet"))?;
    let id0 = add_shared(
        root,
        ctx,
        Element::new("FilterInstanceId").with_attr("idstr", "filter0"),
    )?;
    let id1 = add_shared(
        root,
        ctx,
        Element::new("FilterInstanceId").with_attr("idstr", "filter1"),
    )?;
    let sel = add_shared(root, ctx, Element::new("FilterInstance"))?;
    let lay = add_shared(root, ctx, Element::new("FilterInstance"))?;
    let conn = add_shared(
        root,
        ctx,
        with(
            Element::new("FilterOutputValueConnector"),
            vec![
                el("AValueConnector", "super"),
                stub("FilterInstance", Some("instance"), &sel),
                stub("FilterValueId", Some("id"), &v_sel_out),
                stub("FilterValue", Some("valueDef"), &fv_sel_out),
            ],
        ),
    )?;
    let env_in = |key: &str, env: &str| {
        with(
            Element::new("entry"),
            vec![
                stub("FilterValueId", Some("key"), key),
                with(
                    el("EnvValueConnector", "value"),
                    vec![
                        el("AValueConnector", "super"),
                        stub("FilterValueId", Some("envValueId"), env),
                    ],
                ),
            ],
        )
    };
    *find_shared(root, &sel)? = with(
        find_shared(root, &sel)?
            .clone()
            .with_attr("filterName", "CLayerSelector"),
        vec![
            stub("StaticFilterDefGuid", Some("filterDefGuid"), &def_sel),
            null("filterDef"),
            stub("FilterInstanceId", Some("filterId"), &id0),
            list(
                "hash_map",
                "inputConnectors",
                vec![env_in(&v_sel_map, &v_in_map), env_in(&v_sel_cur, &v_cur)],
            ),
            list(
                "hash_map",
                "outputConnectors",
                vec![with(
                    Element::new("entry"),
                    vec![
                        stub("FilterValueId", Some("key"), &v_sel_out),
                        stub("FilterOutputValueConnector", Some("value"), &conn),
                    ],
                )],
            ),
            stub("ModelImageFilterSet", Some("ownerFilterSet"), &set),
        ],
    );
    let mut empty_out = list("hash_map", "outputConnectors", Vec::new());
    empty_out.set_attr("keyType", "string");
    *find_shared(root, &lay)? = with(
        find_shared(root, &lay)?
            .clone()
            .with_attr("filterName", "CLayerFilter"),
        vec![
            stub("StaticFilterDefGuid", Some("filterDefGuid"), &def_layer),
            null("filterDef"),
            stub("FilterInstanceId", Some("filterId"), &id1),
            list(
                "hash_map",
                "inputConnectors",
                vec![with(
                    Element::new("entry"),
                    vec![
                        stub("FilterValueId", Some("key"), &v_layer_in),
                        stub("FilterOutputValueConnector", Some("value"), &conn),
                    ],
                )],
            ),
            empty_out,
            stub("ModelImageFilterSet", Some("ownerFilterSet"), &set),
        ],
    );
    let connection = |key: &str, env: &str, inst: &str, def: &str| {
        with(
            Element::new("entry"),
            vec![
                stub("FilterValueId", Some("key"), key),
                with(
                    el("EnvConnection", "value"),
                    vec![
                        stub("FilterValue", Some("_envValueDef"), env),
                        stub("FilterInstance", Some("filter"), inst),
                        stub("FilterValue", Some("filterValueDef"), def),
                    ],
                ),
            ],
        )
    };
    let map_entry = |key: &str, inst: &str| {
        with(
            Element::new("entry"),
            vec![
                stub("FilterInstanceId", Some("key"), key),
                stub("FilterInstance", Some("value"), inst),
            ],
        )
    };
    find_shared(root, &set)?.children = vec![with(
        el("FilterSet", "super"),
        vec![
            list(
                "linked_map",
                "filterMap",
                vec![map_entry(&id0, &sel), map_entry(&id1, &lay)],
            ),
            list(
                "linked_map",
                "_externalInputs",
                vec![
                    connection(&v_in_map, &fv_env_in, &sel, &fv_sel_in),
                    connection(&v_cur, &fv_env_cur, &sel, &fv_sel_cur),
                ],
            ),
            list(
                "linked_map",
                "_externalOutputs",
                vec![
                    connection(&v_out_img, &fv_env_img, &lay, &fv_img_res),
                    connection(&v_out_tr, &fv_env_tr, &lay, &fv_tr),
                ],
            ),
        ],
    )];
    Ok(set)
}

/// The 64-padded cache transform of a w × h raw image.
fn cache_transform(w: u32, h: u32) -> [f32; 6] {
    let pad = |v: u32| v.div_ceil(64).max(1) * 64;
    [
        w as f32 / pad(w) as f32,
        0.0,
        0.0,
        0.0,
        h as f32 / pad(h) as f32,
        0.0,
    ]
}

fn cache_manager(res: &str, (w, h): (u32, u32), t: [f32; 6]) -> Element {
    with(
        el("CCachedImageManager", "cachedImageManager"),
        vec![
            el("CachedImageType", "defaultCacheType").with_attr("v", "SCALE_1"),
            stub("CImageResource", Some("rawImage"), res),
            list(
                "array_list",
                "cachedImages",
                vec![with(
                    Element::new("CCachedImage"),
                    vec![
                        stub("CImageResource", Some("_cachedImageResource"), res),
                        scalar("b", "isSharedImage", "true"),
                        el("CSize", "rawImageSize")
                            .with_attr("width", w.to_string())
                            .with_attr("height", h.to_string()),
                        scalar("i", "reductionRatio", "1"),
                        scalar("i", "mipmapLevel", "64"),
                        scalar("b", "hasMargin", "false"),
                        scalar("b", "isCleaned", "false"),
                        affine_el("transformRawImageToCachedImage", t),
                    ],
                )],
            ),
            scalar("i", "requiredMipmapLevel", "64"),
        ],
    )
}

/// Model image `k` of `new`, inline in its group's list.
fn write_model_image(
    root: &mut Element,
    ctx: &mut Ctx,
    archive: &CaffArchive,
    new: &Model,
    k: usize,
    group_ids: &HashMap<usize, String>,
) -> Result<()> {
    let mi = &new.model_images[k];
    let Some(g) = mi.group else {
        return unsupported(format!("model image {}: no group", mi.name));
    };
    let Some(gid) = group_ids.get(&g).cloned() else {
        return unsupported(format!(
            "model image {}: adding to an existing group",
            mi.name
        ));
    };
    let Some((psd, layer)) = &mi.source else {
        return unsupported(format!("model image {}: not from a PSD layer", mi.name));
    };
    let Some((layer_id, res)) = ctx
        .tex
        .layers
        .get(layer)
        .cloned()
        .or_else(|| existing_layer(root, layer))
    else {
        return unsupported(format!("model image {}: its layer was not found", mi.name));
    };
    let png = mi.png.clone().unwrap_or_default();
    let img = RgbaImage::decode_png(&png)?;
    let size = (img.width, img.height);
    let mguid = guid_id(root, ctx, &mi.guid, "CModelImageGuid", "(no debug info)")?;
    let lguid = guid_id(root, ctx, psd, "CLayeredImageGuid", "(no debug info)")?;
    let set = filter_set(root, ctx)?;
    let v_cur = value_id(root, ctx, "mi_currentImageGuid")?;
    let v_in = value_id(root, ctx, "mi_input_layerInputData")?;
    let time = long("updateTimeMs", 0);
    let env = with(
        el("ModelImageFilterEnv", "inputFilterEnv"),
        vec![with(
            el("FilterEnv", "super"),
            vec![
                null("parentEnv"),
                list(
                    "hash_map",
                    "envValues",
                    vec![
                        with(
                            Element::new("entry"),
                            vec![
                                stub("FilterValueId", Some("key"), &v_cur),
                                with(
                                    el("EnvValueSet", "value"),
                                    vec![
                                        stub("FilterValueId", Some("id"), &v_cur),
                                        stub("CLayeredImageGuid", Some("value"), &lguid),
                                        time.clone(),
                                    ],
                                ),
                            ],
                        ),
                        with(
                            Element::new("entry"),
                            vec![
                                stub("FilterValueId", Some("key"), &v_in),
                                with(
                                    el("EnvValueSet", "value"),
                                    vec![
                                        stub("FilterValueId", Some("id"), &v_in),
                                        with(
                                            el("CLayerSelectorMap", "value"),
                                            vec![list(
                                                "linked_map",
                                                "_imageToLayerInput",
                                                vec![with(
                                                    Element::new("entry"),
                                                    vec![
                                                        stub(
                                                            "CLayeredImageGuid",
                                                            Some("key"),
                                                            &lguid,
                                                        ),
                                                        list(
                                                            "array_list",
                                                            "value",
                                                            vec![with(
                                                                Element::new("CLayerInputData"),
                                                                vec![
                                                                    stub(
                                                                        "CLayer",
                                                                        Some("layer"),
                                                                        &layer_id,
                                                                    ),
                                                                    affine_el(
                                                                        "affine",
                                                                        [
                                                                            1.0, 0.0, 0.0, 0.0,
                                                                            1.0, 0.0,
                                                                        ],
                                                                    ),
                                                                    null("clippingOnTexturePx"),
                                                                ],
                                                            )],
                                                        ),
                                                    ],
                                                )],
                                            )],
                                        ),
                                        time,
                                    ],
                                ),
                            ],
                        ),
                    ],
                ),
            ],
        )],
    );
    let icon16 = icon(archive, &mut ctx.tex, "icon16", &img, 16)?;
    let version = shared(root)
        .and_then(|s| {
            s.children.iter().find_map(|c| {
                c.children
                    .iter()
                    .find(|x| x.field_name() == Some("_modelImages"))
                    .and_then(|l| l.children.last())
                    .and_then(|x| x.attr("modelImageVersion").map(str::to_owned))
            })
        })
        .unwrap_or_else(|| "0".into());
    let e = with(
        Element::new("CModelImage").with_attr("modelImageVersion", version),
        vec![
            stub("CModelImageGuid", Some("guid"), &mguid),
            scalar("s", "name", xml_text(&mi.name)),
            stub("ModelImageFilterSet", Some("inputFilter"), &set),
            env,
            stub("CImageResource", Some("_filteredImage"), &res),
            icon16,
            affine_el("_materialLocalToCanvasTransform", mi.local_to_canvas),
            stub("CModelImageGroup", Some("_group"), &gid),
            list(
                "carray_list",
                "linkedRawImageGuids",
                vec![stub("CLayeredImageGuid", None, &lguid)],
            ),
            cache_manager(&res, size, cache_transform(size.0, size.1)),
            scalar("s", "memo", ""),
        ],
    );
    let group = find_shared(root, &gid)?;
    let Some(l) = group
        .children
        .iter_mut()
        .find(|c| c.field_name() == Some("_modelImages"))
    else {
        return invalid("model image group without its list");
    };
    l.children.push(e);
    set_count(l);
    Ok(())
}

/// Atlas page `t` of `new` and its `GTexture2D`. Returns their ids.
fn write_atlas(
    root: &mut Element,
    ctx: &mut Ctx,
    archive: &CaffArchive,
    new: &Model,
    t: usize,
) -> Result<(String, String)> {
    let tex = &new.textures[t];
    let Some(png) = tex.png.clone() else {
        return unsupported(format!("texture {}: no page image", tex.name));
    };
    let res = image_resource(root, ctx, archive, png, (tex.width, tex.height))?;
    let guid = guid_id(root, ctx, &tex.guid, "CTextureAtlasGuid", "(no debug info)")?;
    let atlas = add_shared(root, ctx, Element::new("CTextureAtlas"))?;
    let children = atlas_children(root, ctx, new, t, &atlas, &res, &guid)?;
    find_shared(root, &atlas)?.children = children;
    // The GPU texture the art meshes refer to.
    let g = add_shared(root, ctx, Element::new("GTexture2D"))?;
    let s = tex.sampling;
    find_shared(root, &g)?.children = vec![
        with(
            el("GTexture", "super"),
            vec![
                scalar("s", "name", xml_text(&tex.name)),
                el("WrapMode", "wrapMode").with_attr("v", "CLAMP_TO_BORDER"),
                with(
                    el("FilterMode", "filterMode"),
                    vec![
                        el("MinFilter", "minFilter").with_attr(
                            "v",
                            if s.mipmaps {
                                "LINEAR_MIPMAP_LINEAR"
                            } else {
                                "LINEAR"
                            },
                        ),
                        el("MagFilter", "magFilter").with_attr("v", "LINEAR"),
                        stub("GTexture2D", Some("owner"), &g),
                    ],
                ),
                el("GTextureGuid", "guid")
                    .with_attr("uuid", keyform_model::new_guid())
                    .with_attr("note", "(no debug info)"),
                el("Anisotropy", "anisotropy")
                    .with_attr("v", if s.anisotropy { "ON" } else { "OFF" }),
            ],
        ),
        stub("CImageResource", Some("srcImageResource"), &res),
        affine_el(
            "transformImageResource01toLogical01",
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        ),
        scalar("i", "mipmapLevel", s.pad_to.max(1).to_string()),
        scalar("b", "isPremultiplied", "true"),
    ];
    Ok((atlas, g))
}

/// Adds this save's new PNG entries to `archive`.
/// Archive entries of dropped image resources that `root` no longer
/// refers to are removed.
pub(super) fn add_entries(archive: &mut CaffArchive, tex: &mut TexState, root: &Element) {
    if !tex.dropped.is_empty() {
        let mut used = BTreeSet::new();
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            if e.name == "file" {
                if let Some(p) = e.attr("path") {
                    used.insert(p.to_owned());
                }
            }
            stack.extend(e.children.iter());
        }
        let dropped: BTreeSet<String> = tex
            .dropped
            .drain(..)
            .filter(|p| !used.contains(p))
            .collect();
        archive.entries.retain(|e| !dropped.contains(&e.path));
    }
    for (path, data) in tex.entries.drain(..) {
        archive.entries.push(CaffEntry {
            path,
            tag: String::new(),
            obfuscated: true,
            compression: Compression::Raw,
            data,
        });
    }
}

/// The `GTexture2D` of an art mesh in model-image texture mode, on the
/// model image's resource (`name` is the mesh's). The resource → logical
/// transform maps the image into its 64-padded cache. Returns its id.
pub(super) fn model_image_texture(
    root: &mut Element,
    ctx: &mut Ctx,
    name: &str,
    res: &str,
    (w, h): (u32, u32),
) -> Result<String> {
    let g = add_shared(root, ctx, Element::new("GTexture2D"))?;
    find_shared(root, &g)?.children = vec![
        with(
            el("GTexture", "super"),
            vec![
                scalar("s", "name", xml_text(name)),
                el("WrapMode", "wrapMode").with_attr("v", "CLAMP_TO_BORDER"),
                with(
                    el("FilterMode", "filterMode"),
                    vec![
                        stub("GTexture2D", Some("owner"), &g),
                        el("MinFilter", "minFilter").with_attr("v", "LINEAR_MIPMAP_LINEAR"),
                        el("MagFilter", "magFilter").with_attr("v", "LINEAR"),
                    ],
                ),
                el("GTextureGuid", "guid")
                    .with_attr("uuid", keyform_model::new_guid())
                    .with_attr("note", "(no debug info)"),
                el("Anisotropy", "anisotropy").with_attr("v", "ON"),
            ],
        ),
        stub("CImageResource", Some("srcImageResource"), res),
        affine_el("transformImageResource01toLogical01", cache_transform(w, h)),
        scalar("i", "mipmapLevel", "64"),
        scalar("b", "isPremultiplied", "true"),
    ];
    Ok(g)
}

/// The fields of atlas page `t` (element `atlas`, page image `res`, GUID
/// object `guid`). spec: 70-advanced/texture-atlas.md §1
fn atlas_children(
    root: &mut Element,
    ctx: &mut Ctx,
    new: &Model,
    t: usize,
    atlas: &str,
    res: &str,
    guid: &str,
) -> Result<Vec<Element>> {
    let tex = &new.textures[t];
    let mut entries = Vec::new();
    for e in &tex.entries {
        let mi = &new.model_images[e.model_image];
        let mg = guid_id(root, ctx, &mi.guid, "CModelImageGuid", "(no debug info)")?;
        // The legacy field: model-image local → atlas as position, scale
        // and angle. spec: texture-atlas.md §1
        let to_atlas = crate::atlas::inverse(e.atlas_to_canvas)
            .map_or(crate::atlas::IDENTITY, |inv| {
                crate::atlas::mul(inv, mi.local_to_canvas)
            });
        let (sx, sy) = (
            to_atlas[0].hypot(to_atlas[3]),
            to_atlas[1].hypot(to_atlas[4]),
        );
        let angle = to_atlas[3].atan2(to_atlas[0]).to_degrees();
        let v2 = |name: &str, x: f32, y: f32| {
            with(
                el("GVector2", name),
                vec![
                    scalar("f", "x", format_f32(x)),
                    scalar("f", "y", format_f32(y)),
                ],
            )
        };
        entries.push(with(
            Element::new("ModelImageEntry"),
            vec![
                stub("CTextureAtlas", Some("atlas"), atlas),
                stub("CModelImageGuid", Some("modelImageGuid"), &mg),
                affine_el("atlasLocalToCanvasTransform", e.atlas_to_canvas),
                with(
                    el("GTransform2", "materialLocalToAtlasTransform"),
                    vec![
                        v2("position", to_atlas[2], to_atlas[5]),
                        v2("scale", sx, sy),
                        scalar("f", "eulerAngle", format_f32(angle)),
                    ],
                ),
            ],
        ));
    }
    Ok(vec![
        scalar("s", "name", xml_text(&tex.name)),
        scalar("i", "width", tex.width.to_string()),
        scalar("i", "height", tex.height.to_string()),
        stub("CImageResource", Some("cachedAtlasImage"), res),
        scalar("b", "lockCachedAtlasImage", tex.locked.to_string()),
        stub("CTextureAtlasGuid", Some("guid"), guid),
        list("carray_list", "modelImages", entries),
        cache_manager(res, (tex.width, tex.height), [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
    ])
}

/// Rewrites existing atlas page `t` of `new` (name, size, lock, entries,
/// page image). The page gets a new image resource; its `GTexture2D` is
/// pointed at it and the old resource is dropped.
fn rewrite_atlas(
    root: &mut Element,
    ctx: &mut Ctx,
    archive: &CaffArchive,
    new: &Model,
    t: usize,
) -> Result<()> {
    let tex = &new.textures[t];
    let Some(png) = tex.png.clone() else {
        return unsupported(format!("texture {}: no page image", tex.name));
    };
    let list = manager_list(root, "_textureAtlases")?;
    let ids: Vec<String> = element_at(root, Some(&list))?
        .children
        .iter()
        .filter_map(|c| c.attr("xs.ref").map(str::to_owned))
        .collect();
    let mut found = None;
    for id in ids {
        let Some(e) = get_shared(root, &id) else {
            continue;
        };
        let Some(g) = e.child_field("guid") else {
            continue;
        };
        if uuid_of(root, g).as_deref() == Some(tex.guid.as_str()) {
            let gid = g.attr("xs.ref").map(str::to_owned);
            let res = e
                .child_field("cachedAtlasImage")
                .and_then(|c| c.attr("xs.ref"))
                .map(str::to_owned);
            found = Some((id, gid, res));
            break;
        }
    }
    let Some((atlas, Some(guid), Some(old_res))) = found else {
        return invalid(format!("texture {}: atlas element not found", tex.name));
    };
    ctx.structural = true;
    let res = image_resource(root, ctx, archive, png, (tex.width, tex.height))?;
    let children = atlas_children(root, ctx, new, t, &atlas, &res, &guid)?;
    find_shared(root, &atlas)?.children = children;
    // The GPU textures made from the old page image.
    let sh = shared_mut(root)?;
    for g in sh.children.iter_mut().filter(|c| c.name == "GTexture2D") {
        let Some(src) = g.child_field_mut("srcImageResource") else {
            continue;
        };
        if src.attr("xs.ref") != Some(old_res.as_str()) {
            continue;
        }
        src.set_attr("xs.ref", res.clone());
        if let Some(n) = g
            .child_field_mut("super")
            .and_then(|s| s.child_field_mut("name"))
        {
            n.text = xml_text(&tex.name);
        }
    }
    // Dropped at the end of the save (removing it now would move the
    // elements other writers have paths to); its archive entry with it.
    if let Some(gone) = get_shared(root, &old_res) {
        let mut stack = vec![gone];
        while let Some(e) = stack.pop() {
            if e.name == "file" {
                if let Some(p) = e.attr("path") {
                    ctx.tex.dropped.push(p.to_owned());
                }
            }
            stack.extend(e.children.iter());
        }
    }
    ctx.orphans.push(old_res);
    Ok(())
}

/// The filtered-image resource of the model image with GUID `guid` that is
/// already in the document (`CModelImage._filteredImage`).
pub(super) fn existing_model_image_res(root: &Element, guid: &str) -> Option<String> {
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if e.name == "CModelImage" {
            let g = e.child_field("guid").and_then(|g| uuid_of(root, g));
            if g.as_deref() == Some(guid) {
                return e
                    .child_field("_filteredImage")
                    .and_then(|r| r.attr("xs.ref"))
                    .map(str::to_owned);
            }
        }
        stack.extend(e.children.iter());
    }
    None
}

/// The shared `CLayer` with GUID `guid` already in the document, and its
/// image resource.
fn existing_layer(root: &Element, guid: &str) -> Option<(String, String)> {
    let sh = shared(root)?;
    for c in sh.children.iter().filter(|c| c.name == "CLayer") {
        let entry = c.child_field("super").and_then(|s| s.child_field("super"));
        let g = entry
            .and_then(|e| e.child_field("guid"))
            .and_then(|g| uuid_of(root, g));
        if g.as_deref() != Some(guid) {
            continue;
        }
        let id = c.attr("xs.id")?.to_owned();
        let res = c.child_field("imageResource")?.attr("xs.ref")?.to_owned();
        return Some((id, res));
    }
    None
}
