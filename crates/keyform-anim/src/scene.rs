//! Animation scenes: keyframe curve evaluation, smooth handles, frame step
//! and the model-track effects (eye blink, lip sync).
//! spec: 60-animation/animation-model.md §3–5

use keyform_model::animation::{
    Attr, Curve, CurveType, EyeBlinkEffect, Handle, IntCurve, Key, LipSyncEffect, ModelTrack,
};

/// The Bézier control points of segment i (anchor, next handle, prev
/// handle of the following key, anchor); missing handles sit on the
/// anchors at ⅓ / ⅔ of the segment.
pub fn segment_points(c: &Curve, i: usize) -> [(f64, f64); 4] {
    let (a, b) = (&c.keys[i], &c.keys[i + 1]);
    let (fa, fb) = (f64::from(a.frame), f64::from(b.frame));
    let n = a.next.map_or((fa + (fb - fa) / 3.0, a.value), |h| {
        (f64::from(h.frame), h.value)
    });
    let p = b.prev.map_or((fb - (fb - fa) / 3.0, b.value), |h| {
        (f64::from(h.frame), h.value)
    });
    [(fa, a.value), n, p, (fb, b.value)]
}

fn cubic(p: [f64; 4], t: f64) -> f64 {
    let u = 1.0 - t;
    u * u * u * p[0] + 3.0 * u * u * t * p[1] + 3.0 * u * t * t * p[2] + t * t * t * p[3]
}

