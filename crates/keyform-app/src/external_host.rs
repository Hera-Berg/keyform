//! The editor side of the external application API: documents, models,
//! parameters and pushed values, plugin approval, and the settings window.
//! spec: 70-advanced/external-api.md §3–§5

use eframe::egui;
use keyform_model::Model;

use crate::app::Slot;
use crate::document::Mode;
use crate::external_api::{ApiError, DocInfo, DocKind, Host, ParamInfo};
use crate::log::Log;

/// Registered plugins: (name, token, approved). Approved ones are
/// remembered across sessions.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Plugins {
    pub list: Vec<(String, String, bool)>,
}

pub struct AppHost<'a> {
    pub slots: &'a mut [Slot],
    pub active: Option<usize>,
    pub log: &'a mut Log,
    pub plugins: &'a mut Plugins,
}

fn doc_uid(s: &Slot) -> String {
    format!("doc-{}", s.uid)
}

fn model_uid(s: &Slot) -> String {
    format!("model-{}", s.uid)
}

/// Sorted key values of `p` over every object bound to it (1.0.1 `Keyform`).
fn keys_of(m: &Model, p: usize) -> Vec<f32> {
    let mut k: Vec<f32> = Vec::new();
    let mut add = |b: &[keyform_model::Binding]| {
        for x in b.iter().filter(|x| x.parameter == p) {
            k.extend_from_slice(&x.keys);
        }
    };
    m.parts.iter().for_each(|x| add(&x.grid.bindings));
    m.art_meshes.iter().for_each(|x| add(&x.grid.bindings));
    m.glues.iter().for_each(|x| add(&x.grid.bindings));
    for d in &m.deformers {
        match &d.kind {
            keyform_model::DeformerKind::Warp(w) => add(&w.grid.bindings),
            keyform_model::DeformerKind::Rotation(r) => add(&r.grid.bindings),
        }
    }
    k.sort_by(f32::total_cmp);
    k.dedup();
    k
}

impl AppHost<'_> {
    fn slot_for(&self, model: Option<&str>, doc: Option<&str>) -> Result<usize, ApiError> {
        if let Some(m) = model {
            return self
                .slots
                .iter()
                .position(|s| model_uid(s) == m)
                .ok_or(ApiError::InvalidModel);
        }
        if let Some(d) = doc {
            return self
                .slots
                .iter()
                .position(|s| doc_uid(s) == d)
                .ok_or(ApiError::InvalidDocument);
        }
        self.active
            .filter(|&i| i < self.slots.len())
            .ok_or(ApiError::InvalidModel)
    }

    fn checked(
        &self,
        model: Option<&str>,
        doc: Option<&str>,
        animation: bool,
    ) -> Result<usize, ApiError> {
        let i = self.slot_for(model, doc)?;
        if !animation && self.slots[i].doc.mode == Mode::Animation {
            return Err(ApiError::InvalidModel);
        }
        Ok(i)
    }
}

