//! motion3.json export from an animation scene.
//! spec: 60-animation/motion3-json.md §2, animation-model.md §6

use keyform_model::animation::{Attr, CurveType, IntCurve, Scene, Track, TrackKind};

use crate::motion::{Curve, Motion, Segment, Target};
use crate::scene::{eval_curve, eval_int, param_id, part_id, step_frame};

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
    /// The document's `isSmoothCurveLegacyMode`: recomputed SMOOTH handles
    /// are clamped to the range (§3.3). TODO(Q-0028)
    pub legacy_smooth: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            work_area: true,
            filter: CurveFilter::WithKeys,
            exclude_shy: false,
            exclude_frozen: false,
            legacy_smooth: false,
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

/// The curve of attribute `a` over [s, e] from its keys: the first point
/// at time 0, one segment per key segment (LINEAR → linear, BEZIER and
/// SMOOTH → bezier through the handles, STEP → stepped, INVERSE_STEP →
/// inverse stepped), held linearly to the end. `value` maps the curve
/// value (clamping or effects). spec: motion3-json.md §1–2
/// Rounds in f64 to `d` decimals the way the file is written (exact
/// binary value, ties to even) before narrowing to the motion's f32, so
/// the written text is the reference's.
fn dec(v: f64, d: usize) -> f32 {
    format!("{v:.d$}").parse::<f64>().unwrap_or(0.0) as f32
}

#[allow(clippy::too_many_arguments)]
fn keyed_curve(
    a: &Attr,
    s: i32,
    e: i32,
    fps: f64,
    d: usize,
    clamp: bool,
    value: &dyn Fn(f64, f64) -> f64,
) -> ([f32; 2], Vec<Segment>) {
    let t = |f: f64| dec((f - f64::from(s)) / fps, d);
    let value = |v: f64, f: f64| f64::from(dec(value(v, f), d));
    // SMOOTH handles are recomputed (§3.2), as the reference's exports
    // show (flat keys' handles are exactly their value, others match the
    // recomputation to the last digit). TODO(Q-0028): the validity clamp.
    let mut smooth = a.curve.clone();
    if smooth.types.contains(&CurveType::Smooth) {
        crate::scene::smooth_handles(&mut smooth, false);
        if clamp {
            crate::scene::clamp_handles(
                &mut smooth,
                a.range.0.min(a.range.1),
                a.range.0.max(a.range.1),
            );
        }
    }
    let c = &smooth;
    let keys: Vec<usize> = (0..c.keys.len())
        .filter(|&i| (s..=e).contains(&c.keys[i].frame))
        .collect();
    let at = |f: f64| value(eval_curve(c, f), f) as f32;
    let mut segs = Vec::new();
    let start = [0.0, at(f64::from(s))];
    // A single key: its value held over the whole range, wherever the key
    // sits (observed).
    if keys.len() <= 1 {
        let v = keys.first().map_or(start[1], |&i| {
            value(c.keys[i].value, f64::from(c.keys[i].frame)) as f32
        });
        return ([0.0, v], vec![Segment::Linear([t(f64::from(e)), v])]);
    }
    let mut cur = f64::from(s);
    if let Some(&i0) = keys.first() {
        let f0 = f64::from(c.keys[i0].frame);
        if f0 > cur {
            segs.push(Segment::Linear([t(f0), at(f0)]));
            cur = f0;
        }
    }
    for w in keys.windows(2) {
        let (i, j) = (w[0], w[1]);
        let (ki, kj) = (&c.keys[i], &c.keys[j]);
        let end = [
            t(f64::from(kj.frame)),
            value(kj.value, f64::from(kj.frame)) as f32,
        ];
        let seg = match c.type_at(i) {
            CurveType::Linear => Segment::Linear(end),
            CurveType::Step => Segment::Stepped(end),
            CurveType::InverseStep => Segment::InverseStepped(end),
            CurveType::Bezier | CurveType::Smooth => {
                let p = crate::scene::segment_points(c, i);
                let fi = f64::from(ki.frame);
                Segment::Bezier {
                    c1: [t(p[1].0), value(p[1].1, fi) as f32],
                    c2: [t(p[2].0), value(p[2].1, fi) as f32],
                    end,
                }
            }
        };
        segs.push(seg);
        cur = f64::from(kj.frame);
    }
    if cur < f64::from(e) || segs.is_empty() {
        segs.push(Segment::Linear([
            t(f64::from(e)),
            at(f64::from(e).max(cur)),
        ]));
    }
    (start, segs)
}

