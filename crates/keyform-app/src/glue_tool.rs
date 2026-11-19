//! Glue tool group in the Modeling view: weight brushes A, B, A:B, B:A and
//! Re-merge Glue Vertices, painting the weights of the selected glue.
//! spec: 30-algorithms/glue.md "Weight brushes", 50-ui/canvas-and-tools.md
//! "Glue"

use eframe::egui;
use keyform_edit::glue_brush::{self, GlueBrush};
use keyform_edit::StepKind;
use keyform_model::Model;

use crate::document::{Document, Mode, Selection};
use crate::mesh_edit::BrushShape;

/// Brush settings shared by the glue tools (defaults: size 100 px, level
/// 100 %, hardness 30 %).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GluePrefs {
    /// Diameter in canvas pixels.
    pub size: f32,
    pub level: f32,
    pub hardness: f32,
    /// Degrees.
    pub angle: f32,
    pub shape: BrushShape,
    /// Opacity of the weight display (0…1).
    pub display_opacity: f32,
}

impl Default for GluePrefs {
    fn default() -> Self {
        Self {
            size: 100.0,
            level: 1.0,
            hardness: 0.3,
            angle: 0.0,
            shape: BrushShape::Circle,
            display_opacity: 1.0,
        }
    }
}

impl GluePrefs {
    /// Distance in the brush frame of canvas offset `v` (rotated by −angle,
    /// flipped when the view is mirrored), as for the mesh-edit brush.
    pub fn distance(&self, v: [f32; 2], flip: bool) -> f32 {
        let (s, c) = (-self.angle.to_radians()).sin_cos();
        let (mut x, y) = (v[0] * c - v[1] * s, v[0] * s + v[1] * c);
        if flip {
            x = -x;
        }
        match self.shape {
            BrushShape::Circle => x.hypot(y),
            BrushShape::Square => x.abs().max(y.abs()),
            BrushShape::Line => y.abs(),
        }
    }

    /// Brush amount of a vertex at canvas offset `v` for one drag event.
    pub fn amount(&self, v: [f32; 2], flip: bool, pressure: f32) -> Option<f32> {
        let f = glue_brush::falloff(self.distance(v, flip), self.size / 2.0, self.hardness)?;
        Some(glue_brush::amount(f, self.level, pressure))
    }
}

/// An active glue tool and its stroke in progress.
pub struct GlueTool {
    pub tool: GlueBrush,
    /// The model before the stroke (one undo step per stroke).
    stroke: Option<Model>,
}

impl Document {
    /// Selects a glue tool (or turns the tool off with `None`).
    pub fn set_glue_tool(&mut self, tool: Option<GlueBrush>) -> Result<(), String> {
        self.glue_brush_end();
        let Some(tool) = tool else {
            self.glue_tool = None;
            return Ok(());
        };
        if !self.editable() || self.mode != Mode::Modeling || self.mesh_edit.is_some() {
            return Err("The glue tools work in the Modeling workspace of a project.".into());
        }
        self.glue_tool = Some(GlueTool { tool, stroke: None });
        self.soft_select = None;
        self.path_tool = None;
        self.art_tool = None;
        Ok(())
    }

    /// The glue the brushes paint: the selected glue (empty when one of
    /// them is locked). spec: glue.md "Weight brushes"
    pub fn glue_targets(&self) -> Vec<usize> {
        glue_brush::target_glues(&self.model, &self.selected_glue()).unwrap_or_default()
    }

    fn selected_glue(&self) -> Vec<usize> {
        self.selection
            .objects
            .iter()
            .filter_map(|o| match *o {
                Selection::Glue(g) => Some(g),
                _ => None,
            })
            .collect()
    }

    /// Starts a stroke. Fails when there is no glue to paint.
    pub fn glue_brush_begin(&mut self) -> Result<(), String> {
        let selected = self.selected_glue();
        if selected.is_empty() {
            return Err(glue_brush::NO_SELECTED_GLUE.into());
        }
        if glue_brush::target_glues(&self.model, &selected).is_none() {
            // A locked glue: the brush does nothing.
            return Err("The selected glue is locked.".into());
        }
        let base = self.model.clone();
        if let Some(t) = self.glue_tool.as_mut() {
            t.stroke = Some(base);
        }
        Ok(())
    }

    pub fn glue_stroke_active(&self) -> bool {
        self.glue_tool.as_ref().is_some_and(|t| t.stroke.is_some())
    }

