//! Palettes. spec: 50-ui/palettes.md (§1 Parts, §2 Deformer, §3 Parameter,
//! §4 Inspector, §9 Log), 70-advanced/viewer.md §2 (model settings)

use eframe::egui::{self, Color32, RichText, Sense};
use keyform_model::{DeformerKind, Model, ObjectInfo, ParamEntry, PartChild};

use crate::document::{Document, KeyOp, Selection};
use crate::icons::{self, Icon};
use crate::log::{Level, Log};
use crate::theme::Tokens;

fn selectable(ui: &mut egui::Ui, selected: bool, text: impl Into<egui::WidgetText>) -> bool {
    ui.selectable_label(selected, text).clicked()
}

/// The Draw (eye) column; clicking it toggles visibility when `editable`.
fn eye(ui: &mut egui::Ui, visible: bool, editable: bool) -> bool {
    let r = icons::icon(ui, if visible { Icon::Visible } else { Icon::Hidden });
    editable && r.interact(Sense::click()).clicked()
}

/// A Parts palette row being dragged.
#[derive(Clone, Copy)]
struct DragRow(Selection);

/// A selectable row label that can be dragged (when `dnd`) and reports a
/// row dropped on it.
fn row_label(
    ui: &mut egui::Ui,
    selected: bool,
    text: &str,
    me: Selection,
    dnd: bool,
    into_part: bool,
) -> (egui::Response, Option<(Selection, bool)>) {
    let sense = if dnd {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let r = ui.add(egui::Button::selectable(selected, text).sense(sense));
    if dnd && r.drag_started() {
        egui::DragAndDrop::set_payload(ui.ctx(), DragRow(me));
    }
    let mut dropped = None;
    if dnd {
        // On a part row, the top third means "before it", the rest "into".
        let before = !into_part
            || ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|q| q.y < r.rect.top() + r.rect.height() / 3.0);
        if let Some(p) = r.dnd_hover_payload::<DragRow>() {
            if p.0 != me {
                let st = egui::Stroke::new(1.5, ui.visuals().selection.stroke.color);
                if before {
                    ui.painter().hline(r.rect.x_range(), r.rect.top(), st);
                } else {
                    ui.painter()
                        .rect_stroke(r.rect, 2.0, st, egui::StrokeKind::Inside);
                }
            }
        }
        if let Some(p) = r.dnd_release_payload::<DragRow>() {
            if p.0 != me {
                dropped = Some((p.0, before));
            }
        }
    }
    (r, dropped)
}

/// The Lock column: locked objects can't be selected on the canvas.
fn lock(ui: &mut egui::Ui, locked: bool, editable: bool) -> bool {
    let r = icons::icon(ui, if locked { Icon::Locked } else { Icon::Unlocked });
    editable && r.interact(Sense::click()).clicked()
}

/// Bulk operations of the Parts palette menus.
#[derive(Clone, Copy, PartialEq)]
enum PartsBulk {
    ShowAll(bool),
    LockAll(bool),
    DeleteEmpty,
}

fn parts_menu(ui: &mut egui::Ui, bulk: &mut Option<PartsBulk>) {
    for (label, b) in [
        ("Show all", PartsBulk::ShowAll(true)),
        ("Hide all", PartsBulk::ShowAll(false)),
        ("Lock all", PartsBulk::LockAll(true)),
        ("Unlock all", PartsBulk::LockAll(false)),
        ("Delete empty parts", PartsBulk::DeleteEmpty),
    ] {
        if ui.button(label).clicked() {
            *bulk = Some(b);
            ui.close();
        }
    }
}

// ---------------------------------------------------------------- Parts

