//! Art-path editing: Stroke ArtPath (freehand stroke fitted to control
//! points), point insert / delete / move / fold, Split, Join, the Adjust
//! Width / Adjust Opacity brushes, settings and deletion.
//! spec: 70-advanced/art-path.md §1, §5; 50-ui/canvas-and-tools.md §4
//!
//! Positions are given in canvas coordinates and converted to the art
//! path's target space. Topology changes (insert, delete, split, join)
//! apply to every keyform; positions, widths, opacities and colours to the
//! keyform at the current parameter values.

use std::sync::Arc;

use keyform_eval::deform::DeformerMap;
use keyform_eval::Frame;
use keyform_model::{
    new_guid, ArtPath, ArtPathForm, ArtPathPoint, BlendShapes, Color, KeyformGrid, LineAlgorithm,
    LineAlignment, Model, ObjectInfo, PartChild, PartIndex,
};

pub type V2 = [f32; 2];

/// Brush defaults for new art paths (Keyform's: the reference's default
/// brush setting is per model and absent from new models).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Defaults {
    pub width: f32,
    pub opacity: f32,
    pub color: Color,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            width: 10.0,
            opacity: 1.0,
            color: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

fn dist(a: V2, b: V2) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Resamples a freehand stroke every `interval` px along its length (the
/// Stroke tool's Interval), then keeps the points needed to stay within
/// `tolerance` px of the stroke (Ramer–Douglas–Peucker). Ends are kept.
pub fn fit_stroke(stroke: &[V2], interval: f32, tolerance: f32) -> Vec<V2> {
    let mut pts: Vec<V2> = Vec::new();
    for &p in stroke {
        if pts.last().is_none_or(|q| dist(*q, p) > 1e-3) {
            pts.push(p);
        }
    }
    if pts.len() < 2 {
        return pts;
    }
    let step = interval.max(0.5);
    let mut res = vec![pts[0]];
    let mut carry = 0.0f32;
    for w in pts.windows(2) {
        let l = dist(w[0], w[1]);
        let mut t = step - carry;
        while t <= l {
            let u = t / l;
            res.push([
                w[0][0] + (w[1][0] - w[0][0]) * u,
                w[0][1] + (w[1][1] - w[0][1]) * u,
            ]);
            t += step;
        }
        carry = (carry + l) % step;
    }
    if res
        .last()
        .is_some_and(|q| dist(*q, pts[pts.len() - 1]) > step * 0.25)
    {
        res.push(pts[pts.len() - 1]);
    } else if let Some(q) = res.last_mut() {
        *q = pts[pts.len() - 1];
    }
    let mut keep = vec![false; res.len()];
    keep[0] = true;
    keep[res.len() - 1] = true;
    rdp(&res, 0, res.len() - 1, tolerance.max(0.01), &mut keep);
    res.into_iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(p, _)| p)
        .collect()
}

fn rdp(p: &[V2], a: usize, b: usize, tol: f32, keep: &mut [bool]) {
    if b <= a + 1 {
        return;
    }
    let (pa, pb) = (p[a], p[b]);
    let l = dist(pa, pb).max(1e-6);
    let (mut best, mut at) = (0.0f32, a);
    for (i, q) in p.iter().enumerate().take(b).skip(a + 1) {
        let d = ((pb[0] - pa[0]) * (pa[1] - q[1]) - (pa[0] - q[0]) * (pb[1] - pa[1])).abs() / l;
        if d > best {
            best = d;
            at = i;
        }
    }
    if best > tol {
        keep[at] = true;
        rdp(p, a, at, tol, keep);
        rdp(p, at, b, tol, keep);
    }
}

/// The target deformer's map of art path `k` (None = canvas space).
fn map_of(m: &Model, frame: &Frame, k: usize) -> Result<Option<DeformerMap>, String> {
    let a = m.art_paths.get(k).ok_or("no such art path")?;
    match a.target {
        None => Ok(None),
        Some(d) => frame
            .deformers
            .get(d)
            .and_then(|s| s.map.clone())
            .map(Some)
            .ok_or_else(|| "the art path's deformer could not be evaluated".to_owned()),
    }
}