/// Value of the Bézier segment at frame x: solve frame(t) = x (monotonic,
/// handles are clamped to the segment, §3.3), then value(t).
pub fn bezier_value(pts: [(f64, f64); 4], x: f64) -> f64 {
    let fx = [pts[0].0, pts[1].0, pts[2].0, pts[3].0];
    let fy = [pts[0].1, pts[1].1, pts[2].1, pts[3].1];
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if cubic(fx, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    cubic(fy, 0.5 * (lo + hi))
}

/// The curve's (unclamped) value at frame x. spec: §3.1
pub fn eval_curve(c: &Curve, x: f64) -> f64 {
    let k = &c.keys;
    match k.len() {
        0 => return c.base,
        1 => return k[0].value,
        _ => {}
    }
    if x <= f64::from(k[0].frame) {
        return k[0].value;
    }
    let last = k.len() - 1;
    if x >= f64::from(k[last].frame) {
        return k[last].value;
    }
    let i = k.partition_point(|p| f64::from(p.frame) <= x) - 1;
    let (a, b) = (&k[i], &k[i + 1]);
    let (fa, fb) = (f64::from(a.frame), f64::from(b.frame));
    match c.type_at(i) {
        CurveType::Linear => a.value + (b.value - a.value) * (x - fa) / (fb - fa),
        CurveType::Step => a.value,
        CurveType::InverseStep => {
            if x > fa {
                b.value
            } else {
                a.value
            }
        }
        CurveType::Bezier | CurveType::Smooth => bezier_value(segment_points(c, i), x),
    }
}

/// An attribute's value at frame x, clamped to its range (§3.1 "Clamp").
pub fn eval_attr(a: &Attr, x: f64) -> f64 {
    eval_curve(&a.curve, x).clamp(a.range.0.min(a.range.1), a.range.1.max(a.range.0))
}

/// An integer curve at frame x: the last key at or before x (the first
/// key before it), else the base value.
pub fn eval_int(c: &IntCurve, x: f64) -> i32 {
    match c.keys.iter().rev().find(|k| f64::from(k.0) <= x) {
        Some(k) => k.1,
        None => c.keys.first().map_or(c.base, |k| k.1),
    }
}

/// Frame step (§3.1a step 2): the frame x adjusted by frame-step keys.
pub fn step_frame(x: f64, step: Option<&IntCurve>) -> f64 {
    let Some(c) = step.filter(|c| !c.keys.is_empty()) else {
        return x;
    };
    let f0 = f64::from(c.keys[0].0);
    if x < f0 {
        return x.trunc();
    }
    let Some(&(fk, s)) = c.keys.iter().rev().find(|k| f64::from(k.0) <= x) else {
        return x;
    };
    let fk = f64::from(fk);
    if s == 0 {
        return fk;
    }
    let s = f64::from(s);
    fk + ((x.floor() - fk) / s).floor() * s
}

// ------------------------------------------------------------ smooth handles

fn is_bezier(c: &Curve, seg: usize) -> bool {
    c.type_at(seg) == CurveType::Bezier
}

/// The keys whose automatic handles are flat (zero slope): the first and
/// last keys, and keys equal to a neighbour or a strict local extremum.
/// spec: §3.2 step 2
pub fn flat_keys(c: &Curve) -> Vec<bool> {
    let n = c.keys.len();
    (0..n)
        .map(|i| {
            if i == 0 || i + 1 == n {
                return true;
            }
            let (p, v, q) = (c.keys[i - 1].value, c.keys[i].value, c.keys[i + 1].value);
            v == p || v == q || (v < p && v < q) || (v > p && v > q)
        })
        .collect()
}

/// Recomputes the automatic handles of SMOOTH keys. spec: §3.2 (steps 1–3;
/// `legacy` adds step 4's force-fix handling, without force-fix flags
/// this only skips keys whose own segment is BEZIER)
pub fn smooth_handles(c: &mut Curve, legacy: bool) {
    let n = c.keys.len();
    if n < 2 {
        return;
    }
    let flat = flat_keys(c);
    let keys = c.keys.clone();
    for i in 0..n {
        // Sides: prev segment = i − 1, next segment = i.
        let left_free = i > 0 && !is_bezier(c, i - 1);
        let right_free = i + 1 < n && !is_bezier(c, i);
        if !left_free && !right_free {
            continue;
        }
        if legacy && i + 1 < n && is_bezier(c, i) {
            continue;
        }
        let k = keys[i];
        let pos = f64::from(k.frame);
        let v = k.value;
        let key = &mut c.keys[i];
        // 1. Handle frames.
        if i > 0 && (left_free || key.prev.is_none()) {
            let pp = f64::from(keys[i - 1].frame);
            let f = (pos - (pos - pp) / 3.0) as f32;
            key.prev = Some(Handle {
                frame: f,
                value: key.prev.map_or(v, |h| h.value),
            });
        }
        if i + 1 < n && (right_free || key.next.is_none()) {
            let np = f64::from(keys[i + 1].frame);
            let f = (pos + (np - pos) / 3.0) as f32;
            key.next = Some(Handle {
                frame: f,
                value: key.next.map_or(v, |h| h.value),
            });
        }
        // 2. Flat keys: zero slope on the free sides.
        if flat[i] {
            if left_free {
                if let Some(h) = key.prev.as_mut() {
                    h.value = v;
                }
            }
            if right_free {
                if let Some(h) = key.next.as_mut() {
                    h.value = v;
                }
            }
            continue;
        }
        // 3. Interior keys: weighted slope angle.
        let (pk, nk) = (keys[i - 1], keys[i + 1]);
        let side = |dx: f64, dv: f64, neighbour_flat: bool| -> (f64, f64, f64) {
            let dy = dv * if neighbour_flat { 2.0 / 3.0 } else { 1.0 / 3.0 };
            let a = dy.atan2(dx);
            let w = dx / (a.abs() / std::f64::consts::FRAC_PI_2);
            (dx, a, w)
        };
        let (dx_l, a_l, w_l) = side((pos - f64::from(pk.frame)) / 3.0, v - pk.value, flat[i - 1]);
        let (dx_r, a_r, w_r) = side((f64::from(nk.frame) - pos) / 3.0, nk.value - v, flat[i + 1]);
        let a = if a_l == 0.0 || a_r == 0.0 {
            0.0
        } else {
            let a = (a_l * w_l + a_r * w_r) / (w_l + w_r);
            if a.is_nan() {
                0.0
            } else {
                a
            }
        };
        if left_free {
            if let Some(h) = key.prev.as_mut() {
                h.value = v - dx_l * a.tan();
            }
        }
        if right_free {
            if let Some(h) = key.next.as_mut() {
                h.value = v + dx_r * a.tan();
            }
        }
    }
}

/// Validity clamps (§3.3) for the handles: each must lie within its
/// segment's frame span and the value range [lo, hi]; one that doesn't is
/// pulled back along the line from its anchor until it fits, else clamped
/// per axis.
pub fn clamp_handles(c: &mut Curve, lo: f64, hi: f64) {
    let n = c.keys.len();
    let frames: Vec<f64> = c.keys.iter().map(|k| f64::from(k.frame)).collect();
    for i in 0..n {
        let (af, av) = (frames[i], c.keys[i].value);
        let fit = |h: &mut Handle, f0: f64, f1: f64| {
            let (hf, hv) = (f64::from(h.frame), h.value);
            let inside = |f: f64, v: f64| {
                f >= f0 - 1e-9 && f <= f1 + 1e-9 && v >= lo - 1e-9 && v <= hi + 1e-9
            };
            if inside(hf, hv) {
                return;
            }
            // Largest s in [0, 1] with anchor + s·(h − anchor) inside.
            let mut s = 1.0f64;
            let (df, dv) = (hf - af, hv - av);
            if df > 0.0 && af + df > f1 {
                s = s.min((f1 - af) / df);
            }
            if df < 0.0 && af + df < f0 {
                s = s.min((f0 - af) / df);
            }
            if dv > 0.0 && av + dv > hi {
                s = s.min((hi - av) / dv);
            }
            if dv < 0.0 && av + dv < lo {
                s = s.min((lo - av) / dv);
            }
            let (f, v) = if s >= 0.0 && inside(af + s * df, av + s * dv) {
                (af + s * df, av + s * dv)
            } else {
                (hf.clamp(f0, f1), hv.clamp(lo, hi))
            };
            h.frame = f as f32;
            h.value = v;
        };
        if i > 0 {
            if let Some(h) = c.keys[i].prev.as_mut() {
                fit(h, frames[i - 1], af);
            }
        }
        if i + 1 < n {
            if let Some(h) = c.keys[i].next.as_mut() {
                fit(h, af, frames[i + 1]);
            }
        }
    }
}

// -------------------------------------------------------------- key editing

/// The clamp range of an attribute's curve: its own stored range,
/// unbounded for repeat attributes (Q-0028).
pub fn clamp_range(a: &keyform_model::animation::Attr) -> (f64, f64) {
    if a.repeat.is_some() {
        (f64::NEG_INFINITY, f64::INFINITY)
    } else {
        (a.range.0.min(a.range.1), a.range.0.max(a.range.1))
    }
}

/// After every edit (§3.2–3.3): anchors clamped to [lo, hi] (handles move
/// with them) and rounded to 1/10000, SMOOTH handles recomputed, handles
/// clamped to their segment and the range.
pub fn refresh(c: &mut Curve, lo: f64, hi: f64, legacy: bool) {
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    for k in &mut c.keys {
        let v = (k.value.clamp(lo, hi) * 10000.0).round() / 10000.0;
        let d = v - k.value;
        if d != 0.0 {
            for h in [k.prev.as_mut(), k.next.as_mut()].into_iter().flatten() {
                h.value += d;
            }
        }
        k.value = v;
    }
    c.types.resize(c.keys.len(), CurveType::Smooth);
    smooth_handles(c, legacy);
    clamp_handles(c, lo, hi);
    // Every key carries both handles (the end keys' outer ones on the
    // anchor), as stored by the reference.
    for k in &mut c.keys {
        let at = Handle {
            frame: k.frame as f32,
            value: k.value,
        };
        k.prev.get_or_insert(at);
        k.next.get_or_insert(at);
    }
}

/// Sets a key at `frame` to `value` (inserting one with curve type `t`
/// when there is none). Returns its index.
pub fn set_key(c: &mut Curve, frame: i32, value: f64, t: CurveType) -> usize {
    match c.keys.binary_search_by_key(&frame, |k| k.frame) {
        Ok(i) => {
            c.keys[i].value = value;
            i
        }
        Err(i) => {
            c.keys.insert(
                i,
                Key {
                    frame,
                    value,
                    ..Default::default()
                },
            );
            c.types.insert(i.min(c.types.len()), t);
            i
        }
    }
}

/// Moves key `i` to (`frame`, `value`); refused (false) when another key
/// sits on that frame. Handles move with the anchor.
pub fn move_key(c: &mut Curve, i: usize, frame: i32, value: f64) -> bool {
    if i >= c.keys.len()
        || c.keys
            .iter()
            .enumerate()
            .any(|(j, k)| j != i && k.frame == frame)
    {
        return false;
    }
    let mut k = c.keys.remove(i);
    let t = if i < c.types.len() {
        c.types.remove(i)
    } else {
        CurveType::Smooth
    };
    let (df, dv) = ((frame - k.frame) as f32, value - k.value);
    for h in [k.prev.as_mut(), k.next.as_mut()].into_iter().flatten() {
        h.frame += df;
        h.value += dv;
    }
    k.frame = frame;
    k.value = value;
    let j = c.keys.partition_point(|x| x.frame < frame);
    c.keys.insert(j, k);
    c.types.insert(j.min(c.types.len()), t);
    true
}

pub fn delete_key(c: &mut Curve, i: usize) {
    if i < c.keys.len() {
        c.keys.remove(i);
        if i < c.types.len() {
            c.types.remove(i);
        }
    }
}

// --------------------------------------------------------------- placement

/// A point curve at frame x: linear between its keys (held before the
/// first and after the last), else its base point.
// TODO(Q-0030): the TNS (tension / continuity / bias) interpolation.
pub fn eval_point(c: &keyform_model::animation::PointCurve, x: f64) -> [f32; 2] {
    let k = &c.keys;
    match k.len() {
        0 => return c.base,
        1 => return k[0].1,
        _ => {}
    }
    if x <= f64::from(k[0].0) {
        return k[0].1;
    }
    if x >= f64::from(k[k.len() - 1].0) {
        return k[k.len() - 1].1;
    }
    let i = k.partition_point(|p| f64::from(p.0) <= x) - 1;
    let (a, b) = (k[i], k[i + 1]);
    let t = ((x - f64::from(a.0)) / f64::from(b.0 - a.0)) as f32;
    [
        a.1[0] + (b.1[0] - a.1[0]) * t,
        a.1[1] + (b.1[1] - a.1[1]) * t,
    ]
}

/// Where a track sits in the scene at one frame (its visual effect).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub xy: [f32; 2],
    pub anchor: [f32; 2],
    /// Scale factors (1 = 100 %).
    pub scale: [f32; 2],
    /// Degrees, clockwise on screen.
    pub rotate: f32,
    pub shear: f32,
    /// 0–1.
    pub opacity: f32,
}

