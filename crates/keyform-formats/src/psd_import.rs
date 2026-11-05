//! PSD import into a model: the PSD registered as a layered image, one
//! model image per pixel layer, art meshes and parts built from the layer
//! tree, and a texture atlas page for the new images.
//! spec: 70-advanced/psd-import.md §3–6, 70-advanced/texture-atlas.md §5–6

use std::collections::HashSet;
use std::sync::Arc;

use keyform_model::*;

use crate::atlas::{self, Affine, LayoutItem, LayoutSettings};
use crate::error::{FormatError, Result};
use crate::image::RgbaImage;
use crate::psd::{Psd, PsdLayer, PsdNode};

/// Import settings.
#[derive(Clone, Debug)]
pub struct ImportOptions {
    /// Mesh margin around each layer rectangle (PSD Import Settings,
    /// default 1 px). spec: psd-import.md §5.3
    pub margin: f32,
    /// "(Blend mode before 5.3)": the legacy blend table.
    pub legacy_blend: bool,
    /// The PSD's file path and modification time, and the import time
    /// (ms since the epoch).
    pub path: String,
    pub modified_ms: i64,
    pub imported_ms: i64,
    /// New model only: the preview reduction ratio chosen (§3.1).
    pub preview_reduction: u32,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            margin: 1.0,
            legacy_blend: false,
            path: String::new(),
            modified_ms: 0,
            imported_ms: 0,
            preview_reduction: 1,
        }
    }
}

/// One choice of the preview resolution prompt.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewChoice {
    /// 1, 2, 4 or 8.
    pub factor: u32,
    /// `"<label> ( W x H px / MMB )"` (no space before MB, observed), plus
    /// `" < Recommended"`.
    pub label: String,
}

/// The preview resolution prompt (§3.1): the choices, the preselected
/// one, and whether to ask at all (not when Full Scale qualifies).
pub fn preview_choices(psd: &Psd) -> (Vec<PreviewChoice>, usize, bool) {
    const LIMIT: f64 = 209_715_200.0;
    let total: f64 = psd
        .layers()
        .iter()
        .map(|l| f64::from(l.bounds.width) * f64::from(l.bounds.height) * 4.0)
        .sum();
    let names = ["Full Scale", "1/2 preview", "1/4 preview", "1/8 preview"];
    let factors = [1u32, 2, 4, 8];
    let fits = |f: u32| total / f64::from(f * f) < LIMIT;
    let pre = factors.iter().position(|&f| fits(f)).unwrap_or(3);
    let choices = names
        .iter()
        .zip(factors)
        .enumerate()
        .map(|(k, (name, f))| {
            let mb = total / f64::from(f * f) / (1024.0 * 1024.0);
            let mb = if mb <= 10.0 {
                format!("{mb:.2}")
            } else {
                format!("{}", mb as u64)
            };
            PreviewChoice {
                factor: f,
                label: format!(
                    "{name} ( {} x {} px / {mb}MB ){}",
                    psd.width / f,
                    psd.height / f,
                    if k == pre { " < Recommended" } else { "" }
                ),
            }
        })
        .collect();
    (choices, pre, pre != 0)
}

/// What an import created.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    pub layered_image: usize,
    pub art_meshes: Vec<usize>,
    pub parts: Vec<usize>,
    /// The atlas page made for the new model images.
    pub atlas: Option<usize>,
    /// Layers whose blend mode has no equivalent (imported as Normal).
    pub warnings: Vec<String>,
}

fn translate(x: f32, y: f32) -> Affine {
    [1.0, 0.0, x, 0.0, 1.0, y]
}

/// The layer tree as stored in the model, each layer's pixels encoded as
/// PNG (decoded one layer at a time).
fn layer_tree(psd: &Psd, nodes: &[PsdNode]) -> Result<Vec<LayerEntry>> {
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        out.push(match n {
            PsdNode::Group(g) => LayerEntry::Group(LayerGroup {
                common: LayerCommon {
                    guid: new_guid(),
                    name: g.name.clone(),
                    visible: g.visible,
                    clipping: false,
                    opacity: g.opacity,
                    blend: g.blend,
                    layer_id: g.id.clone(),
                },
                children: layer_tree(psd, &g.children)?,
            }),
            PsdNode::Layer(l) => LayerEntry::Layer(image_layer(psd, l)?),
        });
    }
    Ok(out)
}

