//! Control Point edit tool [P] (deform paths) and the Temporary path
//! handler. spec: 35-editing/deform-paths.md §4–6, 50-ui/canvas-and-tools.md
//! §3 ("Temporary transform handlers"), §4

use eframe::egui;
use keyform_edit::deform_path::{self as dp, Curve, PathDrag, V2};
use keyform_edit::StepKind;
use keyform_eval::{Evaluator, Frame};
use keyform_model::{ControlPoint, DeformPath, Model};

use crate::document::{Document, Mode, Selection};

/// Pick range of a control point: 12 screen px × edit-range scale 2.
const POINT_RANGE: f32 = 24.0;
/// Pick range of a curve, in screen px.
const CURVE_RANGE: f32 = 12.0;

/// Sub-tools of the Control Point edit group, in the reference's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathSub {
    Select,
    AddPoint,
    Join,
    DeleteEdge,
    DeletePoint,
    Fold,
}

impl PathSub {
    pub const ALL: [(PathSub, &'static str); 6] = [
        (PathSub::Select, "Select"),
        (PathSub::AddPoint, "Add point"),
        (PathSub::Join, "Join Paths"),
        (PathSub::DeleteEdge, "Delete edge"),
        (PathSub::DeletePoint, "Delete point"),
        (PathSub::Fold, "Change to Fold line"),
    ];
}

/// Tool Details settings, kept while the tool is off.
#[derive(Clone, Copy, Debug)]
pub struct PathPrefs {
    pub sub: PathSub,
    /// "add line automatically": a new point joins the previous one.
    pub auto_line: bool,
    pub max_bind: i32,
}

impl Default for PathPrefs {
    fn default() -> Self {
        Self {
            sub: PathSub::AddPoint,
            auto_line: true,
            max_bind: 3,
        }
    }
}

/// A control point or curve of a mesh's path: (mesh, path, index).
pub type PathItem = (usize, usize, usize);

struct ActiveDrag {
    drag: PathDrag,
    base: Model,
    delta: V2,
}

/// The Control Point edit tool, when active.
#[derive(Default)]
pub struct PathTool {
    /// Selected control points.
    pub points: Vec<PathItem>,
    /// Selected curves.
    pub curves: Vec<PathItem>,
    drag: Option<ActiveDrag>,
}

/// Modifier keys of a click.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

enum Hit {
    Point(PathItem),
    Curve(PathItem, f32, V2),
}

impl Document {
    /// Turns the Control Point edit tool on or off.
    pub fn set_path_tool(&mut self, on: bool) -> Result<(), String> {
        if on && (!self.editable() || self.mode != Mode::Modeling || self.mesh_edit.is_some()) {
            return Err("Deform paths are edited in the Modeling workspace of a project.".into());
        }
        if on {
            let _ = self.set_glue_tool(None);
            self.soft_select = None;
            self.lasso = false;
            self.temp_path = None;
            if self.path_tool.is_none() {
                self.path_tool = Some(PathTool::default());
            }
        } else {
            self.path_drag_end();
            self.path_tool = None;
        }
        Ok(())
    }

    /// The state at the current parameter values.
    fn path_frame(&self) -> Frame {
        Evaluator::new(&self.model).evaluate(&self.values)
    }

    /// Selected art meshes, in selection order.
    fn selected_meshes(&self) -> Vec<usize> {
        self.selection
            .objects
            .iter()
            .filter_map(|o| match o {
                Selection::ArtMesh(a) => Some(*a),
                _ => None,
            })
            .collect()
    }

    /// (mesh, path) of the current edit level's paths on selected meshes.
    pub fn level_paths(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for a in self.selected_meshes() {
            if let Some(m) = self.model.art_meshes.get(a) {
                if let Some(pi) = m.paths.iter().position(|p| p.edit_level == self.edit_level) {
                    out.push((a, pi));
                }
            }
        }
        out
    }

    fn path_hit(&self, frame: &Frame, at: V2, zoom: f32) -> Option<Hit> {
        let (pr, cr) = (POINT_RANGE / zoom, CURVE_RANGE / zoom);
        let mut best: Option<(f32, PathItem)> = None;
        for (mi, pi) in self.level_paths() {
            for (k, q) in dp::point_positions(&self.model, frame, mi, pi)
                .iter()
                .enumerate()
            {
                let d = (q[0] - at[0]).hypot(q[1] - at[1]);
                if d <= pr && best.is_none_or(|b| d < b.0) {
                    best = Some((d, (mi, pi, k)));
                }
            }
        }
        if let Some((_, it)) = best {
            return Some(Hit::Point(it));
        }
        let mut best: Option<(f32, Hit)> = None;
        for (mi, pi) in self.level_paths() {
            let pts = dp::point_positions(&self.model, frame, mi, pi);
            let p = &self.model.art_meshes[mi].paths[pi];
            for (c, cv) in dp::curves(p, &pts).iter().enumerate() {
                if cv.segments() == 0 {
                    continue;
                }
                let (t, d, pos) = cv.nearest(at);
                if d <= cr && best.as_ref().is_none_or(|b| d < b.0) {
                    best = Some((d, Hit::Curve((mi, pi, c), t, pos)));
                }
            }
        }
        best.map(|b| b.1)
    }

