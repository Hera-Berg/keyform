//! Mesh edit mode: editing the point/edge graph of one art mesh. The edits
//! stay in a working copy with its own undo stack; finishing re-triangulates
//! the mesh and remaps its keyforms as one undo step.
//! spec: 50-ui/canvas-and-tools.md §5, 50-ui/main-window.md §6,
//! 30-algorithms/mesh-triangulation.md, 70-advanced/auto-mesh.md §4

use std::collections::HashSet;

use keyform_edit::mesh::{triangulate, MeshGraph, Remap};
use keyform_model::{ArtMesh, EDGE_AUTO_TRIANGULATION};

use crate::document::in_polygon;

/// Mesh editor sub-tools (canvas-and-tools.md §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshTool {
    /// Select vertices and move them by dragging.
    Select,
    /// Click (or drag) to add a vertex; on an edge it splits the edge.
    AddPoint,
    /// Deletes vertices and edges under the cursor; Alt deletes edges only.
    Erase,
    /// Drags the vertices under a soft brush.
    Brush,
}

/// Mirror Edit axis: x = `axis` (vertical, mirroring left ↔ right) or
/// y = `axis`. spec: canvas-and-tools.md §5.1 "Mirror Edit"
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mirror {
    pub axis: f32,
    pub vertical: bool,
}

impl Mirror {
    pub fn reflect(&self, p: [f32; 2]) -> [f32; 2] {
        if self.vertical {
            [2.0 * self.axis - p[0], p[1]]
        } else {
            [p[0], 2.0 * self.axis - p[1]]
        }
    }

    /// Distance of `p` from the axis.
    pub fn off_axis(&self, p: [f32; 2]) -> f32 {
        (if self.vertical { p[0] } else { p[1] } - self.axis).abs()
    }
}

/// Mesh-edit brush shapes. spec: canvas-and-tools.md §5.1 "Brush"
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushShape {
    Circle,
    Square,
    Line,
}

/// Mesh editor settings kept between mesh-edit sessions (the mirror axis
/// is initialised to the canvas centre once).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshPrefs {
    pub mirror_on: bool,
    pub mirror: Mirror,
    /// Brush diameter (canvas px), level and hardness (0…1), angle (°).
    pub brush_size: f32,
    pub brush_level: f32,
    pub brush_hardness: f32,
    pub brush_angle: f32,
    pub brush_shape: BrushShape,
    /// Auto connect options (defaults: protect on, reconstruction off).
    pub protect_exterior: bool,
    pub reconstruct: bool,
}

impl MeshPrefs {
    pub fn new(canvas: [f32; 2]) -> Self {
        Self {
            mirror_on: false,
            mirror: Mirror {
                axis: canvas[0] / 2.0,
                vertical: true,
            },
            brush_size: 120.0,
            brush_level: 1.0,
            brush_hardness: 0.3,
            brush_angle: 0.0,
            brush_shape: BrushShape::Circle,
            protect_exterior: true,
            reconstruct: false,
        }
    }
}

/// A point being dragged: start position and the factors applied to the
/// drag's x and y movement (brush weight, mirroring).
#[derive(Clone, Copy, Debug)]
struct DragPoint {
    k: usize,
    start: [f32; 2],
    factor: [f32; 2],
}

pub const RESET_NEEDS_NO_GLUE: &str = "Unable to execute on an ArtMesh with glue set.";
pub const OVERLAPPING_MERGED: &str = "Overlapping vertices are merged.";

type Snapshot = (MeshGraph, Vec<usize>);

