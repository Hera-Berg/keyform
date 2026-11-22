//! File ▸ Canvas Settings…: Width, Height and a 3×3 Placement anchor; one
//! undo step "Change Canvas Size". spec: 50-ui/dialogs.md §3

use eframe::egui;

use crate::document::{Document, Mode};

#[derive(Default)]
pub struct CanvasDialog {
    /// (width, height, anchor column, anchor row) being edited.
    edit: Option<(u32, u32, u8, u8)>,
}

impl CanvasDialog {
    pub fn open(&mut self, doc: &Document) {
        self.edit = Some((doc.model.canvas.width, doc.model.canvas.height, 1, 1));
    }
}

pub fn window(ctx: &egui::Context, st: &mut CanvasDialog, doc: Option<&mut Document>) {
    let Some((mut w, mut h, mut col, mut row)) = st.edit else {
        return;
    };
    let Some(doc) = doc.filter(|d| d.editable() && d.mode == Mode::Modeling) else {
        st.edit = None;
        return;
    };
    let (mut ok, mut cancel) = (false, false);
    let mut open = true;
    egui::Window::new(crate::i18n::tr("Canvas Settings"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Grid::new("canvas-settings")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label(crate::i18n::tr("Width"));
                    ui.add(egui::DragValue::new(&mut w).range(1..=30000).suffix(" px"));
                    ui.end_row();
                    ui.label(crate::i18n::tr("Height"));
                    ui.add(egui::DragValue::new(&mut h).range(1..=30000).suffix(" px"));
                    ui.end_row();
                    ui.label(crate::i18n::tr("Placement"));
                    egui::Grid::new("canvas-anchor")
                        .spacing([2.0, 2.0])
                        .show(ui, |ui| {
                            const NAMES: [[&str; 3]; 3] = [
                                ["Top left", "Top", "Top right"],
                                ["Left", "Centre", "Right"],
                                ["Bottom left", "Bottom", "Bottom right"],
                            ];
                            for (r, names) in NAMES.iter().enumerate() {
                                for (c, name) in names.iter().enumerate() {
                                    let on = (col, row) == (c as u8, r as u8);
                                    let b = ui
                                        .add(egui::Button::selectable(
                                            on,
                                            if on { "●" } else { "○" },
                                        ))
                                        .on_hover_text(crate::i18n::tr(name));
                                    crate::icons::accessible_name(&b, crate::i18n::tr(name));
                                    if b.clicked() {
                                        (col, row) = (c as u8, r as u8);
                                    }
                                }
                                ui.end_row();
                            }
                        });
                    ui.end_row();
                });
            ui.separator();
            ui.horizontal(|ui| {
                ok = ui.button(crate::i18n::tr("OK")).clicked();
                cancel = ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
    if ok {
        if let Err(e) = doc.try_edit("Change Canvas Size", |m| {
            keyform_edit::canvas_size::change_canvas_size(m, w, h, (col, row))
        }) {
            doc.notice = Some(e);
        }
        doc.view.fit_pending = true;
    }
    st.edit = if ok || cancel || !open {
        None
    } else {
        Some((w, h, col, row))
    };
}

/// File ▸ Resize Model…: one prompt, Resize Factor (%), default 100,
/// range 1–5000; one undo step "Resize Document". spec: dialogs.md §22
#[derive(Default)]
pub struct ResizeDialog {
    edit: Option<f32>,
}

impl ResizeDialog {
    pub fn open(&mut self) {
        self.edit = Some(100.0);
    }
}

pub fn resize_window(ctx: &egui::Context, st: &mut ResizeDialog, doc: Option<&mut Document>) {
    let Some(mut pct) = st.edit else {
        return;
    };
    let Some(doc) = doc.filter(|d| d.editable() && d.mode == Mode::Modeling) else {
        st.edit = None;
        return;
    };
    let (mut ok, mut cancel) = (false, false);
    let mut open = true;
    egui::Window::new(crate::i18n::tr("Resize Model"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Resize Factor"));
                let r = ui.add(
                    egui::DragValue::new(&mut pct)
                        .range(1.0..=5000.0)
                        .suffix(" %"),
                );
                crate::icons::accessible_name(&r, crate::i18n::tr("Resize Factor"));
            });
            ui.horizontal(|ui| {
                ok = ui.button(crate::i18n::tr("OK")).clicked();
                cancel = ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
    if ok {
        if let Err(e) = doc.try_edit("Resize Document", |m| {
            keyform_edit::canvas_size::resize_model(m, pct)
        }) {
            doc.notice = Some(e);
        }
    }
    st.edit = if ok || cancel || !open {
        None
    } else {
        Some(pct)
    };
}
