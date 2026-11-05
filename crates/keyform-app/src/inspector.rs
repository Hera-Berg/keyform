//! Inspector fields beyond Name/ID/Draw/Lock: the current keyform's draw
//! order, transparency, multiply and screen colour and rotation angle
//! (applied to every selected object of the same kind, with the bulk
//! overwrite question when they differ), Bulk Set Draw Order, blend modes,
//! part and rotation-deformer settings, glue intensity.
//! spec: 50-ui/palettes.md §4, 50-ui/inspector-fields.md (ACDrawableForm,
//! CPartForm, ACDeformerForm, CRotationDeformerForm, CRotationDeformerSource)

use std::sync::Arc;

use eframe::egui;
use keyform_edit::forms::{
    current_cell, form_fields, set_draw_order_all, set_form_fields, FormFields,
};
use keyform_model::{AlphaComposition, Color, ColorComposition, DeformerKind};

use crate::document::{Document, Selection};

/// Multiply and screen colour need SDK target ≥ 4.2.
const COLOR_TARGET: i32 = 4_020_000;
/// Advanced blend modes need SDK target ≥ 5.3 (40-rendering/blending.md §5;
/// `targetVersionNo` grows with the version, 10-formats/cmo3.md §2).
const BLEND_TARGET: i32 = 5_030_000;

/// Blend modes offered for a target: only Normal and the legacy ones (and
/// Over) before 5.3.
fn legacy_only(target: i32) -> bool {
    target != 0 && target < BLEND_TARGET
}

/// UI labels of the colour blend modes (40-rendering/blending.md §1).
const BLEND_LABELS: [(ColorComposition, &str); 18] = [
    (ColorComposition::Normal, "Normal"),
    (ColorComposition::Add, "Add (Before 5.3)"),
    (ColorComposition::Multiply, "Multiply (Before 5.3)"),
    (ColorComposition::AddR2Tsl, "Add"),
    (ColorComposition::AddR2, "Add (Glow)"),
    (ColorComposition::Darken, "Darken"),
    (ColorComposition::MultiplyR2, "Multiply"),
    (ColorComposition::ColorBurnTsl, "Color burn"),
    (ColorComposition::LinearBurnTsl, "Linear burn"),
    (ColorComposition::Lighten, "Lighten"),
    (ColorComposition::Screen, "Screen"),
    (ColorComposition::ColorDodgeTsl, "Color dodge"),
    (ColorComposition::Overlay, "Overlay"),
    (ColorComposition::SoftLight, "Soft light"),
    (ColorComposition::HardLight, "Hard light"),
    (ColorComposition::LinearLightTsl, "Linear light"),
    (ColorComposition::HslHue, "Hue"),
    (ColorComposition::HslColor, "Color"),
];

const ALPHA_LABELS: [(AlphaComposition, &str); 5] = [
    (AlphaComposition::Over, "Over"),
    (AlphaComposition::Atop, "Atop"),
    (AlphaComposition::Out, "Out"),
    (AlphaComposition::Conjoint, "Conjoint"),
    (AlphaComposition::Disjoint, "Disjoint"),
];

/// A field edit waiting for the bulk-overwrite answer.
#[derive(Clone, Copy)]
enum Field {
    DrawOrder(f32),
    Opacity(f32),
    Multiply(Color),
    Screen(Color),
    Angle(f32),
}

impl Field {
    fn apply(self, f: &mut FormFields) {
        match self {
            Field::DrawOrder(v) => f.draw_order = f.draw_order.map(|_| v),
            Field::Opacity(v) => f.opacity = v,
            Field::Multiply(c) => f.multiply = c,
            Field::Screen(c) => f.screen = c,
            Field::Angle(v) => f.angle = f.angle.map(|_| v),
        }
    }

    fn get(f: &FormFields, like: Field) -> Option<Field> {
        Some(match like {
            Field::DrawOrder(_) => Field::DrawOrder(f.draw_order?),
            Field::Opacity(_) => Field::Opacity(f.opacity),
            Field::Multiply(_) => Field::Multiply(f.multiply),
            Field::Screen(_) => Field::Screen(f.screen),
            Field::Angle(_) => Field::Angle(f.angle?),
        })
    }

    fn same(a: Field, b: Field) -> bool {
        match (a, b) {
            (Field::DrawOrder(x), Field::DrawOrder(y))
            | (Field::Opacity(x), Field::Opacity(y))
            | (Field::Angle(x), Field::Angle(y)) => x == y,
            (Field::Multiply(x), Field::Multiply(y)) | (Field::Screen(x), Field::Screen(y)) => {
                x == y
            }
            _ => false,
        }
    }

