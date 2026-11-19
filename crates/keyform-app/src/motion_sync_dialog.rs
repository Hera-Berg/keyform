//! Motion-sync Settings dialog: settings, post-processing, mappings, audio
//! elements and parameters, Keyform's own presets, and an audio preview.
//! spec: 70-advanced/motion-sync.md §1, §3; 50-ui/menus.md (Modeling ▸
//! Parameter ▸ Motion-sync Settings)

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{self, RichText};
use keyform_model::{Model, MotionSyncSetting, SyncAudioParam, SyncCubismParam, SyncMapping};

use crate::document::Document;
use crate::log::Log;

const STEP: &str = "Motion-sync Settings";
/// All audio elements of a stored setting share this GUID (observed).
const NULL_GUID: &str = "00000000-0000-0000-0000-000000000000";
/// The audio elements of the mouth use case, in order (motion-sync.md §1).
const ELEMENTS: [&str; 6] = ["Silence", "A", "I", "U", "E", "O"];
/// Target version needed (5.0).
const MIN_TARGET: i32 = 5_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Mapping,
    Audio,
    Parameter,
}

struct Preview {
    frames: Vec<keyform_anim::motionsync::FrameMap>,
    rate: f32,
    started: Instant,
    _audio: Option<crate::audio_out::AudioOut>,
    /// Parameter values before the preview, restored at its end.
    saved: Vec<f32>,
}

#[derive(Default)]
pub struct MotionSyncDialog {
    pub open: bool,
    setting: usize,
    tab: Tab,
    element: usize,
    add: Option<(String, usize)>,
    rename: Option<String>,
    audio_path: Option<PathBuf>,
    pcm: Option<keyform_anim::audio::Pcm>,
    preview: Option<Preview>,
    message: Option<String>,
    /// Settings before uncommitted edits.
    base: Option<Vec<MotionSyncSetting>>,
    /// User presets (remembered by the app).
    pub user_presets: Vec<UserPreset>,
    preset_add: Option<String>,
    /// Preset List being edited (user presets) and its selected row.
    preset_list: Option<(Vec<UserPreset>, Option<usize>)>,
}

/// A user preset: a setting's parameters, elements, mapping and
/// post-processing (Preset ▸ Add…: "add the current input as a preset").
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UserPreset {
    pub name: String,
    /// (parameter id, name, min, max, damper, smooth).
    pub cubism: Vec<(String, String, f32, f32, f32, i32)>,
    /// (audio id, name, min, max, scale, enabled).
    pub audio: Vec<(String, String, f32, f32, f32, bool)>,
    pub mappings: Vec<(String, Vec<(String, f32)>)>,
    pub blend: f32,
    pub smoothing: i32,
    pub sample_rate: f32,
}

impl UserPreset {
    pub fn from_setting(name: &str, s: &MotionSyncSetting) -> Self {
        Self {
            name: name.to_owned(),
            cubism: s
                .cubism
                .iter()
                .map(|c| {
                    (
                        c.parameter_id.clone(),
                        c.name.clone(),
                        c.min,
                        c.max,
                        c.damper,
                        c.smooth,
                    )
                })
                .collect(),
            audio: s
                .audio
                .iter()
                .map(|a| {
                    (
                        a.audio_id.clone(),
                        a.name.clone(),
                        a.min,
                        a.max,
                        a.scale,
                        a.enabled,
                    )
                })
                .collect(),
            mappings: s
                .mappings
                .iter()
                .map(|x| (x.audio_id.clone(), x.targets.clone()))
                .collect(),
            blend: s.blend,
            smoothing: s.smoothing,
            sample_rate: s.sample_rate,
        }
    }

