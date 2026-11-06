//! Mesh editing: the editable mesh graph, its re-triangulation, and keeping
//! keyforms and UVs when the topology changes.
//! spec: 30-algorithms/mesh-triangulation.md, 70-advanced/auto-mesh.md §4,
//! 10-formats/cmo3.md §4

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use keyform_model::{
    ArtMesh, EditableMesh, Model, EDGE_AUTO_TRIANGULATION, EDGE_NORMAL, EDGE_USER_TRIANGULATION,
    POINT_NORMAL,
};
use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation as _};

/// spec: 50-ui/canvas-and-tools.md §5.1
pub const PROTECT_NEEDS_OUTLINE: &str =
    "Please close the mesh, or turn off the \"Protect Edge Exterior\" setting.";

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum MeshError {
    #[error("no such art mesh")]
    NoMesh,
    #[error("A vertex has an invalid position.")]
    InvalidPoint,
    /// Deleting the vertex would break a glue binding.
    #[error("Glued vertices can't be deleted ({0}).")]
    Glued(String),
    /// A deform path tracks the vertices (TODO: update deform paths).
    #[error("The mesh of an ArtMesh with a deform path can't be edited yet.")]
    Observed,
}

/// An art mesh's points with its editable graph: the mesh editor's working
/// copy. Points are in canvas pixels (the base form).
#[derive(Clone, Debug, PartialEq)]
pub struct MeshGraph {
    /// x,y per point.
    pub points: Vec<f32>,
    pub uids: Vec<i64>,
    /// `point_priority` always has one entry per point.
    pub mesh: EditableMesh,
}

/// The result of a re-triangulation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Triangulated {
    pub indices: Vec<u32>,
    /// Constraint edges that cross another one and were left out
    /// (spec: mesh-triangulation.md §3 step 3, "unresolved crossing").
    pub unresolved: usize,
    /// The constraint edges enclose an area (there are outlines, §2).
    pub enclosed: bool,
}

