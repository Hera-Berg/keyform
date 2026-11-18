//! Generator dialogs: Auto Generation of Sway Motion, and Set Random Pose.
//! spec: 70-advanced/generators.md §2, §4

use eframe::egui;
use keyform_edit::sway::{Pivot, SwaySettings, SwaySide};
use keyform_model::DeformerKind;

use crate::document::{Document, KeyOp, Selection};
use crate::log::Log;

#[derive(Default)]
pub struct SwayDialog {
    pub open: bool,
    param: Option<usize>,
    settings: SwaySettings,
    sync: bool,
    auto: bool,
    /// (deformer, parameter) whose stored setting is shown.
    loaded: Option<(usize, usize)>,
    message: Option<String>,
}

impl SwayDialog {
    pub fn show(&mut self) {
        self.open = true;
        self.sync = true;
    }
}

fn side_ui(ui: &mut egui::Ui, salt: &str, s: &mut SwaySide) {
    egui::Grid::new(salt).num_columns(2).show(ui, |ui| {
        let row = |ui: &mut egui::Ui, label: &str, v: &mut f32, r: (f32, f32)| {
            ui.label(label);
            ui.add(egui::Slider::new(v, r.0..=r.1).step_by(0.1));
            ui.end_row();
        };
        row(ui, "Sway level (horizontal) %", &mut s.sx, SwaySide::SX);
        row(ui, "Sway level (vertical) %", &mut s.sy, SwaySide::SY);
        row(ui, "Softness", &mut s.softness, SwaySide::SOFTNESS);
        row(ui, "Enlarge/shrink", &mut s.enlarge, SwaySide::ENLARGE);
    });
}

/// The selected warp deformer, if exactly one is the primary selection.
fn selected_warp(doc: &Document) -> Option<usize> {
    match doc.primary()? {
        Selection::Deformer(d) => {
            matches!(doc.model.deformers.get(d)?.kind, DeformerKind::Warp(_)).then_some(d)
        }
        _ => None,
    }
}

pub fn sway_dialog(
    ctx: &egui::Context,
    st: &mut SwayDialog,
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
    let mut run = false;
    let before = st.settings;
    egui::Window::new(crate::i18n::tr("Auto Generation of Sway Motion"))
        .default_width(420.0)
        .open(&mut open)
        .show(ctx, |ui| {
            // TODO(Q-0040): settings are not stored with the deformer
            // (`CAutoYureConfigExtension`) yet.
            let Some(d) = selected_warp(doc) else {
                ui.label(crate::i18n::tr("Select a warp deformer."));
                return;
            };
            ui.label(format!("Deformer: {}", doc.model.deformers[d].info.name));
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Parameter:"));
                let name = st
                    .param
                    .and_then(|p| doc.model.parameters.get(p))
                    .map_or("—".to_owned(), |p| p.name.clone());
                egui::ComboBox::from_id_salt("sway-param")
                    .selected_text(name)
                    .height(320.0)
                    .show_ui(ui, |ui| {
                        for (i, q) in doc.model.parameters.iter().enumerate() {
                            ui.selectable_value(&mut st.param, Some(i), &q.name)
                                .on_hover_text(&q.id);
                        }
                    });
            });
            let Some(p) = st.param else { return };
            // Reopen with the deformer's stored setting (generators.md §2).
            if st.loaded != Some((d, p)) {
                st.loaded = Some((d, p));
                if let Some(c) = keyform_edit::sway::stored(&doc.model, d, p) {
                    st.settings = SwaySettings::from_config(&c);
                    st.sync = c.sync;
                }
            }
            let keys = keyform_edit::keys_of(&doc.model, Selection::Deformer(d), p);
            if keys.len() != 3 {
                ui.label(crate::i18n::tr(
                    "The parameter needs exactly 3 keys (minimum, default, maximum).",
                ));
                if ui
                    .button(crate::i18n::tr("Delete the existing keys and add 3 keys"))
                    .clicked()
                {
                    doc.select(Some(Selection::Deformer(d)));
                    let _ = doc.key_op(KeyOp::DeleteAll, p);
                    if let Err(e) = doc.key_op(KeyOp::AddThree, p) {
                        log.warn(e);
                    }
                }
                return;
            }
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Pivot position:"));
                for (v, l) in [
                    (Pivot::Top, "Top"),
                    (Pivot::Right, "Right"),
                    (Pivot::Bottom, "Bottom"),
                    (Pivot::Left, "Left"),
                ] {
                    ui.radio_value(&mut st.settings.pivot, v, l);
                }
            });
            ui.checkbox(
                &mut st.settings.flip,
                crate::i18n::tr("Flip sway horizontally"),
            );
            ui.checkbox(
                &mut st.sync,
                "Synchronize left and right side setting values",
            );
            ui.label(egui::RichText::new(crate::i18n::tr("Left side keyform")).strong());
            side_ui(ui, "sway-left", &mut st.settings.left);
            if st.sync {
                st.settings.right = st.settings.left;
            } else {
                ui.label(egui::RichText::new(crate::i18n::tr("Right side keyform")).strong());
                side_ui(ui, "sway-right", &mut st.settings.right);
            }
            ui.horizontal(|ui| {
                run = ui.button(crate::i18n::tr("Update keyform")).clicked();
                ui.checkbox(
                    &mut st.auto,
                    "Automatically update the keyform when the setting value changes",
                );
            });
            if let Some(m) = &st.message {
                ui.label(m);
            }
            if st.auto && st.settings != before {
                run = true;
            }
            if run {
                let s = st.settings;
                let mut err = None;
                let sync = st.sync;
                doc.edit("Auto Generation of Sway Motion", |m| {
                    if let Err(e) = keyform_edit::sway::generate(m, d, p, &s) {
                        err = Some(e);
                    }
                    keyform_edit::sway::store(m, d, s.to_config(p, sync));
                });
                st.message = err;
            }
        });
    if !open {
        st.open = false;
    }
}