/// Parts palette: part tree with art meshes (and, when toggled, deformers
/// and glue); columns eye, name, draw order. spec: palettes.md §1
pub fn parts_ui(
    ui: &mut egui::Ui,
    doc: &mut Document,
    show_deformers: &mut bool,
    filter: &mut String,
    dnd_disabled: &mut bool,
) -> bool {
    let mut open_settings = false;
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text("Filter by Text")
                .desired_width(ui.available_width() - 60.0),
        );
        ui.toggle_value(show_deformers, "D")
            .on_hover_text(crate::i18n::tr(
                "Show/hide Warp Deformers and Rotation Deformers",
            ));
        ui.menu_button(crate::i18n::tr("☰"), |ui| {
            if ui
                .button(crate::i18n::tr("New part"))
                .on_hover_text(crate::i18n::tr("Create new part"))
                .clicked()
            {
                let parent = match doc.primary() {
                    Some(Selection::Part(p)) => Some(p),
                    _ => None,
                };
                let r = doc.try_edit("New Part", |m| {
                    Ok(keyform_edit::objects::create_part(m, "New Part", parent))
                });
                if let Ok(p) = r {
                    doc.select(Some(Selection::Part(p)));
                }
                ui.close();
            }
            if ui.button(crate::i18n::tr("Part Settings…")).clicked() {
                open_settings = true;
                ui.close();
            }
            ui.checkbox(dnd_disabled, "Disable drag & drop");
            let mut bulk = None;
            parts_menu(ui, &mut bulk);
            if let Some(b) = bulk {
                parts_bulk(doc, b);
            }
        })
        .response
        .on_hover_text(crate::i18n::tr("Show palette menu"));
    });
    ui.separator();
    let Some(root) = doc.model.root_part else {
        ui.label(crate::i18n::tr("(no part tree)"));
        return open_settings;
    };
    let filter = filter.to_lowercase();
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut sel = doc.primary();
            let mut toggled = Vec::new();
            let mut reverse_lookup = None;
            let (mut lock_toggled, mut move_here, mut bulk) = (Vec::new(), None, None);
            let mut drop = None;
            let mut stored_toggled = Vec::new();
            let draw_orders: Vec<f32> = doc
                .frame
                .as_ref()
                .map(|f| f.meshes.iter().map(|m| m.draw_order).collect())
                .unwrap_or_default();
            let tree;
            let model = if doc.palette_from_children {
                &doc.model
            } else {
                tree = parent_link_tree(&doc.model);
                &tree
            };
            let mut t = TreeCtx {
                sel: &mut sel,
                toggled: &mut toggled,
                editable: doc.editable(),
                deformers: *show_deformers,
                filter: &filter,
                draw_orders: &draw_orders,
                reverse_lookup: &mut reverse_lookup,
                lock_toggled: &mut lock_toggled,
                move_here: &mut move_here,
                bulk: &mut bulk,
                dnd: doc.editable() && !*dnd_disabled && doc.palette_from_children,
                drop: &mut drop,
                open_settings: &mut open_settings,
                stored_toggled: &mut stored_toggled,
            };
            part_children(ui, model, root, &mut t, 0);
            for (kind, k, is_lock) in stored_toggled {
                doc.edit(
                    if is_lock {
                        "Lock/unlock"
                    } else {
                        "Change Visibility"
                    },
                    |m| {
                        let info = match kind {
                            Stored::ArtPath => m
                                .art_paths
                                .get_mut(k)
                                .map(|a| &mut std::sync::Arc::make_mut(a).info),
                            Stored::Alias => m
                                .aliases
                                .get_mut(k)
                                .map(|a| &mut std::sync::Arc::make_mut(a).info),
                        };
                        if let Some(info) = info {
                            if is_lock {
                                info.locked = !info.locked;
                            } else {
                                info.visible = !info.visible;
                            }
                        }
                    },
                );
            }
            if let Some(a) = reverse_lookup {
                // Select the art meshes that use `a` as a clipping mask.
                let users = keyform_edit::objects::clip_users(&doc.model, a);
                doc.set_selection(crate::document::SelectionSet {
                    objects: users.into_iter().map(Selection::ArtMesh).collect(),
                    points: Vec::new(),
                    point_weights: Vec::new(),
                });
            } else if sel != doc.primary() {
                doc.select(sel);
            }
            for s in toggled {
                doc.edit("Change Visibility", |m| {
                    if let Some(info) = info_mut(m, s) {
                        info.visible = !info.visible;
                    }
                });
            }
            for s in lock_toggled {
                doc.edit("Lock/unlock", |m| {
                    if let Some(info) = info_mut(m, s) {
                        info.locked = !info.locked;
                    }
                });
            }
            if let Some(p) = move_here {
                let objects: Vec<keyform_edit::ObjectRef> = doc
                    .selection
                    .objects
                    .iter()
                    .filter(|o| **o != Selection::Part(p))
                    .copied()
                    .collect();
                if let Err(e) = doc.try_edit("Move Here", |m| {
                    keyform_edit::objects::move_to_part(m, &objects, p)
                }) {
                    doc.notice = Some(e);
                }
            }
            if let Some(b) = bulk {
                parts_bulk(doc, b);
            }
            if let Some((o, part, before)) = drop {
                if let Err(e) = doc.try_edit("Move", |m| {
                    keyform_edit::objects::place_in_part(m, o, part, before)
                }) {
                    doc.notice = Some(e);
                }
            }
        });
    open_settings && doc.editable()
}

/// Show/Hide all, Lock/Unlock all (parts and art meshes), Delete empty
/// parts. spec: palettes.md §1
fn parts_bulk(doc: &mut Document, b: PartsBulk) {
    use std::sync::Arc;
    let r = match b {
        PartsBulk::ShowAll(v) => doc.try_edit(if v { "Show all" } else { "Hide all" }, |m| {
            for p in &mut m.parts {
                if p.info.visible != v {
                    Arc::make_mut(p).info.visible = v;
                }
            }
            for a in &mut m.art_meshes {
                if a.info.visible != v {
                    Arc::make_mut(a).info.visible = v;
                }
            }
            Ok(())
        }),
        PartsBulk::LockAll(v) => doc.try_edit(if v { "Lock all" } else { "Unlock all" }, |m| {
            for p in &mut m.parts {
                if p.info.locked != v {
                    Arc::make_mut(p).info.locked = v;
                }
            }
            for a in &mut m.art_meshes {
                if a.info.locked != v {
                    Arc::make_mut(a).info.locked = v;
                }
            }
            Ok(())
        }),
        PartsBulk::DeleteEmpty => doc
            .try_edit("Delete empty parts", |m| {
                Ok(keyform_edit::objects::delete_empty_parts(m))
            })
            .map(|_| ()),
    };
    if let Err(e) = r {
        doc.notice = Some(e);
    }
}

/// A copy of the part tree whose child lists come from parent links, in
/// model order (runtime models carry no palette order).
fn parent_link_tree(m: &Model) -> Model {
    let mut t = Model {
        parts: m.parts.clone(),
        art_meshes: m.art_meshes.clone(),
        deformers: m.deformers.clone(),
        glues: m.glues.clone(),
        art_paths: m.art_paths.clone(),
        aliases: m.aliases.clone(),
        ..Model::default()
    };
    for p in &mut t.parts {
        std::sync::Arc::make_mut(p).children.clear();
    }
    let root = m.root_part;
    let parent = |pp: Option<usize>| pp.or(root).filter(|&p| p < m.parts.len());
    for (i, p) in m.parts.iter().enumerate() {
        if Some(i) != root {
            if let Some(q) = parent(p.info.parent_part).filter(|&q| q != i) {
                std::sync::Arc::make_mut(&mut t.parts[q])
                    .children
                    .push(PartChild::Part(i));
            }
        }
    }
    for (i, a) in m.art_meshes.iter().enumerate() {
        if let Some(q) = parent(a.info.parent_part) {
            std::sync::Arc::make_mut(&mut t.parts[q])
                .children
                .push(PartChild::ArtMesh(i));
        }
    }
    for (i, d) in m.deformers.iter().enumerate() {
        if let Some(q) = parent(d.info.parent_part) {
            std::sync::Arc::make_mut(&mut t.parts[q])
                .children
                .push(PartChild::Deformer(i));
        }
    }
    t
}