impl MeshGraph {
    /// The graph of `a`. A mesh without one (imported from a runtime file)
    /// gets its triangle edges as normal edges, which re-triangulate to the
    /// same triangles.
    pub fn of(a: &ArtMesh) -> Self {
        let n = a.vertex_count();
        let mut mesh = a.mesh.clone();
        mesh.point_priority.resize(n, POINT_NORMAL);
        if mesh.edges.is_empty() {
            let mut seen = HashSet::new();
            for t in a.indices.chunks_exact(3) {
                for (p, q) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    if p != q && seen.insert((p.min(q), p.max(q))) {
                        mesh.edges.push([p.min(q), p.max(q)]);
                    }
                }
            }
            mesh.edge_priority = vec![EDGE_NORMAL; mesh.edges.len()];
        }
        let mut uids = a.point_uids.clone();
        if uids.len() != n {
            uids = (0..n as i64).collect();
        }
        let next = uids.iter().max().map_or(0, |&u| u + 1);
        mesh.next_point_uid = mesh.next_point_uid.max(next);
        Self {
            points: a.positions.clone(),
            uids,
            mesh,
        }
    }

    pub fn len(&self) -> usize {
        self.points.len() / 2
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn point(&self, i: usize) -> [f32; 2] {
        [self.points[2 * i], self.points[2 * i + 1]]
    }

    pub fn set_point(&mut self, i: usize, p: [f32; 2]) {
        self.points[2 * i] = p[0];
        self.points[2 * i + 1] = p[1];
    }

    /// Adds a normal point with a fresh UID; returns its index.
    pub fn add_point(&mut self, p: [f32; 2]) -> usize {
        self.points.extend_from_slice(&p);
        self.uids.push(self.mesh.next_point_uid);
        self.mesh.next_point_uid += 1;
        self.mesh.point_priority.push(POINT_NORMAL);
        self.len() - 1
    }

    pub fn edge_index(&self, a: u32, b: u32) -> Option<usize> {
        self.mesh
            .edges
            .iter()
            .position(|e| (e[0] == a && e[1] == b) || (e[0] == b && e[1] == a))
    }

    /// Adds edge a–b (or changes an existing one's priority).
    pub fn add_edge(&mut self, a: u32, b: u32, priority: i8) {
        if a == b {
            return;
        }
        match self.edge_index(a, b) {
            Some(e) => self.mesh.edge_priority[e] = priority,
            None => {
                self.mesh.edges.push([a, b]);
                self.mesh.edge_priority.push(priority);
            }
        }
    }

    /// Inserts a point on edge `e` ("Add Vertex on Edge"): the edge is
    /// replaced by two with its priority. Returns the new point.
    pub fn split_edge(&mut self, e: usize, p: [f32; 2]) -> usize {
        let [a, b] = self.mesh.edges[e];
        let priority = self.mesh.edge_priority[e];
        let c = self.add_point(p) as u32;
        self.mesh.edges[e] = [a, c];
        self.mesh.edges.push([c, b]);
        self.mesh.edge_priority.push(priority);
        c as usize
    }

    /// Deletes the edges whose indices are in `which`.
    pub fn delete_edges(&mut self, which: &HashSet<usize>) {
        let keep: Vec<bool> = (0..self.mesh.edges.len())
            .map(|e| !which.contains(&e))
            .collect();
        retain_parallel(&mut self.mesh.edges, &mut self.mesh.edge_priority, &keep);
    }

    /// Deletes points and every edge touching them; later points move down.
    pub fn delete_points(&mut self, which: &HashSet<usize>) {
        let n = self.len();
        let mut new_index = vec![u32::MAX; n];
        let mut next = 0;
        for (i, slot) in new_index.iter_mut().enumerate() {
            if !which.contains(&i) {
                *slot = next;
                next += 1;
            }
        }
        let keep_point: Vec<bool> = (0..n).map(|i| !which.contains(&i)).collect();
        let mut points = Vec::with_capacity(2 * next as usize);
        for i in (0..n).filter(|&i| keep_point[i]) {
            points.extend_from_slice(&self.point(i));
        }
        self.points = points;
        let mut uids = std::mem::take(&mut self.uids);
        let mut prio = std::mem::take(&mut self.mesh.point_priority);
        retain_parallel(&mut uids, &mut prio, &keep_point);
        (self.uids, self.mesh.point_priority) = (uids, prio);
        let keep_edge: Vec<bool> = self
            .mesh
            .edges
            .iter()
            .map(|e| keep_point[e[0] as usize] && keep_point[e[1] as usize])
            .collect();
        retain_parallel(
            &mut self.mesh.edges,
            &mut self.mesh.edge_priority,
            &keep_edge,
        );
        for e in &mut self.mesh.edges {
            *e = [new_index[e[0] as usize], new_index[e[1] as usize]];
        }
    }

    /// Edge swap: the edge a–b shared by two triangles of `tris` is
    /// replaced by the quad's other diagonal, kept as a user-triangulation
    /// edge (spec: canvas-and-tools.md §5 "Edge swap"). Fails when a–b is
    /// not an inner edge or the quad is not convex.
    pub fn swap_edge(&mut self, a: u32, b: u32, tris: &[u32]) -> Result<(), &'static str> {
        let opposite: Vec<u32> = tris
            .chunks_exact(3)
            .filter(|t| t.contains(&a) && t.contains(&b))
            .filter_map(|t| t.iter().copied().find(|&v| v != a && v != b))
            .collect();
        let [c, d] = opposite[..] else {
            return Err("Select an edge between two triangles.");
        };
        // Convex: a and b lie on opposite sides of c–d.
        let p = |v: u32| self.point(v as usize);
        let side = |x: [f32; 2]| {
            let (pc, pd) = (p(c), p(d));
            (pd[0] - pc[0]) * (x[1] - pc[1]) - (pd[1] - pc[1]) * (x[0] - pc[0])
        };
        if side(p(a)) * side(p(b)) >= 0.0 {
            return Err("The edge can't be swapped (the quadrilateral is not convex).");
        }
        if let Some(e) = self.edge_index(a, b) {
            self.delete_edges(&HashSet::from([e]));
        }
        self.add_edge(c, d, EDGE_USER_TRIANGULATION);
        Ok(())
    }

    /// 4 Division: every triangle of `tris` whose corners are all in
    /// `selected` is split into four by its edge midpoints (shared
    /// midpoints for shared edges; new edges are normal). Returns how many
    /// triangles were divided.
    pub fn divide4(&mut self, tris: &[u32], selected: &HashSet<usize>) -> usize {
        let chosen: Vec<[u32; 3]> = tris
            .chunks_exact(3)
            .filter(|t| t.iter().all(|&v| selected.contains(&(v as usize))))
            .map(|t| [t[0], t[1], t[2]])
            .collect();
        let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
        for t in &chosen {
            let mut m = [0u32; 3];
            for (k, (a, b)) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
                .into_iter()
                .enumerate()
            {
                let key = (a.min(b), a.max(b));
                m[k] = match mid.get(&key) {
                    Some(&v) => v,
                    None => {
                        let (pa, pb) = (self.point(a as usize), self.point(b as usize));
                        let c = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
                        let v = match self.edge_index(a, b) {
                            Some(e) if self.mesh.edge_priority[e] != EDGE_AUTO_TRIANGULATION => {
                                self.split_edge(e, c) as u32
                            }
                            other => {
                                if let Some(e) = other {
                                    self.delete_edges(&HashSet::from([e]));
                                }
                                let v = self.add_point(c) as u32;
                                self.add_edge(a, v, EDGE_NORMAL);
                                self.add_edge(v, b, EDGE_NORMAL);
                                v
                            }
                        };
                        mid.insert(key, v);
                        v
                    }
                };
            }
            self.add_edge(m[0], m[1], EDGE_NORMAL);
            self.add_edge(m[1], m[2], EDGE_NORMAL);
            self.add_edge(m[2], m[0], EDGE_NORMAL);
        }
        chosen.len()
    }

    /// Merge Vertices: the points of `sel` become one, the lowest index
    /// moved to their centroid. The other points' non-auto edges are
    /// re-attached to it as normal edges (no loops, no duplicates), then
    /// they are deleted. Returns the survivor's index afterwards.
    /// spec: 50-ui/canvas-and-tools.md §5.1 "Merge Vertices"
    pub fn merge_vertices(&mut self, sel: &[usize]) -> Option<usize> {
        let n = self.len();
        let mut sel: Vec<usize> = sel.iter().copied().filter(|&k| k < n).collect();
        sel.sort_unstable();
        sel.dedup();
        let (&keep, rest) = sel.split_first()?;
        if rest.is_empty() {
            return None;
        }
        let c = sel.iter().fold([0.0f64; 2], |c, &k| {
            let p = self.point(k);
            [c[0] + f64::from(p[0]), c[1] + f64::from(p[1])]
        });
        let m = sel.len() as f64;
        self.set_point(keep, [(c[0] / m) as f32, (c[1] / m) as f32]);
        let gone: HashSet<usize> = rest.iter().copied().collect();
        let moved: Vec<u32> = self
            .mesh
            .edges
            .iter()
            .zip(&self.mesh.edge_priority)
            .filter(|(_, &p)| p != EDGE_AUTO_TRIANGULATION)
            .filter_map(|(e, _)| {
                let (a, b) = (e[0] as usize, e[1] as usize);
                match (gone.contains(&a), gone.contains(&b)) {
                    (true, false) => Some(b as u32),
                    (false, true) => Some(a as u32),
                    _ => None,
                }
            })
            .collect();
        for other in moved {
            if other as usize != keep {
                self.add_edge(keep as u32, other, EDGE_NORMAL);
            }
        }
        self.delete_points(&gone);
        Some(keep - rest.iter().filter(|&&k| k < keep).count())
    }

    /// Overlapping-vertex clean-up when mesh editing finishes: of points
    /// with exactly equal coordinates the first is kept; the others go with
    /// their own edges. Returns how many were removed.
    /// spec: 50-ui/canvas-and-tools.md §5.1
    pub fn remove_coincident(&mut self) -> usize {
        let mut seen: HashMap<(u32, u32), usize> = HashMap::new();
        let mut gone = HashSet::new();
        for k in 0..self.len() {
            let p = self.point(k);
            if seen.insert((p[0].to_bits(), p[1].to_bits()), k).is_some() {
                gone.insert(k);
            }
        }
        // Keep the first of each group, not the last.
        let gone: HashSet<usize> = gone
            .into_iter()
            .filter(|&k| {
                let p = self.point(k);
                (0..k).any(|j| {
                    let q = self.point(j);
                    q[0].to_bits() == p[0].to_bits() && q[1].to_bits() == p[1].to_bits()
                })
            })
            .collect();
        if !gone.is_empty() {
            self.delete_points(&gone);
        }
        gone.len()
    }

    /// Auto connect (Ctrl+R / Connect / Auto connect): Delaunay connections
    /// made permanent as normal edges. `protect` keeps the outline (and
    /// needs one); `reconstruct` rebuilds every edge.
    /// spec: 50-ui/canvas-and-tools.md §5.1 "Auto connect"
    pub fn auto_connect(&mut self, protect: bool, reconstruct: bool) -> Result<(), &'static str> {
        let n = self.len();
        let constraints = constraint_edges(n, &self.mesh.edges, &self.mesh.edge_priority);
        let current = triangulate_delaunay(&self.points, &constraints, false)
            .map_err(|_| "A vertex has an invalid position.")?;
        if protect && !current.enclosed {
            return Err(PROTECT_NEEDS_OUTLINE);
        }
        if reconstruct {
            self.mesh.edges.clear();
            self.mesh.edge_priority.clear();
            if protect {
                // The outline: sides of exactly one enclosed triangle.
                let mut count: HashMap<(u32, u32), usize> = HashMap::new();
                for t in current.indices.chunks_exact(3) {
                    for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                        *count.entry((a.min(b), a.max(b))).or_default() += 1;
                    }
                }
                let mut outline: Vec<(u32, u32)> = count
                    .into_iter()
                    .filter(|&(_, c)| c == 1)
                    .map(|(e, _)| e)
                    .collect();
                outline.sort_unstable();
                for (a, b) in outline {
                    self.add_edge(a, b, EDGE_NORMAL);
                }
            }
        }
        let constraints = constraint_edges(n, &self.mesh.edges, &self.mesh.edge_priority);
        let t = triangulate_delaunay(&self.points, &constraints, !protect)
            .map_err(|_| "A vertex has an invalid position.")?;
        // Every triangle side becomes a permanent normal edge.
        let keep: Vec<bool> = self
            .mesh
            .edge_priority
            .iter()
            .map(|&p| p != EDGE_AUTO_TRIANGULATION)
            .collect();
        retain_parallel(&mut self.mesh.edges, &mut self.mesh.edge_priority, &keep);
        for tri in t.indices.chunks_exact(3) {
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                if self.edge_index(a, b).is_none() {
                    self.add_edge(a.min(b), a.max(b), EDGE_NORMAL);
                }
            }
        }
        Ok(())
    }

    /// Reset Mesh: all points and edges replaced by the rectangle
    /// `corners` (canvas) with its four sides as normal edges.
    /// spec: 50-ui/canvas-and-tools.md §5.1 "Reset Mesh"
    pub fn reset_to_rect(&mut self, corners: [[f32; 2]; 4]) {
        let n = self.len();
        self.delete_points(&(0..n).collect());
        for c in corners {
            self.add_point(c);
        }
        for k in 0..4u32 {
            self.add_edge(k, (k + 1) % 4, EDGE_NORMAL);
        }
    }

    /// Mirror Edit partner of point `k`: the other point nearest to `k`'s
    /// reflection across the axis (x = `axis` when `vertical`, else
    /// y = `axis`), within `tolerance`.
    pub fn mirror_partner(
        &self,
        k: usize,
        axis: f32,
        vertical: bool,
        tolerance: f32,
    ) -> Option<usize> {
        let p = self.point(k);
        let r = if vertical {
            [2.0 * axis - p[0], p[1]]
        } else {
            [p[0], 2.0 * axis - p[1]]
        };
        (0..self.len())
            .filter(|&j| j != k)
            .map(|j| {
                let q = self.point(j);
                (j, (q[0] - r[0]).hypot(q[1] - r[1]))
            })
            .filter(|&(_, d)| d <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(j, _)| j)
    }

    /// Whether `other` has the same points (by UID, in order) and edges:
    /// then only positions differ, and the triangle list is kept
    /// (spec: mesh-triangulation.md §3a).
    pub fn same_edges(&self, other: &MeshGraph) -> bool {
        self.uids == other.uids
            && self.mesh.edges == other.mesh.edges
            && self.mesh.edge_priority == other.mesh.edge_priority
    }

    /// Re-triangulates the graph (spec: mesh-triangulation.md §3, or the
    /// face fans of §3a when `useDelaunayTriangulation` is off): the
    /// auto-triangulation edges are regenerated from the result.
    pub fn triangulate(&mut self) -> Result<Triangulated, MeshError> {
        let t = triangulate(
            &self.points,
            &self.mesh.edges,
            &self.mesh.edge_priority,
            self.mesh.delaunay,
        )?;
        // Step 9: every triangle edge that isn't a constraint becomes an
        // auto-triangulation edge.
        let keep: Vec<bool> = self
            .mesh
            .edge_priority
            .iter()
            .map(|&p| p != EDGE_AUTO_TRIANGULATION)
            .collect();
        retain_parallel(&mut self.mesh.edges, &mut self.mesh.edge_priority, &keep);
        let mut have: HashSet<(u32, u32)> = self
            .mesh
            .edges
            .iter()
            .map(|e| (e[0].min(e[1]), e[0].max(e[1])))
            .collect();
        for tri in t.indices.chunks_exact(3) {
            for (p, q) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                if have.insert((p.min(q), p.max(q))) {
                    self.mesh.edges.push([p.min(q), p.max(q)]);
                    self.mesh.edge_priority.push(EDGE_AUTO_TRIANGULATION);
                }
            }
        }
        Ok(t)
    }
}

