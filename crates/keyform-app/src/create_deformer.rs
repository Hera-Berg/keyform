//! Create Warp Deformer / Create Rotation Deformer dialogs.
//! spec: 50-ui/dialogs.md §6–7

use eframe::egui;
use keyform_edit::objects::{create_rotation_deformer, create_warp_deformer, NewDeformer};

use crate::document::{Document, Selection};
use crate::log::Log;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Warp,
    Rotation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    /// Set as parent of selected object.
    ParentOfSelection,
    /// Set as child of selected deformer (or root without one).
    ChildOfSelected,
}

pub struct CreateDeformerState {
    pub open: Option<Kind>,
    name: String,
    placement: Placement,
    part: Option<usize>,
    cols: u32,
    rows: u32,
    bezier: (u32, u32),
    keep_open: bool,
}

impl Default for CreateDeformerState {
    fn default() -> Self {
        Self {
            open: None,
            name: String::new(),
            placement: Placement::ParentOfSelection,
            part: None,
            cols: 5,
            rows: 5,
            bezier: (2, 2),
            keep_open: false,
        }
    }
}

impl CreateDeformerState {
    /// Opens the dialog with the defaults for the current selection.
    pub fn show(&mut self, kind: Kind, doc: &Document) {
        self.open = Some(kind);
        self.name.clear();
        // Add To: the part of the first selected object (dialogs.md §6).
        let m = &doc.model;
        self.part = doc.selection.objects.first().and_then(|&o| match o {
            Selection::ArtMesh(i) => m.art_meshes.get(i)?.info.parent_part,
            Selection::Deformer(i) => m.deformers.get(i)?.info.parent_part,
            Selection::Part(i) => Some(i),
            Selection::Glue(i) => m.glues.get(i)?.info.parent_part,
        });
    }
}

fn parent_of(doc: &Document, o: Selection) -> Option<usize> {
    let m = &doc.model;
    match o {
        Selection::ArtMesh(i) => m.art_meshes.get(i).and_then(|a| a.target),
        Selection::Deformer(i) => m.deformers.get(i).and_then(|d| d.target),
        _ => None,
    }
}

pub fn dialog(
    ctx: &egui::Context,
    st: &mut CreateDeformerState,
    doc: Option<&mut Document>,
    log: &mut Log,
) {
    let Some(kind) = st.open else { return };
    let Some(doc) = doc else {
        st.open = None;
        return;
    };
    let title = match kind {
        Kind::Warp => "Create Warp Deformer",
        Kind::Rotation => "Create Rotation Deformer",
    };
    let mut open = true;
    let mut create = false;
    let mut close = false;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Grid::new("create-deformer")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label(crate::i18n::tr("Name"));
                    crate::icons::accessible_name(
                        &ui.add(
                            egui::TextEdit::singleline(&mut st.name).hint_text(match kind {
                                Kind::Warp => "Untitled Deformer",
                                Kind::Rotation => "Rotation Deformer",
                            }),
                        ),
                        crate::i18n::tr("Name"),
                    );
                    ui.end_row();
                    ui.label(crate::i18n::tr("Add To"));
                    let m = &doc.model;
                    let label = |p: Option<usize>| {
                        p.and_then(|p| m.parts.get(p))
                            .map_or("(root part)".to_owned(), |p| {
                                p.info.display_name().to_owned()
                            })
                    };
                    egui::ComboBox::from_id_salt("add-to")
                        .selected_text(label(st.part))
                        .show_ui(ui, |ui| {
                            for (i, p) in m.parts.iter().enumerate() {
                                ui.selectable_value(&mut st.part, Some(i), p.info.display_name());
                            }
                        });
                    ui.end_row();
                });
            ui.separator();
            ui.radio_value(
                &mut st.placement,
                Placement::ParentOfSelection,
                "Set as parent of selected object",
            );
            ui.radio_value(
                &mut st.placement,
                Placement::ChildOfSelected,
                "Set as child of selected deformer",
            );
            if kind == Kind::Warp {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Warp Division"));
                    ui.add(egui::DragValue::new(&mut st.cols).range(2..=100));
                    ui.label(crate::i18n::tr("×"));
                    ui.add(egui::DragValue::new(&mut st.rows).range(2..=100));
                });
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Bezier Division"));
                    ui.add(egui::DragValue::new(&mut st.bezier.0).range(1..=100));
                    ui.label(crate::i18n::tr("×"));
                    ui.add(egui::DragValue::new(&mut st.bezier.1).range(1..=100));
                });
                if st.bezier.0 > 9 || st.bezier.1 > 9 {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Over 9*9 Curve Deformer divisions might impact performance.",
                    );
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.keep_open, crate::i18n::tr("Continuous creation"));
                create = ui.button(crate::i18n::tr("Create")).clicked();
                close = ui.button(crate::i18n::tr("Close")).clicked();
            });
        });
    if !open || close {
        st.open = None;
    }
    if !create {
        return;
    }
    // Placement → parent deformer and adopted children.
    let selected = doc.selection.objects.clone();
    let (parent, children): (Option<usize>, Vec<Selection>) = match st.placement {
        Placement::ParentOfSelection => {
            let kids: Vec<Selection> = selected
                .iter()
                .copied()
                .filter(|o| matches!(o, Selection::ArtMesh(_) | Selection::Deformer(_)))
                .collect();
            (kids.first().and_then(|&o| parent_of(doc, o)), kids)
        }
        Placement::ChildOfSelected => {
            let defs: Vec<usize> = selected
                .iter()
                .filter_map(|o| match o {
                    Selection::Deformer(d) => Some(*d),
                    _ => None,
                })
                .collect();
            if defs.len() > 1 {
                log.warn("Parent Deformer is not selected.");
                return;
            }
            (defs.first().copied(), Vec::new())
        }
    };
    let name = if st.name.trim().is_empty() {
        match kind {
            Kind::Warp => "Untitled Deformer".to_owned(),
            Kind::Rotation => "Rotation Deformer".to_owned(),
        }
    } else {
        st.name.trim().to_owned()
    };
    let values = doc.values.clone();
    let (cols, rows, bezier, part) = (st.cols, st.rows, st.bezier, st.part);
    let result = doc.try_edit(title, |m| {
        let nd = NewDeformer {
            name,
            part,
            parent,
            children: &children,
            values: &values,
        };
        match kind {
            Kind::Warp => create_warp_deformer(m, &nd, cols, rows, bezier),
            Kind::Rotation => create_rotation_deformer(m, &nd),
        }
    });
    match result {
        Ok(d) => {
            doc.select(Some(Selection::Deformer(d)));
            if !st.keep_open {
                st.open = None;
            }
        }
        Err(e) => log.warn(e),
    }
}