    /// "Multiple ArtMeshes with different draw orders are selected. …"
    fn question(self, kind: &str) -> String {
        let what = match self {
            Field::DrawOrder(_) => "draw orders",
            Field::Opacity(_) => "transparencies",
            Field::Multiply(_) => "multiply colors",
            Field::Screen(_) => "screen colors",
            Field::Angle(_) => "angles",
        };
        let field = match self {
            Field::DrawOrder(_) => "draw order",
            Field::Opacity(_) => "transparency",
            Field::Multiply(_) => "multiply color",
            Field::Screen(_) => "screen color",
            Field::Angle(_) => "angle",
        };
        format!(
            "Multiple {kind} with different {what} are selected. Do you want to collectively overwrite the {field} at once?"
        )
    }
}

fn kind_of(s: Selection) -> u8 {
    match s {
        Selection::ArtMesh(_) => 0,
        Selection::Part(_) => 1,
        Selection::Deformer(_) => 2,
        Selection::Glue(_) => 3,
    }
}

fn kind_name(s: Selection) -> &'static str {
    match s {
        Selection::ArtMesh(_) => "ArtMeshes",
        Selection::Part(_) => "parts",
        Selection::Deformer(_) => "deformers",
        Selection::Glue(_) => "glues",
    }
}

/// The selected objects of the primary's kind, with their current cells.
fn targets(doc: &Document, primary: Selection) -> Vec<(Selection, Option<usize>)> {
    let mut v: Vec<Selection> = doc
        .selection
        .objects
        .iter()
        .copied()
        .filter(|o| kind_of(*o) == kind_of(primary))
        .collect();
    if !v.contains(&primary) {
        v.insert(0, primary);
    }
    v.into_iter()
        .map(|o| (o, current_cell(&doc.model, o, &doc.values).ok()))
        .collect()
}

/// Writes `f` into every target that is on a key. One undo step.
fn apply(doc: &mut Document, tg: &[(Selection, Option<usize>)], f: Field) {
    let tg = tg.to_vec();
    let _ = doc.try_edit("Change Keyform", |m| {
        for (o, cell) in &tg {
            let Some(cell) = *cell else { continue };
            let Some(mut v) = form_fields(m, *o, cell) else {
                continue;
            };
            f.apply(&mut v);
            set_form_fields(m, *o, cell, &v).map_err(|e| e.to_string())?;
        }
        Ok(())
    });
}

fn color_edit(ui: &mut egui::Ui, c: &mut Color, enabled: bool) -> bool {
    let mut rgb = [c[0], c[1], c[2]];
    let r = ui
        .add_enabled_ui(enabled, |ui| ui.color_edit_button_rgb(&mut rgb))
        .inner;
    if r.changed() {
        *c = [rgb[0], rgb[1], rgb[2], c[3]];
        return true;
    }
    false
}

