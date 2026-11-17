//! Parameter controllers: points pinned on art meshes that drive
//! parameters through a damped-least-squares solve, and their
//! `*.paramctrl3.json` file.
//! spec: 60-animation/form-animation-and-controllers.md §2

use serde_json::{json, Map, Value};

use crate::AnimError;

pub use keyform_model::animation::{Controller, ControllerSet, CtrlPoint, CtrlShape};

/// `meshIndicesHash`: h = 1; h = 31·h + i for every triangle index (32-bit
/// wrap). spec: form-animation-and-controllers.md §3.2
pub fn mesh_indices_hash(indices: &[u32]) -> i32 {
    indices
        .iter()
        .fold(1i32, |h, &i| h.wrapping_mul(31).wrapping_add(i as i32))
}

/// The point on the mesh's current positions (x, y pairs).
pub fn point_position(p: &CtrlPoint, positions: &[f32]) -> Option<[f64; 2]> {
    let mut q = [0.0; 2];
    for (k, &v) in p.tri.iter().enumerate() {
        let r = positions.get(2 * v..2 * v + 2)?;
        q[0] += p.bary[k] * f64::from(r[0]);
        q[1] += p.bary[k] * f64::from(r[1]);
    }
    Some(q)
}

/// Pins a point at `at` onto a mesh (`positions`, triangle `indices`):
/// the containing triangle, else the nearest one with clamped weights.
pub fn pin(
    name: &str,
    mesh_id: &str,
    positions: &[f32],
    indices: &[u32],
    at: [f64; 2],
) -> Option<CtrlPoint> {
    let v =
        |i: usize| -> [f64; 2] { [f64::from(positions[2 * i]), f64::from(positions[2 * i + 1])] };
    let n = positions.len() / 2;
    let mut best: Option<(f64, [usize; 3], [f64; 3])> = None;
    for t in indices.chunks_exact(3) {
        let tri = [t[0] as usize, t[1] as usize, t[2] as usize];
        if tri.iter().any(|&i| i >= n) {
            continue;
        }
        let (a, b, c) = (v(tri[0]), v(tri[1]), v(tri[2]));
        let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if det.abs() < 1e-12 {
            continue;
        }
        let l0 = ((b[1] - c[1]) * (at[0] - c[0]) + (c[0] - b[0]) * (at[1] - c[1])) / det;
        let l1 = ((c[1] - a[1]) * (at[0] - c[0]) + (a[0] - c[0]) * (at[1] - c[1])) / det;
        let w = [l0, l1, 1.0 - l0 - l1];
        let out = w.iter().map(|x| (-x).max(0.0)).sum::<f64>();
        if best.as_ref().is_none_or(|b| out < b.0) {
            best = Some((out, tri, w));
        }
    }
    let (out, tri, mut w) = best?;
    if out > 0.0 {
        for x in &mut w {
            *x = x.max(0.0);
        }
        let s: f64 = w.iter().sum();
        w.iter_mut().for_each(|x| *x /= s);
    }
    let mut p = CtrlPoint {
        guid: keyform_model::new_guid(),
        name: name.to_owned(),
        mesh_id: mesh_id.to_owned(),
        tri,
        bary: w,
        hash: mesh_indices_hash(indices),
        recovery: [0.0; 2],
        recovery_mesh: None,
    };
    // Stored as a float vector (`recoveryPoint`).
    let r = point_position(&p, positions)?;
    p.recovery = [f64::from(r[0] as f32), f64::from(r[1] as f32)];
    Some(p)
}

