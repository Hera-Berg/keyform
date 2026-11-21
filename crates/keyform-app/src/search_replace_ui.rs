//! Edit ▸ Search and Replace… [Ctrl+F] (dialogs.md §10): a filtered table
//! of objects and parameters; names or IDs of the selected rows are
//! replaced as one undo step.

use std::collections::HashSet;

use eframe::egui;
use keyform_edit::search_replace::{self as sr, Kind};

use crate::document::{Document, SelectionSet};

pub struct SearchReplace {
    pub open: bool,
    condition: String,
    in_name: bool,
    in_id: bool,
    case: bool,
    kinds: HashSet<Kind>,
    /// Selected rows: (kind, index).
    selected: HashSet<(Kind, usize)>,
    sync: bool,
    /// Synchronize from canvas to dialog (else from dialog to canvas).
    from_canvas: bool,
    before: String,
    after: String,
    target_id: bool,
    replace_case: bool,
    message: Option<String>,
}

impl Default for SearchReplace {
    fn default() -> Self {
        Self {
            open: false,
            condition: String::new(),
            in_name: true,
            in_id: true,
            case: false,
            kinds: Kind::ALL.into_iter().collect(),
            selected: HashSet::new(),
            sync: false,
            from_canvas: true,
            before: String::new(),
            after: String::new(),
            target_id: false,
            replace_case: false,
            message: None,
        }
    }
}

pub fn window(ctx: &egui::Context, st: &mut SearchReplace, doc: Option<&mut Document>) {
    if !st.open {
        return;
    }
    let Some(doc) = doc.filter(|d| d.editable()) else {
        st.open = false;
        return;
    };
    let rows: Vec<sr::Item> = sr::items(&doc.model)
        .into_iter()
        .filter(|r| st.kinds.contains(&r.kind))
        .filter(|r| sr::matches(r, &st.condition, st.in_name, st.in_id, st.case))
        .collect();
    if st.sync && st.from_canvas {
        st.selected = rows
            .iter()
            .filter(|r| {
                r.object()
                    .is_some_and(|o| doc.selection.objects.contains(&o))
            })
            .map(|r| (r.kind, r.index))
            .collect();
    }
    let mut open = true;
    let mut replace = false;
    let mut changed_selection = false;
    egui::Window::new(crate::i18n::tr("Search and Replace"))
        .default_size([520.0, 640.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Condition"));
                let r = ui.add(
                    egui::TextEdit::singleline(&mut st.condition).desired_width(f32::INFINITY),
                );
                crate::icons::accessible_name(&r, crate::i18n::tr("Condition"));
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.in_name, crate::i18n::tr("Name"));
                ui.checkbox(&mut st.in_id, crate::i18n::tr("ID"));
                ui.checkbox(
                    &mut st.case,
                    crate::i18n::tr("Distinction between capital and small letters"),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(crate::i18n::tr("Type"));
                for k in Kind::ALL {
                    let mut on = st.kinds.contains(&k);
                    if ui
                        .toggle_value(&mut on, crate::i18n::tr(k.label()))
                        .changed()
                    {
                        if on {
                            st.kinds.insert(k);
                        } else {
                            st.kinds.remove(&k);
                        }
                    }
                }
                if ui.button(crate::i18n::tr("All On")).clicked() {
                    st.kinds = Kind::ALL.into_iter().collect();
                }
                if ui.button(crate::i18n::tr("All Off")).clicked() {
                    st.kinds.clear();
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    egui::Grid::new("sr-table")
                        .num_columns(4)
                        .striped(true)
                        .show(ui, |ui| {
                            for h in ["Parent Part", "Type", "Name", "ID"] {
                                ui.strong(crate::i18n::tr(h));
                            }
                            ui.end_row();
                            for r in &rows {
                                let key = (r.kind, r.index);
                                let sel = st.selected.contains(&key);
                                ui.weak(&r.parent_part);
                                ui.label(crate::i18n::tr(r.kind.label()));
                                let shift = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                                if ui.selectable_label(sel, &r.name).clicked() {
                                    if !shift {
                                        st.selected.clear();
                                    }
                                    if sel && shift {
                                        st.selected.remove(&key);
                                    } else {
                                        st.selected.insert(key);
                                    }
                                    changed_selection = true;
                                }
                                ui.label(&r.id);
                                ui.end_row();
                            }
                        });
                });
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Select all")).clicked() {
                    st.selected = rows.iter().map(|r| (r.kind, r.index)).collect();
                    changed_selection = true;
                }
            });
            ui.checkbox(
                &mut st.sync,
                crate::i18n::tr("Synchronize selection with canvas"),
            );
            ui.add_enabled_ui(st.sync, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut st.from_canvas,
                        true,
                        crate::i18n::tr("Synchronize from canvas to dialog"),
                    );
                    ui.radio_value(
                        &mut st.from_canvas,
                        false,
                        crate::i18n::tr("Synchronize from dialog to canvas"),
                    );
                });
            });
            ui.separator();
            ui.label(crate::i18n::tr(
                "Replace the Name/ID for the selected items in the table",
            ));
            egui::Grid::new("sr-replace").num_columns(3).show(ui, |ui| {
                ui.label(crate::i18n::tr("Before replacement"));
                let r = ui.text_edit_singleline(&mut st.before);
                crate::icons::accessible_name(&r, crate::i18n::tr("Before replacement"));
                if ui
                    .button("⇅")
                    .on_hover_text(crate::i18n::tr("Swap before and after"))
                    .clicked()
                {
                    std::mem::swap(&mut st.before, &mut st.after);
                }
                ui.end_row();
                ui.label(crate::i18n::tr("After replacement"));
                let r = ui.text_edit_singleline(&mut st.after);
                crate::icons::accessible_name(&r, crate::i18n::tr("After replacement"));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Target"));
                ui.radio_value(&mut st.target_id, false, crate::i18n::tr("Name"));
                ui.radio_value(&mut st.target_id, true, crate::i18n::tr("ID"));
                ui.checkbox(
                    &mut st.replace_case,
                    crate::i18n::tr("Distinction between capital and small letters"),
                );
                replace = ui
                    .add_enabled(
                        !st.selected.is_empty() && !st.before.is_empty(),
                        egui::Button::new(crate::i18n::tr("Replace")),
                    )
                    .clicked();
            });
            ui.label(match &st.message {
                Some(m) => m.clone(),
                None => format!("{} are found", rows.len()),
            });
        });
    if changed_selection {
        st.message = None;
        if st.sync && !st.from_canvas {
            let mut set = SelectionSet::of(None);
            set.objects = rows
                .iter()
                .filter(|r| st.selected.contains(&(r.kind, r.index)))
                .filter_map(|r| r.object())
                .collect();
            doc.set_selection(set);
        }
    }
    if replace {
        let chosen: Vec<sr::Item> = rows
            .iter()
            .filter(|r| st.selected.contains(&(r.kind, r.index)))
            .cloned()
            .collect();
        let (before, after, target, case) = (
            st.before.clone(),
            st.after.clone(),
            st.target_id,
            st.replace_case,
        );
        st.message = Some(
            match doc.try_edit("Search and Replace", |m| {
                sr::replace(m, &chosen, &before, &after, target, case)
            }) {
                Ok(n) => format!("{n} are replaced"),
                Err(e) => e,
            },
        );
    }
    if !open {
        st.open = false;
    }
}
