//! Texture atlas operations on a model: creating an atlas from the model
//! images, Auto Layout, rendering pages and pointing art meshes at them.
//! spec: 70-advanced/texture-atlas.md §1–6

use std::sync::Arc;

use keyform_model::{new_guid, AtlasEntry, Model, Texture, TextureSampling};

use crate::atlas::{self, Affine, LayoutItem, LayoutSettings};
use crate::error::{FormatError, Result};
use crate::image::RgbaImage;

/// Which model images "Create texture atlas" inserts. spec: §2
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Insert {
    /// Used by at least one drawable visible in the hierarchy (default).
    Visible,
    All,
    None,
}

/// Page sizes offered for atlases (32 … 16384). spec: §1
pub const PAGE_SIZES: [u32; 10] = [32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384];

/// Whether part `p` and its ancestors are visible / none of them is a
/// sketch part.
fn part_chain(
    m: &Model,
    mut p: Option<usize>,
    test: impl Fn(&keyform_model::Part) -> bool,
) -> bool {
    let mut guard = 0;
    while let Some(k) = p {
        let Some(part) = m.parts.get(k) else { break };
        if !test(part) {
            return false;
        }
        p = part.info.parent_part;
        guard += 1;
        if guard > 1024 {
            break;
        }
    }
    true
}

/// Model images eligible for an atlas (used by a drawable not under a
/// sketch part), or with `visible_only` those used by a drawable visible
/// in the hierarchy. In model-image order. spec: §2
pub fn eligible_images(m: &Model, visible_only: bool) -> Vec<usize> {
    let mut used = vec![false; m.model_images.len()];
    for a in &m.art_meshes {
        let Some(k) = a.model_image else { continue };
        if k >= used.len() || !part_chain(m, a.info.parent_part, |p| !p.sketch) {
            continue;
        }
        if visible_only
            && !(a.info.visible && part_chain(m, a.info.parent_part, |p| p.info.visible))
        {
            continue;
        }
        used[k] = true;
    }
    (0..used.len()).filter(|&k| used[k]).collect()
}

fn image_size(m: &Model, k: usize) -> (u32, u32) {
    m.model_images[k]
        .png
        .as_deref()
        .and_then(crate::image::png_size)
        .unwrap_or((1, 1))
}

/// The first free `Texture<N>` name, N starting at the atlas count.
pub fn default_name(m: &Model) -> String {
    let mut n = m.textures.len();
    loop {
        let c = format!("Texture{n}");
        if !m.textures.iter().any(|t| t.name == c) {
            return c;
        }
        n += 1;
    }
}

/// The default page size: the last atlas's, else 1024 × 1024.
pub fn default_size(m: &Model) -> (u32, u32) {
    m.textures
        .last()
        .map_or((1024, 1024), |t| (t.width.max(32), t.height.max(32)))
}

/// Re-renders page `t` from its entries (unless locked). spec: §6
pub fn render(m: &mut Model, t: usize) -> Result<()> {
    let tex = &m.textures[t];
    if tex.locked {
        return Ok(());
    }
    let (w, h) = (tex.width, tex.height);
    let draw: Vec<(usize, Affine)> = tex
        .entries
        .iter()
        .filter_map(|e| {
            let to_atlas = atlas::inverse(e.atlas_to_canvas)?;
            Some((
                e.model_image,
                atlas::mul(to_atlas, m.model_images[e.model_image].local_to_canvas),
            ))
        })
        .collect();
    let model = &*m;
    let page = atlas::render_page(
        draw.iter().filter_map(|&(k, t)| {
            let img = RgbaImage::decode_png(model.model_images.get(k)?.png.as_ref()?).ok()?;
            Some((img, t))
        }),
        w,
        h,
    );
    Arc::make_mut(&mut m.textures[t]).png = Some(page.encode_png()?);
    Ok(())
}

/// Points every art mesh whose model image is on page `t` at it (the atlas
/// region becomes its current input) and recomputes its UVs. spec: §4
pub fn hook_up(m: &mut Model, t: usize) {
    let (w, h) = (m.textures[t].width as f32, m.textures[t].height as f32);
    let entries = m.textures[t].entries.clone();
    for a in &mut m.art_meshes {
        let Some(k) = a.model_image else { continue };
        let Some(e) = entries.iter().find(|e| e.model_image == k) else {
            continue;
        };
        let Some(c2a) = atlas::inverse(e.atlas_to_canvas) else {
            continue;
        };
        let x = Arc::make_mut(a);
        x.texture = Some(t);
        x.uvs = x
            .positions
            .chunks_exact(2)
            .flat_map(|p| {
                let q = atlas::apply(c2a, [p[0], p[1]]);
                [q[0] / w, q[1] / h]
            })
            .collect();
    }
}