/// Common fields of the selected object, for editing.
pub fn info_mut(m: &mut Model, s: Selection) -> Option<&mut ObjectInfo> {
    use std::sync::Arc;
    Some(match s {
        Selection::Part(i) => &mut Arc::make_mut(m.parts.get_mut(i)?).info,
        Selection::Deformer(i) => &mut Arc::make_mut(m.deformers.get_mut(i)?).info,
        Selection::ArtMesh(i) => &mut Arc::make_mut(m.art_meshes.get_mut(i)?).info,
        Selection::Glue(i) => &mut Arc::make_mut(m.glues.get_mut(i)?).info,
    })
}

pub fn info(m: &Model, s: Selection) -> Option<&ObjectInfo> {
    Some(match s {
        Selection::Part(i) => &m.parts.get(i)?.info,
        Selection::Deformer(i) => &m.deformers.get(i)?.info,
        Selection::ArtMesh(i) => &m.art_meshes.get(i)?.info,
        Selection::Glue(i) => &m.glues.get(i)?.info,
    })
}

struct TreeCtx<'a> {
    sel: &'a mut Option<Selection>,
    /// Objects whose eye was clicked.
    toggled: &'a mut Vec<Selection>,
    editable: bool,
    deformers: bool,
    filter: &'a str,
    draw_orders: &'a [f32],
    /// "Reverse Clipping ID Lookup" requested on this art mesh.
    reverse_lookup: &'a mut Option<usize>,
    /// Objects whose lock was clicked.
    lock_toggled: &'a mut Vec<Selection>,
    /// "Move Here": the selected objects into this part.
    move_here: &'a mut Option<usize>,
    bulk: &'a mut Option<PartsBulk>,
    /// Drag & drop allowed (palette menu "Disable drag & drop" off).
    dnd: bool,
    /// A row dropped: (dragged, target part, before this child).
    drop: &'a mut Option<(Selection, usize, Option<Selection>)>,
    /// Parts Settings requested (double-click or menu).
    open_settings: &'a mut bool,
    /// Art paths / aliases whose eye or lock was clicked: (kind, index, lock).
    stored_toggled: &'a mut Vec<(Stored, usize, bool)>,
}

/// Objects kept as stored (art-path.md, alias.md): listed in their part,
/// with eye and lock; they aren't selectable or editable otherwise.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stored {
    ArtPath,
    Alias,
}

/// The art path / alias rows of `part`.
fn stored_rows(ui: &mut egui::Ui, m: &Model, part: usize, t: &mut TreeCtx) {
    let rows = m
        .art_paths
        .iter()
        .enumerate()
        .filter(|(_, a)| a.info.parent_part == Some(part))
        .map(|(k, a)| (Stored::ArtPath, k, &a.info))
        .chain(
            m.aliases
                .iter()
                .enumerate()
                .filter(|(_, a)| a.info.parent_part == Some(part))
                .map(|(k, a)| (Stored::Alias, k, &a.info)),
        );
    for (kind, k, info) in rows {
        if !t.filter.is_empty()
            && !info.display_name().to_lowercase().contains(t.filter)
            && !info.id.to_lowercase().contains(t.filter)
        {
            continue;
        }
        ui.horizontal(|ui| {
            if eye(ui, info.visible, t.editable) {
                t.stored_toggled.push((kind, k, false));
            }
            if lock(ui, info.locked, t.editable) {
                t.stored_toggled.push((kind, k, true));
            }
            icons::icon(
                ui,
                if kind == Stored::ArtPath {
                    Icon::ArtPath
                } else {
                    Icon::Alias
                },
            );
            ui.label(info.display_name()).on_hover_text(crate::i18n::tr(
                "Kept as stored: shown and saved, not editable in Keyform yet",
            ));
        });
    }
}