    /// One drag event at canvas position `at`, on the displayed vertices.
    pub fn glue_brush_step(&mut self, at: [f32; 2], shift: bool, flip: bool, pressure: f32) {
        let Some(tool) = self
            .glue_tool
            .as_ref()
            .filter(|t| t.stroke.is_some())
            .map(|t| t.tool)
        else {
            return;
        };
        let targets = self.glue_targets();
        let prefs = self.glue_prefs;
        let Some(frame) = self.frame.as_ref() else {
            return;
        };
        // Measured on the deformed positions before glue.
        let amount_at = |mesh: usize, v: usize| {
            let p = frame.positions_before_glue(mesh)?.get(2 * v..2 * v + 2)?;
            prefs.amount([p[0] - at[0], p[1] - at[1]], flip, pressure)
        };
        let mut m = self.model.clone();
        if glue_brush::apply(&mut m, &targets, tool, shift, amount_at) {
            self.model = m;
            self.model_changed();
        }
    }

    /// Ends the stroke: one undo step if a weight changed.
    pub fn glue_brush_end(&mut self) {
        let Some(base) = self.glue_tool.as_mut().and_then(|t| t.stroke.take()) else {
            return;
        };
        if self.model.same_data(&base) {
            return;
        }
        let name = self
            .glue_tool
            .as_ref()
            .map_or("Glue weight", |t| t.tool.label());
        self.history
            .commit(name, StepKind::Real, self.edit_state(), false);
    }
}

/// Canvas input of a glue tool: a drag paints (one drag event per
/// pointer movement), Shift + drag paints the reverse. Space/R navigate.
pub fn input(
    ui: &egui::Ui,
    response: &egui::Response,
    doc: &mut Document,
    rect: egui::Rect,
    ppp: f32,
) {
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (space, rkey, shift, moved) = ui.input(|i| {
        (
            !typing && i.key_down(egui::Key::Space),
            !typing && i.key_down(egui::Key::R),
            i.modifiers.shift,
            i.pointer.delta() != egui::Vec2::ZERO,
        )
    });
    if space || rkey {
        return;
    }
    let zoom = doc.view.zoom;
    let p = &mut doc.glue_prefs;
    if crate::soft_select::brush_adjust(
        ui,
        response,
        zoom,
        (&mut p.size, &mut p.angle, &mut p.hardness),
    ) {
        return;
    }
    let view = crate::canvas::view_of(doc, rect, ppp);
    let to_canvas =
        |p: egui::Pos2| view.to_canvas([(p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp]);
    let primary = egui::PointerButton::Primary;
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let flip = doc.view.mirror;
    if response.drag_started_by(primary) || response.clicked_by(primary) {
        if let Err(e) = doc.glue_brush_begin() {
            doc.notice = Some(e);
            return;
        }
        if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
            doc.glue_brush_step(to_canvas(p), shift, flip, 1.0);
        }
    } else if response.dragged_by(primary) && moved {
        if let Some(p) = pointer {
            doc.glue_brush_step(to_canvas(p), shift, flip, 1.0);
        }
    }
    if doc.glue_stroke_active() && !response.dragged_by(primary) {
        doc.glue_brush_end();
    }
    if let Some(t) = doc.glue_tool.as_ref() {
        if shift && doc.glue_stroke_active() {
            doc.notice = Some(t.tool.shift_status().into());
        }
    }
}