fn image_layer(psd: &Psd, l: &PsdLayer) -> Result<ImageLayer> {
    let png = psd.pixels(l)?.encode_png()?;
    Ok(ImageLayer {
        common: LayerCommon {
            guid: new_guid(),
            name: l.name.clone(),
            visible: l.visible,
            clipping: l.clipping,
            opacity: l.opacity,
            blend: l.blend,
            layer_id: l.id.clone(),
        },
        bounds: [
            l.bounds.left,
            l.bounds.top,
            l.bounds.width as i32,
            l.bounds.height as i32,
        ],
        png: Arc::new(png),
    })
}

/// §6.2 Register PSD file without adding art meshes. Returns the layered
/// image's index.
pub fn register_psd(m: &mut Model, name: &str, psd: &Psd, o: &ImportOptions) -> Result<usize> {
    let tree = layer_tree(psd, &psd.nodes)?;
    let i = m.layered_images.len();
    m.layered_images.push(Arc::new(LayeredImage {
        guid: new_guid(),
        name: name.to_owned(),
        width: psd.width,
        height: psd.height,
        path: o.path.clone(),
        imported_ms: o.imported_ms,
        modified_ms: o.modified_ms,
        replaced: false,
        tree: Some(Arc::new(tree)),
    }));
    Ok(i)
}

/// A valid object ID: `[0-9A-Za-z_]`, not starting with a digit, shorter
/// than 64. spec: psd-import.md §5.5
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 64
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Increments a decimal string, keeping its width (`09` → `10`,
/// `99` → `100`).
fn increment(digits: &str) -> String {
    let mut d: Vec<u8> = digits.bytes().collect();
    let mut i = d.len();
    loop {
        if i == 0 {
            d.insert(0, b'1');
            break;
        }
        i -= 1;
        if d[i] == b'9' {
            d[i] = b'0';
        } else {
            d[i] += 1;
            break;
        }
    }
    String::from_utf8(d).unwrap_or_default()
}

/// ID assignment (psd-import.md §5.5): the candidate if valid and free;
/// else base + counter from its trailing digits (or 2); else the fallback
/// base, then fallback + 0, 1, 2, …
pub fn assign_id(candidate: &str, fallback: &str, taken: &mut HashSet<String>) -> String {
    let id = (|| {
        if valid_id(candidate) && !taken.contains(candidate) {
            return candidate.to_owned();
        }
        let base = candidate.trim_end_matches(|c: char| c.is_ascii_digit());
        let digits = &candidate[base.len()..];
        let mut counter = if digits.is_empty() {
            "2".to_owned()
        } else {
            digits.to_owned()
        };
        while taken.contains(&format!("{base}{counter}")) {
            counter = increment(&counter);
        }
        let id = format!("{base}{counter}");
        if valid_id(&id) {
            return id;
        }
        if !taken.contains(fallback) {
            return fallback.to_owned();
        }
        (0u64..)
            .map(|k| format!("{fallback}{k}"))
            .find(|c| !taken.contains(c))
            .unwrap_or_default()
    })();
    taken.insert(id.clone());
    id
}

/// PSD blend → colour composition (psd-import.md §5.4); `None` = no
/// equivalent (Normal plus a warning).
pub fn blend_composition(key: &[u8; 4], tsl: bool, legacy: bool) -> Option<ColorComposition> {
    use ColorComposition as C;
    if legacy {
        return match key {
            b"norm" => Some(C::Normal),
            b"lddg" => Some(C::Add),
            b"mul " => Some(C::Multiply),
            _ => None,
        };
    }
    Some(match key {
        b"norm" => C::Normal,
        b"lddg" if tsl => C::AddR2Tsl,
        b"lddg" => C::AddR2,
        b"mul " => C::MultiplyR2,
        b"dark" => C::Darken,
        b"idiv" => C::ColorBurnTsl,
        b"lbrn" => C::LinearBurnTsl,
        b"lite" => C::Lighten,
        b"scrn" => C::Screen,
        b"div " => C::ColorDodgeTsl,
        b"over" => C::Overlay,
        b"sLit" => C::SoftLight,
        b"hLit" => C::HardLight,
        b"lLit" => C::LinearLightTsl,
        b"hue " => C::HslHue,
        b"colr" => C::HslColor,
        _ => return None,
    })
}

/// The import's art mesh mesh: the layer rectangle pushed out by the
/// margin, points TR, TL, BR, BL, outline edges, two triangles.
/// spec: psd-import.md §5.3
fn rect_mesh(b: [i32; 4], m: f32) -> (Vec<f32>, Vec<u32>, EditableMesh) {
    let (x0, y0) = (b[0] as f32 - m, b[1] as f32 - m);
    let (x1, y1) = ((b[0] + b[2]) as f32 + m, (b[1] + b[3]) as f32 + m);
    let positions = vec![x1, y0, x0, y0, x1, y1, x0, y1];
    // As the reference writes it: the diagonal TR–BL as an auto edge,
    // triangles (0, 2, 3) and (0, 3, 1) (positive area, y down).
    let indices = vec![0, 2, 3, 0, 3, 1];
    let mesh = EditableMesh {
        point_priority: vec![POINT_NORMAL; 4],
        edges: vec![[0, 1], [0, 2], [1, 3], [2, 3], [0, 3]],
        edge_priority: vec![
            EDGE_NORMAL,
            EDGE_NORMAL,
            EDGE_NORMAL,
            EDGE_NORMAL,
            EDGE_AUTO_TRIANGULATION,
        ],
        next_point_uid: 4,
        ..EditableMesh::default()
    };
    (positions, indices, mesh)
}

struct Builder<'a> {
    m: &'a mut Model,
    o: &'a ImportOptions,
    legacy: bool,
    /// Model image index per pixel layer, in tree order.
    images: std::vec::IntoIter<usize>,
    report: ImportReport,
    mesh_ids: HashSet<String>,
    part_ids: HashSet<String>,
}