impl Placement {
    pub const IDENTITY: Placement = Placement {
        xy: [0.0, 0.0],
        anchor: [0.0, 0.0],
        scale: [1.0, 1.0],
        rotate: 0.0,
        shear: 0.0,
        opacity: 1.0,
    };

    /// Track point → scene point: xy + R(rotate)·Shear·S(scale)·(p − anchor).
    // TODO(Q-0030): the order and the shear's meaning are provisional.
    pub fn apply(&self, p: [f32; 2]) -> [f32; 2] {
        let mut q = [
            (p[0] - self.anchor[0]) * self.scale[0],
            (p[1] - self.anchor[1]) * self.scale[1],
        ];
        if self.shear != 0.0 {
            q[0] += self.shear.to_radians().tan() * q[1];
        }
        if self.rotate != 0.0 {
            let (s, c) = self.rotate.to_radians().sin_cos();
            q = [c * q[0] - s * q[1], s * q[0] + c * q[1]];
        }
        [self.xy[0] + q[0], self.xy[1] + q[1]]
    }
}

/// The track's placement at frame x; identity without a visual effect.
/// spec: animation-model.md §2, §5 step 5
pub fn placement(t: &keyform_model::animation::Track, x: f64) -> Placement {
    let Some(v) = t.visual.as_ref() else {
        return Placement::IDENTITY;
    };
    let pct = |a: &Attr| eval_curve(&a.curve, x) as f32 / 100.0;
    let opacity = t.opacity.as_ref().map_or(1.0, |o| {
        let v = eval_attr(o, x) as f32;
        if o.range.1 > 1.0 {
            v / 100.0
        } else {
            v
        }
    });
    Placement {
        xy: eval_point(&v.xy, x),
        anchor: eval_point(&v.anchor, x),
        scale: [pct(&v.scale_x), pct(&v.scale_y)],
        rotate: eval_curve(&v.rotate.curve, x) as f32,
        shear: eval_curve(&v.shear.curve, x) as f32,
        opacity: opacity.clamp(0.0, 1.0),
    }
}

