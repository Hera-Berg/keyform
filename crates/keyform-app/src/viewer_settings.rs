//! Model Settings palette: the viewer's runtime settings — motions,
//! expressions, pose, hit areas, eye-blink / lip-sync groups, physics.
//! spec: 70-advanced/viewer.md §1–2

use eframe::egui::{self, RichText};
use keyform_anim::expression::{stored_from_target, target_from_stored};
use keyform_anim::{ExpBlend, ExpParam, Expression, Pose, PoseNode};
use keyform_formats::runtime_json::MotionRef;

use crate::document::{load_motion, relative_to, Document, ExpressionEntry, MotionEntry};
use crate::icons::{self, Icon};
use crate::log::Log;

/// Palette-local UI state.
#[derive(Default)]
pub struct SettingsState {
    pub new_group: String,
    pub new_expression: String,
    pub selected_expression: Option<usize>,
    pub add_param: usize,
}

fn optional_seconds(ui: &mut egui::Ui, label: &str, v: &mut Option<f32>) -> bool {
    let mut on = v.is_some();
    let mut changed = ui.checkbox(&mut on, label).changed();
    if on != v.is_some() {
        *v = on.then_some(0.5);
    }
    if let Some(x) = v {
        changed |= ui
            .add(
                egui::DragValue::new(x)
                    .range(0.0..=10.0)
                    .speed(0.01)
                    .suffix(" s")
                    .max_decimals(3),
            )
            .changed();
    }
    changed
}

pub fn settings_ui(ui: &mut egui::Ui, doc: &mut Document, st: &mut SettingsState, log: &mut Log) {
    let runtime = doc.can_save_settings();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if runtime {
            ui.horizontal(|ui| {
                if ui.button("Save model3.json").on_hover_text("Ctrl+S").clicked() {
                    save(doc, log);
                }
            });
        } else {
            ui.weak("Viewer settings are edited on runtime models (.model3.json). Playback works for projects too.");
        }
        motions_ui(ui, doc, st, log, runtime);
        expressions_ui(ui, doc, st, runtime);
        if runtime {
            pose_ui(ui, doc);
            hit_areas_ui(ui, doc);
            groups_ui(ui, doc);
        }
        physics_ui(ui, doc);
    });
}

pub fn save(doc: &mut Document, log: &mut Log) {
    match doc.save_settings() {
        Ok(files) => {
            for f in files {
                log.info(format!("saved {}", f.display()));
            }
        }
        Err(e) => log.error(e),
    }
}

fn motions_ui(
    ui: &mut egui::Ui,
    doc: &mut Document,
    st: &mut SettingsState,
    log: &mut Log,
    editable: bool,
) {
    egui::CollapsingHeader::new(format!("Motions ({})", doc.motions.len()))
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut doc.looping, "Loop");
                if icons::icon_button(ui, Icon::Stop, "Stop motion").clicked() {
                    doc.motion_player.stop();
                }
            });
            let mut play = None;
            let mut remove = None;
            let mut last_group: Option<String> = None;
            for (i, m) in doc.motions.iter_mut().enumerate() {
                if last_group.as_deref() != Some(m.group.as_str()) {
                    ui.label(RichText::new(&m.group).strong());
                    last_group = Some(m.group.clone());
                }
                ui.horizontal(|ui| {
                    if icons::icon_button(ui, Icon::Play, "Play motion").clicked() {
                        play = Some(i);
                    }
                    let label = match &m.motion {
                        Some(mo) => ui.label(&m.reference.file).on_hover_text(format!(
                            "{:.2} s, {} curves, {} fps",
                            mo.duration,
                            mo.curves.len(),
                            mo.fps
                        )),
                        None => ui
                            .label(
                                RichText::new(&m.reference.file).color(ui.visuals().error_fg_color),
                            )
                            .on_hover_text("could not be loaded"),
                    };
                    label.context_menu(|ui| {
                        if editable && ui.button("Remove from model3.json").clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                    });
                });
                if editable {
                    ui.indent(("motion", i), |ui| {
                        ui.horizontal(|ui| {
                            optional_seconds(ui, "Fade in", &mut m.reference.fade_in);
                            optional_seconds(ui, "Fade out", &mut m.reference.fade_out);
                        });
                    });
                }
            }
            if let Some(i) = play {
                doc.play_motion(i);
            }
            if let Some(i) = remove {
                doc.motions.remove(i);
            }
            if editable {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut st.new_group)
                            .hint_text("Group (e.g. Idle)")
                            .desired_width(110.0),
                    );
                    if ui.button("Add motions…").clicked() {
                        add_motions(doc, st, log);
                    }
                });
            }
            if doc.motions.is_empty() && !editable {
                ui.weak("No motions.");
            }
        });
}