/// The keyform and source fields of the primary selection.
pub fn fields_ui(ui: &mut egui::Ui, doc: &mut Document) {
    let Some(s) = doc.primary() else { return };
    let editable = doc.editable();
    let tg = targets(doc, s);
    let cell = tg.iter().find(|(o, _)| *o == s).and_then(|(_, c)| *c);
    let cur = cell.and_then(|c| form_fields(&doc.model, s, c));
    let pending_id = egui::Id::new(("inspector-bulk", s));
    let mut edited: Option<Field> = None;
    let colors_ok = doc.model.target_version == 0 || doc.model.target_version >= COLOR_TARGET;

    if let Some(c) = cur {
        let values: Vec<Option<FormFields>> = tg
            .iter()
            .map(|(o, cell)| cell.and_then(|k| form_fields(&doc.model, *o, k)))
            .collect();
        let mixed = |like: Field| {
            let mine = Field::get(&c, like);
            values
                .iter()
                .flatten()
                .any(|v| match (Field::get(v, like), mine) {
                    (Some(a), Some(b)) => !Field::same(a, b),
                    _ => false,
                })
        };
        egui::Grid::new(("inspector-form", s))
            .num_columns(2)
            .show(ui, |ui| {
                if let Some(mut d) = c.draw_order {
                    ui.weak("Draw Order");
                    ui.horizontal(|ui| {
                        let mut dv = ui.add_enabled(
                            editable,
                            egui::DragValue::new(&mut d)
                                .range(0.0..=1000.0)
                                .speed(1.0)
                                .max_decimals(0),
                        );
                        if mixed(Field::DrawOrder(0.0)) {
                            dv = dv.on_hover_text("The selected objects differ");
                            ui.weak("(mixed)");
                        }
                        if dv.changed() {
                            edited = Some(Field::DrawOrder(d.round()));
                        }
                        if ui
                            .add_enabled(editable, egui::Button::new("Bulk Set"))
                            .on_hover_text("Bulk Set Draw Order: all keyforms of this object")
                            .clicked()
                        {
                            ui.data_mut(|m| m.insert_temp(pending_id.with("all"), d));
                        }
                    });
                    ui.end_row();
                }
                let mut o = c.opacity;
                ui.weak("Transparency");
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(editable, egui::Slider::new(&mut o, 0.0..=1.0))
                        .changed()
                    {
                        edited = Some(Field::Opacity(o));
                    }
                    if mixed(Field::Opacity(0.0)) {
                        ui.weak("(mixed)");
                    }
                });
                ui.end_row();
                let (mut mc, mut sc) = (c.multiply, c.screen);
                ui.weak("Multiply Color");
                if color_edit(ui, &mut mc, editable && colors_ok) {
                    edited = Some(Field::Multiply(mc));
                }
                ui.end_row();
                ui.weak("Screen Color");
                if color_edit(ui, &mut sc, editable && colors_ok) {
                    edited = Some(Field::Screen(sc));
                }
                ui.end_row();
                if let Some(mut a) = c.angle {
                    ui.weak("Angle");
                    if ui
                        .add_enabled(
                            editable,
                            egui::DragValue::new(&mut a).speed(0.5).suffix("°"),
                        )
                        .changed()
                    {
                        edited = Some(Field::Angle(a));
                    }
                    ui.end_row();
                }
            });
        if !colors_ok {
            ui.weak("Multiply and screen colors need SDK 4.2 or later as the target version.");
        }
    } else if !matches!(s, Selection::Glue(_)) {
        ui.weak("There is no keyform to edit at the current parameter values.");
    }

    // Multi-selection: ask before overwriting differing values.
    if let Some(f) = edited {
        let differs = tg.iter().any(|(o, cell)| {
            *o != s
                && cell
                    .and_then(|k| form_fields(&doc.model, *o, k))
                    .zip(cur)
                    .and_then(|(v, c)| Some((Field::get(&v, f)?, Field::get(&c, f)?)))
                    .is_some_and(|(a, b)| !Field::same(a, b))
        });
        if differs {
            ui.data_mut(|m| m.insert_temp(pending_id, PendingField(f)));
        } else {
            apply(doc, &tg, f);
        }
    }
    let ctx = ui.ctx().clone();
    if let Some(PendingField(f)) = ctx.data(|m| m.get_temp::<PendingField>(pending_id)) {
        let mut answer = None;
        egui::Modal::new(pending_id.with("modal")).show(&ctx, |ui| {
            ui.label(f.question(kind_name(s)));
            ui.horizontal(|ui| {
                if ui.button("Yes").clicked() {
                    answer = Some(true);
                }
                if ui.button("No").clicked() {
                    answer = Some(false);
                }
            });
        });
        if let Some(yes) = answer {
            ctx.data_mut(|m| m.remove::<PendingField>(pending_id));
            if yes {
                apply(doc, &tg, f);
            } else {
                // Only the primary object.
                let mine: Vec<_> = tg.iter().copied().filter(|(o, _)| *o == s).collect();
                apply(doc, &mine, f);
            }
        }
    }
    if let Some(d) = ctx.data(|m| m.get_temp::<f32>(pending_id.with("all"))) {
        let mut answer = None;
        egui::Modal::new(pending_id.with("all-modal")).show(&ctx, |ui| {
            ui.label(
                "Would you like to overwrite the draw order of all forms linked to this object with the current value?",
            );
            ui.horizontal(|ui| {
                if ui.button("Yes").clicked() {
                    answer = Some(true);
                }
                if ui.button("No").clicked() {
                    answer = Some(false);
                }
            });
        });
        if let Some(yes) = answer {
            ctx.data_mut(|m| m.remove::<f32>(pending_id.with("all")));
            if yes {
                let _ = doc.try_edit("Bulk Set Draw Order", |m| Ok(set_draw_order_all(m, s, d)));
            }
        }
    }

    source_ui(ui, doc, s, editable);
}

#[derive(Clone, Copy)]
struct PendingField(Field);

