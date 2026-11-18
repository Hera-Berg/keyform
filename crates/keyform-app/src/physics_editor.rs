//! Physics Settings window: groups, inputs, outputs, pendulums, FPS and
//! physics3.json import / export. The main canvas is the preview (with
//! "Apply Physics" on).
//! spec: 70-advanced/physics-editor.md

use eframe::egui::{self, Color32, RichText};
use keyform_model::{PhysicsKind, PhysicsOutput, PhysicsRig};
use keyform_physics::edit::{self as pe, Adjust, Caution};

use crate::document::Document;
use crate::log::Log;

/// Undo-step name of every edit in this window.
// TODO(Q-0038): the reference gives this window its own undo history.
const STEP: &str = "Physics Settings";

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Input,
    Output,
}

#[derive(Default)]
pub struct PhysicsEditor {
    pub open: bool,
    group: usize,
    tab: Tab,
    /// Edits not yet committed (a drag or a text field is in progress):
    /// the rig before them.
    base: Option<PhysicsRig>,
    /// "Add" group dialog: the name being typed and whether it duplicates.
    add_name: Option<(String, bool)>,
    rename: Option<String>,
    /// Group editing dialog open, with the rig to restore on Cancel.
    groups: Option<PhysicsRig>,
    groups_show_ids: bool,
    clipboard: Vec<PhysicsOutput>,
    /// Similar change candidates waiting for an answer: (group, list).
    similar: Option<(usize, Vec<(usize, usize)>)>,
    fps_list: Vec<u32>,
    auto_max: bool,
    held: Option<(usize, Vec<[f32; 2]>)>,
    message: Option<String>,
    verified: bool,
}

impl PhysicsEditor {
    /// Opens the window (Modeling ▸ Physics Settings).
    pub fn show(&mut self) {
        self.open = true;
        self.verified = false;
        if self.fps_list.is_empty() {
            self.fps_list = vec![15, 30, 60, 120];
            // TODO(Q-0038): the reference's default for auto-update.
            self.auto_max = true;
        }
    }
}

fn kind_label(k: PhysicsKind) -> &'static str {
    match k {
        PhysicsKind::X => "Position X",
        PhysicsKind::Y => "Position Y",
        PhysicsKind::Angle => "Angle",
    }
}

fn caution_fill(c: Caution, warn: Color32) -> Option<Color32> {
    match c {
        Caution::None => None,
        Caution::Warning => Some(warn.gamma_multiply(0.45)),
        Caution::Duplicate => Some(Color32::from_rgb(60, 110, 170).gamma_multiply(0.5)),
    }
}

/// A parameter picker: a combo over all parameters (name, ID in tooltip).
fn param_combo(ui: &mut egui::Ui, salt: egui::Id, doc: &Document, p: &mut usize) -> bool {
    let params = &doc.model.parameters;
    let name = params.get(*p).map_or("—".to_owned(), |q| q.name.clone());
    let mut changed = false;
    egui::ComboBox::from_id_salt(salt)
        .selected_text(name)
        .width(130.0)
        .height(320.0)
        .show_ui(ui, |ui| {
            for (i, q) in params.iter().enumerate() {
                if ui
                    .selectable_label(*p == i, &q.name)
                    .on_hover_text(&q.id)
                    .clicked()
                {
                    *p = i;
                    changed = true;
                }
            }
        });
    changed
}

/// The Physics Settings window.
pub fn window(
    ctx: &egui::Context,
    st: &mut PhysicsEditor,
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
    // Opening verifies the settings (§ intro).
    if !st.verified {
        st.verified = true;
        let (fixed, msgs) =
            keyform_physics::json3::verify(&doc.model.physics, &doc.model.parameters);
        if !msgs.is_empty() && fixed != doc.model.physics {
            doc.edit(STEP, |m| m.physics = fixed);
            st.message = Some(
                "Input settings, or Output Settings: Found some parameters that do not exist in \
                 this model. Deleted the parameters that do not exist in Physics Settings."
                    .into(),
            );
        }
    }
    let warn = crate::theme::DARK().warning;
    let mut rig = doc.model.physics.clone();
    let editable = doc.editable();
    let mut open = true;
    egui::Window::new(crate::i18n::tr("Physics Settings"))
        .default_size([760.0, 640.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                menus(ui, st, doc, &mut rig, log);
                ui.separator();
                header(ui, st, &mut rig);
                if rig.settings.is_empty() {
                    ui.weak(crate::i18n::tr("No physics groups. Add one to start."));
                    return;
                }
                st.group = st.group.min(rig.settings.len() - 1);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut st.tab, Tab::Input, crate::i18n::tr("Input settings"));
                    ui.selectable_value(
                        &mut st.tab,
                        Tab::Output,
                        crate::i18n::tr("Output Settings"),
                    );
                });
                ui.separator();
                match st.tab {
                    Tab::Input => inputs(ui, st, doc, &mut rig),
                    Tab::Output => outputs(ui, st, doc, &mut rig, warn),
                }
                ui.separator();
                pendulums(ui, st, &mut rig);
            });
            if let Some(m) = &st.message {
                ui.separator();
                ui.label(RichText::new(m).color(warn));
            }
        });
    if !open {
        st.open = false;
    }
    group_dialog(ctx, st, doc, &mut rig, warn);
    add_dialog(ctx, st, &mut rig);
    similar_dialog(ctx, st, &mut rig);
    commit(ctx, st, doc, rig);
}