/// What reaching the target means (§2.1).
#[derive(Clone, Debug, PartialEq)]
pub enum Assessor {
    /// Single point → point.
    Point([f64; 2]),
    /// Point → area (segment AB) with pre-start / post-end contact.
    Area {
        a: [f64; 2],
        b: [f64; 2],
        pre: bool,
        post: bool,
    },
    /// Line → line: the target direction.
    Direction([f64; 2]),
    /// Line → point T, with post-end contact.
    Aim { t: [f64; 2], post: bool },
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn len(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

/// (effector, target) of a controller from its points' positions (§2.1).
pub fn effector_and_target(
    shape: &CtrlShape,
    a: &Assessor,
    pos: &dyn Fn(usize) -> Option<[f64; 2]>,
) -> Option<([f64; 2], [f64; 2])> {
    match (shape, a) {
        (CtrlShape::Single(p), Assessor::Point(t)) => Some((pos(*p)?, *t)),
        (CtrlShape::Single(p), Assessor::Area { a, b, pre, post }) => {
            let p = pos(*p)?;
            let ab = sub(*b, *a);
            let l2 = ab[0] * ab[0] + ab[1] * ab[1];
            let mut u = if l2 > 0.0 {
                ((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2
            } else {
                0.0
            };
            if u < 0.0 && !pre {
                u = 0.0;
            }
            if u > 1.0 && !post {
                u = 1.0;
            }
            Some((p, [a[0] + u * ab[0], a[1] + u * ab[1]]))
        }
        (CtrlShape::Line(s, e), Assessor::Direction(d)) => {
            let v = sub(pos(*e)?, pos(*s)?);
            let l = len(*d);
            let k = if l > 0.0 { len(v) / l } else { 0.0 };
            Some((v, [d[0] * k, d[1] * k]))
        }
        (CtrlShape::Line(s, e), Assessor::Aim { t, post }) => {
            let (s, e) = (pos(*s)?, pos(*e)?);
            let dir = sub(e, s);
            let l = len(dir);
            let mut dist = len(sub(*t, s));
            if !post {
                dist = dist.min(l);
            }
            let eff = if dist == 0.0 || l == 0.0 {
                s
            } else {
                [s[0] + dir[0] / l * dist, s[1] + dir[1] / l * dist]
            };
            Some((eff, *t))
        }
        _ => None,
    }
}

/// The solver's model evaluation: values → (effector, target) per
/// active controller, in canvas pixels.
pub type EvalFn<'a> = dyn FnMut(&[f64]) -> Vec<([f64; 2], [f64; 2])> + 'a;

/// A solver unknown: the parameter's current value, range and repeat flag.
#[derive(Clone, Copy, Debug)]
pub struct Unknown {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub repeat: bool,
}

/// Damped least squares over the unknowns (§2.2). `eval` returns, for the
/// given values, each active controller's (effector, target) in canvas
/// pixels; `canvas` normalises them to 1024. Returns the solved values.
pub fn solve(unknowns: &[Unknown], canvas: [f64; 2], eval: &mut EvalFn<'_>) -> Vec<f64> {
    let n = unknowns.len();
    let mut p: Vec<f64> = unknowns.iter().map(|u| u.value).collect();
    if n == 0 {
        return p;
    }
    let norm = |q: [f64; 2]| [q[0] / canvas[0] * 1024.0, q[1] / canvas[1] * 1024.0];
    let range: Vec<f64> = unknowns
        .iter()
        .map(|u| (u.max - u.min).abs().max(1e-9))
        .collect();
    let mut w = vec![1.0f64; n];
    let mut d = 1.0f64;
    let mut steps = 0usize;
    for _ in 0..50 {
        // 1. Errors.
        let cur = eval(&p);
        let m = cur.len();
        if m == 0 {
            break;
        }
        let eff: Vec<[f64; 2]> = cur.iter().map(|c| norm(c.0)).collect();
        let mut e: Vec<[f64; 2]> = cur.iter().map(|c| sub(norm(c.1), norm(c.0))).collect();
        if e.iter().map(|x| len(*x)).fold(0.0, f64::max) < 0.1 {
            break;
        }
        // 2. Jacobian by forward differences (2m × n).
        let mut jac = vec![vec![0.0f64; n]; 2 * m];
        for i in 0..n {
            let mut delta = 0.01 * range[i] * d;
            if p[i] + delta >= unknowns[i].max {
                delta = -delta;
            }
            let mut q = p.clone();
            q[i] += delta;
            let moved = eval(&q);
            for k in 0..m.min(moved.len()) {
                let e2 = norm(moved[k].0);
                jac[2 * k][i] = (e2[0] - eff[k][0]) / delta;
                jac[2 * k + 1][i] = (e2[1] - eff[k][1]) / delta;
            }
        }
        if jac.iter().all(|r| r.iter().all(|x| *x == 0.0)) {
            d *= 2.0;
            continue;
        }
        d = 1.0;
        // 3. Damping.
        let l0 = if m == 1 { 2.0 } else { 125.0 };
        let lambda = l0 + (2400.0 - l0) * (steps as f64 / 50.0).min(1.0).powi(5);
        // 4. Scale.
        let s = (0..n)
            .map(|i| jac[0][i].hypot(jac[1][i]) / range[i])
            .fold(1e-6, f64::max);
        let a_inv: Vec<f64> = (0..n).map(|i| 1.0 / (s * w[i])).collect();
        // 5. Error clamp.
        for x in &mut e {
            let l = len(*x);
            if l > 102.4 {
                let k = 102.4 / l;
                let f = k * (2.0 - k);
                x[0] *= f;
                x[1] *= f;
            }
        }
        let ev: Vec<f64> = e.iter().flat_map(|x| [x[0], x[1]]).collect();
        // 6. Δp = A⁻¹ Jᵀ (J A⁻¹ Jᵀ + λI)⁻¹ e.
        let r = 2 * m;
        let mut mm = vec![vec![0.0f64; r]; r];
        for a in 0..r {
            for b in 0..r {
                mm[a][b] = (0..n)
                    .map(|i| jac[a][i] * a_inv[i] * jac[b][i])
                    .sum::<f64>();
            }
            mm[a][a] += lambda;
        }
        let Some(y) = solve_linear(mm, ev) else {
            break;
        };
        let mut dp: Vec<f64> = (0..n)
            .map(|i| a_inv[i] * (0..r).map(|a| jac[a][i] * y[a]).sum::<f64>())
            .collect();
        // 7. Step limit.
        let c = (0..n)
            .map(|i| {
                let rel = dp[i].abs() / range[i];
                if rel < 0.1 {
                    1.0
                } else {
                    0.1 / rel
                }
            })
            .fold(1.0, f64::min);
        dp.iter_mut().for_each(|x| *x *= c);
        // 8. Apply, clamp, weights.
        for i in 0..n {
            let u = unknowns[i];
            let v = p[i] + dp[i];
            if u.repeat {
                p[i] = v;
            } else {
                let (lo, hi) = (u.min.min(u.max), u.min.max(u.max));
                if (v < lo || v > hi) && w[i] < 65536.0 {
                    w[i] *= 2.0;
                }
                p[i] = v.clamp(lo, hi);
            }
        }
        steps += 1;
        // 9. Converged steps.
        if (0..n).all(|i| dp[i].abs() / range[i] < 0.001) {
            break;
        }
    }
    p
}

/// Solves M x = b (Gaussian elimination with partial pivoting).
fn solve_linear(mut m: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for c in 0..n {
        let piv = (c..n).max_by(|&i, &j| m[i][c].abs().total_cmp(&m[j][c].abs()))?;
        if m[piv][c].abs() < 1e-15 {
            return None;
        }
        m.swap(c, piv);
        b.swap(c, piv);
        for r in c + 1..n {
            let f = m[r][c] / m[c][c];
            for k in c..n {
                m[r][k] -= f * m[c][k];
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = vec![0.0; n];
    for c in (0..n).rev() {
        x[c] = (b[c] - (c + 1..n).map(|k| m[c][k] * x[k]).sum::<f64>()) / m[c][c];
    }
    Some(x)
}

fn hex(c: [f32; 4]) -> String {
    c.iter()
        .map(|x| format!("{:02X}", (x.clamp(0.0, 1.0) * 255.0).round() as u8))
        .collect()
}

fn unhex(s: &str) -> [f32; 4] {
    let mut c = [1.0f32; 4];
    for (k, x) in c.iter_mut().enumerate() {
        if let Some(v) = s
            .get(2 * k..2 * k + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            *x = f32::from(v) / 255.0;
        }
    }
    c
}

fn r3(x: f64) -> Value {
    json!((x * 1000.0).round() / 1000.0)
}

/// Writes `*.paramctrl3.json` (§2.4). `mesh_id` gives an art mesh's ID.
pub fn export_json(set: &ControllerSet) -> String {
    let drawables: Vec<Value> = set
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "Id": format!("ControlPoint_{i}"),
                "Name": p.name,
                "ObjectId": p.mesh_id,
                "Point": { "X": r3(p.recovery[0]), "Y": r3(p.recovery[1]) },
            })
        })
        .collect();
    let (mut single, mut line) = (Vec::new(), Vec::new());
    for (j, c) in set.controllers.iter().enumerate() {
        let mut o = Map::new();
        o.insert("Id".into(), json!(format!("Control_{j}")));
        o.insert("Name".into(), json!(c.name));
        o.insert("Color".into(), json!(hex(c.color)));
        match c.shape {
            CtrlShape::Single(p) => {
                o.insert("PointId".into(), json!(format!("ControlPoint_{p}")));
            }
            CtrlShape::Line(s, e) => {
                o.insert("StartId".into(), json!(format!("ControlPoint_{s}")));
                o.insert("EndId".into(), json!(format!("ControlPoint_{e}")));
            }
        }
        o.insert("ParameterIds".into(), json!(c.params));
        if let Some((t, ext)) = c.target {
            let mut tg = Map::new();
            tg.insert("Id".into(), json!(format!("Control_{t}")));
            let has_ext = matches!(
                (&c.shape, set.controllers.get(t).map(|x| &x.shape)),
                (CtrlShape::Single(_), Some(CtrlShape::Line(..)))
                    | (CtrlShape::Line(..), Some(CtrlShape::Single(_)))
            );
            if has_ext {
                tg.insert("ExtendLine".into(), json!(ext));
            }
            o.insert("Target".into(), Value::Object(tg));
        }
        match c.shape {
            CtrlShape::Single(_) => single.push(Value::Object(o)),
            CtrlShape::Line(..) => line.push(Value::Object(o)),
        }
    }
    let v = json!({
        "Version": 3,
        "ParamCtrlPoints": { "Drawables": drawables },
        "ParamCtrls": { "Single": single, "Line": line },
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// A parsed `*.paramctrl3.json`: points still to pin (mesh ID, name,
/// position) and controllers referring to them by index.
#[derive(Debug, Default)]
pub struct Imported {
    pub version: i64,
    pub points: Vec<(String, String, [f64; 2])>,
    pub controllers: Vec<Controller>,
}

/// Reads `*.paramctrl3.json` with the §2.4 checks (Version present, both
/// sections, every point used by exactly one controller reference).
pub fn import_json(text: &str) -> Result<Imported, AnimError> {
    let bad = |m: &str| AnimError::Invalid(format!("paramctrl3: {m}"));
    let v: Value = serde_json::from_str(text).map_err(|e| bad(&e.to_string()))?;
    let version = v
        .get("Version")
        .and_then(Value::as_i64)
        .ok_or_else(|| bad("no Version"))?;
    let pts = v
        .get("ParamCtrlPoints")
        .and_then(|p| p.get("Drawables"))
        .and_then(Value::as_array)
        .ok_or_else(|| bad("no ParamCtrlPoints"))?;
    let ctrls = v.get("ParamCtrls").ok_or_else(|| bad("no ParamCtrls"))?;
    let mut ids = std::collections::HashMap::new();
    let mut out = Imported {
        version,
        ..Default::default()
    };
    for p in pts {
        let s = |k: &str| {
            p.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let xy = |k: &str| {
            p.get("Point")
                .and_then(|q| q.get(k))
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
        };
        ids.insert(s("Id"), out.points.len());
        out.points
            .push((s("ObjectId"), s("Name"), [xy("X"), xy("Y")]));
    }
    let mut used = vec![0usize; out.points.len()];
    let mut cids = std::collections::HashMap::new();
    let mut targets = Vec::new();
    for (kind, list) in [("Single", ctrls.get("Single")), ("Line", ctrls.get("Line"))] {
        for c in list.and_then(Value::as_array).into_iter().flatten() {
            let s = |k: &str| {
                c.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            let mut point = |k: &str| -> Result<usize, AnimError> {
                let i = *ids
                    .get(&s(k))
                    .ok_or_else(|| bad(&format!("unknown point {}", s(k))))?;
                used[i] += 1;
                Ok(i)
            };
            let shape = if kind == "Single" {
                CtrlShape::Single(point("PointId")?)
            } else {
                CtrlShape::Line(point("StartId")?, point("EndId")?)
            };
            cids.insert(s("Id"), out.controllers.len());
            targets.push(c.get("Target").map(|t| {
                (
                    t.get("Id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    t.get("ExtendLine")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                )
            }));
            out.controllers.push(Controller {
                guid: keyform_model::new_guid(),
                name: s("Name"),
                color: unhex(&s("Color")),
                shape,
                params: c
                    .get("ParameterIds")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect(),
                target: None,
                stand_alone: false,
                select_lock: false,
            });
        }
    }
    if used.iter().any(|&u| u != 1) {
        return Err(bad("Point is not referenced correctly"));
    }
    for (i, t) in targets.into_iter().enumerate() {
        if let Some((id, ext)) = t {
            out.controllers[i].target = cids.get(&id).map(|&j| (j, ext));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two parameters move a point linearly (with a cross term); the solver
    /// finds the values that put it on the target.
    #[test]
    fn solver_reaches_target() {
        let unknowns = [
            Unknown {
                value: 0.0,
                min: -30.0,
                max: 30.0,
                repeat: false,
            },
            Unknown {
                value: 0.0,
                min: -30.0,
                max: 30.0,
                repeat: false,
            },
        ];
        let target = [620.0, 380.0];
        let mut eval = |p: &[f64]| {
            vec![(
                [500.0 + 5.0 * p[0] + 0.5 * p[1], 500.0 - 4.0 * p[1]],
                target,
            )]
        };
        let p = solve(&unknowns, [1000.0, 1000.0], &mut eval);
        let (e, _) = eval(&p)[0];
        assert!(
            (e[0] - target[0]).abs() < 0.5 && (e[1] - target[1]).abs() < 0.5,
            "{p:?} → {e:?}"
        );
        // Out of reach: clamped to the range.
        let mut far = |p: &[f64]| vec![([500.0 + 5.0 * p[0], 500.0 + 5.0 * p[1]], [5000.0, 500.0])];
        let p = solve(&unknowns, [1000.0, 1000.0], &mut far);
        assert!((p[0] - 30.0).abs() < 1e-9, "{p:?}");
    }

    #[test]
    fn pin_and_json_round_trip() {
        let pos = [0.0f32, 0.0, 10.0, 0.0, 0.0, 10.0];
        let p = pin("nose", "ArtMesh3", &pos, &[0, 1, 2], [2.0, 3.0]).unwrap();
        assert_eq!(
            point_position(&p, &pos).map(|q| [(q[0] * 1e6).round(), (q[1] * 1e6).round()]),
            Some([2e6, 3e6])
        );
        assert_eq!(p.hash, (31 * 31 + 1) * 31 + 2);
        let set = ControllerSet {
            points: vec![p.clone(), p.clone(), p],
            controllers: vec![
                Controller {
                    guid: "a".into(),
                    stand_alone: false,
                    select_lock: false,
                    name: "A".into(),
                    color: [1.0, 0.0, 0.0, 1.0],
                    shape: CtrlShape::Single(0),
                    params: vec!["ParamAngleX".into()],
                    target: Some((1, true)),
                },
                Controller {
                    guid: "b".into(),
                    stand_alone: false,
                    select_lock: false,
                    name: "B".into(),
                    color: [0.0, 0.5, 1.0, 1.0],
                    shape: CtrlShape::Line(1, 2),
                    params: vec!["ParamAngleY".into()],
                    target: None,
                },
            ],
        };
        let text = export_json(&set);
        assert!(text.contains("\"Color\": \"FF0000FF\""), "{text}");
        let back = import_json(&text).unwrap();
        assert_eq!(back.version, 3);
        assert_eq!(back.points.len(), 3);
        assert_eq!(back.points[0].0, "ArtMesh3");
        assert_eq!(back.controllers[0].target, Some((1, true)));
        assert_eq!(back.controllers[1].shape, CtrlShape::Line(1, 2));
        // A point used twice is refused.
        let twice = text.replace("ControlPoint_2\"", "ControlPoint_1\"");
        assert!(import_json(&twice).is_err());
    }
}