// ----------------------------------------------------------- form animation

/// The form difference D(t) of an object's keys (target − original per
/// value): after the last key the last one's, on or before the first
/// key that key's, else linear between the surrounding keys.
/// spec: form-animation-and-controllers.md §1 step 2
pub fn form_difference(o: &keyform_model::animation::FormObject, f: f64) -> Option<Vec<f32>> {
    let k = &o.keys;
    if k.is_empty() {
        return None;
    }
    let t = match &o.time_remap {
        Some(a) if !a.curve.keys.is_empty() => eval_curve(&a.curve, f),
        _ => f,
    };
    let diff = |key: &keyform_model::animation::FormKey| -> Vec<f32> {
        key.target
            .iter()
            .zip(&key.original)
            .map(|(a, b)| a - b)
            .collect()
    };
    let last = k.len() - 1;
    if t >= f64::from(k[last].frame) {
        return Some(diff(&k[last]));
    }
    if t <= f64::from(k[0].frame) {
        return Some(diff(&k[0]));
    }
    let i = k.partition_point(|x| f64::from(x.frame) <= t) - 1;
    let (a, b) = (&k[i], &k[i + 1]);
    if t == f64::from(a.frame) {
        return Some(diff(a));
    }
    let w = ((t - f64::from(a.frame)) / f64::from(b.frame - a.frame)) as f32;
    Some(
        diff(a)
            .iter()
            .zip(diff(b))
            .map(|(x, y)| (1.0 - w) * x + w * y)
            .collect(),
    )
}