/// Applies `rig` live and makes one undo step when no drag or text edit
/// is in progress.
fn commit(ctx: &egui::Context, st: &mut PhysicsEditor, doc: &mut Document, rig: PhysicsRig) {
    if rig != doc.model.physics {
        if st.base.is_none() {
            st.base = Some(doc.model.physics.clone());
        }
        let structural = rig.settings.len() != doc.model.physics.settings.len()
            || rig
                .settings
                .iter()
                .zip(&doc.model.physics.settings)
                .any(|(a, b)| {
                    a.vertices.len() != b.vertices.len() || a.outputs.len() != b.outputs.len()
                });
        doc.model.physics = rig;
        if structural {
            doc.sim.reset(&doc.model.physics);
        }
    }
    let busy = ctx.input(|i| i.pointer.any_down()) || ctx.memory(|m| m.focused().is_some());
    if !busy {
        if let Some(base) = st.base.take() {
            let new = std::mem::replace(&mut doc.model.physics, base);
            doc.edit(STEP, |m| m.physics = new);
        }
    }
}

fn menus(
    ui: &mut egui::Ui,
    st: &mut PhysicsEditor,
    doc: &mut Document,
    rig: &mut PhysicsRig,
    log: &mut Log,
) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(crate::i18n::tr("Edit"), |ui| {
            if ui.button(crate::i18n::tr("Undo")).clicked() {
                st.base = None;
                doc.undo();
                *rig = doc.model.physics.clone();
                ui.close();
            }
            if ui.button(crate::i18n::tr("Redo")).clicked() {
                st.base = None;
                doc.redo();
                *rig = doc.model.physics.clone();
                ui.close();
            }
            ui.separator();
            if ui.button(crate::i18n::tr("Copy output lines")).clicked() {
                if let Some(s) = rig.settings.get(st.group) {
                    st.clipboard = s.outputs.clone();
                    st.message = Some("Copied".into());
                }
                ui.close();
            }
            if ui.button(crate::i18n::tr("Paste output lines")).clicked() {
                paste_outputs(st, rig);
                ui.close();
            }
        });
        ui.menu_button(crate::i18n::tr("Physics"), |ui| {
            if ui
                .button(crate::i18n::tr("Import Physics Settings…"))
                .clicked()
            {
                ui.close();
                import(st, doc, rig, log);
            }
            if ui
                .button(crate::i18n::tr("Export Physics settings…"))
                .clicked()
            {
                ui.close();
                export(st, doc, rig, log);
            }
            ui.separator();
            if ui
                .button(crate::i18n::tr("Initialize Calculate FPS"))
                .clicked()
            {
                st.fps_list = vec![15, 30, 60, 120];
                rig.fps = Some(60.0);
                ui.close();
            }
        });
    });
}

fn paste_outputs(st: &mut PhysicsEditor, rig: &mut PhysicsRig) {
    if st.clipboard.is_empty() {
        st.message = Some("Do the copying before pasting.".into());
        return;
    }
    if let Some(s) = rig.settings.get_mut(st.group) {
        s.outputs.extend(st.clipboard.iter().cloned());
    }
}

/// §8: replaces all groups from a physics3.json.
fn import(st: &mut PhysicsEditor, doc: &mut Document, rig: &mut PhysicsRig, log: &mut Log) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("physics3.json", &["json"])
        .pick_file()
    else {
        return;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            log.warn(format!("Could not import the physics settings. ({e})"));
            return;
        }
    };
    let idx = |id: &str| doc.model.parameters.iter().position(|p| p.id == id);
    match keyform_physics::json3::parse(&text, &idx) {
        Ok((new, errors)) => {
            let d = PhysicsRig::default();
            let session_only = new.gravity != d.gravity || new.wind != d.wind;
            *rig = new;
            st.group = 0;
            if errors.is_empty() {
                st.message = Some(if session_only {
                    "Physics settings are imported. Gravity and wind apply in this session only: the model file doesn't store them.".into()
                } else {
                    "Physics settings are imported.".into()
                });
            } else {
                st.message = Some(format!(
                    "Following errors occurred when importing the physics settings.\n{}",
                    errors.join("\n")
                ));
            }
        }
        Err(e) => {
            st.message = Some("Could not import the physics settings.".into());
            log.warn(format!("physics import: {e}"));
        }
    }
}

