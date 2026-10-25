//! Palettes. spec: 50-ui/palettes.md (§1 Parts, §2 Deformer, §3 Parameter,
//! §4 Inspector, §9 Log), 70-advanced/viewer.md §2 (model settings)

use eframe::egui::{self, Color32, RichText, Sense};
use keyform_model::{DeformerKind, Model, ParamEntry, PartChild};

use crate::document::{Document, Selection};
use crate::icons::{self, Icon};
use crate::log::{Level, Log};
use crate::theme::Tokens;

fn selectable(ui: &mut egui::Ui, selected: bool, text: impl Into<egui::WidgetText>) -> bool {
    ui.selectable_label(selected, text).clicked()
}

fn eye(ui: &mut egui::Ui, visible: bool) {
    icons::icon(ui, if visible { Icon::Visible } else { Icon::Hidden });
}

// ---------------------------------------------------------------- Parts

/// Read-only Parts palette: part tree with art meshes (and, when toggled,
/// deformers and glue); columns eye, name, draw order.
pub fn parts_ui(
    ui: &mut egui::Ui,
    doc: &mut Document,
    show_deformers: &mut bool,
    filter: &mut String,
) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text("Filter by Text")
                .desired_width(ui.available_width() - 60.0),
        );
        ui.toggle_value(show_deformers, "D")
            .on_hover_text("Show/hide Warp Deformers and Rotation Deformers");
    });
    ui.separator();
    let Some(root) = doc.model.root_part else {
        ui.label("(no part tree)");
        return;
    };
    let filter = filter.to_lowercase();
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut sel = doc.selection;
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
            part_children(
                ui,
                model,
                root,
                &mut sel,
                *show_deformers,
                &filter,
                &draw_orders,
                0,
            );
            doc.selection = sel;
        });
}

/// A copy of the part tree whose child lists come from parent links, in
/// model order (runtime models carry no palette order).
fn parent_link_tree(m: &Model) -> Model {
    let mut t = Model {
        parts: m.parts.clone(),
        art_meshes: m.art_meshes.clone(),
        deformers: m.deformers.clone(),
        glues: m.glues.clone(),
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

#[allow(clippy::too_many_arguments)]
fn part_children(
    ui: &mut egui::Ui,
    m: &Model,
    part: usize,
    sel: &mut Option<Selection>,
    deformers: bool,
    filter: &str,
    dos: &[f32],
    depth: usize,
) {
    if depth > 64 {
        return;
    }
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
                    eye(ui, info.visible);
                    icons::icon(ui, Icon::Part);
                    if selectable(ui, *sel == Some(Selection::Part(p)), info.display_name()) {
                        *sel = Some(Selection::Part(p));
                    }
                })
                .body(|ui| part_children(ui, m, p, sel, deformers, filter, dos, depth + 1));
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
                    eye(ui, info.visible);
                    icons::icon(ui, Icon::ArtMesh);
                    if selectable(ui, *sel == Some(Selection::ArtMesh(a)), info.display_name()) {
                        *sel = Some(Selection::ArtMesh(a));
                    }
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
                        *sel == Some(Selection::Deformer(d)),
                        info.display_name(),
                    ) {
                        *sel = Some(Selection::Deformer(d));
                    }
                });
            }
            PartChild::Glue(g) if deformers && g < m.glues.len() => {
                ui.horizontal(|ui| {
                    icons::icon(ui, Icon::Glue);
                    if selectable(
                        ui,
                        *sel == Some(Selection::Glue(g)),
                        m.glues[g].info.display_name(),
                    ) {
                        *sel = Some(Selection::Glue(g));
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
    let mut sel = doc.selection;
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            deformer_children(ui, m, &children, n, &mut sel, 0);
        });
    doc.selection = sel;
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
    let m = &doc.model;
    let keys = |b: &[keyform_model::Binding]| {
        b.iter()
            .filter(|b| b.parameter == p)
            .flat_map(|b| b.keys.clone())
            .collect::<Vec<_>>()
    };
    match doc.selection {
        Some(Selection::ArtMesh(a)) => m
            .art_meshes
            .get(a)
            .map(|x| keys(&x.grid.bindings))
            .unwrap_or_default(),
        Some(Selection::Part(i)) => m
            .parts
            .get(i)
            .map(|x| keys(&x.grid.bindings))
            .unwrap_or_default(),
        Some(Selection::Glue(g)) => m
            .glues
            .get(g)
            .map(|x| keys(&x.grid.bindings))
            .unwrap_or_default(),
        Some(Selection::Deformer(d)) => m
            .deformers
            .get(d)
            .map(|x| match &x.kind {
                DeformerKind::Warp(w) => keys(&w.grid.bindings),
                DeformerKind::Rotation(r) => keys(&r.grid.bindings),
            })
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

pub fn parameter_ui(ui: &mut egui::Ui, doc: &mut Document, tokens: &Tokens, filter: &mut String) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text("Filter by Text")
                .desired_width(ui.available_width() - 90.0),
        );
        if ui
            .button("Reset")
            .on_hover_text("Reset to default values")
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
            param_entries(ui, doc, tokens, &entries, &filter, 0);
        });
}

