//! Form tool dialogs: Blend Form (Apply Rate, Preview), Options for Paste
//! Form Special, Flip (Reflection), Synthesize corners, Motion Mirroring,
//! Batch Reflect. spec: 35-editing/form-tools.md §2–5; 50-ui/menus.md
//! "Edit Form", "Parameter"

use eframe::egui;
use keyform_edit::form_tools::{
    all_objects, batch_reflect, corner_parameters, flip, mirror_check, motion_mirror,
    synthesize_corners, Corner, CornerSettings, FlipSettings, MirrorSettings, PasteMirror,
    MIRROR_NO_OBJECTS, MIRROR_PARENT_WARNING,
};
use keyform_model::{DeformerKind, Model};

use crate::document::{Document, Selection};
use crate::log::Log;

/// Synthesize corners' Target choice.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CornerTarget {
    All,
    Drawables,
    Deformers,
    Selected,
}

pub struct CornerDialog {
    p1: usize,
    p2: usize,
    centre: bool,
    corners: [bool; 4],
    target: CornerTarget,
    skip_locked: bool,
    skip_invisible: bool,
}

#[derive(Default)]
pub struct FormDialogs {
    /// Blend Form: apply rate (%), preview, and the model before preview.
    blend: Option<(f32, bool, Option<Model>)>,
    pub special_open: bool,
    /// Paste Form Special options (used by Paste Form).
    pub special: PasteMirror,
    special_h: (bool, f32),
    special_v: (bool, f32),
    flip: Option<FlipSettings>,
    corners: Option<CornerDialog>,
    batch: Option<Vec<bool>>,
    mirror: Option<MirrorDialog>,
}

/// Motion Mirroring dialog state; the flips and axis are remembered.
#[derive(Clone, Copy)]
pub struct MirrorDialog {
    p: usize,
    horizontal: bool,
    vertical: bool,
    /// `None` = centre of canvas; `Some(d)` = centre of rotation deformer d.
    axis: Option<usize>,
}

impl FormDialogs {
    pub fn open_blend(&mut self) {
        self.blend = Some((50.0, false, None));
    }

    pub fn open_flip(&mut self) {
        self.flip = Some(FlipSettings {
            horizontal: true,
            ..Default::default()
        });
    }

    pub fn open_corners(&mut self, doc: &Document) -> Result<(), String> {
        let ps = corner_parameters(&doc.model);
        if ps.len() < 2 {
            return Err("There are no parameters with 3 or more keys.".into());
        }
        let find = |id: &str| {
            doc.model
                .parameters
                .iter()
                .position(|p| p.id == id)
                .filter(|p| ps.contains(p))
        };
        self.corners = Some(CornerDialog {
            p1: find("ParamAngleX").unwrap_or(ps[0]),
            p2: find("ParamAngleY").unwrap_or(ps[1]),
            centre: false,
            corners: [true; 4],
            target: CornerTarget::Selected,
            skip_locked: true,
            skip_invisible: false,
        });
        Ok(())
    }

    /// Motion Mirroring: the parameter defaults to one bound to the
    /// selected objects whose value is off its default.
    pub fn open_mirror(&mut self, doc: &Document) -> Result<(), String> {
        if doc.selection.objects.is_empty() {
            return Err(MIRROR_NO_OBJECTS.into());
        }
        let m = &doc.model;
        let p = (0..m.parameters.len())
            .find(|&p| {
                (doc.values[p] - m.parameters[p].default).abs() > 1e-6
                    && doc
                        .selection
                        .objects
                        .iter()
                        .all(|&o| !keyform_edit::keys_of(m, o, p).is_empty())
            })
            .unwrap_or(0);
        let prev = self.mirror.unwrap_or(MirrorDialog {
            p,
            horizontal: true,
            vertical: false,
            axis: None,
        });
        self.mirror = Some(MirrorDialog { p, ..prev });
        Ok(())
    }

    pub fn open_batch(&mut self, doc: &Document) {
        self.batch = Some(vec![false; doc.model.parameters.len()]);
    }
}

