//! Cursor tracking: two smoothed 2-D targets driven by pointer drags, added
//! to the parameters mapped to them. spec: 70-advanced/viewer.md §3.6

use crate::player::Params;

/// Which target component a parameter follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrackKind {
    #[default]
    None,
    /// Primary target (left button), x.
    X,
    /// Primary target, y.
    Y,
    /// Secondary target (right button), x.
    XR,
    /// Secondary target, y.
    YR,
}

impl TrackKind {
    /// Default mapping by parameter ID (§3.6).
    pub fn default_for(id: &str) -> Self {
        match id {
            "ParamAngleX" | "ParamEyeBallX" | "ParamBodyAngleX" | "PARAM_ANGLE_X"
            | "PARAM_EYE_BALL_X" | "PARAM_BODY_ANGLE_X" => Self::X,
            "ParamAngleY" | "ParamEyeBallY" | "PARAM_ANGLE_Y" | "PARAM_EYE_BALL_Y" => Self::Y,
            _ => Self::None,
        }
    }
}

/// One smoothed target.
#[derive(Clone, Debug, Default)]
pub struct TrackPoint {
    target: [f32; 2],
    pos: [f32; 2],
    vel: [f32; 2],
    last: Option<f64>,
}

impl TrackPoint {
    /// Velocity limit per update and its acceleration divisor (§3.6).
    const VMAX: f32 = 7.272_727_5 / 30.0;
    const ACCEL_DIV: f32 = 4.5;

    /// Sets the target, each coordinate clamped to [−1, 1]. (0, 0) when
    /// the button is released.
    pub fn set_target(&mut self, t: [f32; 2]) {
        self.target = [t[0].clamp(-1.0, 1.0), t[1].clamp(-1.0, 1.0)];
    }

    pub fn target(&self) -> [f32; 2] {
        self.target
    }

    /// One smoothing step at time `now` (seconds).
    pub fn update(&mut self, now: f64) {
        let Some(last) = self.last.replace(now) else {
            return;
        };
        let f = ((now - last) * 1000.0) as f32 * 30.0 / 1000.0;
        let amax = f * Self::VMAX / Self::ACCEL_DIV;
        let d = [self.target[0] - self.pos[0], self.target[1] - self.pos[1]];
        if d[0].abs() <= 1e-6 && d[1].abs() <= 1e-6 {
            return;
        }
        let dist = d[0].hypot(d[1]);
        let desired = [Self::VMAX * d[0] / dist, Self::VMAX * d[1] / dist];
        let mut a = [desired[0] - self.vel[0], desired[1] - self.vel[1]];
        let alen = a[0].hypot(a[1]);
        if alen > amax {
            let k = amax / alen;
            a = [a[0] * k, a[1] * k];
        }
        self.vel = [self.vel[0] + a[0], self.vel[1] + a[1]];
        let vs = 0.5 * ((amax * amax + 8.0 * amax * dist).sqrt() - amax);
        let v = self.vel[0].hypot(self.vel[1]);
        if v > vs && v > 0.0 {
            let k = vs / v;
            self.vel = [self.vel[0] * k, self.vel[1] * k];
        }
        self.pos = [self.pos[0] + self.vel[0], self.pos[1] + self.vel[1]];
    }

    /// The smoothed point; components below 0.01 in magnitude read as 0.
    pub fn output(&self) -> [f32; 2] {
        self.pos.map(|v| if v.abs() < 0.01 { 0.0 } else { v })
    }
}

/// Pointer position over the preview (u, v in 0…1, v downward) → target
/// coordinates x = 2u − 1, y = 1 − 2v.
pub fn target_from_view(u: f32, v: f32) -> [f32; 2] {
    [2.0 * u - 1.0, 1.0 - 2.0 * v]
}

/// Tracking state of a model: both targets and the per-parameter mapping.
#[derive(Clone, Debug, Default)]
pub struct Tracking {
    pub primary: TrackPoint,
    pub secondary: TrackPoint,
    /// Per real parameter: kind and weight (default 1).
    pub mapping: Vec<(TrackKind, f32)>,
    clock: f64,
}

