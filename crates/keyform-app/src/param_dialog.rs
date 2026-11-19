//! New / Edit parameter dialog and parameter deletion.
//! spec: 50-ui/palettes.md §3 ("New/Edit parameter dialog", "Delete
//! parameter"), 50-ui/dialogs.md §2 (ID rules)

use eframe::egui;
use keyform_model::{Parameter, ParameterKind};

use crate::document::{Document, Selection};
use crate::log::Log;

/// What a palette asked for (handled by the app).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Edit(usize),
    /// Create in this group (top level if `None`).
    Create(Option<usize>),
    Delete(usize),
}

#[derive(Default)]
pub struct ParamDialog {
    /// Open, editing parameter `Some(p)` or creating (`None`).
    pub open: Option<Option<usize>>,
    /// New parameters go into this group.
    group: Option<usize>,
    name: String,
    id: String,
    min: f32,
    default: f32,
    max: f32,
    repeat: bool,
    blend: bool,
    description: String,
    /// Waiting for "Would you like to continue?" about out-of-range keys.
    confirm_range: bool,
    /// Parameter waiting for the delete confirmation.
    pub confirm_delete: Option<usize>,
}

impl ParamDialog {
    pub fn edit(&mut self, doc: &Document, p: usize) {
        let Some(x) = doc.model.parameters.get(p) else {
            return;
        };
        self.open = Some(Some(p));
        self.name.clone_from(&x.name);
        self.id.clone_from(&x.id);
        self.min = x.min;
        self.default = x.default;
        self.max = x.max;
        self.repeat = x.repeat;
        self.blend = x.kind == ParameterKind::BlendShape;
        self.description.clone_from(&x.description);
        self.confirm_range = false;
    }

    pub fn create(&mut self, doc: &Document, group: Option<usize>) {
        let id = keyform_edit::objects::unique_id(
            "Param",
            doc.model.parameters.iter().map(|p| p.id.as_str()),
        );
        self.open = Some(None);
        self.group = group;
        self.name = id.clone();
        self.id = id;
        (self.min, self.default, self.max) = (0.0, 0.0, 1.0);
        self.repeat = false;
        self.blend = false;
        self.description.clear();
        self.confirm_range = false;
    }
}

/// The objects bound to parameter `p`.
fn bound_objects(doc: &Document, p: usize) -> Vec<Selection> {
    let m = &doc.model;
    (0..m.parts.len())
        .map(Selection::Part)
        .chain((0..m.deformers.len()).map(Selection::Deformer))
        .chain((0..m.art_meshes.len()).map(Selection::ArtMesh))
        .chain((0..m.glues.len()).map(Selection::Glue))
        .filter(|&o| !keyform_edit::keys_of(m, o, p).is_empty())
        .collect()
}

/// Validation messages (palettes.md §3); `None` when the input is usable.
fn problem(doc: &Document, st: &ParamDialog) -> Option<String> {
    if !keyform_edit::valid_id(&st.id) {
        return Some("Invalid ID".into());
    }
    let me = st.open.flatten();
    if doc
        .model
        .parameters
        .iter()
        .enumerate()
        .any(|(i, p)| Some(i) != me && p.id == st.id)
    {
        return Some(format!("ID: \"{}\" is duplicated.", st.id));
    }
    let no = me.map_or(doc.model.parameters.len() + 1, |p| p + 1);
    if st.min.partial_cmp(&st.max) != Some(std::cmp::Ordering::Less) {
        return Some(format!(
            "No.{no} needs to have a minimum value smaller than the maximum value."
        ));
    }
    if !(st.min..=st.max).contains(&st.default) {
        return Some(format!("No.{no} has a default value that is out of range."));
    }
    if st.repeat {
        if let Some(p) = me {
            let m = &doc.model;
            if m.eye_blink_parameters.contains(&p) || m.lip_sync_parameters.contains(&p) {
                return Some(
                    "Parameters that have blinking or lip-sync settings cannot be repeated.".into(),
                );
            }
        }
    }
    None
}