    /// A click of the tool at canvas `at` (`zoom`: screen px per canvas
    /// px). spec: deform-paths.md §5
    pub fn path_click(&mut self, at: V2, mods: Mods, zoom: f32) {
        let Some(sub) = self.path_tool.as_ref().map(|_| self.path_prefs.sub) else {
            return;
        };
        if self.selected_meshes().is_empty() {
            self.notice = Some("Please select an ArtMesh.".into());
            return;
        }
        let frame = self.path_frame();
        let hit = self.path_hit(&frame, at, zoom);
        // Ctrl+Alt on a point: toggle it as a fold point.
        if mods.ctrl && mods.alt {
            if let Some(Hit::Point(it)) = hit {
                self.path_toggle_fold(&frame, &[it]);
            }
            return;
        }
        let sub = if mods.alt { PathSub::DeletePoint } else { sub };
        match sub {
            PathSub::Select => {
                let tool = self.path_tool.as_mut().expect("tool");
                match hit {
                    Some(Hit::Point(it)) => {
                        if !mods.shift {
                            tool.points.clear();
                            tool.curves.clear();
                        }
                        if !tool.points.contains(&it) {
                            tool.points.push(it);
                        }
                    }
                    Some(Hit::Curve(it, _, _)) => {
                        if !mods.shift {
                            tool.points.clear();
                            tool.curves.clear();
                        }
                        if !tool.curves.contains(&it) {
                            tool.curves.push(it);
                        }
                    }
                    None if !mods.shift => {
                        tool.points.clear();
                        tool.curves.clear();
                    }
                    None => {}
                }
            }
            PathSub::DeletePoint => {
                if let Some(Hit::Point(it)) = hit {
                    self.path_delete_points(&frame, &[it]);
                }
            }
            PathSub::DeleteEdge => {
                if let Some(Hit::Curve((mi, pi, c), t, _)) = hit {
                    let values = self.values.clone();
                    self.edit("Delete edge", |m| {
                        let a = std::sync::Arc::make_mut(&mut m.art_meshes[mi]);
                        dp::delete_edge(
                            &mut a.paths[pi],
                            c,
                            t.floor() as usize,
                            keyform_model::new_guid,
                        );
                        dp::rebind(m, &frame, &values, mi, pi);
                    });
                    self.path_clear_selection();
                }
            }
            PathSub::Fold => {
                if let Some(Hit::Point(it)) = hit {
                    self.path_toggle_fold(&frame, &[it]);
                }
            }
            PathSub::AddPoint | PathSub::Join => self.path_add_click(&frame, at, hit, sub, mods),
        }
    }

    fn path_clear_selection(&mut self) {
        if let Some(t) = self.path_tool.as_mut() {
            t.points.clear();
            t.curves.clear();
        }
    }

    fn path_select(&mut self, it: PathItem, keep: bool) {
        if let Some(t) = self.path_tool.as_mut() {
            if !keep {
                t.points.clear();
                t.curves.clear();
            }
            if !t.points.contains(&it) {
                t.points.push(it);
            }
        }
    }

    /// Add point / Join Paths click. spec: deform-paths.md §5 "Clicking
    /// with Add point"
    fn path_add_click(
        &mut self,
        frame: &Frame,
        at: V2,
        hit: Option<Hit>,
        sub: PathSub,
        mods: Mods,
    ) {
        let prev = self
            .path_tool
            .as_ref()
            .and_then(|t| (t.points.len() == 1).then(|| t.points[0]));
        let paths = self.level_paths();
        if paths.len() > 1 {
            self.notice = Some(
                "More than one deform path among the selected ArtMeshes is not supported.".into(),
            );
            return;
        }
        let join_with = |doc: &mut Document, a: PathItem, b: PathItem| {
            if a.0 != b.0 || a.1 != b.1 {
                return;
            }
            let (mi, pi) = (a.0, a.1);
            if !dp::can_join(&doc.model.art_meshes[mi].paths[pi], a.2, b.2) {
                return;
            }
            let values = doc.values.clone();
            doc.edit("Join Paths", |m| {
                let x = std::sync::Arc::make_mut(&mut m.art_meshes[mi]);
                dp::join(&mut x.paths[pi], a.2, b.2, keyform_model::new_guid);
                dp::rebind(m, frame, &values, mi, pi);
            });
        };
        let auto = self.path_prefs.auto_line || sub == PathSub::Join;
        if let Some(Hit::Point(it)) = hit {
            if let Some(p) = prev.filter(|p| *p != it && auto) {
                join_with(self, p, it);
            }
            self.path_select(it, mods.shift);
            return;
        }
        if sub == PathSub::Join {
            return;
        }
        // A new point: on the path of the selected meshes, or a new path on
        // the last selected mesh.
        let meshes = self.selected_meshes();
        let (mi, existing) = match paths.first() {
            Some(&(mi, pi)) => (mi, Some(pi)),
            None => (*meshes.last().expect("a mesh"), None),
        };
        let subs: Vec<String> = meshes
            .iter()
            .filter(|&&k| k != mi)
            .filter_map(|&k| self.model.art_meshes.get(k).map(|a| a.info.guid.clone()))
            .collect();
        let level = self.edit_level;
        let max_bind = self.path_prefs.max_bind;
        let values = self.values.clone();
        let on_curve = match hit {
            Some(Hit::Curve((cm, cp, c), t, pos)) if cm == mi && Some(cp) == existing => {
                Some((c, t, pos))
            }
            _ => None,
        };
        let corner = mods.ctrl;
        let mut made = None;
        self.edit("Add control point", |m| {
            let a = std::sync::Arc::make_mut(&mut m.art_meshes[mi]);
            let pi = match existing {
                Some(pi) => pi,
                None => {
                    let mut p = DeformPath::new(keyform_model::new_guid(), level);
                    p.max_bind = max_bind;
                    a.paths.push(p);
                    a.paths.len() - 1
                }
            };
            for g in &subs {
                if !a.paths[pi].sub_meshes.contains(g) {
                    a.paths[pi].sub_meshes.push(g.clone());
                }
            }
            let q = on_curve.map_or(at, |c| c.2);
            let pos = frame
                .meshes
                .get(mi)
                .map(|s| s.positions.clone())
                .unwrap_or_default();
            let anchor = dp::anchor(&pos, &a.indices, q);
            let p = &mut a.paths[pi];
            p.points.push(ControlPoint {
                guid: keyform_model::new_guid(),
                curve: None,
                corner,
                anchor,
                pos_default: q,
                recovery: q,
                total_effect: 0.0,
            });
            let k = p.points.len() - 1;
            if let Some((c, t, _)) = on_curve {
                dp::insert_on_curve(p, c, t, k);
            } else if let Some(pv) = prev.filter(|pv| pv.0 == mi && Some(pv.1) == existing && auto)
            {
                dp::join(p, pv.2, k, keyform_model::new_guid);
            }
            dp::rebind(m, frame, &values, mi, pi);
            made = Some((mi, pi, k));
        });
        if let Some(it) = made {
            self.path_select(it, false);
        }
    }