fn part_children(ui: &mut egui::Ui, m: &Model, part: usize, t: &mut TreeCtx, depth: usize) {
    let (deformers, filter, dos) = (t.deformers, t.filter, t.draw_orders);
    if depth > 64 {
        return;
    }
    stored_rows(ui, m, part, t);
    for &c in &m.parts[part].children {
        match c {
            PartChild::Part(p) if p < m.parts.len() => {
                let info = &m.parts[p].info;
                let id = ui.make_persistent_id(("part", p));
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    depth < 1,
                )
                .show_header(ui, |ui| {
                    if eye(ui, info.visible, t.editable) {
                        t.toggled.push(Selection::Part(p));
                    }
                    if lock(ui, info.locked, t.editable) {
                        t.lock_toggled.push(Selection::Part(p));
                    }
                    icons::icon(ui, Icon::Part);
                    let me = Selection::Part(p);
                    let (r, dropped) =
                        row_label(ui, *t.sel == Some(me), info.display_name(), me, t.dnd, true);
                    if r.clicked() {
                        *t.sel = Some(me);
                    }
                    if r.double_clicked() {
                        *t.open_settings = true;
                    }
                    if let Some((o, before)) = dropped {
                        *t.drop = Some(if before {
                            (o, part, Some(me))
                        } else {
                            (o, p, None)
                        });
                    }
                    r.context_menu(|ui| {
                        if ui
                            .add_enabled(
                                t.editable,
                                egui::Button::new(crate::i18n::tr("Move Here")),
                            )
                            .on_hover_text(crate::i18n::tr(
                                "Move the selected objects into this part",
                            ))
                            .clicked()
                        {
                            *t.move_here = Some(p);
                            ui.close();
                        }
                        if ui.button(crate::i18n::tr("Part Settings…")).clicked() {
                            *t.open_settings = true;
                            ui.close();
                        }
                        ui.separator();
                        parts_menu(ui, t.bulk);
                    });
                })
                .body(|ui| part_children(ui, m, p, t, depth + 1));
            }
            PartChild::ArtMesh(a) if a < m.art_meshes.len() => {
                let info = &m.art_meshes[a].info;
                if !filter.is_empty()
                    && !info.display_name().to_lowercase().contains(filter)
                    && !info.id.to_lowercase().contains(filter)
                {
                    continue;
                }
                ui.horizontal(|ui| {
                    if eye(ui, info.visible, t.editable) {
                        t.toggled.push(Selection::ArtMesh(a));
                    }
                    if lock(ui, info.locked, t.editable) {
                        t.lock_toggled.push(Selection::ArtMesh(a));
                    }
                    icons::icon(ui, Icon::ArtMesh);
                    let me = Selection::ArtMesh(a);
                    let (r, dropped) = row_label(
                        ui,
                        *t.sel == Some(me),
                        info.display_name(),
                        me,
                        t.dnd,
                        false,
                    );
                    if r.clicked() {
                        *t.sel = Some(me);
                    }
                    if let Some((o, _)) = dropped {
                        // Before this mesh, in its part.
                        *t.drop = Some((o, part, Some(me)));
                    }
                    r.context_menu(|ui| {
                        if ui
                            .button(crate::i18n::tr("Reverse Clipping ID Lookup"))
                            .on_hover_text(crate::i18n::tr(
                                "Select the ArtMeshes that use this one as a clipping mask",
                            ))
                            .clicked()
                        {
                            *t.reverse_lookup = Some(a);
                            ui.close();
                        }
                    });
                    if let Some(d) = dos.get(a) {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.weak(format!("{d:.0}"));
                        });
                    }
                });
            }
            PartChild::Deformer(d) if deformers && d < m.deformers.len() => {
                let info = &m.deformers[d].info;
                ui.horizontal(|ui| {
                    icons::icon(ui, deformer_icon(m, d));
                    if selectable(
                        ui,
                        *t.sel == Some(Selection::Deformer(d)),
                        info.display_name(),
                    ) {
                        *t.sel = Some(Selection::Deformer(d));
                    }
                });
            }
            PartChild::Glue(g) if deformers && g < m.glues.len() => {
                ui.horizontal(|ui| {
                    icons::icon(ui, Icon::Glue);
                    if selectable(
                        ui,
                        *t.sel == Some(Selection::Glue(g)),
                        m.glues[g].info.display_name(),
                    ) {
                        *t.sel = Some(Selection::Glue(g));
                    }
                });
            }
            _ => {}
        }
    }
}

fn deformer_icon(m: &Model, d: usize) -> Icon {
    if matches!(m.deformers[d].kind, DeformerKind::Warp(_)) {
        Icon::WarpDeformer
    } else {
        Icon::RotationDeformer
    }
}

// ---------------------------------------------------------------- Deformer

/// Deformer palette (read-only): the target-deformer tree.
pub fn deformer_ui(ui: &mut egui::Ui, doc: &mut Document) {
    let m = &doc.model;
    let n = m.deformers.len();
    let mut children: Vec<Vec<Selection>> = vec![Vec::new(); n + 1];
    let slot = |t: Option<usize>| t.filter(|&t| t < n).unwrap_or(n);
    for (i, d) in m.deformers.iter().enumerate() {
        children[slot(d.target)].push(Selection::Deformer(i));
    }
    for (i, a) in m.art_meshes.iter().enumerate() {
        children[slot(a.target)].push(Selection::ArtMesh(i));
    }
    let mut sel = doc.primary();
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            deformer_children(ui, m, &children, n, &mut sel, 0);
        });
    if sel != doc.primary() {
        doc.select(sel);
    }
}

fn deformer_children(
    ui: &mut egui::Ui,
    m: &Model,
    ch: &[Vec<Selection>],
    node: usize,
    sel: &mut Option<Selection>,
    depth: usize,
) {
    if depth > 64 {
        return;
    }
    for &c in &ch[node] {
        match c {
            Selection::Deformer(d) => {
                let id = ui.make_persistent_id(("deformer", d));
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    false,
                )
                .show_header(ui, |ui| {
                    icons::icon(ui, deformer_icon(m, d));
                    if selectable(ui, *sel == Some(c), m.deformers[d].info.display_name()) {
                        *sel = Some(c);
                    }
                })
                .body(|ui| deformer_children(ui, m, ch, d, sel, depth + 1));
            }
            Selection::ArtMesh(a) => {
                ui.horizontal(|ui| {
                    icons::icon(ui, Icon::ArtMesh);
                    if selectable(ui, *sel == Some(c), m.art_meshes[a].info.display_name()) {
                        *sel = Some(c);
                    }
                });
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- Parameter

/// Key values of the selected object on parameter `p` (key marks, §3).
fn selected_keys(doc: &Document, p: usize) -> Vec<f32> {
    match doc.primary() {
        // Blend-shape parameters: the base (0) and the entry keys.
        Some(o)
            if doc
                .model
                .parameters
                .get(p)
                .is_some_and(|q| q.kind == keyform_model::ParameterKind::BlendShape) =>
        {
            let mut k = keyform_edit::keys::blend_keys_of(&doc.model, o, p);
            if !k.is_empty() {
                k.insert(0, 0.0);
            }
            k
        }
        Some(o) => keyform_edit::keys_of(&doc.model, o, p),
        None => Vec::new(),
    }
}

pub fn parameter_ui(
    ui: &mut egui::Ui,
    doc: &mut Document,
    tokens: &Tokens,
    filter: &mut String,
    log: &mut Log,
) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text("Filter by Text")
                .desired_width(ui.available_width() - 90.0),
        );
        if ui
            .button(crate::i18n::tr("Reset"))
            .on_hover_text(crate::i18n::tr("Reset to default values"))
            .clicked()
        {
            doc.reset_parameters();
        }
    });
    ui.separator();
    let filter = filter.to_lowercase();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let entries = doc.model.root_parameter_entries.clone();
            param_entries(ui, doc, tokens, &entries, &filter, 0, log);
        });
}