/// Recomputes the UVs of the meshes already on page `t` (after its entries
/// moved), keeping meshes on other pages as they are.
pub fn refresh_uvs(m: &mut Model, t: usize) {
    let on: Vec<bool> = m.art_meshes.iter().map(|a| a.texture == Some(t)).collect();
    let keep: Vec<_> = m
        .art_meshes
        .iter()
        .zip(&on)
        .map(|(a, &o)| (!o).then(|| (a.texture, a.uvs.clone())))
        .collect();
    hook_up(m, t);
    for (a, k) in m.art_meshes.iter_mut().zip(keep) {
        if let Some((tex, uvs)) = k {
            if a.texture != tex {
                let x = Arc::make_mut(a);
                x.texture = tex;
                x.uvs = uvs;
            }
        }
    }
}

/// Create texture atlas: a new page with the chosen model images placed by
/// the grid layout, rendered, and the meshes hooked up. Returns its index.
/// spec: §2–4
pub fn create_atlas(m: &mut Model, name: &str, w: u32, h: u32, insert: Insert) -> Result<usize> {
    if !PAGE_SIZES.contains(&w) || !PAGE_SIZES.contains(&h) {
        return Err(FormatError::Invalid(format!("atlas size {w}×{h}")));
    }
    let images = match insert {
        Insert::Visible => eligible_images(m, true),
        Insert::All => eligible_images(m, false),
        Insert::None => Vec::new(),
    };
    let sizes: Vec<(u32, u32)> = images.iter().map(|&k| image_size(m, k)).collect();
    let placed = atlas::grid_layout(&sizes, w, h);
    let entries = images
        .iter()
        .zip(&placed)
        .filter_map(|(&k, &to_atlas)| {
            let inv = atlas::inverse(to_atlas)?;
            Some(AtlasEntry {
                model_image: k,
                atlas_to_canvas: atlas::mul(m.model_images[k].local_to_canvas, inv),
            })
        })
        .collect();
    let t = m.textures.len();
    m.textures.push(Arc::new(Texture {
        guid: new_guid(),
        name: name.to_owned(),
        width: w,
        height: h,
        png: None,
        locked: false,
        sampling: TextureSampling {
            mipmaps: true,
            anisotropy: true,
            pad_to: 64,
        },
        entries,
    }));
    render(m, t)?;
    hook_up(m, t);
    Ok(t)
}