pub struct MeshEdit {
    /// Index of the art mesh being edited.
    pub mesh: usize,
    pub graph: MeshGraph,
    start: MeshGraph,
    /// Live triangulation of `graph`.
    pub preview: Vec<u32>,
    /// The graph `preview` was computed for: only edge changes
    /// re-triangulate, moving points keeps the triangles
    /// (mesh-triangulation.md §3a).
    triangulated: MeshGraph,
    /// Crossing constraint edges in the preview.
    pub unresolved: usize,
    /// Texture coordinates of the working points.
    pub uvs: Vec<f32>,
    remap: Remap,
    start_uvs: Vec<f32>,
    pub texture: Option<usize>,
    pub selected: Vec<usize>,
    pub tool: MeshTool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Points being dragged, with their positions at the press.
    drag: Option<Vec<DragPoint>>,
    pub prefs: MeshPrefs,
    /// The model image's rectangle on the canvas (Reset Mesh), and whether
    /// the mesh has glue.
    pub model_rect: Option<[[f32; 2]; 4]>,
    pub glued: bool,
    /// State before a live preview (automatic generation popup).
    preview_base: Option<Snapshot>,
    /// Generator settings to store on the mesh when editing finishes.
    pub generator: Option<keyform_model::MeshGenerateSetting>,
    /// The second art mesh in mesh edit (gluing between the two).
    pub partner: Option<Box<MeshEdit>>,
    /// Glue pairs to add / remove at Finish, as (this mesh's vertex UID,
    /// the partner's vertex UID). spec: glue.md "Bind / unbind"
    pub glue_add: Vec<(i64, i64)>,
    pub glue_remove: Vec<(i64, i64)>,
}

impl MeshEdit {
    pub fn start(i: usize, a: &ArtMesh, prefs: MeshPrefs) -> Result<Self, String> {
        if a.mesh.observed {
            return Err(keyform_edit::mesh::MeshError::Observed.to_string());
        }
        let graph = MeshGraph::of(a);
        let mut s = Self {
            mesh: i,
            start: graph.clone(),
            triangulated: graph.clone(),
            graph,
            preview: a.indices.clone(),
            unresolved: 0,
            uvs: Vec::new(),
            remap: Remap::new(&a.positions, &a.indices),
            start_uvs: a.uvs.clone(),
            texture: a.texture,
            selected: Vec::new(),
            tool: MeshTool::Select,
            undo: Vec::new(),
            redo: Vec::new(),
            drag: None,
            prefs,
            model_rect: None,
            glued: false,
            preview_base: None,
            generator: None,
            partner: None,
            glue_add: Vec::new(),
            glue_remove: Vec::new(),
        };
        s.refresh();
        Ok(s)
    }

    /// Whether the graph differs from the mesh at entry.
    pub fn changed(&self) -> bool {
        self.graph != self.start
            || self.generator.is_some()
            || !self.glue_add.is_empty()
            || !self.glue_remove.is_empty()
            || self.partner.as_ref().is_some_and(|p| p.changed())
    }

    /// Whether this mesh's graph differs from the mesh at entry.
    pub fn graph_changed(&self) -> bool {
        self.graph != self.start
    }