fn export(st: &mut PhysicsEditor, doc: &Document, rig: &PhysicsRig, log: &mut Log) {
    let Some(v) = keyform_physics::json3::to_json(rig, &doc.model.parameters, true) else {
        st.message = Some("There are no physics settings to export.".into());
        return;
    };
    let Some(path) = rfd::FileDialog::new()
        .add_filter("physics3.json", &["json"])
        .set_file_name("model.physics3.json")
        .save_file()
    else {
        return;
    };
    match std::fs::write(&path, keyform_physics::to_json_string(&v)) {
        Ok(()) => log.info(format!("exported {}", path.display())),
        Err(e) => log.warn(format!("physics export: {e}")),
    }
}

/// §2 Calculate FPS and §3 the group row.
fn header(ui: &mut egui::Ui, st: &mut PhysicsEditor, rig: &mut PhysicsRig) {
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Calculate FPS:"));
        let mut fps = rig.fps.map_or(60, |f| f.round() as u32);
        let before = fps;
        egui::ComboBox::from_id_salt("physics-fps")
            .selected_text(fps.to_string())
            .width(60.0)
            .show_ui(ui, |ui| {
                for &f in &st.fps_list {
                    ui.selectable_value(&mut fps, f, f.to_string());
                }
            });
        // Any integer 10–120 can be typed; typed values join the list.
        ui.add(egui::DragValue::new(&mut fps).range(10..=120));
        if fps != before {
            if !st.fps_list.contains(&fps) {
                st.fps_list.push(fps);
                st.fps_list.sort_unstable();
            }
            rig.fps = Some(fps as f32);
        }
    });
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Group:"));
        let name = rig
            .settings
            .get(st.group)
            .map_or(String::new(), |s| s.name.clone());
        egui::ComboBox::from_id_salt("physics-group")
            .selected_text(name)
            .width(160.0)
            .show_ui(ui, |ui| {
                for (i, s) in rig.settings.iter().enumerate() {
                    ui.selectable_value(&mut st.group, i, &s.name);
                }
            });
        if ui.button(crate::i18n::tr("Add…")).clicked() {
            st.add_name = Some((String::new(), false));
        }
        if ui
            .add_enabled(
                !rig.settings.is_empty(),
                egui::Button::new(crate::i18n::tr("Duplicate…")),
            )
            .clicked()
        {
            if let Some(i) = pe::duplicate(rig, st.group) {
                st.group = i;
            }
        }
        if let Some(s) = rig.settings.get_mut(st.group) {
            ui.checkbox(&mut s.enabled, crate::i18n::tr("Enable"));
        }
        if ui.button(crate::i18n::tr("Group editing…")).clicked() {
            st.groups = Some(rig.clone());
        }
    });
}

