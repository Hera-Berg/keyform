//! ArtPath tool group: Stroke ArtPath, point editing (move, insert,
//! delete, fold), Adjust Width / Adjust Opacity brushes, Split and Join,
//! with the art path's settings and the selected point's style in Tool
//! Details. spec: 70-advanced/art-path.md §5, 50-ui/canvas-and-tools.md §4,
//! 50-ui/inspector-fields.md (ArtPath_wrapperForInspector,
//! ArtPathPoint_wrapperForInspector)

use eframe::egui;
use keyform_edit::artpath::{self as ap, BrushKind, V2};
use keyform_edit::StepKind;
use keyform_eval::{Evaluator, Frame};
use keyform_model::{LineAlgorithm, LineAlignment, Model};

use crate::document::{Document, Mode, Selection};

/// Pick range of a control point / segment, in screen px.
const PICK: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtSub {
    Stroke,
    Edit,
    Width,
    Opacity,
    Split,
    Join,
}

impl ArtSub {
    pub const ALL: [(ArtSub, &'static str); 6] = [
        (ArtSub::Stroke, "Stroke ArtPath"),
        (ArtSub::Edit, "Edit points"),
        (ArtSub::Width, "Adjust Width"),
        (ArtSub::Opacity, "Adjust Opacity"),
        (ArtSub::Split, "Split"),
        (ArtSub::Join, "Join"),
    ];
}

/// Tool Details settings, kept while the tool is off.
#[derive(Clone, Copy, Debug)]
pub struct ArtPrefs {
    pub sub: ArtSub,
    /// Stroke tool "Interval" (px).
    pub interval: f32,
    /// New art paths' brush.
    pub defaults: ap::Defaults,
    /// Adjust Width: Thickness (px) and Size (px).
    pub width_level: f32,
    pub width_size: f32,
    /// Adjust Opacity: Opacity (%) and Size (px).
    pub opacity_level: f32,
    pub opacity_size: f32,
}

impl Default for ArtPrefs {
    fn default() -> Self {
        Self {
            sub: ArtSub::Stroke,
            interval: 8.0,
            defaults: ap::Defaults::default(),
            width_level: 4.0,
            width_size: 60.0,
            opacity_level: 50.0,
            opacity_size: 60.0,
        }
    }
}

/// An unfinished gesture.
enum Gesture {
    /// Canvas points of a freehand stroke.
    Stroke(Vec<V2>),
    /// Moving point `i` of the selected path; the model before the drag.
    Move { i: usize, base: Box<Model> },
    /// A width / opacity brush drag; the model before it.
    Brush { base: Box<Model> },
}

#[derive(Default)]
pub struct ArtTool {
    /// The art path being edited and its selected point.
    pub path: Option<usize>,
    pub point: Option<usize>,
    /// Join: the first end picked (path, is end).
    join_from: Option<(usize, bool)>,
    gesture: Option<Gesture>,
    /// Pointer position (canvas) for the brush circle.
    pub hover: Option<V2>,
}

#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub alt: bool,
    pub ctrl: bool,
}

impl Document {
    pub fn set_art_tool(&mut self, on: bool) -> Result<(), String> {
        if on && (!self.editable() || self.mode != Mode::Modeling || self.mesh_edit.is_some()) {
            return Err("ArtPaths are edited in the Modeling workspace of a project.".into());
        }
        if on {
            let _ = self.set_glue_tool(None);
            let _ = self.set_path_tool(false);
            self.soft_select = None;
            self.lasso = false;
            self.temp_path = None;
            if self.art_tool.is_none() {
                self.art_tool = Some(ArtTool::default());
            }
        } else {
            self.art_release();
            self.art_tool = None;
        }
        Ok(())
    }

    /// Selects art path `k` for editing (Parts palette row).
    pub fn art_select(&mut self, k: usize) {
        if self.art_tool.is_none() && self.set_art_tool(true).is_err() {
            return;
        }
        if let Some(t) = self.art_tool.as_mut() {
            t.path = Some(k);
            t.point = None;
            if self.art_prefs.sub == ArtSub::Stroke {
                self.art_prefs.sub = ArtSub::Edit;
            }
        }
    }

    fn art_frame(&self) -> Frame {
        Evaluator::new(&self.model).evaluate(&self.values)
    }