/// A curve sampled at every frame with stepped segments where the value
/// changes (frame-step export, §2 "Frame step").
fn sampled_curve(s: i32, e: i32, fps: f64, v: &dyn Fn(f64) -> f64) -> ([f32; 2], Vec<Segment>) {
    let t = |f: i32| (f64::from(f - s) / fps) as f32;
    let start = [0.0, v(f64::from(s)) as f32];
    let mut segs = Vec::new();
    let mut last = start[1];
    for f in s + 1..=e {
        let x = v(f64::from(f)) as f32;
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
    let mut curves = Vec::new();
    for (k, a) in m.params.iter().enumerate() {
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
        // Eye blink and lip sync are exported as Model curves, so the
        // parameter curve is the plain curve (§2 "Targets"). Key values
        // are written as stored: the reference doesn't clamp them to the
        // attribute's (possibly stale) range.
        let (p0, segs) = if stepped {
            let c = &a.curve;
            sampled_curve(start, end, fps, &|f| {
                eval_curve(c, step_frame(f, gs.as_ref()))
            })
        } else {
            keyed_curve(a, start, end, fps, 3, o.legacy_smooth, &|v, _| v)
        };
        let _ = k;
        curves.push(Curve {
            target: Target::Parameter,
            id,
            fade_in: ms(a.fade_in_ms),
            fade_out: ms(a.fade_out_ms),
            start: p0,
            segments: segs,
        });
    }
    // Part curves come out sorted by part ID (observed on chitose, whose
    // model and scene both list them in another order). TODO(Q-0028)
    let mut parts: Vec<&Attr> = m.parts.iter().collect();
    parts.sort_by(|a, b| part_id(&a.id).cmp(&part_id(&b.id)));
    for a in parts {
        if !keep(a, o) {
            continue;
        }
        let Some(id) = id_of(a).or_else(|| part_id(&a.id).map(str::to_owned)) else {
            continue;
        };
        let (p0, segs) = keyed_curve(a, start, end, fps, 2, o.legacy_smooth, &|v, _| v);
        curves.push(Curve {
            target: Target::PartOpacity,
            id,
            fade_in: ms(a.fade_in_ms),
            fade_out: ms(a.fade_out_ms),
            start: p0,
            segments: segs,
        });
    }
    // Model curves: the track's visual opacity, the effects' drivers.
    // TODO(Q-0028): which effect attributes become Model/EyeBlink and
    // Model/LipSync.
    let mut model_curve = |id: &str, a: &Attr, scale: f64| {
        if !keep(a, o) {
            return;
        }
        let d = if id == "Opacity" { 2 } else { 3 };
        let (p0, segs) = keyed_curve(a, start, end, fps, d, o.legacy_smooth, &|v, _| v * scale);
        curves.push(Curve {
            target: Target::Model,
            id: id.to_owned(),
            fade_in: None,
            fade_out: None,
            start: p0,
            segments: segs,
        });
    };
    if let Some(op) = &track.opacity {
        // The visual opacity is in percent when its range goes past 1.
        model_curve("Opacity", op, if op.range.1 > 1.0 { 0.01 } else { 1.0 });
    }
    if let Some(eb) = &m.eye_blink {
        model_curve("EyeBlink", &eb.eye_open, 1.0);
    }
    if let Some(ls) = &m.lip_sync {
        model_curve("LipSync", &ls.sound_level, 1.0);
    }
    Some(Motion {
        duration: (f64::from(end - start) / fps) as f32,
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
            .map(|(f, v)| ((f64::from(f - start) / fps) as f32, v.clone()))
            .collect(),
    })
}

/// Integer curve value helper re-exported for callers.
pub fn int_at(c: &IntCurve, x: f64) -> i32 {
    eval_int(c, x)
}
