//! Parameter controllers in the Animation workspace: add single-point and
//! direction controllers on art meshes, drag them to solve their
//! parameters (keys at the playhead), export and import
//! `*.paramctrl3.json`.
//! spec: 60-animation/form-animation-and-controllers.md §2

use eframe::egui::{self, Pos2, Rect};
use keyform_anim::param_ctrl::{self as pc, Assessor, Controller, CtrlPoint, CtrlShape, Unknown};
use keyform_model::animation::TrackKind;

use crate::animation::UndoStep;
use crate::document::Document;

/// The controller tool on the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum CtrlTool {
    #[default]
    Off,
    /// Drag controller points (Select/Edit Controller).
    Drag,
    AddSingle,
    /// Add Direction Controller; the start point once placed.
    AddLine(Option<usize>),
}

/// A controller drag: (controller, which point: 0 = single/start, 1 = end).
pub struct CtrlDrag {
    pub ctrl: usize,
}

const PICK: f32 = 8.0;

/// The art mesh index a controller point is pinned to (by mesh ID).
fn doc_mesh(m: &keyform_model::Model, p: &CtrlPoint) -> Option<usize> {
    m.art_meshes.iter().position(|a| a.info.id == p.mesh_id)
}

impl Document {
    /// Scene pixels → model canvas pixels (inverse track placement).
    fn scene_to_model(&self, q: [f32; 2]) -> [f32; 2] {
        let Some(st) = self.anim.as_ref() else {
            return q;
        };
        let (Some((ti, _)), Some(s)) = (st.model_track(), st.scene()) else {
            return q;
        };
        let t = &s.tracks[ti];
        let x = st.frame - f64::from(t.start) + f64::from(t.offset);
        let pl = keyform_anim::scene::placement(t, x);
        let a = pl.anchor;
        let o = pl.apply(a);
        let ex = pl.apply([a[0] + 1.0, a[1]]);
        let ey = pl.apply([a[0], a[1] + 1.0]);
        let (m00, m01, m10, m11) = (ex[0] - o[0], ey[0] - o[0], ex[1] - o[1], ey[1] - o[1]);
        let det = m00 * m11 - m01 * m10;
        if det.abs() < 1e-12 {
            return q;
        }
        let d = [q[0] - o[0], q[1] - o[1]];
        [
            a[0] + (d[0] * m11 - d[1] * m01) / det,
            a[1] + (m00 * d[1] - m10 * d[0]) / det,
        ]
    }

    fn model_to_scene(&self, p: [f64; 2]) -> [f32; 2] {
        let q = [p[0] as f32, p[1] as f32];
        let Some(st) = self.anim.as_ref() else {
            return q;
        };
        let (Some((ti, _)), Some(s)) = (st.model_track(), st.scene()) else {
            return q;
        };
        let t = &s.tracks[ti];
        let x = st.frame - f64::from(t.start) + f64::from(t.offset);
        keyform_anim::scene::placement(t, x).apply(q)
    }

    /// Mesh positions (model canvas) at the playhead with `set` replacing
    /// some parameter values.
    fn ctrl_meshes(&self, set: &[(usize, f64)]) -> Option<Vec<Vec<f32>>> {
        let (mut values, parts) = self.anim_values_at(self.anim.as_ref()?.frame)?;
        for &(pi, v) in set {
            values[pi] = v as f32;
        }
        let ov = keyform_eval::Overrides {
            part_opacity: parts,
            ..Default::default()
        };
        let f = keyform_eval::Evaluator::new(&self.model).evaluate_full(
            &values,
            keyform_eval::EvalOptions::default(),
            &ov,
        );
        Some(f.meshes.into_iter().map(|m| m.positions).collect())
    }