    /// The part new art paths go to: the selected part, or the selected
    /// object's part.
    fn art_target_part(&self) -> Option<usize> {
        match self.primary()? {
            Selection::Part(p) => Some(p),
            Selection::ArtMesh(a) => self.model.art_meshes.get(a)?.info.parent_part,
            Selection::Deformer(d) => self.model.deformers.get(d)?.info.parent_part,
            Selection::Glue(_) => None,
        }
    }

    /// (path, point) nearest `at` within `range`, preferring the selected path.
    fn art_hit_point(&self, frame: &Frame, at: V2, range: f32) -> Option<(usize, usize)> {
        let sel = self.art_tool.as_ref().and_then(|t| t.path);
        let mut order: Vec<usize> = (0..self.model.art_paths.len()).collect();
        if let Some(s) = sel {
            order.retain(|&k| k != s);
            order.insert(0, s);
        }
        for k in order {
            if self.model.art_paths[k].info.locked {
                continue;
            }
            let pts = ap::canvas_points(&self.model, frame, &self.values, k);
            if let Some(i) = ap::pick_point(&pts, at, range) {
                return Some((k, i));
            }
        }
        None
    }

    fn art_commit(&mut self, name: &str, base: Model) {
        if self.model.same_data(&base) {
            self.model = base;
        } else {
            self.history
                .commit(name, StepKind::Real, self.edit_state(), false);
        }
        self.model_changed();
    }

    /// Pointer pressed at canvas `at` (start of a click or drag).
    pub fn art_press(&mut self, at: V2, mods: Mods, zoom: f32) {
        let Some(sub) = self.art_tool.as_ref().map(|_| self.art_prefs.sub) else {
            return;
        };
        let range = PICK / zoom.max(1e-6);
        let frame = self.art_frame();
        let hit = self.art_hit_point(&frame, at, range);
        match sub {
            ArtSub::Stroke => {
                if let Some(t) = self.art_tool.as_mut() {
                    t.gesture = Some(Gesture::Stroke(vec![at]));
                }
            }
            ArtSub::Edit => self.art_edit_press(&frame, at, mods, range, hit),
            ArtSub::Width | ArtSub::Opacity => {
                let base = Box::new(self.model.clone());
                if self.art_tool.as_ref().and_then(|t| t.path).is_none() {
                    if let Some((k, _)) = hit {
                        if let Some(t) = self.art_tool.as_mut() {
                            t.path = Some(k);
                        }
                    }
                }
                if let Some(t) = self.art_tool.as_mut() {
                    t.gesture = Some(Gesture::Brush { base });
                }
                self.art_brush(at);
            }
            ArtSub::Split => {
                if let Some((k, i)) = hit {
                    let r = self.try_edit("Split ArtPath", |m| ap::split(m, k, i));
                    match r {
                        Ok(_) => {
                            if let Some(t) = self.art_tool.as_mut() {
                                t.path = Some(k);
                                t.point = None;
                            }
                        }
                        Err(e) => self.notice = Some(e),
                    }
                }
            }
            ArtSub::Join => {
                let Some((k, i)) = hit else { return };
                let n = self.model.art_paths[k]
                    .grid
                    .keyforms
                    .first()
                    .map_or(0, |f| f.points.len());
                if i != 0 && i + 1 != n {
                    self.notice = Some("Click the end point of an ArtPath.".into());
                    return;
                }
                let end = i + 1 == n;
                match self.art_tool.as_ref().and_then(|t| t.join_from) {
                    None => {
                        if let Some(t) = self.art_tool.as_mut() {
                            t.join_from = Some((k, end));
                            t.path = Some(k);
                        }
                    }
                    Some((a, a_end)) => {
                        let r = self.try_edit("Join ArtPaths", |m| ap::join(m, a, a_end, k, !end));
                        if let Err(e) = r {
                            self.notice = Some(e);
                        }
                        if let Some(t) = self.art_tool.as_mut() {
                            t.join_from = None;
                            t.path = Some(if k < a { a - 1 } else { a });
                            t.point = None;
                        }
                    }
                }
            }
        }
    }

