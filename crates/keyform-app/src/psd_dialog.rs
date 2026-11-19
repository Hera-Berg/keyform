//! PSD import dialogs: "Select an action or a model to integrate PSD" and,
//! for an open model, "Select an action or an existing PSD file to
//! replace". spec: 70-advanced/psd-import.md §1–2, §6

use std::path::{Path, PathBuf};

use eframe::egui;
use keyform_formats::cmo3::Cmo3Document;
use keyform_formats::psd::Psd;
use keyform_formats::psd_import::{
    add_psd_layers, new_model_from_psd, register_psd, ImportOptions,
};

use crate::document::Document;
use crate::log::Log;

/// A PSD waiting for the user's choice.
pub struct PendingPsd {
    pub path: PathBuf,
    data: Vec<u8>,
    size: (u32, u32),
    layers: usize,
    /// Dialog 2 is open for this document (slot uid).
    target: Option<u64>,
    add_layers: bool,
    register_only: bool,
    /// Layered images (by index) chosen to be replaced (§7).
    replace: Vec<usize>,
    /// New model: the preview resolution prompt (legacy blend flag,
    /// choices, selected index). spec: psd-import.md §3.1
    resolution: Option<(bool, Vec<keyform_formats::psd_import::PreviewChoice>, usize)>,
}

/// Reads and checks a dropped/opened PSD. spec: psd-import.md §1, §3
pub fn read(path: &Path) -> Result<PendingPsd, String> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("psb"))
    {
        return Err("PSB File loading is not supported.".into());
    }
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (size, layers) = {
        let psd = Psd::parse(&data).map_err(|e| e.to_string())?;
        ((psd.width, psd.height), psd.layers().len())
    };
    Ok(PendingPsd {
        path: path.to_path_buf(),
        data,
        size,
        layers,
        target: None,
        add_layers: true,
        register_only: false,
        replace: Vec::new(),
        resolution: None,
    })
}