    /// Replaces `s`'s contents (keeping its GUID, ID and name).
    pub fn apply_to(&self, s: &mut MotionSyncSetting) {
        s.cubism = self
            .cubism
            .iter()
            .map(|c| SyncCubismParam {
                guid: keyform_model::new_guid(),
                parameter_id: c.0.clone(),
                name: c.1.clone(),
                min: c.2,
                max: c.3,
                damper: c.4,
                smooth: c.5,
            })
            .collect();
        s.audio = self
            .audio
            .iter()
            .map(|a| SyncAudioParam {
                guid: NULL_GUID.into(),
                audio_id: a.0.clone(),
                name: a.1.clone(),
                min: a.2,
                max: a.3,
                scale: a.4,
                enabled: a.5,
            })
            .collect();
        s.mappings = self
            .mappings
            .iter()
            .map(|(a, t)| SyncMapping {
                audio_id: a.clone(),
                kind: "Shape".into(),
                targets: t.clone(),
            })
            .collect();
        s.blend = self.blend;
        s.smoothing = self.smoothing;
        s.sample_rate = self.sample_rate;
    }
}

impl MotionSyncDialog {
    pub fn with_presets(user_presets: Vec<UserPreset>) -> Self {
        Self {
            user_presets,
            ..Self::default()
        }
    }

    pub fn show(&mut self) {
        self.open = true;
    }
}

/// Keyform's built-in presets (motion-sync.md §1: Keyform ships its own
/// values, not the reference's). (name, [(element, [(parameter, value)])]).
type PresetTargets = &'static [(&'static str, &'static [(&'static str, f32)])];
const PRESETS: [(&str, PresetTargets); 2] = [
    (
        "Mouth open and form (Keyform)",
        &[
            (
                "Silence",
                &[("ParamMouthOpenY", 0.0), ("ParamMouthForm", 0.0)],
            ),
            ("A", &[("ParamMouthOpenY", 0.9), ("ParamMouthForm", 0.2)]),
            ("I", &[("ParamMouthOpenY", 0.3), ("ParamMouthForm", 0.9)]),
            ("U", &[("ParamMouthOpenY", 0.35), ("ParamMouthForm", -0.7)]),
            ("E", &[("ParamMouthOpenY", 0.55), ("ParamMouthForm", 0.6)]),
            ("O", &[("ParamMouthOpenY", 0.75), ("ParamMouthForm", -0.5)]),
        ],
    ),
    (
        "Mouth open only (Keyform)",
        &[
            ("Silence", &[("ParamMouthOpenY", 0.0)]),
            ("A", &[("ParamMouthOpenY", 0.9)]),
            ("I", &[("ParamMouthOpenY", 0.35)]),
            ("U", &[("ParamMouthOpenY", 0.4)]),
            ("E", &[("ParamMouthOpenY", 0.6)]),
            ("O", &[("ParamMouthOpenY", 0.75)]),
        ],
    ),
];

/// A new setting from preset `p`, its parameters taken from the model
/// where they exist (name, range).
// TODO(Q-0039): the reference's new-setting ID scheme beyond the first.
pub fn new_setting(m: &Model, name: &str, p: usize) -> MotionSyncSetting {
    let (_, targets) = PRESETS[p.min(PRESETS.len() - 1)];
    let n = m.motion_sync.len();
    let id = if n == 0 {
        "MotionSyncSetting".to_owned()
    } else {
        (n + 1..)
            .map(|k| format!("MotionSyncSetting{k}"))
            .find(|id| m.motion_sync.iter().all(|s| &s.id != id))
            .unwrap_or_default()
    };
    let mut cubism: Vec<SyncCubismParam> = Vec::new();
    for (pid, _) in targets[0].1 {
        let q = m.parameters.iter().find(|q| q.id == *pid);
        cubism.push(SyncCubismParam {
            guid: keyform_model::new_guid(),
            parameter_id: (*pid).to_owned(),
            name: q.map_or_else(|| (*pid).to_owned(), |q| q.name.clone()),
            min: q.map_or(0.0, |q| q.min),
            max: q.map_or(1.0, |q| q.max),
            damper: 0.0,
            smooth: 20,
        });
    }
    MotionSyncSetting {
        guid: keyform_model::new_guid(),
        id,
        name: name.to_owned(),
        version: 1,
        sync_type: "CRI".into(),
        blend: 0.6,
        smoothing: 50,
        sample_rate: 30.0,
        cubism,
        audio: ELEMENTS
            .iter()
            .map(|e| SyncAudioParam {
                guid: NULL_GUID.into(),
                audio_id: (*e).into(),
                name: (*e).into(),
                min: 0.0,
                max: 1.0,
                scale: 1.0,
                enabled: true,
            })
            .collect(),
        mappings: targets
            .iter()
            .map(|(e, t)| SyncMapping {
                audio_id: (*e).into(),
                kind: "Shape".into(),
                targets: t.iter().map(|(p, v)| ((*p).to_owned(), *v)).collect(),
            })
            .collect(),
    }
}