fn retain_parallel<A, B>(a: &mut Vec<A>, b: &mut Vec<B>, keep: &[bool]) {
    let mut k = keep.iter();
    a.retain(|_| *k.next().unwrap_or(&true));
    let mut k = keep.iter();
    b.retain(|_| *k.next().unwrap_or(&true));
}

/// Triangles of the graph: constrained Delaunay (§2–3) or, with
/// `delaunay` off, face fans (§3a). spec: 30-algorithms/mesh-triangulation.md
pub fn triangulate(
    points: &[f32],
    edges: &[[u32; 2]],
    priority: &[i8],
    delaunay: bool,
) -> Result<Triangulated, MeshError> {
    if points.iter().any(|v| !v.is_finite()) {
        return Err(MeshError::InvalidPoint);
    }
    let constraints = constraint_edges(points.len() / 2, edges, priority);
    if delaunay {
        triangulate_delaunay(points, &constraints, false)
    } else {
        Ok(triangulate_fans(points, &constraints))
    }
}

/// The edges that must appear in the result: every edge whose priority
/// isn't auto-triangulation (§1).
fn constraint_edges(n: usize, edges: &[[u32; 2]], priority: &[i8]) -> Vec<[usize; 2]> {
    edges
        .iter()
        .zip(priority.iter().chain(std::iter::repeat(&EDGE_NORMAL)))
        .filter(|&(e, &p)| {
            p != EDGE_AUTO_TRIANGULATION && (e[0] as usize) < n && (e[1] as usize) < n
        })
        .map(|(e, _)| [e[0] as usize, e[1] as usize])
        .collect()
}

