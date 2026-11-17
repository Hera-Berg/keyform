//! Deform paths: anchoring, curves, binding and the drag deformation.
//! spec: 35-editing/deform-paths.md §1–4, 30-algorithms/sns-spline.md

use keyform_model::{ControlCurve, DeformPath, Effect, Effector, PointInTriangle, TargetPoint};

pub type V2 = [f32; 2];

fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: V2, s: f32) -> V2 {
    [a[0] * s, a[1] * s]
}
fn len(a: V2) -> f32 {
    a[0].hypot(a[1])
}
fn dist(a: V2, b: V2) -> f32 {
    len(sub(a, b))
}
fn normalize(a: V2) -> V2 {
    let l = len(a);
    if l > 0.0 {
        mul(a, 1.0 / l)
    } else {
        [0.0, 0.0]
    }
}

// ------------------------------------------------------------- anchoring

/// The control point's position: Σ wᵢ·vertexᵢ over its anchor triangle in
/// `positions` (x,y interleaved). spec: §1
pub fn anchored(a: &PointInTriangle, positions: &[f32]) -> Option<V2> {
    let mut p = [0.0f32; 2];
    for k in 0..3 {
        let i = usize::try_from(a.index[k]).ok()?;
        let v = positions.get(2 * i..2 * i + 2)?;
        p[0] += a.weight[k] * v[0];
        p[1] += a.weight[k] * v[1];
    }
    Some(p)
}

fn barycentric(p: V2, a: V2, b: V2, c: V2) -> Option<[f32; 3]> {
    let d = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if d.abs() < 1e-12 {
        return None;
    }
    let w1 = ((b[0] - p[0]) * (c[1] - p[1]) - (b[1] - p[1]) * (c[0] - p[0])) / d;
    let w2 = ((c[0] - p[0]) * (a[1] - p[1]) - (c[1] - p[1]) * (a[0] - p[0])) / d;
    Some([w1, w2, 1.0 - w1 - w2])
}

/// Anchors canvas position `p` in a mesh (`positions`, `indices`): the
/// first triangle containing it, else the one with the smallest Σ|wᵢ|; a
/// degenerate triangle gets weights (1, 0, 0). spec: §1 "Anchoring"
pub fn anchor(positions: &[f32], indices: &[u32], p: V2) -> PointInTriangle {
    let pt = |i: u32| -> V2 {
        let i = i as usize;
        [positions[2 * i], positions[2 * i + 1]]
    };
    let mut best: Option<(f32, [u32; 3], [f32; 3])> = None;
    for t in indices.chunks_exact(3) {
        if t.iter().any(|&i| 2 * i as usize + 1 >= positions.len()) {
            continue;
        }
        let tri = [t[0], t[1], t[2]];
        let Some(w) = barycentric(p, pt(t[0]), pt(t[1]), pt(t[2])) else {
            if best.is_none() {
                best = Some((f32::MAX, tri, [1.0, 0.0, 0.0]));
            }
            continue;
        };
        if w.iter().all(|&x| x >= -1e-6) {
            return PointInTriangle {
                index: tri.map(|i| i as i32),
                weight: w,
            };
        }
        let s = w.iter().map(|x| x.abs()).sum::<f32>();
        if best.is_none_or(|(b, _, _)| s < b) {
            best = Some((s, tri, w));
        }
    }
    match best {
        Some((_, tri, w)) => PointInTriangle {
            index: tri.map(|i| i as i32),
            weight: w,
        },
        None => PointInTriangle {
            index: [0, 0, 0],
            weight: [1.0, 0.0, 0.0],
        },
    }
}

// ----------------------------------------------------------- SNS curves

/// Velocities and chord lengths of an open SNS spline (sns-spline.md §1).
fn open_velocities(p: &[V2]) -> (Vec<V2>, Vec<f32>) {
    let n = p.len();
    let d: Vec<f32> = (0..n.saturating_sub(1))
        .map(|i| dist(p[i + 1], p[i]))
        .collect();
    let mut v = vec![[0.0f32; 2]; n];
    if n < 2 {
        return (v, d);
    }
    let start = |v: &[V2], i: usize| -> V2 {
        if d[i] <= 0.0 {
            return [0.0, 0.0];
        }
        mul(sub(mul(sub(p[i + 1], p[i]), 3.0 / d[i]), v[i + 1]), 0.5)
    };
    let end = |v: &[V2], i: usize| -> V2 {
        if d[i - 1] <= 0.0 {
            return [0.0, 0.0];
        }
        mul(sub(mul(sub(p[i], p[i - 1]), 3.0 / d[i - 1]), v[i - 1]), 0.5)
    };
    for i in 1..n - 1 {
        v[i] = normalize(sub(p[i + 1], p[i - 1]));
    }
    v[0] = start(&v, 0);
    v[n - 1] = end(&v, n - 1);
    for _ in 0..3 {
        let mut carry = start(&v, 0);
        let mut new = carry;
        for i in 1..n - 1 {
            let s = d[i - 1] + d[i];
            if s > 0.0 {
                new = mul(
                    add(mul(start(&v, i), d[i - 1]), mul(end(&v, i), d[i])),
                    1.0 / s,
                );
            }
            v[i - 1] = carry;
            carry = new;
        }
        v[n - 1] = end(&v, n - 1);
        v[n - 2] = carry;
    }
    (v, d)
}