/// Auto Layout of page `t` (unlocked items; `mesh_bounds` packs only the
/// part of each image its meshes cover). Re-renders and refreshes UVs.
/// Returns how many images didn't fit. spec: §5
pub fn auto_layout(
    m: &mut Model,
    t: usize,
    s: &LayoutSettings,
    mesh_bounds: bool,
) -> Result<usize> {
    let (w, h) = (m.textures[t].width, m.textures[t].height);
    let entries = m.textures[t].entries.clone();
    let items: Vec<LayoutItem> = entries
        .iter()
        .map(|e| {
            let k = e.model_image;
            let size = image_size(m, k);
            let mut src = [0.0, 0.0, size.0 as f32, size.1 as f32];
            if mesh_bounds {
                let inv =
                    atlas::inverse(m.model_images[k].local_to_canvas).unwrap_or(atlas::IDENTITY);
                let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                for a in m.art_meshes.iter().filter(|a| a.model_image == Some(k)) {
                    for p in a.positions.chunks_exact(2) {
                        let q = atlas::apply(inv, [p[0], p[1]]);
                        b = [
                            b[0].min(q[0]),
                            b[1].min(q[1]),
                            b[2].max(q[0]),
                            b[3].max(q[1]),
                        ];
                    }
                }
                if b[0] <= b[2] {
                    src = [b[0], b[1], b[2] - b[0], b[3] - b[1]];
                }
            }
            LayoutItem {
                size,
                src,
                scale: 1.0,
                locked: false,
            }
        })
        .collect();
    let placed = atlas::auto_layout(&items, w, h, s);
    let mut missed = 0;
    let mut new_entries = entries.clone();
    for (e, p) in new_entries.iter_mut().zip(&placed) {
        match p.and_then(atlas::inverse) {
            Some(inv) => {
                e.atlas_to_canvas = atlas::mul(m.model_images[e.model_image].local_to_canvas, inv);
            }
            None => missed += 1,
        }
    }
    Arc::make_mut(&mut m.textures[t]).entries = new_entries;
    render(m, t)?;
    refresh_uvs(m, t);
    Ok(missed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use keyform_model::*;

    fn model_with_images() -> Model {
        let mut m = Model::default();
        let mut root = Part::new("__RootPart__");
        root.info.guid = new_guid();
        m.parts.push(Arc::new(root));
        m.root_part = Some(0);
        for (k, (w, h)) in [(40u32, 20u32), (10, 30)].into_iter().enumerate() {
            let mut img = RgbaImage::new(w, h);
            for p in img.data.chunks_exact_mut(4) {
                p.copy_from_slice(&[200, 10, 10, 255]);
            }
            m.model_images.push(Arc::new(ModelImage {
                guid: new_guid(),
                name: format!("img{k}"),
                png: Some(img.encode_png().unwrap()),
                local_to_canvas: [1.0, 0.0, 100.0 * k as f32, 0.0, 1.0, 50.0],
                group: None,
                source: None,
                other_sources: Vec::new(),
            }));
            let mut info = ObjectInfo::new(&format!("ArtMesh{k}"));
            info.parent_part = Some(0);
            info.visible = k == 0;
            let x0 = 100.0 * k as f32;
            let positions = vec![x0, 50.0, x0 + w as f32, 50.0, x0, 50.0 + h as f32];
            m.art_meshes.push(Arc::new(ArtMesh {
                info,
                target: None,
                uvs: vec![0.0; 6],
                positions: positions.clone(),
                indices: vec![0, 1, 2],
                texture: None,
                clip: vec![],
                invert_clip: false,
                clip_missing: false,
                culling: false,
                color_composition: ColorComposition::Normal,
                alpha_composition: AlphaComposition::Over,
                user_data: String::new(),
                point_uids: vec![0, 1, 2],
                mesh: EditableMesh::default(),
                model_image: Some(k),
                generator: Default::default(),
                grid: KeyformGrid::single(ArtMeshForm {
                    positions,
                    ..Default::default()
                }),
                blend_shapes: Default::default(),
            }));
        }
        m
    }

    #[test]
    fn create_and_layout() {
        let mut m = model_with_images();
        assert_eq!(eligible_images(&m, true), vec![0]);
        assert_eq!(eligible_images(&m, false), vec![0, 1]);
        assert_eq!(default_name(&m), "Texture0");
        let t = create_atlas(&mut m, "Texture0", 256, 256, Insert::All).unwrap();
        assert_eq!(m.textures[t].entries.len(), 2);
        assert!(m.textures[t].png.is_some());
        // Hooked up: UVs map the mesh corner to its image corner on the page.
        let a = &m.art_meshes[0];
        assert_eq!(a.texture, Some(t));
        let e = &m.textures[t].entries[0];
        let c2a = atlas::inverse(e.atlas_to_canvas).unwrap();
        let q = atlas::apply(c2a, [100.0 * 0.0 + 40.0, 50.0]);
        assert!((a.uvs[2] - q[0] / 256.0).abs() < 1e-6);
        // The page has the image's colour inside its placement.
        let page = RgbaImage::decode_png(m.textures[t].png.as_ref().unwrap()).unwrap();
        let c = atlas::apply(c2a, [120.0, 60.0]);
        let j = 4 * (c[1] as usize * 256 + c[0] as usize);
        assert_eq!(&page.data[j..j + 4], &[200, 10, 10, 255]);
        // Auto layout shrinks nothing that fits and keeps the hook-up.
        assert_eq!(
            auto_layout(&mut m, t, &LayoutSettings::default(), true).unwrap(),
            0
        );
        assert_eq!(m.art_meshes[1].texture, Some(t));
        assert!(create_atlas(&mut m, "x", 100, 100, Insert::None).is_err());
    }
}