fn param_entries(
    ui: &mut egui::Ui,
    doc: &mut Document,
    tokens: &Tokens,
    entries: &[ParamEntry],
    filter: &str,
    depth: usize,
    log: &mut Log,
) {
    for e in entries {
        match *e {
            ParamEntry::Parameter(p) if p < doc.model.parameters.len() => {
                param_row(ui, doc, tokens, p, filter, log)
            }
            ParamEntry::Group(g) if g < doc.model.parameter_groups.len() && depth < 64 => {
                let name = doc.model.parameter_groups[g].name.clone();
                let children = doc.model.parameter_groups[g].children.clone();
                let id = ui.make_persistent_id(("pgroup", g));
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    true,
                )
                .show_header(ui, |ui| {
                    icons::icon(ui, Icon::Folder);
                    ui.label(RichText::new(name).strong());
                })
                .body(|ui| param_entries(ui, doc, tokens, &children, filter, depth + 1, log));
            }
            _ => {}
        }
    }
}

fn run_key_op(doc: &mut Document, op: KeyOp, p: usize, log: &mut Log) {
    match doc.key_op(op, p) {
        Ok(Some(w)) => log.warn(w),
        Ok(None) => {}
        Err(e) => log.warn(e),
    }
}

/// The key operations of a parameter row's context menu (palettes.md §3).
fn key_menu(ui: &mut egui::Ui, doc: &mut Document, p: usize, log: &mut Log) {
    use crate::param_dialog::Request;
    // Row context menu (palettes.md §3): Edit / Create / Delete parameter.
    let project = doc.editable() && doc.mode == crate::document::Mode::Modeling;
    let group = doc.model.parameters.get(p).and_then(|x| x.group);
    for (label, req) in [
        ("Edit Parameter", Request::Edit(p)),
        ("Create Parameter", Request::Create(group)),
        ("Delete", Request::Delete(p)),
    ] {
        if ui.add_enabled(project, egui::Button::new(label)).clicked() {
            doc.param_request = Some(req);
            ui.close();
        }
    }
    ui.separator();
    let can = doc.editable() && doc.primary().is_some();
    for (label, op) in [
        ("Add key", KeyOp::Add),
        ("Delete key", KeyOp::Delete),
        ("Add 2 keys", KeyOp::AddTwo),
        ("Add 3 keys", KeyOp::AddThree),
        ("Delete all keys", KeyOp::DeleteAll),
    ] {
        if ui.add_enabled(can, egui::Button::new(label)).clicked() {
            run_key_op(doc, op, p, log);
            ui.close();
        }
    }
}

fn param_row(
    ui: &mut egui::Ui,
    doc: &mut Document,
    tokens: &Tokens,
    p: usize,
    filter: &str,
    log: &mut Log,
) {
    let param = doc.model.parameters[p].clone();
    if !filter.is_empty()
        && !param.name.to_lowercase().contains(filter)
        && !param.id.to_lowercase().contains(filter)
    {
        return;
    }
    let keys = selected_keys(doc, p);
    let live = doc.live.get(p).copied().unwrap_or(param.default);
    let fill = if keys.is_empty() {
        tokens.parameter_row
    } else {
        tokens.parameter_row_selected
    };
    egui::Frame::new()
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(4, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if param.kind == keyform_model::ParameterKind::BlendShape {
                    icons::icon(ui, Icon::BlendShape)
                        .on_hover_text(crate::i18n::tr("Blend Shape parameter"));
                }
                ui.label(&param.name)
                    .on_hover_text(&param.id)
                    .context_menu(|ui| key_menu(ui, doc, p, log));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Key buttons for the selected object (palettes.md §3).
                    if doc.editable() && doc.primary().is_some() {
                        let on_key = keys
                            .iter()
                            .any(|k| (k - doc.shown_value(p)).abs() < param.snap_epsilon);
                        if on_key {
                            if icons::icon_button(ui, Icon::KeyDelete, "Delete key").clicked() {
                                run_key_op(doc, KeyOp::Delete, p, log);
                            }
                        } else if icons::icon_button(ui, Icon::KeyAdd, "Add key").clicked() {
                            run_key_op(doc, KeyOp::Add, p, log);
                        }
                    }
                    ui.weak(format!(
                        "{:.*}",
                        param.decimal_places.clamp(0, 6) as usize,
                        live
                    ));
                });
            });
            let (lo, hi) = (param.min.min(param.max), param.min.max(param.max));
            // Shows the live value until the user takes the parameter by hand
            // (viewer.md §3.1 step 8); double-click releases it.
            let mut v = if doc.manual[p] { doc.values[p] } else { live };
            let width = ui.available_width();
            ui.spacing_mut().slider_width = width;
            let r = ui.add(
                egui::Slider::new(&mut v, lo..=hi)
                    .show_value(false)
                    .clamping(egui::SliderClamping::Always),
            );
            if r.changed() {
                doc.set_manual(p, v);
            }
            if r.double_clicked() {
                doc.release_manual(p);
            }
            // Default marker and key marks under the slider (§3).
            let rect = r.rect;
            let x_of = |k: f32| {
                let t = if hi > lo { (k - lo) / (hi - lo) } else { 0.0 };
                let inset = rect.height() / 2.0;
                rect.left() + inset + t.clamp(0.0, 1.0) * (rect.width() - 2.0 * inset)
            };
            let painter = ui.painter();
            let y = rect.bottom();
            painter.line_segment(
                [
                    egui::pos2(x_of(param.default), y - 4.0),
                    egui::pos2(x_of(param.default), y),
                ],
                egui::Stroke::new(1.0, tokens.secondary_text),
            );
            let blend = param.kind == keyform_model::ParameterKind::BlendShape;
            for k in keys {
                let x = x_of(k);
                // Blend shapes: the base key (0) grey, entry keys teal
                // (as the reference draws them, Q-0037).
                let fill = match (blend, k == 0.0) {
                    (true, true) => tokens.secondary_text,
                    (true, false) => tokens.info,
                    _ => tokens.accent,
                };
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(x, y - 5.0),
                        egui::pos2(x + 3.0, y - 2.0),
                        egui::pos2(x, y + 1.0),
                        egui::pos2(x - 3.0, y - 2.0),
                    ],
                    fill,
                    egui::Stroke::NONE,
                ));
            }
        });
}