// ------------------------------------------------------------------ effects

/// Eye blink result for parameter value v in [lo, hi] at frame x. §4
pub fn eye_blink(e: &EyeBlinkEffect, v: f64, lo: f64, hi: f64, x: f64) -> f64 {
    if !e.active {
        return v.clamp(lo, hi);
    }
    let l = eval_curve(&e.level.curve, x) / 100.0;
    let o = 1.0 - eval_curve(&e.eye_open.curve, x);
    let r = hi - lo;
    let target = match (e.relative, e.invert) {
        (true, false) => v - o * r,
        (true, true) => v + o * r,
        (false, false) => hi - o * r,
        (false, true) => lo + o * r,
    };
    (v + (target - v) * l).clamp(lo, hi)
}

/// Lip sync result: the sound level from the linked sound (`sound`) or
/// else the soundLevel curve. §4
pub fn lip_sync(e: &LipSyncEffect, v: f64, lo: f64, hi: f64, x: f64, sound: Option<f64>) -> f64 {
    if !e.active {
        return v.clamp(lo, hi);
    }
    let l = eval_curve(&e.level.curve, x) / 100.0;
    let scale = eval_curve(&e.scale.curve, x);
    let base = eval_curve(&e.base.curve, x);
    let level = sound.unwrap_or_else(|| eval_curve(&e.sound_level.curve, x));
    let o = ((level - base) * scale).clamp(0.0, 1.0);
    let r = hi - lo;
    let target = match (e.relative, e.invert) {
        (true, false) => v + o * r,
        (true, true) => v - o * r,
        (false, false) => lo + o * r,
        (false, true) => lo + (1.0 - o) * r,
    };
    (v + (target - v) * l).clamp(lo, hi)
}