pub fn dialog(
    ctx: &egui::Context,
    st: &mut MotionSyncDialog,
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
    run_preview(ctx, st, doc);
    let mut list = doc.model.motion_sync.clone();
    let before = list.clone();
    let mut open = true;
    egui::Window::new(crate::i18n::tr("Motion-sync Settings"))
        .default_size([620.0, 600.0])
        .open(&mut open)
        .show(ctx, |ui| {
            if doc.model.target_version != 0 && doc.model.target_version < MIN_TARGET {
                ui.label(
                    RichText::new(crate::i18n::tr(
                        "Motion-sync is only available in Cubism5.0 or later",
                    ))
                    .color(ui.visuals().warn_fg_color),
                );
                return;
            }
            audio_row(ui, st, doc, log);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Setting:"));
                let name = list
                    .get(st.setting)
                    .map_or(String::new(), |s| s.name.clone());
                egui::ComboBox::from_id_salt("ms-setting")
                    .selected_text(name)
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, s) in list.iter().enumerate() {
                            ui.selectable_value(&mut st.setting, i, &s.name);
                        }
                    });
                if ui.button(crate::i18n::tr("Add…")).clicked() {
                    st.add = Some((String::new(), 0));
                }
                ui.menu_button(crate::i18n::tr("Edit…"), |ui| {
                    if ui.button(crate::i18n::tr("Rename…")).clicked() {
                        st.rename = list.get(st.setting).map(|s| s.name.clone());
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("Duplicate")).clicked() {
                        if let Some(s) = list.get(st.setting).cloned() {
                            let mut m2 = doc.model.clone();
                            m2.motion_sync = list.clone();
                            let mut d = new_setting(&m2, &format!("{} copy", s.name), 0);
                            d.cubism = s
                                .cubism
                                .iter()
                                .map(|c| SyncCubismParam {
                                    guid: keyform_model::new_guid(),
                                    ..c.clone()
                                })
                                .collect();
                            d.audio = s.audio.clone();
                            d.mappings = s.mappings.clone();
                            d.blend = s.blend;
                            d.smoothing = s.smoothing;
                            d.sample_rate = s.sample_rate;
                            list.push(d);
                            st.setting = list.len() - 1;
                        }
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("Delete")).clicked() {
                        if st.setting < list.len() {
                            list.remove(st.setting);
                            st.setting = st.setting.saturating_sub(1);
                        }
                        ui.close();
                    }
                });
            });
            if let Some(mut name) = st.rename.take() {
                let mut done = false;
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Name:"));
                    ui.text_edit_singleline(&mut name);
                    if ui.button(crate::i18n::tr("OK")).clicked() {
                        if let Some(s) = list.get_mut(st.setting) {
                            s.name = name.clone();
                        }
                        done = true;
                    }
                    if ui.button(crate::i18n::tr("Cancel")).clicked() {
                        done = true;
                    }
                });
                if !done {
                    st.rename = Some(name);
                }
            }
            let Some(s) = list.get_mut(st.setting) else {
                ui.label(
                    RichText::new(crate::i18n::tr("Please add motion-sync settings."))
                        .color(ui.visuals().warn_fg_color),
                );
                return;
            };
            egui::Grid::new("ms-post").num_columns(2).show(ui, |ui| {
                ui.label(crate::i18n::tr("Sample Rate:"));
                ui.add(
                    egui::DragValue::new(&mut s.sample_rate)
                        .range(1.0..=120.0)
                        .speed(0.5),
                );
                ui.end_row();
                ui.label(crate::i18n::tr("Blending Ratio:"));
                ui.add(
                    egui::DragValue::new(&mut s.blend)
                        .range(0.0..=1.0)
                        .speed(0.01)
                        .fixed_decimals(2),
                );
                ui.end_row();
                ui.label(crate::i18n::tr("Smoothing:"));
                ui.add(egui::DragValue::new(&mut s.smoothing).range(0..=100));
                ui.end_row();
                ui.label(crate::i18n::tr("Audio analysis type:"));
                ui.label(crate::i18n::tr("Lip-sync (Keyform analyser)"));
                ui.end_row();
            });
            // Preset row: built-in presets, then the user's.
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Preset"));
                egui::ComboBox::from_id_salt("ms-preset-row")
                    .selected_text("—")
                    .show_ui(ui, |ui| {
                        for (i, (name, _)) in PRESETS.iter().enumerate() {
                            if ui.selectable_label(false, *name).clicked() {
                                let fresh = new_setting(&doc.model, &s.name, i);
                                UserPreset::from_setting("", &fresh).apply_to(s);
                            }
                        }
                        for p in &st.user_presets {
                            if ui.selectable_label(false, &p.name).clicked() {
                                p.apply_to(s);
                            }
                        }
                    });
                if ui.button(crate::i18n::tr("Add…")).clicked() {
                    st.preset_add = Some("Preset".into());
                }
                if ui.button(crate::i18n::tr("Edit…")).clicked() {
                    st.preset_list = Some((st.user_presets.clone(), None));
                }
            });
            if let Some(mut name) = st.preset_add.take() {
                let mut keep = true;
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Preset Name:"));
                    ui.text_edit_singleline(&mut name);
                    if ui.button(crate::i18n::tr("OK")).clicked() && !name.trim().is_empty() {
                        st.user_presets
                            .push(UserPreset::from_setting(name.trim(), s));
                        keep = false;
                    }
                    if ui.button(crate::i18n::tr("Cancel")).clicked() {
                        keep = false;
                    }
                });
                ui.weak(crate::i18n::tr(
                    "Saves the current setting as a new preset.",
                ));
                if keep {
                    st.preset_add = Some(name);
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut st.tab,
                    Tab::Mapping,
                    crate::i18n::tr("Mapping Settings"),
                );
                ui.selectable_value(&mut st.tab, Tab::Audio, crate::i18n::tr("Audio Elements"));
                ui.selectable_value(&mut st.tab, Tab::Parameter, crate::i18n::tr("Parameter"));
            });
            let mut missing = false;
            match st.tab {
                Tab::Mapping => mapping_tab(ui, st, &doc.model, &mut doc.values, s, &mut missing),
                Tab::Audio => audio_tab(ui, s),
                Tab::Parameter => parameter_tab(ui, &doc.model, s, &mut missing),
            }
            if missing {
                ui.label(
                    RichText::new(
                        "Settings that have no corresponding parameters in the model are ignored.\n\
                         Please check whether the parameters in red exist in the model.",
                    )
                    .color(ui.visuals().error_fg_color),
                );
            }
            if let Some(m) = &st.message {
                ui.label(m);
            }
        });
    if !open {
        stop_preview(st, doc);
        st.open = false;
    }
    add_dialog(ctx, st, &doc.model, &mut list);
    preset_list_window(ctx, st);
    // Live while a drag or text edit is in progress, one undo step after.
    if list != before {
        if st.base.is_none() {
            st.base = Some(before);
        }
        doc.model.motion_sync = list;
        doc.model_changed();
    }
    let busy = ctx.input(|i| i.pointer.any_down()) || ctx.memory(|m| m.focused().is_some());
    if !busy {
        if let Some(base) = st.base.take() {
            let new = std::mem::replace(&mut doc.model.motion_sync, base);
            doc.edit(STEP, |m| m.motion_sync = new);
        }
    }
}