    /// Deletes control points (and a path left without points).
    pub fn path_delete_points(&mut self, frame: &Frame, items: &[PathItem]) {
        let values = self.values.clone();
        let mut items = items.to_vec();
        items.sort_unstable_by(|a, b| b.cmp(a));
        items.dedup();
        self.edit("Delete control point", |m| {
            let mut emptied = Vec::new();
            for &(mi, pi, k) in &items {
                let a = std::sync::Arc::make_mut(&mut m.art_meshes[mi]);
                let Some(p) = a.paths.get_mut(pi) else {
                    continue;
                };
                if dp::delete_point(p, k) {
                    emptied.push((mi, pi));
                }
            }
            let mut touched: Vec<(usize, usize)> = items.iter().map(|i| (i.0, i.1)).collect();
            touched.dedup();
            for &(mi, pi) in &touched {
                if !emptied.contains(&(mi, pi)) {
                    dp::rebind(m, frame, &values, mi, pi);
                }
            }
            emptied.sort_unstable_by(|a, b| b.cmp(a));
            emptied.dedup();
            for (mi, pi) in emptied {
                std::sync::Arc::make_mut(&mut m.art_meshes[mi])
                    .paths
                    .remove(pi);
            }
        });
        self.path_clear_selection();
    }

    fn path_toggle_fold(&mut self, frame: &Frame, items: &[PathItem]) {
        let values = self.values.clone();
        let items = items.to_vec();
        self.edit("Change to Fold line", |m| {
            let mut touched = Vec::new();
            for &(mi, pi, k) in &items {
                let a = std::sync::Arc::make_mut(&mut m.art_meshes[mi]);
                if let Some(c) = a.paths.get_mut(pi).and_then(|p| p.points.get_mut(k)) {
                    c.corner = !c.corner;
                    touched.push((mi, pi));
                }
            }
            touched.dedup();
            for (mi, pi) in touched {
                dp::rebind(m, frame, &values, mi, pi);
            }
        });
    }

    /// Starts dragging control points at canvas `at`; returns whether a
    /// drag started. spec: deform-paths.md §4
    pub fn path_drag_begin(&mut self, at: V2, zoom: f32, shift: bool) -> bool {
        if self.path_tool.is_none()
            || !matches!(self.path_prefs.sub, PathSub::Select | PathSub::AddPoint)
        {
            return false;
        }
        let frame = self.path_frame();
        let Some(Hit::Point(it)) = self.path_hit(&frame, at, zoom) else {
            return false;
        };
        let selected = self
            .path_tool
            .as_ref()
            .is_some_and(|t| t.points.contains(&it));
        if !selected {
            self.path_select(it, shift);
        }
        let ks: Vec<usize> = self
            .path_tool
            .as_ref()
            .map(|t| {
                t.points
                    .iter()
                    .filter(|p| p.0 == it.0 && p.1 == it.1)
                    .map(|p| p.2)
                    .collect()
            })
            .unwrap_or_default();
        match PathDrag::begin(&self.model, &frame, &self.values, it.0, it.1, ks) {
            Ok(drag) => {
                let base = self.model.clone();
                if let Some(t) = self.path_tool.as_mut() {
                    t.drag = Some(ActiveDrag {
                        drag,
                        base,
                        delta: [0.0, 0.0],
                    });
                }
                true
            }
            Err(e) => {
                self.notice = Some(e.to_string());
                false
            }
        }
    }

