//! Texture Atlas window: create a page (name, size, which images to
//! insert), browse the pages with a preview, Auto Layout, lock a page.
//! spec: 70-advanced/texture-atlas.md §2, §5, §8; 50-ui/menus.md
//! "Texture Atlas"

use std::sync::Arc;

use eframe::egui;
use keyform_formats::atlas::LayoutSettings;
use keyform_formats::atlas_edit::{
    auto_layout, create_atlas, default_name, default_size, Insert, PAGE_SIZES,
};
use keyform_model::Texture;

use crate::document::Document;
use crate::log::Log;

/// The "Create texture atlas" form.
struct CreateForm {
    name: String,
    width: u32,
    height: u32,
    insert: Insert,
}

pub struct AtlasWindow {
    pub open: bool,
    page: usize,
    create: Option<CreateForm>,
    /// Shown while the Auto Layout settings are open.
    layout_open: bool,
    layout: LayoutSettings,
    /// `isModelImageLayout`: pack whole model images instead of mesh bounds.
    model_image_layout: bool,
    /// Remembered insert option (`TextureAtlasSetting_autoInsertType`).
    insert: Insert,
    /// Preview of the shown page, keyed by its texture.
    preview: Option<(Arc<Texture>, egui::TextureHandle)>,
}

impl Default for AtlasWindow {
    fn default() -> Self {
        Self {
            open: false,
            page: 0,
            create: None,
            layout_open: false,
            layout: LayoutSettings::default(),
            model_image_layout: false,
            insert: Insert::Visible,
            preview: None,
        }
    }
}

impl AtlasWindow {
    /// Edit Texture Atlas: the editor, or the create form when the model
    /// has no page yet. spec: §2
    pub fn show(&mut self, doc: &Document) -> Result<(), String> {
        if !doc.editable() {
            return Err("Only projects can be edited.".into());
        }
        self.open = true;
        self.layout_open = false;
        if doc.model.textures.is_empty() {
            self.start_create(doc);
        }
        Ok(())
    }

    fn start_create(&mut self, doc: &Document) {
        let (width, height) = default_size(&doc.model);
        self.create = Some(CreateForm {
            name: default_name(&doc.model),
            width,
            height,
            insert: self.insert,
        });
    }
}

/// A light checkerboard behind transparent page areas.
fn checker(p: &egui::Painter, r: egui::Rect) {
    let cell = 8.0;
    p.rect_filled(r, 0.0, egui::Color32::from_gray(250));
    let (nx, ny) = (
        (r.width() / cell).ceil() as i32,
        (r.height() / cell).ceil() as i32,
    );
    for y in 0..ny {
        for x in (y % 2..nx).step_by(2) {
            let min = r.min + egui::vec2(x as f32 * cell, y as f32 * cell);
            let c = egui::Rect::from_min_size(min, egui::vec2(cell, cell)).intersect(r);
            p.rect_filled(c, 0.0, egui::Color32::from_gray(220));
        }
    }
}

fn size_combo(ui: &mut egui::Ui, id: &str, v: &mut u32) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(v.to_string())
        .show_ui(ui, |ui| {
            for s in PAGE_SIZES {
                ui.selectable_value(v, s, s.to_string());
            }
        });
}

/// The page preview, decoded once per page version.
fn preview(
    ctx: &egui::Context,
    st: &mut AtlasWindow,
    tex: &Arc<Texture>,
) -> Option<egui::TextureHandle> {
    if let Some((t, h)) = &st.preview {
        if Arc::ptr_eq(t, tex) {
            return Some(h.clone());
        }
    }
    let png = tex.png.as_ref()?;
    let img = keyform_formats::image::RgbaImage::decode_png(png).ok()?;
    let c = egui::ColorImage::from_rgba_unmultiplied(
        [img.width as usize, img.height as usize],
        &img.data,
    );
    let h = ctx.load_texture("atlas-page", c, egui::TextureOptions::LINEAR);
    st.preview = Some((tex.clone(), h.clone()));
    Some(h)
}