    /// Bind / Unbind Glue Vertices in mesh edit (glue.md "Bind / unbind"):
    /// pairs the selected vertices of the two meshes (`max_dist` = 40
    /// screen pixels in canvas units); binding adds the pairs (0.5 / 0.5
    /// at Finish) and merges each pair at its midpoint; unbinding removes
    /// them. Returns how many pairs.
    pub fn bind(
        &mut self,
        unbind: bool,
        max_dist: f32,
        m: &keyform_model::Model,
    ) -> Result<usize, String> {
        use keyform_edit::objects::{
            glue_between, pair_vertices, GLUE_NOT_ASSOCIATED, GLUE_SAME_POINTS,
            GLUE_SELECT_VERTICES, GLUE_TWO_MESHES,
        };
        let me = self.mesh;
        let Some(p) = self.partner.as_mut() else {
            return Err(GLUE_TWO_MESHES.into());
        };
        let a: Vec<(usize, [f32; 2])> = self
            .selected
            .iter()
            .map(|&k| (k, self.graph.point(k)))
            .collect();
        let b: Vec<(usize, [f32; 2])> = p.selected.iter().map(|&k| (k, p.graph.point(k))).collect();
        if a.is_empty() || b.is_empty() {
            return Err(GLUE_SELECT_VERTICES.into());
        }
        let pairs = pair_vertices(&a, &b, max_dist);
        if pairs.is_empty() {
            return Err(GLUE_SELECT_VERTICES.into());
        }
        // Pairs already in the model, by vertex UID (this mesh, partner).
        let existing: Vec<(i64, i64)> = match glue_between(m, me, p.mesh) {
            Some((g, me_first)) => {
                let (ua, ub) = (
                    &m.art_meshes[me].point_uids,
                    &m.art_meshes[p.mesh].point_uids,
                );
                m.glues[g]
                    .pairs
                    .iter()
                    .filter_map(|&(x, y)| {
                        let (i, j) = if me_first { (x?, y?) } else { (y?, x?) };
                        Some((*ua.get(i)?, *ub.get(j)?))
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        let uid = |g: &MeshGraph, k: usize| g.uids.get(k).copied().unwrap_or(-1);
        if unbind {
            if existing.is_empty() && self.glue_add.is_empty() {
                return Err(GLUE_NOT_ASSOCIATED.into());
            }
            for &(i, j) in &pairs {
                let pair = (uid(&self.graph, i), uid(&p.graph, j));
                if let Some(k) = self.glue_add.iter().position(|x| *x == pair) {
                    self.glue_add.remove(k);
                } else if existing.contains(&pair) && !self.glue_remove.contains(&pair) {
                    self.glue_remove.push(pair);
                }
            }
            return Ok(pairs.len());
        }
        for &(i, j) in &pairs {
            let pair = (uid(&self.graph, i), uid(&p.graph, j));
            let present = (existing.contains(&pair) && !self.glue_remove.contains(&pair))
                || self.glue_add.contains(&pair);
            if present {
                return Err(GLUE_SAME_POINTS.into());
            }
        }
        // Merge each pair at its midpoint (both meshes' editing positions
        // are canvas positions).
        let mids: Vec<(usize, usize, [f32; 2])> = pairs
            .iter()
            .map(|&(i, j)| {
                let (a, b) = (self.graph.point(i), p.graph.point(j));
                (i, j, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0])
            })
            .collect();
        for &(i, j, c) in &mids {
            let pair = (uid(&self.graph, i), uid(&p.graph, j));
            self.glue_remove.retain(|x| *x != pair);
            self.glue_add.push(pair);
            p.change(|g, _| {
                g.points[2 * j] = c[0];
                g.points[2 * j + 1] = c[1];
            });
        }
        self.change(|g, _| {
            for &(i, _, c) in &mids {
                g.points[2 * i] = c[0];
                g.points[2 * i + 1] = c[1];
            }
        });
        Ok(pairs.len())
    }

    /// Re-triangulates the preview after edge changes and recomputes the
    /// UVs.
    fn refresh(&mut self) {
        let g = &self.graph;
        if !g.same_edges(&self.triangulated) {
            let m = &g.mesh;
            match triangulate(&g.points, &m.edges, &m.edge_priority, m.delaunay) {
                Ok(t) => {
                    self.preview = t.indices;
                    self.unresolved = t.unresolved;
                }
                Err(_) => self.preview.clear(),
            }
            self.triangulated = g.clone();
        }
        // UVs follow the positions like keyforms do (texture-atlas.md §4).
        self.uvs = (0..g.len())
            .flat_map(|k| {
                self.remap
                    .apply(self.remap.locate(g.point(k)), &self.start_uvs)
            })
            .collect();
    }

    /// Applies `f` as one step of the mesh editor's own undo stack.
    pub fn change(&mut self, f: impl FnOnce(&mut MeshGraph, &mut Vec<usize>)) {
        let before = (self.graph.clone(), self.selected.clone());
        f(&mut self.graph, &mut self.selected);
        let n = self.graph.len();
        self.selected.retain(|&p| p < n);
        if (&self.graph, &self.selected) != (&before.0, &before.1) {
            self.undo.push(before);
            self.redo.clear();
            self.refresh();
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else {
            return false;
        };
        let now = (
            std::mem::replace(&mut self.graph, s.0),
            std::mem::replace(&mut self.selected, s.1),
        );
        self.redo.push(now);
        self.drag = None;
        self.refresh();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else {
            return false;
        };
        let now = (
            std::mem::replace(&mut self.graph, s.0),
            std::mem::replace(&mut self.selected, s.1),
        );
        self.undo.push(now);
        self.drag = None;
        self.refresh();
        true
    }

    /// The active mirror, if Mirror Edit is on.
    pub fn mirror(&self) -> Option<Mirror> {
        self.prefs.mirror_on.then_some(self.prefs.mirror)
    }

    /// Mirror partners of `points` (within `tolerance`, canvas units of one
    /// screen pixel), not already in it.
    fn partners(&self, points: &[usize], tolerance: f32) -> Vec<usize> {
        let Some(m) = self.mirror() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for &k in points {
            if let Some(j) = self.graph.mirror_partner(k, m.axis, m.vertical, tolerance) {
                if !points.contains(&j) && !out.contains(&j) {
                    out.push(j);
                }
            }
        }
        out
    }

    /// Adds the mirror partners of the selected points to the selection
    /// (mirrored rectangle selection).
    pub fn select_with_partners(&mut self, mut sel: Vec<usize>, tolerance: f32) {
        let extra = self.partners(&sel, tolerance);
        sel.extend(extra);
        self.set_selected(sel);
    }

    /// The point nearest `q` within `radius`.
    pub fn point_at(&self, q: [f32; 2], radius: f32) -> Option<usize> {
        let g = &self.graph;
        (0..g.len())
            .map(|k| (k, dist(g.point(k), q)))
            .filter(|&(_, d)| d <= radius)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    }

    /// The graph edge nearest `q` within `radius`.
    pub fn points_in(&self, quad: &[[f32; 2]]) -> Vec<usize> {
        (0..self.graph.len())
            .filter(|&k| in_polygon(quad, self.graph.point(k)))
            .collect()
    }

    /// Add Point: a new vertex at `q`, splitting the edge under it. The new
    /// point becomes the selection. Returns it.
    pub fn add_point(&mut self, q: [f32; 2], radius: f32) -> usize {
        let mirror = self.mirror().filter(|m| m.off_axis(q) > radius);
        let mut new = 0;
        self.change(|g, sel| {
            let add = |g: &mut MeshGraph, q: [f32; 2]| {
                let edge = edge_near(g, q, radius);
                match edge {
                    Some(e) => {
                        // On the edge itself, so the outline keeps its shape.
                        let [a, b] = g.mesh.edges[e].map(|v| g.point(v as usize));
                        g.split_edge(e, closest_on_segment(q, a, b))
                    }
                    None => g.add_point(q),
                }
            };
            new = add(g, q);
            *sel = vec![new];
            // Mirror Edit adds the mirrored point too.
            if let Some(m) = mirror {
                sel.push(add(g, m.reflect(q)));
            }
        });
        new
    }

    /// Eraser at `q`: points (unless `edges_only`) and edges within
    /// `radius`. Returns whether anything was deleted.
    pub fn erase(&mut self, q: [f32; 2], radius: f32, edges_only: bool) -> bool {
        let g = &self.graph;
        // Mirror Edit erases at the reflected position too.
        let mut at = vec![q];
        if let Some(m) = self.mirror() {
            at.push(m.reflect(q));
        }
        let points: HashSet<usize> = if edges_only {
            HashSet::new()
        } else {
            (0..g.len())
                .filter(|&k| at.iter().any(|&c| dist(g.point(k), c) <= radius))
                .collect()
        };
        let edges: HashSet<usize> = if points.is_empty() {
            at.iter().filter_map(|&c| edge_near(g, c, radius)).collect()
        } else {
            HashSet::new()
        };
        if points.is_empty() && edges.is_empty() {
            return false;
        }
        self.change(|g, sel| {
            if !points.is_empty() {
                g.delete_points(&points);
                sel.clear();
            } else {
                g.delete_edges(&edges);
            }
        });
        true
    }

    /// Deletes the selected points (and their edges), with their mirror
    /// partners when Mirror Edit is on.
    pub fn delete_selected(&mut self, tolerance: f32) {
        let mut which: HashSet<usize> = self.selected.iter().copied().collect();
        which.extend(self.partners(&self.selected, tolerance));
        if which.is_empty() {
            return;
        }
        self.change(|g, sel| {
            g.delete_points(&which);
            sel.clear();
        });
    }

    /// Auto connect (Ctrl+R, Connect, Auto connect) with the Protect Edge
    /// exterior and Mesh reconstruction options. spec: §5.1
    pub fn auto_connect(&mut self) -> Result<(), String> {
        let (protect, reconstruct) = (self.prefs.protect_exterior, self.prefs.reconstruct);
        let mut r = Ok(());
        self.change(|g, sel| {
            r = g.auto_connect(protect, reconstruct);
            if r.is_ok() {
                sel.clear();
            }
        });
        r.map_err(str::to_owned)
    }

    /// Reset Mesh: the model image's rectangle. spec: §5.1
    pub fn reset_mesh(&mut self) -> Result<(), String> {
        if self.glued {
            return Err(RESET_NEEDS_NO_GLUE.into());
        }
        let Some(rect) = self.model_rect else {
            return Err("The ArtMesh has no model image.".into());
        };
        self.drag = None;
        self.change(|g, sel| {
            g.reset_to_rect(rect);
            sel.clear();
        });
        Ok(())
    }

    /// The graph to commit when editing finishes with OK: exactly
    /// coincident vertices merged. Returns it with the clean-up hint.
    pub fn finished_graph(&self) -> (MeshGraph, Option<&'static str>) {
        let mut g = self.graph.clone();
        let n = g.remove_coincident();
        (g, (n > 0).then_some(OVERLAPPING_MERGED))
    }

    /// Starts moving the selected points (one undo step until released).
    /// `tolerance` is one screen pixel in canvas units (mirror partners).
    pub fn drag_begin(&mut self, tolerance: f32) {
        let points = self.selected.iter().map(|&k| (k, 1.0)).collect();
        self.drag_points(points, tolerance);
    }

    /// Brush weight of a point at canvas offset `v` from the brush centre
    /// (`flip` when the view is mirrored). spec: §5.1 "Brush"
    pub fn brush_weight(&self, v: [f32; 2], flip: bool) -> f32 {
        let b = &self.prefs;
        let (s, c) = (-b.brush_angle.to_radians()).sin_cos();
        let (mut x, y) = (v[0] * c - v[1] * s, v[0] * s + v[1] * c);
        if flip {
            x = -x;
        }
        let d = match b.brush_shape {
            BrushShape::Circle => x.hypot(y),
            BrushShape::Square => x.abs().max(y.abs()),
            BrushShape::Line => y.abs(),
        };
        let r = (b.brush_size / 2.0).max(1e-3);
        if d > r {
            return 0.0;
        }
        let t = d / r;
        let hard = b.brush_hardness.clamp(0.0, 1.0);
        let f = if hard >= 1.0 {
            1.0
        } else {
            ((1.0 - t) / (1.0 - hard)).clamp(0.0, 1.0)
        };
        (b.brush_level * f).clamp(0.0, 1.0)
    }

    /// Brush: starts a stroke at `at`; weights are fixed at the press and
    /// each vertex moves by its weight times the drag.
    pub fn brush_begin(&mut self, at: [f32; 2], tolerance: f32, flip: bool) {
        let points = (0..self.graph.len())
            .filter_map(|k| {
                let p = self.graph.point(k);
                let w = self.brush_weight([p[0] - at[0], p[1] - at[1]], flip);
                (w > 0.0).then_some((k, w))
            })
            .collect();
        self.drag_points(points, tolerance);
    }

    fn drag_points(&mut self, points: Vec<(usize, f32)>, tolerance: f32) {
        let mirror = self.mirror();
        let mut drag: Vec<DragPoint> = Vec::new();
        for &(k, w) in &points {
            let mut start = self.graph.point(k);
            let mut factor = [w, w];
            // A vertex on the axis stays on it.
            if let Some(m) = mirror.filter(|m| m.off_axis(start) <= tolerance) {
                if m.vertical {
                    (start[0], factor[0]) = (m.axis, 0.0);
                } else {
                    (start[1], factor[1]) = (m.axis, 0.0);
                }
            }
            drag.push(DragPoint { k, start, factor });
        }
        // Mirror Edit: partners move to the reflected position.
        if let Some(m) = mirror {
            for &(k, w) in &points {
                if m.off_axis(self.graph.point(k)) <= tolerance {
                    continue;
                }
                if let Some(j) = self.graph.mirror_partner(k, m.axis, m.vertical, tolerance) {
                    if drag.iter().all(|d| d.k != j) {
                        drag.push(DragPoint {
                            k: j,
                            start: self.graph.point(j),
                            factor: if m.vertical { [-w, w] } else { [w, -w] },
                        });
                    }
                }
            }
        }
        self.undo.push((self.graph.clone(), self.selected.clone()));
        self.redo.clear();
        self.drag = Some(drag);
    }

    pub fn drag_update(&mut self, delta: [f32; 2]) {
        let Some(drag) = &self.drag else { return };
        for d in drag {
            if d.k < self.graph.len() {
                self.graph.set_point(
                    d.k,
                    [
                        d.start[0] + delta[0] * d.factor[0],
                        d.start[1] + delta[1] * d.factor[1],
                    ],
                );
            }
        }
        self.refresh();
    }

    pub fn drag_end(&mut self) {
        if self.drag.take().is_some()
            && self
                .undo
                .last()
                .is_some_and(|(g, s)| *g == self.graph && *s == self.selected)
        {
            // Nothing moved: no undo step.
            self.undo.pop();
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Edge swap on the two selected vertices' edge.
    pub fn swap_edge(&mut self) -> Result<(), String> {
        let [a, b] = self.selected[..] else {
            return Err("Select the two vertices of an edge.".into());
        };
        let tris = self.preview.clone();
        let mut r = Ok(());
        self.change(|g, _| r = g.swap_edge(a as u32, b as u32, &tris));
        r.map_err(str::to_owned)
    }

    /// 4 Division of the triangles whose corners are all selected.
    pub fn divide4(&mut self) -> Result<(), String> {
        let sel: HashSet<usize> = self.selected.iter().copied().collect();
        let tris = self.preview.clone();
        let mut n = 0;
        self.change(|g, _| n = g.divide4(&tris, &sel));
        if n == 0 {
            return Err("Select the vertices of the triangles to divide.".into());
        }
        Ok(())
    }

    /// Merge Vertices: the selected vertices become one at their centroid.
    /// spec: §5.1 "Merge Vertices"
    pub fn merge(&mut self) -> Result<(), String> {
        if self.selected.len() < 2 {
            return Err("Select the vertices to merge.".into());
        }
        let sel = self.selected.clone();
        self.change(|g, s| {
            *s = g.merge_vertices(&sel).into_iter().collect();
        });
        Ok(())
    }

    /// The centre of the mesh's or the selected vertices' bounding box
    /// ("Center of Selected Vertices").
    pub fn centre(&self, selected_only: bool) -> Option<[f32; 2]> {
        let pts: Vec<[f32; 2]> = if selected_only {
            self.selected.iter().map(|&k| self.graph.point(k)).collect()
        } else {
            (0..self.graph.len()).map(|k| self.graph.point(k)).collect()
        };
        if pts.is_empty() {
            return None;
        }
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in &pts {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        Some([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0])
    }

    /// Shows `g` as a live preview; [`Self::end_preview`] commits it as one
    /// step or restores the graph from before the first preview.
    pub fn set_preview(&mut self, g: MeshGraph) {
        if self.preview_base.is_none() {
            self.preview_base = Some((self.graph.clone(), self.selected.clone()));
        }
        self.graph = g;
        self.selected.clear();
        self.drag = None;
        self.refresh();
    }

    pub fn end_preview(&mut self, commit: bool) {
        let Some(base) = self.preview_base.take() else {
            return;
        };
        if commit {
            if base.0 != self.graph {
                self.undo.push(base);
                self.redo.clear();
            }
        } else {
            (self.graph, self.selected) = base;
            self.refresh();
        }
    }

    /// The graph the preview started from (or the current one).
    pub fn preview_base(&self) -> &MeshGraph {
        self.preview_base.as_ref().map_or(&self.graph, |b| &b.0)
    }

    /// Replaces the selection (an undoable step of the mesh editor).
    pub fn set_selected(&mut self, sel: Vec<usize>) {
        self.change(|_, s| *s = sel);
    }
}

/// The non-auto edge of `g` nearest `q` within `radius`.
fn edge_near(g: &MeshGraph, q: [f32; 2], radius: f32) -> Option<usize> {
    (0..g.mesh.edges.len())
        .filter(|&e| g.mesh.edge_priority[e] != EDGE_AUTO_TRIANGULATION)
        .map(|e| {
            let [a, b] = g.mesh.edges[e].map(|v| g.point(v as usize));
            (e, seg_dist(q, a, b))
        })
        .filter(|&(_, d)| d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e)
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn closest_on_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    [a[0] + t * dx, a[1] + t * dy]
}

fn seg_dist(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    dist(p, closest_on_segment(p, a, b))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use keyform_model::{ArtMeshForm, EditableMesh, KeyformGrid, ObjectInfo, EDGE_NORMAL};

    fn square() -> ArtMesh {
        let positions = vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
        let mesh = EditableMesh {
            edges: vec![[0, 1], [1, 2], [2, 3], [3, 0], [0, 2]],
            edge_priority: vec![EDGE_NORMAL; 5],
            ..Default::default()
        };
        ArtMesh {
            info: ObjectInfo::new("ArtMesh0"),
            target: None,
            uvs: positions.iter().map(|v| v / 10.0).collect(),
            grid: KeyformGrid::single(ArtMeshForm {
                positions: positions.clone(),
                ..ArtMeshForm::default()
            }),
            positions,
            indices: vec![0, 1, 2, 0, 2, 3],
            texture: None,
            clip: vec![],
            invert_clip: false,
            clip_missing: false,
            culling: false,
            color_composition: keyform_model::ColorComposition::Normal,
            alpha_composition: keyform_model::AlphaComposition::Over,
            user_data: String::new(),
            point_uids: vec![0, 1, 2, 3],
            mesh,
            model_image: None,
            generator: Default::default(),
            blend_shapes: Default::default(),
        }
    }

    #[test]
    fn tools_and_local_undo() {
        let mut s = MeshEdit::start(0, &square(), MeshPrefs::new([10.0, 10.0])).unwrap();
        assert!(!s.changed());
        assert_eq!(s.preview.len(), 6);
        // A point on the bottom edge splits it.
        let p = s.add_point([5.0, 0.5], 1.0);
        assert_eq!(s.graph.point(p), [5.0, 0.0]);
        assert_eq!(s.selected, vec![p]);
        assert_eq!(s.preview.len() / 3, 3);
        assert!((s.uvs[2 * p] - 0.5).abs() < 1e-6);
        // Inside: a free point.
        s.add_point([3.0, 7.0], 0.5);
        assert_eq!(s.preview.len() / 3, 5);
        // Move it.
        s.drag_begin(0.5);
        s.drag_update([1.0, 1.0]);
        s.drag_end();
        assert_eq!(s.graph.point(5), [4.0, 8.0]);
        assert!(s.undo());
        assert_eq!(s.graph.point(5), [3.0, 7.0]);
        assert!(s.redo());
        // The eraser deletes the point; Alt deletes the edge only.
        assert!(s.erase([4.0, 8.0], 0.5, false));
        assert_eq!(s.graph.len(), 5);
        assert!(!s.erase([4.0, 8.0], 0.5, true));
        assert!(s.erase([0.0, 5.0], 0.5, true));
        assert!(s.graph.edge_index(3, 0).is_none());
        // Undo everything: back to the start.
        while s.undo() {}
        assert!(!s.changed());
    }

    #[test]
    fn mirror_and_brush() {
        let mut s = MeshEdit::start(0, &square(), MeshPrefs::new([10.0, 10.0])).unwrap();
        // The axis starts at the canvas centre (x = 5).
        s.prefs.mirror_on = true;
        s.set_selected(vec![0]);
        s.drag_begin(0.5);
        s.drag_update([1.0, 2.0]);
        s.drag_end();
        assert_eq!(s.graph.point(0), [1.0, 2.0]);
        assert_eq!(s.graph.point(1), [9.0, 2.0]);
        // Mirrored add: a point and its reflection.
        s.add_point([3.0, 6.0], 0.1);
        assert_eq!(s.graph.len(), 6);
        assert_eq!(s.graph.point(5), [7.0, 6.0]);
        // A point on the axis stays on it.
        let k = s.add_point([5.0, 8.0], 0.1);
        assert_eq!(s.graph.len(), 7);
        s.set_selected(vec![k]);
        s.drag_begin(0.5);
        s.drag_update([2.0, -1.0]);
        s.drag_end();
        assert_eq!(s.graph.point(k), [5.0, 7.0]);
        s.prefs.mirror_on = false;
        // Linear falloff: full inside the hardness radius, 0 at the edge.
        s.prefs.brush_size = 8.0;
        s.prefs.brush_hardness = 0.5;
        assert_eq!(s.brush_weight([1.0, 0.0], false), 1.0);
        assert!((s.brush_weight([3.0, 0.0], false) - 0.5).abs() < 1e-6);
        assert_eq!(s.brush_weight([4.5, 0.0], false), 0.0);
        // Square and line shapes, rotated by the angle.
        s.prefs.brush_shape = BrushShape::Square;
        assert_eq!(s.brush_weight([1.9, 1.9], false), 1.0);
        s.prefs.brush_shape = BrushShape::Line;
        assert_eq!(s.brush_weight([100.0, 1.0], false), 1.0);
        s.prefs.brush_angle = 90.0;
        assert_eq!(s.brush_weight([100.0, 1.0], false), 0.0);
        // A hard circle brush on a corner moves it fully.
        s.prefs.brush_shape = BrushShape::Circle;
        s.prefs.brush_hardness = 1.0;
        s.brush_begin([10.0, 10.0], 0.5, false);
        s.drag_update([1.0, 0.0]);
        s.drag_end();
        assert_eq!(s.graph.point(2), [11.0, 10.0]);
        assert_eq!(s.graph.point(3), [0.0, 10.0]);
    }

    #[test]
    fn reset_merge_and_finish() {
        let mut s = MeshEdit::start(0, &square(), MeshPrefs::new([10.0, 10.0])).unwrap();
        assert!(s.reset_mesh().is_err(), "no model image");
        s.model_rect = Some([[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]]);
        s.glued = true;
        assert_eq!(s.reset_mesh(), Err(RESET_NEEDS_NO_GLUE.into()));
        s.glued = false;
        s.reset_mesh().unwrap();
        assert_eq!(s.graph.point(2), [20.0, 20.0]);
        assert_eq!(s.preview.len(), 6);
        // Merge two corners at their centroid.
        s.set_selected(vec![1, 2]);
        s.merge().unwrap();
        assert_eq!(s.graph.len(), 3);
        assert_eq!(s.graph.point(1), [20.0, 10.0]);
        assert_eq!(s.selected, vec![1]);
        // Coincident vertices are merged on Finish.
        s.add_point([0.0, 0.0], 0.0);
        let (g, note) = s.finished_graph();
        assert_eq!((g.len(), note), (3, Some(OVERLAPPING_MERGED)));
    }

    #[test]
    fn connect_and_delete() {
        let mut s = MeshEdit::start(0, &square(), MeshPrefs::new([10.0, 10.0])).unwrap();
        s.auto_connect().unwrap();
        assert!(s.graph.mesh.edge_priority.iter().all(|&p| p == EDGE_NORMAL));
        s.set_selected(vec![1, 3]);
        s.delete_selected(0.5);
        assert_eq!(s.graph.len(), 2);
        assert!(s.selected.is_empty());
    }
}