/// The older non-Delaunay mode (`useDelaunayTriangulation` = false): faces
/// traced on the constraint edges, kept by the top-most-vertex test, and
/// fanned from their first vertex. Orientation is left as traced.
/// spec: 30-algorithms/mesh-triangulation.md §3a
fn triangulate_fans(points: &[f32], constraints: &[[usize; 2]]) -> Triangulated {
    let n = points.len() / 2;
    let p = |i: usize| (f64::from(points[2 * i]), f64::from(points[2 * i + 1]));
    let angle = |from: usize, to: usize| {
        let ((x0, y0), (x1, y1)) = (p(from), p(to));
        (y1 - y0).atan2(x1 - x0)
    };
    // Half-edges in edge order, both directions; `out[v]` lists v's.
    let mut half: Vec<[usize; 2]> = Vec::new();
    let mut seen = HashSet::new();
    for &[a, b] in constraints {
        if a != b && seen.insert((a.min(b), a.max(b))) {
            half.push([a, b]);
            half.push([b, a]);
        }
    }
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (h, e) in half.iter().enumerate() {
        out[e[0]].push(h);
    }
    let mut used = vec![false; half.len()];
    let mut faces: Vec<Vec<usize>> = Vec::new();
    let two_pi = std::f64::consts::TAU;
    for start in 0..half.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let [s, mut v] = half[start];
        let mut prev = s;
        let mut walk = vec![s, v];
        for _ in 0..2 * n {
            if v == s {
                walk.pop();
                faces.push(std::mem::take(&mut walk));
                break;
            }
            // Leave by the unused half-edge with the smallest
            // (back − out) normalised to [0, 2π]; never straight back.
            let back = angle(v, prev);
            let next = out[v]
                .iter()
                .copied()
                .filter(|&h| !used[h] && half[h][1] != prev)
                .map(|h| {
                    let mut d = back - angle(v, half[h][1]);
                    while d < 0.0 {
                        d += two_pi;
                    }
                    while d > two_pi {
                        d -= two_pi;
                    }
                    (h, d)
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let Some((h, _)) = next else { break };
            used[h] = true;
            let w = half[h][1];
            (prev, v) = (v, w);
            if w != s {
                // A repeated vertex closes a sub-loop: cut it off.
                if let Some(i) = walk.iter().position(|&x| x == w) {
                    faces.push(walk.split_off(i));
                }
            }
            walk.push(w);
        }
    }
    let mut indices = Vec::new();
    for f in faces {
        if f.len() < 3 || f.len() > 1000 {
            continue;
        }
        // The vertex with the greatest y, first reached walking cyclically
        // from the last vertex.
        let m = f.len();
        let mut best = m - 1;
        for k in (0..m).map(|k| (m - 1 + k) % m) {
            if p(f[k]).1 > p(f[best]).1 {
                best = k;
            }
        }
        let (v, a, b) = (f[best], f[(best + m - 1) % m], f[(best + 1) % m]);
        let ((xv, yv), (xa, ya), (xb, yb)) = (p(v), p(a), p(b));
        let keep = if ya == yv {
            xv < xa
        } else if yb == yv {
            xv > xb
        } else {
            angle(v, b) < angle(v, a)
        };
        if keep {
            for i in 1..m - 1 {
                indices.extend_from_slice(&[f[0] as u32, f[i] as u32, f[i + 1] as u32]);
            }
        }
    }
    let enclosed = !indices.is_empty();
    Triangulated {
        indices,
        unresolved: 0,
        enclosed,
    }
}

/// Constrained Delaunay triangulation of `points` honouring the
/// constraint edges, keeping only the triangles enclosed by them, oriented
/// to positive area. spec: 30-algorithms/mesh-triangulation.md §2–3
/// With `keep_hull` every triangle of the convex hull is kept.
fn triangulate_delaunay(
    points: &[f32],
    constraints: &[[usize; 2]],
    keep_hull: bool,
) -> Result<Triangulated, MeshError> {
    let n = points.len() / 2;
    let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
    let mut handle: Vec<Option<FixedVertexHandle>> = vec![None; n];
    // Vertex handle → the first point inserted there (coincident points
    // share a vertex; the later ones get no triangles).
    let mut owner: HashMap<usize, u32> = HashMap::new();
    let mut insert = |cdt: &mut ConstrainedDelaunayTriangulation<Point2<f64>>,
                      handle: &mut Vec<Option<FixedVertexHandle>>,
                      i: usize|
     -> Result<(), MeshError> {
        if handle[i].is_some() {
            return Ok(());
        }
        let (x, y) = (f64::from(points[2 * i]), f64::from(points[2 * i + 1]));
        let h = cdt
            .insert(Point2::new(x, y))
            .map_err(|_| MeshError::InvalidPoint)?;
        owner.entry(h.index()).or_insert(i as u32);
        handle[i] = Some(h);
        Ok(())
    };
    // Steps 2 and 5: constraint endpoints first, then the other points.
    for c in constraints {
        insert(&mut cdt, &mut handle, c[0])?;
        insert(&mut cdt, &mut handle, c[1])?;
    }
    for i in 0..n {
        insert(&mut cdt, &mut handle, i)?;
    }
    // Steps 3–4 (spade restores the Delaunay property itself).
    let mut unresolved = 0;
    for c in constraints {
        let (Some(a), Some(b)) = (handle[c[0]], handle[c[1]]) else {
            continue;
        };
        if a == b || cdt.exists_constraint(a, b) {
            continue;
        }
        if cdt.can_add_constraint(a, b) {
            cdt.add_constraint(a, b);
        } else {
            unresolved += 1;
        }
    }
    // Step 8: drop the triangles outside every outline. Stored triangle
    // lists with fewer triangles get filled by this (§3a, as the reference). A triangle is
    // inside an outline (a bounded face of the constraint graph) exactly
    // when it can't be reached from the outer face without crossing a
    // constraint edge; with no outlines every triangle is kept.
    let faces: Vec<_> = cdt.inner_faces().collect();
    let slot: HashMap<usize, usize> = faces
        .iter()
        .enumerate()
        .map(|(k, f)| (f.fix().index(), k))
        .collect();
    let mut outside = vec![false; faces.len()];
    let mut queue = Vec::new();
    for (k, f) in faces.iter().enumerate() {
        let open = f
            .adjacent_edges()
            .iter()
            .any(|e| !cdt.is_constraint_edge(e.as_undirected().fix()) && e.rev().face().is_outer());
        if open {
            outside[k] = true;
            queue.push(k);
        }
    }
    while let Some(k) = queue.pop() {
        for e in faces[k].adjacent_edges() {
            if cdt.is_constraint_edge(e.as_undirected().fix()) {
                continue;
            }
            if let Some(g) = e.rev().face().as_inner() {
                if let Some(&j) = slot.get(&g.fix().index()) {
                    if !outside[j] {
                        outside[j] = true;
                        queue.push(j);
                    }
                }
            }
        }
    }
    let enclosed = !outside.iter().all(|&o| o);
    let keep_all = keep_hull || !enclosed;
    let mut indices = Vec::with_capacity(faces.len() * 3);
    for (k, f) in faces.iter().enumerate() {
        if outside[k] && !keep_all {
            continue;
        }
        let v = f.vertices().map(|v| owner[&v.fix().index()]);
        // Step 10: positive signed area in canvas pixels (y down).
        let p = |i: u32| {
            let i = i as usize;
            (f64::from(points[2 * i]), f64::from(points[2 * i + 1]))
        };
        let ((x0, y0), (x1, y1), (x2, y2)) = (p(v[0]), p(v[1]), p(v[2]));
        if (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0) > 0.0 {
            indices.extend_from_slice(&[v[0], v[1], v[2]]);
        } else {
            indices.extend_from_slice(&[v[0], v[2], v[1]]);
        }
    }
    Ok(Triangulated {
        indices,
        unresolved,
        enclosed,
    })
}

/// Where a point sits on a source mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Location {
    /// Exactly on source vertex `v` (copied bit for bit).
    Vertex(usize),
    /// Barycentric coordinates in source triangle `t` (may be negative
    /// outside it).
    Triangle(usize, [f64; 3]),
    /// The source has no usable triangle.
    None,
}