// ---------------------------------------------------------------- Inspector

/// Single-line text field that commits on Enter or focus loss. Shows the
/// model value whenever it isn't being edited (so undo updates it).
fn commit_text(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    value: &str,
    enabled: bool,
) -> Option<String> {
    let id = ui.make_persistent_id(salt);
    let editing = ui.memory(|m| m.has_focus(id));
    let mut buf = if editing {
        ui.data(|d| d.get_temp::<String>(id))
            .unwrap_or_else(|| value.to_owned())
    } else {
        value.to_owned()
    };
    let r = ui.add_enabled(
        enabled,
        egui::TextEdit::singleline(&mut buf)
            .id(id)
            .desired_width(f32::INFINITY),
    );
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        return (buf != value).then_some(buf);
    }
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf));
    }
    None
}

/// The message for an unusable new object ID, or `None`.
/// spec: 50-ui/dialogs.md §2 (letters, digits, underscores; at most 63
/// characters; no leading digit). Uniqueness is checked among objects of
/// the same kind.
fn id_problem(m: &Model, s: Selection, id: &str) -> Option<String> {
    if !keyform_edit::valid_id(id) {
        return Some("Invalid ID".into());
    }
    fn used<'a>(mut ids: impl Iterator<Item = (usize, &'a str)>, me: usize, id: &str) -> bool {
        ids.any(|(j, x)| j != me && x == id)
    }
    let taken = match s {
        Selection::Part(i) => used(
            m.parts.iter().map(|x| x.info.id.as_str()).enumerate(),
            i,
            id,
        ),
        Selection::Deformer(i) => used(
            m.deformers.iter().map(|x| x.info.id.as_str()).enumerate(),
            i,
            id,
        ),
        Selection::ArtMesh(i) => used(
            m.art_meshes.iter().map(|x| x.info.id.as_str()).enumerate(),
            i,
            id,
        ),
        Selection::Glue(i) => used(
            m.glues.iter().map(|x| x.info.id.as_str()).enumerate(),
            i,
            id,
        ),
    };
    taken.then(|| format!("ID: \"{id}\" is duplicated."))
}

/// Editable name, ID, visibility and lock of the selected object.
fn common_fields(ui: &mut egui::Ui, doc: &mut Document, s: Selection, log: &mut Log) {
    let Some(info) = info(&doc.model, s).cloned() else {
        return;
    };
    let editable = doc.editable();
    egui::Grid::new("inspector-common")
        .num_columns(2)
        .show(ui, |ui| {
            ui.weak(crate::i18n::tr("Name"));
            if let Some(name) = commit_text(ui, ("insp-name", s), &info.name, editable) {
                doc.edit("Change Name", |m| {
                    if let Some(i) = info_mut(m, s) {
                        i.name = name;
                    }
                });
            }
            ui.end_row();
            ui.weak(crate::i18n::tr("ID"));
            if let Some(id) = commit_text(ui, ("insp-id", s), &info.id, editable) {
                match id_problem(&doc.model, s, &id) {
                    Some(p) => log.warn(p),
                    None => {
                        doc.edit("Change ID", |m| {
                            if let Some(i) = info_mut(m, s) {
                                i.id = id;
                            }
                        });
                    }
                }
            }
            ui.end_row();
            let (mut visible, mut locked) = (info.visible, info.locked);
            ui.weak(crate::i18n::tr("Draw"));
            if ui
                .add_enabled(editable, egui::Checkbox::without_text(&mut visible))
                .changed()
            {
                doc.edit("Change Visibility", |m| {
                    if let Some(i) = info_mut(m, s) {
                        i.visible = visible;
                    }
                });
            }
            ui.end_row();
            ui.weak(crate::i18n::tr("Lock"));
            if ui
                .add_enabled(editable, egui::Checkbox::without_text(&mut locked))
                .changed()
            {
                doc.edit("Lock", |m| {
                    if let Some(i) = info_mut(m, s) {
                        i.locked = locked;
                    }
                });
            }
            ui.end_row();
        });
    if let Selection::ArtMesh(a) = s {
        let mut culling = doc.model.art_meshes[a].culling;
        if ui
            .add_enabled(editable, egui::Checkbox::new(&mut culling, "Culling"))
            .changed()
        {
            doc.edit("Change Culling", |m| {
                std::sync::Arc::make_mut(&mut m.art_meshes[a]).culling = culling;
            });
        }
        clipping_fields(ui, doc, a, log);
    }
    ui.separator();
}