/// Velocities of a closed SNS spline (sns-spline.md §2): indices modulo n,
/// the averaged form for every node.
fn closed_velocities(p: &[V2]) -> (Vec<V2>, Vec<f32>) {
    let n = p.len();
    let d: Vec<f32> = (0..n).map(|i| dist(p[(i + 1) % n], p[i])).collect();
    let mut v: Vec<V2> = (0..n)
        .map(|i| normalize(sub(p[(i + 1) % n], p[(i + n - 1) % n])))
        .collect();
    for _ in 0..3 {
        for i in 0..n {
            let (dp, dn) = (d[(i + n - 1) % n], d[i]);
            let st = if dn > 0.0 {
                mul(
                    sub(mul(sub(p[(i + 1) % n], p[i]), 3.0 / dn), v[(i + 1) % n]),
                    0.5,
                )
            } else {
                [0.0, 0.0]
            };
            let en = if dp > 0.0 {
                mul(
                    sub(
                        mul(sub(p[i], p[(i + n - 1) % n]), 3.0 / dp),
                        v[(i + n - 1) % n],
                    ),
                    0.5,
                )
            } else {
                [0.0, 0.0]
            };
            if dp + dn > 0.0 {
                v[i] = mul(add(mul(st, dp), mul(en, dn)), 1.0 / (dp + dn));
            }
        }
    }
    (v, d)
}

fn hermite(p0: V2, p1: V2, v0: V2, v1: V2, d: f32, t: f32) -> V2 {
    let (t2, t3) = (t * t, t * t * t);
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h10 = t3 - 2.0 * t2 + t;
    let h11 = t3 - t2;
    let (a, b) = (mul(v0, d), mul(v1, d));
    [
        h00 * p0[0] + h01 * p1[0] + h10 * a[0] + h11 * b[0],
        h00 * p0[1] + h01 * p1[1] + h10 * a[1] + h11 * b[1],
    ]
}

/// A curve through `pts` with `corners` flags, open or closed, evaluated
/// at curve parameter `t` (one unit per segment). spec: sns-spline.md §1–3
#[derive(Clone, Debug)]
pub struct Curve {
    pts: Vec<V2>,
    open: bool,
    /// Per segment: its sub-curve (points, velocities, chords, local index).
    segs: Vec<(usize, usize)>,
    subs: Vec<(Vec<V2>, Vec<V2>, Vec<f32>)>,
}

impl Curve {
    pub fn new(pts: Vec<V2>, corners: &[bool], open: bool) -> Self {
        let n = pts.len();
        let mut c = Curve {
            pts: pts.clone(),
            open,
            segs: Vec::new(),
            subs: Vec::new(),
        };
        if n < 2 {
            return c;
        }
        let corner = |i: usize| corners.get(i).copied().unwrap_or(false);
        let nseg = if open { n - 1 } else { n };
        let any_corner = (0..n).any(|i| corner(i) && (!open || (i > 0 && i + 1 < n)));
        if !open && !any_corner {
            let (v, d) = closed_velocities(&pts);
            let mut sp = pts.clone();
            sp.push(pts[0]);
            let mut sv = v.clone();
            sv.push(v[0]);
            c.subs.push((sp, sv, d));
            c.segs = (0..n).map(|i| (0, i)).collect();
            return c;
        }
        // Split into open sub-curves at corners (closed: start at the first
        // corner and wrap around).
        let start = if open {
            0
        } else {
            (0..n).find(|&i| corner(i)).unwrap_or(0)
        };
        let order: Vec<usize> = if open {
            (0..n).collect()
        } else {
            (0..=n).map(|k| (start + k) % n).collect()
        };
        let mut cur: Vec<usize> = vec![order[0]];
        let mut seg_of_sub: Vec<(usize, usize, usize)> = Vec::new(); // (global seg, sub, local)
        let mut subs_idx: Vec<Vec<usize>> = Vec::new();
        for w in 1..order.len() {
            cur.push(order[w]);
            let at_end = w + 1 == order.len();
            if corner(order[w]) || at_end {
                subs_idx.push(std::mem::replace(&mut cur, vec![order[w]]));
            }
        }
        for (si, ids) in subs_idx.iter().enumerate() {
            for l in 0..ids.len() - 1 {
                let g = ids[l]; // segment starting at this point
                seg_of_sub.push((g, si, l));
            }
            let sp: Vec<V2> = ids.iter().map(|&i| pts[i]).collect();
            // 2-point sub-curves too follow sns-spline.md §1 (no special
            // case, Q-0026).
            let (v, d) = open_velocities(&sp);
            c.subs.push((sp, v, d));
        }
        c.segs = vec![(0, 0); nseg];
        for (g, si, l) in seg_of_sub {
            if g < nseg {
                c.segs[g] = (si, l);
            }
        }
        c
    }

    /// The number of segments (the curve parameter's range).
    pub fn segments(&self) -> usize {
        self.segs.len()
    }

    pub fn point(&self, t: f32) -> V2 {
        let n = self.segs.len();
        if n == 0 {
            return self.pts.first().copied().unwrap_or([0.0, 0.0]);
        }
        let t = t.clamp(0.0, n as f32);
        let mut i = t.floor() as usize;
        let mut f = t - i as f32;
        if i >= n {
            i = n - 1;
            f = 1.0;
        }
        let (si, l) = self.segs[i];
        let (sp, v, d) = &self.subs[si];
        if l + 1 >= sp.len() {
            return sp[sp.len() - 1];
        }
        hermite(sp[l], sp[l + 1], v[l], v[l + 1], d[l], f)
    }

    /// Unit tangent at `t`: towards the point at t + 0.05 (from t − 0.05 at
    /// the end). spec: §4
    pub fn tangent(&self, t: f32) -> V2 {
        let n = self.segs.len() as f32;
        let (a, b) = if t + 0.05 <= n || !self.open {
            (self.point(t), self.point((t + 0.05).min(n)))
        } else {
            (self.point(t - 0.05), self.point(t))
        };
        let d = normalize(sub(b, a));
        if d == [0.0, 0.0] && t > 0.0 {
            normalize(sub(self.point(t), self.point(t - 0.05)))
        } else {
            d
        }
    }