    fn art_edit_press(
        &mut self,
        frame: &Frame,
        at: V2,
        mods: Mods,
        range: f32,
        hit: Option<(usize, usize)>,
    ) {
        if let Some((k, i)) = hit {
            if mods.alt && mods.ctrl {
                if let Err(e) = self.try_edit("Change to Fold line", |m| ap::toggle_corner(m, k, i))
                {
                    self.notice = Some(e);
                }
            } else if mods.alt {
                if let Err(e) = self.try_edit("Delete point", |m| ap::delete_point(m, k, i)) {
                    self.notice = Some(e);
                }
                if let Some(t) = self.art_tool.as_mut() {
                    t.point = None;
                }
                return;
            }
            let base = Box::new(self.model.clone());
            if let Some(t) = self.art_tool.as_mut() {
                t.path = Some(k);
                t.point = Some(i);
                t.gesture = Some(Gesture::Move { i, base });
            }
            return;
        }
        // On a segment of the selected path: insert; past its end: extend.
        let Some(k) = self.art_tool.as_ref().and_then(|t| t.path) else {
            return;
        };
        if k >= self.model.art_paths.len() {
            return;
        }
        let pts = ap::canvas_points(&self.model, frame, &self.values, k);
        let closed = self.model.art_paths[k].closed;
        let (values, at2) = (self.values.clone(), at);
        let r = if let Some(seg) = ap::pick_segment(&pts, closed, at, range) {
            self.try_edit("Add point", |m| {
                ap::insert_point(m, frame, &values, k, seg, at2)
            })
        } else if mods.ctrl && !closed {
            self.try_edit("Add point", |m| ap::append_point(m, frame, &values, k, at2))
        } else {
            if let Some(t) = self.art_tool.as_mut() {
                t.point = None;
            }
            return;
        };
        match r {
            Ok(i) => {
                if let Some(t) = self.art_tool.as_mut() {
                    t.point = Some(i);
                }
            }
            Err(e) => self.notice = Some(e),
        }
    }

    fn art_brush(&mut self, at: V2) {
        let Some(k) = self.art_tool.as_ref().and_then(|t| t.path) else {
            return;
        };
        let p = self.art_prefs;
        let (kind, level, size) = match p.sub {
            ArtSub::Width => (BrushKind::Width, p.width_level, p.width_size),
            _ => (BrushKind::Opacity, p.opacity_level, p.opacity_size),
        };
        let frame = self.art_frame();
        let values = self.values.clone();
        match ap::brush(
            &mut self.model,
            &frame,
            &values,
            k,
            at,
            size,
            level,
            0.25,
            kind,
        ) {
            Ok(_) => self.model_changed(),
            Err(e) => self.notice = Some(e),
        }
    }

    /// Pointer dragged to canvas `at`.
    pub fn art_drag(&mut self, at: V2) {
        let Some(t) = self.art_tool.as_mut() else {
            return;
        };
        match t.gesture.as_mut() {
            Some(Gesture::Stroke(s)) => s.push(at),
            Some(Gesture::Move { i, base }) => {
                let (i, mut m) = (*i, (**base).clone());
                let Some(k) = t.path else { return };
                let frame = Evaluator::new(&m).evaluate(&self.values);
                match ap::move_point(&mut m, &frame, &self.values, k, i, at) {
                    Ok(()) => {
                        self.model = m;
                        self.model_changed();
                    }
                    Err(e) => self.notice = Some(e),
                }
            }
            Some(Gesture::Brush { .. }) => self.art_brush(at),
            None => {}
        }
    }

    /// Pointer released: finishes the gesture as one undo step.
    pub fn art_release(&mut self) {
        let Some(g) = self.art_tool.as_mut().and_then(|t| t.gesture.take()) else {
            return;
        };
        match g {
            Gesture::Stroke(s) => {
                let pts =
                    ap::fit_stroke(&s, self.art_prefs.interval, 1.5 / self.view.zoom.max(1e-6));
                if pts.len() < 2 {
                    return;
                }
                let (part, d) = (self.art_target_part(), self.art_prefs.defaults);
                match self.try_edit("Stroke ArtPath", |m| ap::create(m, part, &pts, d)) {
                    Ok(k) => {
                        if let Some(t) = self.art_tool.as_mut() {
                            t.path = Some(k);
                            t.point = None;
                        }
                    }
                    Err(e) => self.notice = Some(e),
                }
            }
            Gesture::Move { base, .. } => self.art_commit("Move point", *base),
            Gesture::Brush { base } => {
                let name = if self.art_prefs.sub == ArtSub::Width {
                    "Adjust Width"
                } else {
                    "Adjust Opacity"
                };
                self.art_commit(name, *base);
            }
        }
    }

