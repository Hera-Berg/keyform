//! motion3.json export from an animation scene.
//! spec: 60-animation/motion3-json.md §2, animation-model.md §3.2–3.3, §6

use keyform_model::animation::{
    Attr, Curve as KeyCurve, CurveType, IntCurve, Scene, Track, TrackKind,
};

use crate::motion::{Curve, Motion, Segment, Target};
use crate::scene::{eval_curve, eval_int, param_id, part_id, segment_points, step_frame};

/// Which curves are written (export option).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CurveFilter {
    All,
    /// Curves that change (min ≠ max and has keys).
    Changing,
    /// Curves with keys.
    #[default]
    WithKeys,
}

#[derive(Clone, Copy, Debug)]
pub struct ExportOptions {
    /// The work area only (else the whole scene).
    pub work_area: bool,
    pub filter: CurveFilter,
    pub exclude_shy: bool,
    pub exclude_frozen: bool,
    /// The document's `isSmoothCurveLegacyMode` (which keys get automatic
    /// handles, animation-model.md §3.2 item 4).
    pub legacy_smooth: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            work_area: true,
            filter: CurveFilter::WithKeys,
            exclude_shy: false,
            exclude_frozen: false,
            legacy_smooth: true,
        }
    }
}

/// The exported frame range [start, end].
pub fn export_range(s: &Scene, o: &ExportOptions) -> (i32, i32) {
    if o.work_area {
        (s.work_start, s.work_end)
    } else {
        (s.start_frame, s.duration)
    }
}

fn keep(a: &Attr, o: &ExportOptions) -> bool {
    if (o.exclude_shy && a.shy) || (o.exclude_frozen && a.freeze) {
        return false;
    }
    match o.filter {
        CurveFilter::All => true,
        CurveFilter::WithKeys => !a.curve.keys.is_empty(),
        CurveFilter::Changing => {
            let k = &a.curve.keys;
            !k.is_empty() && k.iter().any(|x| x.value != k[0].value)
        }
    }
}

fn ms(v: i32) -> Option<f32> {
    (v >= 0).then(|| v as f32 / 1000.0)
}

/// Rounds in f64 to `d` decimals the way the file is written (exact
/// binary value, ties to even) before narrowing to the motion's f32, so
/// the written text is the reference's.
fn dec(v: f64, d: usize) -> f32 {
    format!("{v:.d$}").parse::<f64>().unwrap_or(0.0) as f32
}

fn cubic(p: [f64; 4], t: f64) -> f64 {
    let u = 1.0 - t;
    u * u * u * p[0] + 3.0 * u * u * t * p[1] + 3.0 * u * t * t * p[2] + t * t * t * p[3]
}