    /// Nearest point to `v`: 8 samples per segment, then ±1/8 in steps of
    /// 1/24. Returns (t, distance, position). spec: §3
    pub fn nearest(&self, v: V2) -> (f32, f32, V2) {
        let n = self.segs.len();
        let mut best = (0.0f32, f32::MAX, self.point(0.0));
        for k in 0..=(8 * n) {
            let t = k as f32 / 8.0;
            let p = self.point(t);
            let d = dist(p, v);
            if d < best.1 {
                best = (t, d, p);
            }
        }
        let t0 = best.0;
        for k in -3i32..=3 {
            let t = (t0 + k as f32 / 24.0).clamp(0.0, n as f32);
            let p = self.point(t);
            let d = dist(p, v);
            if d < best.1 {
                best = (t, d, p);
            }
        }
        best
    }
}

/// The curves of `path` through the control points at `pts`.
pub fn curves(path: &DeformPath, pts: &[V2]) -> Vec<Curve> {
    path.curves
        .iter()
        .map(|c: &ControlCurve| {
            let p: Vec<V2> = c
                .points
                .iter()
                .map(|&k| pts.get(k).copied().unwrap_or([0.0, 0.0]))
                .collect();
            let corners: Vec<bool> = c
                .points
                .iter()
                .map(|&k| path.points.get(k).is_some_and(|q| q.corner))
                .collect();
            Curve::new(p, &corners, c.open)
        })
        .collect()
}

// ---------------------------------------------------------------- binding

/// Direction correction (§3): effectors in the same direction as a nearer
/// one are suppressed; renormalised.
fn direction_correction(v: V2, cand: &[(V2, f32)], w: &mut [f32]) {
    if w.first().is_some_and(|&x| x >= 1.0 - 1e-6) {
        return;
    }
    let u: Vec<V2> = cand.iter().map(|(p, _)| sub(*p, v)).collect();
    let angle = |a: V2, b: V2| -> f32 {
        let (la, lb) = (len(a), len(b));
        if la <= 0.0 || lb <= 0.0 {
            return 0.0;
        }
        ((a[0] * b[0] + a[1] * b[1]) / (la * lb))
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    };
    for i in 1..w.len() {
        let imp: f32 = (0..i)
            .map(|j| (angle(u[i], u[j]) / 90.0).clamp(0.0, 1.0))
            .product();
        w[i] *= imp;
    }
    let s: f32 = w.iter().sum();
    if s > 0.0 {
        w.iter_mut().for_each(|x| *x /= s);
    }
}

/// The effects of a vertex at `v` (§3). `pts` are the control points'
/// positions, `curves` the path's curves through them.
pub fn bind_vertex(path: &DeformPath, pts: &[V2], cs: &[Curve], v: V2) -> Vec<Effect> {
    let line = path.bind_method.is_line();
    let max = path.max_bind.max(1) as usize;
    // (effector, its position, distance)
    let mut cand: Vec<(Effector, V2, f32)> = Vec::new();
    for (k, p) in pts.iter().enumerate() {
        if line && path.points[k].curve.is_some() {
            continue;
        }
        cand.push((Effector::Point(k), *p, dist(*p, v)));
    }
    if line {
        for (ci, c) in cs.iter().enumerate() {
            if c.segments() == 0 {
                continue;
            }
            let (t, d, pos) = c.nearest(v);
            cand.push((
                Effector::Curve {
                    curve: ci,
                    t,
                    distance: d,
                    pos,
                },
                pos,
                d,
            ));
        }
    }
    cand.sort_by(|a, b| a.2.total_cmp(&b.2));
    cand.truncate(if line { 2 * max } else { max });
    if cand.is_empty() {
        return Vec::new();
    }
    if cand[0].2 < 1e-6 {
        return vec![Effect {
            effector: cand.swap_remove(0).0,
            weight: 1.0,
        }];
    }
    let inv: Vec<f32> = cand.iter().map(|c| 1.0 / c.2).collect();
    let s: f32 = inv.iter().sum();
    let mut w: Vec<f32> = inv.iter().map(|x| x / s).collect();
    if path.bind_method.with_direction() {
        let cd: Vec<(V2, f32)> = cand.iter().map(|c| (c.1, c.2)).collect();
        direction_correction(v, &cd, &mut w);
    }
    let mut out: Vec<Effect> = cand
        .into_iter()
        .zip(w)
        .map(|(c, weight)| Effect {
            effector: c.0,
            weight,
        })
        .collect();
    if line {
        out.retain(|e| e.weight >= 0.001);
    }
    out
}

/// A vertex to bind: (mesh GUID, vertex UID, index, keyform GUID, canvas
/// position).
pub type BindVertex = (String, i64, usize, String, V2);

/// Binds every target vertex. Control points use `pts`. Stores positions at bind
/// (pos_default). spec: §2–3
pub fn bind(path: &mut DeformPath, pts: &[V2], verts: &[BindVertex]) {
    let cs = curves(path, pts);
    path.targets = verts
        .iter()
        .map(|(mesh, uid, index, form, v)| TargetPoint {
            mesh: mesh.clone(),
            uid: *uid,
            index: *index,
            form: form.clone(),
            effects: bind_vertex(path, pts, &cs, *v),
        })
        .collect();
    for (k, p) in pts.iter().enumerate() {
        if let Some(c) = path.points.get_mut(k) {
            c.pos_default = *p;
        }
    }
}

// ---------------------------------------------------------------- dragging

