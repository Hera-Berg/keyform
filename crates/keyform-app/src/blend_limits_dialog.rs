//! Limit Settings for Blend Shape Weights: for the selected objects, the
//! weight-limit graphs of a blend-shape parameter over other parameters.
//! Layout follows the reference (observed, Q-0037), in Keyform's words.

use eframe::egui::{self, Color32, RichText};
use keyform_edit::blend_limits as bl;
use keyform_model::ParameterKind;

use crate::document::{Document, Selection};
use crate::i18n::tr;

#[derive(Default)]
pub struct BlendLimitsDialog {
    pub open: bool,
    blend: Option<usize>,
    add_param: Option<usize>,
    selected: Option<usize>,
    /// Edit Graph table being edited: rows (value, weight %).
    editing: Option<Vec<(f32, f32)>>,
    message: Option<String>,
}

impl BlendLimitsDialog {
    pub fn show(&mut self) {
        self.open = true;
    }
}

/// Selected objects that use blend-shape parameter `b`.
fn targets(doc: &Document, b: usize) -> Vec<Selection> {
    doc.selection
        .objects
        .iter()
        .copied()
        .filter(|&o| !keyform_edit::keys::blend_keys_of(&doc.model, o, b).is_empty())
        .collect()
}

pub fn dialog(ctx: &egui::Context, st: &mut BlendLimitsDialog, doc: Option<&mut Document>) {
    if !st.open {
        return;
    }
    let Some(doc) = doc else {
        st.open = false;
        return;
    };
    let blends: Vec<usize> = doc
        .model
        .parameters
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind == ParameterKind::BlendShape)
        .map(|(i, _)| i)
        .collect();
    if st.blend.is_none_or(|b| !blends.contains(&b)) {
        st.blend = blends.first().copied();
    }
    let mut open = true;
    let mut apply: Option<(usize, Option<Vec<(f32, f32)>>)> = None;
    egui::Window::new(tr("Limit Settings for Blend Shape Weights"))
        .open(&mut open)
        .default_width(600.0)
        .show(ctx, |ui| {
            let Some(b) = st.blend else {
                ui.label(tr("The model has no blend-shape parameters."));
                return;
            };
            let ts = targets(doc, b);
            let first = ts.first().copied();
            let qs = first.map_or(Vec::new(), |o| bl::constraint_params(&doc.model, o, b));
            ui.horizontal(|ui| {
                let name = |i: usize| doc.model.parameters[i].name.clone();
                egui::ComboBox::from_id_salt("bl-blend")
                    .selected_text(format!("{} ({})", name(b), qs.len()))
                    .show_ui(ui, |ui| {
                        for &x in &blends {
                            ui.selectable_value(&mut st.blend, Some(x), name(x));
                        }
                    });
                ui.label(format!(
                    "{} {}",
                    tr("Number of Target Objects to be Edited:"),
                    ts.len()
                ));
            });
            if ts.is_empty() {
                ui.label(tr("Select objects that use this blend shape."));
                return;
            }
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.label(RichText::new(tr("Parameter")).strong());
                ui.horizontal(|ui| {
                    let text = st
                        .add_param
                        .map_or(String::new(), |p| doc.model.parameters[p].name.clone());
                    egui::ComboBox::from_id_salt("bl-add")
                        .selected_text(text)
                        .height(300.0)
                        .show_ui(ui, |ui| {
                            for (i, p) in doc.model.parameters.iter().enumerate() {
                                if p.kind == ParameterKind::BlendShape || qs.contains(&i) {
                                    continue;
                                }
                                ui.selectable_value(&mut st.add_param, Some(i), &p.name);
                            }
                        });
                    if ui.button(tr("Add")).clicked() {
                        if let Some(q) = st.add_param.take() {
                            let p = &doc.model.parameters[q];
                            apply = Some((q, Some(bl::preset_points(0, p.min, p.max))));
                            st.selected = Some(q);
                        }
                    }
                });
                for &q in &qs {
                    if ui
                        .selectable_label(st.selected == Some(q), &doc.model.parameters[q].name)
                        .clicked()
                    {
                        st.selected = Some(q);
                    }
                }
                if ui.button(tr("Delete")).clicked() {
                    if let Some(q) = st.selected.take() {
                        apply = Some((q, None));
                    }
                }
                let ui = &mut cols[1];
                ui.label(RichText::new(tr("Weight Limit Graph")).strong());
                let Some(q) = st.selected.filter(|q| qs.contains(q)) else {
                    return;
                };
                let (min, max) = (doc.model.parameters[q].min, doc.model.parameters[q].max);
                ui.horizontal(|ui| {
                    ui.label(tr("Preset"));
                    egui::ComboBox::from_id_salt("bl-preset")
                        .selected_text("—")
                        .show_ui(ui, |ui| {
                            for (i, n) in bl::PRESETS.iter().enumerate() {
                                if ui.selectable_label(false, tr(n)).clicked() {
                                    apply = Some((q, Some(bl::preset_points(i, min, max))));
                                }
                            }
                        });
                });
                let pts = first.map_or(Vec::new(), |o| bl::points(&doc.model, o, b, q));
                graph(ui, &pts, min, max, &doc.model.parameters[q].name);
                if ui.button(tr("Edit Graph…")).clicked() {
                    st.editing = Some(pts.iter().map(|p| (p.0, p.1 * 100.0)).collect());
                }
            });
            if let Some(m) = &st.message {
                ui.colored_label(ui.visuals().warn_fg_color, m);
            }
        });
    if !open {
        st.open = false;
    }
    if let (Some(b), Some(q)) = (st.blend, st.selected) {
        edit_graph(ctx, st, doc, b, q, &mut apply);
    }
    if let (Some(b), Some((q, pts))) = (st.blend, apply) {
        let ts = targets(doc, b);
        doc.edit("Limit Settings for Blend Shape Weights", |m| {
            for o in ts {
                bl::set_points(m, o, b, q, pts.as_deref());
            }
        });
    }
}