fn param_combo(ui: &mut egui::Ui, id: &str, m: &Model, choices: &[usize], v: &mut usize) {
    let name = |p: usize| {
        m.parameters
            .get(p)
            .map_or("?".to_owned(), |x| x.info_name())
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(name(*v))
        .show_ui(ui, |ui| {
            for &p in choices {
                ui.selectable_value(v, p, name(p));
            }
        });
}

trait ParamName {
    fn info_name(&self) -> String;
}

impl ParamName for keyform_model::Parameter {
    fn info_name(&self) -> String {
        if self.name.is_empty() {
            self.id.clone()
        } else {
            format!("{} ({})", self.name, self.id)
        }
    }
}

fn yes_no(ui: &mut egui::Ui) -> (bool, bool) {
    let mut r = (false, false);
    ui.separator();
    ui.horizontal(|ui| {
        r.0 = ui.button("OK").clicked();
        r.1 = ui.button("Cancel").clicked();
    });
    r
}

pub fn dialogs(
    ctx: &egui::Context,
    st: &mut FormDialogs,
    doc: Option<&mut Document>,
    log: &mut Log,
) {
    let Some(doc) = doc else {
        *st = FormDialogs {
            special: st.special,
            special_h: st.special_h,
            special_v: st.special_v,
            ..Default::default()
        };
        return;
    };
    blend_dialog(ctx, st, doc, log);
    special_dialog(ctx, st, doc);
    flip_dialog(ctx, st, doc, log);
    corners_dialog(ctx, st, doc, log);
    batch_dialog(ctx, st, doc, log);
    mirror_dialog(ctx, st, doc, log);
}

/// Motion Mirroring (§5.1).
fn mirror_dialog(ctx: &egui::Context, st: &mut FormDialogs, doc: &mut Document, log: &mut Log) {
    let Some(mut d) = st.mirror.take() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    let m = &doc.model;
    let all_deformers = doc
        .selection
        .objects
        .iter()
        .all(|o| matches!(o, Selection::Deformer(_)));
    let rotations: Vec<usize> = (0..m.deformers.len())
        .filter(|&i| matches!(m.deformers[i].kind, DeformerKind::Rotation(_)))
        .collect();
    let choices: Vec<usize> = (0..m.parameters.len())
        .filter(|&p| !m.parameters[p].repeat)
        .collect();
    egui::Window::new("Motion Mirroring")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Source parameter");
                param_combo(ui, "mirror-param", m, &choices, &mut d.p);
            });
            ui.strong("Basic Settings");
            ui.checkbox(&mut d.horizontal, "Flip horizontally");
            ui.checkbox(&mut d.vertical, "Flip vertically");
            ui.strong("Mirror Axis Settings");
            ui.add_enabled_ui(!all_deformers, |ui| {
                ui.radio_value(&mut d.axis, None, "Use Center of Canvas");
                ui.horizontal(|ui| {
                    let mut use_rot = d.axis.is_some();
                    if ui
                        .add_enabled(
                            !rotations.is_empty(),
                            egui::RadioButton::new(
                                use_rot,
                                "Use Center Point of Rotation Deformer",
                            ),
                        )
                        .clicked()
                    {
                        use_rot = true;
                        d.axis = d.axis.or(rotations.first().copied());
                    }
                    if use_rot {
                        let name = |i: usize| m.deformers[i].info.display_name().to_owned();
                        let cur = d.axis.unwrap_or(0);
                        egui::ComboBox::from_id_salt("mirror-rot")
                            .selected_text(name(cur))
                            .show_ui(ui, |ui| {
                                for &r in &rotations {
                                    ui.selectable_value(&mut d.axis, Some(r), name(r));
                                }
                            });
                    }
                });
            });
            ui.weak(format!(
                "Targets: {} objects",
                doc.selection
                    .objects
                    .iter()
                    .filter(|o| !matches!(o, Selection::Glue(_)))
                    .count()
            ));
            (ok, cancel) = yes_no(ui);
        });
    if cancel || !open {
        return;
    }
    if !ok {
        st.mirror = Some(d);
        return;
    }
    let objects = doc.selection.objects.clone();
    let values = doc.values.clone();
    match mirror_check(&doc.model, &values, &objects, d.p) {
        Ok(true) => log.warn(MIRROR_PARENT_WARNING),
        Ok(false) => {}
        Err(e) => {
            log.warn(e);
            return;
        }
    }
    // Axis value at the current state (canvas pixels).
    let (cw, ch) = (
        doc.model.canvas.width as f32,
        doc.model.canvas.height as f32,
    );
    let (mut ax, mut ay) = (cw / 2.0, ch / 2.0);
    if let (Some(r), false) = (d.axis, all_deformers) {
        if let Some(keyform_eval::DeformerMap::Rotation(rm)) = doc
            .frame
            .as_ref()
            .and_then(|f| f.deformers.get(r)?.map.clone())
        {
            (ax, ay) = (rm.origin[0], rm.origin[1]);
        }
    }
    let s = MirrorSettings {
        horizontal: d.horizontal,
        vertical: d.vertical,
        axis_x: ax,
        axis_y: ay,
    };
    let sel = doc.selection.clone();
    match doc.try_edit("Motion Mirroring", |m| {
        motion_mirror(m, &values, &objects, d.p, &s)
    }) {
        Ok(v) => {
            doc.set_selection(sel);
            // The parameter is left at v′.
            doc.set_manual(d.p, v);
        }
        Err(e) => log.warn(e),
    }
}