/// The curve parameter where the segment's frame cubic reaches `x`.
fn param_at(fx: [f64; 4], x: f64) -> f64 {
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if cubic(fx, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

type P = (f64, f64);

/// The part of a cubic Bézier between parameters a < b (de Casteljau).
fn sub_bezier(p: [P; 4], a: f64, b: f64) -> [P; 4] {
    fn split(p: [P; 4], t: f64) -> ([P; 4], [P; 4]) {
        let l = |a: P, b: P| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        let (ab, bc, cd) = (l(p[0], p[1]), l(p[1], p[2]), l(p[2], p[3]));
        let (abc, bcd) = (l(ab, bc), l(bc, cd));
        let m = l(abc, bcd);
        ([p[0], ab, abc, m], [m, bcd, cd, p[3]])
    }
    let right = if a > 0.0 { split(p, a).1 } else { p };
    if b >= 1.0 {
        return right;
    }
    let t = if a < 1.0 { (b - a) / (1.0 - a) } else { 1.0 };
    split(right, t).0
}

/// The curve as written: SMOOTH handles recomputed and clamped to the
/// attribute's own range (unbounded for repeat attributes); key values
/// as stored. spec: motion3-json.md §2, animation-model.md §3.2–3.3
/// The curve as the export reads it: reading the key list forces the
/// update: SMOOTH handles recomputed and clamped, then anchors clamped to
/// the attribute's range and rounded to 1/10000 (fresh 5.3.04
/// exports of hiyori_free: ParamCheek's stored 1 in range −1…0 is written
/// as 0; a stored 9.79949… as 9.8). spec: motion3-json.md §2 (Q-0029)
fn export_curve(a: &Attr, legacy: bool) -> KeyCurve {
    let mut c = a.curve.clone();
    if c.keys.is_empty() {
        return c;
    }
    let (lo, hi) = if a.repeat.is_some() {
        (f64::NEG_INFINITY, f64::INFINITY)
    } else {
        (a.range.0.min(a.range.1), a.range.0.max(a.range.1))
    };
    // Handles from the stored anchors first; then the anchors are clamped
    // and rounded to 1/10000 without moving their handles (m05: anchor
    // 9.79949… → 9.8, its flat handle stays 9.799).
    c.types.resize(c.keys.len(), CurveType::Smooth);
    crate::scene::smooth_handles(&mut c, legacy);
    crate::scene::clamp_handles(&mut c, lo, hi);
    for k in &mut c.keys {
        k.value = (k.value.clamp(lo, hi) * 10000.0).round() / 10000.0;
    }
    c
}

/// The segments of curve `c` over [s, e] with `d` decimals and value
/// map `value`. spec: motion3-json.md §2 "Segments of one curve"
fn keyed_curve(
    c: &KeyCurve,
    s: i32,
    e: i32,
    fps: f64,
    d: usize,
    value: &dyn Fn(f64) -> f64,
) -> ([f32; 2], Vec<Segment>) {
    let (sf, ef) = (f64::from(s), f64::from(e));
    let t = |f: f64| dec((f - sf) / fps, d);
    let v = |x: f64| dec(value(x), d);
    let k = &c.keys;
    let constant = |x: f64| ([0.0, v(x)], vec![Segment::Linear([t(ef), v(x)])]);
    if k.is_empty() {
        return constant(c.base);
    }
    if k.len() == 1 {
        return constant(k[0].value);
    }
    if k.iter().all(|x| f64::from(x.frame) <= sf) {
        return constant(k[k.len() - 1].value);
    }
    if k.iter().all(|x| f64::from(x.frame) >= ef) {
        return constant(k[0].value);
    }
    let mut segs = Vec::new();
    let f0 = f64::from(k[0].frame);
    let start = if f0 > sf {
        // A flat lead-in to the first key.
        segs.push(Segment::Linear([t(f0), v(k[0].value)]));
        [0.0, v(k[0].value)]
    } else {
        [0.0, v(eval_curve(c, sf))]
    };
    for i in 0..k.len() - 1 {
        let (fa, fb) = (f64::from(k[i].frame), f64::from(k[i + 1].frame));
        if fb <= sf || fa >= ef {
            continue;
        }
        let (ca, cb) = (fa.max(sf), fb.min(ef));
        let end_v = if cb < fb {
            eval_curve(c, cb)
        } else {
            k[i + 1].value
        };
        let end = [t(cb), v(end_v)];
        let seg = match c.type_at(i) {
            CurveType::Linear => Segment::Linear(end),
            CurveType::Step => Segment::Stepped(end),
            CurveType::InverseStep => Segment::InverseStepped(end),
            CurveType::Bezier | CurveType::Smooth => {
                let p = segment_points(c, i);
                let fx = [p[0].0, p[1].0, p[2].0, p[3].0];
                let (ua, ub) = (
                    if ca > fa { param_at(fx, ca) } else { 0.0 },
                    if cb < fb { param_at(fx, cb) } else { 1.0 },
                );
                let q = if ua > 0.0 || ub < 1.0 {
                    sub_bezier(p, ua, ub)
                } else {
                    p
                };
                Segment::Bezier {
                    c1: [t(q[1].0), v(q[1].1)],
                    c2: [t(q[2].0), v(q[2].1)],
                    end,
                }
            }
        };
        segs.push(seg);
    }
    let fl = f64::from(k[k.len() - 1].frame);
    if fl < ef {
        segs.push(Segment::Linear([t(ef), v(k[k.len() - 1].value)]));
    }
    (start, segs)
}

/// A curve sampled at every frame with stepped segments where the value
/// changes (frame-step export, §2 "Frame step").
fn sampled_curve(
    s: i32,
    e: i32,
    fps: f64,
    d: usize,
    v: &dyn Fn(f64) -> f64,
) -> ([f32; 2], Vec<Segment>) {
    let t = |f: i32| dec(f64::from(f - s) / fps, d);
    let start = [0.0, dec(v(f64::from(s)), d)];
    let mut segs = Vec::new();
    let mut last = start[1];
    for f in s + 1..=e {
        let x = dec(v(f64::from(f)), d);
        if x != last || f == e {
            segs.push(Segment::Stepped([t(f), x]));
            last = x;
        }
    }
    if segs.is_empty() {
        segs.push(Segment::Stepped([t(e), last]));
    }
    (start, segs)
}

/// Exports model track `ti` of scene `s` as a motion. `group_step(attr)`
/// gives the frame-step curve of the attribute's parameter group, if any;
/// `id_of(attr)` the model ID it targets (default: from the attribute ID).
pub fn export_motion(
    s: &Scene,
    ti: usize,
    o: &ExportOptions,
    group_step: &dyn Fn(&Attr) -> Option<IntCurve>,
    id_of: &dyn Fn(&Attr) -> Option<String>,
) -> Option<Motion> {
    let track: &Track = s.tracks.get(ti)?;
    let TrackKind::Model(m) = &track.kind else {
        return None;
    };
    let (start, end) = export_range(s, o);
    let fps = s.fps;
    let curve_of = |target: Target, id: String, a: &Attr, d: usize| -> Curve {
        let c = export_curve(a, o.legacy_smooth);
        let (p0, segs) = keyed_curve(&c, start, end, fps, d, &|v| v);
        Curve {
            target,
            id,
            fade_in: ms(a.fade_in_ms),
            fade_out: ms(a.fade_out_ms),
            start: p0,
            segments: segs,
        }
    };
    // Processing order (decides the digits left for Duration):
    // Parameters → PartOpacity → Model/Opacity → EyeBlink → LipSync.
    let mut last_digits = 3usize;
    let mut params = Vec::new();
    for a in &m.params {
        if !keep(a, o) {
            continue;
        }
        let Some(id) = id_of(a).or_else(|| param_id(&a.id).map(str::to_owned)) else {
            continue;
        };
        // Parameters with '@' are internal (§2).
        if id.contains('@') {
            continue;
        }
        let gs = group_step(a).or_else(|| track.frame_step.clone());
        let stepped = gs.as_ref().is_some_and(|c| !c.keys.is_empty());
        let curve = if stepped {
            let c = &a.curve;
            let (p0, segs) = sampled_curve(start, end, fps, 3, &|f| {
                eval_curve(c, step_frame(f, gs.as_ref()))
            });
            Curve {
                target: Target::Parameter,
                id,
                fade_in: ms(a.fade_in_ms),
                fade_out: ms(a.fade_out_ms),
                start: p0,
                segments: segs,
            }
        } else {
            curve_of(Target::Parameter, id, a, 3)
        };
        params.push(curve);
        last_digits = 3;
    }
    // Part curves sorted by part ID (observed).
    let mut part_attrs: Vec<&Attr> = m.parts.iter().filter(|a| keep(a, o)).collect();
    part_attrs.sort_by(|a, b| part_id(&a.id).cmp(&part_id(&b.id)));
    let mut parts = Vec::new();
    for a in part_attrs {
        let Some(id) = id_of(a).or_else(|| part_id(&a.id).map(str::to_owned)) else {
            continue;
        };
        parts.push(curve_of(Target::PartOpacity, id, a, 2));
        last_digits = 2;
    }
    // Model curves: the visual opacity (0–1, as is), eyeOpen, soundLevel.
    let mut model = Vec::new();
    if let Some(op) = track.opacity.as_ref().filter(|a| keep(a, o)) {
        model.push(curve_of(Target::Model, "Opacity".into(), op, 2));
        last_digits = 2;
    }
    if let Some(eb) = m.eye_blink.as_ref().filter(|e| keep(&e.eye_open, o)) {
        model.push(curve_of(Target::Model, "EyeBlink".into(), &eb.eye_open, 3));
        last_digits = 3;
    }
    if let Some(ls) = m.lip_sync.as_ref().filter(|e| keep(&e.sound_level, o)) {
        model.push(curve_of(
            Target::Model,
            "LipSync".into(),
            &ls.sound_level,
            3,
        ));
        last_digits = 3;
    }
    // Output order: Model curves, Parameters, PartOpacity.
    let mut curves = model;
    curves.extend(params);
    curves.extend(parts);
    let d = last_digits;
    Some(Motion {
        duration: dec(f64::from(end - start) / fps, d),
        fps: fps as f32,
        fade_in: ms(s.fade_in_ms),
        fade_out: ms(s.fade_out_ms),
        looped: true,
        beziers_restricted: s.bezier_restricted,
        curves,
        user_data: track
            .user_data
            .iter()
            .filter(|u| (start..=end).contains(&u.0))
            .map(|(f, v)| (dec(f64::from(f - start) / fps, d), v.clone()))
            .collect(),
    })
}

/// Integer curve value helper re-exported for callers.
pub fn int_at(c: &IntCurve, x: f64) -> i32 {
    eval_int(c, x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::animation::Key;

    fn key(frame: i32, value: f64) -> Key {
        Key {
            frame,
            value,
            ..Default::default()
        }
    }

    #[test]
    fn segment_rules() {
        let c = KeyCurve {
            keys: vec![key(10, 1.0), key(20, 3.0), key(40, 5.0)],
            types: vec![CurveType::Linear, CurveType::Smooth, CurveType::Smooth],
            ..Default::default()
        };
        // First key after start: flat lead-in; last key before end: tail.
        let (p0, s) = keyed_curve(&c, 0, 50, 10.0, 3, &|v| v);
        assert_eq!(p0, [0.0, 1.0]);
        assert_eq!(s[0], Segment::Linear([1.0, 1.0]));
        assert_eq!(s[1], Segment::Linear([2.0, 3.0]));
        assert!(matches!(
            s[2],
            Segment::Bezier {
                end: [4.0, 5.0],
                ..
            }
        ));
        assert_eq!(s[3], Segment::Linear([5.0, 5.0]));
        // Cut at both ends: the range starts inside the linear segment and
        // ends inside the Bezier one.
        let (p0, s) = keyed_curve(&c, 15, 30, 10.0, 3, &|v| v);
        assert_eq!(p0, [0.0, 2.0]);
        assert_eq!(s[0], Segment::Linear([0.5, 3.0]));
        let Segment::Bezier { end, .. } = s[1] else {
            panic!()
        };
        assert!((end[0] - 1.5).abs() < 1e-6);
        assert!((f64::from(end[1]) - eval_curve(&c, 30.0)).abs() < 1e-3);
        assert_eq!(s.len(), 2);
        // All keys before the range: constant at the last key.
        let (p0, s) = keyed_curve(&c, 45, 50, 10.0, 3, &|v| v);
        assert_eq!((p0, s), ([0.0, 5.0], vec![Segment::Linear([0.5, 5.0])]));
    }

    #[test]
    fn bezier_cut_matches_curve() {
        let p = [(0.0, 0.0), (1.0, 2.0), (2.0, -1.0), (3.0, 1.0)];
        let q = sub_bezier(p, 0.25, 0.75);
        let fx = [p[0].0, p[1].0, p[2].0, p[3].0];
        let fy = [p[0].1, p[1].1, p[2].1, p[3].1];
        assert!((q[0].1 - cubic(fy, 0.25)).abs() < 1e-12);
        assert!((q[3].0 - cubic(fx, 0.75)).abs() < 1e-12);
    }
}