/// The share of control point `k` in curve `ci` at parameter `t`: linear
/// between the two neighbouring points of the segment.
fn curve_share(path: &DeformPath, ci: usize, t: f32, k: usize) -> f32 {
    let Some(c) = path.curves.get(ci) else {
        return 0.0;
    };
    let n = c.points.len();
    if n == 0 {
        return 0.0;
    }
    let i = t.floor().max(0.0) as usize;
    let f = t - i as f32;
    let a = c.points.get(i % n).copied();
    let b = c.points.get((i + 1) % n).copied();
    let mut s = 0.0;
    if a == Some(k) {
        s += 1.0 - f;
    }
    if b == Some(k) && !(c.open && i + 1 >= n) {
        s += f;
    }
    s
}

/// Total influence of each control point on its own anchor vertices (§4
/// "At press"): Σ over the anchor's vertices (of the main mesh) of their
/// effects' weight from this point.
pub fn total_effects(path: &DeformPath, main: &str) -> Vec<f32> {
    let mut out = vec![0.0f32; path.points.len()];
    for (k, cp) in path.points.iter().enumerate() {
        for &vi in &cp.anchor.index {
            let Some(tp) = path
                .targets
                .iter()
                .find(|t| t.mesh == main && t.index as i32 == vi)
            else {
                continue;
            };
            for e in &tp.effects {
                out[k] += match &e.effector {
                    Effector::Point(j) if *j == k => e.weight,
                    Effector::Curve { curve, t, .. } => e.weight * curve_share(path, *curve, *t, k),
                    _ => 0.0,
                };
            }
        }
    }
    out
}

/// The drag deformation (§4): new canvas positions of the target vertices
/// (`p0`, in target order) when the control points move from `before` to
/// `after`.
pub fn deform(path: &DeformPath, before: &[V2], after: &[V2], p0: &[V2]) -> Vec<V2> {
    let cb = curves(path, before);
    let ca = curves(path, after);
    path.targets
        .iter()
        .zip(p0)
        .map(|(t, &p0)| {
            let mut p = p0;
            for e in &t.effects {
                let w = e.weight;
                let dst = match &e.effector {
                    Effector::Point(k) => {
                        let (Some(a), Some(b)) = (after.get(*k), before.get(*k)) else {
                            continue;
                        };
                        add(p0, sub(*a, *b))
                    }
                    Effector::Curve { curve, t, .. } => {
                        let (Some(c0), Some(c1), Some(cc)) =
                            (cb.get(*curve), ca.get(*curve), path.curves.get(*curve))
                        else {
                            continue;
                        };
                        let (pb, tb) = (c0.point(*t), c0.tangent(*t));
                        let (pa, ta) = (c1.point(*t), c1.tangent(*t));
                        let (nb, na) = ([-tb[1], tb[0]], [-ta[1], ta[0]]);
                        let r = sub(p0, pb);
                        let (a, b) = (r[0] * tb[0] + r[1] * tb[1], r[0] * nb[0] + r[1] * nb[1]);
                        let rotated = add(pa, add(mul(ta, a), mul(na, b)));
                        let translated = add(p0, sub(pa, pb));
                        let d = len(r);
                        let (wd, h) = (cc.width, cc.hardness / 100.0);
                        let lambda = if wd - wd * h <= 0.0 {
                            if d > wd * h {
                                1.0
                            } else {
                                0.0
                            }
                        } else {
                            ((d - wd * h) / (wd - wd * h)).clamp(0.0, 1.0)
                        };
                        add(mul(rotated, 1.0 - lambda), mul(translated, lambda))
                    }
                };
                p = add(mul(p, 1.0 - w), mul(dst, w));
            }
            p
        })
        .collect()
}

// ------------------------------------------------------ path structure edits
// spec: deform-paths.md §5

/// Whether points `a` and `b` can be joined: neither on a curve, or each a
/// free end of an open curve (on different curves, or the two ends of the
/// same curve with more than 2 points, which closes it).
pub fn can_join(p: &DeformPath, a: usize, b: usize) -> bool {
    if a == b || a >= p.points.len() || b >= p.points.len() {
        return false;
    }
    let end = |k: usize| -> Option<Option<usize>> {
        match p.points[k].curve {
            None => Some(None),
            Some(c) => {
                let cv = p.curves.get(c)?;
                (cv.open && (cv.points.first() == Some(&k) || cv.points.last() == Some(&k)))
                    .then_some(Some(c))
            }
        }
    };
    match (end(a), end(b)) {
        (Some(Some(c)), Some(Some(d))) if c == d => p.curves[c].points.len() > 2,
        (Some(_), Some(_)) => true,
        _ => false,
    }
}

/// Joins `a` and `b` (see [`can_join`]); returns whether anything changed.
pub fn join(
    p: &mut DeformPath,
    a: usize,
    b: usize,
    new_curve_guid: impl FnOnce() -> String,
) -> bool {
    if !can_join(p, a, b) {
        return false;
    }
    match (p.points[a].curve, p.points[b].curve) {
        (None, None) => {
            let c = p.curves.len();
            p.curves
                .push(ControlCurve::new(new_curve_guid(), vec![a, b]));
            p.points[a].curve = Some(c);
            p.points[b].curve = Some(c);
        }
        (Some(c), None) | (None, Some(c)) => {
            let (free, on) = if p.points[a].curve.is_none() {
                (a, b)
            } else {
                (b, a)
            };
            let cv = &mut p.curves[c];
            if cv.points.last() == Some(&on) {
                cv.points.push(free);
            } else {
                cv.points.insert(0, free);
            }
            p.points[free].curve = Some(c);
        }
        (Some(c), Some(d)) if c == d => p.curves[c].open = false,
        (Some(c), Some(d)) => {
            // Merge d into c at the touching ends.
            let mut first = p.curves[c].points.clone();
            let mut second = p.curves[d].points.clone();
            if first.first() == Some(&a) {
                first.reverse();
            }
            if second.last() == Some(&b) {
                second.reverse();
            }
            first.extend(second);
            p.curves[c].points = first;
            remove_curve(p, d);
            for &k in &p.curves[if d < c { c - 1 } else { c }].points.clone() {
                p.points[k].curve = Some(if d < c { c - 1 } else { c });
            }
        }
    }
    true
}