/// Parameter attribute `k` of `m` at track frame x (already frame-stepped
/// for its group), with effect selection: lip sync, else eye blink, else
/// the clamped curve value. spec: §4 "Effect selection"
pub fn param_value(m: &ModelTrack, k: usize, x: f64) -> f64 {
    param_value_with(m, k, x, None)
}

/// [`param_value`] with the lip-sync level from a sound (`sound`).
pub fn param_value_with(m: &ModelTrack, k: usize, x: f64, sound: Option<f64>) -> f64 {
    let a = &m.params[k];
    let (lo, hi) = (a.range.0.min(a.range.1), a.range.0.max(a.range.1));
    let v = eval_curve(&a.curve, x);
    if let Some(e) = m.lip_sync.as_ref().filter(|e| e.targets.contains(&a.id)) {
        return lip_sync(e, v, lo, hi, x, sound);
    }
    if let Some(e) = m.eye_blink.as_ref().filter(|e| e.targets.contains(&a.id)) {
        return eye_blink(e, v, lo, hi, x);
    }
    v.clamp(lo, hi)
}

/// The volume level of mono samples at `rate` over [t0, t1) seconds: the
/// RMS (0–1). spec: animation-model.md §4 ("volume envelope")
// TODO(Q-0031): the reference's envelope (window, RMS or peak).
pub fn sound_level(samples: &[f32], rate: u32, t0: f64, t1: f64) -> f64 {
    let r = f64::from(rate);
    let a = (t0 * r).floor().max(0.0) as usize;
    let b = ((t1 * r).ceil().max(0.0) as usize).min(samples.len());
    if a >= b {
        return 0.0;
    }
    let sum: f64 = samples[a..b]
        .iter()
        .map(|&x| f64::from(x) * f64::from(x))
        .sum();
    (sum / (b - a) as f64).sqrt().clamp(0.0, 1.0)
}

/// The model parameter ID an attribute animates (`live2dParam_<Id>`).
pub fn param_id(attr_id: &str) -> Option<&str> {
    attr_id.strip_prefix("live2dParam_")
}

/// The part ID of a part opacity attribute (`live2DPartsOpacity_<Id>`).
pub fn part_id(attr_id: &str) -> Option<&str> {
    attr_id.strip_prefix("live2DPartsOpacity_")
}