/// Set Random Pose: every enabled, non-blend-shape parameter gets a random
/// value in its range. Keyforms are not changed. spec: generators.md §4
// Patterns A/B, Posing and Apply Opacity wait (Q-0040).
pub fn random_pose(doc: &mut Document, seed: u64) {
    let mut x = seed | 1;
    let mut next = || {
        // xorshift64*
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    };
    for (i, p) in doc.model.parameters.iter().enumerate() {
        // Blend-shape parameters and those switched off in the model's
        // random-pose setting are skipped (Q-0040).
        if p.kind == keyform_model::ParameterKind::BlendShape
            || doc.model.random_pose_off.contains(&i)
        {
            continue;
        }
        let (lo, hi) = (p.min.min(p.max), p.max.max(p.min));
        if let Some(v) = doc.values.get_mut(i) {
            *v = lo + (hi - lo) * next() as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sway on a sample warp deformer: 3 keys, left / right keyforms
    /// written, base untouched, one undo step; random pose stays in range.
    #[test]
    fn sway_and_random_pose_on_a_sample() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dirs = std::env::var("KEYFORM_SAMPLES").unwrap_or_else(|_| root.display().to_string());
        let Some(path) = std::env::split_paths(&dirs)
            .map(|d| d.join("hiyori_free_t08.cmo3"))
            .find(|p| p.exists())
        else {
            return;
        };
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        // A warp deformer under the root with few bindings.
        let d = doc
            .model
            .deformers
            .iter()
            .position(|x| matches!(x.kind, DeformerKind::Warp(_)) && x.target.is_none())
            .unwrap();
        let p = doc
            .model
            .parameters
            .iter()
            .position(|q| q.id == "ParamAngleZ")
            .unwrap();
        doc.select(Some(Selection::Deformer(d)));
        let _ = doc.key_op(KeyOp::DeleteAll, p);
        doc.key_op(KeyOp::AddThree, p).unwrap();
        let before = doc.model.clone();
        let s = SwaySettings::default();
        assert!(doc.edit("Auto Generation of Sway Motion", |m| {
            keyform_edit::sway::generate(m, d, p, &s).unwrap();
        }));
        let DeformerKind::Warp(a) = &before.deformers[d].kind else {
            panic!()
        };
        let DeformerKind::Warp(b) = &doc.model.deformers[d].kind else {
            panic!()
        };
        let changed = a
            .grid
            .keyforms
            .iter()
            .zip(&b.grid.keyforms)
            .filter(|(x, y)| x.positions != y.positions)
            .count();
        // Left and right keyforms of every combination of other keys.
        assert!(changed >= 2, "{changed}");
        random_pose(&mut doc, 42);
        for (q, v) in doc.model.parameters.iter().zip(&doc.values) {
            assert!(*v >= q.min.min(q.max) - 1e-4 && *v <= q.max.max(q.min) + 1e-4);
        }
    }
}