fn blend_combos(
    ui: &mut egui::Ui,
    id: (&str, usize),
    (c, a): (ColorComposition, AlphaComposition),
    editable: bool,
    target: i32,
) -> Option<(ColorComposition, AlphaComposition)> {
    let legacy = legacy_only(target);
    let (mut c2, mut a2) = (c, a);
    let label = |c: ColorComposition| {
        BLEND_LABELS
            .iter()
            .find(|(x, _)| *x == c)
            .map_or("?", |(_, l)| l)
    };
    let alabel = |a: AlphaComposition| {
        ALPHA_LABELS
            .iter()
            .find(|(x, _)| *x == a)
            .map_or("?", |(_, l)| l)
    };
    ui.add_enabled_ui(editable, |ui| {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("blend", id))
                .selected_text(label(c))
                .show_ui(ui, |ui| {
                    for (v, l) in BLEND_LABELS {
                        let ok = !legacy
                            || matches!(
                                v,
                                ColorComposition::Normal
                                    | ColorComposition::Add
                                    | ColorComposition::Multiply
                            );
                        if ok {
                            ui.selectable_value(&mut c2, v, l);
                        }
                    }
                });
            egui::ComboBox::from_id_salt(("alpha", id))
                .selected_text(alabel(a))
                .show_ui(ui, |ui| {
                    for (v, l) in ALPHA_LABELS {
                        if !legacy || v == AlphaComposition::Over {
                            ui.selectable_value(&mut a2, v, l);
                        }
                    }
                });
        });
    });
    ((c2, a2) != (c, a)).then_some((c2, a2))
}

/// Object-level settings (not per keyform).
fn source_ui(ui: &mut egui::Ui, doc: &mut Document, s: Selection, editable: bool) {
    let target = doc.model.target_version;
    match s {
        Selection::ArtMesh(i) => {
            let x = &doc.model.art_meshes[i];
            let blend = (x.color_composition, x.alpha_composition);
            ui.horizontal(|ui| {
                ui.weak("Blend mode");
                if let Some((c, a)) = blend_combos(ui, ("mesh", i), blend, editable, target) {
                    doc.edit("Change Blend Mode", |m| {
                        let x = Arc::make_mut(&mut m.art_meshes[i]);
                        x.color_composition = c;
                        x.alpha_composition = a;
                    });
                }
            });
        }
        Selection::Part(p) => {
            let x = &doc.model.parts[p];
            let (mut group, mut off) = (x.draw_order_group, x.offscreen);
            let blend = (x.color_composition, x.alpha_composition);
            if ui
                .add_enabled(
                    editable,
                    egui::Checkbox::new(&mut group, "Draw order group"),
                )
                .changed()
            {
                doc.edit("Change Draw Order Group", |m| {
                    Arc::make_mut(&mut m.parts[p]).draw_order_group = group;
                });
            }
            if ui
                .add_enabled(editable, egui::Checkbox::new(&mut off, "Offscreen drawing"))
                .changed()
            {
                doc.edit("Change Offscreen Drawing", |m| {
                    Arc::make_mut(&mut m.parts[p]).offscreen = off;
                });
            }
            if off {
                ui.horizontal(|ui| {
                    ui.weak("Blend mode");
                    if let Some((c, a)) = blend_combos(ui, ("part", p), blend, editable, target) {
                        doc.edit("Change Blend Mode", |m| {
                            let x = Arc::make_mut(&mut m.parts[p]);
                            x.color_composition = c;
                            x.alpha_composition = a;
                        });
                    }
                });
            }
        }
        Selection::Deformer(d) => {
            if let DeformerKind::Rotation(r) = &doc.model.deformers[d].kind {
                let (mut h, mut c) = (r.handle_length, r.circle_radius);
                egui::Grid::new(("rotation-source", d))
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.weak("Handle length");
                        let a = ui.add_enabled(
                            editable,
                            egui::DragValue::new(&mut h).range(5.0..=4000.0),
                        );
                        ui.end_row();
                        ui.weak("Circle radius");
                        let b = ui.add_enabled(
                            editable,
                            egui::DragValue::new(&mut c).range(10.0..=1000.0),
                        );
                        ui.end_row();
                        if a.changed() || b.changed() {
                            doc.edit("Change Rotation Deformer", |m| {
                                if let DeformerKind::Rotation(r) =
                                    &mut Arc::make_mut(&mut m.deformers[d]).kind
                                {
                                    r.handle_length = h;
                                    r.circle_radius = c;
                                }
                            });
                        }
                    });
            }
        }
        Selection::Glue(g) => {
            let Ok(cell) = current_cell(&doc.model, s, &doc.values) else {
                ui.weak("There is no keyform to edit at the current parameter values.");
                return;
            };
            let Some(mut v) = doc.model.glues[g]
                .grid
                .keyforms
                .get(cell)
                .map(|f| f.intensity)
            else {
                return;
            };
            ui.horizontal(|ui| {
                ui.weak("Intensity");
                if ui
                    .add_enabled(editable, egui::Slider::new(&mut v, 0.0..=1.0))
                    .changed()
                {
                    doc.edit("Change Glue Intensity", |m| {
                        Arc::make_mut(&mut m.glues[g]).grid.keyforms[cell].intensity = v;
                    });
                }
            });
        }
    }
}
