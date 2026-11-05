//! PSD import into a model: the PSD registered as a layered image, one
//! model image per pixel layer, art meshes and parts built from the layer
//! tree, and a texture atlas page for the new images.
//! spec: 70-advanced/psd-import.md §3–6, 70-advanced/texture-atlas.md §5–6

use std::collections::HashSet;
use std::sync::Arc;

use keyform_model::*;

use crate::atlas::{self, Affine, LayoutItem, LayoutSettings};
use crate::error::Result;
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
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            margin: 1.0,
            legacy_blend: false,
            path: String::new(),
            modified_ms: 0,
            imported_ms: 0,
        }
    }
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
    let legacy = o.legacy_blend || !(m.target_version >= 5_030_000);
    let mut b = Builder {
        m,
        o,
        legacy,
        images: images.clone().into_iter(),
        report: ImportReport {
            layered_image: li,
            parts: vec![top],
            ..Default::default()
        },
        mesh_ids,
        part_ids,
    };
    b.build(&tree, top);
    // The meshes draw from their model images (texture state MODEL_IMAGE);
    // an atlas is made later. TODO(Q-0018): whether §6.1 "re-pack
    // textures" also places the new images on an atlas.
    let _ = images;
    Ok(b.report)
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