    /// Pins a controller point at scene position `q` on the top-most drawn
    /// art mesh there.
    pub fn pin_ctrl_point(&mut self, q: [f32; 2]) -> Option<usize> {
        let m = self.scene_to_model(q);
        let at = [f64::from(m[0]), f64::from(m[1])];
        let frame = self.frame.as_ref()?;
        let meshes = self.ctrl_meshes(&[])?;
        let order: Vec<usize> = frame.render_order.iter().rev().copied().collect();
        for mi in order {
            if !frame.meshes[mi].is_drawn() {
                continue;
            }
            let am = &self.model.art_meshes[mi];
            let Some(mut p) = pc::pin(&am.info.name, &am.info.id, &meshes[mi], &am.indices, at)
            else {
                continue;
            };
            p.recovery_mesh = Some(am.info.guid.clone());
            // Only a mesh that contains the point.
            if p.bary.iter().all(|w| *w >= -1e-9)
                && (p.recovery[0] - at[0]).hypot(p.recovery[1] - at[1]) < 1e-3
            {
                let set = self.anim.as_mut()?.ctrls_mut()?;
                set.points.push(p);
                return Some(set.points.len() - 1);
            }
        }
        None
    }

    /// Adds a controller over `shape` driving the default parameters (head
    /// angles when the model has them).
    pub fn add_controller(&mut self, shape: CtrlShape) {
        let params: Vec<String> = ["ParamAngleX", "ParamAngleY"]
            .iter()
            .filter(|id| self.model.parameters.iter().any(|p| p.id == **id))
            .map(|s| (*s).to_owned())
            .collect();
        let Some(st) = self.anim.as_mut() else {
            return;
        };
        let before = st.doc.animation.clone();
        let Some(set) = st.ctrls_mut() else {
            return;
        };
        let n = set.controllers.len();
        set.controllers.push(Controller {
            guid: keyform_model::new_guid(),
            name: format!("Controller{}", n + 1),
            color: [0.9, 0.3, 0.2, 1.0],
            shape,
            params,
            target: None,
            stand_alone: false,
            select_lock: false,
        });
        st.undo
            .push(("Add Controller".into(), UndoStep::Anim(before)));
        st.redo.clear();
        st.dirty = true;
    }

    /// The scene position of a controller's handle (single point, or a
    /// line's end).
    fn ctrl_handle(&self, meshes: &[Vec<f32>], c: &Controller) -> Option<[f32; 2]> {
        let st = self.anim.as_ref()?;
        let pi = match c.shape {
            CtrlShape::Single(p) | CtrlShape::Line(_, p) => p,
        };
        let p = st.ctrls().points.get(pi)?;
        Some(self.model_to_scene(pc::point_position(
            p,
            meshes.get(doc_mesh(&self.model, p)?)?,
        )?))
    }