fn to_local(map: Option<&DeformerMap>, p: V2) -> Result<V2, String> {
    match map {
        None => Ok(p),
        Some(m) => m
            .inverse(p)
            .ok_or_else(|| "the point is outside the art path's deformer".to_owned()),
    }
}

/// Canvas positions of art path `k`'s control points at the current values.
pub fn canvas_points(m: &Model, frame: &Frame, values: &[f32], k: usize) -> Vec<V2> {
    let Some(a) = m.art_paths.get(k) else {
        return Vec::new();
    };
    let Some(r) =
        keyform_eval::interp::interpolate(&m.parameters, values, &a.grid, &a.blend_shapes)
    else {
        return Vec::new();
    };
    let map = map_of(m, frame, k).ok().flatten();
    r.form
        .points
        .iter()
        .map(|p| map.as_ref().map_or(p.position, |mp| mp.map(p.position)))
        .collect()
}

/// Creates an art path through the canvas points `pts` (≥ 2), in `part`
/// (else the root part), on the root deformer, with one keyform.
/// Returns its index.
pub fn create(
    m: &mut Model,
    part: Option<PartIndex>,
    pts: &[V2],
    d: Defaults,
) -> Result<usize, String> {
    if pts.len() < 2 {
        return Err("An ArtPath needs at least 2 points.".into());
    }
    let id = crate::objects::unique_id("ArtPath", m.art_paths.iter().map(|a| a.info.id.as_str()));
    let part = part.or(m.root_part).filter(|&p| p < m.parts.len());
    let mut info = ObjectInfo::new(&id);
    info.guid = new_guid();
    info.name = id.clone();
    info.parent_part = part;
    let form = ArtPathForm {
        points: pts
            .iter()
            .map(|&p| ArtPathPoint {
                position: p,
                width: d.width,
                opacity: d.opacity,
                color: d.color,
                corner: false,
            })
            .collect(),
        draw_order: 500.0,
        opacity: 1.0,
        multiply: [1.0; 4],
        screen: [0.0, 0.0, 0.0, 1.0],
    };
    let mut grid = KeyformGrid::single(form);
    grid.guids = vec![new_guid()];
    m.art_paths.push(Arc::new(ArtPath {
        info,
        target: None,
        grid,
        blend_shapes: BlendShapes::default(),
        div_num: 20,
        algorithm: LineAlgorithm::Lcns,
        alignment: LineAlignment::Center,
        closed: false,
        clip: Vec::new(),
        invert_clip: false,
        // No brush: SIMPLE without texture (stored as the null GUID).
        brush: "00000000-0000-0000-0000-000000000000".into(),
        user_data: String::new(),
    }));
    let k = m.art_paths.len() - 1;
    if let Some(p) = part {
        Arc::make_mut(&mut m.parts[p])
            .children
            .insert(0, PartChild::ArtPath(k));
    }
    Ok(k)
}

/// The cell of art path `k`'s keyform at `values`.
pub fn current_cell(m: &Model, k: usize, values: &[f32]) -> Result<usize, String> {
    let a = m.art_paths.get(k).ok_or("no such art path")?;
    crate::forms::cell_at(&a.grid.bindings, &m.parameters, values)
        .map_err(|_| "There is no keyform to edit at the current parameter values.".to_owned())
}

fn path_mut(m: &mut Model, k: usize) -> Result<&mut ArtPath, String> {
    m.art_paths
        .get_mut(k)
        .map(Arc::make_mut)
        .ok_or_else(|| "no such art path".to_owned())
}

/// Moves point `i` of art path `k` to canvas position `to` in the current
/// keyform.
pub fn move_point(
    m: &mut Model,
    frame: &Frame,
    values: &[f32],
    k: usize,
    i: usize,
    to: V2,
) -> Result<(), String> {
    let cell = current_cell(m, k, values)?;
    let local = to_local(map_of(m, frame, k)?.as_ref(), to)?;
    let a = path_mut(m, k)?;
    let p = a
        .grid
        .keyforms
        .get_mut(cell)
        .and_then(|f| f.points.get_mut(i))
        .ok_or("no such point")?;
    p.position = local;
    Ok(())
}