impl Host for AppHost<'_> {
    fn documents(&self) -> Vec<DocInfo> {
        self.slots
            .iter()
            .map(|s| {
                let path = s.doc.path.display().to_string();
                let anim = s.doc.mode == Mode::Animation;
                DocInfo {
                    uid: doc_uid(s),
                    kind: if anim {
                        DocKind::Animation
                    } else {
                        DocKind::Modeling
                    },
                    path: path.clone(),
                    views: vec![(model_uid(s), anim.then_some(path))],
                }
            })
            .collect()
    }

    fn current_document(&self) -> Option<String> {
        self.active.and_then(|i| self.slots.get(i)).map(doc_uid)
    }

    fn current_model(&self, animation: bool) -> Option<String> {
        let s = self.active.and_then(|i| self.slots.get(i))?;
        (animation || s.doc.mode != Mode::Animation).then(|| model_uid(s))
    }

    fn edit_mode(&self) -> &'static str {
        match self.active.and_then(|i| self.slots.get(i)) {
            Some(s) if s.doc.mesh_edit.is_some() => "ModelingMeshEdit",
            Some(s) if s.doc.mode == Mode::Animation => {
                if s.doc.anim.as_ref().is_some_and(|a| a.form_tool) {
                    "FormAnimation"
                } else {
                    "Animation"
                }
            }
            _ => "Modeling",
        }
    }

    fn parameters(
        &self,
        model: Option<&str>,
        doc: Option<&str>,
        animation: bool,
    ) -> Result<Vec<ParamInfo>, ApiError> {
        let m = &self.slots[self.checked(model, doc, animation)?].doc.model;
        Ok(m.parameters
            .iter()
            .enumerate()
            .map(|(i, p)| ParamInfo {
                id: p.id.clone(),
                name: p.name.clone(),
                group_uid: p
                    .group
                    .and_then(|g| m.parameter_groups.get(g))
                    .map_or(String::new(), |g| g.guid.clone()),
                default: p.default,
                max: p.max,
                min: p.min,
                repeat: p.repeat,
                kind: u8::from(p.kind == keyform_model::ParameterKind::BlendShape),
                keys: keys_of(m, i),
            })
            .collect())
    }

    fn groups(
        &self,
        model: Option<&str>,
        doc: Option<&str>,
        animation: bool,
    ) -> Result<Vec<(String, String)>, ApiError> {
        let m = &self.slots[self.checked(model, doc, animation)?].doc.model;
        Ok(m.parameter_groups
            .iter()
            .map(|g| (g.guid.clone(), g.name.clone()))
            .collect())
    }

    fn values(&self, model: &str) -> Result<Vec<(String, f32)>, ApiError> {
        let d = &self.slots[self.slot_for(Some(model), None)?].doc;
        Ok(d.model
            .parameters
            .iter()
            .zip(&d.live)
            .map(|(p, v)| (p.id.clone(), *v))
            .collect())
    }

    fn set_values(&mut self, model: &str, values: &[(String, f32)]) -> Result<(), ApiError> {
        let i = self.slot_for(Some(model), None)?;
        let d = &mut self.slots[i].doc;
        if d.mesh_edit.is_some() {
            return Err(ApiError::InvalidView);
        }
        for (id, v) in values {
            let Some(p) = d.model.parameters.iter().position(|q| &q.id == id) else {
                continue;
            };
            match d.external.iter_mut().find(|e| e.0 == p) {
                Some(e) => e.1 = *v,
                None => d.external.push((p, *v)),
            }
        }
        Ok(())
    }

    fn clear_values(&mut self, model: &str) -> Result<(), ApiError> {
        let i = self.slot_for(Some(model), None)?;
        self.slots[i].doc.external.clear();
        Ok(())
    }

    fn log(&mut self, warning: bool, message: &str, _display: bool) {
        let m = format!("[external] {message}");
        if warning {
            self.log.warn(m);
        } else {
            self.log.info(m);
        }
    }

    fn register(
        &mut self,
        name: &str,
        token: Option<&str>,
        _path: Option<&str>,
        _icon: Option<&str>,
    ) -> (String, bool) {
        // TODO(Q-0042): icon validation and display.
        if let Some(t) = token {
            if let Some(e) = self.plugins.list.iter().find(|e| e.1 == t) {
                return (t.to_owned(), e.2);
            }
        }
        let t = keyform_model::new_guid();
        self.plugins.list.push((name.to_owned(), t.clone(), false));
        self.log.info(format!("External application \"{name}\" asks for access (Show ▸ External Application Integration…)."));
        (t, false)
    }

    fn approved(&self, token: &str) -> bool {
        self.plugins.list.iter().any(|e| e.1 == token && e.2)
    }
}

/// The settings window: enable (off by default; 127.0.0.1 only), port,
/// and the registered applications with approval.
pub fn settings_window(
    ctx: &egui::Context,
    open: &mut bool,
    enabled: &mut bool,
    port: &mut u16,
    running: Option<(u16, usize)>,
    plugins: &mut Plugins,
) {
    if !*open {
        return;
    }
    egui::Window::new(crate::i18n::tr("External Application Integration Settings"))
        .open(open)
        .show(ctx, |ui| {
            ui.checkbox(enabled, "Enable external application integration");
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Port (127.0.0.1 only):"));
                ui.add_enabled(!*enabled, egui::DragValue::new(port).range(1024..=65535));
            });
            match running {
                Some((p, n)) => ui.label(format!("Listening on 127.0.0.1:{p} — {n} connected")),
                None => ui.weak(crate::i18n::tr("Not running")),
            };
            ui.separator();
            let mut remove = None;
            egui::Grid::new("ext-apps")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    for (i, (name, _, ok)) in plugins.list.iter_mut().enumerate() {
                        ui.label(name.as_str());
                        ui.checkbox(ok, "Approved");
                        if ui.small_button(crate::i18n::tr("Remove")).clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
            if let Some(i) = remove {
                plugins.list.remove(i);
            }
        });
}

#[cfg(test)]
mod tests {
    use crate::document::Document;
    use crate::log::Log;

    /// Pushed values override the slider values until cleared.
    #[test]
    fn pushed_values_override_sliders() {
        let Some(dir) = std::env::var_os("KEYFORM_BB_P5") else {
            return;
        };
        let mut log = Log::default();
        let mut doc =
            Document::open(&std::path::Path::new(&dir).join("bs.cmo3"), &mut log).unwrap();
        let p = doc
            .model
            .parameters
            .iter()
            .position(|q| q.id == "ParamAngleX")
            .unwrap();
        doc.external.push((p, 17.0));
        doc.tick(0.0, &Default::default(), &mut Log::default());
        assert_eq!(doc.live[p], 17.0);
        assert_eq!(doc.values[p], 0.0, "the slider value is kept");
        doc.external.clear();
        doc.tick(0.0, &Default::default(), &mut Log::default());
        assert_eq!(doc.live[p], 0.0);
    }
}