    /// Solves controller `ci` towards scene position `q` and writes the
    /// changed parameters as keys at the playhead (§2.2, §2.3).
    pub fn drag_controller(&mut self, ci: usize, q: [f32; 2]) {
        let m = self.scene_to_model(q);
        let t = [f64::from(m[0]), f64::from(m[1])];
        let Some(st) = self.anim.as_ref() else {
            return;
        };
        let Some(c) = st.ctrls().controllers.get(ci).cloned() else {
            return;
        };
        let assessor = match c.shape {
            CtrlShape::Single(_) => Assessor::Point(t),
            CtrlShape::Line(..) => Assessor::Aim { t, post: false },
        };
        let Some((values, _)) = self.anim_values_at(st.frame) else {
            return;
        };
        let pis: Vec<usize> = c
            .params
            .iter()
            .filter_map(|id| self.model.parameters.iter().position(|p| &p.id == id))
            .collect();
        let unknowns: Vec<Unknown> = pis
            .iter()
            .map(|&pi| {
                let p = &self.model.parameters[pi];
                Unknown {
                    value: f64::from(values[pi]),
                    min: f64::from(p.min),
                    max: f64::from(p.max),
                    repeat: p.repeat,
                }
            })
            .collect();
        let points = st.ctrls().points.clone();
        let mesh_of: Vec<Option<usize>> = points.iter().map(|p| doc_mesh(&self.model, p)).collect();
        let canvas = [
            f64::from(self.model.canvas.width),
            f64::from(self.model.canvas.height),
        ];
        let mut eval = |p: &[f64]| -> Vec<([f64; 2], [f64; 2])> {
            let set: Vec<(usize, f64)> = pis.iter().copied().zip(p.iter().copied()).collect();
            let Some(meshes) = self.ctrl_meshes(&set) else {
                return Vec::new();
            };
            let pos = |i: usize| {
                let pt = points.get(i)?;
                pc::point_position(pt, meshes.get((*mesh_of.get(i)?)?)?)
            };
            pc::effector_and_target(&c.shape, &assessor, &pos)
                .into_iter()
                .collect()
        };
        let solved = pc::solve(&unknowns, canvas, &mut eval);
        // Write keys where the curve value differs by more than 0.001.
        let Some(st) = self.anim.as_mut() else {
            return;
        };
        let Some(scene) = st.scene().cloned() else {
            return;
        };
        let Some((ti, _)) = st.model_track() else {
            return;
        };
        let tr = &scene.tracks[ti];
        let f = (st.frame - f64::from(tr.start) + f64::from(tr.offset)).round() as i32;
        let kind = scene.default_param_curve;
        let legacy = st.doc.animation.smooth_legacy;
        let s = st.scene;
        let TrackKind::Model(mt) = &mut st.doc.animation.scenes[s].tracks[ti].kind else {
            return;
        };
        for (k, &pi) in pis.iter().enumerate() {
            let id = format!("live2dParam_{}", self.model.parameters[pi].id);
            let Some(a) = mt.params.iter_mut().find(|a| a.id == id) else {
                continue;
            };
            let cur = keyform_anim::scene::eval_curve(&a.curve, f64::from(f));
            if (cur - solved[k]).abs() > 0.001 {
                let (lo, hi) = keyform_anim::scene::clamp_range(a);
                a.curve.fixed = false;
                keyform_anim::scene::set_key(&mut a.curve, f, solved[k], kind);
                keyform_anim::scene::refresh(&mut a.curve, lo, hi, legacy);
                st.dirty = true;
            }
        }
    }
}

impl Document {
    /// Solves the controllers that have a target (another controller) at
    /// scene `frame`: each one's effector is driven onto its target
    /// marker, all of them as one system. Returns (parameter index, value)
    /// for their parameters. spec: form-animation-and-controllers.md §2.1–2.3
    pub fn solve_ctrl_targets(&self, frame: f64) -> Vec<(usize, f64)> {
        let Some(st) = self.anim.as_ref() else {
            return Vec::new();
        };
        let set = st.ctrls();
        let active: Vec<usize> = (0..set.controllers.len())
            .filter(|&i| {
                set.controllers[i]
                    .target
                    .is_some_and(|(t, _)| t < set.controllers.len() && t != i)
            })
            .collect();
        if active.is_empty() {
            return Vec::new();
        }
        let Some((values, parts)) = self.anim_values_at(frame) else {
            return Vec::new();
        };
        let mut pis: Vec<usize> = Vec::new();
        for &i in &active {
            for id in &set.controllers[i].params {
                if let Some(pi) = self.model.parameters.iter().position(|p| &p.id == id) {
                    if !pis.contains(&pi) {
                        pis.push(pi);
                    }
                }
            }
        }
        let unknowns: Vec<Unknown> = pis
            .iter()
            .map(|&pi| {
                let p = &self.model.parameters[pi];
                Unknown {
                    value: f64::from(values[pi]),
                    min: f64::from(p.min),
                    max: f64::from(p.max),
                    repeat: p.repeat,
                }
            })
            .collect();
        let mesh_of: Vec<Option<usize>> = set
            .points
            .iter()
            .map(|p| doc_mesh(&self.model, p))
            .collect();
        let canvas = [
            f64::from(self.model.canvas.width),
            f64::from(self.model.canvas.height),
        ];
        let mut eval = |p: &[f64]| -> Vec<([f64; 2], [f64; 2])> {
            let mut v = values.clone();
            for (k, &pi) in pis.iter().enumerate() {
                v[pi] = p[k] as f32;
            }
            let ov = keyform_eval::Overrides {
                part_opacity: parts.clone(),
                ..Default::default()
            };
            let f = keyform_eval::Evaluator::new(&self.model).evaluate_full(
                &v,
                keyform_eval::EvalOptions::default(),
                &ov,
            );
            let pos = |i: usize| {
                pc::point_position(
                    set.points.get(i)?,
                    &f.meshes.get((*mesh_of.get(i)?)?)?.positions,
                )
            };
            active
                .iter()
                .filter_map(|&i| {
                    let c = &set.controllers[i];
                    let (ti, ext) = c.target?;
                    let tc = &set.controllers[ti];
                    let a = match (&c.shape, &tc.shape) {
                        (CtrlShape::Single(_), CtrlShape::Single(q)) => Assessor::Point(pos(*q)?),
                        (CtrlShape::Single(_), CtrlShape::Line(a, b)) => Assessor::Area {
                            a: pos(*a)?,
                            b: pos(*b)?,
                            pre: ext,
                            post: ext,
                        },
                        (CtrlShape::Line(..), CtrlShape::Single(q)) => Assessor::Aim {
                            t: pos(*q)?,
                            post: ext,
                        },
                        (CtrlShape::Line(..), CtrlShape::Line(a, b)) => {
                            let (a, b) = (pos(*a)?, pos(*b)?);
                            Assessor::Direction([b[0] - a[0], b[1] - a[1]])
                        }
                    };
                    pc::effector_and_target(&c.shape, &a, &pos)
                })
                .collect()
        };
        let solved = pc::solve(&unknowns, canvas, &mut eval);
        pis.into_iter().zip(solved).collect()
    }