/// Piecewise-linear map from a source mesh (base positions + triangles) to
/// any per-vertex data on it. spec: 70-advanced/auto-mesh.md §4
pub struct Remap {
    points: Vec<[f64; 2]>,
    triangles: Vec<[usize; 3]>,
}

impl Remap {
    pub fn new(positions: &[f32], indices: &[u32]) -> Self {
        let points: Vec<[f64; 2]> = positions
            .chunks_exact(2)
            .map(|p| [f64::from(p[0]), f64::from(p[1])])
            .collect();
        let n = points.len();
        let triangles = indices
            .chunks_exact(3)
            .map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
            .filter(|t| t.iter().all(|&i| i < n))
            .filter(|t| area2(points[t[0]], points[t[1]], points[t[2]]).abs() > 1e-12)
            .collect();
        Self { points, triangles }
    }

    /// The triangle containing `p`, or the nearest one.
    pub fn locate(&self, p: [f32; 2]) -> Location {
        let p = [f64::from(p[0]), f64::from(p[1])];
        let mut best: Option<(f64, usize, [f64; 3])> = None;
        for (k, t) in self.triangles.iter().enumerate() {
            let [a, b, c] = t.map(|i| self.points[i]);
            let d = area2(a, b, c);
            let w = [area2(p, b, c) / d, area2(a, p, c) / d, area2(a, b, p) / d];
            let dist = if w.iter().all(|&x| x >= -1e-9) {
                0.0
            } else {
                seg_dist(p, a, b)
                    .min(seg_dist(p, b, c))
                    .min(seg_dist(p, c, a))
            };
            if best.is_none_or(|(bd, _, _)| dist < bd) {
                best = Some((dist, k, w));
                if dist == 0.0 {
                    break;
                }
            }
        }
        match best {
            Some((_, k, w)) => Location::Triangle(k, w),
            None => self
                .nearest_vertex(p)
                .map_or(Location::None, Location::Vertex),
        }
    }