fn millis(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// What the dialogs decided this frame.
pub enum Outcome {
    None,
    Cancel,
    /// Imported into the document with this uid.
    Done(u64),
    /// A new model (suggested path next to the PSD).
    NewModel(PathBuf, Box<Cmo3Document>),
}

/// Shows the open dialog. `docs` lists (uid, title, editable model).
pub fn dialog(
    ctx: &egui::Context,
    p: &mut PendingPsd,
    docs: &mut [(u64, &mut Document)],
    log: &mut Log,
) -> Outcome {
    let name = p
        .path
        .file_name()
        .map_or("PSD".into(), |n| n.to_string_lossy().to_string());
    let mut outcome = Outcome::None;
    let mut open = true;
    let mut new_model = None;
    // The preview resolution prompt of a new model.
    if let Some((legacy, choices, sel)) = p.resolution.as_mut() {
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new(crate::i18n::tr("Confirm"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(crate::i18n::tr(
                    "Please select the resolution for displaying the model image",
                ));
                for (k, c) in choices.iter().enumerate() {
                    ui.radio_value(sel, k, &c.label);
                }
                ui.horizontal(|ui| {
                    ok = ui.button(crate::i18n::tr("OK")).clicked();
                    cancel = ui.button(crate::i18n::tr("Cancel")).clicked();
                });
            });
        if !ok && !cancel && open {
            return Outcome::None;
        }
        // Cancel or closing returns no choice: Full Scale, and the model
        // is still made (no abort). spec: psd-import.md §3.1
        let factor = if ok { choices[*sel].factor } else { 1 };
        let legacy = *legacy;
        p.resolution = None;
        return make_new_model(p, &name, legacy, factor, log);
    }
    match p.target {
        None => {
            egui::Window::new(crate::i18n::tr(
                "Select an action or a model to integrate PSD",
            ))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(format!(
                    "{name}: {} × {} px, {} layers",
                    p.size.0, p.size.1, p.layers
                ));
                ui.separator();
                for (label, legacy) in [
                    ("< Create new model from PSD file >", false),
                    (
                        "< Create new model from PSD file (Blend mode before 5.3) >",
                        true,
                    ),
                ] {
                    if ui.add(egui::Button::selectable(false, label)).clicked() {
                        new_model = Some(legacy);
                    }
                }
                for (uid, d) in docs.iter() {
                    let ok = d.editable();
                    let b = egui::Button::selectable(
                        false,
                        egui::RichText::new(&d.title).color(ui.visuals().hyperlink_color),
                    );
                    if ui.add_enabled(ok, b).clicked() {
                        p.target = Some(*uid);
                    }
                }
                ui.separator();
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    outcome = Outcome::Cancel;
                }
            });
        }
        Some(uid) => {
            let Some((_, doc)) = docs.iter_mut().find(|(u, _)| *u == uid) else {
                return Outcome::Cancel;
            };
            let mut ok = false;
            egui::Window::new(crate::i18n::tr(
                "Select an action or an existing PSD file to replace",
            ))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(format!("{name} → {}", doc.title));
                ui.separator();
                ui.checkbox(
                    &mut p.add_layers,
                    crate::i18n::tr("< Add all layers as new ArtMesh >"),
                );
                ui.checkbox(
                    &mut p.register_only,
                    crate::i18n::tr("< Register PSD file without adding ArtMeshes >"),
                );
                // Existing PSDs, newest first: Replace (§7).
                for (k, li) in doc.model.layered_images.iter().enumerate().rev() {
                    let label = format!(
                        "Replace [ {} ]{}",
                        li.name,
                        if li.replaced { " (replaced)" } else { "" }
                    );
                    let mut on = p.replace.contains(&k);
                    if ui.checkbox(&mut on, label).changed() {
                        if on {
                            p.replace.push(k);
                            p.add_layers = false;
                            p.register_only = false;
                        } else {
                            p.replace.retain(|x| *x != k);
                        }
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ok = ui
                        .add_enabled(
                            p.add_layers || p.register_only || !p.replace.is_empty(),
                            egui::Button::new(crate::i18n::tr("OK")),
                        )
                        .clicked();
                    if ui.button(crate::i18n::tr("Cancel")).clicked() {
                        outcome = Outcome::Cancel;
                    }
                });
            });
            if ok {
                let o = ImportOptions {
                    path: p.path.to_string_lossy().to_string(),
                    modified_ms: std::fs::metadata(&p.path)
                        .and_then(|m| m.modified())
                        .map_or(0, millis),
                    imported_ms: millis(std::time::SystemTime::now()),
                    ..ImportOptions::default()
                };
                let data = &p.data;
                // "Register" wins over "add all layers" (§2); a chosen
                // PSD to replace wins over both (§7).
                let register = p.register_only;
                let replace = p.replace.clone();
                let r = doc.try_edit("Import PSD", |m| {
                    let psd = Psd::parse(data).map_err(|e| e.to_string())?;
                    if !replace.is_empty() {
                        let r = keyform_formats::psd_replace::replace_psd(m, &replace, &name, &psd, &o)
                            .map_err(|e| e.to_string())?;
                        let mut notes = vec![format!(
                            "{} layers paired, {} model images relinked.",
                            r.pairs.len(),
                            r.relinked.len()
                        )];
                        if !r.import.art_meshes.is_empty() {
                            notes.push(format!(
                                "{} layers without a corresponding layer were added as new ArtMeshes.",
                                r.import.art_meshes.len()
                            ));
                        }
                        notes.extend(r.import.warnings);
                        Ok(notes)
                    } else if register {
                        register_psd(m, &name, &psd, &o).map_err(|e| e.to_string())?;
                        Ok(Vec::new())
                    } else {
                        let r = add_psd_layers(m, &name, &psd, &o).map_err(|e| e.to_string())?;
                        Ok(r.warnings)
                    }
                });
                match r {
                    Ok(warnings) => {
                        for w in warnings {
                            log.warn(w);
                        }
                        log.info(format!("Imported {name}."));
                        outcome = Outcome::Done(uid);
                    }
                    Err(e) => {
                        log.error(e);
                        outcome = Outcome::Cancel;
                    }
                }
            }
        }
    }
    if let Some(legacy) = new_model {
        // spec: psd-import.md §3.1: ask only when Full Scale is too large.
        let choices =
            Psd::parse(&p.data).map(|psd| keyform_formats::psd_import::preview_choices(&psd));
        return match choices {
            Ok((choices, pre, true)) => {
                p.resolution = Some((legacy, choices, pre));
                Outcome::None
            }
            Ok((_, _, false)) => make_new_model(p, &name, legacy, 1, log),
            Err(e) => {
                log.error(e.to_string());
                Outcome::Cancel
            }
        };
    }
    if !open {
        outcome = Outcome::Cancel;
    }
    outcome
}

/// A new model from the PSD (spec: psd-import.md §4) with preview
/// reduction `factor`.
fn make_new_model(p: &PendingPsd, name: &str, legacy: bool, factor: u32, log: &mut Log) -> Outcome {
    let o = ImportOptions {
        legacy_blend: legacy,
        path: p.path.to_string_lossy().to_string(),
        modified_ms: std::fs::metadata(&p.path)
            .and_then(|m| m.modified())
            .map_or(0, millis),
        imported_ms: millis(std::time::SystemTime::now()),
        preview_reduction: factor,
        ..ImportOptions::default()
    };
    let made = Psd::parse(&p.data)
        .map_err(|e| e.to_string())
        .and_then(|psd| new_model_from_psd(name, &psd, &o).map_err(|e| e.to_string()));
    match made {
        Ok((doc, report)) => {
            for w in report.warnings {
                log.warn(w);
            }
            log.info(format!(
                "New model from {name}: {} art meshes.",
                report.art_meshes.len()
            ));
            Outcome::NewModel(p.path.with_extension("cmo3"), Box::new(doc))
        }
        Err(e) => {
            log.error(e);
            Outcome::Cancel
        }
    }
}
