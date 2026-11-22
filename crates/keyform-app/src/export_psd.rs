//! File ▸ Export Image/Video ▸ PSD images (beta) [Ctrl+P].
//! spec: 70-advanced/psd-export.md

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use keyform_eval::{Evaluator, Frame};
use keyform_formats::psd_write::{write_psd, PsdLayer, PsdNode};
use keyform_model::{ColorComposition, LayerEntry, Model};
use keyform_render::{CpuRenderer, RenderOptions, View};

use crate::document::{Document, Mode};

// ------------------------------------------------------------ source PSDs

/// "Export All Source Images as PSD" (§2): every stored source image with
/// its layer tree and pixels, as (file name, bytes). Nothing is rendered.
pub fn source_psds(m: &Model) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for li in &m.layered_images {
        let Some(tree) = &li.tree else { continue };
        let nodes = tree_nodes(tree);
        let merged = flatten(&nodes, li.width, li.height);
        let name = li.name.strip_suffix(".psd").unwrap_or(&li.name);
        let name = if name.is_empty() { "source" } else { name };
        out.push((
            format!("{name}.psd"),
            write_psd(li.width, li.height, &nodes, &merged),
        ));
    }
    out
}

fn tree_nodes(entries: &[LayerEntry]) -> Vec<PsdNode> {
    entries
        .iter()
        .map(|e| match e {
            LayerEntry::Group(g) => PsdNode::Group {
                name: g.common.name.clone(),
                visible: g.common.visible,
                children: tree_nodes(&g.children),
            },
            LayerEntry::Layer(l) => {
                let img = keyform_formats::image::RgbaImage::decode_png(&l.png).ok();
                let (w, h, px) = img.map_or((0, 0, Vec::new()), |i| (i.width, i.height, i.data));
                PsdNode::Layer(PsdLayer {
                    name: l.common.name.clone(),
                    left: l.bounds[0],
                    top: l.bounds[1],
                    width: w,
                    height: h,
                    rgba: px,
                    opacity: l.common.opacity,
                    blend: l.common.blend,
                    visible: l.common.visible,
                    transparency_shapes: true,
                    mask: None,
                })
            }
        })
        .collect()
}

/// The merged image: visible layers source-over, bottom first (a preview;
/// readers use the layers).
fn flatten(nodes: &[PsdNode], w: u32, h: u32) -> Vec<u8> {
    let mut acc = vec![[0f32; 4]; (w * h) as usize];
    fn draw(nodes: &[PsdNode], acc: &mut [[f32; 4]], w: u32, h: u32) {
        for n in nodes.iter().rev() {
            match n {
                PsdNode::Group {
                    visible, children, ..
                } => {
                    if *visible {
                        draw(children, acc, w, h);
                    }
                }
                PsdNode::Layer(l) if l.visible => {
                    for y in 0..l.height as i32 {
                        for x in 0..l.width as i32 {
                            let (dx, dy) = (l.left + x, l.top + y);
                            if dx < 0 || dy < 0 || dx >= w as i32 || dy >= h as i32 {
                                continue;
                            }
                            let s = ((y as u32 * l.width + x as u32) * 4) as usize;
                            let Some(p) = l.rgba.get(s..s + 4) else {
                                continue;
                            };
                            let a = p[3] as f32 / 255.0 * l.opacity as f32 / 255.0;
                            let d = &mut acc[(dy as u32 * w + dx as u32) as usize];
                            for c in 0..3 {
                                d[c] = p[c] as f32 / 255.0 * a + d[c] * (1.0 - a);
                            }
                            d[3] = a + d[3] * (1.0 - a);
                        }
                    }
                }
                PsdNode::Layer(_) => {}
            }
        }
    }
    draw(nodes, &mut acc, w, h);
    acc.iter()
        .flat_map(|p| {
            let a = p[3].max(1e-6);
            [
                (p[0] / a * 255.0).round().clamp(0.0, 255.0) as u8,
                (p[1] / a * 255.0).round().clamp(0.0, 255.0) as u8,
                (p[2] / a * 255.0).round().clamp(0.0, 255.0) as u8,
                (p[3] * 255.0).round() as u8,
            ]
        })
        .collect()
}

// --------------------------------------------------------- display content