    /// Shows the drag at `total` canvas px from its start.
    pub fn path_drag_update(&mut self, total: V2) {
        let Some(d) = self.path_tool.as_mut().and_then(|t| t.drag.as_mut()) else {
            return;
        };
        d.delta = total;
        let mut m = d.base.clone();
        match d.drag.apply(&mut m, total) {
            Ok(()) => {
                self.model = m;
                self.model_changed();
            }
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    pub fn path_dragging(&self) -> bool {
        self.path_tool.as_ref().is_some_and(|t| t.drag.is_some())
    }

    /// Ends a drag: locked points are re-anchored; one step "Edit by
    /// DeformPath".
    pub fn path_drag_end(&mut self) {
        let Some(d) = self.path_tool.as_mut().and_then(|t| t.drag.take()) else {
            return;
        };
        if self.model.same_data(&d.base) {
            self.model = d.base;
            self.model_changed();
            return;
        }
        let frame = self.path_frame();
        let mut m = self.model.clone();
        d.drag.finish(&mut m, &frame, d.delta);
        self.model = m;
        self.history.commit(
            "Edit by DeformPath",
            StepKind::Real,
            self.edit_state(),
            false,
        );
        self.model_changed();
    }

    /// Tool Details buttons. spec: deform-paths.md §5
    pub fn path_delete_selected_points(&mut self) {
        let items = self
            .path_tool
            .as_ref()
            .map(|t| t.points.clone())
            .unwrap_or_default();
        if !items.is_empty() {
            let frame = self.path_frame();
            self.path_delete_points(&frame, &items);
        }
    }

    pub fn path_fold_selected(&mut self) {
        let items = self
            .path_tool
            .as_ref()
            .map(|t| t.points.clone())
            .unwrap_or_default();
        if !items.is_empty() {
            let frame = self.path_frame();
            self.path_toggle_fold(&frame, &items);
        }
    }

    /// Delete path: the selected curves.
    pub fn path_delete_selected_curves(&mut self) {
        let mut items = self
            .path_tool
            .as_ref()
            .map(|t| t.curves.clone())
            .unwrap_or_default();
        if items.is_empty() {
            return;
        }
        items.sort_unstable_by(|a, b| b.cmp(a));
        items.dedup();
        let frame = self.path_frame();
        let values = self.values.clone();
        self.edit("Delete path", |m| {
            let mut touched = Vec::new();
            for &(mi, pi, c) in &items {
                let a = std::sync::Arc::make_mut(&mut m.art_meshes[mi]);
                if let Some(p) = a.paths.get_mut(pi) {
                    dp::remove_curve(p, c);
                    touched.push((mi, pi));
                }
            }
            touched.dedup();
            for (mi, pi) in touched {
                dp::rebind(m, &frame, &values, mi, pi);
            }
        });
        self.path_clear_selection();
    }

    /// Delete deform paths for current edit level (all selected meshes).
    pub fn path_delete_level(&mut self) {
        let paths = self.level_paths();
        if paths.is_empty() {
            return;
        }
        self.edit("Delete deform paths for current edit level", |m| {
            for &(mi, pi) in paths.iter().rev() {
                std::sync::Arc::make_mut(&mut m.art_meshes[mi])
                    .paths
                    .remove(pi);
            }
        });
        self.path_clear_selection();
    }

    /// Sets maxBindCount of the current level's paths and rebinds.
    pub fn path_set_max_bind(&mut self, n: i32) {
        self.path_prefs.max_bind = n;
        let paths = self.level_paths();
        if paths.is_empty() {
            return;
        }
        let frame = self.path_frame();
        let values = self.values.clone();
        self.edit("maxBindCount", |m| {
            for &(mi, pi) in &paths {
                std::sync::Arc::make_mut(&mut m.art_meshes[mi]).paths[pi].max_bind = n;
                dp::rebind(m, &frame, &values, mi, pi);
            }
        });
    }

    /// Sets the width or hardness of the selected curves.
    pub fn path_set_curves(&mut self, width: Option<f32>, hardness: Option<f32>) {
        let items = self
            .path_tool
            .as_ref()
            .map(|t| t.curves.clone())
            .unwrap_or_default();
        self.edit("Deform Path", |m| {
            for &(mi, pi, c) in &items {
                let a = std::sync::Arc::make_mut(&mut m.art_meshes[mi]);
                if let Some(cv) = a.paths.get_mut(pi).and_then(|p| p.curves.get_mut(c)) {
                    if let Some(w) = width {
                        cv.width = w.clamp(1.0, 10000.0);
                    }
                    if let Some(h) = hardness {
                        cv.hardness = h.clamp(0.0, 100.0);
                    }
                }
            }
        });
    }
}

/// Canvas input of the tool.
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
                ctrl: i.modifiers.command,
                alt: i.modifiers.alt,
                shift: i.modifiers.shift,
            },
        )
    });
    if space {
        return;
    }
    let view = crate::canvas::view_of(doc, rect, ppp);
    let to_canvas =
        |q: egui::Pos2| view.to_canvas([(q.x - rect.min.x) * ppp, (q.y - rect.min.y) * ppp]);
    let zoom = doc.view.zoom;
    let primary = egui::PointerButton::Primary;
    if response.drag_started_by(primary) && !mods.alt {
        if let Some(q) = ui.input(|i| i.pointer.press_origin()) {
            if !doc.path_drag_begin(to_canvas(q), zoom, mods.shift) {
                doc.path_click(to_canvas(q), mods, zoom);
            }
        }
    } else if response.clicked_by(primary) || (response.drag_started_by(primary) && mods.alt) {
        if let Some(q) = ui.input(|i| i.pointer.interact_pos()) {
            doc.path_click(to_canvas(q), mods, zoom);
        }
    }
    if doc.path_dragging() {
        if response.dragged_by(primary) {
            if let (Some(a), Some(b)) =
                ui.input(|i| (i.pointer.press_origin(), i.pointer.interact_pos()))
            {
                let (p, q) = (to_canvas(a), to_canvas(b));
                doc.path_drag_update([q[0] - p[0], q[1] - p[1]]);
            }
        } else {
            doc.path_drag_end();
        }
    }
}

fn sample_curve(c: &Curve, to_screen: &dyn Fn(V2) -> egui::Pos2) -> Vec<egui::Pos2> {
    let n = c.segments() * 16;
    (0..=n)
        .map(|k| to_screen(c.point(k as f32 / 16.0)))
        .collect()
}