/// §5.1 Input settings.
fn inputs(ui: &mut egui::Ui, st: &mut PhysicsEditor, doc: &Document, rig: &mut PhysicsRig) {
    let g = st.group;
    let s = &mut rig.settings[g];
    ui.label(RichText::new(crate::i18n::tr("Input normalization")).strong());
    egui::Grid::new("physics-norm")
        .num_columns(4)
        .show(ui, |ui| {
            ui.label(crate::i18n::tr(""));
            ui.label(crate::i18n::tr("Minimum"));
            ui.label(crate::i18n::tr("Center"));
            ui.label(crate::i18n::tr("Maximum"));
            ui.end_row();
            for (label, n, lim) in [
                ("Angle:", &mut s.angle, 180.0f32),
                ("Position X:", &mut s.position, 100.0),
            ] {
                ui.label(label);
                // Minimum ≤ Center ≤ Maximum: each field is bounded by its neighbours.
                ui.add(
                    egui::DragValue::new(&mut n.minimum)
                        .speed(0.1)
                        .fixed_decimals(1)
                        .range(-lim..=n.default),
                );
                ui.add(
                    egui::DragValue::new(&mut n.default)
                        .speed(0.1)
                        .fixed_decimals(1)
                        .range(n.minimum..=n.maximum),
                );
                ui.add(
                    egui::DragValue::new(&mut n.maximum)
                        .speed(0.1)
                        .fixed_decimals(1)
                        .range(n.default..=lim),
                );
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.menu_button(crate::i18n::tr("Add…"), |ui| {
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (i, q) in doc.model.parameters.iter().enumerate() {
                        if ui.button(&q.name).on_hover_text(&q.id).clicked() {
                            if let Some(inp) = pe::new_input(&doc.model.parameters, i) {
                                s.inputs.push(inp);
                            }
                            ui.close();
                        }
                    }
                });
        });
        ui.weak(crate::i18n::tr(
            "Input parameter can be added by right clicking on menu.",
        ));
    });
    let mut action = None;
    egui::Grid::new("physics-inputs")
        .num_columns(6)
        .striped(true)
        .show(ui, |ui| {
            for h in ["Input", "Type", "Effectiveness (%)", "Reflect", "", ""] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for (k, inp) in s.inputs.iter_mut().enumerate() {
                param_combo(ui, egui::Id::new(("in-p", g, k)), doc, &mut inp.parameter);
                egui::ComboBox::from_id_salt(("in-t", g, k))
                    .selected_text(kind_label(inp.kind))
                    .show_ui(ui, |ui| {
                        // Y is not offered (§5.1).
                        ui.selectable_value(
                            &mut inp.kind,
                            PhysicsKind::X,
                            crate::i18n::tr("Position X"),
                        );
                        ui.selectable_value(
                            &mut inp.kind,
                            PhysicsKind::Angle,
                            crate::i18n::tr("Angle"),
                        );
                    });
                let mut w = inp.weight.round() as i32;
                if ui
                    .add(egui::DragValue::new(&mut w).range(0..=100))
                    .changed()
                {
                    inp.weight = w as f32;
                }
                ui.checkbox(&mut inp.reflect, crate::i18n::tr(""));
                ui.horizontal(|ui| {
                    if ui.small_button(crate::i18n::tr("Up")).clicked() {
                        action = Some(("up", k));
                    }
                    if ui.small_button(crate::i18n::tr("Down")).clicked() {
                        action = Some(("down", k));
                    }
                });
                if ui.small_button(crate::i18n::tr("Delete")).clicked() {
                    action = Some(("del", k));
                }
                ui.end_row();
            }
        });
    row_action(&mut s.inputs, action);
}

fn row_action<T>(v: &mut Vec<T>, a: Option<(&str, usize)>) {
    match a {
        Some(("up", k)) if k > 0 => v.swap(k, k - 1),
        Some(("down", k)) if k + 1 < v.len() => v.swap(k, k + 1),
        Some(("del", k)) if k < v.len() => {
            v.remove(k);
        }
        _ => {}
    }
}