/// The PSD blend key of a mesh's colour blend and its "transparency shapes
/// layer" flag (§3 table; off only for Add (Glow)).
pub fn psd_blend(c: ColorComposition) -> ([u8; 4], bool) {
    use ColorComposition as C;
    let key = match c {
        C::Normal => *b"norm",
        C::Add | C::AddR2Tsl | C::AddR2 => *b"lddg",
        C::Multiply | C::MultiplyR2 => *b"mul ",
        C::Darken => *b"dark",
        C::Lighten => *b"lite",
        C::Screen => *b"scrn",
        C::Overlay => *b"over",
        C::SoftLight => *b"sLit",
        C::HardLight => *b"hLit",
        C::ColorBurnTsl => *b"idiv",
        C::LinearBurnTsl => *b"lbrn",
        C::ColorDodgeTsl => *b"div ",
        C::LinearLightTsl => *b"lLit",
        C::HslHue => *b"hue ",
        C::HslColor => *b"colr",
    };
    (key, c != C::AddR2)
}

/// Whether the "Notes on PSD Export" notice applies: a visible offscreen
/// part, or a visible mesh with a blend other than Normal-over.
pub fn needs_notice(m: &Model) -> bool {
    m.parts.iter().any(|p| p.offscreen && p.info.visible)
        || m.art_meshes.iter().any(|a| {
            a.info.visible
                && (a.color_composition != Default::default()
                    || a.alpha_composition != Default::default())
        })
}

/// "Export Current Display Content as PSD" (§3): one layer per visible art
/// mesh at the current pose, cropped to its bounds + 4 px, in one level of
/// part folders, with the first clip source as a layer mask; canvas size.
/// Returns the file and notes (extra clip sources).
pub fn display_psd(model: &Model, values: &[f32]) -> Result<(Vec<u8>, Vec<String>), String> {
    // Meshes are rendered alone: clipping becomes layer masks.
    let mut m = model.clone();
    for a in &mut m.art_meshes {
        if !a.clip.is_empty() {
            Arc::make_mut(a).clip.clear();
        }
    }
    let frame = Evaluator::new(&m).evaluate(values);
    let renderer = CpuRenderer::new(&m);
    let (w, h) = (m.canvas.width, m.canvas.height);
    let view = View::fit_canvas(&m, w.max(h));
    let opts = RenderOptions {
        view,
        background: None,
        supersample: 1,
        msaa: false,
    };
    let mut notes = Vec::new();
    let mut rendered: std::collections::HashMap<usize, (i32, i32, u32, u32, Vec<u8>)> =
        Default::default();
    let mut layer_of = |i: usize| -> Result<(i32, i32, u32, u32, Vec<u8>), String> {
        if let Some(l) = rendered.get(&i) {
            return Ok(l.clone());
        }
        let l = render_alone(&renderer, &frame, &opts, i, w, h)?;
        rendered.insert(i, l.clone());
        Ok(l)
    };
    // Back to front.
    let order: Vec<usize> = frame.render_order.clone();
    let mut layers: Vec<(Option<usize>, PsdLayer)> = Vec::new();
    for &i in &order {
        let a = &model.art_meshes[i];
        if !a.info.visible || frame.meshes[i].hidden {
            continue;
        }
        let (left, top, lw, lh, px) = layer_of(i)?;
        let (blend, tsly) = psd_blend(a.color_composition);
        let mask = match a.clip.first() {
            Some(&c) => {
                if a.clip.len() > 1 {
                    notes.push(format!(
                        "{}: only the first clipping mask is exported.",
                        a.info.display_name()
                    ));
                }
                let (ml, mt, mw, mh, mp) = layer_of(c)?;
                Some((ml, mt, mw, mh, mp.chunks_exact(4).map(|p| p[3]).collect()))
            }
            None => None,
        };
        layers.push((
            a.info.parent_part,
            PsdLayer {
                name: a.info.display_name().to_owned(),
                left,
                top,
                width: lw,
                height: lh,
                rgba: px,
                opacity: 255,
                blend,
                visible: true,
                transparency_shapes: tsly,
                mask,
            },
        ));
    }
    // One level of folders: runs of consecutive same-part meshes (draw
    // order) of two or more; the PSD lists top-most first.
    let mut nodes: Vec<PsdNode> = Vec::new();
    let mut k = 0;
    while k < layers.len() {
        let part = layers[k].0;
        let mut j = k + 1;
        while j < layers.len() && layers[j].0 == part && part.is_some() {
            j += 1;
        }
        if j - k >= 2 {
            let name = part
                .and_then(|p| model.parts.get(p))
                .map_or("Part".to_owned(), |p| p.info.display_name().to_owned());
            let children: Vec<PsdNode> = layers[k..j]
                .iter()
                .rev()
                .map(|(_, l)| PsdNode::Layer(l.clone()))
                .collect();
            nodes.push(PsdNode::Group {
                name,
                visible: true,
                children,
            });
        } else {
            nodes.push(PsdNode::Layer(layers[k].1.clone()));
        }
        k = j;
    }
    nodes.reverse();
    let merged = CpuRenderer::new(model)
        .render(&Evaluator::new(model).evaluate(values), &opts)
        .map_err(|e| e.to_string())?;
    Ok((write_psd(w, h, &nodes, &merged.data), notes))
}