    /// Bake Animation from Parameter Controller: solves the controllers'
    /// targets at every frame of the scene and writes the results as keys
    /// (default curve type), one undo step. Returns the number of keys.
    /// spec: form-animation-and-controllers.md §2.3
    pub fn bake_controllers(&mut self) -> Result<usize, String> {
        let st = self.anim.as_ref().ok_or("No animation is open.")?;
        let scene = st.scene().ok_or("No scene.")?.clone();
        let (ti, _) = st.model_track().ok_or("This scene has no model track.")?;
        let tr = &scene.tracks[ti];
        let mut per_frame = Vec::new();
        for f in 0..=scene.duration {
            per_frame.push((f, self.solve_ctrl_targets(f64::from(f))));
        }
        if per_frame.iter().all(|(_, v)| v.is_empty()) {
            return Err("No parameter controller has a target.".into());
        }
        let shift = tr.offset - tr.start;
        let ids: Vec<String> = self
            .model
            .parameters
            .iter()
            .map(|p| format!("live2dParam_{}", p.id))
            .collect();
        let st = self.anim.as_mut().ok_or("No animation is open.")?;
        let before = st.doc.animation.clone();
        let legacy = before.smooth_legacy;
        let s = st.scene;
        let TrackKind::Model(mt) = &mut st.doc.animation.scenes[s].tracks[ti].kind else {
            return Err("no model track".into());
        };
        let mut n = 0;
        let mut touched = Vec::new();
        for (f, vals) in &per_frame {
            for &(pi, v) in vals {
                if let Some(k) = mt.params.iter().position(|a| a.id == ids[pi]) {
                    let a = &mut mt.params[k];
                    a.curve.fixed = false;
                    keyform_anim::scene::set_key(
                        &mut a.curve,
                        f + shift,
                        v,
                        scene.default_param_curve,
                    );
                    n += 1;
                    if !touched.contains(&k) {
                        touched.push(k);
                    }
                }
            }
        }
        for k in touched {
            let a = &mut mt.params[k];
            let (lo, hi) = keyform_anim::scene::clamp_range(a);
            keyform_anim::scene::refresh(&mut a.curve, lo, hi, legacy);
        }
        st.undo.push((
            "Bake Animation from Parameter Controller".into(),
            UndoStep::Anim(before),
        ));
        st.redo.clear();
        st.dirty = true;
        Ok(n)
    }
}