/// §5.2 Output Settings.
fn outputs(
    ui: &mut egui::Ui,
    st: &mut PhysicsEditor,
    doc: &mut Document,
    rig: &mut PhysicsRig,
    warn: Color32,
) {
    let g = st.group;
    if rig.settings[g].vertices.len() < 2 {
        ui.label(
            RichText::new(crate::i18n::tr("The pendulum has not been set. Please add a pendulum in the Physics model settings."))
                .color(warn),
        );
    }
    let params = doc.model.parameters.clone();
    // Without "Auto-update maximum of output" the recorded extremes hold.
    let held = st
        .held
        .take()
        .filter(|h| !st.auto_max && h.0 == g)
        .map(|h| h.1);
    let mut extremes: Vec<[f32; 2]> =
        held.unwrap_or_else(|| doc.sim.output_extremes.get(g).cloned().unwrap_or_default());
    extremes.resize(
        rig.settings[g].outputs.len(),
        [f32::INFINITY, f32::NEG_INFINITY],
    );
    ui.horizontal_wrapped(|ui| {
        if ui.button(crate::i18n::tr("Add")).clicked() {
            let s = &mut rig.settings[g];
            let p = s.outputs.last().map_or(0, |o| o.parameter);
            let o = pe::new_output(s, p);
            s.outputs.push(o);
            extremes.push([f32::INFINITY, f32::NEG_INFINITY]);
        }
        let mut adjust = None;
        if ui
            .button(crate::i18n::tr("Increase Output"))
            .on_hover_text(crate::i18n::tr(
                "Adjust all output magnification of objects which are under 100%",
            ))
            .clicked()
        {
            adjust = Some(Adjust::Increase);
        }
        if ui
            .button(crate::i18n::tr("Decrease Output"))
            .on_hover_text(crate::i18n::tr(
                "Adjust all output magnification of objects which are over 100%",
            ))
            .clicked()
        {
            adjust = Some(Adjust::Decrease);
        }
        if ui
            .button(crate::i18n::tr("Fit to parameter range"))
            .clicked()
        {
            adjust = Some(Adjust::Fit);
        }
        if let Some(a) = adjust {
            pe::adjust_outputs(&mut rig.settings[g], &mut extremes, &params, a);
        }
        if ui.button(crate::i18n::tr("Reset Magnification")).clicked() {
            for o in &mut rig.settings[g].outputs {
                pe::set_scale(o, 1.0);
            }
        }
        if ui
            .button(crate::i18n::tr("Reset Output Maximum"))
            .on_hover_text(crate::i18n::tr("Set output maximum by 0%"))
            .clicked()
        {
            extremes.iter_mut().for_each(|e| *e = [0.0, 0.0]);
        }
        if ui.button(crate::i18n::tr("Copy")).clicked() {
            st.clipboard = rig.settings[g].outputs.clone();
            st.message = Some("Copied".into());
        }
        if ui.button(crate::i18n::tr("Paste")).clicked() {
            paste_outputs(st, rig);
            extremes.resize(
                rig.settings[g].outputs.len(),
                [f32::INFINITY, f32::NEG_INFINITY],
            );
        }
        ui.checkbox(
            &mut st.auto_max,
            crate::i18n::tr("Auto-update maximum of output"),
        );
    });
    let n_pend = pe::pendulum_count(&rig.settings[g]);
    let s = &mut rig.settings[g];
    let mut action = None;
    let mut changed_param = None;
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .id_salt("physics-out-scroll")
        .show(ui, |ui| {
            egui::Grid::new("physics-outputs")
                .num_columns(9)
                .striped(true)
                .show(ui, |ui| {
                    for h in [
                        "Pendulum No",
                        "Output",
                        "Type",
                        "Effectiveness (%)",
                        "Reflect",
                        "Scale",
                        "Output Maximum (%)",
                        "",
                        "",
                    ] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (k, o) in s.outputs.iter_mut().enumerate() {
                        let mut no = o.vertex_index;
                        ui.add(egui::DragValue::new(&mut no).range(1..=n_pend.max(1)));
                        o.vertex_index = no;
                        let before = o.parameter;
                        if param_combo(ui, egui::Id::new(("out-p", g, k)), doc, &mut o.parameter)
                            && o.parameter != before
                        {
                            changed_param = Some((k, before, o.parameter));
                        }
                        let mut kind = o.kind;
                        egui::ComboBox::from_id_salt(("out-t", g, k))
                            .selected_text(kind_label(kind))
                            .show_ui(ui, |ui| {
                                // TODO(Q-0038): output types the reference offers.
                                ui.selectable_value(
                                    &mut kind,
                                    PhysicsKind::X,
                                    crate::i18n::tr("Position X"),
                                );
                                ui.selectable_value(
                                    &mut kind,
                                    PhysicsKind::Angle,
                                    crate::i18n::tr("Angle"),
                                );
                            });
                        if kind != o.kind {
                            // The scale carries over to the new type's field.
                            let sc = o.scale();
                            o.kind = kind;
                            pe::set_scale(o, sc);
                        }
                        let mut w = o.weight.round() as i32;
                        if ui
                            .add(egui::DragValue::new(&mut w).range(0..=100))
                            .changed()
                        {
                            o.weight = w as f32;
                        }
                        ui.checkbox(&mut o.reflect, crate::i18n::tr(""));
                        let old = o.scale();
                        let mut sc = old;
                        if ui
                            .add(
                                egui::DragValue::new(&mut sc)
                                    .speed(0.001)
                                    .fixed_decimals(3)
                                    .range(pe::SCALE.0..=pe::SCALE.1),
                            )
                            .changed()
                        {
                            pe::set_scale(o, sc);
                            if let Some(e) = extremes.get_mut(k) {
                                pe::rescale_extremes(e, old, sc);
                            }
                        }
                        let pct = params.get(o.parameter).and_then(|p| {
                            extremes.get(k).and_then(|e| pe::output_max_percent(*e, p))
                        });
                        match pct {
                            Some(v) => {
                                let t = RichText::new(format!("{v:.0}"));
                                ui.label(if v > 100.0 { t.color(warn) } else { t });
                            }
                            None => {
                                ui.weak(crate::i18n::tr("—"));
                            }
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button(crate::i18n::tr("Up")).clicked() {
                                action = Some(("up", k));
                            }
                            if ui.small_button(crate::i18n::tr("Down")).clicked() {
                                action = Some(("down", k));
                            }
                        });
                        if ui.small_button(crate::i18n::tr("Delete")).clicked() {
                            action = Some(("del", k));
                        }
                        ui.end_row();
                    }
                });
        });
    if s.outputs.iter().any(|o| o.vertex_index > n_pend) {
        ui.label(
            RichText::new(
                "A nonexistent Pendulum No is specified for output. Please set the Pendulum No \
                 again from the output settings.",
            )
            .color(warn),
        );
    }
    row_action(&mut s.outputs, action);
    row_action(&mut extremes, action);
    if let Some((k, from, to)) = changed_param {
        let c = pe::similar_changes(s, &params, k, from, to);
        if !c.is_empty() {
            st.similar = Some((g, c));
        }
    }
    if !st.auto_max {
        st.held = Some((g, extremes.clone()));
    }
    if let Some(e) = doc.sim.output_extremes.get_mut(g) {
        *e = extremes;
    }
}