fn add_motions(doc: &mut Document, st: &mut SettingsState, log: &mut Log) {
    let Some(dir) = doc.path.parent().map(std::path::Path::to_path_buf) else {
        return;
    };
    let files = rfd::FileDialog::new()
        .add_filter("Motion (*.motion3.json)", &["json"])
        .set_directory(&dir)
        .pick_files();
    let group = if st.new_group.trim().is_empty() {
        "Idle".to_owned()
    } else {
        st.new_group.trim().to_owned()
    };
    for f in files.unwrap_or_default() {
        let Some(rel) = relative_to(&dir, &f) else {
            log.warn(format!(
                "{} is outside the model folder; copy it next to the model3.json first",
                f.display()
            ));
            continue;
        };
        let motion = load_motion(&dir, &rel, log);
        doc.motions.push(MotionEntry {
            group: group.clone(),
            reference: MotionRef {
                file: rel,
                ..MotionRef::default()
            },
            motion,
        });
    }
    // Keep groups together.
    let order: Vec<String> =
        doc.motions
            .iter()
            .map(|m| m.group.clone())
            .fold(Vec::new(), |mut v, g| {
                if !v.contains(&g) {
                    v.push(g);
                }
                v
            });
    doc.motions
        .sort_by_key(|m| order.iter().position(|g| *g == m.group));
}

fn expressions_ui(ui: &mut egui::Ui, doc: &mut Document, st: &mut SettingsState, editable: bool) {
    egui::CollapsingHeader::new(format!("Expressions ({})", doc.expressions.len()))
        .default_open(true)
        .show(ui, |ui| {
            let mut apply = None;
            for (i, e) in doc.expressions.iter().enumerate() {
                ui.horizontal(|ui| {
                    if icons::icon_button(ui, Icon::Play, "Apply expression").clicked() {
                        apply = Some(i);
                    }
                    let mut text = RichText::new(&e.name);
                    if e.dirty {
                        text = text.italics();
                    }
                    if ui
                        .selectable_label(st.selected_expression == Some(i), text)
                        .on_hover_text(&e.file)
                        .clicked()
                    {
                        st.selected_expression = if st.selected_expression == Some(i) {
                            None
                        } else {
                            Some(i)
                        };
                    }
                });
            }
            if let Some(i) = apply {
                doc.play_expression(i);
            }
            if !doc.expressions.is_empty() && ui.button("Reset expression").clicked() {
                let now = doc.time;
                doc.expression_player.stop(now);
            }
            if editable {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut st.new_expression)
                            .hint_text("Name")
                            .desired_width(110.0),
                    );
                    let name = st.new_expression.trim().to_owned();
                    if ui
                        .add_enabled(
                            !name.is_empty(),
                            egui::Button::new("New from current values"),
                        )
                        .clicked()
                    {
                        new_expression(doc, &name);
                        st.selected_expression = Some(doc.expressions.len() - 1);
                        st.new_expression.clear();
                    }
                });
            }
            if let (true, Some(i)) = (editable, st.selected_expression) {
                if i < doc.expressions.len() {
                    expression_editor(ui, doc, i, st);
                }
            }
        });
}

/// An expression holding every parameter whose current value differs from
/// its default, as Add values (viewer.md §1.1).
fn new_expression(doc: &mut Document, name: &str) {
    let params = doc
        .model
        .parameters
        .iter()
        .enumerate()
        .filter(|(i, p)| (doc.live[*i] - p.default).abs() > 1e-4)
        .map(|(i, p)| ExpParam {
            id: p.id.clone(),
            value: stored_from_target(ExpBlend::Add, doc.live[i], p.default),
            blend: ExpBlend::Add,
        })
        .collect();
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    doc.expressions.push(ExpressionEntry {
        name: format!("{name}.exp3.json"),
        file: format!("expressions/{safe}.exp3.json"),
        expression: Expression {
            fade_in: None,
            fade_out: None,
            params,
        },
        dirty: true,
        broken: false,
    });
}

