//! Recording parameter values into motion data, and the physics window's
//! procedural test motions.
//! spec: 70-advanced/physics-editor.md §2 (playlist, recording),
//! 70-advanced/external-api.md §5 (recording from external input)

use serde_json::json;

/// Thinning of recorded keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Thinning {
    /// One key per recorded frame.
    #[default]
    None,
    /// Drop keys that lie on the line through their neighbours.
    Linear,
}

/// Samples parameter values at a fixed frame rate.
#[derive(Clone, Debug, PartialEq)]
pub struct Recorder {
    pub fps: f32,
    pub ids: Vec<String>,
    pub frames: Vec<Vec<f32>>,
    acc: f32,
}

impl Recorder {
    pub fn new(fps: f32, ids: Vec<String>) -> Self {
        Self {
            fps: fps.max(1.0),
            ids,
            frames: Vec::new(),
            acc: 0.0,
        }
    }

    /// Advances by `dt` seconds; records `values` at every frame boundary
    /// passed (the first call records frame 0).
    pub fn push(&mut self, dt: f32, values: &[f32]) {
        if self.frames.is_empty() {
            self.frames.push(values.to_vec());
            return;
        }
        self.acc += dt.max(0.0);
        let step = 1.0 / self.fps;
        while self.acc >= step {
            self.acc -= step;
            self.frames.push(values.to_vec());
        }
    }

    pub fn duration(&self) -> f32 {
        self.frames.len().saturating_sub(1) as f32 / self.fps
    }

    /// Keys of parameter `k` (frame, value) after thinning.
    pub fn keys(&self, k: usize, thinning: Thinning) -> Vec<(usize, f32)> {
        let v: Vec<f32> = self
            .frames
            .iter()
            .map(|f| f.get(k).copied().unwrap_or(0.0))
            .collect();
        let mut out: Vec<(usize, f32)> = v.iter().copied().enumerate().collect();
        if thinning == Thinning::Linear && out.len() > 2 {
            let mut kept = vec![out[0]];
            for i in 1..out.len() - 1 {
                let (a, b, c) = (*kept.last().unwrap_or(&out[0]), out[i], out[i + 1]);
                let t = (b.0 - a.0) as f32 / (c.0 - a.0) as f32;
                let lerp = a.1 + (c.1 - a.1) * t;
                if (lerp - b.1).abs() > 1e-4 {
                    kept.push(b);
                }
            }
            kept.push(out[out.len() - 1]);
            out = kept;
        }
        out
    }

    /// motion3.json of the recording: one linear curve per parameter that
    /// changes (motion3-json.md layout; parameters that never change are
    /// left out).
    pub fn to_motion3(&self, thinning: Thinning, looped: bool) -> String {
        let mut curves = Vec::new();
        let (mut segs, mut points) = (0usize, 0usize);
        for (k, id) in self.ids.iter().enumerate() {
            let keys = self.keys(k, thinning);
            if keys.windows(2).all(|w| (w[0].1 - w[1].1).abs() < 1e-6) {
                continue;
            }
            let mut s: Vec<f32> = vec![0.0, keys[0].1];
            for w in keys.windows(2) {
                s.extend_from_slice(&[0.0, w[1].0 as f32 / self.fps, w[1].1]);
            }
            segs += keys.len() - 1;
            points += keys.len();
            let rounded: Vec<f64> = s
                .iter()
                .map(|x| (f64::from(*x) * 1000.0).round() / 1000.0)
                .collect();
            curves.push(json!({ "Target": "Parameter", "Id": id, "Segments": rounded }));
        }
        let v = json!({
            "Version": 3,
            "Meta": {
                "Duration": (f64::from(self.duration()) * 1000.0).round() / 1000.0,
                "Fps": self.fps,
                "Loop": looped,
                "AreBeziersRestricted": true,
                "CurveCount": curves.len(),
                "TotalSegmentCount": segs,
                "TotalPointCount": points,
                "UserDataCount": 0,
                "TotalUserDataSize": 0
            },
            "Curves": curves,
        });
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }
}

/// The built-in test motions of the physics playlist ("[Pattern A]",
/// "[Pattern B]", "[Posing]"). Keyform's own procedural loops (§2: the
/// reference constants aren't reproduced).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestMotion {
    PatternA,
    PatternB,
    Posing,
}

impl TestMotion {
    pub const ALL: [TestMotion; 3] = [
        TestMotion::PatternA,
        TestMotion::PatternB,
        TestMotion::Posing,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TestMotion::PatternA => "[Pattern A]",
            TestMotion::PatternB => "[Pattern B]",
            TestMotion::Posing => "[Posing]",
        }
    }

    /// (parameter id, fraction of the half range −1…1) at time `t` seconds.
    pub fn sample(self, t: f32) -> Vec<(&'static str, f32)> {
        const IDS: [&str; 10] = [
            "ParamAngleX",
            "ParamAngleY",
            "ParamAngleZ",
            "ParamBodyAngleX",
            "ParamBodyAngleY",
            "ParamBodyAngleZ",
            "ParamEyeBallX",
            "ParamEyeBallY",
            "ParamBrowLY",
            "ParamBrowRY",
        ];
        let tau = std::f32::consts::TAU;
        match self {
            TestMotion::PatternA | TestMotion::PatternB => {
                // Unrelated frequencies so the sweeps don't line up.
                let (base, phase) = if self == TestMotion::PatternA {
                    (0.29, 0.0)
                } else {
                    (0.47, 1.3)
                };
                IDS.iter()
                    .enumerate()
                    .map(|(i, id)| {
                        let f = base * (1.0 + 0.37 * i as f32);
                        (*id, 0.8 * (tau * f * t + phase * i as f32).sin())
                    })
                    .collect()
            }
            TestMotion::Posing => {
                // Held poses every 1.6 s, eased over 0.4 s.
                let (hold, ease) = (1.6f32, 0.4f32);
                let n = (t / hold).floor();
                let u = ((t - n * hold) / ease).clamp(0.0, 1.0);
                let s = u * u * (3.0 - 2.0 * u);
                let pose = |k: f32, i: usize| {
                    let x = ((k * 12.9898 + i as f32 * 78.233).sin() * 43_758.547).rem_euclid(1.0);
                    x * 1.6 - 0.8
                };
                IDS.iter()
                    .enumerate()
                    .map(|(i, id)| (*id, pose(n - 1.0, i) + (pose(n, i) - pose(n - 1.0, i)) * s))
                    .collect()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_thins() {
        let mut r = Recorder::new(10.0, vec!["A".into(), "B".into()]);
        for i in 0..=10 {
            r.push(if i == 0 { 0.0 } else { 0.1 }, &[i as f32, 1.0]);
        }
        assert_eq!(r.frames.len(), 11);
        assert!((r.duration() - 1.0).abs() < 1e-5);
        // A is a straight ramp: thinning keeps the two ends.
        assert_eq!(r.keys(0, Thinning::Linear).len(), 2);
        let j: serde_json::Value =
            serde_json::from_str(&r.to_motion3(Thinning::Linear, false)).unwrap();
        assert_eq!(j["Meta"]["CurveCount"], 1, "B never changes");
        assert_eq!(j["Curves"][0]["Segments"].as_array().unwrap().len(), 5);
        for m in TestMotion::ALL {
            for t in [0.0, 0.5, 3.3] {
                assert!(m.sample(t).iter().all(|(_, v)| v.abs() <= 0.8 + 1e-5));
            }
        }
    }
}