impl Builder<'_> {
    /// Objects for `entries` under part `parent`; returns whether any art
    /// mesh was made. spec: psd-import.md §5.2
    fn build(&mut self, entries: &[LayerEntry], parent: usize) -> bool {
        let mut any = false;
        for e in entries {
            match e {
                LayerEntry::Layer(l) => {
                    let Some(mi) = self.images.next() else {
                        continue;
                    };
                    let a = self.art_mesh(l, mi, parent);
                    let p = Arc::make_mut(&mut self.m.parts[parent]);
                    p.children.push(PartChild::ArtMesh(a));
                    any = true;
                }
                LayerEntry::Group(g) => {
                    let id = assign_id(&g.common.name, "Part", &mut self.part_ids);
                    let mut part = Part::new(&id);
                    part.info.guid = new_guid();
                    part.info.name = g.common.name.clone();
                    part.info.visible = g.common.visible && g.common.opacity > 0;
                    part.info.parent_part = Some(parent);
                    part.grid.guids = vec![new_guid()];
                    let k = self.m.parts.len();
                    self.m.parts.push(Arc::new(part));
                    let has = self.build(&g.children, k);
                    if g.children.is_empty() || has {
                        Arc::make_mut(&mut self.m.parts[parent])
                            .children
                            .push(PartChild::Part(k));
                        self.report.parts.push(k);
                        any |= has;
                    } else {
                        // Children but no art mesh below: dropped (it is
                        // the last part, and nothing refers to it).
                        self.m.parts.pop();
                    }
                }
            }
        }
        any
    }

    fn art_mesh(&mut self, l: &ImageLayer, mi: usize, part: usize) -> usize {
        let name = &l.common.name;
        let candidate = if name.is_ascii() {
            name.as_str()
        } else {
            "ArtMesh"
        };
        let id = assign_id(candidate, "ArtMesh", &mut self.mesh_ids);
        let (positions, indices, mesh) = rect_mesh(l.bounds, self.o.margin);
        let tsl = true;
        let color = match blend_composition(&l.common.blend, tsl, self.legacy) {
            Some(c) => c,
            None => {
                self.report.warnings.push(format!(
                    "{name}: blend mode {:?} is not supported; Normal is used.",
                    String::from_utf8_lossy(&l.common.blend)
                ));
                ColorComposition::Normal
            }
        };
        let mut info = ObjectInfo::new(&id);
        info.guid = new_guid();
        info.name = name.clone();
        info.visible = l.common.visible;
        info.parent_part = Some(part);
        let form = ArtMeshForm {
            positions: positions.clone(),
            draw_order: 500.0,
            opacity: f32::from(l.common.opacity) / 255.0,
            ..ArtMeshForm::default()
        };
        let mut grid = KeyformGrid::single(form);
        grid.guids = vec![new_guid()];
        // Model-image texture mode: UVs relative to the image padded to a
        // multiple of 64 (as the editor's cache), y down.
        let (pw, ph) = (
            (l.bounds[2].max(1) as u32).div_ceil(64) as f32 * 64.0,
            (l.bounds[3].max(1) as u32).div_ceil(64) as f32 * 64.0,
        );
        let uvs: Vec<f32> = positions
            .chunks_exact(2)
            .flat_map(|p| {
                [
                    (p[0] - l.bounds[0] as f32) / pw,
                    (p[1] - l.bounds[1] as f32) / ph,
                ]
            })
            .collect();
        let a = ArtMesh {
            info,
            target: None,
            uvs,
            positions,
            indices,
            texture: None,
            clip: Vec::new(),
            invert_clip: false,
            clip_missing: false,
            culling: false,
            color_composition: color,
            alpha_composition: AlphaComposition::Over,
            user_data: String::new(),
            point_uids: vec![0, 1, 2, 3],
            mesh,
            model_image: Some(mi),
            generator: MeshGenerateSetting::default(),
            grid,
            blend_shapes: BlendShapes::default(),
        };
        let k = self.m.art_meshes.len();
        self.m.art_meshes.push(Arc::new(a));
        self.report.art_meshes.push(k);
        k
    }
}