/// The weight display of the painted glue (each bound vertex coloured by
/// its UI weight, blue 0 → red 1) and the brush outline at the cursor.
pub fn draw(
    ctx: &egui::Context,
    painter: &egui::Painter,
    doc: &Document,
    rect: egui::Rect,
    to_screen: &dyn Fn([f32; 2]) -> egui::Pos2,
    tokens: &crate::theme::Tokens,
) {
    let prefs = &doc.glue_prefs;
    if let Some(f) = doc.frame.as_ref() {
        let alpha = prefs.display_opacity.clamp(0.0, 1.0);
        let colour = |ui_w: f32| {
            let t = ui_w.clamp(0.0, 1.0);
            egui::Color32::from_rgb((255.0 * t) as u8, 40, (255.0 * (1.0 - t)) as u8)
                .gamma_multiply(alpha)
        };
        let point = |mesh: usize, v: usize| -> Option<egui::Pos2> {
            let p = f.meshes.get(mesh)?.positions.get(2 * v..2 * v + 2)?;
            Some(to_screen([p[0], p[1]]))
        };
        for g in doc.glue_targets() {
            let glue = &doc.model.glues[g];
            let (Some(ma), Some(mb)) = (glue.mesh_a, glue.mesh_b) else {
                continue;
            };
            for (&(ia, ib), w) in glue.pairs.iter().zip(&glue.weights) {
                let (Some(ia), Some(ib)) = (ia, ib) else {
                    continue;
                };
                let (Some(pa), Some(pb)) = (point(ma, ia), point(mb, ib)) else {
                    continue;
                };
                painter.line_segment(
                    [pa, pb],
                    egui::Stroke::new(1.0, egui::Color32::WHITE.gamma_multiply(0.5 * alpha)),
                );
                painter.circle_filled(pa, 3.5, colour(1.0 - w[0]));
                painter.circle_filled(pb, 3.5, colour(1.0 - w[1]));
            }
        }
    }
    if let Some(c) = ctx
        .input(|i| i.pointer.hover_pos())
        .filter(|p| rect.contains(*p))
    {
        let r = (to_screen([prefs.size / 2.0, 0.0]) - to_screen([0.0, 0.0])).length();
        crate::canvas::brush_outline(
            painter,
            c,
            r,
            prefs.hardness,
            prefs.angle,
            prefs.shape,
            tokens.accent,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_frame() {
        let p = GluePrefs {
            size: 20.0,
            hardness: 0.0,
            ..GluePrefs::default()
        };
        assert!((p.amount([5.0, 0.0], false, 1.0).unwrap() - 0.1).abs() < 1e-6);
        assert_eq!(p.amount([11.0, 0.0], false, 1.0), None);
        let sq = GluePrefs {
            shape: BrushShape::Square,
            ..p
        };
        assert!(sq.amount([9.0, 9.0], false, 1.0).is_some());
        let line = GluePrefs {
            shape: BrushShape::Line,
            angle: 90.0,
            ..p
        };
        // Rotated 90°: the band runs vertically.
        assert!(line.amount([0.0, 500.0], false, 1.0).is_some());
        assert_eq!(line.amount([15.0, 0.0], false, 1.0), None);
    }

    /// A stroke on a sample's glue changes its weights in one undo step,
    /// and the project saves and reloads with them.
    #[test]
    fn stroke_undo_and_save() {
        use crate::log::Log;
        use std::path::Path;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dirs = std::env::var("KEYFORM_SAMPLES").unwrap_or_else(|_| root.display().to_string());
        let mut log = Log::default();
        let Some(src) = std::env::split_paths(&dirs)
            .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "cmo3"))
            .find(|p| {
                std::fs::read(p)
                    .ok()
                    .and_then(|d| keyform_formats::cmo3::Cmo3Document::read(&d).ok())
                    .is_some_and(|d| !d.model.glues.is_empty())
            })
        else {
            return;
        };
        let scratch = root.join("target/test-scratch/glue-brush");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("model.cmo3");
        std::fs::copy(&src, &path).unwrap();
        let mut doc = Document::open(&path, &mut log).unwrap();
        doc.tick(0.0, &Default::default(), &mut log);
        let g = (0..doc.model.glues.len())
            .find(|&g| {
                doc.model.glues[g]
                    .pairs
                    .iter()
                    .any(|p| p.0.is_some() && p.1.is_some())
            })
            .unwrap();
        doc.set_glue_tool(Some(GlueBrush::AB)).unwrap();
        doc.select(None);
        assert_eq!(
            doc.glue_brush_begin(),
            Err(glue_brush::NO_SELECTED_GLUE.into()),
            "selected art meshes don't count"
        );
        doc.select(Some(Selection::Glue(g)));
        let before = doc.model.glues[g].weights.clone();
        let glue = doc.model.glues[g].clone();
        let (k, ia) = glue
            .pairs
            .iter()
            .enumerate()
            .find_map(|(k, p)| Some((k, p.0?)))
            .unwrap();
        let ma = glue.mesh_a.unwrap();
        let pos = doc
            .frame
            .as_ref()
            .unwrap()
            .positions_before_glue(ma)
            .unwrap()
            .to_vec();
        let at = [pos[2 * ia], pos[2 * ia + 1]];
        doc.glue_brush_begin().unwrap();
        doc.glue_brush_step(at, false, false, 1.0);
        doc.glue_brush_step(at, false, false, 1.0);
        doc.glue_brush_end();
        let after = doc.model.glues[g].weights.clone();
        let w0 = before[k];
        assert!(
            (after[k][0] - (w0[0] - 0.4).max(0.0)).abs() < 1e-5,
            "{w0:?} → {:?}",
            after[k]
        );
        // wB moves only by B's own vertex amount (0 when outside).
        assert!(after[k][1] >= w0[1] && after[k][1] <= (w0[1] + 0.4).min(1.0) + 1e-5);
        assert_eq!(doc.history.undo_name(), Some("Weight brush A:B"));
        doc.save_project(None).unwrap();
        let back = Document::open(&path, &mut log).unwrap();
        assert_eq!(back.model.glues[g].weights, after);
        doc.undo();
        assert_eq!(doc.model.glues[g].weights, before);
        // Unbind the pair's vertices, then undo.
        let ib = glue.pairs[k].1.unwrap();
        let mb = glue.mesh_b.unwrap();
        doc.set_glue_tool(None).unwrap();
        doc.set_selection(crate::document::SelectionSet {
            objects: vec![Selection::ArtMesh(ma), Selection::ArtMesh(mb)],
            points: vec![(Selection::ArtMesh(ma), ia), (Selection::ArtMesh(mb), ib)],
            point_weights: Vec::new(),
        });
        let n = glue.pairs.len();
        doc.unglue_selected_vertices().unwrap();
        assert_eq!(doc.model.glues[g].pairs.len(), n - 1);
        assert!(doc.unglue_selected_vertices().is_err());
        // The selection is restored as its own step.
        while doc.undo().is_some_and(|name| name != "Unbind Glue") {}
        assert_eq!(doc.model.glues[g].pairs.len(), n);
    }
}

