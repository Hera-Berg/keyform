//! File ▸ New ▸ Model: "Create with specified size" (width, height, Fix
//! Aspect Ratio). spec: 50-ui/dialogs.md §1, 50-ui/menus.md "File"

use eframe::egui;

pub struct NewModelDialog {
    pub open: bool,
    width: u32,
    height: u32,
    fix_aspect: bool,
}

impl Default for NewModelDialog {
    fn default() -> Self {
        // The reference's defaults (observed): 1000 × 2000, aspect fixed.
        Self {
            open: false,
            width: 1000,
            height: 2000,
            fix_aspect: true,
        }
    }
}

/// Shows the dialog; returns the size when Create is pressed.
pub fn dialog(ctx: &egui::Context, st: &mut NewModelDialog) -> Option<(u32, u32)> {
    if !st.open {
        return None;
    }
    let mut open = true;
    let mut create = false;
    egui::Window::new("New Model")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("Create with specified size");
            let (w0, h0) = (st.width, st.height);
            egui::Grid::new("new-model").num_columns(3).show(ui, |ui| {
                ui.label("Width");
                ui.add(egui::DragValue::new(&mut st.width).range(1..=16384));
                ui.label("pixel");
                ui.end_row();
                ui.label("Height");
                ui.add(egui::DragValue::new(&mut st.height).range(1..=16384));
                ui.label("pixel");
                ui.end_row();
            });
            if st.fix_aspect && w0 > 0 && h0 > 0 {
                if st.width != w0 {
                    st.height = ((st.width as f64 * h0 as f64 / w0 as f64).round() as u32).max(1);
                } else if st.height != h0 {
                    st.width = ((st.height as f64 * w0 as f64 / h0 as f64).round() as u32).max(1);
                }
            }
            ui.checkbox(&mut st.fix_aspect, "Fix Aspect Ratio");
            ui.separator();
            ui.horizontal(|ui| {
                create = ui.button("Create").clicked();
                if ui.button("Cancel").clicked() {
                    st.open = false;
                }
            });
        });
    if !open {
        st.open = false;
    }
    if create {
        st.open = false;
        return Some((st.width, st.height));
    }
    None
}
