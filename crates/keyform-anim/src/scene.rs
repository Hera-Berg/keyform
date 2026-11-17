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

/// Lip sync result (the sound level from its attribute curve). §4
pub fn lip_sync(e: &LipSyncEffect, v: f64, lo: f64, hi: f64, x: f64) -> f64 {
    if !e.active {
        return v.clamp(lo, hi);
    }
    let l = eval_curve(&e.level.curve, x) / 100.0;
    let scale = eval_curve(&e.scale.curve, x);
    let base = eval_curve(&e.base.curve, x);
    let o = ((eval_curve(&e.sound_level.curve, x) - base) * scale).clamp(0.0, 1.0);
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
    let a = &m.params[k];
    let (lo, hi) = (a.range.0.min(a.range.1), a.range.0.max(a.range.1));
    let v = eval_curve(&a.curve, x);
    if let Some(e) = m.lip_sync.as_ref().filter(|e| e.targets.contains(&a.id)) {
        return lip_sync(e, v, lo, hi, x);
    }
    if let Some(e) = m.eye_blink.as_ref().filter(|e| e.targets.contains(&a.id)) {
        return eye_blink(e, v, lo, hi, x);
    }
    v.clamp(lo, hi)
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
}