fn layers_in_order<'t>(entries: &'t [LayerEntry], out: &mut Vec<&'t ImageLayer>) {
    for e in entries {
        match e {
            LayerEntry::Layer(l) => out.push(l),
            LayerEntry::Group(g) => layers_in_order(&g.children, out),
        }
    }
}

/// §6.1 Add all layers as new art meshes: registers the PSD, adds model
/// images (to the group linked to it, or a new one), builds the objects in
/// a new part at the top of the root part, and packs the new images onto a
/// new atlas page.
pub fn add_psd_layers(
    m: &mut Model,
    name: &str,
    psd: &Psd,
    o: &ImportOptions,
) -> Result<ImportReport> {
    import_layers(m, name, psd, o, true)
}

/// Objects into a new part at the top of the root part (`new_part`, §6.1)
/// or directly under the root part (§4).
fn import_layers(
    m: &mut Model,
    name: &str,
    psd: &Psd,
    o: &ImportOptions,
    new_part: bool,
) -> Result<ImportReport> {
    let li = register_psd(m, name, psd, o)?;
    let psd_guid = m.layered_images[li].guid.clone();
    let tree = m.layered_images[li].tree.clone().unwrap_or_default();
    // Model image group.
    let group = match m
        .model_image_groups
        .iter()
        .position(|g| g.linked.contains(&psd_guid))
    {
        Some(g) => g,
        None => {
            m.model_image_groups.push(Arc::new(ModelImageGroup {
                name: name.to_owned(),
                linked: vec![psd_guid.clone()],
            }));
            m.model_image_groups.len() - 1
        }
    };
    // One model image per pixel layer. spec: psd-import.md §5.1
    let mut layers = Vec::new();
    layers_in_order(&tree, &mut layers);
    let mut images = Vec::with_capacity(layers.len());
    for l in &layers {
        images.push(m.model_images.len());
        m.model_images.push(Arc::new(ModelImage {
            guid: new_guid(),
            name: l.common.name.clone(),
            png: Some((*l.png).clone()),
            local_to_canvas: translate(l.bounds[0] as f32, l.bounds[1] as f32),
            group: Some(group),
            source: Some((psd_guid.clone(), l.common.guid.clone())),
        }));
    }
    // A new part at index 0 of the root part holds everything.
    let mut part_ids: HashSet<String> = m.parts.iter().map(|p| p.info.id.clone()).collect();
    let mesh_ids: HashSet<String> = m.art_meshes.iter().map(|a| a.info.id.clone()).collect();
    let root = m.root_part;
    let top = if new_part || root.is_none() {
        let pid = assign_id(name, "Part", &mut part_ids);
        let mut part = Part::new(&pid);
        part.info.guid = new_guid();
        part.info.name = name.to_owned();
        part.info.parent_part = root;
        part.grid.guids = vec![new_guid()];
        let top = m.parts.len();
        m.parts.push(Arc::new(part));
        if let Some(r) = root {
            Arc::make_mut(&mut m.parts[r])
                .children
                .insert(0, PartChild::Part(top));
        }
        top
    } else {
        root.unwrap_or(0)
    };
    let legacy = o.legacy_blend || !(m.target_version >= 5_030_000);
    let mut b = Builder {
        m,
        o,
        legacy,
        images: images.clone().into_iter(),
        report: ImportReport {
            layered_image: li,
            parts: if new_part { vec![top] } else { Vec::new() },
            ..Default::default()
        },
        mesh_ids,
        part_ids,
    };
    b.build(&tree, top);
    // The meshes draw from their model images (texture state MODEL_IMAGE);
    // adding a PSD doesn't make or re-pack an atlas. spec: §6.1
    let _ = images;
    Ok(b.report)
}