/// Removes curve `c`; its points become free, later curves shift down.
pub fn remove_curve(p: &mut DeformPath, c: usize) {
    if c >= p.curves.len() {
        return;
    }
    p.curves.remove(c);
    for q in &mut p.points {
        q.curve = match q.curve {
            Some(x) if x == c => None,
            Some(x) if x > c => Some(x - 1),
            x => x,
        };
    }
    for t in &mut p.targets {
        t.effects.retain_mut(|e| match &mut e.effector {
            Effector::Curve { curve, .. } if *curve == c => false,
            Effector::Curve { curve, .. } => {
                if *curve > c {
                    *curve -= 1;
                }
                true
            }
            _ => true,
        });
    }
}

/// Curves with fewer than 2 points are deleted; closed curves with 2
/// points reopen.
fn tidy_curves(p: &mut DeformPath) {
    let mut c = 0;
    while c < p.curves.len() {
        if p.curves[c].points.len() < 2 {
            remove_curve(p, c);
            continue;
        }
        if p.curves[c].points.len() == 2 {
            p.curves[c].open = true;
        }
        c += 1;
    }
    for (ci, cv) in p.curves.iter().enumerate() {
        for &k in &cv.points {
            if let Some(q) = p.points.get_mut(k) {
                q.curve = Some(ci);
            }
        }
    }
}

/// Inserts a point (already pushed as index `k`, free) into curve `c` at
/// parameter `t`, inside segment ⌊t⌋. spec: §5 ("after index ⌈t⌉" read as
/// the list position ⌈t⌉)
pub fn insert_on_curve(p: &mut DeformPath, c: usize, t: f32, k: usize) {
    let Some(cv) = p.curves.get_mut(c) else {
        return;
    };
    // List position ⌈t⌉ (= ⌊t⌋ + 1 inside a segment): between the
    // segment's two points; after the last point of a closed curve.
    let at = (t.max(0.0).floor() as usize + 1).min(cv.points.len());
    cv.points.insert(at, k);
    p.points[k].curve = Some(c);
}

/// Deletes control point `k`. Returns whether the path has no points left.
pub fn delete_point(p: &mut DeformPath, k: usize) -> bool {
    if k >= p.points.len() {
        return p.points.is_empty();
    }
    p.points.remove(k);
    for cv in &mut p.curves {
        cv.points.retain(|&x| x != k);
        for x in &mut cv.points {
            if *x > k {
                *x -= 1;
            }
        }
    }
    for t in &mut p.targets {
        t.effects.retain_mut(|e| match &mut e.effector {
            Effector::Point(j) if *j == k => false,
            Effector::Point(j) => {
                if *j > k {
                    *j -= 1;
                }
                true
            }
            _ => true,
        });
    }
    tidy_curves(p);
    p.points.is_empty()
}

/// Deletes the segment of curve `c` starting at index `seg`. spec: §5
/// "Delete edge"
pub fn delete_edge(
    p: &mut DeformPath,
    c: usize,
    seg: usize,
    new_curve_guid: impl FnOnce() -> String,
) {
    let Some(cv) = p.curves.get(c).cloned() else {
        return;
    };
    let n = cv.points.len();
    if !cv.open {
        // Open it so the removed segment becomes the gap.
        let s = seg % n;
        let pts: Vec<usize> = (0..n).map(|i| cv.points[(s + 1 + i) % n]).collect();
        p.curves[c].points = pts;
        p.curves[c].open = true;
    } else if seg + 1 >= n {
        return;
    } else if seg == 0 {
        let k = p.curves[c].points.remove(0);
        p.points[k].curve = None;
    } else if seg + 2 == n {
        if let Some(k) = p.curves[c].points.pop() {
            p.points[k].curve = None;
        }
    } else {
        let tail = p.curves[c].points.split_off(seg + 1);
        let mut nc = ControlCurve::new(new_curve_guid(), tail);
        nc.width = cv.width;
        nc.hardness = cv.hardness;
        p.curves.push(nc);
    }
    tidy_curves(p);
}

// ------------------------------------------------------------ on a model

use keyform_eval::{DeformerMap, Frame};
use keyform_model::Model;

use crate::forms::{current_cell, parent_deformer, FormError};
use crate::ObjectRef;

/// Canvas positions of mesh `i` in `frame`.
fn canvas(frame: &Frame, i: usize) -> Option<&[f32]> {
    frame.meshes.get(i).map(|s| &s.positions[..])
}

/// Current canvas positions of path `pi` of mesh `mi`'s control points:
/// anchored on the main mesh, else the stored bind position.
pub fn point_positions(m: &Model, frame: &Frame, mi: usize, pi: usize) -> Vec<V2> {
    let Some(p) = m.art_meshes.get(mi).and_then(|a| a.paths.get(pi)) else {
        return Vec::new();
    };
    let pos = canvas(frame, mi).unwrap_or(&[]);
    p.points
        .iter()
        .map(|c| anchored(&c.anchor, pos).unwrap_or(c.pos_default))
        .collect()
}