/// Inserts a point on segment `seg` (between `seg` and `seg + 1`, or after
/// the last point of a closed path) at canvas position `at`; every keyform
/// gets the point at the same fraction along its segment. Returns its index.
pub fn insert_point(
    m: &mut Model,
    frame: &Frame,
    values: &[f32],
    k: usize,
    seg: usize,
    at: V2,
) -> Result<usize, String> {
    let cell = current_cell(m, k, values)?;
    let local = to_local(map_of(m, frame, k)?.as_ref(), at)?;
    let a = path_mut(m, k)?;
    let n = a.grid.keyforms.first().map_or(0, |f| f.points.len());
    let next = if seg + 1 < n {
        seg + 1
    } else if a.closed {
        0
    } else {
        return Err("no such segment".into());
    };
    // Fraction along the segment in the edited keyform.
    let f = &a.grid.keyforms[cell];
    let (p0, p1) = (f.points[seg].position, f.points[next].position);
    let l = dist(p0, p1).max(1e-6);
    let t = (((local[0] - p0[0]) * (p1[0] - p0[0]) + (local[1] - p0[1]) * (p1[1] - p0[1]))
        / (l * l))
        .clamp(0.0, 1.0);
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    for (c, form) in a.grid.keyforms.iter_mut().enumerate() {
        let (q0, q1) = (form.points[seg].clone(), form.points[next].clone());
        let q = ArtPathPoint {
            position: if c == cell {
                local
            } else {
                [
                    lerp(q0.position[0], q1.position[0]),
                    lerp(q0.position[1], q1.position[1]),
                ]
            },
            width: lerp(q0.width, q1.width),
            opacity: lerp(q0.opacity, q1.opacity),
            color: std::array::from_fn(|j| lerp(q0.color[j], q1.color[j])),
            corner: false,
        };
        form.points.insert(seg + 1, q);
    }
    for s in &mut a.blend_shapes.shapes {
        let (q0, q1) = (
            s.form.points.get(seg).cloned(),
            s.form.points.get(next).cloned(),
        );
        if let (Some(q0), Some(q1)) = (q0, q1) {
            // Blend-shape forms are deltas: interpolate them too.
            let mut q = q0.clone();
            q.position = [
                lerp(q0.position[0], q1.position[0]),
                lerp(q0.position[1], q1.position[1]),
            ];
            q.width = lerp(q0.width, q1.width);
            s.form.points.insert(seg + 1, q);
        }
    }
    Ok(seg + 1)
}

/// Appends a point after the last one (extending an open path).
pub fn append_point(
    m: &mut Model,
    frame: &Frame,
    values: &[f32],
    k: usize,
    at: V2,
) -> Result<usize, String> {
    let cell = current_cell(m, k, values)?;
    let local = to_local(map_of(m, frame, k)?.as_ref(), at)?;
    let a = path_mut(m, k)?;
    for (c, form) in a.grid.keyforms.iter_mut().enumerate() {
        let Some(last) = form.points.last().cloned() else {
            continue;
        };
        let mut q = last.clone();
        q.corner = false;
        if c == cell {
            q.position = local;
        } else if form.points.len() >= 2 {
            // Same offset from the last point as in the edited keyform.
            let prev = form.points[form.points.len() - 2].position;
            q.position = [
                2.0 * last.position[0] - prev[0],
                2.0 * last.position[1] - prev[1],
            ];
        }
        form.points.push(q);
    }
    for s in &mut a.blend_shapes.shapes {
        if let Some(last) = s.form.points.last().cloned() {
            s.form.points.push(last);
        }
    }
    Ok(a.grid.keyforms.first().map_or(0, |f| f.points.len() - 1))
}