/// Clipping mask IDs (comma-separated, with a picker) and Invert Mask.
/// spec: 50-ui/palettes.md §4, 50-ui/dialogs.md §21
fn clipping_fields(ui: &mut egui::Ui, doc: &mut Document, a: usize, log: &mut Log) {
    let editable = doc.editable();
    let m = &doc.model;
    let x = &m.art_meshes[a];
    let mut ids: Vec<String> = x
        .clip
        .iter()
        .filter_map(|&k| m.art_meshes.get(k))
        .map(|c| c.info.id.clone())
        .collect();
    let text = ids.join(", ");
    let missing = x.clip_missing;
    let mut new_ids = None;
    ui.horizontal(|ui| {
        ui.weak(crate::i18n::tr("Clipping"));
        if let Some(t) = commit_text(ui, ("insp-clip", a), &text, editable) {
            new_ids = Some(
                t.split([',', ' ', ';'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            );
        }
    });
    if missing {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Referencing a deleted mask. Fix the clipping field.",
        );
    }
    ui.add_enabled_ui(editable, |ui| {
        ui.menu_button(crate::i18n::tr("Add mask…"), |ui| {
            ui.weak(crate::i18n::tr("Select the clipping mask objects."));
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    for (k, c) in doc.model.art_meshes.iter().enumerate() {
                        if k == a || ids.contains(&c.info.id) {
                            continue;
                        }
                        if ui
                            .button(format!("{}  ({})", c.info.display_name(), c.info.id))
                            .clicked()
                        {
                            ids.push(c.info.id.clone());
                            new_ids = Some(ids.clone());
                            ui.close();
                        }
                    }
                });
        });
    });
    let mut invert = doc.model.art_meshes[a].invert_clip;
    if ui
        .add_enabled(editable, egui::Checkbox::new(&mut invert, "Invert Mask"))
        .changed()
    {
        let v = doc.model.target_version;
        if invert && v != 0 && v < 4_000_000 {
            log.warn("Inverted masks can only be used in Cubism 4.0 or later.");
        }
        doc.edit("Invert Mask", |m| {
            std::sync::Arc::make_mut(&mut m.art_meshes[a]).invert_clip = invert;
        });
    }
    if let Some(list) = new_ids {
        let refs: Vec<&str> = list.iter().map(String::as_str).collect();
        let mut dropped = Vec::new();
        doc.edit("Change Clipping", |m| {
            dropped = keyform_edit::objects::set_clip_ids(m, a, &refs);
        });
        if !dropped.is_empty() {
            log.warn(format!(
                "{} ({})",
                keyform_edit::objects::CLIP_IDS_CANCELLED,
                dropped.join(", ")
            ));
        }
    }
}

pub fn inspector_ui(ui: &mut egui::Ui, doc: &mut Document, log: &mut Log) {
    if let Some(s) = doc.primary() {
        common_fields(ui, doc, s, log);
        crate::inspector::fields_ui(ui, doc);
        ui.separator();
    }
    let m = &doc.model;
    let row = |ui: &mut egui::Ui, k: &str, v: String| {
        ui.horizontal(|ui| {
            ui.weak(k);
            ui.label(v);
        });
    };
    let bindings = |ui: &mut egui::Ui, b: &[keyform_model::Binding], kf: usize| {
        row(ui, "Keyforms", kf.to_string());
        for x in b {
            let p = m
                .parameters
                .get(x.parameter)
                .map_or("?".to_owned(), |p| p.id.clone());
            row(ui, "Parameter", format!("{p}  keys {:?}", x.keys));
        }
    };
    match doc.primary() {
        None => {
            ui.weak(crate::i18n::tr(
                "Select an object in the Parts or Deformer palette.",
            ));
            row(ui, "Model", m.name.clone());
            row(
                ui,
                "Canvas",
                format!("{} × {}", m.canvas.width, m.canvas.height),
            );
        }
        Some(Selection::ArtMesh(a)) => {
            let x = &m.art_meshes[a];
            row(ui, "Type", "ArtMesh".into());
            row(ui, "Vertices", x.vertex_count().to_string());
            row(ui, "Polygons", (x.indices.len() / 3).to_string());
            row(
                ui,
                "Texture",
                x.texture.map_or("—".into(), |t| t.to_string()),
            );
            row(
                ui,
                "Blend",
                format!("{:?} / {:?}", x.color_composition, x.alpha_composition),
            );
            if !x.clip.is_empty() {
                let ids: Vec<&str> = x
                    .clip
                    .iter()
                    .filter_map(|&c| m.art_meshes.get(c))
                    .map(|c| c.info.id.as_str())
                    .collect();
                row(
                    ui,
                    if x.invert_clip {
                        "Mask (inverted)"
                    } else {
                        "Mask"
                    },
                    ids.join(", "),
                );
            }
            if let Some(s) = doc.frame.as_ref().and_then(|f| f.meshes.get(a)) {
                row(ui, "Opacity", format!("{:.3}", s.opacity));
                row(ui, "Draw order", format!("{:.0}", s.draw_order));
            }
            bindings(ui, &x.grid.bindings, x.grid.keyforms.len());
        }
        Some(Selection::Part(p)) => {
            let x = &m.parts[p];
            row(ui, "Type", "Part".into());
            row(ui, "Draw order group", x.draw_order_group.to_string());
            row(ui, "Offscreen", x.offscreen.to_string());
            bindings(ui, &x.grid.bindings, x.grid.keyforms.len());
        }
        Some(Selection::Deformer(d)) => {
            let x = &m.deformers[d];
            match &x.kind {
                DeformerKind::Warp(w) => {
                    row(ui, "Type", "Warp Deformer".into());
                    row(
                        ui,
                        "Divisions",
                        format!(
                            "{} × {}{}",
                            w.cols,
                            w.rows,
                            if w.quad { " (quad)" } else { "" }
                        ),
                    );
                    bindings(ui, &w.grid.bindings, w.grid.keyforms.len());
                }
                DeformerKind::Rotation(r) => {
                    row(ui, "Type", "Rotation Deformer".into());
                    row(ui, "Base angle", format!("{}", r.base_angle));
                    bindings(ui, &r.grid.bindings, r.grid.keyforms.len());
                }
            }
        }
        Some(Selection::Glue(g)) => {
            let x = &m.glues[g];
            row(ui, "Type", "Glue".into());
            row(ui, "Pairs", x.pairs.len().to_string());
            bindings(ui, &x.grid.bindings, x.grid.keyforms.len());
        }
    }
}