pub fn dialog(
    ctx: &egui::Context,
    st: &mut AtlasWindow,
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
    if st.create.is_some() {
        create_dialog(ctx, st, doc, log);
        if doc.model.textures.is_empty() && st.create.is_none() {
            st.open = false;
        }
        return;
    }
    if doc.model.textures.is_empty() {
        st.open = false;
        return;
    }
    st.page = st.page.min(doc.model.textures.len() - 1);
    let mut open = true;
    let (mut add, mut run_layout) = (false, false);
    let mut lock = None;
    egui::Window::new("Texture Atlas")
        .collapsible(false)
        .default_width(560.0)
        .open(&mut open)
        .show(ctx, |ui| {
            let m = &doc.model;
            ui.horizontal(|ui| {
                egui::ComboBox::from_label("Page")
                    .selected_text(&m.textures[st.page].name)
                    .show_ui(ui, |ui| {
                        for (k, t) in m.textures.iter().enumerate() {
                            ui.selectable_value(&mut st.page, k, &t.name);
                        }
                    });
                add = ui.button("Add texture…").clicked();
            });
            let tex = m.textures[st.page].clone();
            ui.label(format!(
                "{} × {} px, {} model images",
                tex.width,
                tex.height,
                tex.entries.len()
            ));
            let mut locked = tex.locked;
            if ui
                .checkbox(&mut locked, "Lock texture")
                .on_hover_text("Use the stored page image as is; it is not re-rendered")
                .changed()
            {
                lock = Some(locked);
            }
            ui.separator();
            match preview(ui.ctx(), st, &tex) {
                Some(h) => {
                    let side = ui.available_width().min(400.0);
                    let k = side / tex.width.max(tex.height) as f32;
                    let size = egui::vec2(tex.width as f32 * k, tex.height as f32 * k);
                    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    checker(ui.painter(), r);
                    ui.painter().image(
                        h.id(),
                        r,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                None => {
                    ui.weak("(no page image)");
                }
            }
            ui.separator();
            ui.add_enabled_ui(!tex.locked, |ui| {
                if ui.button("Auto Layout…").clicked() {
                    st.layout_open = !st.layout_open;
                }
            });
            if st.layout_open && !tex.locked {
                egui::Grid::new("atlas-auto-layout")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Margin (px)");
                        ui.add(egui::DragValue::new(&mut st.layout.margin).range(0.0..=256.0));
                        ui.end_row();
                        ui.label("Rotation");
                        ui.checkbox(&mut st.layout.rotate, "Allow 90° rotation");
                        ui.end_row();
                        ui.label("Layout");
                        ui.checkbox(
                            &mut st.model_image_layout,
                            "Whole model images (not only the mesh bounds)",
                        );
                        ui.end_row();
                        ui.label("Scale");
                        ui.horizontal(|ui| {
                            let mut auto = st.layout.scale <= 0.0;
                            if ui.checkbox(&mut auto, "Automatic").changed() {
                                st.layout.scale = if auto { 0.0 } else { 1.0 };
                            }
                            if !auto {
                                ui.add(
                                    egui::DragValue::new(&mut st.layout.scale)
                                        .speed(0.01)
                                        .range(0.01..=16.0),
                                );
                            }
                        });
                        ui.end_row();
                    });
                run_layout = ui.button("Run Auto Layout").clicked();
            }
        });
    if !open {
        st.open = false;
        st.preview = None;
        return;
    }
    let t = st.page;
    if let Some(locked) = lock {
        let _ = doc.try_edit("Lock/unlock texture", |m| {
            Arc::make_mut(&mut m.textures[t]).locked = locked;
            Ok(())
        });
    }
    if run_layout {
        let (s, mesh_bounds) = (st.layout, !st.model_image_layout);
        match doc.try_edit("Auto Layout", |m| {
            auto_layout(m, t, &s, mesh_bounds).map_err(|e| e.to_string())
        }) {
            Ok(0) => {}
            Ok(n) => log.warn(format!("{n} model images did not fit on the texture.")),
            Err(e) => log.warn(e),
        }
    }
    if add {
        st.start_create(doc);
    }
}

fn create_dialog(ctx: &egui::Context, st: &mut AtlasWindow, doc: &mut Document, log: &mut Log) {
    let Some(f) = st.create.as_mut() else { return };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new("Create texture atlas")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Grid::new("atlas-create")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut f.name);
                    ui.end_row();
                    ui.label("Width");
                    size_combo(ui, "atlas-w", &mut f.width);
                    ui.end_row();
                    ui.label("Height");
                    size_combo(ui, "atlas-h", &mut f.height);
                    ui.end_row();
                    ui.label("Insert");
                    ui.vertical(|ui| {
                        ui.radio_value(&mut f.insert, Insert::Visible, "Visible model images");
                        ui.radio_value(&mut f.insert, Insert::All, "All model images");
                        ui.radio_value(&mut f.insert, Insert::None, "None");
                    });
                    ui.end_row();
                });
            ui.separator();
            ui.horizontal(|ui| {
                ok = ui.button("OK").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
    if !open || cancel {
        st.create = None;
        return;
    }
    if !ok {
        return;
    }
    let Some(f) = st.create.take() else { return };
    st.insert = f.insert;
    let name = if f.name.trim().is_empty() {
        default_name(&doc.model)
    } else {
        f.name.trim().to_owned()
    };
    match doc.try_edit("Create texture atlas", |m| {
        create_atlas(m, &name, f.width, f.height, f.insert).map_err(|e| e.to_string())
    }) {
        Ok(t) => st.page = t,
        Err(e) => log.warn(e),
    }
}
