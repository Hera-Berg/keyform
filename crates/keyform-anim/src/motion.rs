//! motion3.json: parsing, writing and curve evaluation.
//!
//! spec: 60-animation/motion3-json.md

use serde_json::{json, Map, Value};

use crate::json::{f32_of, num};
use crate::AnimError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Parameter,
    PartOpacity,
    Model,
}

impl Target {
    fn name(self) -> &'static str {
        match self {
            Target::Parameter => "Parameter",
            Target::PartOpacity => "PartOpacity",
            Target::Model => "Model",
        }
    }
}

/// One segment, continuing from the previous segment's end point (§1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Linear([f32; 2]),
    Bezier {
        c1: [f32; 2],
        c2: [f32; 2],
        end: [f32; 2],
    },
    Stepped([f32; 2]),
    InverseStepped([f32; 2]),
}

impl Segment {
    pub fn end(&self) -> [f32; 2] {
        match *self {
            Segment::Linear(p) | Segment::Stepped(p) | Segment::InverseStepped(p) => p,
            Segment::Bezier { end, .. } => end,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub target: Target,
    pub id: String,
    pub fade_in: Option<f32>,
    pub fade_out: Option<f32>,
    /// First point (time, value).
    pub start: [f32; 2],
    pub segments: Vec<Segment>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Motion {
    pub duration: f32,
    pub fps: f32,
    pub fade_in: Option<f32>,
    pub fade_out: Option<f32>,
    pub looped: bool,
    pub beziers_restricted: bool,
    pub curves: Vec<Curve>,
    /// (time, value) events.
    pub user_data: Vec<(f32, String)>,
}

/// Fade time from JSON: absent → None; negative → 1 s (§3 defaults).
fn fade(v: &Value) -> Option<f32> {
    f32_of(v).map(|x| if x < 0.0 { 1.0 } else { x })
}

impl Motion {
    /// Parses a motion3.json. Malformed curves are an error; unknown members
    /// are ignored.
    pub fn parse(text: &str) -> Result<Self, AnimError> {
        let v: Value = serde_json::from_str(text).map_err(|e| AnimError::Json(e.to_string()))?;
        let meta = &v["Meta"];
        let mut curves = Vec::new();
        for (ci, c) in v["Curves"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            let target = match c["Target"].as_str() {
                Some("Parameter") => Target::Parameter,
                Some("PartOpacity") => Target::PartOpacity,
                Some("Model") => Target::Model,
                t => {
                    return Err(AnimError::Invalid(format!(
                        "curve {ci}: unknown target {t:?}"
                    )))
                }
            };
            let s: Vec<f32> = c["Segments"]
                .as_array()
                .ok_or_else(|| AnimError::Invalid(format!("curve {ci}: no segments")))?
                .iter()
                .map(|x| {
                    f32_of(x).ok_or_else(|| {
                        AnimError::Invalid(format!("curve {ci}: non-numeric segment data"))
                    })
                })
                .collect::<Result<_, _>>()?;
            if s.len() < 2 {
                return Err(AnimError::Invalid(format!(
                    "curve {ci}: missing first point"
                )));
            }
            let mut segments = Vec::new();
            let mut i = 2;
            let bad = || AnimError::Invalid(format!("curve {ci}: truncated segment"));
            while i < s.len() {
                let code = s[i];
                let take = |k: usize| -> Result<[f32; 2], AnimError> {
                    Ok([
                        *s.get(i + 1 + 2 * k).ok_or_else(bad)?,
                        *s.get(i + 2 + 2 * k).ok_or_else(bad)?,
                    ])
                };
                let (seg, n) = match code as i32 {
                    0 => (Segment::Linear(take(0)?), 3),
                    1 => (
                        Segment::Bezier {
                            c1: take(0)?,
                            c2: take(1)?,
                            end: take(2)?,
                        },
                        7,
                    ),
                    2 => (Segment::Stepped(take(0)?), 3),
                    3 => (Segment::InverseStepped(take(0)?), 3),
                    _ => {
                        return Err(AnimError::Invalid(format!(
                            "curve {ci}: segment type {code}"
                        )))
                    }
                };
                segments.push(seg);
                i += n;
            }
            curves.push(Curve {
                target,
                id: c["Id"].as_str().unwrap_or_default().to_owned(),
                fade_in: fade(&c["FadeInTime"]),
                fade_out: fade(&c["FadeOutTime"]),
                start: [s[0], s[1]],
                segments,
            });
        }
        let user_data = v["UserData"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|u| Some((f32_of(&u["Time"])?, u["Value"].as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            duration: f32_of(&meta["Duration"]).unwrap_or(0.0).max(0.0),
            fps: f32_of(&meta["Fps"]).filter(|f| *f > 0.0).unwrap_or(30.0),
            fade_in: fade(&meta["FadeInTime"]),
            fade_out: fade(&meta["FadeOutTime"]),
            looped: meta["Loop"].as_bool().unwrap_or(false),
            beziers_restricted: meta["AreBeziersRestricted"].as_bool().unwrap_or(false),
            curves,
            user_data,
        })
    }

    /// Serializes in the reference layout (§1–2): 3 decimals, 2 for opacity curves.
    pub fn to_json(&self) -> Value {
        let mut segs_total = 0;
        let mut points_total = 0;
        let curves: Vec<Value> = self
            .curves
            .iter()
            .map(|c| {
                let d = if c.target == Target::PartOpacity
                    || (c.target == Target::Model && c.id == "Opacity")
                {
                    2
                } else {
                    3
                };
                // Opacity curves use 2 decimals for times as well.
                let n = |x: f32| num(f64::from(x), d);
                let t = |x: f32| num(f64::from(x), d);
                let mut s = vec![t(c.start[0]), n(c.start[1])];
                points_total += 1;
                for seg in &c.segments {
                    segs_total += 1;
                    match *seg {
                        Segment::Linear(p) | Segment::Stepped(p) | Segment::InverseStepped(p) => {
                            let code = match seg {
                                Segment::Linear(_) => 0,
                                Segment::Stepped(_) => 2,
                                _ => 3,
                            };
                            s.extend([Value::from(code), t(p[0]), n(p[1])]);
                            points_total += 1;
                        }
                        Segment::Bezier { c1, c2, end } => {
                            s.extend([
                                Value::from(1),
                                t(c1[0]),
                                n(c1[1]),
                                t(c2[0]),
                                n(c2[1]),
                                t(end[0]),
                                n(end[1]),
                            ]);
                            points_total += 3;
                        }
                    }
                }
                let mut o = Map::new();
                o.insert("Target".into(), json!(c.target.name()));
                o.insert("Id".into(), json!(c.id));
                if let Some(f) = c.fade_in {
                    o.insert("FadeInTime".into(), num(f64::from(f), 3));
                }
                if let Some(f) = c.fade_out {
                    o.insert("FadeOutTime".into(), num(f64::from(f), 3));
                }
                o.insert("Segments".into(), Value::Array(s));
                Value::Object(o)
            })
            .collect();
        let mut meta = Map::new();
        // Duration: 2 decimals (observed in the reference's exports).
        meta.insert("Duration".into(), num(f64::from(self.duration), 2));
        meta.insert("Fps".into(), json!(f64::from(self.fps)));
        if let Some(f) = self.fade_in {
            meta.insert("FadeInTime".into(), num(f64::from(f), 3));
        }
        if let Some(f) = self.fade_out {
            meta.insert("FadeOutTime".into(), num(f64::from(f), 3));
        }
        meta.insert("Loop".into(), json!(true));
        meta.insert(
            "AreBeziersRestricted".into(),
            json!(self.beziers_restricted),
        );
        meta.insert("CurveCount".into(), json!(self.curves.len()));
        meta.insert("TotalSegmentCount".into(), json!(segs_total));
        meta.insert("TotalPointCount".into(), json!(points_total));
        meta.insert("UserDataCount".into(), json!(self.user_data.len()));
        meta.insert(
            "TotalUserDataSize".into(),
            json!(self.user_data.iter().map(|u| u.1.len()).sum::<usize>()),
        );
        let mut root = Map::new();
        root.insert("Version".into(), json!(3));
        root.insert("Meta".into(), Value::Object(meta));
        root.insert("Curves".into(), Value::Array(curves));
        if !self.user_data.is_empty() {
            root.insert(
                "UserData".into(),
                Value::Array(
                    self.user_data
                        .iter()
                        .map(|(t, v)| json!({ "Time": num(f64::from(*t), 3), "Value": v }))
                        .collect(),
                ),
            );
        }
        Value::Object(root)
    }
}

fn lerp(a: [f32; 2], b: [f32; 2], u: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u]
}

/// Cubic bezier point at parameter u (de Casteljau).
fn bezier_at(p0: [f32; 2], c1: [f32; 2], c2: [f32; 2], p1: [f32; 2], u: f32) -> [f32; 2] {
    let a = lerp(p0, c1, u);
    let b = lerp(c1, c2, u);
    let c = lerp(c2, p1, u);
    let d = lerp(a, b, u);
    let e = lerp(b, c, u);
    lerp(d, e, u)
}

/// Solves x(u) = t for the bezier's time component, taking the real root in
/// [−0.01, 1.01] (clamped to [0, 1]). spec §3 step 2
fn solve_bezier_u(p0: f32, c1: f32, c2: f32, p1: f32, t: f32) -> f32 {
    let (p0, c1, c2, p1, t) = (
        f64::from(p0),
        f64::from(c1),
        f64::from(c2),
        f64::from(p1),
        f64::from(t),
    );
    let a = p1 - 3.0 * c2 + 3.0 * c1 - p0;
    let b = 3.0 * c2 - 6.0 * c1 + 3.0 * p0;
    let c = 3.0 * c1 - 3.0 * p0;
    let d = p0 - t;
    let ok = |u: f64| (-0.01..=1.01).contains(&u);
    let pick = |roots: &[f64]| {
        roots
            .iter()
            .copied()
            .find(|&u| ok(u))
            .map_or(0.0, |u| u.clamp(0.0, 1.0)) as f32
    };
    if a.abs() < 1e-5 {
        if b.abs() < 1e-5 {
            if c.abs() < 1e-12 {
                return 0.0;
            }
            return pick(&[-d / c]);
        }
        let disc = c * c - 4.0 * b * d;
        if disc < 0.0 {
            return 0.0;
        }
        let s = disc.sqrt();
        return pick(&[(-c + s) / (2.0 * b), (-c - s) / (2.0 * b)]);
    }
    // Cardano on the depressed cubic.
    let (bb, cc, dd) = (b / a, c / a, d / a);
    let p = (3.0 * cc - bb * bb) / 3.0;
    let q = (2.0 * bb * bb * bb - 9.0 * bb * cc + 27.0 * dd) / 27.0;
    let disc = q * q / 4.0 + p * p * p / 27.0;
    let shift = -bb / 3.0;
    let roots: Vec<f64> = if disc > 1e-14 {
        let s = disc.sqrt();
        vec![(-q / 2.0 + s).cbrt() + (-q / 2.0 - s).cbrt() + shift]
    } else if disc.abs() <= 1e-14 {
        let u = (-q / 2.0).cbrt();
        vec![2.0 * u + shift, -u + shift]
    } else {
        let r = (-p * p * p / 27.0).sqrt();
        let phi = (-q / (2.0 * r)).clamp(-1.0, 1.0).acos();
        let m = 2.0 * (-p / 3.0).sqrt();
        (0..3)
            .map(|k| m * ((phi + 2.0 * std::f64::consts::PI * f64::from(k)) / 3.0).cos() + shift)
            .collect()
    };
    pick(&roots)
}

impl Curve {
    /// Value at motion-local time `t`. spec §3 steps 1–2
    pub fn eval(&self, t: f32, restricted: bool) -> f32 {
        let mut p0 = self.start;
        for seg in &self.segments {
            let p1 = seg.end();
            if p1[0] > t {
                let span = p1[0] - p0[0];
                let u = if span > 0.0 {
                    ((t - p0[0]) / span).max(0.0)
                } else {
                    0.0
                };
                return match *seg {
                    Segment::Linear(_) => p0[1] + (p1[1] - p0[1]) * u,
                    Segment::Stepped(_) => p0[1],
                    Segment::InverseStepped(_) => p1[1],
                    Segment::Bezier { c1, c2, end } => {
                        let u = if restricted {
                            u
                        } else {
                            solve_bezier_u(p0[0], c1[0], c2[0], end[0], t)
                        };
                        bezier_at(p0, c1, c2, end, u)[1]
                    }
                };
            }
            p0 = p1;
        }
        p0[1]
    }

    pub fn last_point(&self) -> [f32; 2] {
        self.segments.last().map_or(self.start, Segment::end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"Version":3,"Meta":{"Duration":2.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":false,
      "CurveCount":2,"TotalSegmentCount":4,"TotalPointCount":7,"UserDataCount":1,"TotalUserDataSize":5},
      "Curves":[{"Target":"Parameter","Id":"A","Segments":[0,0,0,1,10,1,1.333,10,1.667,-10,2,-10]},
                {"Target":"PartOpacity","Id":"P","FadeInTime":-1,"Segments":[0,1,2,1,0,3,2,1]}],
      "UserData":[{"Time":1.5,"Value":"event"}]}"#;

    #[test]
    fn parse_and_evaluate() {
        let m = Motion::parse(SAMPLE).unwrap();
        assert_eq!(m.curves.len(), 2);
        assert_eq!(m.curves[1].fade_in, Some(1.0));
        let a = &m.curves[0];
        assert_eq!(a.eval(0.5, false), 5.0);
        assert_eq!(a.eval(1.0, false), 10.0);
        // Symmetric S-curve: the midpoint in time is the midpoint in value.
        assert!(a.eval(1.5, false).abs() < 1e-3);
        assert_eq!(a.eval(9.0, false), -10.0);
        let p = &m.curves[1];
        assert_eq!(p.eval(0.5, false), 1.0); // stepped holds p0
        assert_eq!(p.eval(1.5, false), 1.0); // inverse stepped jumps to p1
        assert_eq!(m.user_data, vec![(1.5, "event".to_owned())]);
    }

    #[test]
    fn json_roundtrip() {
        let m = Motion::parse(SAMPLE).unwrap();
        let back = Motion::parse(&crate::json::to_string(&m.to_json())).unwrap();
        assert_eq!(m.curves, back.curves);
        let j = m.to_json();
        assert_eq!(j["Meta"]["TotalSegmentCount"], 4);
        assert_eq!(j["Meta"]["TotalPointCount"], 8);
    }

    #[test]
    fn unrestricted_bezier_solves_time() {
        // Handles bunched near the start in time: value at t differs from u = t.
        let c = Curve {
            target: Target::Parameter,
            id: "X".into(),
            fade_in: None,
            fade_out: None,
            start: [0.0, 0.0],
            segments: vec![Segment::Bezier {
                c1: [0.1, 0.0],
                c2: [0.2, 1.0],
                end: [1.0, 1.0],
            }],
        };
        let t = 0.5;
        let v = c.eval(t, false);
        // Check by inverting: find u with x(u) = t numerically.
        let mut lo = 0.0f32;
        let mut hi = 1.0f32;
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            if bezier_at([0.0, 0.0], [0.1, 0.0], [0.2, 1.0], [1.0, 1.0], mid)[0] < t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let expect = bezier_at([0.0, 0.0], [0.1, 0.0], [0.2, 1.0], [1.0, 1.0], lo)[1];
        assert!((v - expect).abs() < 1e-3, "{v} vs {expect}");
        assert!(
            (c.eval(t, true) - bezier_at([0.0, 0.0], [0.1, 0.0], [0.2, 1.0], [1.0, 1.0], 0.5)[1])
                .abs()
                < 1e-6
        );
    }

    #[test]
    fn malformed_is_error() {
        for bad in [
            "x",
            r#"{"Curves":[{"Target":"Bogus","Segments":[0,0]}]}"#,
            r#"{"Curves":[{"Target":"Model","Segments":[0]}]}"#,
            r#"{"Curves":[{"Target":"Model","Segments":[0,0,1,1]}]}"#,
            r#"{"Curves":[{"Target":"Model","Segments":[0,0,9,1,1]}]}"#,
        ] {
            assert!(Motion::parse(bad).is_err(), "{bad}");
        }
    }
}