fn expression_editor(ui: &mut egui::Ui, doc: &mut Document, i: usize, st: &mut SettingsState) {
    let params = doc.model.parameters.clone();
    let e = &mut doc.expressions[i];
    if e.broken {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "This file could not be loaded and is not edited.",
        );
        return;
    }
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Name");
            changed |= ui.text_edit_singleline(&mut e.name).changed();
        });
        ui.horizontal(|ui| {
            changed |= optional_seconds(ui, "Fade in", &mut e.expression.fade_in);
            changed |= optional_seconds(ui, "Fade out", &mut e.expression.fade_out);
        });
        let mut remove = None;
        for (pi, p) in e.expression.params.iter_mut().enumerate() {
            let default = params
                .iter()
                .find(|q| q.id == p.id)
                .map_or(0.0, |q| q.default);
            let range = params
                .iter()
                .find(|q| q.id == p.id)
                .map_or(-1.0..=1.0, |q| q.min.min(q.max)..=q.min.max(q.max));
            ui.horizontal(|ui| {
                ui.label(&p.id);
                let mut blend = p.blend;
                egui::ComboBox::from_id_salt(("blend", i, pi))
                    .width(80.0)
                    .selected_text(blend.name())
                    .show_ui(ui, |ui| {
                        for b in [ExpBlend::Add, ExpBlend::Multiply, ExpBlend::Overwrite] {
                            ui.selectable_value(&mut blend, b, b.name());
                        }
                    });
                // The editor shows target values; switching blend keeps the target.
                let mut target = target_from_stored(p.blend, p.value, default);
                if blend != p.blend {
                    p.value = stored_from_target(blend, target, default);
                    p.blend = blend;
                    changed = true;
                }
                if ui
                    .add(
                        egui::DragValue::new(&mut target)
                            .range(range)
                            .speed(0.01)
                            .max_decimals(3),
                    )
                    .on_hover_text("Target value")
                    .changed()
                {
                    p.value = stored_from_target(p.blend, target, default);
                    changed = true;
                }
                if icons::icon_button(ui, Icon::Stop, "Remove parameter").clicked() {
                    remove = Some(pi);
                }
            });
        }
        if let Some(pi) = remove {
            e.expression.params.remove(pi);
            changed = true;
        }
        ui.horizontal(|ui| {
            let ids: Vec<&str> = params.iter().map(|p| p.id.as_str()).collect();
            st.add_param = st.add_param.min(ids.len().saturating_sub(1));
            egui::ComboBox::from_id_salt(("addparam", i))
                .width(140.0)
                .selected_text(ids.get(st.add_param).copied().unwrap_or(""))
                .show_ui(ui, |ui| {
                    for (k, id) in ids.iter().enumerate() {
                        ui.selectable_value(&mut st.add_param, k, *id);
                    }
                });
            if ui.button("Add parameter").clicked() {
                if let Some(p) = params.get(st.add_param) {
                    if !e.expression.params.iter().any(|x| x.id == p.id) {
                        e.expression.params.push(ExpParam {
                            id: p.id.clone(),
                            value: 0.0,
                            blend: ExpBlend::Add,
                        });
                        changed = true;
                    }
                }
            }
        });
        if changed {
            e.dirty = true;
        }
    });
}