// ---------------------------------------------------------------- Log

pub fn log_ui(ui: &mut egui::Ui, log: &mut Log, tokens: &Tokens) {
    ui.horizontal(|ui| {
        if ui.button(crate::i18n::tr("Clear")).clicked() {
            log.entries.clear();
        }
    });
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for (level, msg) in &log.entries {
                let color = match level {
                    Level::Info => tokens.info,
                    Level::Warning => tokens.warning,
                    Level::Error => tokens.error,
                };
                ui.label(RichText::new(msg).color(color));
            }
        });
}

/// What the user picked in the Project palette.
pub enum ProjectAction {
    /// Make document `i` active.
    Activate(usize),
    /// Layer ▸ Create ArtMesh (layered image `image`, layer GUID `layer`).
    CreateArtMesh {
        doc: usize,
        image: usize,
        layer: String,
    },
}

/// Project palette: open documents and, inside a model, its source images
/// (PSDs and their layers), model images (grouped) and texture atlases.
/// spec: 50-ui/palettes.md §6
pub fn project_ui(
    ui: &mut egui::Ui,
    docs: &[&Document],
    active: Option<usize>,
) -> Option<ProjectAction> {
    let mut pick = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, d) in docs.iter().enumerate() {
                let r = ui.add(
                    egui::Label::new(RichText::new(&d.title).strong().color(
                        if Some(i) == active {
                            Color32::WHITE
                        } else {
                            ui.visuals().text_color()
                        },
                    ))
                    .sense(Sense::click()),
                );
                if r.clicked() {
                    pick = Some(ProjectAction::Activate(i));
                }
                ui.indent(("proj", i), |ui| {
                    ui.weak(d.path.display().to_string());
                    project_model(ui, d, i, &mut pick);
                });
            }
            if docs.is_empty() {
                ui.weak(crate::i18n::tr("No documents open."));
            }
        });
    pick
}

fn project_model(ui: &mut egui::Ui, d: &Document, di: usize, pick: &mut Option<ProjectAction>) {
    use keyform_model::LayerEntry;
    let m = &d.model;
    // Layers some art mesh draws from (through its model image).
    let used: std::collections::HashSet<&str> = m
        .art_meshes
        .iter()
        .filter_map(|a| m.model_images.get(a.model_image?)?.source.as_ref())
        .map(|(_, l)| l.as_str())
        .collect();
    if !m.layered_images.is_empty() {
        egui::CollapsingHeader::new("Source images")
            .id_salt(("src", di))
            .show(ui, |ui| {
                for (li, img) in m.layered_images.iter().enumerate() {
                    egui::CollapsingHeader::new(format!(
                        "{} ({}×{})",
                        img.name, img.width, img.height
                    ))
                    .id_salt(("psd", di, li))
                    .show(ui, |ui| {
                        fn level(
                            ui: &mut egui::Ui,
                            entries: &[LayerEntry],
                            used: &std::collections::HashSet<&str>,
                            editable: bool,
                            at: (usize, usize),
                            pick: &mut Option<ProjectAction>,
                        ) {
                            for e in entries {
                                match e {
                                    LayerEntry::Group(g) => {
                                        egui::CollapsingHeader::new(&g.common.name)
                                            .id_salt(("lg", at, &g.common.guid))
                                            .show(ui, |ui| {
                                                level(ui, &g.children, used, editable, at, pick)
                                            });
                                    }
                                    LayerEntry::Layer(l) => {
                                        let unused = !used.contains(l.common.guid.as_str());
                                        let text = if unused {
                                            format!("{}  # Unused #", l.common.name)
                                        } else {
                                            l.common.name.clone()
                                        };
                                        let r =
                                            ui.add(egui::Label::new(text).sense(Sense::click()));
                                        r.context_menu(|ui| {
                                            if ui
                                                .add_enabled(
                                                    editable,
                                                    egui::Button::new(crate::i18n::tr(
                                                        "Create ArtMesh",
                                                    )),
                                                )
                                                .clicked()
                                            {
                                                *pick = Some(ProjectAction::CreateArtMesh {
                                                    doc: at.0,
                                                    image: at.1,
                                                    layer: l.common.guid.clone(),
                                                });
                                                ui.close();
                                            }
                                        });
                                    }
                                }
                            }
                        }
                        match &img.tree {
                            Some(t) => level(ui, t, &used, d.editable(), (di, li), pick),
                            None => {
                                ui.weak(crate::i18n::tr("(layers not loaded)"));
                            }
                        }
                    });
                }
            });
    }
    if !m.model_images.is_empty() {
        egui::CollapsingHeader::new("Model images")
            .id_salt(("mi", di))
            .show(ui, |ui| {
                for (gi, g) in m.model_image_groups.iter().enumerate() {
                    egui::CollapsingHeader::new(&g.name)
                        .id_salt(("mig", di, gi))
                        .show(ui, |ui| {
                            for mi in m.model_images.iter().filter(|x| x.group == Some(gi)) {
                                ui.label(&mi.name);
                            }
                        });
                }
                for mi in m.model_images.iter().filter(|x| x.group.is_none()) {
                    ui.label(&mi.name);
                }
            });
    }
    if !m.textures.is_empty() {
        egui::CollapsingHeader::new("Texture atlases")
            .id_salt(("tex", di))
            .show(ui, |ui| {
                for t in &m.textures {
                    ui.label(format!("{} ({}×{})", t.name, t.width, t.height));
                }
            });
    }
}