#[cfg(test)]
mod mesh_edit_glue_tests {
    use crate::document::{Document, Selection, SelectionSet};
    use crate::log::Log;
    use std::path::Path;

    /// Bind in mesh edit with two meshes (glue.md "Bind / unbind"): one
    /// vertex selected on each, Bind merges them at their midpoint; Finish
    /// writes both meshes and a new glue with the pair (0.5 / 0.5) in one
    /// undo step; saves and reloads; Unbind afterwards removes the pair.
    #[test]
    fn bind_in_mesh_edit() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Ok(data) = std::fs::read(root.join("blitty.psd")) else {
            return;
        };
        let psd = keyform_formats::psd::Psd::parse(&data).unwrap();
        let (mut doc, _) = keyform_formats::psd_import::new_model_from_psd(
            "blitty.psd",
            &psd,
            &Default::default(),
        )
        .unwrap();
        let scratch = root.join("target/test-scratch/mesh-glue");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("model.cmo3");
        doc.save_to(&path).unwrap();
        let mut log = Log::default();
        let mut d = Document::open(&path, &mut log).unwrap();
        d.tick(0.0, &Default::default(), &mut log);
        let (a, b) = (0usize, 1usize);
        d.set_selection(SelectionSet {
            objects: vec![Selection::ArtMesh(b), Selection::ArtMesh(a)],
            points: Vec::new(),
            point_weights: Vec::new(),
        });
        d.start_mesh_edit().unwrap();
        let model = d.model.clone();
        let s = d.mesh_edit.as_mut().unwrap();
        assert_eq!(s.mesh, a, "the primary selection is edited");
        assert_eq!(s.partner.as_ref().unwrap().mesh, b);
        let pa = s.graph.point(0);
        let pb = s.partner.as_ref().unwrap().graph.point(0);
        s.set_selected(vec![0]);
        s.partner.as_mut().unwrap().set_selected(vec![0]);
        assert_eq!(s.bind(false, 1e9, &model), Ok(1));
        let mid = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
        assert_eq!(s.graph.point(0), mid);
        assert_eq!(s.partner.as_ref().unwrap().graph.point(0), mid);
        assert_eq!(
            s.bind(false, 1e9, &model),
            Err(keyform_edit::objects::GLUE_SAME_POINTS.into())
        );
        d.finish_mesh_edit().unwrap();
        assert_eq!(d.model.glues.len(), 1);
        let g = &d.model.glues[0];
        assert_eq!(g.pairs.len(), 1);
        assert_eq!(g.weights, vec![[0.5, 0.5]]);
        let base = |m: &keyform_model::Model, i: usize| {
            [m.art_meshes[i].positions[0], m.art_meshes[i].positions[1]]
        };
        assert_eq!(base(&d.model, a), mid);
        assert_eq!(base(&d.model, b), mid);
        d.save_project(None).unwrap();
        let back = Document::open(&path, &mut log).unwrap();
        assert!(back.model.same_data(&d.model));
        // Unbind in a new session.
        d.set_selection(SelectionSet {
            objects: vec![Selection::ArtMesh(b), Selection::ArtMesh(a)],
            points: Vec::new(),
            point_weights: Vec::new(),
        });
        d.start_mesh_edit().unwrap();
        let model = d.model.clone();
        let s = d.mesh_edit.as_mut().unwrap();
        s.set_selected(vec![0]);
        s.partner.as_mut().unwrap().set_selected(vec![0]);
        assert_eq!(s.bind(true, 1e9, &model), Ok(1));
        d.finish_mesh_edit().unwrap();
        assert!(d.model.glues[0].pairs.is_empty());
        // Undo (past the selection step Finish adds) restores the pair.
        while d.undo().is_some_and(|n| n != "Mesh Edit") {}
        assert_eq!(d.model.glues[0].pairs.len(), 1);
    }
}