/// The image layer with GUID `guid` in a layer tree.
fn find_layer<'t>(entries: &'t [LayerEntry], guid: &str) -> Option<&'t ImageLayer> {
    for e in entries {
        match e {
            LayerEntry::Layer(l) if l.common.guid == guid => return Some(l),
            LayerEntry::Layer(_) => {}
            LayerEntry::Group(g) => {
                if let Some(l) = find_layer(&g.children, guid) {
                    return Some(l);
                }
            }
        }
    }
    None
}

/// Project palette ▸ layer ▸ Create ArtMesh: an art mesh for layer
/// `layer_guid` of layered image `li`, drawing from the layer's model
/// image (made if the layer has none), as PSD import makes it (§5.3–5.5),
/// at the top of part `part` (the root part if `None`). Returns the art
/// mesh index. spec: 50-ui/palettes.md §6, psd-import.md §5
pub fn create_art_mesh_from_layer(
    m: &mut Model,
    li: usize,
    layer_guid: &str,
    part: Option<usize>,
    o: &ImportOptions,
) -> Result<usize> {
    let Some(img) = m.layered_images.get(li) else {
        return Err(FormatError::Invalid("no such source image".into()));
    };
    let psd_guid = img.guid.clone();
    let Some(l) = img
        .tree
        .as_ref()
        .and_then(|t| find_layer(t, layer_guid))
        .cloned()
    else {
        return Err(FormatError::Invalid("no such layer".into()));
    };
    let Some(part) = part.or(m.root_part).filter(|&p| p < m.parts.len()) else {
        return Err(FormatError::Invalid("the model has no part".into()));
    };
    let source = Some((psd_guid.clone(), layer_guid.to_owned()));
    let mi = match m.model_images.iter().position(|x| x.source == source) {
        Some(k) => k,
        None => {
            let group = match m
                .model_image_groups
                .iter()
                .position(|g| g.linked.contains(&psd_guid))
            {
                Some(g) => g,
                None => {
                    let name = m.layered_images[li].name.clone();
                    m.model_image_groups.push(Arc::new(ModelImageGroup {
                        name,
                        linked: vec![psd_guid.clone()],
                    }));
                    m.model_image_groups.len() - 1
                }
            };
            let group = Some(group);
            m.model_images.push(Arc::new(ModelImage {
                guid: new_guid(),
                name: l.common.name.clone(),
                png: Some((*l.png).clone()),
                local_to_canvas: translate(l.bounds[0] as f32, l.bounds[1] as f32),
                group,
                source,
            }));
            m.model_images.len() - 1
        }
    };
    let part_ids: HashSet<String> = m.parts.iter().map(|p| p.info.id.clone()).collect();
    let mesh_ids: HashSet<String> = m.art_meshes.iter().map(|a| a.info.id.clone()).collect();
    let legacy = o.legacy_blend || !(m.target_version >= 5_030_000);
    let mut b = Builder {
        m,
        o,
        legacy,
        images: vec![mi].into_iter(),
        report: ImportReport {
            layered_image: li,
            ..Default::default()
        },
        mesh_ids,
        part_ids,
    };
    let a = b.art_mesh(&l, mi, part);
    Arc::make_mut(&mut m.parts[part])
        .children
        .insert(0, PartChild::ArtMesh(a));
    Ok(a)
}