    fn nearest_vertex(&self, p: [f64; 2]) -> Option<usize> {
        (0..self.points.len()).min_by(|&i, &j| {
            let d = |q: [f64; 2]| (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
            d(self.points[i]).total_cmp(&d(self.points[j]))
        })
    }

    /// `values` (2 per source vertex) at `at`.
    pub fn apply(&self, at: Location, values: &[f32]) -> [f32; 2] {
        let get = |i: usize| -> [f32; 2] {
            [
                values.get(2 * i).copied().unwrap_or(0.0),
                values.get(2 * i + 1).copied().unwrap_or(0.0),
            ]
        };
        match at {
            Location::Vertex(v) => get(v),
            Location::Triangle(k, w) => {
                let t = self.triangles[k];
                let mut out = [0.0f64; 2];
                for (&i, &wi) in t.iter().zip(&w) {
                    let v = get(i);
                    out[0] += wi * f64::from(v[0]);
                    out[1] += wi * f64::from(v[1]);
                }
                [out[0] as f32, out[1] as f32]
            }
            Location::None => [0.0, 0.0],
        }
    }
}

fn area2(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

/// Replaces art mesh `i`'s points and graph with `g`, re-triangulates, and
/// remaps every keyform, blend shape and the UVs from the old mesh
/// (spec: 70-advanced/auto-mesh.md §4). Points whose UID and position are
/// unchanged keep their data exactly. Glue pairs follow the vertex UIDs.
///
/// UVs are remapped piecewise-linearly like the keyforms. They are an
/// affine function of the base positions (texture-atlas.md §4), which
/// barycentric interpolation and extrapolation reproduce exactly.
pub fn set_mesh(m: &mut Model, i: usize, g: &MeshGraph) -> Result<Triangulated, MeshError> {
    let old = m.art_meshes.get(i).ok_or(MeshError::NoMesh)?.clone();
    if old.mesh.observed {
        return Err(MeshError::Observed);
    }
    if g.points.iter().any(|v| !v.is_finite()) {
        return Err(MeshError::InvalidPoint);
    }
    let mut g = g.clone();
    // Moving points alone keeps the triangle list (§3a).
    let t = if g.same_edges(&MeshGraph::of(&old)) {
        Triangulated {
            indices: old.indices.clone(),
            ..Default::default()
        }
    } else {
        g.triangulate()?
    };
    let n = g.len();

    // Glue pairs are stored by vertex UID.
    let uid_index: HashMap<i64, usize> = g.uids.iter().enumerate().map(|(k, &u)| (u, k)).collect();
    let mut glues = Vec::new();
    for (gi, glue) in m.glues.iter().enumerate() {
        let (a, b) = (glue.mesh_a == Some(i), glue.mesh_b == Some(i));
        if !a && !b {
            continue;
        }
        let map = |v: Option<usize>| -> Result<Option<usize>, MeshError> {
            let Some(v) = v else { return Ok(None) };
            let uid = old.point_uids.get(v).copied().unwrap_or(v as i64);
            uid_index
                .get(&uid)
                .copied()
                .map(Some)
                .ok_or_else(|| MeshError::Glued(glue.info.display_name().to_owned()))
        };
        let mut pairs = glue.pairs.clone();
        for p in &mut pairs {
            if a {
                p.0 = map(p.0)?;
            }
            if b {
                p.1 = map(p.1)?;
            }
        }
        if pairs != glue.pairs {
            glues.push((gi, pairs));
        }
    }

    let remap = Remap::new(&old.positions, &old.indices);
    let old_uid: HashMap<i64, usize> = old
        .point_uids
        .iter()
        .enumerate()
        .map(|(k, &u)| (u, k))
        .collect();
    let at: Vec<Location> = (0..n)
        .map(|k| {
            let p = g.point(k);
            match old_uid.get(&g.uids[k]) {
                Some(&v) if old.positions.get(2 * v..2 * v + 2) == Some(&p[..]) => {
                    Location::Vertex(v)
                }
                _ => remap.locate(p),
            }
        })
        .collect();
    let map_all =
        |values: &[f32]| -> Vec<f32> { at.iter().flat_map(|&l| remap.apply(l, values)).collect() };

    let a = Arc::make_mut(&mut m.art_meshes[i]);
    for f in &mut a.grid.keyforms {
        f.positions = map_all(&f.positions);
    }
    for s in &mut a.blend_shapes.shapes {
        s.form.positions = map_all(&s.form.positions);
    }
    a.uvs = map_all(&old.uvs);
    a.positions = g.points;
    a.point_uids = g.uids;
    a.indices = t.indices.clone();
    a.mesh = g.mesh;
    for (gi, pairs) in glues {
        Arc::make_mut(&mut m.glues[gi]).pairs = pairs;
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use keyform_model::{ArtMeshForm, KeyformGrid, ObjectInfo, EDGE_LOCKED};

    fn graph(points: &[[f32; 2]], edges: &[[u32; 2]]) -> MeshGraph {
        let mesh = EditableMesh {
            point_priority: vec![POINT_NORMAL; points.len()],
            edges: edges.to_vec(),
            edge_priority: vec![EDGE_NORMAL; edges.len()],
            ..Default::default()
        };
        MeshGraph {
            points: points.iter().flatten().copied().collect(),
            uids: (0..points.len() as i64).collect(),
            mesh,
        }
    }

    fn tri_set(indices: &[u32]) -> HashSet<[u32; 3]> {
        indices
            .chunks_exact(3)
            .map(|t| {
                let mut s = [t[0], t[1], t[2]];
                s.sort_unstable();
                s
            })
            .collect()
    }

    fn positive(points: &[f32], indices: &[u32]) -> bool {
        indices.chunks_exact(3).all(|t| {
            let p = |i: u32| [points[2 * i as usize], points[2 * i as usize + 1]];
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0
        })
    }

    #[test]
    fn square_outline() {
        let mut g = graph(
            &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            &[[0, 1], [1, 2], [2, 3], [3, 0]],
        );
        let t = g.triangulate().unwrap();
        assert_eq!(t.indices.len(), 6);
        assert!(positive(&g.points, &t.indices));
        // One diagonal was recorded as an auto edge.
        let auto: Vec<_> = (0..g.mesh.edges.len())
            .filter(|&e| g.mesh.edge_priority[e] == EDGE_AUTO_TRIANGULATION)
            .collect();
        assert_eq!(auto.len(), 1);
        // Re-triangulating replaces it instead of keeping it.
        g.triangulate().unwrap();
        assert_eq!(g.mesh.edges.len(), 5);
    }

    #[test]
    fn concave_outline_drops_outside_triangles() {
        // An L shape: the notch between (5,5), (10,5) and (5,10) is outside.
        let pts = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 5.0],
            [5.0, 5.0],
            [5.0, 10.0],
            [0.0, 10.0],
        ];
        let edges = [[0, 1], [1, 2], [2, 3], [3, 4], [4, 5], [5, 0]];
        let t = triangulate(&graph(&pts, &edges).points, &edges, &[EDGE_NORMAL; 6], true).unwrap();
        assert_eq!(t.indices.len() / 3, 4);
        assert!(!tri_set(&t.indices).contains(&[2, 3, 4]));
        // The constraint (3,4)–(2,3) are triangle edges, and every
        // centroid is inside the L.
        for tri in t.indices.chunks_exact(3) {
            let c = tri.iter().fold([0.0, 0.0], |c, &i| {
                [
                    c[0] + pts[i as usize][0] / 3.0,
                    c[1] + pts[i as usize][1] / 3.0,
                ]
            });
            assert!(c[0] < 5.0 || c[1] < 5.0, "{c:?}");
        }
    }

    #[test]
    fn no_outline_keeps_everything() {
        let pts = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [4.0, 6.0],
        ];
        let g = graph(&pts, &[]);
        let t = triangulate(&g.points, &[], &[], true).unwrap();
        assert_eq!(t.indices.len() / 3, 4);
        assert!(positive(&g.points, &t.indices));
    }