fn param_entries(
    ui: &mut egui::Ui,
    doc: &mut Document,
    tokens: &Tokens,
    entries: &[ParamEntry],
    filter: &str,
    depth: usize,
) {
    for e in entries {
        match *e {
            ParamEntry::Parameter(p) if p < doc.model.parameters.len() => {
                param_row(ui, doc, tokens, p, filter)
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
                .body(|ui| param_entries(ui, doc, tokens, &children, filter, depth + 1));
            }
            _ => {}
        }
    }
}

fn param_row(ui: &mut egui::Ui, doc: &mut Document, tokens: &Tokens, p: usize, filter: &str) {
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
                    icons::icon(ui, Icon::BlendShape).on_hover_text("Blend Shape parameter");
                }
                ui.label(&param.name).on_hover_text(&param.id);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
            for k in keys {
                let x = x_of(k);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(x, y - 5.0),
                        egui::pos2(x + 3.0, y - 2.0),
                        egui::pos2(x, y + 1.0),
                        egui::pos2(x - 3.0, y - 2.0),
                    ],
                    tokens.accent,
                    egui::Stroke::NONE,
                ));
            }
        });
}

// ---------------------------------------------------------------- Inspector

pub fn inspector_ui(ui: &mut egui::Ui, doc: &Document) {
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
    match doc.selection {
        None => {
            ui.weak("Select an object in the Parts or Deformer palette.");
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
            row(ui, "ID", x.info.id.clone());
            row(ui, "Name", x.info.display_name().to_owned());
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
            row(ui, "Culling", x.culling.to_string());
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
            row(ui, "ID", x.info.id.clone());
            row(ui, "Name", x.info.display_name().to_owned());
            row(ui, "Draw order group", x.draw_order_group.to_string());
            row(ui, "Offscreen", x.offscreen.to_string());
            bindings(ui, &x.grid.bindings, x.grid.keyforms.len());
        }
        Some(Selection::Deformer(d)) => {
            let x = &m.deformers[d];
            row(ui, "ID", x.info.id.clone());
            row(ui, "Name", x.info.display_name().to_owned());
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
            row(ui, "ID", x.info.id.clone());
            row(ui, "Pairs", x.pairs.len().to_string());
            bindings(ui, &x.grid.bindings, x.grid.keyforms.len());
        }
    }
}

// ---------------------------------------------------------------- Log

pub fn log_ui(ui: &mut egui::Ui, log: &mut Log, tokens: &Tokens) {
    ui.horizontal(|ui| {
        if ui.button("Clear").clicked() {
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

/// Project palette: open documents and their resources.
pub fn project_ui(ui: &mut egui::Ui, docs: &[&Document], active: Option<usize>) -> Option<usize> {
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
                    pick = Some(i);
                }
                ui.indent(("proj", i), |ui| {
                    ui.weak(d.path.display().to_string());
                    for t in &d.model.textures {
                        ui.label(format!("{} ({}×{})", t.name, t.width, t.height));
                    }
                });
            }
            if docs.is_empty() {
                ui.weak("No documents open.");
            }
        });
    pick
}