/// Blend Form: Apply Rate (−10000…10000 %), live Preview restored on
/// Cancel. spec: form-tools.md §2
fn blend_dialog(ctx: &egui::Context, st: &mut FormDialogs, doc: &mut Document, log: &mut Log) {
    let Some((mut rate, mut preview, base)) = st.blend.take() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel, mut changed) = (false, false, false);
    egui::Window::new("Blend Form")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Apply Rate");
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut rate)
                            .range(-10000.0..=10000.0)
                            .speed(1.0)
                            .suffix(" %"),
                    )
                    .changed();
            });
            changed |= ui.checkbox(&mut preview, "Preview").changed();
            (ok, cancel) = yes_no(ui);
        });
    let restore = |doc: &mut Document, base: &Option<Model>| {
        if let Some(b) = base {
            doc.model = b.clone();
            doc.model_changed();
        }
    };
    if cancel || !open {
        restore(doc, &base);
        return;
    }
    let w = rate.round() / 100.0;
    if ok {
        restore(doc, &base);
        if let Err(e) = doc.paste_form("Blend Form", w, st.special) {
            log.warn(e);
        }
        return;
    }
    let mut base = base;
    if changed {
        restore(doc, &base);
        base = None;
        if preview {
            let before = doc.model.clone();
            let copies = doc.form_clipboard.clone();
            let targets: Vec<Selection> = if doc.selection.objects.is_empty() {
                copies.iter().map(|c| c.object).collect()
            } else {
                doc.selection.objects.clone()
            };
            let mut m = doc.model.clone();
            if keyform_edit::form_tools::paste_form(
                &mut m,
                &doc.values,
                &copies,
                &targets,
                w,
                st.special,
            )
            .is_ok()
            {
                doc.model = m;
                doc.model_changed();
                base = Some(before);
            }
        }
    }
    st.blend = Some((rate, preview, base));
}

fn special_dialog(ctx: &egui::Context, st: &mut FormDialogs, doc: &Document) {
    if !st.special_open {
        return;
    }
    let mut open = true;
    let (cw, ch) = (
        doc.model.canvas.width as f32,
        doc.model.canvas.height as f32,
    );
    egui::Window::new("Options for Paste Form Special")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.special_h.0, "Flip horizontally about x =");
                ui.add(egui::DragValue::new(&mut st.special_h.1));
                if ui.small_button("Centre").clicked() {
                    st.special_h.1 = cw / 2.0;
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.special_v.0, "Flip vertically about y =");
                ui.add(egui::DragValue::new(&mut st.special_v.1));
                if ui.small_button("Centre").clicked() {
                    st.special_v.1 = ch / 2.0;
                }
            });
            ui.weak("Paste Form and Blend Form mirror the copied shape first.");
        });
    st.special = PasteMirror {
        horizontal: st.special_h.0.then_some(st.special_h.1),
        vertical: st.special_v.0.then_some(st.special_v.1),
    };
    if !open {
        st.special_open = false;
    }
}

/// Flip (Reflection dialog). spec: form-tools.md §4
fn flip_dialog(ctx: &egui::Context, st: &mut FormDialogs, doc: &mut Document, log: &mut Log) {
    let Some(mut s) = st.flip.take() else { return };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    let m = &doc.model;
    egui::Window::new("Flip")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.strong("Flip");
            ui.checkbox(&mut s.horizontal, "Flip horizontally");
            ui.checkbox(&mut s.vertical, "Flip vertically");
            ui.checkbox(
                &mut s.all_keyforms,
                "All keyforms (not only the current one)",
            );
            ui.collapsing("Parameters to reflect", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for (p, x) in m.parameters.iter().enumerate() {
                            let mut on = s.reflect.contains(&p);
                            if ui.checkbox(&mut on, x.info_name()).changed() {
                                if on {
                                    s.reflect.push(p);
                                } else {
                                    s.reflect.retain(|q| *q != p);
                                }
                            }
                        }
                    });
            });
            ui.strong("Other");
            ui.checkbox(
                &mut s.flip_rotation_position,
                "Flip rotation deformer position",
            );
            ui.checkbox(&mut s.keep_culling, "Cancel auto-adjusted culling");
            (ok, cancel) = yes_no(ui);
        });
    if cancel || !open {
        return;
    }
    if !ok {
        st.flip = Some(s);
        return;
    }
    let objects = doc.selection.objects.clone();
    if objects.is_empty() {
        log.warn("Select the objects to flip.");
        return;
    }
    let values = doc.values.clone();
    let sel = doc.selection.clone();
    match doc.try_edit("Flip", |m| Ok(flip(m, &values, &objects, &s))) {
        Ok(0) => log.warn("There is no keyform to edit at the current parameter values."),
        Ok(_) => doc.set_selection(sel),
        Err(e) => log.warn(e),
    }
}