/// The standard parameters of a new model, in order: ID, name, min, max,
/// default, description. spec: psd-import.md §4.1
pub const STANDARD_PARAMETERS: [(&str, &str, f32, f32, f32, &str); 27] = [
    (
        "ParamAngleX",
        "Angle X",
        -30.0,
        30.0,
        0.0,
        "+ causes it to face right side of screen",
    ),
    (
        "ParamAngleY",
        "Angle Y",
        -30.0,
        30.0,
        0.0,
        "+ causes it to face upper side of screen",
    ),
    (
        "ParamAngleZ",
        "Angle Z",
        -30.0,
        30.0,
        0.0,
        "+ causes it to face right side of screen",
    ),
    (
        "ParamEyeLOpen",
        "EyeL Open",
        0.0,
        1.0,
        1.0,
        "+ causes eye to open",
    ),
    (
        "ParamEyeLSmile",
        "EyeL Smile",
        0.0,
        1.0,
        0.0,
        "+ causes smiley eye",
    ),
    (
        "ParamEyeROpen",
        "EyeR Open",
        0.0,
        1.0,
        1.0,
        "+ causes eye to open",
    ),
    (
        "ParamEyeRSmile",
        "EyeR Smile",
        0.0,
        1.0,
        0.0,
        "+ causes smiley eye",
    ),
    (
        "ParamEyeBallX",
        "Eyeball X",
        -1.0,
        1.0,
        0.0,
        "+ causes it to look right",
    ),
    (
        "ParamEyeBallY",
        "Eyeball Y",
        -1.0,
        1.0,
        0.0,
        "+ causes it to look up",
    ),
    ("ParamBrowLY", "BrowL Y", -1.0, 1.0, 0.0, "+ raises eyebrow"),
    ("ParamBrowRY", "BrowR Y", -1.0, 1.0, 0.0, "+ raises eyebrow"),
    (
        "ParamBrowLX",
        "BrowL X",
        -1.0,
        1.0,
        0.0,
        "- moves eyebrow to create frown",
    ),
    (
        "ParamBrowRX",
        "BrowR X",
        -1.0,
        1.0,
        0.0,
        "- moves eyebrow to create frown",
    ),
    (
        "ParamBrowLAngle",
        "BrowL Angle",
        -1.0,
        1.0,
        0.0,
        "- moves/deforms eyebrow to create scowl",
    ),
    (
        "ParamBrowRAngle",
        "BrowR Angle",
        -1.0,
        1.0,
        0.0,
        "- moves/deforms eyebrow to create scowl",
    ),
    (
        "ParamBrowLForm",
        "BrowL Form",
        -1.0,
        1.0,
        0.0,
        "- moves/deforms eyebrow to create scowl",
    ),
    (
        "ParamBrowRForm",
        "BrowR Form",
        -1.0,
        1.0,
        0.0,
        "- moves/deforms eyebrow to create scowl",
    ),
    (
        "ParamMouthForm",
        "Mouth Form",
        -1.0,
        1.0,
        0.0,
        "+ causes smiling mouth, - causes scowling mouth",
    ),
    (
        "ParamMouthOpenY",
        "Mouth Open",
        0.0,
        1.0,
        0.0,
        "+ causes mouth to open",
    ),
    ("ParamCheek", "Cheek", 0.0, 1.0, 0.0, "+ causes blush"),
    (
        "ParamBodyAngleX",
        "Body X",
        -10.0,
        10.0,
        0.0,
        "+ causes it to face right side of screen",
    ),
    (
        "ParamBodyAngleY",
        "Body Y",
        -10.0,
        10.0,
        0.0,
        "+ causes moving it up on the screen",
    ),
    (
        "ParamBodyAngleZ",
        "Body Z",
        -10.0,
        10.0,
        0.0,
        "+ causes tilting it to the right of the screen",
    ),
    (
        "ParamBreath",
        "Breath",
        0.0,
        1.0,
        0.0,
        "+ causes taking a breath",
    ),
    (
        "ParamHairFront",
        "Hair Move Front",
        -1.0,
        1.0,
        0.0,
        "+ causes it to move to the right side of the screen",
    ),
    (
        "ParamHairSide",
        "Hair Move Side",
        -1.0,
        1.0,
        0.0,
        "+ causes it to move to the right side of the screen",
    ),
    (
        "ParamHairBack",
        "Hair Move Back",
        -1.0,
        1.0,
        0.0,
        "+ causes it to move to the right side of the screen",
    ),
];

/// §4 New model from PSD: a model named after the file (without its
/// extension) with the PSD's canvas, the standard parameters, and every
/// layer imported under the root part.
pub fn new_model_from_psd(
    file_name: &str,
    psd: &Psd,
    o: &ImportOptions,
) -> Result<(crate::cmo3::Cmo3Document, ImportReport)> {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
    let mut doc = crate::cmo3::Cmo3Document::new_model(stem, psd.width, psd.height)?;
    let m = &mut doc.model;
    m.preview_reduction = o.preview_reduction.max(1);
    add_standard_parameters(m);
    let report = import_layers(m, file_name, psd, o, false)?;
    Ok((doc, report))
}

