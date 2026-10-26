//! Automatic Mesh generator: the dialog for the selected art meshes and the
//! popup in mesh edit mode with a live preview.
//! spec: 70-advanced/auto-mesh.md §1–2, §5

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui;
use keyform_edit::automesh::{
    auto_mesh, generate, generated_graph, is_glued, presets, AlphaImage, GLUED, NO_SELECTION,
};
use keyform_model::{MeshGenerateSetting, Model, DEFAULT_DENSITY_SCALE};

use crate::document::{Document, Selection};
use crate::log::Log;

#[derive(Default)]
pub struct AutoMeshDialog {
    pub open: bool,
    /// Popup type: edits the mesh editor's working graph with a preview.
    popup: bool,
    setting: MeshGenerateSetting,
    density_scale: f32,
    preset: Option<usize>,
    /// Decoded model-image alpha by model image index.
    cache: HashMap<usize, Arc<AlphaImage>>,
    /// Settings changed since the last preview.
    dirty: bool,
}

fn selected_meshes(doc: &Document) -> Vec<usize> {
    doc.selection
        .objects
        .iter()
        .filter_map(|o| match o {
            Selection::ArtMesh(a) => Some(*a),
            _ => None,
        })
        .collect()
}

impl AutoMeshDialog {
    /// Opens the dialog type for the selected art meshes, or the popup
    /// type in mesh edit mode. Fails with the §1 messages.
    pub fn show(&mut self, doc: &Document) -> Result<(), String> {
        let meshes = match &doc.mesh_edit {
            Some(s) => vec![s.mesh],
            None => selected_meshes(doc),
        };
        let Some(&first) = meshes.first() else {
            return Err(NO_SELECTION.into());
        };
        if meshes.iter().any(|&i| is_glued(&doc.model, i)) {
            return Err(GLUED.into());
        }
        self.open = true;
        self.popup = doc.mesh_edit.is_some();
        self.setting = doc.model.art_meshes[first].generator;
        self.density_scale = DEFAULT_DENSITY_SCALE;
        self.preset = None;
        self.dirty = self.popup;
        Ok(())
    }
}

/// The model image alpha of art mesh `i` (decoded once per dialog).
fn alpha_of(
    m: &Model,
    i: usize,
    cache: &mut HashMap<usize, Arc<AlphaImage>>,
) -> Result<(Arc<AlphaImage>, [f32; 6]), String> {
    let a = &m.art_meshes[i];
    let k = a
        .model_image
        .ok_or_else(|| format!("{}: the ArtMesh has no model image.", a.info.display_name()))?;
    let mi = &m.model_images[k];
    if let Some(img) = cache.get(&k) {
        return Ok((img.clone(), mi.local_to_canvas));
    }
    let png = mi
        .png
        .as_ref()
        .ok_or_else(|| format!("{}: the model image has no pixels.", a.info.display_name()))?;
    let img = keyform_formats::image::RgbaImage::decode_png(png).map_err(|e| e.to_string())?;
    let alpha = Arc::new(AlphaImage {
        width: img.width,
        height: img.height,
        alpha: img.data.chunks_exact(4).map(|p| p[3]).collect(),
    });
    cache.insert(k, alpha.clone());
    Ok((alpha, mi.local_to_canvas))
}

fn int_row(
    ui: &mut egui::Ui,
    label: &str,
    v: &mut i32,
    range: std::ops::RangeInclusive<i32>,
) -> bool {
    ui.label(label);
    let r = ui.add(egui::Slider::new(v, range));
    ui.end_row();
    r.changed()
}