/// Draws the current level's paths of the selected meshes.
pub fn draw(
    painter: &egui::Painter,
    doc: &Document,
    to_screen: &dyn Fn(V2) -> egui::Pos2,
    tokens: &crate::theme::Tokens,
) {
    let Some(frame) = doc.frame.as_ref() else {
        return;
    };
    let Some(tool) = doc.path_tool.as_ref() else {
        return;
    };
    let line = egui::Stroke::new(1.5, tokens.accent);
    let thick = egui::Stroke::new(3.0, tokens.accent);
    for (mi, pi) in doc.level_paths() {
        let pts = dp::point_positions(&doc.model, frame, mi, pi);
        let p = &doc.model.art_meshes[mi].paths[pi];
        for (c, cv) in dp::curves(p, &pts).iter().enumerate() {
            let s = if tool.curves.contains(&(mi, pi, c)) {
                thick
            } else {
                line
            };
            painter.add(egui::Shape::line(sample_curve(cv, to_screen), s));
        }
        for (k, q) in pts.iter().enumerate() {
            let c = to_screen(*q);
            let sel = tool.points.contains(&(mi, pi, k));
            let r = if sel { 5.0 } else { 3.5 };
            let fill = if sel {
                tokens.accent
            } else {
                egui::Color32::WHITE
            };
            let cp = &p.points[k];
            if cp.corner {
                let d = [
                    c + egui::vec2(0.0, -r - 1.0),
                    c + egui::vec2(r + 1.0, 0.0),
                    c + egui::vec2(0.0, r + 1.0),
                    c + egui::vec2(-r - 1.0, 0.0),
                ];
                painter.add(egui::Shape::convex_polygon(d.to_vec(), fill, line));
            } else if cp.curve.is_none() {
                painter.rect(
                    egui::Rect::from_center_size(c, egui::vec2(2.0 * r, 2.0 * r)),
                    0.0,
                    fill,
                    line,
                    egui::StrokeKind::Middle,
                );
            } else {
                painter.circle(c, r, fill, line);
            }
        }
    }
}

/// Tool Details of the Control Point edit tool.
pub fn controls(ui: &mut egui::Ui, doc: &mut Document) {
    ui.strong("Control Point edit");
    ui.horizontal_wrapped(|ui| {
        for (s, name) in PathSub::ALL {
            ui.selectable_value(&mut doc.path_prefs.sub, s, name);
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button(crate::i18n::tr("Add control point")).clicked() {
            doc.path_prefs.sub = PathSub::AddPoint;
        }
        if ui.button(crate::i18n::tr("Delete control point")).clicked() {
            doc.path_delete_selected_points();
        }
        if ui.button(crate::i18n::tr("Delete path")).clicked() {
            doc.path_delete_selected_curves();
        }
        if ui.button(crate::i18n::tr("Fold line")).clicked() {
            doc.path_fold_selected();
        }
    });
    if ui
        .button(crate::i18n::tr(
            "Delete deform paths for current edit level",
        ))
        .clicked()
    {
        doc.path_delete_level();
    }
    ui.checkbox(
        &mut doc.path_prefs.auto_line,
        crate::i18n::tr("add line automatically"),
    );
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("maxBindCount"));
        let mut n = doc.path_prefs.max_bind;
        if ui.add(egui::DragValue::new(&mut n).range(1..=10)).changed() {
            doc.path_set_max_bind(n);
        }
    });
    // Inspector fields of the selected curves (inspector-fields.md).
    let sel: Vec<PathItem> = doc
        .path_tool
        .as_ref()
        .map(|t| t.curves.clone())
        .unwrap_or_default();
    if let Some(&(mi, pi, c)) = sel.first() {
        if let Some(cv) = doc
            .model
            .art_meshes
            .get(mi)
            .and_then(|a| a.paths.get(pi))
            .and_then(|p| p.curves.get(c))
        {
            let (mut w, mut h) = (cv.width, cv.hardness);
            egui::Grid::new("path-curve").num_columns(2).show(ui, |ui| {
                ui.label(crate::i18n::tr("Deform Path width"));
                let wc = ui.add(egui::DragValue::new(&mut w).range(1.0..=10000.0)).changed();
                ui.end_row();
                ui.label(crate::i18n::tr("Deform Path hardness"));
                let hc = ui
                    .add(egui::Slider::new(&mut h, 0.0..=100.0).step_by(1.0).suffix(" %"))
                    .on_hover_text(crate::i18n::tr("Path range reduction: 0 reduces the range gradually, 100 lets the path greatly affect the surroundings"))
                    .changed();
                ui.end_row();
                if wc || hc {
                    doc.path_set_curves(wc.then_some(w), hc.then_some(h));
                }
            });
        }
    }
    ui.weak(crate::i18n::tr("Click to add points to the selected ArtMesh's path; drag points to deform. Alt deletes a point, Ctrl+Alt toggles a fold point, Shift keeps the selection."));
}

// ------------------------------------------------------- temporary path §6

/// One bound point: (object, point index, its canvas position at bind,
/// segment, arc-length fraction, signed angle, length).
#[derive(Clone, Debug)]
struct TempBind {
    object: Selection,
    point: usize,
    segment: usize,
    fraction: f32,
    angle: f32,
    length: f32,
}

/// The Temporary path handler: an unsaved line bending the selected points.
pub struct TempPath {
    pub points: Vec<V2>,
    pub selected: Option<usize>,
    binds: Vec<TempBind>,
    /// The model when the handler started (Cancel restores it).
    base: Model,
    /// The model the current binding was made on.
    bound: Model,
    /// Canvas positions of the bound points at bind.
    at_bind: Vec<V2>,
    drag: Option<(usize, V2)>,
}

/// Samples per segment of the temporary path's line.
const TEMP_DIV: usize = 60;