/// The vertices bound by a path of mesh `mi`: all of the main mesh's, then
/// each sub mesh's. spec: §2
fn target_vertices(
    m: &Model,
    frame: &Frame,
    values: &[f32],
    mi: usize,
    subs: &[String],
) -> Vec<BindVertex> {
    let mut meshes = vec![mi];
    for g in subs {
        if let Some(k) = m.art_meshes.iter().position(|a| &a.info.guid == g) {
            if k != mi && !meshes.contains(&k) {
                meshes.push(k);
            }
        }
    }
    let mut out = Vec::new();
    for k in meshes {
        let a = &m.art_meshes[k];
        let Some(pos) = canvas(frame, k) else {
            continue;
        };
        let form = current_cell(m, ObjectRef::ArtMesh(k), values)
            .ok()
            .and_then(|c| a.grid.guids.get(c).cloned())
            .unwrap_or_default();
        for v in 0..a.vertex_count() {
            let Some(q) = pos.get(2 * v..2 * v + 2) else {
                continue;
            };
            let uid = a.point_uids.get(v).copied().unwrap_or(v as i64);
            out.push((a.info.guid.clone(), uid, v, form.clone(), [q[0], q[1]]));
        }
    }
    out
}

/// Resets the target points of path `pi` of mesh `mi` and binds them in
/// the state of `frame`. spec: §2, §3
pub fn rebind(m: &mut Model, frame: &Frame, values: &[f32], mi: usize, pi: usize) {
    let pts = point_positions(m, frame, mi, pi);
    let Some(subs) = m
        .art_meshes
        .get(mi)
        .and_then(|a| a.paths.get(pi))
        .map(|p| p.sub_meshes.clone())
    else {
        return;
    };
    let verts = target_vertices(m, frame, values, mi, &subs);
    let a = std::sync::Arc::make_mut(&mut m.art_meshes[mi]);
    bind(&mut a.paths[pi], &pts, &verts);
}

/// Anchors control point `k` of path `pi` of mesh `mi` at canvas `at`,
/// in the main mesh's state in `frame`. spec: §1 "Anchoring"
pub fn reanchor(m: &mut Model, frame: &Frame, mi: usize, pi: usize, k: usize, at: V2) {
    let Some(pos) = canvas(frame, mi).map(<[f32]>::to_vec) else {
        return;
    };
    let a = std::sync::Arc::make_mut(&mut m.art_meshes[mi]);
    let an = anchor(&pos, &a.indices, at);
    if let Some(c) = a.paths.get_mut(pi).and_then(|p| p.points.get_mut(k)) {
        c.anchor = an;
        c.recovery = at;
    }
}

/// One mesh a drag writes: its index, keyform cell and parent map.
#[derive(Clone, Debug)]
struct DragMesh {
    mesh: usize,
    cell: usize,
    parent: Option<DeformerMap>,
}

/// A drag of control points (§4), set up at press.
#[derive(Clone, Debug)]
pub struct PathDrag {
    pub mesh: usize,
    pub path: usize,
    /// Dragged control points.
    pub selected: Vec<usize>,
    /// Control point positions at press.
    pub before: Vec<V2>,
    /// Points locked at their canvas position (total effect < 0.5).
    pub locked: Vec<bool>,
    /// Target vertices' positions at press (target order).
    p0: Vec<V2>,
    /// Per target: (index into `meshes`, vertex index).
    at: Vec<Option<(usize, usize)>>,
    meshes: Vec<DragMesh>,
}

impl PathDrag {
    /// Sets up a drag of `selected` points of path `pi` of mesh `mi` in the
    /// state of `frame`. Fails when a target mesh isn't on a keyform.
    pub fn begin(
        m: &Model,
        frame: &Frame,
        values: &[f32],
        mi: usize,
        pi: usize,
        selected: Vec<usize>,
    ) -> Result<Self, FormError> {
        let a = m.art_meshes.get(mi).ok_or(FormError::Missing)?;
        let p = a.paths.get(pi).ok_or(FormError::Missing)?;
        let before = point_positions(m, frame, mi, pi);
        let totals = total_effects(p, &a.info.guid);
        let locked = totals.iter().map(|&t| t < 0.5).collect();
        let mut meshes: Vec<DragMesh> = Vec::new();
        let mut p0 = Vec::with_capacity(p.targets.len());
        let mut at = Vec::with_capacity(p.targets.len());
        for t in &p.targets {
            let found = m
                .art_meshes
                .iter()
                .position(|x| x.info.guid == t.mesh)
                .and_then(|k| {
                    let v = m.art_meshes[k].vertex_of_uid(t.uid)?;
                    Some((k, v))
                });
            let Some((k, v)) = found else {
                p0.push([0.0, 0.0]);
                at.push(None);
                continue;
            };
            let slot = match meshes.iter().position(|d| d.mesh == k) {
                Some(s) => s,
                None => {
                    let cell = current_cell(m, ObjectRef::ArtMesh(k), values)?;
                    let parent = parent_deformer(m, ObjectRef::ArtMesh(k))
                        .and_then(|d| frame.deformers.get(d))
                        .and_then(|s| s.map.clone());
                    meshes.push(DragMesh {
                        mesh: k,
                        cell,
                        parent,
                    });
                    meshes.len() - 1
                }
            };
            let pos = canvas(frame, k).unwrap_or(&[]);
            p0.push(
                pos.get(2 * v..2 * v + 2)
                    .map_or([0.0, 0.0], |q| [q[0], q[1]]),
            );
            at.push(Some((slot, v)));
        }
        Ok(Self {
            mesh: mi,
            path: pi,
            selected,
            before,
            locked,
            p0,
            at,
            meshes,
        })
    }

    /// Control point positions with the selected points moved by `delta`.
    pub fn after(&self, delta: V2) -> Vec<V2> {
        let mut out = self.before.clone();
        for &k in &self.selected {
            if let Some(q) = out.get_mut(k) {
                *q = add(*q, delta);
            }
        }
        out
    }

