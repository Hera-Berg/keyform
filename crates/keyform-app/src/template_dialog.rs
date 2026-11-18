//! Apply Model Template dialog: choose a template .cmo3, lay it out,
//! match art meshes (automatic + manual), and apply.
//! spec: 70-advanced/templates.md §1–§5

use eframe::egui::{self, RichText};
use keyform_edit::template::{self as tp, ApplyOptions, DrawOrderMode, Layout};
use keyform_model::Model;

use crate::document::Document;
use crate::log::Log;

#[derive(Default)]
pub struct TemplateDialog {
    pub open: bool,
    template: Option<(String, Model)>,
    layout: Layout,
    names: bool,
    /// Per destination mesh: template mesh, or `None`.
    mapping: Vec<Option<usize>>,
    options: ApplyOptions,
    message: Option<String>,
    noted: bool,
}

impl TemplateDialog {
    pub fn show(&mut self) {
        self.open = true;
        self.names = true;
    }
}

const NOTE: &str = "Using someone else's model as a template without permission may lead to \
copyright, moral rights or other infringements. Use only models you are allowed to use.";

pub fn dialog(
    ctx: &egui::Context,
    st: &mut TemplateDialog,
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
    let mut open = true;
    egui::Window::new("Apply Model Template")
        .default_size([640.0, 560.0])
        .open(&mut open)
        .show(ctx, |ui| {
            if !st.noted {
                ui.label(NOTE);
                if ui.button("OK").clicked() {
                    st.noted = true;
                }
                return;
            }
            ui.horizontal(|ui| {
                if ui.button("Select Model Template…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Model (*.cmo3)", &["cmo3"]).pick_file() {
                        match std::fs::read(&p)
                            .map_err(|e| e.to_string())
                            .and_then(|b| keyform_formats::cmo3::Cmo3Document::read(&b).map_err(|e| e.to_string()))
                        {
                            Ok(d) => {
                                if d.model.target_version > doc.model.target_version && doc.model.target_version != 0 {
                                    st.message = Some("The template targets a newer version than the model.".into());
                                }
                                st.template = Some((p.display().to_string(), d.model));
                                st.mapping.clear();
                            }
                            Err(e) => log.warn(format!("{}: {e}", p.display())),
                        }
                    }
                }
                if let Some((name, _)) = &st.template {
                    ui.label(name);
                }
            });
            let Some((_, tpl)) = &st.template else { return };
            ui.separator();
            ui.label(RichText::new("Layout").strong());
            ui.horizontal(|ui| {
                ui.label("Scale");
                ui.add(egui::DragValue::new(&mut st.layout.scale).speed(0.005).range(0.01..=100.0));
                ui.label("Offset X");
                ui.add(egui::DragValue::new(&mut st.layout.offset[0]).speed(1.0));
                ui.label("Y");
                ui.add(egui::DragValue::new(&mut st.layout.offset[1]).speed(1.0));
                if ui.button("Center").clicked() {
                    // Centre the template canvas on the model canvas.
                    let (tw, th) = (tpl.canvas.width as f32, tpl.canvas.height as f32);
                    let (dw, dh) = (doc.model.canvas.width as f32, doc.model.canvas.height as f32);
                    st.layout.offset = [(dw - tw * st.layout.scale) / 2.0, (dh - th * st.layout.scale) / 2.0];
                }
            });
            ui.separator();
            ui.label(RichText::new("Mapping").strong());
            ui.horizontal(|ui| {
                ui.radio_value(&mut st.names, true, "Prioritize duplicate names");
                ui.radio_value(&mut st.names, false, "Adjust to position and shape");
                if ui.button("Automatic mapping").clicked() || st.mapping.len() != doc.model.art_meshes.len() {
                    let (w, h) = (doc.model.canvas.width as f32, doc.model.canvas.height as f32);
                    st.mapping = tp::auto_match(&tp::mesh_infos(&doc.model), &tp::mesh_infos(tpl), &st.layout, w.hypot(h), st.names, &[]);
                }
            });
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                egui::Grid::new("tpl-map").num_columns(2).striped(true).show(ui, |ui| {
                    ui.label(RichText::new("Target").strong());
                    ui.label(RichText::new("Template").strong());
                    ui.end_row();
                    for (i, a) in doc.model.art_meshes.iter().enumerate() {
                        ui.label(a.info.display_name());
                        let cur = st.mapping.get(i).copied().flatten();
                        let text = cur.and_then(|t| tpl.art_meshes.get(t)).map_or("—".to_owned(), |t| t.info.display_name().to_owned());
                        egui::ComboBox::from_id_salt(("tpl-pair", i)).selected_text(text).height(300.0).show_ui(ui, |ui| {
                            if ui.selectable_label(cur.is_none(), "—").clicked() {
                                st.mapping[i] = None;
                            }
                            for (t, m) in tpl.art_meshes.iter().enumerate() {
                                if ui.selectable_label(cur == Some(t), m.info.display_name()).clicked() {
                                    st.mapping[i] = Some(t);
                                }
                            }
                        });
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            ui.label(RichText::new("Apply settings").strong());
            ui.horizontal(|ui| {
                ui.label("Draw order:");
                ui.radio_value(&mut st.options.draw_order, DrawOrderMode::AdjustToTemplate, "Adjust to template");
                ui.radio_value(&mut st.options.draw_order, DrawOrderMode::DontChange, "Don't change");
            });
            ui.horizontal(|ui| {
                ui.label("Also apply:");
                ui.checkbox(&mut st.options.mask, "Mask");
                ui.checkbox(&mut st.options.culling, "Culling");
                ui.checkbox(&mut st.options.color_blend, "Color Blend");
                ui.checkbox(&mut st.options.alpha_blend, "Alpha Blend");
            });
            if ui.button("Apply").clicked() {
                let pairs: Vec<(usize, usize)> =
                    st.mapping.iter().enumerate().filter_map(|(d, t)| t.map(|t| (d, t))).collect();
                let (layout, options) = (st.layout, st.options);
                let tpl = tpl.clone();
                let mut result = None;
                doc.edit("Apply Model Template", |m| {
                    result = Some(tp::apply(m, &tpl, &pairs, &layout, &options));
                });
                st.message = Some(match result {
                    Some(Ok(r)) => format!(
                        "Applied: {} art meshes, {} deformers, {} parts, {} parameters added ({} reused), {} physics groups.",
                        r.meshes, r.deformers, r.parts, r.parameters_added, r.parameters_reused, r.physics
                    ),
                    Some(Err(e)) => e,
                    None => "The model cannot be edited.".into(),
                });
            }
            if let Some(m) = &st.message {
                ui.label(m);
            }
        });
    if !open {
        st.open = false;
    }
}