/// One mesh rendered alone at canvas size, cropped to its bounds + 4 px.
fn render_alone(
    r: &CpuRenderer,
    frame: &Frame,
    opts: &RenderOptions,
    i: usize,
    w: u32,
    h: u32,
) -> Result<(i32, i32, u32, u32, Vec<u8>), String> {
    let mut f = frame.clone();
    f.items
        .retain(|it| matches!(it, keyform_eval::draw_order::DrawItem::Mesh(k) if *k == i));
    f.render_order.retain(|&k| k == i);
    for (k, s) in f.meshes.iter_mut().enumerate() {
        if k != i {
            s.opacity = 0.0;
        } else {
            // Rendered even when hidden (a hidden clip source).
            s.hidden = false;
        }
    }
    for s in &mut f.art_paths {
        s.opacity = 0.0;
    }
    let img = r.render(&f, opts).map_err(|e| e.to_string())?;
    let pos = &frame.meshes[i].positions;
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in pos.chunks_exact(2) {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let clamp = |v: f32, hi: u32| (v.floor() as i64).clamp(0, i64::from(hi)) as u32;
    let (cx0, cy0) = (clamp(x0 - 4.0, w), clamp(y0 - 4.0, h));
    let (cx1, cy1) = (clamp(x1 + 5.0, w), clamp(y1 + 5.0, h));
    if cx1 <= cx0 || cy1 <= cy0 {
        return Ok((0, 0, 1, 1, vec![0; 4]));
    }
    let (cw, ch) = (cx1 - cx0, cy1 - cy0);
    let mut px = Vec::with_capacity((cw * ch * 4) as usize);
    for y in cy0..cy1 {
        let s = ((y * img.width + cx0) * 4) as usize;
        px.extend_from_slice(&img.data[s..s + (cw * 4) as usize]);
    }
    Ok((cx0 as i32, cy0 as i32, cw, ch, px))
}

// ------------------------------------------------------------------ dialog

#[derive(Default)]
pub struct PsdExportDialog {
    pub open: bool,
    /// "Notes on PSD Export" is showing.
    notice: bool,
}

/// A free name in `dir` for `name` (`name.psd`, `name_2.psd`, …).
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let stem = name.strip_suffix(".psd").unwrap_or(name);
    (2..)
        .map(|n| dir.join(format!("{stem}_{n}.psd")))
        .find(|p| !p.exists())
        .unwrap_or(p)
}

