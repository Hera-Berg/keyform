//! Replace PSD (re-import): swaps the pixels of existing art meshes for
//! the matching layers of a new version of the PSD, keeping meshes,
//! deformers and keyforms. spec: 70-advanced/psd-import.md §7

use std::collections::HashSet;
use std::sync::Arc;

use keyform_model::{
    new_guid, ImageLayer, LayerEntry, Model, ModelImage, ModelImageGroup, Part, PartChild,
};

use crate::error::{FormatError, Result};
use crate::psd::Psd;
use crate::psd_import::{assign_id, register_psd, ImportOptions, ImportReport};

/// What a replace did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplaceReport {
    /// The new layered image.
    pub layered_image: usize,
    /// (new layer GUID, old layer GUID) pairs.
    pub pairs: Vec<(String, String)>,
    /// Model images relinked to the new PSD.
    pub relinked: Vec<usize>,
    /// Objects made from unpaired new layers.
    pub import: ImportReport,
    /// "By ID first" (true) or "by name first" was used.
    pub by_id: bool,
}

/// A pixel layer with its PSD.
struct L<'t> {
    psd: String,
    layer: &'t ImageLayer,
}

fn pixel_layers<'t>(entries: &'t [LayerEntry], psd: &str, out: &mut Vec<L<'t>>) {
    for e in entries {
        match e {
            LayerEntry::Layer(l) => out.push(L {
                psd: psd.to_owned(),
                layer: l,
            }),
            LayerEntry::Group(g) => pixel_layers(&g.children, psd, out),
        }
    }
}

/// Layout score: area(bounds ∩) / area(bounds ∪). spec: §7 step 3
pub fn layout_score(a: [i32; 4], b: [i32; 4]) -> f32 {
    let (ax1, ay1, ax2, ay2) = (a[0], a[1], a[0] + a[2], a[1] + a[3]);
    let (bx1, by1, bx2, by2) = (b[0], b[1], b[0] + b[2], b[1] + b[3]);
    let iw = (ax2.min(bx2) - ax1.max(bx1)).max(0) as f64;
    let ih = (ay2.min(by2) - ay1.max(by1)).max(0) as f64;
    let inter = iw * ih;
    let union = (a[2] as f64) * (a[3] as f64) + (b[2] as f64) * (b[3] as f64) - inter;
    if union <= 0.0 {
        0.0
    } else {
        (inter / union) as f32
    }
}

/// One pairing strategy's result: (new index, old index, score).
type Pairs = Vec<(usize, usize, f32)>;

fn by_name(new: &L, old: &[L], i: usize, out: &mut Pairs) -> bool {
    let same: Vec<usize> = (0..old.len())
        .filter(|&k| old[k].layer.common.name == new.layer.common.name)
        .collect();
    let score = |k: usize| layout_score(new.layer.bounds, old[k].layer.bounds);
    match same.len() {
        0 => false,
        1 => {
            let s = score(same[0]);
            if s > 0.1 {
                out.push((i, same[0], s));
                true
            } else {
                false
            }
        }
        _ => {
            let best = same
                .iter()
                .copied()
                .max_by(|&a, &b| score(a).total_cmp(&score(b)));
            match best {
                Some(k) if score(k) > 0.1 => {
                    out.push((i, k, score(k)));
                    true
                }
                _ => false,
            }
        }
    }
}

fn by_id(new: &L, old: &[L], i: usize, out: &mut Pairs) -> bool {
    let Some(id) = new.layer.common.layer_id.as_deref() else {
        return false;
    };
    let mut any = false;
    for (k, o) in old.iter().enumerate() {
        if o.layer.common.layer_id.as_deref() == Some(id) {
            let s = layout_score(new.layer.bounds, o.layer.bounds);
            if s > 0.1 {
                out.push((i, k, s));
                any = true;
            }
        }
    }
    any
}

fn has_id_match(new: &L, old: &[L]) -> bool {
    new.layer.common.layer_id.as_deref().is_some_and(|id| {
        old.iter()
            .any(|o| o.layer.common.layer_id.as_deref() == Some(id))
    })
}

/// (perfect-fit ratio, matching level) of a strategy.
fn quality(p: &Pairs) -> (f32, f32) {
    if p.is_empty() {
        return (0.0, 0.0);
    }
    let perfect = p.iter().filter(|x| x.2 >= 0.95).count() as f32;
    let level = p.iter().map(|x| x.2).sum::<f32>() / p.len() as f32;
    (perfect / p.len() as f32, level)
}