    #[test]
    fn crossing_constraints_are_reported() {
        let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let edges = [[0, 1], [1, 2], [2, 3], [3, 0], [0, 2], [1, 3]];
        let g = graph(&pts, &edges);
        let t = triangulate(&g.points, &edges, &[EDGE_LOCKED; 6], true).unwrap();
        assert_eq!(t.unresolved, 1);
        assert_eq!(t.indices.len(), 6);
    }

    #[test]
    fn invalid_points_fail() {
        let edges = [[0, 1]];
        let r = triangulate(&[0.0, 0.0, f32::NAN, 1.0], &edges, &[EDGE_NORMAL], true);
        assert_eq!(r, Err(MeshError::InvalidPoint));
    }

    #[test]
    fn graph_edits_keep_arrays_parallel() {
        let mut g = graph(
            &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            &[[0, 1], [1, 2], [2, 3], [3, 0]],
        );
        let c = g.split_edge(0, [5.0, 0.0]);
        assert_eq!(c, 4);
        assert_eq!(g.uids[4], 1000);
        assert_eq!(g.mesh.next_point_uid, 1001);
        assert!(g.edge_index(0, 4).is_some() && g.edge_index(4, 1).is_some());
        g.delete_points(&HashSet::from([1]));
        assert_eq!(g.len(), 4);
        assert_eq!(g.uids, vec![0, 2, 3, 1000]);
        assert_eq!(g.mesh.point_priority.len(), 4);
        assert_eq!(g.mesh.edges.len(), g.mesh.edge_priority.len());
        // Edges touching point 1 are gone; the rest were renumbered.
        assert!(g.mesh.edges.iter().all(|e| e[0] < 4 && e[1] < 4));
        assert_eq!(g.mesh.edges.len(), 3);
    }

    fn quad_mesh() -> ArtMesh {
        let positions = vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
        // Keyform: an affine map x' = 2x + 1, y' = y − 3.
        let form = ArtMeshForm {
            positions: positions
                .chunks_exact(2)
                .flat_map(|p| [2.0 * p[0] + 1.0, p[1] - 3.0])
                .collect(),
            ..ArtMeshForm::default()
        };
        let mesh = EditableMesh {
            edges: vec![[0, 1], [1, 2], [2, 3], [3, 0], [0, 2]],
            edge_priority: vec![EDGE_NORMAL; 5],
            ..Default::default()
        };
        ArtMesh {
            info: ObjectInfo::new("ArtMesh0"),
            target: None,
            uvs: positions.iter().map(|v| v / 20.0).collect(),
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
            grid: KeyformGrid::single(form),
            blend_shapes: Default::default(),
            paths: Vec::new(),
        }
    }

    #[test]
    fn set_mesh_remaps_keyforms_and_uvs() {
        let mut m = Model::default();
        m.art_meshes.push(Arc::new(quad_mesh()));
        let before = m.art_meshes[0].grid.keyforms[0].positions.clone();
        let mut g = MeshGraph::of(&m.art_meshes[0]);
        // A point inside and one outside (to the right).
        g.add_point([4.0, 6.0]);
        let out = g.add_point([14.0, 5.0]);
        g.add_edge(1, out as u32, EDGE_NORMAL);
        g.add_edge(out as u32, 2, EDGE_NORMAL);
        let t = set_mesh(&mut m, 0, &g).unwrap();
        let a = &m.art_meshes[0];
        assert_eq!(a.vertex_count(), 6);
        assert_eq!(t.indices, a.indices);
        assert!(positive(&a.positions, &a.indices));
        // The triangle (1, 5, 2) to the right is inside the new outline.
        assert!(tri_set(&a.indices).contains(&[1, 2, 5]));
        let f = &a.grid.keyforms[0].positions;
        // Unchanged vertices are copied exactly.
        assert_eq!(&f[..8], &before[..]);
        // New vertices follow the affine keyform, inside and outside.
        for (k, p) in [(4, [4.0f32, 6.0]), (5, [14.0, 5.0])] {
            assert!((f[2 * k] - (2.0 * p[0] + 1.0)).abs() < 1e-5);
            assert!((f[2 * k + 1] - (p[1] - 3.0)).abs() < 1e-5);
            assert!((a.uvs[2 * k] - p[0] / 20.0).abs() < 1e-6);
            assert!((a.uvs[2 * k + 1] - p[1] / 20.0).abs() < 1e-6);
        }
    }

    #[test]
    fn moving_points_keeps_triangles() {
        let mut m = Model::default();
        let mut a = quad_mesh();
        // A stored list that re-triangulation would change (one triangle).
        a.indices = vec![0, 1, 2];
        m.art_meshes.push(Arc::new(a));
        let mut g = MeshGraph::of(&m.art_meshes[0]);
        g.set_point(2, [11.0, 12.0]);
        set_mesh(&mut m, 0, &g).unwrap();
        assert_eq!(m.art_meshes[0].indices, vec![0, 1, 2]);
        assert_eq!(m.art_meshes[0].positions[4..6], [11.0, 12.0]);
        // An edge change re-triangulates (and fills).
        let mut g = MeshGraph::of(&m.art_meshes[0]);
        g.add_edge(1, 3, EDGE_NORMAL);
        g.mesh.edge_priority[4] = EDGE_AUTO_TRIANGULATION;
        set_mesh(&mut m, 0, &g).unwrap();
        assert_eq!(m.art_meshes[0].indices.len(), 6);
    }