/// The standard parameters (§4.1) at the top of the Parameter palette.
fn add_standard_parameters(m: &mut Model) {
    // Names and descriptions are localized UI strings (§4.1).
    for (id, _, min, max, def, _) in STANDARD_PARAMETERS {
        let mut p = Parameter::new(id, min, max, def);
        p.guid = new_guid();
        p.name = crate::strings::text(&format!("param.{id}")).to_owned();
        p.description = crate::strings::text(&format!("param.{id}.desc")).to_owned();
        let i = m.parameters.len();
        m.root_parameter_entries.push(ParamEntry::Parameter(i));
        m.parameters.push(Arc::new(p));
    }
}

/// The parts of a blank new model under the root part, in order: ID,
/// draw order (= default order), sketch. Their names are the localized
/// UI strings `part.<ID>`. spec: 50-ui/dialogs.md §1.1
pub const BLANK_MODEL_PARTS: [(&str, i32, bool); 15] = [
    ("PartRough", 500, false),
    ("PartFace", 500, false),
    ("PartEye", 600, false),
    ("PartEyeBall", 600, false),
    ("PartBrow", 600, false),
    ("PartMouth", 600, false),
    ("PartNose", 600, false),
    ("PartEar", 500, false),
    ("PartHairFront", 700, false),
    ("PartHairSide", 500, false),
    ("PartHairBack", 300, false),
    ("PartNeck", 400, false),
    ("PartBody", 400, false),
    ("PartBackground", 0, false),
    ("PartSketch", 1000, true),
];

/// File ▸ New ▸ Model (Home ▸ New ▸ "Create with specified size"): a
/// model of `width` × `height` with the standard parameters and the blank
/// model's parts. spec: 50-ui/dialogs.md §1.1, psd-import.md §4.1
pub fn new_blank_model(name: &str, width: u32, height: u32) -> Result<crate::cmo3::Cmo3Document> {
    let mut doc = crate::cmo3::Cmo3Document::new_model(name, width, height)?;
    let m = &mut doc.model;
    add_standard_parameters(m);
    let root = m.root_part;
    for (id, order, sketch) in BLANK_MODEL_PARTS {
        let mut part = Part::new(id);
        part.info.guid = new_guid();
        part.info.name = crate::strings::text(&format!("part.{id}")).to_owned();
        part.info.parent_part = root;
        part.sketch = sketch;
        part.default_order = order;
        part.grid = KeyformGrid::single(keyform_model::PartForm {
            draw_order: order as f32,
            ..Default::default()
        });
        part.grid.guids = vec![new_guid()];
        let k = m.parts.len();
        m.parts.push(Arc::new(part));
        if let Some(r) = root {
            Arc::make_mut(&mut m.parts[r])
                .children
                .push(PartChild::Part(k));
        }
    }
    Ok(doc)
}