/// Pairs new layers with old ones by the two strategies and picks one
/// (§7 step 3). Returns the pairs (new index, old index) and whether "by
/// ID first" won.
fn pair_layers(new: &[L], old: &[L]) -> (Vec<(usize, usize)>, bool) {
    let mut a: Pairs = Vec::new();
    let mut b: Pairs = Vec::new();
    for (i, n) in new.iter().enumerate() {
        // By ID first.
        if has_id_match(n, old) {
            by_id(n, old, i, &mut a);
        } else {
            by_name(n, old, i, &mut a);
        }
        // By name first.
        if !by_name(n, old, i, &mut b) {
            by_id(n, old, i, &mut b);
        }
    }
    let (qa, qb) = (quality(&a), quality(&b));
    let use_a = match (qa.0 > 0.5, qb.0 > 0.5) {
        (true, true) => qa.0 >= qb.0,
        (true, false) => true,
        (false, true) => false,
        (false, false) => qa.1 >= qb.1,
    };
    let chosen = if use_a { a } else { b };
    (chosen.into_iter().map(|x| (x.0, x.1)).collect(), use_a)
}

fn translate(x: f32, y: f32) -> [f32; 6] {
    [1.0, 0.0, x, 0.0, 1.0, y]
}

/// The layer tree keeping only the image layers in `keep` (groups that
/// still hold one stay).
fn pruned(entries: &[LayerEntry], keep: &HashSet<String>) -> Vec<LayerEntry> {
    let mut out = Vec::new();
    for e in entries {
        match e {
            LayerEntry::Layer(l) if keep.contains(&l.common.guid) => out.push(e.clone()),
            LayerEntry::Layer(_) => {}
            LayerEntry::Group(g) => {
                let children = pruned(&g.children, keep);
                if !children.is_empty() {
                    let mut g = g.clone();
                    g.children = children;
                    out.push(LayerEntry::Group(g));
                }
            }
        }
    }
    out
}

/// Replace PSD (§7): `old` are the layered images being replaced.
pub fn replace_psd(
    m: &mut Model,
    old: &[usize],
    name: &str,
    psd: &Psd,
    o: &ImportOptions,
) -> Result<ReplaceReport> {
    if old.is_empty() || old.iter().any(|&i| i >= m.layered_images.len()) {
        return Err(FormatError::Invalid("no source image to replace".into()));
    }
    // 1. Register the new PSD; the old ones are marked replaced.
    let li = register_psd(m, name, psd, o)?;
    let new_guid_psd = m.layered_images[li].guid.clone();
    let old_guids: Vec<String> = old
        .iter()
        .map(|&i| m.layered_images[i].guid.clone())
        .collect();
    for &i in old {
        if !m.layered_images[i].replaced {
            Arc::make_mut(&mut m.layered_images[i]).replaced = true;
        }
    }
    // 2. The model-image group: linked to an old PSD, else a new one.
    let group = match m
        .model_image_groups
        .iter()
        .position(|g| g.linked.iter().any(|x| old_guids.contains(x)))
    {
        Some(g) => {
            Arc::make_mut(&mut m.model_image_groups[g])
                .linked
                .push(new_guid_psd.clone());
            g
        }
        None => {
            m.model_image_groups.push(Arc::new(ModelImageGroup {
                name: name.to_owned(),
                linked: vec![new_guid_psd.clone()],
            }));
            m.model_image_groups.len() - 1
        }
    };
    // 3. Pair layers.
    let new_tree = m.layered_images[li].tree.clone().unwrap_or_default();
    let old_trees: Vec<(String, Arc<Vec<LayerEntry>>)> = old
        .iter()
        .map(|&i| {
            let x = &m.layered_images[i];
            (x.guid.clone(), x.tree.clone().unwrap_or_default())
        })
        .collect();
    let mut new_layers = Vec::new();
    pixel_layers(&new_tree, &new_guid_psd, &mut new_layers);
    let mut old_layers = Vec::new();
    for (g, t) in &old_trees {
        pixel_layers(t, g, &mut old_layers);
    }
    let (pairs, by_id_first) = pair_layers(&new_layers, &old_layers);
    let mut report = ReplaceReport {
        layered_image: li,
        by_id: by_id_first,
        pairs: pairs
            .iter()
            .map(|&(n, k)| {
                (
                    new_layers[n].layer.common.guid.clone(),
                    old_layers[k].layer.common.guid.clone(),
                )
            })
            .collect(),
        ..Default::default()
    };
    // 4. Relink model images whose layer map uses an old PSD.
    for k in 0..m.model_images.len() {
        let mi = m.model_images[k].clone();
        let entries: Vec<(String, String)> =
            mi.source.iter().chain(&mi.other_sources).cloned().collect();
        let mapped = entries.iter().find_map(|(p, l)| {
            if !old_guids.contains(p) {
                return None;
            }
            let (n, _) = pairs.iter().find(|&&(_, ok)| {
                old_layers[ok].layer.common.guid == *l && old_layers[ok].psd == *p
            })?;
            Some(*n)
        });
        let Some(n) = mapped else { continue };
        let nl = new_layers[n].layer;
        let x = Arc::make_mut(&mut m.model_images[k]);
        x.other_sources = entries
            .into_iter()
            .filter(|(p, _)| *p != new_guid_psd)
            .collect();
        x.source = Some((new_guid_psd.clone(), nl.common.guid.clone()));
        x.png = Some((*nl.png).clone());
        x.local_to_canvas = translate(nl.bounds[0] as f32, nl.bounds[1] as f32);
        report.relinked.push(k);
    }
    // 5. Unpaired new layers become new objects in their own part.
    let paired: HashSet<usize> = pairs.iter().map(|p| p.0).collect();
    let keep: HashSet<String> = (0..new_layers.len())
        .filter(|i| !paired.contains(i))
        .map(|i| new_layers[i].layer.common.guid.clone())
        .collect();
    if !keep.is_empty() {
        let tree = pruned(&new_tree, &keep);
        report.import = import_unpaired(m, li, name, &tree, group, o)?;
    }
    // 6. Placement changed: UVs of model-image meshes, atlas pages.
    refresh_after_relink(m, &report.relinked)?;
    Ok(report)
}

