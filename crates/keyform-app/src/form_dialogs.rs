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
    /// Extended Interpolation: the parameter shown.
    ext: Option<usize>,
    /// Key-count hint: shown until this time (seconds), for this parameter.
    ext_hint: Option<(f64, usize)>,
    /// The notice "About Number of Keys for Extended Interpolation" is open.
    ext_notice_open: bool,
    /// "Don't show again" was checked.
    ext_dont_show: bool,
    /// The multi-paste dialog is turned off (`PasteSetting.
    /// displayMultiPasteShapeDialog` = false): ambiguous pastes use the
    /// default pairing.
    pub multi_paste_off: bool,
    /// The dialog's "Don't show again" box.
    pairing_dont_show: bool,
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

    /// Extended Interpolation (a floating panel for the selected objects
    /// and a parameter).
    pub fn open_extended(&mut self, doc: &Document) {
        let m = &doc.model;
        let p = (0..m.parameters.len())
            .find(|&p| {
                doc.selection.objects.iter().any(|&o| {
                    keyform_edit::form_tools::extended_settings(m, o, p).is_some_and(|s| s.3 >= 3)
                })
            })
            .unwrap_or(0);
        self.ext = Some(p);
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
        r.0 = ui.button(crate::i18n::tr("OK")).clicked();
        r.1 = ui.button(crate::i18n::tr("Cancel")).clicked();
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
            multi_paste_off: st.multi_paste_off,
            ..Default::default()
        };
        return;
    };
    blend_dialog(ctx, st, doc, log);
    pairing_dialog(ctx, st, doc, log);
    special_dialog(ctx, st, doc);
    flip_dialog(ctx, st, doc, log);
    corners_dialog(ctx, st, doc, log);
    batch_dialog(ctx, st, doc, log);
    mirror_dialog(ctx, st, doc, log);
    extended_dialog(ctx, st, doc, log);
}