/// Packs `images` onto a new atlas page (named `Texture<N>`, the size of
/// the last page or 1024²) with Auto Layout of the meshes' bounds, renders
/// it, and points `meshes` at it with their UVs.
/// spec: texture-atlas.md §1, §4–6
pub fn pack_new_atlas(m: &mut Model, images: &[usize], meshes: &[usize]) -> Result<Option<usize>> {
    if images.is_empty() {
        return Ok(None);
    }
    let (w, h) = m
        .textures
        .last()
        .map_or((1024, 1024), |t| (t.width.max(32), t.height.max(32)));
    let mut n = m.textures.len();
    let name = loop {
        let c = format!("Texture{n}");
        if !m.textures.iter().any(|t| t.name == c) {
            break c;
        }
        n += 1;
    };
    // Packing rectangles: the bounds of the meshes using each image, in
    // image-local pixels (isModelImageLayout off).
    let items: Vec<LayoutItem> = images
        .iter()
        .map(|&k| {
            let mi = &m.model_images[k];
            let size = mi
                .png
                .as_deref()
                .and_then(crate::image::png_size)
                .unwrap_or((1, 1));
            let inv = atlas::inverse(mi.local_to_canvas).unwrap_or(atlas::IDENTITY);
            let mut src = [0.0, 0.0, size.0 as f32, size.1 as f32];
            let pts: Vec<[f32; 2]> = meshes
                .iter()
                .filter(|&&a| m.art_meshes[a].model_image == Some(k))
                .flat_map(|&a| {
                    m.art_meshes[a]
                        .positions
                        .chunks_exact(2)
                        .map(|p| atlas::apply(inv, [p[0], p[1]]))
                        .collect::<Vec<_>>()
                })
                .collect();
            if !pts.is_empty() {
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for p in &pts {
                    (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
                }
                src = [x0, y0, x1 - x0, y1 - y0];
            }
            LayoutItem {
                size,
                src,
                scale: 1.0,
                locked: false,
            }
        })
        .collect();
    let placed = atlas::auto_layout(&items, w, h, &LayoutSettings::default());
    let mut entries = Vec::new();
    let mut draw = Vec::new();
    for (&k, t) in images.iter().zip(&placed) {
        let Some(to_atlas) = *t else { continue };
        let Some(inv) = atlas::inverse(to_atlas) else {
            continue;
        };
        let mi = &m.model_images[k];
        entries.push(AtlasEntry {
            model_image: k,
            atlas_to_canvas: atlas::mul(mi.local_to_canvas, inv),
        });
        draw.push((k, to_atlas));
    }
    // Render, decoding one image at a time.
    let model = &*m;
    let page = atlas::render_page(
        draw.iter().filter_map(|&(k, t)| {
            let img = RgbaImage::decode_png(model.model_images[k].png.as_ref()?).ok()?;
            Some((img, t))
        }),
        w,
        h,
    );
    let ti = m.textures.len();
    m.textures.push(Arc::new(Texture {
        guid: new_guid(),
        name,
        width: w,
        height: h,
        png: Some(page.encode_png()?),
        locked: false,
        // As the GTexture2D written for it loads (mipmapLevel 64).
        sampling: TextureSampling {
            mipmaps: true,
            anisotropy: true,
            pad_to: 64,
        },
        entries: entries.clone(),
    }));
    // Hook up the meshes: atlas texture and UVs (texture-atlas.md §4).
    for &a in meshes {
        let Some(mi) = m.art_meshes[a].model_image else {
            continue;
        };
        let Some(e) = entries.iter().find(|e| e.model_image == mi) else {
            continue;
        };
        let Some(c2a) = atlas::inverse(e.atlas_to_canvas) else {
            continue;
        };
        let x = Arc::make_mut(&mut m.art_meshes[a]);
        x.texture = Some(ti);
        x.uvs = x
            .positions
            .chunks_exact(2)
            .flat_map(|p| {
                let q = atlas::apply(c2a, [p[0], p[1]]);
                [q[0] / w as f32, q[1] / h as f32]
            })
            .collect();
    }
    Ok(Some(ti))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_per_spec() {
        let mut taken = HashSet::new();
        assert_eq!(assign_id("Hair", "ArtMesh", &mut taken), "Hair");
        assert_eq!(assign_id("Hair", "ArtMesh", &mut taken), "Hair2");
        assert_eq!(assign_id("Hair", "ArtMesh", &mut taken), "Hair3");
        assert_eq!(assign_id("Eye09", "ArtMesh", &mut taken), "Eye09");
        assert_eq!(assign_id("Eye09", "ArtMesh", &mut taken), "Eye10");
        // Spaces → fallback base, then fallback + 0.
        assert_eq!(assign_id("Layer 1", "ArtMesh", &mut taken), "ArtMesh");
        assert_eq!(assign_id("Layer 2", "ArtMesh", &mut taken), "ArtMesh0");
        assert_eq!(increment("99"), "100");
        assert!(!valid_id("1abc") && valid_id("a_1"));
    }

    #[test]
    fn blend_tables() {
        assert_eq!(
            blend_composition(b"lddg", true, false),
            Some(ColorComposition::AddR2Tsl)
        );
        assert_eq!(
            blend_composition(b"lddg", false, false),
            Some(ColorComposition::AddR2)
        );
        assert_eq!(
            blend_composition(b"mul ", true, true),
            Some(ColorComposition::Multiply)
        );
        assert_eq!(blend_composition(b"diff", true, false), None);
        assert_eq!(blend_composition(b"scrn", true, true), None);
    }

    #[test]
    fn rect_mesh_is_positive() {
        let (p, ix, m) = rect_mesh([10, 20, 30, 40], 1.0);
        assert_eq!(m.edges[4], [0, 3]);
        assert_eq!(p[..4], [41.0, 19.0, 9.0, 19.0]);
        for t in ix.chunks_exact(3) {
            let q = |i: u32| [p[2 * i as usize], p[2 * i as usize + 1]];
            let (a, b, c) = (q(t[0]), q(t[1]), q(t[2]));
            assert!((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0);
        }
    }
}
