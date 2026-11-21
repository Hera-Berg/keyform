//! Show ▸ Grid and Show ▸ Guide: the Modeling view's grid and ruler guides,
//! their settings dialogs and snapping. spec: 50-ui/view-aids.md §1,
//! 50-ui/menus.md (Show)
//!
//! Grid: view-aids.md §6. Guides: view-aids.md §1 (stored RGBA, snapping
//! guides green, selected or dragged guides 3 px wide). The Guide Settings
//! layout (table No / Type / Position / Color / Memo, New Guide / Delete,
//! Features ▸ Edit All Guide Colors…, Setting ▸ Default Guide Line
//! Color…) follows the reference (Q-0050).

use eframe::egui::{self, Color32, Pos2, Stroke};
use keyform_model::Guide;

use crate::document::{Document, Mode};

/// App-wide view-aid settings (persisted with the other settings).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ViewAids {
    pub show_grid: bool,
    /// Grid spacing in canvas pixels.
    pub grid_spacing: f32,
    pub grid_color: [u8; 3],
    pub show_guides: bool,
    pub snap_guides: bool,
    /// Colour of new guides (Guide Settings ▸ Setting ▸ Default Guide Line
    /// Color…).
    pub default_guide_color: [f32; 4],
}

impl Default for ViewAids {
    fn default() -> Self {
        Self {
            show_grid: false,
            grid_spacing: 50.0,
            grid_color: [0x80, 0x80, 0x80],
            show_guides: false,
            snap_guides: false,
            default_guide_color: Guide::DEFAULT_COLOR,
        }
    }
}

impl ViewAids {
    /// Whether guides are drawn (and snap) on `doc`'s canvas.
    pub fn guides_on(&self, doc: &Document) -> bool {
        self.show_guides && doc.mode == Mode::Modeling && !doc.model.guides.is_empty()
    }
}

/// A guide being dragged on the canvas: its index and current position.
pub type GuideDrag = Option<(usize, f32)>;

/// Snap distance: 2 screen pixels in canvas units (view-aids.md §1).
pub fn snap_distance(zoom: f32) -> f32 {
    2.0 / zoom.max(1e-6)
}

/// Snaps `p` to the guides within `s` (canvas units): x to the nearest
/// vertical guide, y to the nearest horizontal one. Returns the snapped
/// point and the guides that snapped (vertical, horizontal).
pub fn snap(guides: &[Guide], p: [f32; 2], s: f32) -> ([f32; 2], [Option<usize>; 2]) {
    let mut out = p;
    let mut hit = [None, None];
    for (axis, vertical) in [(0usize, true), (1usize, false)] {
        let best = guides
            .iter()
            .enumerate()
            .filter(|(_, g)| g.vertical == vertical)
            .map(|(i, g)| (i, (g.position - p[axis]).abs()))
            .filter(|&(_, d)| d <= s)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = best {
            out[axis] = guides[i].position;
            hit[axis] = Some(i);
        }
    }
    (out, hit)
}

/// The nearest of `points` inside a ±`s` box around `p` (rotation
/// deformers snapping to each other's origins, view-aids.md §1).
pub fn snap_to_points(points: &[[f32; 2]], p: [f32; 2], s: f32) -> Option<[f32; 2]> {
    points
        .iter()
        .copied()
        .filter(|q| (q[0] - p[0]).abs() <= s && (q[1] - p[1]).abs() <= s)
        .min_by(|a, b| {
            let d = |q: &[f32; 2]| (q[0] - p[0]).hypot(q[1] - p[1]);
            d(a).total_cmp(&d(b))
        })
}

/// The guide whose line passes within 3 screen pixels of `p`.
pub fn guide_hit(doc: &Document, to_screen: &dyn Fn([f32; 2]) -> Pos2, p: Pos2) -> Option<usize> {
    doc.model
        .guides
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let (a, b) = line_ends(doc, g, g.position, to_screen);
            (i, point_line_distance(p, a, b))
        })
        .filter(|&(_, d)| d <= 3.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

fn point_line_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len = ab.length();
    if len < 1e-6 {
        return (p - a).length();
    }
    ((p - a).x * ab.y - (p - a).y * ab.x).abs() / len
}