fn exists(m: &Model, id: &str) -> bool {
    m.parameters.iter().any(|p| p.id == id)
}

fn mapping_tab(
    ui: &mut egui::Ui,
    st: &mut MotionSyncDialog,
    m: &Model,
    values: &mut [f32],
    s: &mut MotionSyncSetting,
    missing: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Audio Elements:"));
        let name = s
            .audio
            .get(st.element)
            .map_or(String::new(), |a| a.name.clone());
        egui::ComboBox::from_id_salt("ms-element")
            .selected_text(name)
            .show_ui(ui, |ui| {
                for (i, a) in s.audio.iter().enumerate() {
                    ui.selectable_value(&mut st.element, i, &a.name);
                }
            });
    });
    let Some(aid) = s.audio.get(st.element).map(|a| a.audio_id.clone()) else {
        return;
    };
    if !s.mappings.iter().any(|x| x.audio_id == aid) {
        s.mappings.push(SyncMapping {
            audio_id: aid.clone(),
            kind: "Shape".into(),
            targets: Vec::new(),
        });
    }
    // Apply: the model's current values become this element's targets;
    // Confirm: the targets are set on the model (observed, Q-0039).
    let (apply, confirm) = ui
        .horizontal(|ui| {
            (
                ui.button(crate::i18n::tr("Apply"))
                    .on_hover_text(crate::i18n::tr(
                        "Use the model's current parameter values as this element's shape",
                    ))
                    .clicked(),
                ui.button(crate::i18n::tr("Confirm"))
                    .on_hover_text(crate::i18n::tr("Show this element's shape on the model"))
                    .clicked(),
            )
        })
        .inner;
    if apply || confirm {
        if let Some(mp) = s.mappings.iter_mut().find(|x| x.audio_id == aid) {
            for c in &s.cubism {
                let Some(pi) = m.parameters.iter().position(|q| q.id == c.parameter_id) else {
                    continue;
                };
                if apply {
                    let v = values.get(pi).copied().unwrap_or(0.0);
                    match mp.targets.iter_mut().find(|t| t.0 == c.parameter_id) {
                        Some(t) => t.1 = v,
                        None => mp.targets.push((c.parameter_id.clone(), v)),
                    }
                } else if let (Some(t), Some(x)) = (
                    mp.targets.iter().find(|t| t.0 == c.parameter_id),
                    values.get_mut(pi),
                ) {
                    *x = t.1;
                }
            }
        }
    }
    let cubism = s.cubism.clone();
    let Some(mapping) = s.mappings.iter_mut().find(|x| x.audio_id == aid) else {
        return;
    };
    egui::Grid::new("ms-map")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ui.label(RichText::new(crate::i18n::tr("Parameter name")).strong());
            ui.label(RichText::new(crate::i18n::tr("Value")).strong());
            ui.end_row();
            for c in &cubism {
                let t = RichText::new(&c.name);
                if exists(m, &c.parameter_id) {
                    ui.label(t).on_hover_text(&c.parameter_id);
                } else {
                    *missing = true;
                    ui.label(t.color(ui.visuals().error_fg_color))
                        .on_hover_text(&c.parameter_id);
                }
                match mapping
                    .targets
                    .iter_mut()
                    .find(|(p, _)| *p == c.parameter_id)
                {
                    Some((_, v)) => {
                        ui.add(
                            egui::DragValue::new(v)
                                .range(c.min.min(c.max)..=c.max.max(c.min))
                                .speed(0.01)
                                .fixed_decimals(2),
                        );
                    }
                    None => {
                        if ui.small_button(crate::i18n::tr("Set")).clicked() {
                            mapping.targets.push((c.parameter_id.clone(), 0.0));
                        }
                    }
                }
                ui.end_row();
            }
        });
}