fn pose_ui(ui: &mut egui::Ui, doc: &mut Document) {
    egui::CollapsingHeader::new("Pose").show(ui, |ui| {
        let part_ids: Vec<String> = doc
            .model
            .parts
            .iter()
            .map(|p| p.info.id.clone())
            .filter(|id| id != "__RootPart__")
            .collect();
        let pose = doc.pose.get_or_insert_with(Pose::default);
        let mut remove_group = None;
        for (gi, g) in pose.groups.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("Group {}", gi + 1));
                if icons::icon_button(ui, Icon::Stop, "Remove group").clicked() {
                    remove_group = Some(gi);
                }
            });
            ui.indent(("pose", gi), |ui| {
                let mut remove = None;
                for (ni, n) in g.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(&n.id);
                        if !n.links.is_empty() {
                            ui.weak(format!("→ {}", n.links.join(", ")));
                        }
                        if icons::icon_button(ui, Icon::Stop, "Remove part").clicked() {
                            remove = Some(ni);
                        }
                    });
                }
                if let Some(ni) = remove {
                    g.remove(ni);
                }
                egui::ComboBox::from_id_salt(("poseadd", gi))
                    .selected_text("Add part…")
                    .show_ui(ui, |ui| {
                        for id in &part_ids {
                            if ui.selectable_label(false, id).clicked()
                                && !g.iter().any(|n| &n.id == id)
                            {
                                g.push(PoseNode {
                                    id: id.clone(),
                                    links: Vec::new(),
                                });
                            }
                        }
                    });
            });
        }
        if let Some(gi) = remove_group {
            pose.groups.remove(gi);
        }
        if ui.button("Add group").clicked() {
            pose.groups.push(Vec::new());
        }
        let mut fade = pose.fade_in;
        ui.horizontal(|ui| {
            optional_seconds(ui, "Fade in", &mut fade);
        });
        pose.fade_in = fade;
    });
    if doc
        .pose
        .as_ref()
        .is_some_and(|p| p.groups.is_empty() && p.fade_in.is_none())
    {
        doc.pose = None;
    }
}

fn hit_areas_ui(ui: &mut egui::Ui, doc: &mut Document) {
    egui::CollapsingHeader::new(format!("Hit areas ({})", doc.hit_areas.len())).show(ui, |ui| {
        let mut remove = None;
        for (i, (id, name)) in doc.hit_areas.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(name)
                        .desired_width(90.0)
                        .hint_text("Name"),
                );
                ui.label(id.as_str());
                if icons::icon_button(ui, Icon::Stop, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            doc.hit_areas.remove(i);
        }
        // Art meshes whose IDs start with "HitArea" are pre-filled (viewer.md §1.3).
        if ui.button("Add HitArea meshes").clicked() {
            for a in &doc.model.art_meshes {
                if a.info.id.starts_with("HitArea")
                    && !doc.hit_areas.iter().any(|(i, _)| *i == a.info.id)
                {
                    doc.hit_areas.push((a.info.id.clone(), String::new()));
                }
            }
        }
    });
}

fn groups_ui(ui: &mut egui::Ui, doc: &mut Document) {
    egui::CollapsingHeader::new("Eye blinking and lip-sync").show(ui, |ui| {
        egui::Grid::new("effects").striped(true).show(ui, |ui| {
            ui.strong("Parameter");
            ui.strong("Eye blink");
            ui.strong("Lip-sync");
            ui.end_row();
            for (i, p) in doc.model.parameters.iter().enumerate() {
                ui.label(&p.name).on_hover_text(&p.id);
                for list in [
                    &mut doc.model.eye_blink_parameters,
                    &mut doc.model.lip_sync_parameters,
                ] {
                    let mut on = list.contains(&i);
                    if ui
                        .add_enabled(!p.repeat, egui::Checkbox::without_text(&mut on))
                        .changed()
                    {
                        if on {
                            list.push(i);
                        } else {
                            list.retain(|&x| x != i);
                        }
                    }
                }
                ui.end_row();
            }
        });
    });
}

fn physics_ui(ui: &mut egui::Ui, doc: &mut Document) {
    egui::CollapsingHeader::new(format!("Physics ({})", doc.model.physics.settings.len()))
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut doc.physics_enabled, "Enable physics");
                if ui.button("Reset").clicked() {
                    doc.sim.reset(&doc.model.physics);
                }
            });
            for s in &mut doc.model.physics.settings {
                ui.checkbox(
                    &mut s.enabled,
                    format!(
                        "{}  ({} in, {} out, {} pendulums)",
                        s.name,
                        s.inputs.len(),
                        s.outputs.len(),
                        s.vertices.len()
                    ),
                );
            }
        });
}