    /// Applies the drag by canvas `delta` to `m` (the model at press):
    /// each target vertex moves to its deformed position in its mesh's
    /// current keyform. spec: §4 "On drag"
    pub fn apply(&self, m: &mut Model, delta: V2) -> Result<(), FormError> {
        let Some(p) = m
            .art_meshes
            .get(self.mesh)
            .and_then(|a| a.paths.get(self.path))
        else {
            return Err(FormError::Missing);
        };
        let after = self.after(delta);
        let moved = deform(p, &self.before, &after, &self.p0);
        for (slot, d) in self.meshes.iter().enumerate() {
            let a = std::sync::Arc::make_mut(&mut m.art_meshes[d.mesh]);
            let f = a.grid.keyforms.get_mut(d.cell).ok_or(FormError::Missing)?;
            for (t, at) in self.at.iter().enumerate() {
                let Some((s, v)) = *at else { continue };
                if s != slot || 2 * v + 1 >= f.positions.len() {
                    continue;
                }
                let dl = sub(moved[t], self.p0[t]);
                if dl == [0.0, 0.0] {
                    continue;
                }
                let local = [f.positions[2 * v], f.positions[2 * v + 1]];
                let q = match &d.parent {
                    None => add(local, dl),
                    Some(map) => map
                        .inverse(add(map.map(local), dl))
                        .ok_or(FormError::NotInvertible)?,
                };
                f.positions[2 * v] = q[0];
                f.positions[2 * v + 1] = q[1];
            }
        }
        Ok(())
    }

    /// On release, with `frame` the state after the drag: locked points
    /// are re-anchored at their positions (dragged or kept), and the total
    /// effects stored. spec: §4 "On release"
    pub fn finish(&self, m: &mut Model, frame: &Frame, delta: V2) {
        let after = self.after(delta);
        for (k, &l) in self.locked.iter().enumerate() {
            if l {
                reanchor(m, frame, self.mesh, self.path, k, after[k]);
            }
        }
        let a = std::sync::Arc::make_mut(&mut m.art_meshes[self.mesh]);
        let guid = a.info.guid.clone();
        if let Some(p) = a.paths.get_mut(self.path) {
            let totals = total_effects(p, &guid);
            for (c, t) in p.points.iter_mut().zip(totals) {
                c.total_effect = t;
            }
        }
    }
}