/// Two far points of a guide's line, in screen space.
fn line_ends(
    doc: &Document,
    g: &Guide,
    position: f32,
    to_screen: &dyn Fn([f32; 2]) -> Pos2,
) -> (Pos2, Pos2) {
    let (w, h) = (
        doc.model.canvas.width as f32,
        doc.model.canvas.height as f32,
    );
    let far = 20.0 * w.max(h).max(1.0);
    if g.vertical {
        (to_screen([position, -far]), to_screen([position, far]))
    } else {
        (to_screen([-far, position]), to_screen([far, position]))
    }
}

/// Draws the grid (over the canvas) and the guides (across the view).
pub fn draw(
    painter: &egui::Painter,
    doc: &Document,
    aids: &ViewAids,
    to_screen: &dyn Fn([f32; 2]) -> Pos2,
    drag: GuideDrag,
    snapping: [Option<usize>; 2],
) {
    if doc.mode != Mode::Modeling {
        return;
    }
    if aids.show_grid && aids.grid_spacing > 0.0 {
        draw_grid(painter, doc, aids, to_screen);
    }
    if aids.guides_on(doc) {
        for (i, g) in doc.model.guides.iter().enumerate() {
            let position = match drag {
                Some((k, p)) if k == i => p,
                _ => g.position,
            };
            let (a, b) = line_ends(doc, g, position, to_screen);
            let wide = drag.is_some_and(|d| d.0 == i) || doc.guide_selected == Some(i);
            let color = if snapping.contains(&Some(i)) {
                Color32::from_rgb(0, 255, 0)
            } else {
                guide_color(g.color)
            };
            painter.line_segment([a, b], Stroke::new(if wide { 3.0 } else { 1.0 }, color));
        }
    }
}

/// The grid (view-aids.md §6): lines every spacing/5 canvas px from the
/// canvas origin, over the canvas only; every 5th is a major line. Close up
/// (at least 1 screen px per canvas px) minor lines are 0.8 wide at alpha
/// × 0.25 and major lines 1.0 wide at × 0.5; zoomed out the minor lines
/// are hidden and the major lines are at × 0.3.
fn draw_grid(
    painter: &egui::Painter,
    doc: &Document,
    aids: &ViewAids,
    to_screen: &dyn Fn([f32; 2]) -> Pos2,
) {
    let (w, h) = (
        doc.model.canvas.width as f32,
        doc.model.canvas.height as f32,
    );
    let minor = aids.grid_spacing / 5.0;
    let px_per_canvas = (to_screen([1.0, 0.0]) - to_screen([0.0, 0.0])).length();
    let close = px_per_canvas >= 1.0;
    let [r, g, b] = aids.grid_color;
    let color = |a: f32| Color32::from_rgba_unmultiplied(r, g, b, (a * 255.0).round() as u8);
    let (minor_stroke, major_stroke) = if close {
        (Stroke::new(0.8, color(0.25)), Stroke::new(1.0, color(0.5)))
    } else {
        (Stroke::NONE, Stroke::new(1.0, color(0.3)))
    };
    for (len, other, vertical) in [(w, h, true), (h, w, false)] {
        let count = (len / minor).floor() as usize;
        if count > 20_000 {
            continue;
        }
        for k in 0..=count {
            let major = k % 5 == 0;
            if !major && !close {
                continue;
            }
            let t = k as f32 * minor;
            let (a, b) = if vertical {
                ([t, 0.0], [t, other])
            } else {
                ([0.0, t], [other, t])
            };
            painter.line_segment(
                [to_screen(a), to_screen(b)],
                if major { major_stroke } else { minor_stroke },
            );
        }
    }
}

/// A guide's colour: its stored RGBA, premultiplied by its own alpha
/// (view-aids.md §1; no extra factor).
fn guide_color(c: [f32; 4]) -> Color32 {
    let u = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(u(c[0]), u(c[1]), u(c[2]), u(c[3]))
}

