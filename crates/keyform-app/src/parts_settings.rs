//! Parts Settings dialog: Name and Part ID of every part in one table.
//! spec: 50-ui/palettes.md §1 "Parts Settings dialog", 50-ui/dialogs.md §2

use std::sync::Arc;

use eframe::egui;

use crate::document::Document;
use crate::log::Log;

/// One table row: part index, name, ID.
type Row = (usize, String, String);

#[derive(Default)]
pub struct PartsSettings {
    pub open: bool,
    rows: Vec<Row>,
    original: Vec<Row>,
    /// IDs taken over from other parts, waiting for confirmation.
    replaced: Option<Vec<String>>,
    /// Closing with unsaved changes: asking.
    ask_save: bool,
}

/// Validation messages for the rows ("No.%s %s contains invalid
/// characters.", `ID: "%s" is duplicated.`), in table order.
pub fn problems(rows: &[Row]) -> Vec<String> {
    let mut out = Vec::new();
    for (n, (_, _, id)) in rows.iter().enumerate() {
        if !keyform_edit::valid_id(id) {
            out.push(format!("No.{} {id} contains invalid characters.", n + 1));
        }
    }
    let mut seen: Vec<&str> = Vec::new();
    for (_, _, id) in rows {
        if seen.contains(&id.as_str()) {
            let msg = format!("ID: \"{id}\" is duplicated.");
            if !out.contains(&msg) {
                out.push(msg);
            }
        }
        seen.push(id);
    }
    out
}

/// IDs that now belong to a different part than before (§1 "The following
/// IDs were replaced with other parts…").
pub fn replaced_ids(original: &[Row], rows: &[Row]) -> Vec<String> {
    rows.iter()
        .filter(|(p, _, id)| original.iter().any(|(q, _, oid)| q != p && oid == id))
        .map(|(_, _, id)| id.clone())
        .collect()
}

impl PartsSettings {
    pub fn show(&mut self, doc: &Document) {
        let m = &doc.model;
        self.rows = (0..m.parts.len())
            .filter(|&p| Some(p) != m.root_part)
            .map(|p| (p, m.parts[p].info.name.clone(), m.parts[p].info.id.clone()))
            .collect();
        self.original = self.rows.clone();
        self.replaced = None;
        self.ask_save = false;
        self.open = true;
    }

    fn apply(&mut self, doc: &mut Document) {
        let rows = self.rows.clone();
        doc.edit("Part Settings", |m| {
            for (p, name, id) in &rows {
                let Some(x) = m.parts.get_mut(*p) else {
                    continue;
                };
                if x.info.name != *name || x.info.id != *id {
                    let x = Arc::make_mut(x);
                    x.info.name = name.clone();
                    x.info.id = id.clone();
                }
            }
        });
        self.open = false;
    }

    /// OK: validate, confirm replaced IDs, apply.
    fn ok(&mut self, doc: &mut Document, log: &mut Log) {
        let p = problems(&self.rows);
        if !p.is_empty() {
            for msg in p {
                log.warn(msg);
            }
            return;
        }
        let r = replaced_ids(&self.original, &self.rows);
        if r.is_empty() {
            self.apply(doc);
        } else {
            self.replaced = Some(r);
        }
    }
}

pub fn dialog(
    ctx: &egui::Context,
    st: &mut PartsSettings,
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
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new("Parts Settings")
        .collapsible(false)
        .default_height(420.0)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    egui::Grid::new("parts-settings")
                        .num_columns(3)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("No.");
                            ui.strong("Name");
                            ui.strong("Part ID");
                            ui.end_row();
                            for (n, (_, name, id)) in st.rows.iter_mut().enumerate() {
                                ui.label((n + 1).to_string());
                                ui.add(egui::TextEdit::singleline(name).desired_width(150.0));
                                let bad = !keyform_edit::valid_id(id);
                                let mut e = egui::TextEdit::singleline(id).desired_width(170.0);
                                if bad {
                                    e = e.text_color(ui.visuals().error_fg_color);
                                }
                                ui.add(e);
                                ui.end_row();
                            }
                        });
                });
            ui.weak("ID naming rules (hover)").on_hover_text(
                "Only letters, numbers and underscores; at most 63 characters; \
                     the first character must not be a number.",
            );
            ui.separator();
            ui.horizontal(|ui| {
                ok = ui.button("OK").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
    if ok {
        st.ok(doc, log);
    }
    if (cancel || !open) && !st.ask_save {
        if st.rows != st.original {
            st.ask_save = true;
        } else {
            st.open = false;
        }
    }
    if st.ask_save {
        let mut answer = None;
        egui::Modal::new(egui::Id::new("parts-settings-save")).show(ctx, |ui| {
            ui.label("Would you like to save the changes to your part settings?");
            ui.horizontal(|ui| {
                if ui.button("Yes").clicked() {
                    answer = Some(Some(true));
                }
                if ui.button("No").clicked() {
                    answer = Some(Some(false));
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(None);
                }
            });
        });
        match answer {
            Some(Some(true)) => {
                st.ask_save = false;
                st.ok(doc, log);
            }
            Some(Some(false)) => {
                st.ask_save = false;
                st.open = false;
            }
            Some(None) => st.ask_save = false,
            None => {}
        }
    }
    if let Some(r) = st.replaced.clone() {
        let mut answer = None;
        egui::Modal::new(egui::Id::new("parts-settings-replaced")).show(ctx, |ui| {
            ui.label("The following IDs were replaced with other parts:");
            for id in &r {
                ui.monospace(id);
            }
            ui.add_space(6.0);
            ui.collapsing("Notes on Replacing Part ID", |ui| {
                ui.label(
                    "Animations that refer to these part IDs will now affect the other \
                     parts, and data for the old IDs may no longer apply.",
                );
            });
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
        if let Some(yes) = answer {
            st.replaced = None;
            if yes {
                st.apply(doc);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_and_replaced_ids() {
        let rows = vec![
            (1, "A".into(), "PartA".into()),
            (2, "B".into(), "9bad".into()),
            (3, "C".into(), "PartA".into()),
        ];
        assert_eq!(
            problems(&rows),
            vec![
                "No.2 9bad contains invalid characters.".to_owned(),
                "ID: \"PartA\" is duplicated.".to_owned()
            ]
        );
        let original = vec![
            (1, "A".into(), "PartA".into()),
            (2, "B".into(), "PartB".into()),
        ];
        let swapped = vec![
            (1, "A".into(), "PartB".into()),
            (2, "B".into(), "PartA".into()),
        ];
        assert_eq!(
            replaced_ids(&original, &swapped),
            vec!["PartB".to_owned(), "PartA".to_owned()]
        );
        assert!(replaced_ids(&original, &original).is_empty());
    }
}
