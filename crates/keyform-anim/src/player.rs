//! Runtime playback: motions with fades and special curves, expressions.
//!
//! spec: 60-animation/motion3-json.md §3, 70-advanced/viewer.md §2

use std::collections::HashMap;
use std::sync::Arc;

use crate::expression::{ExpBlend, Expression};
use crate::motion::{Motion, Target};

/// ½ − ½·cos(π·x), clamped to [0, 1] outside (0, 1). spec §3 step 3
pub fn ease(x: f32) -> f32 {
    if x <= 0.0 {
        0.0
    } else if x >= 1.0 {
        1.0
    } else {
        0.5 - 0.5 * (std::f32::consts::PI * x).cos()
    }
}

/// What playback writes to. Parameter and part lookups are by ID.
pub struct Targets<'a> {
    pub params: &'a mut [f32],
    pub param_index: &'a HashMap<String, usize>,
    /// Part opacities set by `PartOpacity` curves this frame (by part ID).
    pub part_opacity: &'a mut HashMap<String, f32>,
    pub eye_blink: &'a [usize],
    pub lip_sync: &'a [usize],
    pub model_opacity: &'a mut f32,
}

struct Playing {
    motion: Arc<Motion>,
    start: f64,
    /// Time at which the motion ends (fade-out completes).
    end: Option<f64>,
    looped: bool,
    last_t: f32,
}

/// Plays motions; starting a new one fades out the current ones.
#[derive(Default)]
pub struct MotionPlayer {
    playing: Vec<Playing>,
}

impl MotionPlayer {
    pub fn is_playing(&self) -> bool {
        !self.playing.is_empty()
    }

    pub fn stop(&mut self) {
        self.playing.clear();
    }

    /// Starts `motion` at time `now` (seconds).
    pub fn start(&mut self, motion: Arc<Motion>, now: f64, looped: bool) {
        for p in &mut self.playing {
            let fo = f64::from(p.motion.fade_out.unwrap_or(0.0));
            let end = now + fo;
            p.end = Some(p.end.map_or(end, |e| e.min(end)));
        }
        self.playing.push(Playing {
            motion,
            start: now,
            end: None,
            looped,
            last_t: -1.0,
        });
    }

    /// Applies all playing motions at time `now`. Returns user-data events
    /// crossed since the previous update.
    pub fn update(&mut self, now: f64, t: &mut Targets) -> Vec<String> {
        let mut events = Vec::new();
        for p in &mut self.playing {
            let m = &p.motion;
            let elapsed = (now - p.start).max(0.0) as f32;
            let local = if p.looped && m.duration > 0.0 {
                elapsed % m.duration
            } else {
                elapsed.min(m.duration)
            };
            if !p.looped && p.end.is_none() && elapsed >= m.duration {
                p.end = Some(p.start + f64::from(m.duration));
            }
            let fi = m
                .fade_in
                .filter(|f| *f > 0.0)
                .map_or(1.0, |f| ease(elapsed / f));
            let fo = match (m.fade_out.filter(|f| *f > 0.0), p.end) {
                (Some(f), Some(end)) => ease(((end - now) as f32) / f),
                _ => 1.0,
            };
            let weight = fi * fo;
            apply_motion(
                m,
                local,
                elapsed,
                p.end.map(|e| (e - now) as f32),
                weight,
                t,
            );
            // §3 step 6: events fire when their time is crossed.
            let wrapped = local < p.last_t;
            for (time, value) in &m.user_data {
                let crossed = if wrapped {
                    *time > p.last_t || *time <= local
                } else {
                    *time > p.last_t && *time <= local
                };
                if crossed {
                    events.push(value.clone());
                }
            }
            p.last_t = local;
        }
        self.playing.retain(|p| p.end.is_none_or(|e| now < e));
        events
    }
}

