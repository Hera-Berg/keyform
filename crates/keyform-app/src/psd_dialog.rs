//! PSD import dialogs: "Select an action or a model to integrate PSD" and,
//! for an open model, "Select an action or an existing PSD file to
//! replace". spec: 70-advanced/psd-import.md §1–2, §6

use std::path::{Path, PathBuf};

use eframe::egui;
use keyform_formats::psd::Psd;
use keyform_formats::psd_import::{add_psd_layers, register_psd, ImportOptions};

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
    match p.target {
        None => {
            egui::Window::new("Select an action or a model to integrate PSD")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "{name}: {} × {} px, {} layers",
                        p.size.0, p.size.1, p.layers
                    ));
                    ui.separator();
                    for label in [
                        "< Create new model from PSD file >",
                        "< Create new model from PSD file (Blend mode before 5.3) >",
                    ] {
                        ui.add_enabled(false, egui::Button::selectable(false, label))
                            .on_disabled_hover_text("Creating a new model is not available yet.");
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
                    if ui.button("Cancel").clicked() {
                        outcome = Outcome::Cancel;
                    }
                });
        }
        Some(uid) => {
            let Some((_, doc)) = docs.iter_mut().find(|(u, _)| *u == uid) else {
                return Outcome::Cancel;
            };
            let mut ok = false;
            egui::Window::new("Select an action or an existing PSD file to replace")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(format!("{name} → {}", doc.title));
                    ui.separator();
                    ui.checkbox(&mut p.add_layers, "< Add all layers as new ArtMesh >");
                    ui.checkbox(
                        &mut p.register_only,
                        "< Register PSD file without adding ArtMeshes >",
                    );
                    // Existing PSDs, newest first (replacing comes later).
                    for li in doc.model.layered_images.iter().rev() {
                        let label = format!(
                            "{}{}",
                            li.name,
                            if li.replaced { " (replaced)" } else { "" }
                        );
                        ui.add_enabled(false, egui::Checkbox::new(&mut false, label))
                            .on_disabled_hover_text("Replacing a PSD is not available yet.");
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ok = ui
                            .add_enabled(p.add_layers || p.register_only, egui::Button::new("OK"))
                            .clicked();
                        if ui.button("Cancel").clicked() {
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
                // "Register" wins over "add all layers" (§2).
                let register = p.register_only;
                let r = doc.try_edit("Import PSD", |m| {
                    let psd = Psd::parse(data).map_err(|e| e.to_string())?;
                    if register {
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
    if !open {
        outcome = Outcome::Cancel;
    }
    outcome
}