/// Replaces mesh `i`'s topology ([`crate::mesh::set_mesh`]) keeping its
/// deform paths: control points are held at their canvas positions,
/// re-anchored on the new mesh, and every path driving it is rebound, in
/// the state at `values`. spec: deform-paths.md §1 "Topology observers"
pub fn set_mesh_keep_paths(
    m: &mut Model,
    values: &[f32],
    i: usize,
    g: &crate::mesh::MeshGraph,
) -> Result<crate::mesh::Triangulated, crate::mesh::MeshError> {
    let guid = m
        .art_meshes
        .get(i)
        .map(|a| a.info.guid.clone())
        .unwrap_or_default();
    // Paths that drive mesh i: its own, and those listing it as a sub mesh.
    let mut driving: Vec<(usize, usize)> = Vec::new();
    for (k, a) in m.art_meshes.iter().enumerate() {
        for (pi, p) in a.paths.iter().enumerate() {
            if k == i || p.sub_meshes.contains(&guid) {
                driving.push((k, pi));
            }
        }
    }
    if driving.is_empty() {
        return crate::mesh::set_mesh(m, i, g);
    }
    let ev = |m: &Model| keyform_eval::Evaluator::new(m).evaluate(values);
    let f0 = ev(m);
    let held: Vec<Vec<V2>> = driving
        .iter()
        .map(|&(k, pi)| point_positions(m, &f0, k, pi))
        .collect();
    let t = crate::mesh::set_mesh(m, i, g)?;
    let f1 = ev(m);
    for (&(k, pi), pts) in driving.iter().zip(&held) {
        if k == i {
            for (c, &q) in pts.iter().enumerate() {
                reanchor(m, &f1, k, pi, c, q);
            }
        }
        rebind(m, &f1, values, k, pi);
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::{BindMethod, ControlCurve, ControlPoint, DeformPath};

    fn cp(curve: Option<usize>) -> ControlPoint {
        ControlPoint {
            guid: String::new(),
            curve,
            corner: false,
            anchor: PointInTriangle::default(),
            pos_default: [0.0, 0.0],
            recovery: [0.0, 0.0],
            total_effect: 0.0,
        }
    }

    #[test]
    fn anchoring() {
        let pos = [0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 10.0];
        let idx = [0, 1, 2, 1, 3, 2];
        let a = anchor(&pos, &idx, [2.0, 3.0]);
        assert_eq!(a.index, [0, 1, 2]);
        let p = anchored(&a, &pos).unwrap();
        assert!((p[0] - 2.0).abs() < 1e-5 && (p[1] - 3.0).abs() < 1e-5);
        // Outside: extrapolated weights still reproduce the point.
        let a = anchor(&pos, &idx, [15.0, 5.0]);
        let p = anchored(&a, &pos).unwrap();
        assert!((p[0] - 15.0).abs() < 1e-4 && (p[1] - 5.0).abs() < 1e-4);
    }

    #[test]
    fn curve_passes_through_points() {
        let pts = vec![[0.0, 0.0], [10.0, 5.0], [20.0, 0.0], [30.0, 5.0]];
        for open in [true, false] {
            let c = Curve::new(pts.clone(), &[false; 4], open);
            for (i, p) in pts.iter().enumerate() {
                let q = c.point(i as f32);
                assert!(dist(*p, q) < 1e-4, "{open} {i}: {q:?}");
            }
            assert_eq!(c.segments(), if open { 3 } else { 4 });
        }
        // A corner splits the curve; it still passes through every point.
        let c = Curve::new(pts.clone(), &[false, true, false, false], true);
        for (i, p) in pts.iter().enumerate() {
            assert!(dist(*p, c.point(i as f32)) < 1e-4);
        }
        let (t, d, _) = Curve::new(pts.clone(), &[false; 4], true).nearest([10.0, 5.0]);
        assert!(d < 1e-3 && (t - 1.0).abs() < 0.05);
    }

    #[test]
    fn binding_and_drag() {
        // Two free points; a vertex between them.
        let mut path = DeformPath::new(String::new(), 2);
        path.bind_method = BindMethod::DistanceSimple;
        path.points = vec![cp(None), cp(None)];
        let pts = [[0.0, 0.0], [10.0, 0.0]];
        bind(
            &mut path,
            &pts,
            &[("m".into(), 0, 0, String::new(), [2.5, 0.0])],
        );
        let e = &path.targets[0].effects;
        assert_eq!(e.len(), 2);
        assert!((e[0].weight - 0.75).abs() < 1e-5 && (e[1].weight - 0.25).abs() < 1e-5);
        // Moving point 0 by (4, 0): p ← p·(1−w) + dst·w for each effect.
        let out = deform(&path, &pts, &[[4.0, 0.0], [10.0, 0.0]], &[[2.5, 0.0]]);
        // Sequential: (2.5·0.25 + 6.5·0.75) = 5.5, then 5.5·0.75 + 2.5·0.25 = 4.75.
        assert!((out[0][0] - 4.75).abs() < 1e-5, "{out:?}");
        // On a curve, LINE methods use the curve's nearest point.
        let mut path = DeformPath::new(String::new(), 2);
        path.points = vec![cp(Some(0)), cp(Some(0)), cp(Some(0))];
        path.curves = vec![ControlCurve::new(String::new(), vec![0, 1, 2])];
        let pts = [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]];
        bind(
            &mut path,
            &pts,
            &[("m".into(), 0, 0, String::new(), [10.0, 3.0])],
        );
        let e = &path.targets[0].effects;
        assert_eq!(e.len(), 1);
        assert!(matches!(e[0].effector, Effector::Curve { .. }));
        // Translating the whole curve translates the vertex.
        let after = [[0.0, 5.0], [10.0, 5.0], [20.0, 5.0]];
        let out = deform(&path, &pts, &after, &[[10.0, 3.0]]);
        assert!((out[0][1] - 8.0).abs() < 1e-3, "{out:?}");
    }

    fn path_with(n: usize) -> DeformPath {
        let mut p = DeformPath::new(String::new(), 2);
        p.points = (0..n).map(|_| cp(None)).collect();
        p
    }

    #[test]
    fn joining() {
        let g = || "c".to_string();
        let mut p = path_with(5);
        assert!(join(&mut p, 0, 1, g));
        assert_eq!(p.curves[0].points, vec![0, 1]);
        // A free point joins an end.
        assert!(join(&mut p, 1, 2, g));
        assert!(join(&mut p, 3, 0, g));
        assert_eq!(p.curves[0].points, vec![3, 0, 1, 2]);
        // A middle point can't be joined.
        assert!(!can_join(&p, 0, 4));
        // Two curves merge at their touching ends.
        let mut q = path_with(4);
        join(&mut q, 0, 1, g);
        join(&mut q, 2, 3, g);
        assert!(join(&mut q, 0, 2, g));
        assert_eq!(q.curves.len(), 1);
        assert_eq!(q.curves[0].points, vec![1, 0, 2, 3]);
        assert!(q.points.iter().all(|c| c.curve == Some(0)));
        // The ends of one curve with more than 2 points close it.
        assert!(join(&mut q, 1, 3, g));
        assert!(!q.curves[0].open);
        let mut r = path_with(2);
        join(&mut r, 0, 1, g);
        assert!(!can_join(&r, 0, 1));
    }

    #[test]
    fn deleting() {
        let g = || "c".to_string();
        let mut p = path_with(3);
        join(&mut p, 0, 1, g);
        join(&mut p, 1, 2, g);
        assert!(!delete_point(&mut p, 1));
        assert_eq!(p.curves[0].points, vec![0, 1]);
        assert!(!delete_point(&mut p, 0));
        assert!(p.curves.is_empty() && p.points[0].curve.is_none());
        assert!(delete_point(&mut p, 0));
        // A closed curve left with 2 points reopens.
        let mut q = path_with(3);
        join(&mut q, 0, 1, g);
        join(&mut q, 1, 2, g);
        join(&mut q, 2, 0, g);
        assert!(!q.curves[0].open);
        delete_point(&mut q, 2);
        assert!(q.curves[0].open);
        // Delete edge: split in the middle, drop an end, open a loop.
        let mut r = path_with(4);
        for k in 0..3 {
            join(&mut r, k, k + 1, g);
        }
        delete_edge(&mut r, 0, 1, g);
        assert_eq!(r.curves.len(), 2);
        assert_eq!(
            (r.curves[0].points.clone(), r.curves[1].points.clone()),
            (vec![0, 1], vec![2, 3])
        );
        assert_eq!(r.points[3].curve, Some(1));
        delete_edge(&mut r, 1, 0, g);
        assert_eq!(r.curves.len(), 1);
        assert!(r.points[2].curve.is_none() && r.points[3].curve.is_none());
        let mut c = path_with(4);
        for k in 0..3 {
            join(&mut c, k, k + 1, g);
        }
        join(&mut c, 3, 0, g);
        delete_edge(&mut c, 0, 3, g);
        assert!(c.curves[0].open);
        assert_eq!(c.curves[0].points, vec![0, 1, 2, 3]);
        delete_edge(&mut c, 0, 0, g);
        assert_eq!(c.curves[0].points, vec![1, 2, 3]);
        // Insert inside segment 1.
        c.points.push(cp(None));
        insert_on_curve(&mut c, 0, 1.4, 4);
        assert_eq!(c.curves[0].points, vec![1, 2, 4, 3]);
    }
}