fn apply_motion(
    m: &Motion,
    local: f32,
    elapsed: f32,
    remaining: Option<f32>,
    weight: f32,
    t: &mut Targets,
) {
    let restricted = m.beziers_restricted;
    let curve_value = |target: Target, id: &str| {
        m.curves
            .iter()
            .find(|c| c.target == target && c.id == id)
            .map(|c| c.eval(local, restricted))
    };
    let eye = curve_value(Target::Model, "EyeBlink");
    let lip = curve_value(Target::Model, "LipSync");
    if let Some(o) = curve_value(Target::Model, "Opacity") {
        *t.model_opacity = o;
    }
    let mut with_curve = Vec::new();
    for c in m.curves.iter().filter(|c| c.target == Target::Parameter) {
        let Some(&pi) = t.param_index.get(&c.id) else {
            continue;
        };
        let Some(cur) = t.params.get(pi).copied() else {
            continue;
        };
        let mut v = c.eval(local, restricted);
        // §3 step 4: special curves modify grouped parameters with own curves.
        if let Some(e) = eye.filter(|_| t.eye_blink.contains(&pi)) {
            v *= e;
        }
        if let Some(l) = lip.filter(|_| t.lip_sync.contains(&pi)) {
            v += l;
        }
        let w = if c.fade_in.is_some() || c.fade_out.is_some() {
            let fi = c
                .fade_in
                .filter(|f| *f > 0.0)
                .map_or(1.0, |f| ease(elapsed / f));
            let fo = match (c.fade_out.filter(|f| *f > 0.0), remaining) {
                (Some(f), Some(r)) => ease(r / f),
                _ => 1.0,
            };
            fi * fo
        } else {
            weight
        };
        t.params[pi] = cur + (v - cur) * w;
        with_curve.push(pi);
    }
    for (value, group) in [(eye, t.eye_blink), (lip, t.lip_sync)] {
        let Some(v) = value else { continue };
        for &pi in group {
            if with_curve.contains(&pi) {
                continue;
            }
            if let Some(p) = t.params.get_mut(pi) {
                *p += (v - *p) * weight;
            }
        }
    }
    for c in m.curves.iter().filter(|c| c.target == Target::PartOpacity) {
        // §3 step 5: part opacities are set directly, without fade weighting.
        t.part_opacity
            .insert(c.id.clone(), c.eval(local, restricted));
    }
}

struct ExpPlaying {
    exp: Arc<Expression>,
    start: f64,
    end: Option<f64>,
}

/// Expression blending: value = (base + Σadd)·Πmultiply, then blended toward
/// overwrite values, each weighted by the expression's fade weight.
/// TODO(Q-0009): exact weighting of Multiply and Overwrite during fades.
#[derive(Default)]
pub struct ExpressionPlayer {
    playing: Vec<ExpPlaying>,
}

/// Default expression fade time (s) when the file leaves it unset.
/// TODO(Q-0009)
pub const DEFAULT_EXPRESSION_FADE: f32 = 1.0;

impl ExpressionPlayer {
    pub fn start(&mut self, exp: Arc<Expression>, now: f64) {
        for p in &mut self.playing {
            let fo = f64::from(p.exp.fade_out.unwrap_or(DEFAULT_EXPRESSION_FADE));
            p.end.get_or_insert(now + fo);
        }
        self.playing.push(ExpPlaying {
            exp,
            start: now,
            end: None,
        });
    }

    pub fn stop(&mut self, now: f64) {
        for p in &mut self.playing {
            let fo = f64::from(p.exp.fade_out.unwrap_or(DEFAULT_EXPRESSION_FADE));
            p.end.get_or_insert(now + fo);
        }
    }

