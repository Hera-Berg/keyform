//! Mesh edit mode: editing the point/edge graph of one art mesh. The edits
//! stay in a working copy with its own undo stack; finishing re-triangulates
//! the mesh and remaps its keyforms as one undo step.
//! spec: 50-ui/canvas-and-tools.md §5, 50-ui/main-window.md §6,
//! 30-algorithms/mesh-triangulation.md, 70-advanced/auto-mesh.md §4

use std::collections::HashSet;

use keyform_edit::mesh::{triangulate, MeshGraph, Remap};
use keyform_model::{ArtMesh, EDGE_AUTO_TRIANGULATION, EDGE_NORMAL};

use crate::document::in_quad;

/// Mesh editor sub-tools (canvas-and-tools.md §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshTool {
    /// Select vertices and move them by dragging.
    Select,
    /// Click (or drag) to add a vertex; on an edge it splits the edge.
    AddPoint,
    /// Deletes vertices and edges under the cursor; Alt deletes edges only.
    Erase,
}

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
    drag: Option<Vec<(usize, [f32; 2])>>,
}

impl MeshEdit {
    pub fn start(i: usize, a: &ArtMesh) -> Result<Self, String> {
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
        };
        s.refresh();
        Ok(s)
    }

    /// Whether the graph differs from the mesh at entry.
    pub fn changed(&self) -> bool {
        self.graph != self.start
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
    pub fn edge_at(&self, q: [f32; 2], radius: f32) -> Option<usize> {
        let g = &self.graph;
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

    pub fn points_in(&self, quad: &[[f32; 2]; 4]) -> Vec<usize> {
        (0..self.graph.len())
            .filter(|&k| in_quad(quad, self.graph.point(k)))
            .collect()
    }

    /// Add Point: a new vertex at `q`, splitting the edge under it. The new
    /// point becomes the selection. Returns it.
    pub fn add_point(&mut self, q: [f32; 2], radius: f32) -> usize {
        let edge = self.edge_at(q, radius);
        let mut new = 0;
        self.change(|g, sel| {
            new = match edge {
                Some(e) => {
                    // On the edge itself, so the outline keeps its shape.
                    let [a, b] = g.mesh.edges[e].map(|v| g.point(v as usize));
                    g.split_edge(e, closest_on_segment(q, a, b))
                }
                None => g.add_point(q),
            };
            *sel = vec![new];
        });
        new
    }

    /// Eraser at `q`: points (unless `edges_only`) and edges within
    /// `radius`. Returns whether anything was deleted.
    pub fn erase(&mut self, q: [f32; 2], radius: f32, edges_only: bool) -> bool {
        let g = &self.graph;
        let points: HashSet<usize> = if edges_only {
            HashSet::new()
        } else {
            (0..g.len())
                .filter(|&k| dist(g.point(k), q) <= radius)
                .collect()
        };
        let edges: HashSet<usize> = if points.is_empty() {
            self.edge_at(q, radius).into_iter().collect()
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

    /// Deletes the selected points (and their edges).
    pub fn delete_selected(&mut self) {
        let which: HashSet<usize> = self.selected.iter().copied().collect();
        if which.is_empty() {
            return;
        }
        self.change(|g, sel| {
            g.delete_points(&which);
            sel.clear();
        });
    }

    /// Connect: normal edges between successive selected points.
    pub fn connect(&mut self) {
        let sel = self.selected.clone();
        self.change(|g, _| {
            for w in sel.windows(2) {
                g.add_edge(w[0] as u32, w[1] as u32, EDGE_NORMAL);
            }
        });
    }

    /// Starts moving the selected points (one undo step until released).
    pub fn drag_begin(&mut self) {
        let start = self
            .selected
            .iter()
            .map(|&k| (k, self.graph.point(k)))
            .collect();
        self.undo.push((self.graph.clone(), self.selected.clone()));
        self.redo.clear();
        self.drag = Some(start);
    }

    pub fn drag_update(&mut self, delta: [f32; 2]) {
        let Some(start) = &self.drag else { return };
        for &(k, p) in start {
            if k < self.graph.len() {
                self.graph.set_point(k, [p[0] + delta[0], p[1] + delta[1]]);
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

    /// Replaces the selection (an undoable step of the mesh editor).
    pub fn set_selected(&mut self, sel: Vec<usize>) {
        self.change(|_, s| *s = sel);
    }
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
    use keyform_model::{ArtMeshForm, EditableMesh, KeyformGrid, ObjectInfo};

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
            blend_shapes: Default::default(),
        }
    }

    #[test]
    fn tools_and_local_undo() {
        let mut s = MeshEdit::start(0, &square()).unwrap();
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
        s.drag_begin();
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
    fn connect_and_delete() {
        let mut s = MeshEdit::start(0, &square()).unwrap();
        s.set_selected(vec![1, 3]);
        s.connect();
        assert!(s.graph.edge_index(1, 3).is_some());
        s.delete_selected();
        assert_eq!(s.graph.len(), 2);
        assert!(s.selected.is_empty());
    }
}