/// New objects for the unpaired layers, in a part "<name> (Corresponding
/// layer not found)" at index 0 of the root part.
fn import_unpaired(
    m: &mut Model,
    li: usize,
    name: &str,
    tree: &[LayerEntry],
    group: usize,
    o: &ImportOptions,
) -> Result<ImportReport> {
    let psd_guid = m.layered_images[li].guid.clone();
    let root = m
        .root_part
        .ok_or_else(|| FormatError::Invalid("the model has no part".into()))?;
    let mut part_ids: HashSet<String> = m.parts.iter().map(|p| p.info.id.clone()).collect();
    let pname = format!("{name} (Corresponding layer not found)");
    let pid = assign_id(&pname, "Part", &mut part_ids);
    let mut part = Part::new(&pid);
    part.info.guid = new_guid();
    part.info.name = pname;
    part.info.parent_part = Some(root);
    part.grid.guids = vec![new_guid()];
    let top = m.parts.len();
    m.parts.push(Arc::new(part));
    Arc::make_mut(&mut m.parts[root])
        .children
        .insert(0, PartChild::Part(top));
    let mut report = ImportReport {
        layered_image: li,
        parts: vec![top],
        ..Default::default()
    };
    // Each unpaired layer: a model image and an art mesh (§5.2/§5.3).
    #[allow(clippy::too_many_arguments)]
    fn walk(
        m: &mut Model,
        entries: &[LayerEntry],
        parent: usize,
        psd: &str,
        group: usize,
        li: usize,
        o: &ImportOptions,
        report: &mut ImportReport,
    ) -> Result<()> {
        for e in entries {
            match e {
                LayerEntry::Layer(l) => {
                    m.model_images.push(Arc::new(ModelImage {
                        guid: new_guid(),
                        name: l.common.name.clone(),
                        png: Some((*l.png).clone()),
                        local_to_canvas: translate(l.bounds[0] as f32, l.bounds[1] as f32),
                        group: Some(group),
                        source: Some((psd.to_owned(), l.common.guid.clone())),
                        other_sources: Vec::new(),
                    }));
                    let a = crate::psd_import::create_art_mesh_from_layer(
                        m,
                        li,
                        &l.common.guid,
                        Some(parent),
                        o,
                    )?;
                    report.art_meshes.push(a);
                    // create_art_mesh_from_layer inserts at the top; keep
                    // the layer order (append).
                    let p = Arc::make_mut(&mut m.parts[parent]);
                    p.children.retain(|c| *c != PartChild::ArtMesh(a));
                    p.children.push(PartChild::ArtMesh(a));
                }
                LayerEntry::Group(g) => {
                    let mut ids: HashSet<String> =
                        m.parts.iter().map(|p| p.info.id.clone()).collect();
                    let id = assign_id(&g.common.name, "Part", &mut ids);
                    let mut part = Part::new(&id);
                    part.info.guid = new_guid();
                    part.info.name = g.common.name.clone();
                    part.info.visible = g.common.visible && g.common.opacity > 0;
                    part.info.parent_part = Some(parent);
                    part.grid.guids = vec![new_guid()];
                    let k = m.parts.len();
                    m.parts.push(Arc::new(part));
                    Arc::make_mut(&mut m.parts[parent])
                        .children
                        .push(PartChild::Part(k));
                    report.parts.push(k);
                    walk(m, &g.children, k, psd, group, li, o, report)?;
                }
            }
        }
        Ok(())
    }
    walk(m, tree, top, &psd_guid, group, li, o, &mut report)?;
    Ok(report)
}