/// The dialog; `simple` is the remembered choice
/// (`PsdExportDialog_radioSimple`), `notice_off` its "don't show again".
pub fn window(
    ctx: &egui::Context,
    st: &mut PsdExportDialog,
    simple: &mut bool,
    notice_off: &mut bool,
    doc: Option<&mut Document>,
    log: &mut crate::log::Log,
) {
    if !st.open {
        return;
    }
    let Some(doc) = doc else {
        st.open = false;
        return;
    };
    if doc.mode != Mode::Modeling || !doc.editable() {
        log.warn("Modeling View must be active to use.");
        st.open = false;
        return;
    }
    if doc.mesh_edit.is_some() {
        log.warn("Please call in Modeling View");
        st.open = false;
        return;
    }
    let mut run = false;
    if st.notice {
        let mut proceed = None;
        egui::Window::new(crate::i18n::tr("Notes on PSD Export"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(crate::i18n::tr(
                    "The model uses offscreen drawing or alpha blend.",
                ));
                ui.label(crate::i18n::tr(
                    "Models with the above settings may not export to PSD correctly.",
                ));
                ui.checkbox(notice_off, crate::i18n::tr("Don't show again"));
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("OK")).clicked() {
                        proceed = Some(true);
                    }
                    if ui.button(crate::i18n::tr("Cancel")).clicked() {
                        proceed = Some(false);
                    }
                });
            });
        match proceed {
            Some(true) => {
                st.notice = false;
                run = true;
            }
            Some(false) => {
                st.notice = false;
                st.open = false;
            }
            None => {}
        }
    } else {
        let (mut ok, mut cancel) = (false, false);
        let mut open = true;
        egui::Window::new(crate::i18n::tr("Export PSD (beta)"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(crate::i18n::tr("PSD Export Type"));
                ui.radio_value(simple, true, crate::i18n::tr("Export All Source Images as PSD"));
                ui.weak(crate::i18n::tr(
                    "You can make individually output by right-clicking the source image in the project palette",
                ));
                ui.radio_value(
                    simple,
                    false,
                    crate::i18n::tr("Export Current Display Content as PSD"),
                );
                ui.horizontal(|ui| {
                    ok = ui.button(crate::i18n::tr("OK")).clicked();
                    cancel = ui.button(crate::i18n::tr("Cancel")).clicked();
                });
            });
        if cancel || !open {
            st.open = false;
        }
        if ok {
            if !*simple && !*notice_off && needs_notice(&doc.model) {
                st.notice = true;
            } else {
                run = true;
            }
        }
    }
    if !run {
        return;
    }
    st.open = false;
    if *simple {
        let files = source_psds(&doc.model);
        if files.is_empty() {
            log.warn("There is no source image to export.");
            return;
        }
        let Some(dir) = crate::file_dialog::FileDialog::new()
            .set_title("Folder for the source images")
            .pick_folder()
        else {
            return;
        };
        for (name, bytes) in files {
            let p = free_path(&dir, &name);
            match std::fs::write(&p, bytes) {
                Ok(()) => log.info(format!("exported {}", p.display())),
                Err(_) => {
                    log.warn(
                        "Failed to save due to lack of access privileges to the selected folder. \
                         Please select another folder.",
                    );
                    return;
                }
            }
        }
    } else {
        let stem = doc
            .path
            .file_stem()
            .map_or("model".to_owned(), |s| s.to_string_lossy().into_owned());
        let Some(path) = crate::file_dialog::FileDialog::new()
            .add_filter("Photoshop (*.psd)", &["psd"])
            .set_file_name(format!("{stem}.psd"))
            .save_file()
        else {
            return;
        };
        match display_psd(&doc.model, &doc.values) {
            Ok((bytes, notes)) => {
                for n in notes {
                    log.info(n);
                }
                match keyform_formats::cmo3::write_atomic(&path, &bytes) {
                    Ok(()) => log.info(format!("exported {}", path.display())),
                    Err(e) => log.error(e.to_string()),
                }
            }
            Err(e) => log.error(e),
        }
    }
}

#[cfg(test)]
mod tests {
    /// Both kinds on the e2e character: the source PSD comes back with its
    /// layers; the display PSD has one layer per visible mesh and parses.
    #[test]
    fn exports_parse_back() {
        let (d, _) = crate::document::e2e_regressions::character("psd-export");
        let files = super::source_psds(&d.model);
        assert_eq!(files.len(), 1);
        let psd = keyform_formats::psd::Psd::parse(&files[0].1).unwrap();
        assert!(!psd.nodes.is_empty());
        let (bytes, _) = super::display_psd(&d.model, &d.values).unwrap();
        // Kept for a look in other readers (target/test-scratch).
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch/psd-export");
        let _ = std::fs::write(dir.join("display.psd"), &bytes);
        let _ = std::fs::write(dir.join("source.psd"), &files[0].1);
        let psd = keyform_formats::psd::Psd::parse(&bytes).unwrap();
        assert_eq!(
            (psd.width, psd.height),
            (d.model.canvas.width, d.model.canvas.height)
        );
        fn count(n: &[keyform_formats::psd::PsdNode]) -> usize {
            n.iter()
                .map(|x| match x {
                    keyform_formats::psd::PsdNode::Layer(_) => 1,
                    keyform_formats::psd::PsdNode::Group(g) => count(&g.children),
                })
                .sum()
        }
        let visible = d.model.art_meshes.iter().filter(|a| a.info.visible).count();
        assert_eq!(count(&psd.nodes), visible);
    }
}