fn audio_tab(ui: &mut egui::Ui, s: &mut MotionSyncSetting) {
    egui::Grid::new("ms-audio")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            for h in ["Audio element", "Scale", "Enabled"] {
                ui.label(RichText::new(crate::i18n::tr(h)).strong());
            }
            ui.end_row();
            for a in &mut s.audio {
                ui.label(&a.name);
                ui.add(
                    egui::DragValue::new(&mut a.scale)
                        .range(0.0..=100.0)
                        .speed(0.05)
                        .fixed_decimals(2),
                );
                ui.checkbox(&mut a.enabled, crate::i18n::tr(""));
                ui.end_row();
            }
        });
}

fn parameter_tab(ui: &mut egui::Ui, m: &Model, s: &mut MotionSyncSetting, missing: &mut bool) {
    let mut remove = None;
    egui::Grid::new("ms-params")
        .num_columns(5)
        .striped(true)
        .show(ui, |ui| {
            for h in ["Parameter", "Damper", "Smooth", "", ""] {
                ui.label(RichText::new(crate::i18n::tr(h)).strong());
            }
            ui.end_row();
            for (k, c) in s.cubism.iter_mut().enumerate() {
                let t = RichText::new(&c.name);
                if exists(m, &c.parameter_id) {
                    ui.label(t).on_hover_text(&c.parameter_id);
                } else {
                    *missing = true;
                    ui.label(t.color(ui.visuals().error_fg_color))
                        .on_hover_text(&c.parameter_id);
                }
                ui.add(
                    egui::DragValue::new(&mut c.damper)
                        .range(0.0..=1.0)
                        .speed(0.001)
                        .fixed_decimals(3),
                );
                ui.add(egui::DragValue::new(&mut c.smooth).range(0..=100));
                ui.label(crate::i18n::tr(""));
                if ui.small_button(crate::i18n::tr("Delete")).clicked() {
                    remove = Some(k);
                }
                ui.end_row();
            }
        });
    if let Some(k) = remove {
        let id = s.cubism.remove(k).parameter_id;
        for x in &mut s.mappings {
            x.targets.retain(|(p, _)| *p != id);
        }
    }
    ui.menu_button(crate::i18n::tr("Add parameter…"), |ui| {
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for q in &m.parameters {
                    if s.cubism.iter().any(|c| c.parameter_id == q.id) {
                        continue;
                    }
                    if ui.button(&q.name).on_hover_text(&q.id).clicked() {
                        s.cubism.push(SyncCubismParam {
                            guid: keyform_model::new_guid(),
                            parameter_id: q.id.clone(),
                            name: q.name.clone(),
                            min: q.min,
                            max: q.max,
                            damper: 0.0,
                            smooth: 20,
                        });
                        // Every element gets a target (the default value).
                        for x in &mut s.mappings {
                            x.targets.push((q.id.clone(), q.default));
                        }
                        ui.close();
                    }
                }
            });
    });
}