/// Canvas input for the controller tools.
pub fn input(ui: &egui::Ui, response: &egui::Response, doc: &mut Document, rect: Rect, ppp: f32) {
    let view = crate::canvas::view_of(doc, rect, ppp);
    let to_canvas = |p: Pos2| view.to_canvas([(p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp]);
    let primary = egui::PointerButton::Primary;
    let Some(tool) = doc.anim.as_ref().map(|a| a.ctrl_tool) else {
        return;
    };
    match tool {
        CtrlTool::Off => {}
        CtrlTool::AddSingle | CtrlTool::AddLine(_) => {
            if !response.clicked_by(primary) {
                return;
            }
            let Some(p) = response.interact_pointer_pos() else {
                return;
            };
            let Some(pi) = doc.pin_ctrl_point(to_canvas(p)) else {
                doc.notice = Some("Click on an art mesh to place the controller point.".into());
                return;
            };
            match tool {
                CtrlTool::AddSingle => doc.add_controller(CtrlShape::Single(pi)),
                CtrlTool::AddLine(None) => {
                    if let Some(a) = doc.anim.as_mut() {
                        a.ctrl_tool = CtrlTool::AddLine(Some(pi));
                    }
                }
                CtrlTool::AddLine(Some(s)) => {
                    doc.add_controller(CtrlShape::Line(s, pi));
                    if let Some(a) = doc.anim.as_mut() {
                        a.ctrl_tool = CtrlTool::AddLine(None);
                    }
                }
                _ => {}
            }
        }
        CtrlTool::Drag => {
            if response.drag_started_by(primary) {
                let Some(press) = ui.input(|i| i.pointer.press_origin()) else {
                    return;
                };
                let q = to_canvas(press);
                let r = PICK / doc.view.zoom.max(1e-6);
                let Some(meshes) = doc.ctrl_meshes(&[]) else {
                    return;
                };
                let Some(st) = doc.anim.as_ref() else {
                    return;
                };
                let hit = st
                    .ctrls()
                    .controllers
                    .iter()
                    .enumerate()
                    .find_map(|(i, c)| {
                        let h = doc.ctrl_handle(&meshes, c)?;
                        ((h[0] - q[0]).hypot(h[1] - q[1]) <= r).then_some(i)
                    });
                if let (Some(ci), Some(st)) = (hit, doc.anim.as_mut()) {
                    let before = st.doc.animation.clone();
                    st.undo
                        .push(("Parameter Controller".into(), UndoStep::Anim(before)));
                    st.redo.clear();
                    st.ctrl_drag = Some(CtrlDrag { ctrl: ci });
                }
            } else if response.dragged_by(primary) {
                let (Some(p), Some(ci)) = (
                    ui.input(|i| i.pointer.interact_pos()),
                    doc.anim
                        .as_ref()
                        .and_then(|a| a.ctrl_drag.as_ref())
                        .map(|d| d.ctrl),
                ) else {
                    return;
                };
                doc.drag_controller(ci, to_canvas(p));
            } else if response.drag_stopped() {
                if let Some(a) = doc.anim.as_mut() {
                    a.ctrl_drag = None;
                }
            }
        }
    }
}

/// Draws the controllers: points, direction lines and handles.
pub fn draw(painter: &egui::Painter, doc: &Document, to_screen: &dyn Fn([f32; 2]) -> Pos2) {
    let Some(st) = doc.anim.as_ref() else {
        return;
    };
    if st.ctrls().controllers.is_empty() && st.ctrls().points.is_empty() {
        return;
    }
    let Some(meshes) = doc.ctrl_meshes(&[]) else {
        return;
    };
    let at = |i: usize| -> Option<Pos2> {
        let p = st.ctrls().points.get(i)?;
        Some(to_screen(doc.model_to_scene(pc::point_position(
            p,
            meshes.get(doc_mesh(&doc.model, p)?)?,
        )?)))
    };
    for c in &st.ctrls().controllers {
        let col = egui::Color32::from_rgba_unmultiplied(
            (c.color[0] * 255.0) as u8,
            (c.color[1] * 255.0) as u8,
            (c.color[2] * 255.0) as u8,
            255,
        );
        match c.shape {
            CtrlShape::Single(p) => {
                if let Some(q) = at(p) {
                    painter.circle_filled(q, 5.0, col);
                    painter.circle_stroke(q, 7.0, egui::Stroke::new(1.0, egui::Color32::WHITE));
                }
            }
            CtrlShape::Line(s, e) => {
                if let (Some(a), Some(b)) = (at(s), at(e)) {
                    painter.line_segment([a, b], egui::Stroke::new(2.0, col));
                    painter.circle_filled(b, 5.0, col);
                }
            }
        }
    }
    if let CtrlTool::AddLine(Some(s)) = st.ctrl_tool {
        if let Some(a) = at(s) {
            painter.circle_stroke(a, 6.0, egui::Stroke::new(1.5, egui::Color32::YELLOW));
        }
    }
}

/// A file operation chosen in the Controllers menu.
pub enum CtrlFileOp {
    Export(std::path::PathBuf),
    Import(std::path::PathBuf),
    Bake,
}

/// The Controllers menu: tools, the controller list (parameters, delete),
/// export and import of `*.paramctrl3.json`.
pub fn menu(
    ui: &mut egui::Ui,
    st: &mut crate::animation::AnimState,
    params: &[String],
) -> Option<CtrlFileOp> {
    let mut op = None;
    let t = &mut st.ctrl_tool;
    ui.selectable_value(t, CtrlTool::Drag, "Select/Edit Controller");
    ui.selectable_value(t, CtrlTool::AddSingle, "Add Single Point Controller");
    if ui
        .selectable_label(
            matches!(t, CtrlTool::AddLine(_)),
            "Add Direction Controller",
        )
        .clicked()
    {
        *t = CtrlTool::AddLine(None);
    }
    ui.selectable_value(t, CtrlTool::Off, "No controller tool");
    ui.separator();
    ui.checkbox(&mut st.ctrl_follow, "Target follow")
        .on_hover_text("During playback, controllers with a target follow it");
    if ui
        .button("Bake Animation from Parameter Controller")
        .clicked()
    {
        op = Some(CtrlFileOp::Bake);
        ui.close();
    }
    ui.separator();
    let mut delete = None;
    let before = st.doc.animation.clone();
    let mut changed = false;
    let empty = &mut keyform_anim::param_ctrl::ControllerSet::default();
    let set = st.ctrls_mut().unwrap_or(empty);
    for (i, c) in set.controllers.iter_mut().enumerate() {
        ui.menu_button(c.name.clone(), |ui| {
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    for id in params {
                        let mut on = c.params.contains(id);
                        if ui.checkbox(&mut on, id.as_str()).changed() {
                            if on {
                                c.params.push(id.clone());
                            } else {
                                c.params.retain(|x| x != id);
                            }
                            changed = true;
                        }
                    }
                });
            if ui.button("Delete Controller").clicked() {
                delete = Some(i);
                ui.close();
            }
        });
    }
    if let Some(i) = delete {
        set.controllers.remove(i);
        changed = true;
    }
    if changed {
        st.undo
            .push(("Edit Controller".into(), UndoStep::Anim(before)));
        st.redo.clear();
        st.dirty = true;
    }
    ui.separator();
    if ui.button("Export Parameter Controller…").clicked() {
        ui.close();
        op = rfd::FileDialog::new()
            .add_filter("Parameter controller", &["json"])
            .set_file_name("controllers.paramctrl3.json")
            .save_file()
            .map(CtrlFileOp::Export);
    }
    if ui.button("Import Parameter Controller…").clicked() {
        ui.close();
        op = rfd::FileDialog::new()
            .add_filter("Parameter controller", &["json"])
            .pick_file()
            .map(CtrlFileOp::Import);
    }
    op
}

