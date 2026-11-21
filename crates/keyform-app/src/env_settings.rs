//! File ▸ Environment Settings…: application settings by category.
//! spec: 50-ui/dialogs.md §11, defaults in 50-ui/settings-keys.md
//!
//! Only settings Keyform has behind them are listed; the others in §11
//! (auto backup, view-area colours, object styles, …) arrive with their
//! features (docs/reachability.md, "Spec menu coverage").

use std::path::PathBuf;

use eframe::egui;

use crate::app::Settings;

const CATEGORIES: [&str; 4] = ["General", "File", "Modeling", "Notification"];

#[derive(Default)]
pub struct EnvDialog {
    pub open: bool,
    /// The settings being edited (applied with OK).
    edit: Option<Settings>,
    category: usize,
    /// Closed with unsaved changes: "Settings have been changed…".
    confirm: bool,
}

impl EnvDialog {
    pub fn open(&mut self, current: &Settings) {
        self.open = true;
        self.edit = Some(*current);
        self.confirm = false;
    }
}

/// What the dialog did besides editing the settings.
#[derive(Default)]
pub struct EnvActions {
    pub reset_dialogs: bool,
    pub clear_recent: bool,
    pub redisplay_dialogs: bool,
}

pub fn show(
    ctx: &egui::Context,
    st: &mut EnvDialog,
    settings: &mut Settings,
    recent: &[PathBuf],
) -> EnvActions {
    let mut act = EnvActions::default();
    if !st.open {
        return act;
    }
    let Some(mut edit) = st.edit else {
        st.open = false;
        return act;
    };
    let (mut ok, mut cancel) = (false, false);
    let mut open = true;
    egui::Window::new(crate::i18n::tr("Environment Settings"))
        .default_size([560.0, 360.0])
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(130.0);
                    for (i, c) in CATEGORIES.iter().enumerate() {
                        ui.selectable_value(&mut st.category, i, crate::i18n::tr(c));
                    }
                });
                ui.separator();
                ui.vertical(|ui| match st.category {
                    0 => general(ui, &mut edit, &mut act),
                    1 => file(ui, &mut edit, &mut act, recent),
                    2 => modeling(ui, &mut edit),
                    _ => notification(ui, &mut edit, &mut act),
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ok = ui.button(crate::i18n::tr("OK")).clicked();
                cancel = ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
    if st.confirm {
        egui::Window::new(crate::i18n::tr("Confirm"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(crate::i18n::tr(
                    "Settings have been changed. Do you want to save the changes?",
                ));
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("Yes")).clicked() {
                        ok = true;
                    }
                    if ui.button(crate::i18n::tr("No")).clicked() {
                        cancel = true;
                    }
                });
            });
    }
    if !open && !ok && !cancel {
        if edit != *settings {
            st.confirm = true;
        } else {
            cancel = true;
        }
    }
    if ok {
        *settings = edit;
    }
    if ok || cancel {
        st.open = false;
        st.edit = None;
        st.confirm = false;
    } else {
        st.edit = Some(edit);
    }
    act
}

fn general(ui: &mut egui::Ui, s: &mut Settings, act: &mut EnvActions) {
    egui::Grid::new("env-general")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(crate::i18n::tr("Color Theme"));
            egui::ComboBox::from_id_salt("env-theme")
                .selected_text(crate::i18n::tr(if s.dark_theme { "Dark" } else { "Light" }))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.dark_theme, false, crate::i18n::tr("Light"));
                    ui.selectable_value(&mut s.dark_theme, true, crate::i18n::tr("Dark"));
                });
            ui.end_row();
            ui.label(crate::i18n::tr("Language"));
            egui::ComboBox::from_id_salt("env-language")
                .selected_text(s.lang.native_name())
                .show_ui(ui, |ui| {
                    for l in crate::i18n::Lang::ALL {
                        ui.selectable_value(&mut s.lang, l, l.native_name());
                    }
                });
            ui.end_row();
        });
    if ui
        .button(crate::i18n::tr("Reset Dialog Size and Position"))
        .clicked()
    {
        act.reset_dialogs = true;
    }
}

fn file(ui: &mut egui::Ui, s: &mut Settings, act: &mut EnvActions, recent: &[PathBuf]) {
    ui.checkbox(
        &mut s.recent_full_path,
        crate::i18n::tr("Show recent files with full paths"),
    );
    if ui
        .add_enabled(
            !recent.is_empty(),
            egui::Button::new(crate::i18n::tr("Clear recent files")),
        )
        .clicked()
    {
        act.clear_recent = true;
    }
}

fn modeling(ui: &mut egui::Ui, s: &mut Settings) {
    ui.checkbox(
        &mut s.mesh_edit_double_click,
        crate::i18n::tr("Double-click an ArtMesh to start editing its mesh"),
    );
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Rotation deformer angle increment (Shift)"));
        ui.add(
            egui::DragValue::new(&mut s.rotation_step)
                .range(1.0..=180.0)
                .speed(1.0)
                .suffix("°"),
        );
    });
    ui.checkbox(
        &mut s.paste_numbered_names,
        crate::i18n::tr("Add a number to the names of pasted objects"),
    );
}

fn notification(ui: &mut egui::Ui, s: &mut Settings, act: &mut EnvActions) {
    ui.checkbox(
        &mut s.mask_warnings,
        crate::i18n::tr("Always show warnings regarding masks"),
    );
    if ui
        .button(crate::i18n::tr("Redisplay all dialogs that are hidden"))
        .clicked()
    {
        act.redisplay_dialogs = true;
    }
}