/// Extended Interpolation (extended-interpolation.md §5): method, number
/// of inserted points, scale (ELLIPSE), for the selected objects' binding
/// to one parameter; illegal ELLIPSE settings are marked.
fn extended_dialog(ctx: &egui::Context, st: &mut FormDialogs, doc: &mut Document, log: &mut Log) {
    use keyform_edit::form_tools::{extended_settings, set_extended, EXTENDED_NEEDS_3};
    use keyform_model::ExtendedInterpolationKind as K;
    let Some(mut p) = st.ext.take() else { return };
    let mut open = true;
    let m = &doc.model;
    let targets: Vec<Selection> = doc.selection.objects.clone();
    let current = targets.iter().find_map(|&o| extended_settings(m, o, p));
    let choices: Vec<usize> = (0..m.parameters.len()).collect();
    let mut change: Option<(K, i32, f32)> = None;
    egui::Window::new(crate::i18n::tr("Extended Interpolation"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Parameter"));
                param_combo(ui, "ext-param", m, &choices, &mut p);
            });
            let Some((kind, n, s, keys)) = current else {
                ui.weak(crate::i18n::tr("Select an object with keys on this parameter."));
                return;
            };
            let enabled = doc.editable() && keys >= 3;
            ui.add_enabled_ui(enabled, |ui| {
                ui.strong(crate::i18n::tr("Interpolation method"));
                let label = |k: K| match k {
                    K::Linear => "Linear Interpolation",
                    K::Ellipse => "Ellipse Interpolation",
                    K::SnsCurve => "SNS Interpolation",
                };
                let mut k2 = kind;
                egui::ComboBox::from_id_salt("ext-kind")
                    .selected_text(label(kind))
                    .show_ui(ui, |ui| {
                        for k in [K::Linear, K::Ellipse, K::SnsCurve] {
                            ui.selectable_value(&mut k2, k, label(k));
                        }
                    });
                ui.strong(crate::i18n::tr("Interpolation adjustment"));
                let (mut n2, mut s2) = (n, s);
                ui.add_enabled_ui(kind != K::Linear, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("Points:"));
                        egui::ComboBox::from_id_salt("ext-points")
                            .selected_text(n.to_string())
                            .show_ui(ui, |ui| {
                                for v in (1..=10).chain([15, 20]) {
                                    ui.selectable_value(&mut n2, v, v.to_string());
                                }
                            });
                    });
                    ui.add_enabled_ui(kind == K::Ellipse, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr("Scale"));
                            ui.add(egui::DragValue::new(&mut s2).speed(0.01).range(0.1..=1000.0));
                        });
                    });
                });
                if k2 != kind {
                    // Switching to a curve starts with 5 points (observed).
                    let n = if kind == K::Linear && k2 != K::Linear { 5 } else { n2 };
                    change = Some((k2, n, s2));
                } else if (n2, s2) != (n, s) {
                    change = Some((k2, n2, s2));
                }
            });
            if keys < 3 {
                ui.weak(crate::i18n::tr("This parameter has fewer than 3 keys on the object."));
            }
            if kind == K::Ellipse {
                let legal = targets.iter().all(|&o| {
                    let b = match o {
                        Selection::ArtMesh(i) => m.art_meshes[i].grid.bindings.iter().find(|b| b.parameter == p).cloned(),
                        _ => None,
                    };
                    b.is_none_or(|b| keyform_eval::extended::ellipse_legal(&b.keys, b.extended_scale))
                });
                if !legal {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Illegal setting: adjacent keys differ by more than 175° after scaling (or keys two apart by more than 270°).",
                    );
                }
            }
        });
    // Hint (10 s) when the parameter has an Ellipse/SNS binding with 3 or
    // fewer keys; clicking it opens the notice. spec: §5
    let short = targets.iter().any(|&o| {
        extended_settings(&doc.model, o, p).is_some_and(|(k, _, _, n)| k != K::Linear && n <= 3)
    });
    let now = ctx.input(|i| i.time);
    if short && !st.ext_dont_show && st.ext_hint.is_none_or(|(_, q)| q != p) {
        st.ext_hint = Some((now + 10.0, p));
    }
    if let Some((until, _)) = st.ext_hint {
        if now < until && !st.ext_dont_show {
            egui::Area::new(egui::Id::new("ext-hint"))
                .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0))
                .show(ctx, |ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new(EXTENDED_NEEDS_3).color(ui.visuals().warn_fg_color),
                        ))
                        .clicked()
                    {
                        st.ext_notice_open = true;
                    }
                });
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }
    if st.ext_notice_open {
        let mut keep = true;
        egui::Window::new(crate::i18n::tr("About Number of Keys for Extended Interpolation"))
            .collapsible(false)
            .resizable(false)
            .open(&mut keep)
            .show(ctx, |ui| {
                ui.label(
                    crate::i18n::tr("Extended interpolation requires at least 3 keyforms.\nWhen setting extended interpolation and changing to 2 keyforms or less, please select Linear Interpolation."),
                );
                ui.checkbox(&mut st.ext_dont_show, crate::i18n::tr("Don't show again."));
                if ui.button(crate::i18n::tr("OK")).clicked() {
                    st.ext_notice_open = false;
                }
            });
        if !keep {
            st.ext_notice_open = false;
        }
    }
    if !open {
        return;
    }
    if let Some((k, n, s)) = change {
        let r = doc.try_edit("Extended Interpolation", |m| {
            let mut any = false;
            for &o in &targets {
                if extended_settings(m, o, p).is_some() {
                    any |= set_extended(m, o, p, k, n, s)?;
                }
            }
            Ok(any)
        });
        if let Err(e) = r {
            log.warn(e);
        }
        let sel = doc.selection.clone();
        doc.set_selection(sel);
    }
    st.ext = Some(p);
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
    egui::Window::new(crate::i18n::tr("Motion Mirroring"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Source parameter"));
                param_combo(ui, "mirror-param", m, &choices, &mut d.p);
            });
            ui.strong(crate::i18n::tr("Basic Settings"));
            ui.checkbox(&mut d.horizontal, crate::i18n::tr("Flip horizontally"));
            ui.checkbox(&mut d.vertical, crate::i18n::tr("Flip vertically"));
            ui.strong(crate::i18n::tr("Mirror Axis Settings"));
            ui.add_enabled_ui(!all_deformers, |ui| {
                ui.radio_value(&mut d.axis, None, crate::i18n::tr("Use Center of Canvas"));
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
/// Name of an object for lists.
fn object_label(doc: &Document, o: Selection) -> String {
    let m = &doc.model;
    let info = match o {
        Selection::ArtMesh(i) => m.art_meshes.get(i).map(|a| &a.info),
        Selection::Deformer(i) => m.deformers.get(i).map(|d| &d.info),
        Selection::Part(i) => m.parts.get(i).map(|p| &p.info),
        Selection::Glue(i) => m.glues.get(i).map(|g| &g.info),
    };
    info.map_or_else(|| "?".into(), |i| i.display_name().to_owned())
}

/// Multi-paste dialog: pairs copied forms with targets when several
/// copies fit a target. spec: form-tools.md §2; settings-keys.md
/// (`PasteSetting.displayMultiPasteShapeDialog`)
fn pairing_dialog(ctx: &egui::Context, st: &mut FormDialogs, doc: &mut Document, log: &mut Log) {
    let Some(mut req) = doc.paste_pairing.take() else {
        return;
    };
    let run = |doc: &mut Document, req: &crate::document::PastePairing, log: &mut Log| match doc
        .paste_paired(&req.name, req.w, req.mirror, &req.pairs)
    {
        Ok(_) => {}
        Err(e) => log.warn(e),
    };
    if st.multi_paste_off {
        run(doc, &req, log);
        return;
    }
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    let mut dont_show = st.pairing_dont_show;
    let copy_names: Vec<String> = doc
        .form_clipboard
        .iter()
        .map(|c| object_label(doc, c.object))
        .collect();
    egui::Window::new(crate::i18n::tr("Paste Form: pair copies with targets"))
        .collapsible(false)
        .resizable(true)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(crate::i18n::tr(
                "Several copied forms fit these objects. Choose the form to paste into each:",
            ));
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    egui::Grid::new("paste-pairs")
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            for (k, (t, cands)) in req.candidates.iter().enumerate() {
                                ui.label(object_label(doc, *t));
                                let cur = &mut req.pairs[k].1;
                                let text = cur
                                    .and_then(|c| copy_names.get(c))
                                    .cloned()
                                    .unwrap_or_else(|| "(none)".into());
                                egui::ComboBox::from_id_salt(("paste-pair", k))
                                    .selected_text(text)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(cur, None, crate::i18n::tr("(none)"));
                                        for &c in cands {
                                            ui.selectable_value(cur, Some(c), &copy_names[c]);
                                        }
                                    });
                                ui.end_row();
                            }
                        });
                });
            ui.checkbox(&mut dont_show, crate::i18n::tr("Don't show again"));
            (ok, cancel) = yes_no(ui);
        });
    st.pairing_dont_show = dont_show;
    if ok {
        st.multi_paste_off = dont_show;
        st.pairing_dont_show = false;
        run(doc, &req, log);
    } else if !cancel && open {
        doc.paste_pairing = Some(req);
    }
}