/// Deletes point `i` (a path keeps at least 2 points).
pub fn delete_point(m: &mut Model, k: usize, i: usize) -> Result<(), String> {
    let a = path_mut(m, k)?;
    let n = a.grid.keyforms.first().map_or(0, |f| f.points.len());
    if n <= 2 {
        return Err("An ArtPath needs at least 2 points.".into());
    }
    if i >= n {
        return Err("no such point".into());
    }
    for f in &mut a.grid.keyforms {
        f.points.remove(i);
    }
    for s in &mut a.blend_shapes.shapes {
        if i < s.form.points.len() {
            s.form.points.remove(i);
        }
    }
    Ok(())
}

/// Toggles the fold (corner) flag of point `i` in every keyform.
pub fn toggle_corner(m: &mut Model, k: usize, i: usize) -> Result<bool, String> {
    let a = path_mut(m, k)?;
    let now = !a
        .grid
        .keyforms
        .first()
        .and_then(|f| f.points.get(i))
        .ok_or("no such point")?
        .corner;
    for f in &mut a.grid.keyforms {
        if let Some(p) = f.points.get_mut(i) {
            p.corner = now;
        }
    }
    Ok(now)
}

/// Splits art path `k` at point `i` into two paths sharing that point (a
/// closed path opens there instead). Returns the new path's index.
pub fn split(m: &mut Model, k: usize, i: usize) -> Result<Option<usize>, String> {
    let a = m.art_paths.get(k).ok_or("no such art path")?.clone();
    let n = a.grid.keyforms.first().map_or(0, |f| f.points.len());
    if i >= n {
        return Err("no such point".into());
    }
    if a.closed {
        let p = path_mut(m, k)?;
        p.closed = false;
        for f in &mut p.grid.keyforms {
            f.points.rotate_left(i);
            let first = f.points[0].clone();
            f.points.push(first);
        }
        p.blend_shapes = BlendShapes::default();
        return Ok(None);
    }
    if i == 0 || i + 1 >= n {
        return Err("Split at an inner point.".into());
    }
    let mut b = (*a).clone();
    let id = crate::objects::unique_id("ArtPath", m.art_paths.iter().map(|x| x.info.id.as_str()));
    b.info.id = id.clone();
    b.info.name = id;
    b.info.guid = new_guid();
    b.grid.guids = b.grid.guids.iter().map(|_| new_guid()).collect();
    b.blend_shapes = BlendShapes::default();
    for f in &mut b.grid.keyforms {
        f.points.drain(..i);
    }
    {
        let p = path_mut(m, k)?;
        for f in &mut p.grid.keyforms {
            f.points.truncate(i + 1);
        }
        p.blend_shapes = BlendShapes::default();
    }
    m.art_paths.push(Arc::new(b));
    let nk = m.art_paths.len() - 1;
    if let Some(part) = m.art_paths[nk]
        .info
        .parent_part
        .filter(|&p| p < m.parts.len())
    {
        let pm = Arc::make_mut(&mut m.parts[part]);
        let at = pm
            .children
            .iter()
            .position(|c| *c == PartChild::ArtPath(k))
            .unwrap_or(0);
        pm.children.insert(at, PartChild::ArtPath(nk));
    }
    Ok(Some(nk))
}

/// Joins the end of art path `a` to the start of art path `b` (the
/// endpoints chosen by `a_end` / `b_start`; paths reversed as needed).
/// Both must be open and have the same keyform grid. `b` is deleted.
pub fn join(m: &mut Model, a: usize, a_end: bool, b: usize, b_start: bool) -> Result<(), String> {
    if a == b {
        return Err("Select another ArtPath to join.".into());
    }
    let (pa, pb) = (
        m.art_paths.get(a).ok_or("no such art path")?,
        m.art_paths.get(b).ok_or("no such art path")?,
    );
    if pa.closed || pb.closed {
        return Err("Closed ArtPaths can't be joined.".into());
    }
    if pa.grid.bindings != pb.grid.bindings {
        return Err("The ArtPaths have different keyforms.".into());
    }
    let bf = pb.grid.keyforms.clone();
    {
        let p = path_mut(m, a)?;
        for (f, g) in p.grid.keyforms.iter_mut().zip(bf) {
            if !a_end {
                f.points.reverse();
            }
            let mut g = g.points;
            if !b_start {
                g.reverse();
            }
            f.points.extend(g);
        }
        p.blend_shapes = BlendShapes::default();
    }
    delete(m, b)
}