/// UVs of model-image-mode meshes on relinked images (local / pad64), and
/// re-rendered atlas pages holding them.
fn refresh_after_relink(m: &mut Model, relinked: &[usize]) -> Result<()> {
    if relinked.is_empty() {
        return Ok(());
    }
    for i in 0..m.art_meshes.len() {
        let a = &m.art_meshes[i];
        let (None, Some(k)) = (a.texture, a.model_image) else {
            continue;
        };
        if !relinked.contains(&k) {
            continue;
        }
        let mi = &m.model_images[k];
        let Some((w, h)) = mi.png.as_deref().and_then(crate::image::png_size) else {
            continue;
        };
        let pad = |v: u32| (v.max(1)).div_ceil(64) as f32 * 64.0;
        let (pw, ph) = (pad(w), pad(h));
        let inv = crate::atlas::inverse(mi.local_to_canvas).unwrap_or(crate::atlas::IDENTITY);
        let uvs: Vec<f32> = a
            .positions
            .chunks_exact(2)
            .flat_map(|p| {
                let q = crate::atlas::apply(inv, [p[0], p[1]]);
                [q[0] / pw, q[1] / ph]
            })
            .collect();
        if uvs != a.uvs {
            Arc::make_mut(&mut m.art_meshes[i]).uvs = uvs;
        }
    }
    for t in 0..m.textures.len() {
        let tex = &m.textures[t];
        if tex.locked
            || !tex
                .entries
                .iter()
                .any(|e| relinked.contains(&e.model_image))
        {
            continue;
        }
        crate::atlas_edit::render(m, t)?;
        crate::atlas_edit::refresh_uvs(m, t);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(name: &str, id: Option<&str>, b: [i32; 4]) -> ImageLayer {
        ImageLayer {
            common: keyform_model::LayerCommon {
                guid: new_guid(),
                name: name.into(),
                visible: true,
                clipping: false,
                opacity: 255,
                blend: *b"norm",
                layer_id: id.map(str::to_owned),
            },
            bounds: b,
            png: Arc::new(Vec::new()),
        }
    }

    #[test]
    fn strategies() {
        // Renamed layers keep their IDs: ID first wins (perfect fits).
        let old = [
            layer("eye", Some("1"), [0, 0, 10, 10]),
            layer("mouth", Some("2"), [20, 0, 10, 10]),
        ];
        let new = [
            layer("eye L", Some("1"), [0, 0, 10, 10]),
            layer("mouth 2", Some("2"), [20, 0, 10, 10]),
        ];
        let ol: Vec<L> = old
            .iter()
            .map(|l| L {
                psd: "o".into(),
                layer: l,
            })
            .collect();
        let nl: Vec<L> = new
            .iter()
            .map(|l| L {
                psd: "n".into(),
                layer: l,
            })
            .collect();
        let (p, by_id) = pair_layers(&nl, &ol);
        assert!(by_id);
        assert_eq!(p, vec![(0, 0), (1, 1)]);
        // Re-numbered IDs, same names: name first pairs them.
        let new = [
            layer("eye", Some("7"), [0, 0, 10, 10]),
            layer("mouth", Some("8"), [20, 0, 10, 10]),
        ];
        let nl: Vec<L> = new
            .iter()
            .map(|l| L {
                psd: "n".into(),
                layer: l,
            })
            .collect();
        let (p, _) = pair_layers(&nl, &ol);
        assert_eq!(p, vec![(0, 0), (1, 1)]);
        // Far away: no pair (score ≤ 0.1).
        let new = [layer("eye", Some("1"), [500, 500, 10, 10])];
        let nl: Vec<L> = new
            .iter()
            .map(|l| L {
                psd: "n".into(),
                layer: l,
            })
            .collect();
        assert!(pair_layers(&nl, &ol).0.is_empty());
    }

    #[test]
    fn scores() {
        assert_eq!(layout_score([0, 0, 10, 10], [0, 0, 10, 10]), 1.0);
        assert!((layout_score([0, 0, 10, 10], [5, 0, 10, 10]) - 50.0 / 150.0).abs() < 1e-6);
        assert_eq!(layout_score([0, 0, 10, 10], [20, 20, 5, 5]), 0.0);
    }
}