pub fn dialog(
    ctx: &egui::Context,
    st: &mut ParamDialog,
    doc: Option<&mut Document>,
    log: &mut Log,
) {
    let Some(doc) = doc else {
        st.open = None;
        st.confirm_delete = None;
        return;
    };
    match doc.param_request.take() {
        Some(Request::Edit(p)) => st.edit(doc, p),
        Some(Request::Create(g)) => st.create(doc, g),
        Some(Request::Delete(p)) => st.confirm_delete = Some(p),
        None => {}
    }
    delete_confirmation(ctx, st, doc, log);
    let Some(editing) = st.open else { return };
    let title = if editing.is_some() {
        "Edit Parameter"
    } else {
        "Create Parameter"
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    let issue = problem(doc, st);
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Grid::new("param-dialog").num_columns(2).show(ui, |ui| {
                ui.label(crate::i18n::tr("Name"));
                ui.text_edit_singleline(&mut st.name);
                ui.end_row();
                ui.label(crate::i18n::tr("ID"));
                ui.text_edit_singleline(&mut st.id)
                    .on_hover_text(crate::i18n::tr("Letters, numbers and underscores; up to 63 characters; not starting with a number."));
                ui.end_row();
                ui.label(crate::i18n::tr("Minimum"));
                ui.add(egui::DragValue::new(&mut st.min).speed(0.1));
                ui.end_row();
                ui.label(crate::i18n::tr("Default"));
                ui.add(egui::DragValue::new(&mut st.default).speed(0.1));
                ui.end_row();
                ui.label(crate::i18n::tr("Maximum"));
                ui.add(egui::DragValue::new(&mut st.max).speed(0.1));
                ui.end_row();
                ui.label(crate::i18n::tr("Repeat"));
                ui.checkbox(&mut st.repeat, crate::i18n::tr(""));
                ui.end_row();
                ui.label(crate::i18n::tr("Blend Shape"));
                ui.add_enabled(
                    editing.is_none(),
                    egui::Checkbox::without_text(&mut st.blend),
                );
                ui.end_row();
                ui.label(crate::i18n::tr("Description"));
                ui.text_edit_multiline(&mut st.description);
                ui.end_row();
            });
            if st.blend && doc.model.target_version != 0 && doc.model.target_version < 4_020_000 {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Blend Shape can only be used in Cubism4.2 or later.",
                );
            }
            if let Some(p) = &issue {
                ui.colored_label(ui.visuals().error_fg_color, p);
            }
            if st.confirm_range {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!(
                        "Some keyforms are out of range of {}. Would you like to continue?",
                        st.name
                    ),
                );
            }
            ui.separator();
            ui.horizontal(|ui| {
                ok = ui
                    .add_enabled(issue.is_none(), egui::Button::new(if st.confirm_range { "Continue" } else { "OK" }))
                    .clicked();
                cancel = ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
    if !open || cancel {
        st.open = None;
        return;
    }
    if !ok {
        return;
    }
    // Keys outside a narrowed range: ask once.
    if let Some(p) = editing {
        let outside = bound_objects(doc, p).iter().any(|&o| {
            keyform_edit::keys_of(&doc.model, o, p)
                .iter()
                .any(|&k| k < st.min || k > st.max)
        });
        if outside && !st.confirm_range {
            st.confirm_range = true;
            return;
        }
    }
    let (name, id, description) = (st.name.clone(), st.id.clone(), st.description.clone());
    let (min, default, max, repeat, blend, group) =
        (st.min, st.default, st.max, st.repeat, st.blend, st.group);
    let result = match editing {
        Some(p) => {
            let objects = bound_objects(doc, p);
            let was_repeat = doc.model.parameters[p].repeat;
            doc.try_edit("Edit Parameter", |m| {
                let x = std::sync::Arc::make_mut(&mut m.parameters[p]);
                x.name = name;
                x.id = id;
                x.description = description;
                (x.min, x.default, x.max, x.repeat) = (min, default, max, repeat);
                // Repeat on: every bound object gets min and max keys.
                if repeat && !was_repeat {
                    for &o in &objects {
                        for v in [min, max] {
                            keyform_edit::add_key(m, o, p, v).map_err(|e| e.to_string())?;
                        }
                    }
                }
                Ok(())
            })
            .map(|()| {
                if repeat && !was_repeat {
                    log.info(
                        "Automatically add maximum and minimum key value when Repeat is activated.",
                    );
                }
            })
        }
        None => doc
            .try_edit("Create Parameter", |m| {
                let mut x = Parameter::new(&id, min, max, default);
                x.name = name;
                x.description = description;
                x.repeat = repeat;
                if blend {
                    x.kind = ParameterKind::BlendShape;
                }
                Ok(keyform_edit::objects::add_parameter(m, x, group))
            })
            .map(|_| ()),
    };
    match result {
        Ok(()) => st.open = None,
        Err(e) => log.warn(e),
    }
}

fn delete_confirmation(
    ctx: &egui::Context,
    st: &mut ParamDialog,
    doc: &mut Document,
    log: &mut Log,
) {
    let Some(p) = st.confirm_delete else { return };
    let Some(param) = doc.model.parameters.get(p).cloned() else {
        st.confirm_delete = None;
        return;
    };
    let physics = doc.model.physics.settings.iter().any(|s| {
        s.inputs.iter().any(|i| i.parameter == p) || s.outputs.iter().any(|o| o.parameter == p)
    });
    let mut choice = None;
    egui::Modal::new(egui::Id::new("delete-parameter")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.label(format!("Would you like to delete {}?", param.name));
        if physics {
            ui.label(crate::i18n::tr(
                "Related physics setting will also be deleted.",
            ));
        }
        ui.horizontal(|ui| {
            if ui.button(crate::i18n::tr("OK")).clicked() {
                choice = Some(true);
            }
            if ui.button(crate::i18n::tr("Cancel")).clicked() {
                choice = Some(false);
            }
        });
    });
    match choice {
        Some(true) => {
            st.confirm_delete = None;
            if let Err(e) = doc.try_edit("Delete Parameter", |m| {
                if keyform_edit::objects::remove_parameter(m, p) {
                    Ok(())
                } else {
                    Err(crate::i18n::tr(keyform_edit::objects::PARAMETER_USED_BY_STORED).into())
                }
            }) {
                log.warn(e);
            }
        }
        Some(false) => st.confirm_delete = None,
        None => {}
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::path::Path;

    #[test]
    fn validation_messages() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../hiyori_pro_t11.cmo3");
        if !path.exists() {
            return;
        }
        let mut log = Log::default();
        let doc = Document::open(&path, &mut log).unwrap();
        let mut st = ParamDialog::default();
        st.create(&doc, None);
        assert_eq!(problem(&doc, &st), None, "defaults are valid ({})", st.id);
        st.id = "1bad".into();
        assert_eq!(problem(&doc, &st).as_deref(), Some("Invalid ID"));
        st.id = doc.model.parameters[0].id.clone();
        assert!(problem(&doc, &st).unwrap().contains("is duplicated"));
        st.id = "ParamNew".into();
        st.max = st.min;
        assert!(problem(&doc, &st)
            .unwrap()
            .contains("minimum value smaller"));
        st.max = 1.0;
        st.default = 2.0;
        assert!(problem(&doc, &st).unwrap().contains("out of range"));
        // Repeat is refused on eye-blink parameters.
        if let Some(&p) = doc.model.eye_blink_parameters.first() {
            st.edit(&doc, p);
            assert_eq!(problem(&doc, &st), None);
            st.repeat = true;
            assert!(problem(&doc, &st).unwrap().contains("cannot be repeated"));
        }
    }
}