/// Show ▸ Guide ▸ Add Guide to Center: a horizontal guide at the canvas's
/// centre y and a vertical one at its centre x.
pub fn center_guides(doc: &Document, color: [f32; 4]) -> Vec<Guide> {
    let (w, h) = (
        doc.model.canvas.width as f32,
        doc.model.canvas.height as f32,
    );
    let mut out = vec![Guide::new(false, h / 2.0), Guide::new(true, w / 2.0)];
    out.iter_mut().for_each(|g| g.color = color);
    out
}

/// Show ▸ Guide ▸ Add Guides at Coordinates of Selected Vertices: a
/// horizontal and a vertical guide through each selected point (displayed
/// positions), without duplicates.
pub fn vertex_guides(doc: &Document, color: [f32; 4]) -> Vec<Guide> {
    let mut out: Vec<Guide> = Vec::new();
    for &(o, k) in &doc.selection.points {
        let Some(p) = doc.object_points(o).get(k).copied() else {
            continue;
        };
        for (vertical, position) in [(false, p[1]), (true, p[0])] {
            let exists = |g: &Guide| g.vertical == vertical && (g.position - position).abs() < 1e-3;
            if !out.iter().any(exists) && !doc.model.guides.iter().any(exists) {
                let mut g = Guide::new(vertical, position);
                g.color = color;
                out.push(g);
            }
        }
    }
    out
}

/// State of the Grid settings and Guide Settings dialogs.
#[derive(Default)]
pub struct AidDialogs {
    /// Grid settings: the values being edited.
    pub grid: Option<ViewAids>,
    pub guides_open: bool,
    /// Guide Settings: the table being edited (committed when no field is
    /// being dragged or typed in).
    guides: Option<Vec<Guide>>,
    selected: Option<usize>,
    /// Features ▸ Edit All Guide Colors… / Setting ▸ Default Guide Line
    /// Color…: the colour being picked.
    all_colors: Option<[f32; 4]>,
    default_color: Option<[f32; 4]>,
}