/// Deletes art path `k`.
pub fn delete(m: &mut Model, k: usize) -> Result<(), String> {
    if k >= m.art_paths.len() {
        return Err("no such art path".into());
    }
    m.art_paths.remove(k);
    for p in &mut m.parts {
        if p.children
            .iter()
            .any(|c| matches!(c, PartChild::ArtPath(i) if *i >= k))
        {
            let p = Arc::make_mut(p);
            p.children.retain(|c| *c != PartChild::ArtPath(k));
            for c in &mut p.children {
                if let PartChild::ArtPath(i) = c {
                    if *i > k {
                        *i -= 1;
                    }
                }
            }
        }
    }
    for x in &mut m.aliases {
        let r = match x.reference {
            keyform_model::AliasRef::ArtPath(i) if i == k => keyform_model::AliasRef::Missing,
            keyform_model::AliasRef::ArtPath(i) if i > k => keyform_model::AliasRef::ArtPath(i - 1),
            r => r,
        };
        if r != x.reference {
            Arc::make_mut(x).reference = r;
        }
    }
    Ok(())
}

/// What a brush stroke changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushKind {
    /// Line width toward `level` px.
    Width,
    /// Point opacity toward `level` %.
    Opacity,
}

/// Adjust Width / Adjust Opacity brush: points of the current keyform within
/// `size` px of canvas position `at` move toward `level` by `strength`
/// (0–1) times a smooth falloff. Returns how many points changed.
#[allow(clippy::too_many_arguments)]
pub fn brush(
    m: &mut Model,
    frame: &Frame,
    values: &[f32],
    k: usize,
    at: V2,
    size: f32,
    level: f32,
    strength: f32,
    kind: BrushKind,
) -> Result<usize, String> {
    let cell = current_cell(m, k, values)?;
    let pts = canvas_points(m, frame, values, k);
    let a = path_mut(m, k)?;
    let f = a.grid.keyforms.get_mut(cell).ok_or("no keyform")?;
    let mut n = 0;
    for (p, c) in f.points.iter_mut().zip(pts) {
        let d = dist(c, at);
        if d > size {
            continue;
        }
        let u = 1.0 - d / size.max(1e-6);
        let w = (u * u * (3.0 - 2.0 * u) * strength).clamp(0.0, 1.0);
        match kind {
            BrushKind::Width => p.width += (level.max(0.0) - p.width) * w,
            BrushKind::Opacity => p.opacity += ((level / 100.0).clamp(0.0, 1.0) - p.opacity) * w,
        }
        n += 1;
    }
    Ok(n)
}

/// Sets width / opacity / colour of point `i` in the current keyform
/// (inspector "Line Width", "Opacity", "Line Color").
pub fn set_point_style(
    m: &mut Model,
    values: &[f32],
    k: usize,
    i: usize,
    width: Option<f32>,
    opacity: Option<f32>,
    color: Option<Color>,
) -> Result<(), String> {
    let cell = current_cell(m, k, values)?;
    let a = path_mut(m, k)?;
    let p = a
        .grid
        .keyforms
        .get_mut(cell)
        .and_then(|f| f.points.get_mut(i))
        .ok_or("no such point")?;
    if let Some(w) = width {
        p.width = w.max(0.0);
    }
    if let Some(o) = opacity {
        p.opacity = o.clamp(0.0, 1.0);
    }
    if let Some(c) = color {
        p.color = c.map(|x| x.clamp(0.0, 1.0));
    }
    Ok(())
}

/// The art path settings of the inspector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub div_num: u32,
    pub closed: bool,
    pub algorithm: LineAlgorithm,
    pub alignment: LineAlignment,
}

pub fn settings(a: &ArtPath) -> Settings {
    Settings {
        div_num: a.div_num,
        closed: a.closed,
        algorithm: a.algorithm,
        alignment: a.alignment,
    }
}