/// The line through `pts` (open, no corners), sampled: per segment its
/// samples (TEMP_DIV + 1) and cumulative arc lengths.
fn temp_line(pts: &[V2]) -> Vec<(Vec<V2>, Vec<f32>)> {
    if pts.len() < 2 {
        return Vec::new();
    }
    let c = Curve::new(pts.to_vec(), &vec![false; pts.len()], true);
    (0..c.segments())
        .map(|s| {
            let q: Vec<V2> = (0..=TEMP_DIV)
                .map(|k| c.point(s as f32 + k as f32 / TEMP_DIV as f32))
                .collect();
            let mut acc = vec![0.0f32];
            for w in q.windows(2) {
                let l = acc.last().copied().unwrap_or(0.0);
                acc.push(l + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
            }
            (q, acc)
        })
        .collect()
}

/// Position and unit tangent at arc-length `fraction` of a sampled segment.
fn temp_at(seg: &(Vec<V2>, Vec<f32>), fraction: f32) -> (V2, V2) {
    let (q, acc) = seg;
    let total = acc.last().copied().unwrap_or(0.0);
    let target = fraction.clamp(0.0, 1.0) * total;
    let i = acc
        .iter()
        .position(|&a| a >= target)
        .unwrap_or(acc.len() - 1)
        .max(1);
    let (a, b) = (q[i - 1], q[i]);
    let span = (acc[i] - acc[i - 1]).max(1e-9);
    let f = ((target - acc[i - 1]) / span).clamp(0.0, 1.0);
    let p = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]).max(1e-9);
    (p, [d[0] / l, d[1] / l])
}

fn rotate(v: V2, ang: f32) -> V2 {
    let (s, c) = ang.sin_cos();
    [v[0] * c - v[1] * s, v[0] * s + v[1] * c]
}

impl Document {
    /// Starts or cancels the Temporary path handler on the selected points.
    pub fn set_temp_path(&mut self, on: bool) -> Result<(), String> {
        if !on {
            self.temp_path_cancel();
            return Ok(());
        }
        if !self.editable() || self.mode != Mode::Modeling || self.mesh_edit.is_some() {
            return Err("Temporary path works in the Modeling workspace of a project.".into());
        }
        if self.selection.points.is_empty() {
            return Err("Please select the points to deform.".into());
        }
        self.path_tool = None;
        self.temp_path = Some(TempPath {
            points: Vec::new(),
            selected: None,
            binds: Vec::new(),
            base: self.model.clone(),
            bound: self.model.clone(),
            at_bind: Vec::new(),
            drag: None,
        });
        Ok(())
    }

    /// Binds the selected points to the line through the current points.
    /// spec: deform-paths.md §6 "Bind"
    fn temp_bind(&mut self) {
        let Some(t) = self.temp_path.as_ref() else {
            return;
        };
        let line = temp_line(&t.points);
        let pts: Vec<(Selection, usize)> = self.selection.points.clone();
        let mut binds = Vec::new();
        let mut at_bind = Vec::new();
        for (o, i) in pts {
            let Some(q) = self.object_points(o).get(i).copied() else {
                continue;
            };
            let mut best: Option<(f32, usize, usize)> = None;
            for (s, (samples, _)) in line.iter().enumerate() {
                for (k, p) in samples.iter().enumerate() {
                    let d = (p[0] - q[0]).hypot(p[1] - q[1]);
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, s, k));
                    }
                }
            }
            let Some((len, s, k)) = best else { continue };
            let (_, acc) = &line[s];
            let fraction = acc[k] / acc.last().copied().unwrap_or(1.0).max(1e-9);
            let (sp, tan) = temp_at(&line[s], fraction);
            let u = [sp[0] - q[0], sp[1] - q[1]];
            let angle = (tan[0] * u[1] - tan[1] * u[0]).atan2(tan[0] * u[0] + tan[1] * u[1]);
            binds.push(TempBind {
                object: o,
                point: i,
                segment: s,
                fraction,
                angle,
                length: len,
            });
            at_bind.push(q);
        }
        let model = self.model.clone();
        if let Some(t) = self.temp_path.as_mut() {
            t.binds = binds;
            t.at_bind = at_bind;
            t.bound = model;
        }
    }

    /// Applies the line to the bound points. spec: deform-paths.md §6 "Apply"
    fn temp_apply(&mut self) {
        let Some(t) = self.temp_path.as_ref() else {
            return;
        };
        let line = temp_line(&t.points);
        let mut m = t.bound.clone();
        for (b, q0) in t.binds.iter().zip(&t.at_bind) {
            let Some(seg) = line.get(b.segment) else {
                continue;
            };
            let (p, tan) = temp_at(seg, b.fraction);
            let r = rotate([-tan[0], -tan[1]], b.angle);
            let q = [p[0] + r[0] * b.length, p[1] + r[1] * b.length];
            let delta = [q[0] - q0[0], q[1] - q0[1]];
            let Ok(cell) = keyform_edit::current_cell(&m, b.object, &self.values) else {
                continue;
            };
            let parent = keyform_edit::parent_deformer(&m, b.object)
                .and_then(|d| self.frame.as_ref().and_then(|f| f.deformers.get(d)))
                .and_then(|s| s.map.clone());
            let _ = keyform_edit::forms::translate(
                &mut m,
                b.object,
                cell,
                parent.as_ref(),
                delta,
                Some(&[b.point]),
            );
        }
        self.model = m;
        self.model_changed();
    }

    /// A click or drag start of the handler at canvas `at`.
    pub fn temp_press(&mut self, at: V2, zoom: f32, delete: bool) {
        let Some(t) = self.temp_path.as_mut() else {
            return;
        };
        let r = POINT_RANGE / zoom;
        if let Some(k) = t
            .points
            .iter()
            .position(|p| (p[0] - at[0]).hypot(p[1] - at[1]) <= r)
        {
            if delete {
                t.points.remove(k);
                t.selected = None;
                self.temp_rebind();
            } else {
                t.selected = Some(k);
                t.drag = Some((k, t.points[k]));
            }
            return;
        }
        if delete {
            return;
        }
        // On the line: insert in its segment; else append.
        let line = temp_line(&t.points);
        let mut ins = None;
        for (s, (samples, _)) in line.iter().enumerate() {
            if samples
                .iter()
                .any(|p| (p[0] - at[0]).hypot(p[1] - at[1]) <= CURVE_RANGE / zoom)
            {
                ins = Some(s + 1);
                break;
            }
        }
        let k = match ins {
            Some(i) => {
                t.points.insert(i, at);
                i
            }
            None => {
                t.points.push(at);
                t.points.len() - 1
            }
        };
        t.selected = Some(k);
        t.drag = Some((k, at));
        self.temp_rebind();
    }

    /// Point set changed: keep the current result and bind anew.
    fn temp_rebind(&mut self) {
        self.temp_bind();
    }

    /// Drags the pressed point by `total` from its press position; Shift
    /// constrains the move to one axis.
    pub fn temp_drag(&mut self, total: V2, shift: bool) {
        let Some(t) = self.temp_path.as_mut() else {
            return;
        };
        let Some((k, start)) = t.drag else { return };
        let d = if shift {
            if total[0].abs() >= total[1].abs() {
                [total[0], 0.0]
            } else {
                [0.0, total[1]]
            }
        } else {
            total
        };
        t.points[k] = [start[0] + d[0], start[1] + d[1]];
        self.temp_apply();
    }

    pub fn temp_release(&mut self) {
        if let Some(t) = self.temp_path.as_mut() {
            t.drag = None;
        }
    }

    /// Confirm: one undo step with the resulting keyform positions.
    pub fn temp_path_confirm(&mut self) {
        let Some(t) = self.temp_path.take() else {
            return;
        };
        if self.model.same_data(&t.base) {
            return;
        }
        self.history
            .commit("Temporary path", StepKind::Real, self.edit_state(), false);
        self.model_changed();
    }

    pub fn temp_path_cancel(&mut self) {
        if let Some(t) = self.temp_path.take() {
            self.model = t.base;
            self.model_changed();
        }
    }
}