fn graph(ui: &mut egui::Ui, pts: &[(f32, f32)], min: f32, max: f32, name: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(260.0, 200.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let plot = rect.shrink2(egui::vec2(30.0, 20.0));
    let grid = ui.visuals().weak_text_color().gamma_multiply(0.4);
    for i in 0..=6 {
        let x = plot.left() + plot.width() * i as f32 / 6.0;
        p.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            (1.0, grid),
        );
        let y = plot.top() + plot.height() * i as f32 / 6.0;
        p.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            (1.0, grid),
        );
    }
    let font = egui::FontId::proportional(10.0);
    p.text(
        egui::pos2(plot.left() - 4.0, plot.top()),
        egui::Align2::RIGHT_CENTER,
        "100%",
        font.clone(),
        grid,
    );
    p.text(
        egui::pos2(plot.left() - 4.0, plot.bottom()),
        egui::Align2::RIGHT_CENTER,
        "0%",
        font.clone(),
        grid,
    );
    p.text(
        egui::pos2(plot.center().x, rect.bottom() - 2.0),
        egui::Align2::CENTER_BOTTOM,
        name,
        font.clone(),
        ui.visuals().text_color(),
    );
    let at = |v: f32, w: f32| {
        let t = if max > min {
            (v - min) / (max - min)
        } else {
            0.0
        };
        egui::pos2(
            plot.left() + t * plot.width(),
            plot.bottom() - w * plot.height(),
        )
    };
    let green = Color32::from_rgb(20, 170, 110);
    for w in pts.windows(2) {
        p.line_segment([at(w[0].0, w[0].1), at(w[1].0, w[1].1)], (2.0, green));
    }
    for &(v, w) in pts {
        p.circle_filled(at(v, w), 4.0, green);
        p.text(
            at(v, w) + egui::vec2(0.0, -6.0),
            egui::Align2::CENTER_BOTTOM,
            format!("({v}, {:.0}%)", w * 100.0),
            font.clone(),
            green,
        );
    }
}

/// The Edit Graph table: (parameter value, weight limit %) rows.
fn edit_graph(
    ctx: &egui::Context,
    st: &mut BlendLimitsDialog,
    doc: &Document,
    _b: usize,
    q: usize,
    apply: &mut Option<(usize, Option<Vec<(f32, f32)>>)>,
) {
    let Some(rows) = &mut st.editing else { return };
    let p = &doc.model.parameters[q];
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new(format!("{} - {}", tr("Edit Graph"), p.name)).collapsible(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button(tr("Add Row")).clicked() {
                rows.push((p.default, 100.0));
            }
            if ui.button(tr("Delete Row")).clicked() {
                rows.pop();
            }
        });
        egui::Grid::new("bl-rows").num_columns(2).striped(true).show(ui, |ui| {
            ui.label(RichText::new(tr("Parameter Value")).strong());
            ui.label(RichText::new(tr("Weight Limit (%)")).strong());
            ui.end_row();
            for r in rows.iter_mut() {
                ui.add(egui::DragValue::new(&mut r.0).speed(0.1));
                ui.add(egui::DragValue::new(&mut r.1).speed(1.0));
                ui.end_row();
            }
        });
        ui.label(RichText::new(tr("Rules: 5 rows or fewer; no duplicate values; values within the parameter range; weights 0–100 %.")).weak());
        ui.horizontal(|ui| {
            ok = ui.button(tr("OK")).clicked();
            cancel = ui.button(tr("Cancel")).clicked();
        });
    });
    if ok {
        let pts: Vec<(f32, f32)> = rows.iter().map(|r| (r.0, r.1 / 100.0)).collect();
        match bl::validate(&pts, p.min, p.max) {
            Ok(()) => {
                *apply = Some((q, Some(pts)));
                st.editing = None;
                st.message = None;
            }
            Err(e) => st.message = Some(tr(e).to_owned()),
        }
    } else if cancel {
        st.editing = None;
    }
}
