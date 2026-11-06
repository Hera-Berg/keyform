//! Soft Selection (Brush Selection tool [B]): paint weighted point
//! selections; and the B / C / V + drag brush adjustments shared by the
//! brush tools. spec: 50-ui/canvas-and-tools.md §1 ("Brush tools"), §4
//! "Soft Selection", §5.1 "Brush"; inspector-fields.md
//! (ToolMode_BrushSelection$Setting)

use eframe::egui;
use keyform_edit::glue_brush::falloff;
use keyform_edit::StepKind;

use crate::document::{Document, Mode, Selection};
use crate::glue_tool::GluePrefs;

/// How a stroke changes the weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoftMode {
    /// A plain stroke: the point selection is cleared at its start.
    Paint,
    /// Shift: added to the selection.
    Add,
    /// Ctrl+Shift: weight subtracted.
    Subtract,
}

/// The Soft Selection tool, when active.
pub struct SoftSelect {
    /// A stroke is in progress (one "Select" history step at its end).
    stroke: bool,
}

impl Document {
    /// Turns the Soft Selection tool on or off.
    pub fn set_soft_select(&mut self, on: bool) -> Result<(), String> {
        if on && (!self.editable() || self.mode != Mode::Modeling || self.mesh_edit.is_some()) {
            return Err("Soft selection works in the Modeling workspace of a project.".into());
        }
        self.soft_select = on.then_some(SoftSelect { stroke: false });
        if on {
            let _ = self.set_glue_tool(None);
            self.lasso = false;
        }
        Ok(())
    }

    /// One drag event of a soft-selection stroke at canvas `at`: each point
    /// of the selected objects gets ± level·falloff added to its weight,
    /// clamped to [0, 1]. spec: canvas-and-tools.md §5.1 (same weight
    /// formula as the brush)
    pub fn soft_paint(&mut self, at: [f32; 2], mode: SoftMode, start: bool, flip: bool) {
        let Some(st) = self.soft_select.as_mut() else {
            return;
        };
        st.stroke = true;
        let prefs = self.glue_prefs_for_soft();
        let mut sel = self.selection.clone();
        if start && mode == SoftMode::Paint {
            sel.set_points(Vec::new());
        }
        // Current weights.
        let mut w: Vec<((Selection, usize), f32)> = sel
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| (*p, sel.weight(i)))
            .collect();
        for &o in &sel.objects {
            for (k, p) in self.object_points(o).iter().enumerate() {
                let d = prefs.distance([p[0] - at[0], p[1] - at[1]], flip);
                let Some(f) = falloff(d, prefs.size / 2.0, prefs.hardness) else {
                    continue;
                };
                let a = (prefs.level * f).clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let delta = if mode == SoftMode::Subtract { -a } else { a };
                match w.iter_mut().find(|(q, _)| *q == (o, k)) {
                    Some((_, x)) => *x = (*x + delta).clamp(0.0, 1.0),
                    None if delta > 0.0 => w.push(((o, k), delta.min(1.0))),
                    None => {}
                }
            }
        }
        w.retain(|(_, x)| *x > 0.0);
        sel.points = w.iter().map(|(p, _)| *p).collect();
        sel.point_weights = w.iter().map(|(_, x)| *x).collect();
        // Within the stroke the selection changes without history steps.
        self.selection = sel;
    }

    /// Ends a stroke: one selection step.
    pub fn soft_end(&mut self) {
        if let Some(st) = self.soft_select.as_mut() {
            if std::mem::take(&mut st.stroke) {
                let state = self.edit_state();
                self.history
                    .commit("Select", StepKind::SelectionOnly, state, false);
            }
        }
    }

    pub fn soft_stroke_active(&self) -> bool {
        self.soft_select.as_ref().is_some_and(|s| s.stroke)
    }

    fn glue_prefs_for_soft(&self) -> GluePrefs {
        self.soft_prefs
    }
}