/// Runs a Controllers menu file operation.
pub fn run_file_op(doc: &mut Document, op: CtrlFileOp) -> Result<(), String> {
    match op {
        CtrlFileOp::Export(p) => {
            let st = doc.anim.as_ref().ok_or("No animation is open.")?;
            let text = pc::export_json(st.ctrls());
            std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))
        }
        CtrlFileOp::Import(p) => doc.import_controllers(&p),
        CtrlFileOp::Bake => doc.bake_controllers().map(|_| ()),
    }
}

impl Document {
    /// Imports `*.paramctrl3.json`: pins each point onto its art mesh at
    /// its stored position, and adds everything to the track's controllers
    /// (§2.4).
    pub fn import_controllers(&mut self, path: &std::path::Path) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let im = pc::import_json(&text).map_err(|e| e.to_string())?;
        let meshes = self.ctrl_meshes(&[]).ok_or("No animation is open.")?;
        let mut points = Vec::new();
        for (obj, name, at) in &im.points {
            let mi = self
                .model
                .art_meshes
                .iter()
                .position(|m| &m.info.id == obj)
                .ok_or_else(|| format!("No art mesh {obj}."))?;
            let am = &self.model.art_meshes[mi];
            let mut p = pc::pin(name, obj, &meshes[mi], &am.indices, *at)
                .ok_or_else(|| format!("Can't pin {name} on {obj}."))?;
            p.recovery_mesh = Some(am.info.guid.clone());
            points.push(p);
        }
        let st = self.anim.as_mut().ok_or("No animation is open.")?;
        let before = st.doc.animation.clone();
        let set = st.ctrls_mut().ok_or("This scene has no model track.")?;
        let base = set.points.len();
        let cbase = set.controllers.len();
        set.points.extend(points);
        for mut c in im.controllers {
            c.shape = match c.shape {
                CtrlShape::Single(p) => CtrlShape::Single(p + base),
                CtrlShape::Line(a, b) => CtrlShape::Line(a + base, b + base),
            };
            c.target = c.target.map(|(t, e)| (t + cbase, e));
            set.controllers.push(c);
        }
        st.undo
            .push(("Import Parameter Controller".into(), UndoStep::Anim(before)));
        st.redo.clear();
        st.dirty = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::log::Log;
    use keyform_anim::param_ctrl::CtrlShape;
    use std::path::Path;