/// Show ▸ Grid ▸ Grid settings….
pub fn grid_dialog(ctx: &egui::Context, st: &mut AidDialogs, aids: &mut ViewAids) {
    let Some(edit) = st.grid.as_mut() else {
        return;
    };
    let (mut ok, mut cancel) = (false, false);
    let mut open = true;
    egui::Window::new(crate::i18n::tr("Grid settings"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Grid::new("grid-settings")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label(crate::i18n::tr("Grid spacing (px)"));
                    ui.add(
                        egui::DragValue::new(&mut edit.grid_spacing)
                            .range(1.0..=10000.0)
                            .speed(1.0),
                    );
                    ui.end_row();
                    ui.label(crate::i18n::tr("Grid Color"));
                    ui.horizontal(|ui| {
                        ui.color_edit_button_srgb(&mut edit.grid_color);
                        let [r, g, b] = edit.grid_color;
                        ui.monospace(format!("#{r:02X}{g:02X}{b:02X}"));
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
        aids.grid_spacing = edit.grid_spacing;
        aids.grid_color = edit.grid_color;
    }
    if ok || cancel || !open {
        st.grid = None;
    }
}

/// Show ▸ Guide ▸ Guide Settings (Modeling View)….
pub fn guides_dialog(
    ctx: &egui::Context,
    st: &mut AidDialogs,
    aids: &mut ViewAids,
    doc: Option<&mut Document>,
) {
    if !st.guides_open {
        return;
    }
    let Some(doc) = doc.filter(|d| d.editable()) else {
        st.guides_open = false;
        st.guides = None;
        return;
    };
    let mut table = st.guides.take().unwrap_or_else(|| doc.model.guides.clone());
    if table.len() != doc.model.guides.len() && st.guides.is_none() {
        table = doc.model.guides.clone();
    }
    let mut open = true;
    let mut busy = false;
    let mut undo = None;
    egui::Window::new(crate::i18n::tr("Guide Settings"))
        .default_size([470.0, 420.0])
        .open(&mut open)
        .show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(crate::i18n::tr("Edit"), |ui| {
                    if ui
                        .add(egui::Button::new(crate::i18n::tr("Undo")).shortcut_text("Ctrl+Z"))
                        .clicked()
                    {
                        undo = Some(false);
                    }
                    if ui
                        .add(egui::Button::new(crate::i18n::tr("Redo")).shortcut_text("Ctrl+Y"))
                        .clicked()
                    {
                        undo = Some(true);
                    }
                });
                ui.menu_button(crate::i18n::tr("Features"), |ui| {
                    if ui
                        .button(crate::i18n::tr("Edit All Guide Colors…"))
                        .clicked()
                    {
                        st.all_colors =
                            Some(table.first().map_or(aids.default_guide_color, |g| g.color));
                        ui.close();
                    }
                });
                ui.menu_button(crate::i18n::tr("Setting"), |ui| {
                    if ui
                        .button(crate::i18n::tr("Default Guide Line Color…"))
                        .clicked()
                    {
                        st.default_color = Some(aids.default_guide_color);
                        ui.close();
                    }
                });
            });
            if let Some(mut c) = st.all_colors.take() {
                let mut keep = true;
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("All guides:"));
                    ui.color_edit_button_rgba_unmultiplied(&mut c);
                    if ui.button(crate::i18n::tr("OK")).clicked() {
                        table.iter_mut().for_each(|g| g.color = c);
                        keep = false;
                    } else if ui.button(crate::i18n::tr("Cancel")).clicked() {
                        keep = false;
                    }
                });
                if keep {
                    st.all_colors = Some(c);
                }
            }
            if let Some(mut c) = st.default_color.take() {
                let mut keep = true;
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("New guides:"));
                    ui.color_edit_button_rgba_unmultiplied(&mut c);
                    if ui.button(crate::i18n::tr("OK")).clicked() {
                        aids.default_guide_color = c;
                        keep = false;
                    } else if ui.button(crate::i18n::tr("Cancel")).clicked() {
                        keep = false;
                    }
                });
                if keep {
                    st.default_color = Some(c);
                }
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    egui::Grid::new("guide-table")
                        .num_columns(5)
                        .striped(true)
                        .show(ui, |ui| {
                            for h in ["No", "Type", "Position", "Color", "Memo"] {
                                ui.strong(crate::i18n::tr(h));
                            }
                            ui.end_row();
                            for (i, g) in table.iter_mut().enumerate() {
                                let sel = st.selected == Some(i);
                                if ui.selectable_label(sel, format!("{}", i + 1)).clicked() {
                                    st.selected = Some(i);
                                }
                                egui::ComboBox::from_id_salt(("guide-type", i))
                                    .selected_text(crate::i18n::tr(if g.vertical {
                                        "Vertical"
                                    } else {
                                        "Horizontal"
                                    }))
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut g.vertical,
                                            false,
                                            crate::i18n::tr("Horizontal"),
                                        );
                                        ui.selectable_value(
                                            &mut g.vertical,
                                            true,
                                            crate::i18n::tr("Vertical"),
                                        );
                                    });
                                let r = ui.add(
                                    egui::DragValue::new(&mut g.position)
                                        .speed(1.0)
                                        .max_decimals(1),
                                );
                                busy |= r.dragged() || r.has_focus();
                                let r = ui.color_edit_button_rgba_unmultiplied(&mut g.color);
                                busy |= r.dragged();
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut g.memo)
                                        .desired_width(140.0)
                                        .hint_text(crate::i18n::tr("Memo")),
                                );
                                busy |= r.has_focus();
                                ui.end_row();
                            }
                        });
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("New Guide")).clicked() {
                    let h = doc.model.canvas.height as f32;
                    let mut g = Guide::new(false, h / 2.0);
                    g.color = aids.default_guide_color;
                    table.push(g);
                    st.selected = Some(table.len() - 1);
                }
                if ui
                    .add_enabled(
                        st.selected.is_some_and(|i| i < table.len()),
                        egui::Button::new(crate::i18n::tr("Delete")),
                    )
                    .clicked()
                {
                    if let Some(i) = st.selected.take() {
                        table.remove(i);
                    }
                }
            });
        });
    doc.guide_selected = st.selected.filter(|&i| i < table.len());
    if let Some(redo) = undo {
        if redo {
            doc.redo();
        } else {
            doc.undo();
        }
        table = doc.model.guides.clone();
    } else if !busy && table != doc.model.guides {
        let t = table.clone();
        if let Err(e) = doc.try_edit("Edit Guide", |m| {
            m.guides = t;
            Ok(())
        }) {
            doc.notice = Some(e);
        }
        aids.show_guides = true;
    }
    if open {
        st.guides = Some(table);
    } else {
        st.guides_open = false;
        st.guides = None;
        doc.guide_selected = None;
    }
}

