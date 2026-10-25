//! Mesh editing: the editable mesh graph, its re-triangulation, and keeping
//! keyforms and UVs when the topology changes.
//! spec: 30-algorithms/mesh-triangulation.md, 70-advanced/auto-mesh.md §4,
//! 10-formats/cmo3.md §4

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use keyform_model::{
    ArtMesh, EditableMesh, Model, EDGE_AUTO_TRIANGULATION, EDGE_NORMAL, POINT_NORMAL,
};
use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation as _};

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

    /// Re-triangulates the graph (spec: mesh-triangulation.md §3): the
    /// auto-triangulation edges are regenerated from the result.
    pub fn triangulate(&mut self) -> Result<Triangulated, MeshError> {
        let t = triangulate(&self.points, &self.mesh.edges, &self.mesh.edge_priority)?;
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

/// Constrained Delaunay triangulation of `points` honouring every edge
/// whose priority isn't auto-triangulation, keeping only the triangles
/// enclosed by those edges, oriented to positive area.
/// spec: 30-algorithms/mesh-triangulation.md §2–3
pub fn triangulate(
    points: &[f32],
    edges: &[[u32; 2]],
    priority: &[i8],
) -> Result<Triangulated, MeshError> {
    let n = points.len() / 2;
    let constraints: Vec<[usize; 2]> = edges
        .iter()
        .zip(priority.iter().chain(std::iter::repeat(&EDGE_NORMAL)))
        .filter(|&(e, &p)| {
            p != EDGE_AUTO_TRIANGULATION && (e[0] as usize) < n && (e[1] as usize) < n
        })
        .map(|(e, _)| [e[0] as usize, e[1] as usize])
        .collect();
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
    for c in &constraints {
        insert(&mut cdt, &mut handle, c[0])?;
        insert(&mut cdt, &mut handle, c[1])?;
    }
    for i in 0..n {
        insert(&mut cdt, &mut handle, i)?;
    }
    // Steps 3–4 (spade restores the Delaunay property itself).
    let mut unresolved = 0;
    for c in &constraints {
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
    // Step 8: drop the triangles outside every outline. TODO(Q-0015): some
    // sample meshes store fewer triangles than this keeps. A triangle is
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
    let keep_all = outside.iter().all(|&o| o);
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
    let t = g.triangulate()?;
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
        let t = triangulate(&graph(&pts, &edges).points, &edges, &[EDGE_NORMAL; 6]).unwrap();
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
        let t = triangulate(&g.points, &[], &[]).unwrap();
        assert_eq!(t.indices.len() / 3, 4);
        assert!(positive(&g.points, &t.indices));
    }

    #[test]
    fn crossing_constraints_are_reported() {
        let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let edges = [[0, 1], [1, 2], [2, 3], [3, 0], [0, 2], [1, 3]];
        let g = graph(&pts, &edges);
        let t = triangulate(&g.points, &edges, &[EDGE_LOCKED; 6]).unwrap();
        assert_eq!(t.unresolved, 1);
        assert_eq!(t.indices.len(), 6);
    }

    #[test]
    fn invalid_points_fail() {
        let edges = [[0, 1]];
        let r = triangulate(&[0.0, 0.0, f32::NAN, 1.0], &edges, &[EDGE_NORMAL]);
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
            grid: KeyformGrid::single(form),
            blend_shapes: Default::default(),
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