impl Tracking {
    /// Default mapping for parameters with these IDs.
    pub fn new<'a>(ids: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            mapping: ids
                .into_iter()
                .map(|id| (TrackKind::default_for(id), 1.0))
                .collect(),
            ..Self::default()
        }
    }

    /// Smooths both targets (once per frame while tracking is on) and adds
    /// the result to the mapped parameters (§3.1 step 6).
    pub fn update(&mut self, dt: f32, p: &mut Params) {
        self.clock += f64::from(dt.max(0.0));
        self.primary.update(self.clock);
        self.secondary.update(self.clock);
        let (a, b) = (self.primary.output(), self.secondary.output());
        for (i, &(kind, w)) in self.mapping.iter().enumerate() {
            let s = match kind {
                TrackKind::None => continue,
                TrackKind::X => a[0],
                TrackKind::Y => a[1],
                TrackKind::XR => b[0],
                TrackKind::YR => b[1],
            };
            let (Some(&cur), Some(r)) = (p.values.get(i), p.ranges.get(i)) else {
                continue;
            };
            // The current value's sign picks the side (reference quirk).
            let range = if cur >= 0.0 { r.max.abs() } else { r.min.abs() };
            p.set_index(i, cur + s * range * 1.2 * w, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::ParamRange;
    use std::collections::HashMap;

    #[test]
    fn default_mapping() {
        assert_eq!(TrackKind::default_for("ParamAngleX"), TrackKind::X);
        assert_eq!(TrackKind::default_for("PARAM_EYE_BALL_Y"), TrackKind::Y);
        assert_eq!(TrackKind::default_for("ParamBodyAngleY"), TrackKind::None);
    }

    #[test]
    fn smoothing_converges_without_overshoot() {
        let mut t = TrackPoint::default();
        t.set_target([2.0, -0.5]);
        assert_eq!(t.target(), [1.0, -0.5]);
        let mut now = 0.0;
        t.update(now);
        assert_eq!(t.output(), [0.0, 0.0], "first call only stores the time");
        let mut prev = 0.0;
        for _ in 0..600 {
            now += 1.0 / 60.0;
            t.update(now);
            let x = t.output()[0];
            assert!(x <= 1.0 + 1e-2, "overshoot {x}");
            assert!(x + 1e-2 >= prev, "no swinging back");
            prev = x;
        }
        let o = t.output();
        assert!(
            (o[0] - 1.0).abs() < 1e-3 && (o[1] + 0.5).abs() < 1e-3,
            "{o:?}"
        );
        // Released: back to the centre.
        t.set_target([0.0, 0.0]);
        for _ in 0..600 {
            now += 1.0 / 60.0;
            t.update(now);
        }
        assert_eq!(t.output(), [0.0, 0.0]);
    }

    #[test]
    fn adds_scaled_by_the_side_of_the_current_value() {
        let mut values = vec![5.0, -5.0, 0.0];
        let ranges = vec![
            ParamRange {
                min: -10.0,
                max: 30.0,
                repeat: false,
            };
            3
        ];
        let index = HashMap::new();
        let mut virtuals = HashMap::new();
        let mut tr = Tracking::new(["ParamAngleX", "ParamAngleY", "Other"]);
        // Pretend the smoothed point already sits at (0.5, 0.5).
        tr.primary.pos = [0.5, 0.5];
        tr.primary.target = [0.5, 0.5];
        let mut p = Params {
            values: &mut values,
            ranges: &ranges,
            index: &index,
            virtuals: &mut virtuals,
        };
        tr.update(0.0, &mut p);
        // x: current 5 ≥ 0 → |max| 30: 5 + 0.5·30·1.2 = 23.
        assert!((values[0] - 23.0).abs() < 1e-4, "{values:?}");
        // y: current −5 < 0 → |min| 10: −5 + 0.5·10·1.2 = 1.
        assert!((values[1] - 1.0).abs() < 1e-4);
        assert_eq!(values[2], 0.0);
        assert_eq!(target_from_view(0.75, 0.25), [0.5, 0.5]);
    }
}