/// Canvas input of the Temporary path handler.
pub fn temp_input(
    ui: &egui::Ui,
    response: &egui::Response,
    doc: &mut Document,
    rect: egui::Rect,
    ppp: f32,
) {
    let (alt, shift, enter, esc) = ui.input(|i| {
        (
            i.modifiers.alt,
            i.modifiers.shift,
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if enter {
        doc.temp_path_confirm();
        return;
    }
    if esc {
        doc.temp_path_cancel();
        return;
    }
    let view = crate::canvas::view_of(doc, rect, ppp);
    let to_canvas =
        |q: egui::Pos2| view.to_canvas([(q.x - rect.min.x) * ppp, (q.y - rect.min.y) * ppp]);
    let zoom = doc.view.zoom;
    let primary = egui::PointerButton::Primary;
    if response.drag_started_by(primary) || response.clicked_by(primary) {
        if let Some(q) = ui.input(|i| i.pointer.press_origin()) {
            doc.temp_press(to_canvas(q), zoom, alt);
        }
    }
    if response.dragged_by(primary) {
        if let (Some(a), Some(b)) =
            ui.input(|i| (i.pointer.press_origin(), i.pointer.interact_pos()))
        {
            let (p, q) = (to_canvas(a), to_canvas(b));
            doc.temp_drag([q[0] - p[0], q[1] - p[1]], shift);
        }
    } else {
        doc.temp_release();
    }
}

pub fn temp_draw(
    painter: &egui::Painter,
    doc: &Document,
    to_screen: &dyn Fn(V2) -> egui::Pos2,
    tokens: &crate::theme::Tokens,
) {
    let Some(t) = doc.temp_path.as_ref() else {
        return;
    };
    let stroke = egui::Stroke::new(1.5, tokens.accent);
    for (q, _) in temp_line(&t.points) {
        painter.add(egui::Shape::line(
            q.iter().map(|p| to_screen(*p)).collect(),
            stroke,
        ));
    }
    for (k, p) in t.points.iter().enumerate() {
        let fill = if t.selected == Some(k) {
            tokens.accent
        } else {
            egui::Color32::WHITE
        };
        painter.circle(to_screen(*p), 4.0, fill, stroke);
    }
}

pub fn temp_controls(ui: &mut egui::Ui, doc: &mut Document) {
    ui.strong("Temporary path");
    ui.label(crate::i18n::tr("Click to add points to the line; drag them to bend the selected points. Alt+click deletes a point; Shift constrains a drag."));
    ui.horizontal(|ui| {
        if ui.button(crate::i18n::tr("Confirm (Enter)")).clicked() {
            doc.temp_path_confirm();
        }
        if ui.button(crate::i18n::tr("Cancel (Esc)")).clicked() {
            doc.temp_path_cancel();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Log;
    use std::path::Path;

    fn open_hiyori() -> Option<(Document, Log)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("hiyori_pro_t11.cmo3");
        if !path.exists() {
            return None;
        }
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        doc.tick(0.0, &Default::default(), &mut log);
        Some((doc, log))
    }

    /// A pathless mesh on a key with points spread far enough apart.
    fn pick_mesh(doc: &Document) -> (usize, Vec<V2>) {
        (0..doc.model.art_meshes.len())
            .find_map(|a| {
                let m = &doc.model.art_meshes[a];
                if !m.paths.is_empty()
                    || m.vertex_count() < 20
                    || keyform_edit::current_cell(&doc.model, Selection::ArtMesh(a), &doc.values)
                        .is_err()
                {
                    return None;
                }
                let pts = doc.object_points(Selection::ArtMesh(a));
                let (x0, x1) = pts
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
                (x1 - x0 > 200.0).then_some((a, pts))
            })
            .unwrap()
    }

    /// Clicks add points joined into a curve (one step each); dragging the
    /// curve deforms the mesh as one "Edit by DeformPath" step; Alt+click
    /// deletes; the path saves and reloads. spec: deform-paths.md §4–5
    #[test]
    fn add_drag_delete_save() {
        let Some((mut doc, mut log)) = open_hiyori() else {
            return;
        };
        let (a, pts) = pick_mesh(&doc);
        doc.select(Some(Selection::ArtMesh(a)));
        doc.set_path_tool(true).unwrap();
        let left = *pts.iter().min_by(|p, q| p[0].total_cmp(&q[0])).unwrap();
        let right = *pts.iter().max_by(|p, q| p[0].total_cmp(&q[0])).unwrap();
        let mid = [(left[0] + right[0]) / 2.0, (left[1] + right[1]) / 2.0];
        let none = Mods::default();
        for q in [left, mid, right] {
            doc.path_click(q, none, 1.0);
            assert_eq!(doc.history.undo_name(), Some("Add control point"));
        }
        let p = &doc.model.art_meshes[a].paths[0];
        assert_eq!(p.edit_level, doc.edit_level);
        assert_eq!(p.points.len(), 3);
        assert_eq!(p.curves.len(), 1);
        assert_eq!(p.curves[0].points, vec![0, 1, 2]);
        assert_eq!(p.targets.len(), doc.model.art_meshes[a].vertex_count());
        // Drag the middle point down by 30 px.
        doc.tick(0.0, &Default::default(), &mut log);
        let before = doc.object_points(Selection::ArtMesh(a));
        assert!(doc.path_drag_begin(mid, 1.0, false));
        doc.path_drag_update([0.0, 30.0]);
        doc.path_drag_end();
        assert_eq!(doc.history.undo_name(), Some("Edit by DeformPath"));
        doc.tick(0.0, &Default::default(), &mut log);
        let after = doc.object_points(Selection::ArtMesh(a));
        let moved = before
            .iter()
            .zip(&after)
            .filter(|(x, y)| (y[1] - x[1]).abs() > 1.0)
            .count();
        assert!(moved > 0, "the drag moved vertices");
        // Save and reload.
        let dir = std::env::temp_dir().join(format!("kf-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("p.cmo3");
        doc.save_project(Some(&out)).unwrap();
        if let Some(keep) = std::env::var_os("KEYFORM_PATH_OUT") {
            std::fs::copy(&out, keep).unwrap();
        }
        let back = Document::open(&out, &mut log).unwrap();
        assert_eq!(
            back.model.art_meshes[a].paths,
            doc.model.art_meshes[a].paths
        );
        // Alt+click deletes points; the last one removes the path.
        doc.tick(0.0, &Default::default(), &mut log);
        let frame = doc.path_frame();
        let at: Vec<V2> = dp::point_positions(&doc.model, &frame, a, 0);
        let alt = Mods {
            alt: true,
            ..Default::default()
        };
        for q in at {
            doc.path_click(q, alt, 1.0);
        }
        assert!(doc.model.art_meshes[a].paths.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The temporary path bends the selected points; Confirm is one step,
    /// Cancel restores. spec: deform-paths.md §6
    #[test]
    fn temporary_path() {
        let Some((mut doc, mut log)) = open_hiyori() else {
            return;
        };
        let (a, pts) = pick_mesh(&doc);
        doc.select(Some(Selection::ArtMesh(a)));
        let mut sel = doc.selection.clone();
        sel.set_points((0..pts.len()).map(|i| (Selection::ArtMesh(a), i)).collect());
        doc.set_selection(sel);
        doc.set_temp_path(true).unwrap();
        let (x0, x1) = pts
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
        let y = pts.iter().map(|p| p[1]).sum::<f32>() / pts.len() as f32;
        doc.temp_press([x0 - 50.0, y], 1.0, false);
        doc.temp_release();
        doc.temp_press([x1 + 50.0, y], 1.0, false);
        doc.temp_release();
        // Moving the second point straight down rotates the line about the
        // first: the right side drops more than the left.
        let before = doc.object_points(Selection::ArtMesh(a));
        doc.temp_press([x1 + 50.0, y], 1.0, false);
        doc.temp_drag([0.0, 100.0], false);
        doc.temp_release();
        doc.tick(0.0, &Default::default(), &mut log);
        let after = doc.object_points(Selection::ArtMesh(a));
        let dy = |p: V2| {
            let i = before.iter().position(|q| *q == p).unwrap();
            after[i][1] - before[i][1]
        };
        let l = *before.iter().min_by(|p, q| p[0].total_cmp(&q[0])).unwrap();
        let r = *before.iter().max_by(|p, q| p[0].total_cmp(&q[0])).unwrap();
        assert!(dy(r) > dy(l) + 10.0, "left {} right {}", dy(l), dy(r));
        doc.temp_path_confirm();
        assert_eq!(doc.history.undo_name(), Some("Temporary path"));
        // Cancel restores.
        let m = doc.model.clone();
        doc.set_temp_path(true).unwrap();
        doc.temp_press([x0, y], 1.0, false);
        doc.temp_press([x1, y], 1.0, false);
        doc.temp_drag([0.0, 40.0], false);
        doc.temp_path_cancel();
        assert!(doc.model.same_data(&m));
    }
}