/// Synthesize corners. spec: form-tools.md §3
fn corners_dialog(ctx: &egui::Context, st: &mut FormDialogs, doc: &mut Document, log: &mut Log) {
    let Some(mut d) = st.corners.take() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    let choices = corner_parameters(&doc.model);
    let m = &doc.model;
    egui::Window::new("Synthesize corners")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.strong("Parameter");
            egui::Grid::new("corner-params")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Parameter 1");
                    param_combo(ui, "corner-p1", m, &choices, &mut d.p1);
                    ui.end_row();
                    ui.label("Parameter 2");
                    param_combo(ui, "corner-p2", m, &choices, &mut d.p2);
                    ui.end_row();
                });
            ui.strong("Parameter Value to Reference");
            ui.radio_value(&mut d.centre, false, "Default value");
            ui.radio_value(&mut d.centre, true, "Centre (min + max) / 2");
            ui.strong("Keys to Overwrite");
            egui::Grid::new("corner-keys")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.checkbox(&mut d.corners[0], "Top left");
                    ui.checkbox(&mut d.corners[1], "Top right");
                    ui.end_row();
                    ui.checkbox(&mut d.corners[2], "Bottom left");
                    ui.checkbox(&mut d.corners[3], "Bottom right");
                    ui.end_row();
                });
            ui.strong("Target");
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(&mut d.target, CornerTarget::All, "All objects");
                ui.radio_value(&mut d.target, CornerTarget::Drawables, "All drawables");
                ui.radio_value(&mut d.target, CornerTarget::Deformers, "All deformers");
                ui.radio_value(&mut d.target, CornerTarget::Selected, "Selected objects");
            });
            ui.strong("Option");
            ui.checkbox(&mut d.skip_locked, "Skip locked objects");
            ui.checkbox(&mut d.skip_invisible, "Skip invisible objects");
            (ok, cancel) = yes_no(ui);
        });
    if cancel || !open {
        return;
    }
    if !ok {
        st.corners = Some(d);
        return;
    }
    if d.p1 == d.p2 {
        log.warn("Choose two different parameters.");
        st.corners = Some(d);
        return;
    }
    let m = &doc.model;
    let mut objects: Vec<Selection> = match d.target {
        CornerTarget::Selected => doc.selection.objects.clone(),
        _ => all_objects(m),
    };
    objects.retain(|o| match d.target {
        CornerTarget::Drawables => matches!(o, Selection::ArtMesh(_)),
        CornerTarget::Deformers => matches!(o, Selection::Deformer(_)),
        _ => true,
    });
    objects.retain(|&o| {
        crate::palettes::info(m, o)
            .is_some_and(|i| !(d.skip_locked && i.locked) && !(d.skip_invisible && !i.visible))
    });
    let corners: Vec<Corner> = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ]
    .into_iter()
    .zip(d.corners)
    .filter_map(|(c, on)| on.then_some(c))
    .collect();
    let s = CornerSettings {
        p1: d.p1,
        p2: d.p2,
        centre: d.centre,
        corners,
    };
    let values = doc.values.clone();
    match doc.try_edit("Synthesize corners", |m| {
        Ok(synthesize_corners(m, &values, &objects, &s))
    }) {
        Ok(0) => log.warn("No object has 3 or more keys on both parameters."),
        Ok(n) => {
            log.info(format!("Corners synthesized on {n} objects."));
            // Afterwards the parameters go back to their defaults.
            doc.reset_parameters();
        }
        Err(e) => log.warn(e),
    }
}

/// Batch Reflect. spec: form-tools.md §5.2
fn batch_dialog(ctx: &egui::Context, st: &mut FormDialogs, doc: &mut Document, log: &mut Log) {
    let Some(mut chosen) = st.batch.take() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    let m = &doc.model;
    chosen.resize(m.parameters.len(), false);
    egui::Window::new("Batch Reflect")
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("Select the parameters for batch reflection.");
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for (p, x) in m.parameters.iter().enumerate() {
                        ui.checkbox(&mut chosen[p], x.info_name());
                    }
                });
            (ok, cancel) = yes_no(ui);
        });
    if cancel || !open {
        return;
    }
    if !ok {
        st.batch = Some(chosen);
        return;
    }
    let ps: Vec<usize> = (0..chosen.len()).filter(|&p| chosen[p]).collect();
    if ps.is_empty() {
        log.warn("Select the parameters for batch reflection.");
        st.batch = Some(chosen);
        return;
    }
    if let Err(e) = doc.try_edit("Batch Reflect", |m| Ok(batch_reflect(m, &ps))) {
        log.warn(e);
    }
}