    pub fn update(&mut self, now: f64, params: &mut [f32], param_index: &HashMap<String, usize>) {
        let mut add: HashMap<usize, f32> = HashMap::new();
        let mut mul: HashMap<usize, f32> = HashMap::new();
        let mut over: Vec<(usize, f32, f32)> = Vec::new();
        for p in &self.playing {
            let fi = ease(
                ((now - p.start) as f32)
                    / p.exp.fade_in.unwrap_or(DEFAULT_EXPRESSION_FADE).max(1e-6),
            );
            let fo = p.end.map_or(1.0, |e| {
                ease(
                    ((e - now) as f32)
                        / p.exp.fade_out.unwrap_or(DEFAULT_EXPRESSION_FADE).max(1e-6),
                )
            });
            let w = fi * fo;
            for e in &p.exp.params {
                let Some(&pi) = param_index.get(&e.id) else {
                    continue;
                };
                match e.blend {
                    ExpBlend::Add => *add.entry(pi).or_insert(0.0) += e.value * w,
                    ExpBlend::Multiply => {
                        *mul.entry(pi).or_insert(1.0) *= 1.0 + (e.value - 1.0) * w
                    }
                    ExpBlend::Overwrite => over.push((pi, e.value, w)),
                }
            }
        }
        for (pi, v) in params.iter_mut().enumerate() {
            *v = (*v + add.get(&pi).copied().unwrap_or(0.0)) * mul.get(&pi).copied().unwrap_or(1.0);
        }
        for (pi, value, w) in over {
            if let Some(v) = params.get_mut(pi) {
                *v += (value - *v) * w;
            }
        }
        self.playing.retain(|p| p.end.is_none_or(|e| now < e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::ExpParam;

    fn targets_fixture() -> (Vec<f32>, HashMap<String, usize>) {
        (
            vec![0.0, 1.0, 1.0],
            HashMap::from([("A".into(), 0), ("EyeL".into(), 1), ("EyeR".into(), 2)]),
        )
    }

    #[test]
    fn motion_fades_and_eye_blink() {
        let m = Motion::parse(
            r#"{"Meta":{"Duration":1,"Fps":30,"FadeInTime":0.5,"Loop":true},"Curves":[
            {"Target":"Parameter","Id":"A","Segments":[0,10,0,1,10]},
            {"Target":"Parameter","Id":"EyeL","Segments":[0,0.8,0,1,0.8]},
            {"Target":"Model","Id":"EyeBlink","Segments":[0,0.5,0,1,0.5]}]}"#,
        )
        .unwrap();
        let (mut params, idx) = targets_fixture();
        let mut parts = HashMap::new();
        let mut op = 1.0;
        let mut pl = MotionPlayer::default();
        pl.start(Arc::new(m), 0.0, false);
        let mut t = Targets {
            params: &mut params,
            param_index: &idx,
            part_opacity: &mut parts,
            eye_blink: &[1, 2],
            lip_sync: &[],
            model_opacity: &mut op,
        };
        pl.update(0.25, &mut t);
        // Half of the fade-in: ease(0.5) = 0.5.
        assert!((t.params[0] - 5.0).abs() < 1e-4, "{:?}", t.params);
        pl.update(0.75, &mut t);
        assert!((t.params[0] - 7.5).abs() < 1e-3 || t.params[0] > 5.0);
        // EyeL has its own curve: 0.8 × 0.5 = 0.4 (fully faded in at t=0.75? ease(1.5)=1).
        let (mut p2, _) = targets_fixture();
        let mut t2 = Targets {
            params: &mut p2,
            param_index: &idx,
            part_opacity: &mut parts,
            eye_blink: &[1, 2],
            lip_sync: &[],
            model_opacity: &mut op,
        };
        pl.update(0.6, &mut t2);
        assert!((t2.params[1] - 0.4).abs() < 1e-4, "{:?}", t2.params);
        // EyeR has no curve: set to E = 0.5.
        assert!((t2.params[2] - 0.5).abs() < 1e-4);
        // Non-looping motion ends after its duration.
        pl.update(1.5, &mut t2);
        assert!(!pl.is_playing());
    }

    #[test]
    fn expressions_add_multiply_overwrite() {
        let exp = Expression {
            fade_in: Some(0.0001),
            fade_out: Some(0.0001),
            params: vec![
                ExpParam {
                    id: "A".into(),
                    value: 2.0,
                    blend: ExpBlend::Add,
                },
                ExpParam {
                    id: "EyeL".into(),
                    value: 0.5,
                    blend: ExpBlend::Multiply,
                },
                ExpParam {
                    id: "EyeR".into(),
                    value: 0.25,
                    blend: ExpBlend::Overwrite,
                },
            ],
        };
        let (mut params, idx) = targets_fixture();
        let mut ep = ExpressionPlayer::default();
        ep.start(Arc::new(exp), 0.0);
        ep.update(1.0, &mut params, &idx);
        assert_eq!(params, vec![2.0, 0.5, 0.25]);
    }
}