pub fn set_settings(m: &mut Model, k: usize, s: Settings) -> Result<(), String> {
    let a = path_mut(m, k)?;
    a.div_num = s.div_num.clamp(1, 200);
    a.closed = s.closed && a.grid.keyforms.first().is_some_and(|f| f.points.len() >= 3);
    a.algorithm = s.algorithm;
    a.alignment = s.alignment;
    Ok(())
}

/// Nearest control point of art path `k` within `range` canvas px.
pub fn pick_point(pts: &[V2], at: V2, range: f32) -> Option<usize> {
    pts.iter()
        .enumerate()
        .map(|(i, p)| (i, dist(*p, at)))
        .filter(|(_, d)| *d <= range)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Nearest segment (between control points) within `range` canvas px.
pub fn pick_segment(pts: &[V2], closed: bool, at: V2, range: f32) -> Option<usize> {
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    (0..segs)
        .map(|s| {
            let (a, b) = (pts[s], pts[(s + 1) % n]);
            let l2 = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).max(1e-9);
            let t = (((at[0] - a[0]) * (b[0] - a[0]) + (at[1] - a[1]) * (b[1] - a[1])) / l2)
                .clamp(0.0, 1.0);
            (
                s,
                dist(at, [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]),
            )
        })
        .filter(|(_, d)| *d <= range)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, _)| s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::Part;

    fn model() -> Model {
        let mut m = Model::default();
        m.parts.push(Arc::new(Part::new("Root")));
        m.root_part = Some(0);
        m
    }

    #[test]
    fn stroke_fit_keeps_corners_and_ends() {
        // An L-shaped stroke sampled densely: the corner and the ends stay.
        let mut s = Vec::new();
        for i in 0..=100 {
            s.push([i as f32, 0.0]);
        }
        for i in 1..=100 {
            s.push([100.0, i as f32]);
        }
        let f = fit_stroke(&s, 5.0, 1.0);
        assert_eq!(f.first(), Some(&[0.0, 0.0]));
        assert_eq!(f.last(), Some(&[100.0, 100.0]));
        assert!(f.iter().any(|p| dist(*p, [100.0, 0.0]) < 5.0));
        assert!(f.len() <= 5, "{f:?}");
    }

    #[test]
    fn edit_operations() {
        let mut m = model();
        let k = create(
            &mut m,
            None,
            &[[0.0, 0.0], [100.0, 0.0], [200.0, 0.0]],
            Defaults::default(),
        )
        .unwrap();
        assert_eq!(m.parts[0].children, vec![PartChild::ArtPath(0)]);
        let frame = keyform_eval::Evaluator::new(&m).evaluate(&[]);
        let i = insert_point(&mut m, &frame, &[], k, 0, [50.0, 10.0]).unwrap();
        assert_eq!(i, 1);
        assert_eq!(
            m.art_paths[k].grid.keyforms[0].points[1].position,
            [50.0, 10.0]
        );
        move_point(&mut m, &frame, &[], k, 1, [50.0, 20.0]).unwrap();
        assert!(toggle_corner(&mut m, k, 1).unwrap());
        brush(
            &mut m,
            &frame,
            &[],
            k,
            [0.0, 0.0],
            30.0,
            2.0,
            1.0,
            BrushKind::Width,
        )
        .unwrap();
        assert!((m.art_paths[k].grid.keyforms[0].points[0].width - 2.0).abs() < 1e-4);
        let nk = split(&mut m, k, 2).unwrap().unwrap();
        assert_eq!(m.art_paths[k].grid.keyforms[0].points.len(), 3);
        assert_eq!(m.art_paths[nk].grid.keyforms[0].points.len(), 2);
        assert_eq!(m.parts[0].children.len(), 2);
        join(&mut m, k, true, nk, true).unwrap();
        assert_eq!(m.art_paths.len(), 1);
        assert_eq!(m.art_paths[0].grid.keyforms[0].points.len(), 5);
        delete_point(&mut m, 0, 4).unwrap();
        delete(&mut m, 0).unwrap();
        assert!(m.art_paths.is_empty() && m.parts[0].children.is_empty());
    }
}