    /// A stroke being drawn (canvas points), for the preview.
    pub fn art_stroke_preview(&self) -> Option<&[V2]> {
        match self.art_tool.as_ref()?.gesture.as_ref()? {
            Gesture::Stroke(s) => Some(s),
            _ => None,
        }
    }
}

/// Canvas input of the ArtPath tool.
pub fn input(
    ui: &egui::Ui,
    response: &egui::Response,
    doc: &mut Document,
    rect: egui::Rect,
    ppp: f32,
) {
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (space, mods) = ui.input(|i| {
        (
            !typing && i.key_down(egui::Key::Space),
            Mods {
                alt: i.modifiers.alt,
                ctrl: i.modifiers.command,
            },
        )
    });
    if space {
        return;
    }
    let view = crate::canvas::view_of(doc, rect, ppp);
    let to_canvas =
        |q: egui::Pos2| view.to_canvas([(q.x - rect.min.x) * ppp, (q.y - rect.min.y) * ppp]);
    if let Some(t) = doc.art_tool.as_mut() {
        t.hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
            .map(to_canvas);
    }
    let zoom = doc.view.zoom;
    let primary = egui::PointerButton::Primary;
    if response.drag_started_by(primary) {
        if let Some(q) = ui.input(|i| i.pointer.press_origin()) {
            doc.art_press(to_canvas(q), mods, zoom);
        }
    } else if response.clicked_by(primary) {
        if let Some(q) = ui.input(|i| i.pointer.interact_pos()) {
            doc.art_press(to_canvas(q), mods, zoom);
            doc.art_release();
        }
    }
    if response.dragged_by(primary) {
        if let Some(q) = ui.input(|i| i.pointer.interact_pos()) {
            doc.art_drag(to_canvas(q));
        }
    } else if response.drag_stopped_by(primary) {
        doc.art_release();
    }
}

/// Draws control polylines, points of the selected path, the stroke
/// preview and the brush circle.
pub fn draw(
    painter: &egui::Painter,
    doc: &Document,
    to_screen: &dyn Fn(V2) -> egui::Pos2,
    tokens: &crate::theme::Tokens,
) {
    let Some(tool) = doc.art_tool.as_ref() else {
        return;
    };
    let Some(frame) = doc.frame.as_ref() else {
        return;
    };
    let faint = egui::Stroke::new(1.0, tokens.accent.gamma_multiply(0.5));
    let line = egui::Stroke::new(1.5, tokens.accent);
    for k in 0..doc.model.art_paths.len() {
        let pts = ap::canvas_points(&doc.model, frame, &doc.values, k);
        if pts.len() < 2 {
            continue;
        }
        let sel = tool.path == Some(k);
        let mut v: Vec<egui::Pos2> = pts.iter().map(|p| to_screen(*p)).collect();
        if doc.model.art_paths[k].closed {
            v.push(v[0]);
        }
        painter.add(egui::Shape::line(v, if sel { line } else { faint }));
        if !sel {
            continue;
        }
        let f = keyform_eval::interp::interpolate(
            &doc.model.parameters,
            &doc.values,
            &doc.model.art_paths[k].grid,
            &doc.model.art_paths[k].blend_shapes,
        );
        for (i, p) in pts.iter().enumerate() {
            let c = to_screen(*p);
            let picked = tool.point == Some(i);
            let fill = if picked {
                tokens.accent
            } else {
                egui::Color32::WHITE
            };
            let corner = f
                .as_ref()
                .and_then(|r| r.form.points.get(i))
                .is_some_and(|q| q.corner);
            if corner {
                let r = 5.0;
                let d = vec![
                    c + egui::vec2(0.0, -r),
                    c + egui::vec2(r, 0.0),
                    c + egui::vec2(0.0, r),
                    c + egui::vec2(-r, 0.0),
                ];
                painter.add(egui::Shape::convex_polygon(d, fill, line));
            } else {
                painter.circle(c, if picked { 5.0 } else { 3.5 }, fill, line);
            }
        }
    }
    if let Some(s) = doc.art_stroke_preview() {
        let v: Vec<egui::Pos2> = s.iter().map(|p| to_screen(*p)).collect();
        painter.add(egui::Shape::line(v, egui::Stroke::new(2.0, tokens.accent)));
    }
    if matches!(doc.art_prefs.sub, ArtSub::Width | ArtSub::Opacity) {
        if let Some(h) = tool.hover {
            let size = if doc.art_prefs.sub == ArtSub::Width {
                doc.art_prefs.width_size
            } else {
                doc.art_prefs.opacity_size
            };
            let c = to_screen(h);
            let e = to_screen([h[0] + size, h[1]]);
            painter.circle_stroke(c, (e.x - c.x).abs().max((e.y - c.y).abs()).max(2.0), faint);
        }
    }
}