/// §6 Physics model settings: the pendulum table and a preview.
fn pendulums(ui: &mut egui::Ui, st: &mut PhysicsEditor, rig: &mut PhysicsRig) {
    let g = st.group;
    let s = &mut rig.settings[g];
    let before = s.vertices.clone();
    ui.label(RichText::new(crate::i18n::tr("Pendulum settings")).strong());
    ui.weak(crate::i18n::tr(
        "When a multi-stage pendulum is set, it will be numbered in order from the top.",
    ));
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Add")).clicked() {
                    s.vertices.push(pe::default_pendulum());
                }
            });
            let mut action = None;
            egui::Grid::new("physics-pendulums").num_columns(7).striped(true).show(ui, |ui| {
                for (h, tip) in [
                    ("No", ""),
                    ("Duration", "The smaller the value, the faster the swaying; the larger the value, the slower the swaying."),
                    ("Shaking influence", "The larger the value, the larger the swaying even with the slightest input. Set the value within the range of 0.7-0.99 as a guideline."),
                    ("Reaction time", "1 as the standard speed. Reacts faster if set to a value larger than 1; reacts slower if set to a value smaller than 1."),
                    ("Convergence speed", "1 as the standard speed. Converges faster if set to a value larger than 1; converges slower if set to a value smaller than 1."),
                    ("", ""),
                    ("", ""),
                ] {
                    let r = ui.label(RichText::new(h).strong());
                    if !tip.is_empty() {
                        r.on_hover_text(tip);
                    }
                }
                ui.end_row();
                // The root particle is not a row; rows are numbered from 1
                // (observed in the reference).
                for k in 1..s.vertices.len() {
                    let v = &mut s.vertices[k];
                    ui.label(k.to_string());
                    ui.add(egui::DragValue::new(&mut v.radius).speed(0.1).fixed_decimals(1).range(pe::RADIUS.0..=pe::RADIUS.1));
                    ui.add(egui::DragValue::new(&mut v.mobility).speed(0.01).fixed_decimals(2).range(pe::MOBILITY.0..=pe::MOBILITY.1));
                    ui.add(egui::DragValue::new(&mut v.delay).speed(0.01).fixed_decimals(2).range(pe::DELAY.0..=pe::DELAY.1));
                    ui.add(egui::DragValue::new(&mut v.acceleration).speed(0.01).fixed_decimals(2).range(pe::ACCELERATION.0..=pe::ACCELERATION.1));
                    ui.horizontal(|ui| {
                        if ui.small_button(crate::i18n::tr("Up")).clicked() && k > 1 {
                            action = Some(("up", k));
                        }
                        if ui.small_button(crate::i18n::tr("Down")).clicked() {
                            action = Some(("down", k));
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.small_button(crate::i18n::tr("Duplicate")).clicked() {
                            action = Some(("dup", k));
                        }
                        if ui.small_button(crate::i18n::tr("Delete")).clicked() {
                            action = Some(("del", k));
                        }
                    });
                    ui.end_row();
                }
            });
            match action {
                Some(("dup", k)) => {
                    let v = s.vertices[k].clone();
                    s.vertices.insert(k + 1, v);
                }
                a => row_action(&mut s.vertices, a),
            }
        });
        pendulum_preview(ui, s);
    });
    // Rest positions follow the radii once the pendulums are edited (only
    // then: opening the window must not change the model).
    if s.vertices != before {
        pe::relayout(s);
    }
}