/// B / C / V + drag: brush size, angle, hardness. Each drag event sums
/// both axes of the movement since the previous event, s = dx + dy: B adds
/// s in canvas units to the size (0.1…10000), C adds 0.25°·s (screen px,
/// wrapped to [−180°, 180°)), V adds 0.005·s (0…1); nothing is painted.
/// Returns whether the drag was taken. spec: canvas-and-tools.md §4
/// ("Brush adjust drags")
pub fn brush_adjust(
    ui: &egui::Ui,
    response: &egui::Response,
    zoom: f32,
    (size, angle, hardness): (&mut f32, &mut f32, &mut f32),
) -> bool {
    if ui.ctx().egui_wants_keyboard_input() {
        return false;
    }
    let (b, c, v) = ui.input(|i| {
        (
            i.key_down(egui::Key::B),
            i.key_down(egui::Key::C),
            i.key_down(egui::Key::V),
        )
    });
    if !(b || c || v) {
        return false;
    }
    let primary = egui::PointerButton::Primary;
    if response.dragged_by(primary) {
        let d = response.drag_delta();
        let s = d.x + d.y;
        if b {
            // Pen pressure would scale the size first (1 for a mouse).
            *size = (*size + s / zoom.max(1e-6)).clamp(0.1, 10000.0);
        } else if c {
            *angle = (*angle + 0.25 * s + 180.0).rem_euclid(360.0) - 180.0;
        } else {
            *hardness = (*hardness + 0.005 * s).clamp(0.0, 1.0);
        }
    }
    response.dragged_by(primary) || response.drag_started_by(primary)
}

/// Canvas input of the Soft Selection tool.
pub fn input(
    ui: &egui::Ui,
    response: &egui::Response,
    doc: &mut Document,
    rect: egui::Rect,
    ppp: f32,
) {
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (space, rkey, shift, cmd, moved) = ui.input(|i| {
        (
            !typing && i.key_down(egui::Key::Space),
            !typing && i.key_down(egui::Key::R),
            i.modifiers.shift,
            i.modifiers.command,
            i.pointer.delta() != egui::Vec2::ZERO,
        )
    });
    if space || rkey {
        return;
    }
    let zoom = doc.view.zoom;
    let p = &mut doc.soft_prefs;
    if brush_adjust(
        ui,
        response,
        zoom,
        (&mut p.size, &mut p.angle, &mut p.hardness),
    ) {
        return;
    }
    let view = crate::canvas::view_of(doc, rect, ppp);
    let to_canvas =
        |q: egui::Pos2| view.to_canvas([(q.x - rect.min.x) * ppp, (q.y - rect.min.y) * ppp]);
    let mode = match (shift, cmd) {
        (true, true) => SoftMode::Subtract,
        (true, false) => SoftMode::Add,
        _ => SoftMode::Paint,
    };
    let flip = doc.view.mirror;
    let primary = egui::PointerButton::Primary;
    if response.drag_started_by(primary) || response.clicked_by(primary) {
        if let Some(q) = ui.input(|i| i.pointer.press_origin()) {
            doc.soft_paint(to_canvas(q), mode, true, flip);
        }
    } else if response.dragged_by(primary) && moved {
        if let Some(q) = ui.input(|i| i.pointer.interact_pos()) {
            doc.soft_paint(to_canvas(q), mode, false, flip);
        }
    }
    if doc.soft_stroke_active() && !response.dragged_by(primary) {
        doc.soft_end();
    }
}