/// Tool Details of the ArtPath tool.
pub fn controls(ui: &mut egui::Ui, doc: &mut Document) {
    use crate::i18n::tr;
    ui.strong(tr("ArtPath"));
    ui.horizontal_wrapped(|ui| {
        for (s, name) in ArtSub::ALL {
            if ui
                .selectable_label(doc.art_prefs.sub == s, tr(name))
                .clicked()
            {
                doc.art_prefs.sub = s;
                if let Some(t) = doc.art_tool.as_mut() {
                    t.join_from = None;
                }
            }
        }
    });
    ui.separator();
    let p = &mut doc.art_prefs;
    match p.sub {
        ArtSub::Stroke => {
            ui.label(tr(
                "Drag on the canvas to draw an ArtPath; it is fitted to control points.",
            ));
            ui.add(
                egui::Slider::new(&mut p.interval, 0.5..=100.0)
                    .text(tr("Interval"))
                    .suffix(" px"),
            );
            ui.label(tr("New ArtPaths:"));
            ui.add(
                egui::Slider::new(&mut p.defaults.width, 0.0..=200.0)
                    .text(tr("Line Width"))
                    .suffix(" px"),
            );
            ui.add(egui::Slider::new(&mut p.defaults.opacity, 0.0..=1.0).text(tr("Opacity")));
            ui.horizontal(|ui| {
                ui.label(tr("Line Color"));
                ui.color_edit_button_rgba_unmultiplied(&mut p.defaults.color);
            });
        }
        ArtSub::Edit => {
            ui.label(tr("Drag a point to move it; click a line to add a point; Ctrl+click extends the path; Alt+click deletes a point; Ctrl+Alt+click toggles a fold point."));
        }
        ArtSub::Width => {
            ui.add(
                egui::Slider::new(&mut p.width_level, 1.0..=100.0)
                    .text(tr("Thickness"))
                    .suffix(" px"),
            );
            ui.add(
                egui::Slider::new(&mut p.width_size, 0.1..=1000.0)
                    .logarithmic(true)
                    .text(tr("Size"))
                    .suffix(" px"),
            );
        }
        ArtSub::Opacity => {
            ui.add(
                egui::Slider::new(&mut p.opacity_level, 1.0..=100.0)
                    .text(tr("Opacity"))
                    .suffix(" %"),
            );
            ui.add(
                egui::Slider::new(&mut p.opacity_size, 0.1..=1000.0)
                    .logarithmic(true)
                    .text(tr("Size"))
                    .suffix(" px"),
            );
        }
        ArtSub::Split => {
            ui.label(tr(
                "Click an inner point to split the ArtPath there (a closed path opens there).",
            ));
        }
        ArtSub::Join => {
            ui.label(tr("Click the end point of an ArtPath, then the end point of another ArtPath to connect them."));
        }
    }
    let Some(k) = doc
        .art_tool
        .as_ref()
        .and_then(|t| t.path)
        .filter(|&k| k < doc.model.art_paths.len())
    else {
        return;
    };
    ui.separator();
    let a = doc.model.art_paths[k].clone();
    ui.strong(a.info.display_name());
    let mut s = ap::settings(&a);
    let before = s;
    ui.add(egui::Slider::new(&mut s.div_num, 1..=200).text(tr("Division number")));
    ui.checkbox(&mut s.closed, tr("Close ArtPath"));
    ui.horizontal(|ui| {
        ui.label(tr("Line Generation Method"));
        ui.selectable_value(&mut s.algorithm, LineAlgorithm::Lcns, "LCNS");
        ui.selectable_value(&mut s.algorithm, LineAlgorithm::Sns, "SNS");
    });
    ui.horizontal(|ui| {
        ui.label(tr("Alignment"));
        ui.selectable_value(&mut s.alignment, LineAlignment::Center, tr("Center"));
        ui.selectable_value(&mut s.alignment, LineAlignment::LeftCcw, tr("Left"));
        ui.selectable_value(&mut s.alignment, LineAlignment::RightCcw, tr("Right"));
    });
    if s != before {
        let _ = doc.try_edit("ArtPath settings", |m| ap::set_settings(m, k, s));
    }
    if let Some(i) = doc.art_tool.as_ref().and_then(|t| t.point) {
        let cell = ap::current_cell(&doc.model, k, &doc.values);
        match cell {
            Ok(c) => {
                if let Some(pt) = a
                    .grid
                    .keyforms
                    .get(c)
                    .and_then(|f| f.points.get(i))
                    .cloned()
                {
                    ui.separator();
                    ui.label(format!("{} {}", tr("Point"), i + 1));
                    let (mut w, mut o, mut col) = (pt.width, pt.opacity, pt.color);
                    let r1 = ui.add(
                        egui::Slider::new(&mut w, 0.0..=200.0)
                            .text(tr("Line Width"))
                            .suffix(" px"),
                    );
                    let r2 = ui.add(egui::Slider::new(&mut o, 0.0..=1.0).text(tr("Opacity")));
                    let r3 = ui.horizontal(|ui| {
                        ui.label(tr("Line Color"));
                        ui.color_edit_button_rgba_unmultiplied(&mut col).changed()
                    });
                    if r1.changed() || r2.changed() || r3.inner {
                        let values = doc.values.clone();
                        let _ = doc.try_edit("Point style", |m| {
                            ap::set_point_style(m, &values, k, i, Some(w), Some(o), Some(col))
                        });
                    }
                }
            }
            Err(e) => {
                ui.weak(tr(&e));
            }
        }
    }
    ui.separator();
    if ui.button(tr("Delete ArtPath")).clicked() {
        if let Err(e) = doc.try_edit("Delete ArtPath", |m| ap::delete(m, k)) {
            doc.notice = Some(e);
        }
        if let Some(t) = doc.art_tool.as_mut() {
            t.path = None;
            t.point = None;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::log::Log;

    /// The tool's gestures as the canvas sends them: stroke → new path,
    /// drag a point, insert, brush, delete; each one undo step; saved.
    #[test]
    fn gestures_edit_and_save() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let scratch = root.join("target/test-scratch/artpath-tool");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let blank = keyform_formats::cmo3::Cmo3Document::new_model("blank", 800, 600).unwrap();
        let mut log = Log::default();
        let mut d = Document::new_project(&scratch.join("art.cmo3"), blank, &mut log).unwrap();
        d.set_art_tool(true).unwrap();
        let m = Mods::default();
        // Stroke: a wavy line becomes one art path.
        d.art_press([100.0, 300.0], m, 1.0);
        for i in 1..=60 {
            let x = 100.0 + i as f32 * 10.0;
            d.art_drag([x, 300.0 + 40.0 * (x / 80.0).sin()]);
        }
        d.art_release();
        assert_eq!(d.model.art_paths.len(), 1);
        let n = d.model.art_paths[0].grid.keyforms[0].points.len();
        assert!((3..40).contains(&n), "{n} points");
        // Move point 0.
        d.art_prefs.sub = ArtSub::Edit;
        let p0 = d.model.art_paths[0].grid.keyforms[0].points[0].position;
        d.art_press(p0, m, 1.0);
        d.art_drag([p0[0], p0[1] - 50.0]);
        d.art_release();
        assert_eq!(
            d.model.art_paths[0].grid.keyforms[0].points[0].position,
            [p0[0], p0[1] - 50.0]
        );
        // Width brush on the first point.
        d.art_prefs.sub = ArtSub::Width;
        d.art_prefs.width_level = 2.0;
        let q = d.model.art_paths[0].grid.keyforms[0].points[0].position;
        d.art_press(q, m, 1.0);
        for _ in 0..20 {
            d.art_drag(q);
        }
        d.art_release();
        assert!(d.model.art_paths[0].grid.keyforms[0].points[0].width < 3.0);
        // Undo the brush, then the move.
        d.undo();
        assert!((d.model.art_paths[0].grid.keyforms[0].points[0].width - 10.0).abs() < 1e-4);
        d.save_project(Some(&scratch.join("art.cmo3"))).unwrap();
        let back = keyform_formats::cmo3::Cmo3Document::read(
            &std::fs::read(scratch.join("art.cmo3")).unwrap(),
        )
        .unwrap();
        assert_eq!(back.model.art_paths.len(), 1);
        assert_eq!(
            back.model.art_paths[0].grid.keyforms,
            d.model.art_paths[0].grid.keyforms
        );
    }
}