/// The pendulum chain at rest, scaled to fit.
fn pendulum_preview(ui: &mut egui::Ui, s: &keyform_model::PhysicsSetting) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(200.0, 220.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    let len: f32 = s
        .vertices
        .iter()
        .skip(1)
        .map(|v| v.radius)
        .sum::<f32>()
        .max(1.0);
    let scale = (rect.height() - 40.0) / len;
    let top = egui::pos2(rect.center().x, rect.top() + 20.0);
    let pt =
        |v: &keyform_model::PhysicsVertex| top + egui::vec2(v.position[0], v.position[1]) * scale;
    let stroke = egui::Stroke::new(2.0, ui.visuals().text_color());
    for w in s.vertices.windows(2) {
        p.line_segment([pt(&w[0]), pt(&w[1])], stroke);
    }
    for (i, v) in s.vertices.iter().enumerate() {
        let c = if i == 0 {
            ui.visuals().text_color()
        } else {
            Color32::from_rgb(40, 120, 230)
        };
        p.circle_filled(pt(v), 5.0, c);
        if i > 0 {
            p.text(
                pt(v) + egui::vec2(9.0, 0.0),
                egui::Align2::LEFT_CENTER,
                i.to_string(),
                egui::FontId::proportional(12.0),
                c,
            );
        }
    }
    p.text(
        rect.left_top() + egui::vec2(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        format!("Scale: {len:.1}"),
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
}

/// §4 "Edit Physics Group".
fn group_dialog(
    ctx: &egui::Context,
    st: &mut PhysicsEditor,
    doc: &Document,
    rig: &mut PhysicsRig,
    warn: Color32,
) {
    if st.groups.is_none() {
        return;
    }
    let caution = pe::caution(rig);
    let (mut ok, mut cancel) = (false, false);
    let name = |p: usize, ids: bool| {
        doc.model.parameters.get(p).map_or("?".to_owned(), |q| {
            if ids {
                q.id.clone()
            } else {
                q.name.clone()
            }
        })
    };
    egui::Window::new(crate::i18n::tr("Edit Physics Group")).collapsible(false).default_width(760.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(&mut st.groups_show_ids, false, crate::i18n::tr("Show parameter name and effectiveness in tooltip"));
            ui.radio_value(&mut st.groups_show_ids, true, crate::i18n::tr("Show parameter ID and effectiveness in tooltip"));
        });
        ui.weak(crate::i18n::tr("Pay caution to the input and output parameters when the background of the table cell is colored."));
        ui.horizontal_top(|ui| {
            egui::Grid::new("physics-groups").num_columns(7).striped(true).show(ui, |ui| {
                for h in ["Enable", "Priority", "Group name", "Number of pendulums", "Input parameters (positionX)", "Input parameters (angle)", "Output parameter"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (i, s) in rig.settings.iter_mut().enumerate() {
                    let c = &caution[i];
                    ui.checkbox(&mut s.enabled, crate::i18n::tr(""));
                    ui.label((i + 1).to_string());
                    if ui.selectable_label(st.group == i, &s.name).clicked() {
                        st.group = i;
                    }
                    ui.label(pe::pendulum_count(s).to_string());
                    let tip = |kinds: &[PhysicsKind], s: &keyform_model::PhysicsSetting, input: bool| {
                        let mut t = String::new();
                        if input {
                            for (k, x) in s.inputs.iter().enumerate().filter(|(_, x)| kinds.contains(&x.kind)) {
                                let pre = match c.inputs[k] {
                                    Caution::Warning => "!! ",
                                    Caution::Duplicate => "# ",
                                    Caution::None => "",
                                };
                                t += &format!("{pre}{} ({:.0}%)\n", name(x.parameter, st.groups_show_ids), x.weight);
                            }
                        } else {
                            for (k, x) in s.outputs.iter().enumerate() {
                                let pre = match c.outputs[k] {
                                    Caution::Warning => "!! ",
                                    Caution::Duplicate => "# ",
                                    Caution::None => "",
                                };
                                t += &format!("{pre}{} ({:.0}%)\n", name(x.parameter, st.groups_show_ids), x.weight);
                            }
                        }
                        t
                    };
                    let cell = |ui: &mut egui::Ui, n: usize, cau: Caution, tip: String| {
                        let mut f = egui::Frame::new().inner_margin(egui::Margin::symmetric(6, 1));
                        if let Some(col) = caution_fill(cau, warn) {
                            f = f.fill(col);
                        }
                        let r = f.show(ui, |ui| ui.label(n.to_string())).response;
                        if !tip.is_empty() {
                            r.on_hover_text(tip);
                        }
                    };
                    let nx = s.inputs.iter().filter(|x| x.kind != PhysicsKind::Angle).count();
                    let na = s.inputs.len() - nx;
                    cell(ui, nx, c.input_cell(s, PhysicsKind::X), tip(&[PhysicsKind::X, PhysicsKind::Y], s, true));
                    cell(ui, na, c.input_cell(s, PhysicsKind::Angle), tip(&[PhysicsKind::Angle], s, true));
                    cell(ui, s.outputs.len(), c.output_cell(), tip(&[], s, false));
                    ui.end_row();
                }
            });
            ui.vertical(|ui| {
                if ui.button(crate::i18n::tr("Add…")).clicked() {
                    st.add_name = Some((String::new(), false));
                }
                if ui.button(crate::i18n::tr("Duplicate…")).clicked() {
                    if let Some(i) = pe::duplicate(rig, st.group) {
                        st.group = i;
                    }
                }
                if ui.button(crate::i18n::tr("Rename…")).clicked() {
                    st.rename = rig.settings.get(st.group).map(|s| s.name.clone());
                }
                ui.separator();
                if ui.button(crate::i18n::tr("Increase priority")).clicked() && st.group > 0 && st.group < rig.settings.len() {
                    rig.settings.swap(st.group, st.group - 1);
                    st.group -= 1;
                }
                if ui.button(crate::i18n::tr("Decrease priority")).clicked() && st.group + 1 < rig.settings.len() {
                    rig.settings.swap(st.group, st.group + 1);
                    st.group += 1;
                }
                ui.separator();
                if ui.button(crate::i18n::tr("Delete")).clicked() && st.group < rig.settings.len() {
                    rig.settings.remove(st.group);
                    st.group = st.group.saturating_sub(1);
                }
            });
        });
        if let Some(mut name) = st.rename.take() {
            let mut done = false;
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Group name:"));
                ui.text_edit_singleline(&mut name);
                if ui.button(crate::i18n::tr("Rename")).clicked() {
                    if let Some(s) = rig.settings.get_mut(st.group) {
                        s.name = name.clone();
                    }
                    done = true;
                }
            });
            if !done {
                st.rename = Some(name);
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ok = ui.button(crate::i18n::tr("OK")).clicked();
            cancel = ui.button(crate::i18n::tr("Cancel")).clicked();
        });
    });
    if cancel {
        if let Some(r) = st.groups.take() {
            *rig = r;
        }
    } else if ok {
        st.groups = None;
    }
}