    use super::*;

    /// A single-point controller on the face, dragged to the right: the
    /// solver turns the head (ParamAngleX keys at the playhead) so the
    /// point follows; export and import keep it.
    #[test]
    fn drag_single_point_controller() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = std::env::var_os("KEYFORM_BB_MODEL")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| root.join("hiyori_pro_t11.cmo3"));
        if !model.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        doc.frame = doc.anim_frame_at(0.0);
        // A point on the drawn mesh ParamAngleX moves most, in scene pixels.
        let ax = doc
            .model
            .parameters
            .iter()
            .position(|p| p.id == "ParamAngleX")
            .unwrap();
        let (m0, m1) = (
            doc.ctrl_meshes(&[]).unwrap(),
            doc.ctrl_meshes(&[(ax, 20.0)]).unwrap(),
        );
        let fr = doc.frame.clone().unwrap();
        let moved = |i: usize| (m1[i][0] - m0[i][0]).hypot(m1[i][1] - m0[i][1]);
        let face = (0..m0.len())
            .filter(|&i| fr.meshes[i].is_drawn() && !m0[i].is_empty())
            .max_by(|&a, &b| moved(a).total_cmp(&moved(b)))
            .unwrap();
        let p = &fr.meshes[face].positions;
        let c = [p[0], p[1]];
        let pi = doc.pin_ctrl_point(c).expect("pinned");
        doc.add_controller(CtrlShape::Single(pi));
        let meshes = doc.ctrl_meshes(&[]).unwrap();
        let st = doc.anim.as_ref().unwrap();
        let h0 = doc
            .ctrl_handle(&meshes, &st.ctrls().controllers[0])
            .unwrap();
        let goal = [h0[0] + 15.0, h0[1]];
        doc.drag_controller(0, goal);
        let meshes = doc.ctrl_meshes(&[]).unwrap();
        let st = doc.anim.as_ref().unwrap();
        let h1 = doc
            .ctrl_handle(&meshes, &st.ctrls().controllers[0])
            .unwrap();
        assert!(h1[0] - h0[0] > 5.0, "{h0:?} → {h1:?}");
        let (_, m) = st.model_track().unwrap();
        let ax = m
            .params
            .iter()
            .find(|a| a.id == "live2dParam_ParamAngleX")
            .unwrap();
        assert!(!ax.curve.keys.is_empty());
        // Export → import adds an equal controller.
        let text = pc::export_json(st.ctrls());
        let f = std::env::temp_dir().join(format!("kf-{}.paramctrl3.json", std::process::id()));
        std::fs::write(&f, text).unwrap();
        doc.import_controllers(&f).unwrap();
        let st = doc.anim.as_ref().unwrap();
        assert_eq!(st.ctrls().controllers.len(), 2);
        assert_eq!(st.ctrls().points[1].mesh_id, st.ctrls().points[0].mesh_id);
        // A target, then save and reopen: the controllers come back (§3.2).
        doc.anim.as_mut().unwrap().ctrls_mut().unwrap().controllers[1].target = Some((0, false));
        let set = doc.anim.as_ref().unwrap().ctrls().clone();
        let tmp = std::env::temp_dir().join(format!("kf-ctrl-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let out = tmp.join("ctrl.can3");
        let model_copy = out.with_file_name(model.file_name().unwrap());
        let _ = std::fs::copy(&model, &model_copy);
        doc.save_animation_to(Some(&out)).unwrap();
        if let Ok(dir) = std::env::var("KEYFORM_BB_DIR") {
            let _ = std::fs::copy(&out, Path::new(&dir).join("ctrl_kf.can3"));
        }
        let back = Document::open_animation(&out, &mut log).unwrap();
        assert_eq!(back.anim.as_ref().unwrap().ctrls(), &set);
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&model_copy);
        let _ = std::fs::remove_file(f);
    }

    /// A controller targeting another: Bake writes keys on every frame and
    /// brings the effector closer to the target marker.
    #[test]
    fn bake_controller_target() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = root.join("hiyori_pro_t11.cmo3");
        if !model.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        doc.frame = doc.anim_frame_at(0.0);
        let ax = doc
            .model
            .parameters
            .iter()
            .position(|p| p.id == "ParamAngleX")
            .unwrap();
        let (m0, m1) = (
            doc.ctrl_meshes(&[]).unwrap(),
            doc.ctrl_meshes(&[(ax, 20.0)]).unwrap(),
        );
        let fr = doc.frame.clone().unwrap();
        let moved = |i: usize| (m1[i][0] - m0[i][0]).hypot(m1[i][1] - m0[i][1]);
        let drawn: Vec<usize> = (0..m0.len())
            .filter(|&i| fr.meshes[i].is_drawn() && !m0[i].is_empty())
            .collect();
        let mover = *drawn
            .iter()
            .max_by(|&&a, &&b| moved(a).total_cmp(&moved(b)))
            .unwrap();
        let still = *drawn
            .iter()
            .min_by(|&&a, &&b| moved(a).total_cmp(&moved(b)))
            .unwrap();
        let at = |i: usize| [fr.meshes[i].positions[0], fr.meshes[i].positions[1]];
        let pa = doc.pin_ctrl_point(at(mover)).unwrap();
        doc.add_controller(CtrlShape::Single(pa));
        let pb = doc.pin_ctrl_point(at(still)).unwrap();
        doc.add_controller(CtrlShape::Single(pb));
        {
            let set = doc.anim.as_mut().unwrap().ctrls_mut().unwrap();
            set.controllers[0].target = Some((1, false));
            set.controllers[0].params = vec!["ParamAngleX".into()];
            set.controllers[1].params.clear();
        }
        let dist = |doc: &Document| {
            let m = doc.ctrl_meshes(&[]).unwrap();
            let st = doc.anim.as_ref().unwrap();
            let (a, b) = (
                doc.ctrl_handle(&m, &st.ctrls().controllers[0]).unwrap(),
                doc.ctrl_handle(&m, &st.ctrls().controllers[1]).unwrap(),
            );
            (a[0] - b[0]).hypot(a[1] - b[1])
        };
        let d0 = dist(&doc);
        let n = doc.bake_controllers().unwrap();
        let dur = doc.anim.as_ref().unwrap().scene().unwrap().duration;
        assert_eq!(n, (dur + 1) as usize);
        let d1 = dist(&doc);
        assert!(d1 < d0, "{d0} → {d1}");
    }
}