fn add_dialog(
    ctx: &egui::Context,
    st: &mut MotionSyncDialog,
    m: &Model,
    list: &mut Vec<MotionSyncSetting>,
) {
    let Some((name, preset)) = &mut st.add else {
        return;
    };
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new(crate::i18n::tr("Add Motion-sync Settings"))
        .collapsible(false)
        .show(ctx, |ui| {
            egui::Grid::new("ms-add").num_columns(2).show(ui, |ui| {
                ui.label(crate::i18n::tr("Setting name:"));
                ui.text_edit_singleline(name);
                ui.end_row();
                ui.label(crate::i18n::tr("Audio analysis type:"));
                ui.label(crate::i18n::tr("Lip-sync"));
                ui.end_row();
                ui.label(crate::i18n::tr("Setting preset:"));
                egui::ComboBox::from_id_salt("ms-preset")
                    .selected_text(PRESETS[*preset].0)
                    .show_ui(ui, |ui| {
                        for (i, p) in PRESETS.iter().enumerate() {
                            ui.selectable_value(preset, i, p.0);
                        }
                    });
                ui.end_row();
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
        let mut m2 = m.clone();
        m2.motion_sync = list.clone();
        list.push(new_setting(&m2, name.trim(), *preset));
        st.setting = list.len() - 1;
        st.add = None;
    } else if cancel {
        st.add = None;
    }
}

/// Preset List: built-ins (read-only) and user presets (duplicate, rename,
/// delete). Layout as the reference's (observed, Q-0039).
fn preset_list_window(ctx: &egui::Context, st: &mut MotionSyncDialog) {
    let Some((list, sel)) = &mut st.preset_list else {
        return;
    };
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new(crate::i18n::tr("Preset List"))
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                egui::Grid::new("ms-presets")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        for h in ["No", "Preset Name", "Parameters"] {
                            ui.label(egui::RichText::new(crate::i18n::tr(h)).strong());
                        }
                        ui.end_row();
                        let n_builtin = PRESETS.len();
                        for (i, (name, t)) in PRESETS.iter().enumerate() {
                            ui.label((i + 1).to_string());
                            ui.weak(*name);
                            ui.label(t[0].1.len().to_string());
                            ui.end_row();
                        }
                        for (k, p) in list.iter_mut().enumerate() {
                            ui.label((n_builtin + k + 1).to_string());
                            if *sel == Some(k) {
                                ui.text_edit_singleline(&mut p.name);
                            } else if ui.selectable_label(false, &p.name).clicked() {
                                *sel = Some(k);
                            }
                            ui.label(p.cubism.len().to_string());
                            ui.end_row();
                        }
                    });
                ui.vertical(|ui| {
                    if ui
                        .add_enabled(
                            sel.is_some(),
                            egui::Button::new(crate::i18n::tr("Duplicate…")),
                        )
                        .clicked()
                    {
                        if let Some(p) = sel.and_then(|k| list.get(k)).cloned() {
                            list.push(UserPreset {
                                name: format!("{} copy", p.name),
                                ..p
                            });
                        }
                    }
                    if ui
                        .add_enabled(sel.is_some(), egui::Button::new(crate::i18n::tr("Delete")))
                        .clicked()
                    {
                        if let Some(k) = sel.take() {
                            list.remove(k);
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ok = ui.button(crate::i18n::tr("OK")).clicked();
                cancel = ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
    if ok {
        if let Some((l, _)) = st.preset_list.take() {
            st.user_presets = l;
        }
    } else if cancel {
        st.preset_list = None;
    }
}

/// "Load Audio…" and play / stop of the audio preview.
fn audio_row(ui: &mut egui::Ui, st: &mut MotionSyncDialog, doc: &mut Document, log: &mut Log) {
    ui.horizontal(|ui| {
        if ui.button(crate::i18n::tr("Load Audio…")).clicked() {
            if let Some(p) = crate::file_dialog::FileDialog::new()
                .add_filter("WAV", &["wav"])
                .pick_file()
            {
                match std::fs::read(&p)
                    .map_err(|e| e.to_string())
                    .and_then(|b| keyform_anim::audio::read_wav(&b).map_err(|e| e.to_string()))
                {
                    Ok(pcm) => {
                        st.pcm = Some(pcm);
                        st.audio_path = Some(p);
                    }
                    Err(e) => log.warn(format!("{}: {e}", p.display())),
                }
            }
        }
        ui.label(
            st.audio_path
                .as_ref()
                .map_or(String::new(), |p| p.display().to_string()),
        );
        let can = st.pcm.is_some() && doc.model.motion_sync.get(st.setting).is_some();
        if st.preview.is_some() {
            if ui.button(crate::i18n::tr("Stop")).clicked() {
                stop_preview(st, doc);
            }
        } else if ui
            .add_enabled(can, egui::Button::new(crate::i18n::tr("Play")))
            .clicked()
        {
            start_preview(st, doc);
        }
    });
}

fn start_preview(st: &mut MotionSyncDialog, doc: &mut Document) {
    let (Some(pcm), Some(s)) = (&st.pcm, doc.model.motion_sync.get(st.setting)) else {
        return;
    };
    let eng = keyform_anim::motionsync::Setting::from_model(s);
    let frames = eng.frames(pcm);
    let audio =
        crate::audio_out::AudioOut::play(std::sync::Arc::new(pcm.samples.clone()), pcm.rate, 0);
    if let Err(e) = &audio {
        st.message = Some(e.clone());
    }
    st.preview = Some(Preview {
        frames,
        rate: eng.sample_rate,
        started: Instant::now(),
        _audio: audio.ok(),
        saved: doc.values.clone(),
    });
}

fn stop_preview(st: &mut MotionSyncDialog, doc: &mut Document) {
    if let Some(p) = st.preview.take() {
        if p.saved.len() == doc.values.len() {
            doc.values = p.saved;
        }
    }
}

/// Drives the mouth parameters from the analysed rows at the audio
/// position (motion-sync.md §3 step 5).
fn run_preview(ctx: &egui::Context, st: &mut MotionSyncDialog, doc: &mut Document) {
    let Some(p) = &st.preview else { return };
    let t = p.started.elapsed().as_secs_f64();
    let x = t * f64::from(p.rate);
    let i0 = x.floor() as usize;
    if i0 + 1 >= p.frames.len() {
        stop_preview(st, doc);
        return;
    }
    let u = (x - x.floor()) as f32;
    for (id, a) in &p.frames[i0] {
        let b = p.frames[i0 + 1]
            .iter()
            .find(|(q, _)| q == id)
            .map_or(*a, |x| x.1);
        if let Some(pi) = doc.model.parameters.iter().position(|q| &q.id == id) {
            doc.values[pi] = a + (b - a) * u;
        }
    }
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_complete() {
        let m = Model::default();
        for p in 0..PRESETS.len() {
            let s = new_setting(&m, "x", p);
            assert_eq!(s.audio.len(), 6);
            // Every audio element has a mapping (the reference drops a
            // setting otherwise, Q-0039), with a target per parameter.
            assert_eq!(s.mappings.len(), 6);
            for x in &s.mappings {
                assert_eq!(x.targets.len(), s.cubism.len());
            }
            assert_eq!(s.id, "MotionSyncSetting");
        }
    }
}

#[cfg(test)]
mod doc_tests {
    use super::*;

    /// Adding a setting through the dialog's helper saves and reloads,
    /// and the runtime export then writes a motionsync3.json.
    #[test]
    fn new_setting_saves_and_exports() {
        let Some(dir) = std::env::var_os("KEYFORM_BB_P5") else {
            return;
        };
        let path = std::path::Path::new(&dir).join("bs.cmo3");
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        assert!(doc.model.motion_sync.is_empty());
        let s = new_setting(&doc.model, "Lips", 0);
        doc.edit(STEP, |m| m.motion_sync.push(s));
        assert_eq!(doc.motion_sync.len(), 1, "engine view follows the model");
        let tmp = std::env::temp_dir().join(format!("kf-ms-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let out = tmp.join("lips.cmo3");
        doc.save_project(Some(&out)).unwrap();
        let back = Document::open(&out, &mut log).unwrap();
        assert_eq!(back.model.motion_sync, doc.model.motion_sync);
        let v = keyform_formats::runtime_json::motionsync3_json(&back.model).unwrap();
        assert_eq!(v["Settings"][0]["Mappings"].as_array().unwrap().len(), 6);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod preset_tests {
    use super::*;

    #[test]
    fn user_preset_round_trip() {
        let m = Model::default();
        let mut a = new_setting(&m, "a", 0);
        a.blend = 0.3;
        a.mappings[1].targets[0].1 = 0.42;
        let p = UserPreset::from_setting("mine", &a);
        let mut b = new_setting(&m, "b", 1);
        let (guid, id) = (b.guid.clone(), b.id.clone());
        p.apply_to(&mut b);
        assert_eq!((b.guid, b.id, b.name.as_str()), (guid, id, "b"));
        assert_eq!(b.blend, 0.3);
        assert_eq!(b.mappings, a.mappings);
        assert_eq!(b.cubism.len(), a.cubism.len());
    }
}