/// "Add" a group: asks for a name.
// TODO(Q-0038): the Add dialog's preset combos (no presets yet).
fn add_dialog(ctx: &egui::Context, st: &mut PhysicsEditor, rig: &mut PhysicsRig) {
    let Some((name, _)) = &mut st.add_name else {
        return;
    };
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new(crate::i18n::tr("Add Physics Group"))
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Name:"));
                ui.text_edit_singleline(name);
            });
            ui.horizontal(|ui| {
                ok = ui
                    .add_enabled(
                        !name.trim().is_empty(),
                        egui::Button::new(crate::i18n::tr("OK")),
                    )
                    .clicked();
                cancel = ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
    if ok {
        let s = pe::new_setting(rig, name.trim());
        rig.settings.push(s);
        st.group = rig.settings.len() - 1;
        st.add_name = None;
    } else if cancel {
        st.add_name = None;
    }
}

/// "Similar change candidates exist. Change them as well?" (§5.2).
fn similar_dialog(ctx: &egui::Context, st: &mut PhysicsEditor, rig: &mut PhysicsRig) {
    let Some((g, list)) = &st.similar else {
        return;
    };
    let (mut yes, mut no) = (false, false);
    egui::Window::new(crate::i18n::tr("Similar changes"))
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(crate::i18n::tr(
                "Similar change candidates exist. Change them as well?",
            ));
            for (k, p) in list {
                ui.label(format!("Output {}: → parameter #{p}", k + 1));
            }
            ui.horizontal(|ui| {
                yes = ui.button(crate::i18n::tr("Yes")).clicked();
                no = ui.button(crate::i18n::tr("No")).clicked();
            });
        });
    if yes {
        if let Some(s) = rig.settings.get_mut(*g) {
            for &(k, p) in list {
                if let Some(o) = s.outputs.get_mut(k) {
                    o.parameter = p;
                }
            }
        }
    }
    if yes || no {
        st.similar = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Showing the window (both tabs, the group dialog) changes nothing;
    /// "Add" through the dialog state and an undo round-trip work.
    #[test]
    fn showing_changes_nothing_and_add_undoes() {
        let Some(dir) = std::env::var_os("KEYFORM_BB_P5") else {
            return;
        };
        let mut log = Log::default();
        let path = std::path::Path::new(&dir).join("phys_kf.cmo3");
        if !path.exists() {
            return;
        }
        let mut doc = Document::open(&path, &mut log).unwrap();
        let ctx = egui::Context::default();
        let mut st = PhysicsEditor::default();
        st.show();
        let before = doc.model.physics.clone();
        let frame = |st: &mut PhysicsEditor, doc: &mut Document| {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let c = ui.ctx().clone();
                window(&c, st, Some(&mut *doc), &mut Log::default());
            });
        };
        for tab in [Tab::Input, Tab::Output] {
            st.tab = tab;
            frame(&mut st, &mut doc);
            frame(&mut st, &mut doc);
        }
        st.groups = Some(doc.model.physics.clone());
        frame(&mut st, &mut doc);
        assert_eq!(doc.model.physics, before);
        assert!(!doc.is_modified());
        // Add a group: one undo step, undone by Undo.
        st.groups = None;
        st.add_name = Some(("Tail".into(), false));
        let n = before.settings.len();
        // The dialog's OK is a click; emulate its effect on the next frame.
        let mut rig = doc.model.physics.clone();
        rig.settings.push(pe::new_setting(&rig, "Tail"));
        commit(&ctx, &mut st, &mut doc, rig);
        assert_eq!(doc.model.physics.settings.len(), n + 1);
        assert!(doc.is_modified());
        doc.undo();
        assert_eq!(doc.model.physics.settings.len(), n);
    }
}