    #[test]
    fn swap_divide_merge_mirror() {
        let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let mut g = graph(&pts, &[[0, 1], [1, 2], [2, 3], [3, 0]]);
        let t = g.triangulate().unwrap();
        // Swap whichever diagonal the triangulation chose.
        let diag = if tri_set(&t.indices).contains(&[0, 1, 2]) {
            (0, 2)
        } else {
            (1, 3)
        };
        g.swap_edge(diag.0, diag.1, &t.indices).unwrap();
        let t = g.triangulate().unwrap();
        let other = if diag == (0, 2) { (1, 3) } else { (0, 2) };
        let e = g.edge_index(other.0, other.1).unwrap();
        assert_eq!(g.mesh.edge_priority[e], EDGE_USER_TRIANGULATION);
        assert!(g.edge_index(diag.0, diag.1).is_none());
        // A boundary edge has one triangle: refused.
        assert!(g.swap_edge(0, 1, &t.indices).is_err());

        // 4 division of both triangles: 4 corners + 5 midpoints, 8 triangles.
        let all: HashSet<usize> = (0..4).collect();
        assert_eq!(g.divide4(&t.indices, &all), 2);
        assert_eq!(g.len(), 9);
        assert_eq!(g.triangulate().unwrap().indices.len() / 3, 8);

        // Merge: a point near 2 is merged into it at the centroid and its
        // edge is re-attached.
        let d = g.add_point([12.0, 10.0]);
        g.add_edge(d as u32, 0, EDGE_NORMAL);
        assert_eq!(g.merge_vertices(&[2, d]), Some(2));
        assert_eq!(g.len(), 9);
        assert_eq!(g.point(2), [11.0, 10.0]);
        assert!(g.edge_index(2, 0).is_some());
        g.set_point(2, [10.0, 10.0]);
        // Exact duplicates go on Finish; the first one stays.
        let e = g.add_point([10.0, 10.0]);
        g.add_edge(e as u32, 1, EDGE_NORMAL);
        assert_eq!(g.remove_coincident(), 1);
        assert_eq!(g.len(), 9);

        // Mirror partner across x = 5.
        assert_eq!(g.mirror_partner(0, 5.0, true, 0.5), Some(1));
        assert_eq!(g.mirror_partner(0, 5.0, false, 0.5), Some(3));
    }

    #[test]
    fn auto_connect_options() {
        // A square outline with a free point inside.
        let pts = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [4.0, 5.0],
        ];
        let mut g = graph(&pts, &[[0, 1], [1, 2], [2, 3], [3, 0]]);
        g.auto_connect(true, false).unwrap();
        assert!(g.mesh.edge_priority.iter().all(|&p| p == EDGE_NORMAL));
        // 4 sides + 4 spokes to the inner point.
        assert_eq!(g.mesh.edges.len(), 8);
        // Reconstruction keeps only the outline and rebuilds the inside.
        g.add_edge(0, 2, EDGE_NORMAL);
        g.auto_connect(true, true).unwrap();
        assert_eq!(g.mesh.edges.len(), 8);
        // Without an outline, protect refuses; unprotected, the hull is used.
        let mut open = graph(&pts, &[[0, 1]]);
        assert_eq!(open.auto_connect(true, false), Err(PROTECT_NEEDS_OUTLINE));
        open.auto_connect(false, false).unwrap();
        assert_eq!(open.triangulate().unwrap().indices.len() / 3, 4);
        // Reset to a rectangle.
        open.reset_to_rect([[0.0, 0.0], [5.0, 0.0], [5.0, 5.0], [0.0, 5.0]]);
        assert_eq!((open.len(), open.mesh.edges.len()), (4, 4));
        assert_eq!(open.triangulate().unwrap().indices.len() / 3, 2);
    }

    #[test]
    fn face_fans() {
        // A square split by a diagonal, traced and fanned (§3a).
        let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let edges = [[0, 1], [1, 2], [2, 3], [3, 0], [0, 2]];
        let g = graph(&pts, &edges);
        let t = triangulate(&g.points, &edges, &[EDGE_NORMAL; 5], false).unwrap();
        assert_eq!(tri_set(&t.indices), HashSet::from([[0, 1, 2], [0, 2, 3]]));
        // A pentagon is one fan of three triangles; the outer face is
        // dropped.
        let pts = [
            [0.0, 0.0],
            [10.0, 0.0],
            [12.0, 8.0],
            [5.0, 12.0],
            [-2.0, 8.0],
        ];
        let edges = [[0, 1], [1, 2], [2, 3], [3, 4], [4, 0]];
        let g = graph(&pts, &edges);
        let t = triangulate(&g.points, &edges, &[EDGE_NORMAL; 5], false).unwrap();
        assert_eq!(t.indices.len(), 9);
        // An open chain has no face.
        let t = triangulate(&g.points, &edges[..3], &[EDGE_NORMAL; 3], false).unwrap();
        assert!(t.indices.is_empty());
    }

    #[test]
    fn set_mesh_moves_glue_pairs_and_refuses_deleting_glued_points() {
        let mut m = Model::default();
        m.art_meshes.push(Arc::new(quad_mesh()));
        m.art_meshes.push(Arc::new(quad_mesh()));
        m.glues.push(Arc::new(keyform_model::Glue {
            info: ObjectInfo::new("Glue0"),
            mesh_a: Some(0),
            mesh_b: Some(1),
            pairs: vec![(Some(3), Some(1))],
            weights: vec![[0.5, 0.5]],
            grid: KeyformGrid::single(Default::default()),
            blend_shapes: Default::default(),
        }));
        let mut g = MeshGraph::of(&m.art_meshes[0]);
        g.delete_points(&HashSet::from([1]));
        set_mesh(&mut m, 0, &g).unwrap();
        assert_eq!(m.glues[0].pairs, vec![(Some(2), Some(1))]);
        let mut g = MeshGraph::of(&m.art_meshes[0]);
        g.delete_points(&HashSet::from([2]));
        assert!(matches!(set_mesh(&mut m, 0, &g), Err(MeshError::Glued(_))));
    }
}