/// Tool Details of the Soft Selection tool.
pub fn controls(ui: &mut egui::Ui, doc: &mut Document) {
    ui.strong("Soft Selection");
    let p = &mut doc.soft_prefs;
    egui::Grid::new("soft-select")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Weight");
            let mut level = p.level * 100.0;
            if ui
                .add(
                    egui::DragValue::new(&mut level)
                        .range(1.0..=100.0)
                        .speed(0.1)
                        .suffix(" %"),
                )
                .changed()
            {
                p.level = level / 100.0;
            }
            ui.end_row();
            ui.label("Brush shape");
            egui::ComboBox::from_id_salt("soft-shape")
                .selected_text(format!("{:?}", p.shape))
                .show_ui(ui, |ui| {
                    use crate::mesh_edit::BrushShape;
                    for s in [BrushShape::Circle, BrushShape::Square, BrushShape::Line] {
                        ui.selectable_value(&mut p.shape, s, format!("{s:?}"));
                    }
                });
            ui.end_row();
            ui.label("Size");
            ui.add(
                egui::DragValue::new(&mut p.size)
                    .range(0.1..=10000.0)
                    .suffix(" px"),
            );
            ui.end_row();
            ui.label("Angle");
            ui.add(
                egui::DragValue::new(&mut p.angle)
                    .range(-180.0..=180.0)
                    .speed(0.1)
                    .suffix("°"),
            );
            ui.end_row();
            ui.label("Hardness");
            let mut h = p.hardness * 100.0;
            if ui
                .add(
                    egui::DragValue::new(&mut h)
                        .range(0.0..=100.0)
                        .speed(0.1)
                        .suffix(" %"),
                )
                .changed()
            {
                p.hardness = h / 100.0;
            }
            ui.end_row();
        });
    ui.weak("Drag to paint the selection weight of the selected objects' points; Shift adds, Ctrl+Shift subtracts. B / C / V + drag: size / angle / hardness.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Log;
    use std::path::Path;

    /// Painting weights then dragging moves each vertex by weight × offset
    /// (on screen, i.e. canvas) in the current keyform; one "Select" step
    /// per stroke.
    #[test]
    fn soft_selection_paint_and_drag() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("hiyori_pro_t11.cmo3");
        if !path.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        doc.tick(0.0, &Default::default(), &mut log);
        // A mesh on a key at the current values.
        let a = (0..doc.model.art_meshes.len())
            .find(|&a| {
                doc.model.art_meshes[a].vertex_count() > 20
                    && keyform_edit::current_cell(&doc.model, Selection::ArtMesh(a), &doc.values)
                        .is_ok()
            })
            .unwrap();
        doc.select(Some(Selection::ArtMesh(a)));
        doc.set_soft_select(true).unwrap();
        let pts = doc.object_points(Selection::ArtMesh(a));
        let at = pts[0];
        doc.soft_prefs.size = 60.0;
        doc.soft_prefs.hardness = 0.0;
        doc.soft_prefs.level = 0.5;
        doc.soft_paint(at, SoftMode::Paint, true, false);
        doc.soft_end();
        assert_eq!(doc.history.undo_name(), Some("Select"));
        let sel = doc.selection.clone();
        assert!(!sel.points.is_empty());
        // The centre point got level × 1 = 0.5; farther ones less.
        let k0 = sel.points.iter().position(|p| p.1 == 0).unwrap();
        assert!((sel.weight(k0) - 0.5).abs() < 1e-5);
        assert!(sel
            .point_weights
            .iter()
            .all(|&w| w > 0.0 && w <= 0.5 + 1e-6));
        // Shift adds: weight 0.5 + 0.5 = 1 at the centre.
        doc.soft_paint(at, SoftMode::Add, true, false);
        doc.soft_end();
        let k0 = doc.selection.points.iter().position(|p| p.1 == 0).unwrap();
        assert!((doc.selection.weight(k0) - 1.0).abs() < 1e-5);
        // Drag the weighted points by (10, 0) canvas px.
        let before = doc.object_points(Selection::ArtMesh(a));
        doc.drag_begin(true).unwrap();
        doc.drag_update([10.0, 0.0], false);
        doc.drag_end();
        doc.tick(0.0, &Default::default(), &mut log);
        let after = doc.object_points(Selection::ArtMesh(a));
        for (k, &(o, i)) in doc.selection.points.iter().enumerate() {
            assert_eq!(o, Selection::ArtMesh(a));
            let w = doc.selection.weight(k);
            let dx = after[i][0] - before[i][0];
            assert!(
                (dx - 10.0 * w).abs() < 0.05,
                "point {i}: moved {dx}, weight {w}"
            );
        }
        // Unselected points stay.
        let moved = (0..before.len())
            .filter(|&i| (after[i][0] - before[i][0]).abs() > 1e-3)
            .count();
        assert!(moved <= doc.selection.points.len());
    }
}