fn blend_dialog(ctx: &egui::Context, st: &mut FormDialogs, doc: &mut Document, log: &mut Log) {
    let Some((mut rate, mut preview, base)) = st.blend.take() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel, mut changed) = (false, false, false);
    egui::Window::new(crate::i18n::tr("Blend Form"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Apply Rate"));
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut rate)
                            .range(-10000.0..=10000.0)
                            .speed(1.0)
                            .suffix(" %"),
                    )
                    .changed();
            });
            changed |= ui
                .checkbox(&mut preview, crate::i18n::tr("Preview"))
                .changed();
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
    egui::Window::new(crate::i18n::tr("Options for Paste Form Special"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut st.special_h.0,
                    crate::i18n::tr("Flip horizontally about x ="),
                );
                ui.add(egui::DragValue::new(&mut st.special_h.1));
                if ui.small_button(crate::i18n::tr("Centre")).clicked() {
                    st.special_h.1 = cw / 2.0;
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut st.special_v.0,
                    crate::i18n::tr("Flip vertically about y ="),
                );
                ui.add(egui::DragValue::new(&mut st.special_v.1));
                if ui.small_button(crate::i18n::tr("Centre")).clicked() {
                    st.special_v.1 = ch / 2.0;
                }
            });
            ui.weak(crate::i18n::tr(
                "Paste Form and Blend Form mirror the copied shape first.",
            ));
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
    egui::Window::new(crate::i18n::tr("Flip"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.strong(crate::i18n::tr("Flip"));
            ui.checkbox(&mut s.horizontal, crate::i18n::tr("Flip horizontally"));
            ui.checkbox(&mut s.vertical, crate::i18n::tr("Flip vertically"));
            ui.checkbox(
                &mut s.all_keyforms,
                crate::i18n::tr("All keyforms (not only the current one)"),
            );
            ui.collapsing(crate::i18n::tr("Parameters to reflect"), |ui| {
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
            ui.strong(crate::i18n::tr("Other"));
            ui.checkbox(
                &mut s.flip_rotation_position,
                crate::i18n::tr("Flip rotation deformer position"),
            );
            ui.checkbox(
                &mut s.keep_culling,
                crate::i18n::tr("Cancel auto-adjusted culling"),
            );
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
    egui::Window::new(crate::i18n::tr("Synthesize corners"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.strong(crate::i18n::tr("Parameter"));
            egui::Grid::new("corner-params")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label(crate::i18n::tr("Parameter 1"));
                    param_combo(ui, "corner-p1", m, &choices, &mut d.p1);
                    ui.end_row();
                    ui.label(crate::i18n::tr("Parameter 2"));
                    param_combo(ui, "corner-p2", m, &choices, &mut d.p2);
                    ui.end_row();
                });
            ui.strong(crate::i18n::tr("Parameter Value to Reference"));
            ui.radio_value(&mut d.centre, false, crate::i18n::tr("Default value"));
            ui.radio_value(
                &mut d.centre,
                true,
                crate::i18n::tr("Centre (min + max) / 2"),
            );
            ui.strong(crate::i18n::tr("Keys to Overwrite"));
            egui::Grid::new("corner-keys")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.checkbox(&mut d.corners[0], crate::i18n::tr("Top left"));
                    ui.checkbox(&mut d.corners[1], crate::i18n::tr("Top right"));
                    ui.end_row();
                    ui.checkbox(&mut d.corners[2], crate::i18n::tr("Bottom left"));
                    ui.checkbox(&mut d.corners[3], crate::i18n::tr("Bottom right"));
                    ui.end_row();
                });
            ui.strong(crate::i18n::tr("Target"));
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(
                    &mut d.target,
                    CornerTarget::All,
                    crate::i18n::tr("All objects"),
                );
                ui.radio_value(
                    &mut d.target,
                    CornerTarget::Drawables,
                    crate::i18n::tr("All drawables"),
                );
                ui.radio_value(
                    &mut d.target,
                    CornerTarget::Deformers,
                    crate::i18n::tr("All deformers"),
                );
                ui.radio_value(
                    &mut d.target,
                    CornerTarget::Selected,
                    crate::i18n::tr("Selected objects"),
                );
            });
            ui.strong(crate::i18n::tr("Option"));
            ui.checkbox(&mut d.skip_locked, crate::i18n::tr("Skip locked objects"));
            ui.checkbox(
                &mut d.skip_invisible,
                crate::i18n::tr("Skip invisible objects"),
            );
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
    egui::Window::new(crate::i18n::tr("Batch Reflect"))
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(crate::i18n::tr(
                "Select the parameters for batch reflection.",
            ));
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