/// Whether two keys lists are the same up to handles (for tests).
pub fn same_anchors(a: &[Key], b: &[Key]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.frame == y.frame && x.value == y.value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(frame: i32, value: f64) -> Key {
        Key {
            frame,
            value,
            ..Default::default()
        }
    }

    #[test]
    fn curve_types() {
        let mut c = Curve {
            keys: vec![key(0, 0.0), key(10, 10.0), key(20, 0.0)],
            types: vec![CurveType::Linear, CurveType::Step, CurveType::Smooth],
            ..Default::default()
        };
        assert_eq!(eval_curve(&c, -5.0), 0.0);
        assert!((eval_curve(&c, 5.0) - 5.0).abs() < 1e-12);
        assert_eq!(eval_curve(&c, 15.0), 10.0);
        assert_eq!(eval_curve(&c, 30.0), 0.0);
        c.types[1] = CurveType::InverseStep;
        assert_eq!(eval_curve(&c, 10.0), 10.0);
        assert_eq!(eval_curve(&c, 10.5), 0.0);
        // Smooth with default handles at ⅓: symmetric about the middle.
        c.types[1] = CurveType::Smooth;
        smooth_handles(&mut c, false);
        let (a, b) = (eval_curve(&c, 12.0), eval_curve(&c, 18.0));
        assert!((a - 10.0).abs() > 0.1 && (b - 0.0).abs() > 0.1);
        // The middle key is an extremum: flat handles.
        assert_eq!(c.keys[1].prev.unwrap().value, 10.0);
        assert_eq!(c.keys[1].next.unwrap().value, 10.0);
    }

    #[test]
    fn frame_step() {
        let s = IntCurve {
            base: 1,
            keys: vec![(10, 3), (30, 0)],
        };
        assert_eq!(step_frame(5.5, Some(&s)), 5.0);
        assert_eq!(step_frame(10.0, Some(&s)), 10.0);
        assert_eq!(step_frame(12.9, Some(&s)), 10.0);
        assert_eq!(step_frame(13.0, Some(&s)), 13.0);
        assert_eq!(step_frame(35.0, Some(&s)), 30.0);
        assert_eq!(step_frame(7.3, None), 7.3);
    }

    #[test]
    fn key_editing() {
        let mut c = Curve::default();
        assert_eq!(set_key(&mut c, 10, 1.0, CurveType::Linear), 0);
        assert_eq!(set_key(&mut c, 0, 0.0, CurveType::Smooth), 0);
        assert_eq!(set_key(&mut c, 20, 50.0, CurveType::Smooth), 2);
        refresh(&mut c, -10.0, 10.0, false);
        assert_eq!(c.keys[2].value, 10.0, "clamped to the range");
        assert_eq!(
            c.types,
            vec![CurveType::Smooth, CurveType::Linear, CurveType::Smooth]
        );
        assert!(!move_key(&mut c, 0, 10, 3.0), "frame taken");
        assert!(move_key(&mut c, 0, 15, 3.0));
        assert_eq!(
            c.keys.iter().map(|k| k.frame).collect::<Vec<_>>(),
            vec![10, 15, 20]
        );
        assert_eq!(c.types[1], CurveType::Smooth, "the type moves with its key");
        delete_key(&mut c, 0);
        assert_eq!(c.keys.len(), 2);
        refresh(&mut c, -10.0, 10.0, false);
        // Handles within their segments.
        assert!(c.keys[0].next.unwrap().frame <= 20.0);
    }

    #[test]
    fn placement_maps_anchor_to_position() {
        let p = Placement {
            xy: [100.0, 50.0],
            anchor: [10.0, 20.0],
            scale: [0.5, 0.5],
            rotate: 90.0,
            shear: 0.0,
            opacity: 1.0,
        };
        assert_eq!(p.apply([10.0, 20.0]), [100.0, 50.0]);
        // (12, 20): 2 px right of the anchor → 1 px, turned 90° clockwise
        // (down on screen).
        let q = p.apply([12.0, 20.0]);
        assert!(
            (q[0] - 100.0).abs() < 1e-5 && (q[1] - 51.0).abs() < 1e-5,
            "{q:?}"
        );
    }

    #[test]
    fn form_differences() {
        use keyform_model::animation::{FormKey, FormObject};
        let o = FormObject {
            guid: String::new(),
            keys: vec![
                FormKey {
                    frame: 10,
                    original: vec![0.0, 0.0],
                    target: vec![2.0, 0.0],
                },
                FormKey {
                    frame: 20,
                    original: vec![0.0, 0.0],
                    target: vec![4.0, 2.0],
                },
            ],
            time_remap: None,
        };
        assert_eq!(form_difference(&o, 0.0).unwrap(), vec![2.0, 0.0]);
        assert_eq!(form_difference(&o, 15.0).unwrap(), vec![3.0, 1.0]);
        assert_eq!(form_difference(&o, 30.0).unwrap(), vec![4.0, 2.0]);
    }
}