pub fn dialog(
    ctx: &egui::Context,
    st: &mut AutoMeshDialog,
    doc: Option<&mut Document>,
    log: &mut Log,
) {
    if !st.open {
        return;
    }
    let Some(doc) = doc else {
        st.open = false;
        return;
    };
    if st.popup && doc.mesh_edit.is_none() {
        st.open = false;
        return;
    }
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new("Automatic Mesh Generator")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            let presets = presets();
            let label = st.preset.map_or("(custom)", |p| presets[p].0);
            egui::ComboBox::from_label("Preset")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (k, (name, s, d)) in presets.iter().enumerate() {
                        if ui.selectable_label(st.preset == Some(k), *name).clicked() {
                            st.preset = Some(k);
                            st.setting = *s;
                            st.density_scale = *d;
                            st.dirty = true;
                        }
                    }
                });
            ui.separator();
            let s = &mut st.setting;
            let mut changed = false;
            egui::Grid::new("auto-mesh").num_columns(2).show(ui, |ui| {
                changed |= int_row(
                    ui,
                    "Vertices interval (outside)",
                    &mut s.outer_density,
                    10..=200,
                );
                changed |= int_row(
                    ui,
                    "Vertices interval (inside)",
                    &mut s.inner_density,
                    10..=200,
                );
                changed |= int_row(ui, "Boundary margin (outside)", &mut s.margin, 0..=50);
                changed |= int_row(ui, "Boundary margin (inside)", &mut s.inner_margin, 0..=50);
                changed |= int_row(ui, "Minimum boundary margin", &mut s.min_margin, 1..=10);
                changed |= int_row(
                    ui,
                    "Minimum number of points on a boundary",
                    &mut s.min_bounds_pt,
                    3..=20,
                );
                changed |= int_row(
                    ui,
                    "Alpha value (Transparency)",
                    &mut s.threshold_alpha,
                    0..=254,
                );
            });
            ui.weak("Intervals and margins are pixel values after re-sizing to 1024.");
            if changed {
                st.preset = None;
                st.dirty = true;
            }
            ui.separator();
            ui.horizontal(|ui| {
                ok = ui.button("OK").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
    if !open {
        cancel = true;
    }
    if st.popup {
        popup_step(st, doc, log, ok, cancel);
        return;
    }
    if cancel {
        st.open = false;
        return;
    }
    if !ok {
        return;
    }
    st.open = false;
    let meshes = selected_meshes(doc);
    if meshes.is_empty() {
        log.warn(NO_SELECTION);
        return;
    }
    let mut images = Vec::new();
    for &i in &meshes {
        match alpha_of(&doc.model, i, &mut st.cache) {
            Ok((img, _)) => images.push((i, img)),
            Err(e) => {
                log.warn(e);
                return;
            }
        }
    }
    let (setting, scale) = (st.setting, st.density_scale);
    let r = doc.try_edit("Automatic Mesh Generation", |m| {
        let mut crossings = 0;
        for (i, img) in &images {
            crossings += auto_mesh(m, *i, &setting, scale, img)?.unresolved;
        }
        Ok(crossings)
    });
    match r {
        Ok(c) => {
            doc.set_selection(crate::document::SelectionSet {
                objects: meshes.into_iter().map(Selection::ArtMesh).collect(),
                points: Vec::new(),
            });
            if c > 0 {
                log.warn(format!("{c} crossing edges could not be kept in the mesh."));
            }
        }
        Err(e) => log.warn(e),
    }
}

/// Popup type: regenerate the preview when the settings change; OK keeps
/// it (one mesh-editor step, settings stored when mesh editing finishes),
/// Cancel restores the graph.
fn popup_step(st: &mut AutoMeshDialog, doc: &mut Document, log: &mut Log, ok: bool, cancel: bool) {
    let Some(i) = doc.mesh_edit.as_ref().map(|s| s.mesh) else {
        return;
    };
    if cancel {
        if let Some(s) = doc.mesh_edit.as_mut() {
            s.end_preview(false);
        }
        st.open = false;
        return;
    }
    if st.dirty {
        st.dirty = false;
        match alpha_of(&doc.model, i, &mut st.cache) {
            Ok((img, t)) => {
                let g = generate(&img, t, &st.setting, st.density_scale);
                if let Some(s) = doc.mesh_edit.as_mut() {
                    let graph = generated_graph(s.preview_base(), &g);
                    s.set_preview(graph);
                }
            }
            Err(e) => {
                log.warn(e);
                st.open = false;
                return;
            }
        }
    }
    if ok {
        if let Some(s) = doc.mesh_edit.as_mut() {
            s.end_preview(true);
            s.generator = Some(st.setting);
        }
        st.open = false;
    }
}