/// Show ▸ Grid and Show ▸ Guide submenus.
pub fn show_menu(
    ui: &mut egui::Ui,
    aids: &mut ViewAids,
    st: &mut AidDialogs,
    doc: Option<&mut Document>,
) {
    ui.menu_button(crate::i18n::tr("Grid"), |ui| {
        ui.checkbox(
            &mut aids.show_grid,
            crate::i18n::tr("Show Grid (Modeling View)"),
        );
        ui.separator();
        if ui.button(crate::i18n::tr("Grid settings…")).clicked() {
            st.grid = Some(*aids);
            ui.close();
        }
    });
    let modeling = doc
        .as_ref()
        .is_some_and(|d| d.editable() && d.mode == Mode::Modeling);
    ui.menu_button(crate::i18n::tr("Guide"), |ui| {
        ui.checkbox(
            &mut aids.show_guides,
            crate::i18n::tr("Show Guide (Modeling View)"),
        );
        // No animation-view guides exist (view-aids.md §1): shown disabled.
        let mut off = false;
        ui.add_enabled(
            false,
            egui::Checkbox::new(&mut off, crate::i18n::tr("Show Guide (Animation view)")),
        );
        ui.separator();
        let mut add = None;
        if ui
            .add_enabled(
                modeling,
                egui::Button::new(crate::i18n::tr("Add Guide to Center")),
            )
            .clicked()
        {
            add = doc
                .as_ref()
                .map(|d| center_guides(d, aids.default_guide_color));
        }
        let has_points = doc.as_ref().is_some_and(|d| !d.selection.points.is_empty());
        if ui
            .add_enabled(
                modeling && has_points,
                egui::Button::new(crate::i18n::tr(
                    "Add Guides at Coordinates of Selected Vertices",
                )),
            )
            .on_disabled_hover_text(crate::i18n::tr("Select vertices first"))
            .clicked()
        {
            add = doc
                .as_ref()
                .map(|d| vertex_guides(d, aids.default_guide_color));
        }
        ui.separator();
        if ui
            .add_enabled(
                modeling,
                egui::Button::new(crate::i18n::tr("Guide Settings (Modeling View)…")),
            )
            .clicked()
        {
            st.guides_open = true;
            st.guides = None;
            ui.close();
        }
        ui.separator();
        ui.checkbox(
            &mut aids.snap_guides,
            crate::i18n::tr("Snap Moving Vertices to Guide"),
        );
        if let (Some(new), Some(d)) = (add, doc) {
            if !new.is_empty() {
                if let Err(e) = d.try_edit("Add Guide", |m| {
                    m.guides.extend(new);
                    Ok(())
                }) {
                    d.notice = Some(e);
                }
            }
            aids.show_guides = true;
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_within_two_screen_pixels() {
        let guides = vec![Guide::new(true, 100.0), Guide::new(false, 50.0)];
        // Zoom 2: s = 1 canvas unit.
        let s = snap_distance(2.0);
        assert_eq!(s, 1.0);
        let (p, hit) = snap(&guides, [100.8, 49.5], s);
        assert_eq!(p, [100.0, 50.0]);
        assert_eq!(hit, [Some(0), Some(1)]);
        let (p, hit) = snap(&guides, [101.5, 49.5], s);
        assert_eq!(p, [101.5, 50.0], "x is 1.5 units away: only y snaps");
        assert_eq!(hit, [None, Some(1)]);
        let pts = [[10.0, 10.0], [10.5, 11.5]];
        assert_eq!(snap_to_points(&pts, [10.4, 10.9], 1.0), Some([10.5, 11.5]));
        assert_eq!(snap_to_points(&pts, [12.5, 10.0], 1.0), None);
    }
}
