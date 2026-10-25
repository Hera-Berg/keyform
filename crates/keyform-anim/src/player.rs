//! Runtime playback: parameter access with virtual parameters, the motion
//! queue, the expression manager (and the old fading method), breath and
//! automatic eye blink.
//!
//! spec: 70-advanced/viewer.md §3, 60-animation/motion3-json.md §3

use std::collections::HashMap;
use std::sync::Arc;

use crate::expression::{ExpBlend, Expression};
use crate::motion::{Motion, Target};

/// ½ − ½·cos(π·x), clamped to [0, 1] outside (0, 1). motion3-json.md §3
pub fn ease(x: f32) -> f32 {
    if x <= 0.0 {
        0.0
    } else if x >= 1.0 {
        1.0
    } else {
        0.5 - 0.5 * (std::f32::consts::PI * x).cos()
    }
}

/// Range of a real parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamRange {
    pub min: f32,
    pub max: f32,
    pub repeat: bool,
}

impl ParamRange {
    /// Clamps, or wraps into [min, max) for a repeat parameter (§3).
    pub fn fit(&self, v: f32) -> f32 {
        let (lo, hi) = (self.min.min(self.max), self.min.max(self.max));
        if self.repeat && hi > lo {
            lo + (v - lo).rem_euclid(hi - lo)
        } else {
            v.clamp(lo, hi)
        }
    }
}

/// Parameter access for playback: real parameters by index or ID, and
/// virtual parameters (unknown IDs, created at 0, unclamped, not drawn).
/// spec: viewer.md §3 "Virtual parameters"
pub struct Params<'a> {
    pub values: &'a mut [f32],
    pub ranges: &'a [ParamRange],
    pub index: &'a HashMap<String, usize>,
    pub virtuals: &'a mut HashMap<String, f32>,
}

impl Params<'_> {
    pub fn get(&mut self, id: &str) -> f32 {
        match self.index.get(id) {
            Some(&i) => self.values.get(i).copied().unwrap_or(0.0),
            None => *self.virtuals.entry(id.to_owned()).or_insert(0.0),
        }
    }

    /// Sets `id` toward `target` with weight `w` (§3 "Setting a parameter").
    pub fn set(&mut self, id: &str, target: f32, w: f32) {
        match self.index.get(id).copied() {
            Some(i) => self.set_index(i, target, w),
            None => {
                let p = self.virtuals.entry(id.to_owned()).or_insert(0.0);
                *p = if w == 1.0 {
                    target
                } else {
                    *p * (1.0 - w) + target * w
                };
            }
        }
    }

    pub fn set_index(&mut self, i: usize, target: f32, w: f32) {
        let Some(r) = self.ranges.get(i) else { return };
        let target = r.fit(target);
        if let Some(p) = self.values.get_mut(i) {
            *p = if w == 1.0 {
                target
            } else {
                *p * (1.0 - w) + target * w
            };
        }
    }

    /// "Add v with weight w": p + v·w, clamped.
    pub fn add(&mut self, id: &str, v: f32, w: f32) {
        let p = self.get(id);
        self.set(id, p + v * w, 1.0);
    }

    /// "Multiply by v with weight w": p·(1 + (v − 1)·w), clamped.
    pub fn mul(&mut self, id: &str, v: f32, w: f32) {
        let p = self.get(id);
        self.set(id, p * (1.0 + (v - 1.0) * w), 1.0);
    }
}

/// Model-level outputs of motions.
#[derive(Clone, Copy, Debug)]
pub struct MotionOutputs {
    /// Set by `Model/Opacity` curves.
    pub model_opacity: f32,
}

impl Default for MotionOutputs {
    fn default() -> Self {
        Self { model_opacity: 1.0 }
    }
}

struct Entry {
    motion: Arc<Motion>,
    looped: bool,
    /// Set on the first update.
    start: Option<f64>,
    fade_in_start: f64,
    /// −1 = none.
    end: f64,
    pending_fade_out: Option<f32>,
    last_t: f32,
    finished: bool,
}

/// Default fade time when absent or negative (motion3-json.md §3, viewer.md §3.2).
fn fade_or_default(f: Option<f32>) -> f32 {
    f.filter(|f| *f >= 0.0).unwrap_or(1.0)
}

/// The motion queue. spec: viewer.md §3.2
#[derive(Default)]
pub struct MotionPlayer {
    queue: Vec<Entry>,
    clock: f64,
    /// Animation ▸ "Use old playback methods": evaluate every Bezier
    /// segment in the restricted form (viewer.md §3.7).
    pub force_restricted_beziers: bool,
}

pub struct MotionUpdate {
    /// User-data events crossed this frame.
    pub events: Vec<String>,
    /// Whether any motion was updated (eye blink runs only if not, §3.1).
    pub updated: bool,
}

impl MotionPlayer {
    pub fn is_playing(&self) -> bool {
        !self.queue.is_empty()
    }

    pub fn stop(&mut self) {
        self.queue.clear();
    }

    /// Queues `motion`; existing entries get a pending fade-out (§3.2).
    pub fn start(&mut self, motion: Arc<Motion>, looped: bool) {
        for e in &mut self.queue {
            e.pending_fade_out = Some(fade_or_default(e.motion.fade_out));
        }
        self.queue.push(Entry {
            motion,
            looped,
            start: None,
            fade_in_start: 0.0,
            end: -1.0,
            pending_fade_out: None,
            last_t: -1.0,
            finished: false,
        });
    }

    /// Advances the queue clock by `dt` and applies every entry, oldest first.
    pub fn update(
        &mut self,
        dt: f32,
        p: &mut Params,
        eye_blink: &[usize],
        lip_sync: &[usize],
        out: &mut MotionOutputs,
    ) -> MotionUpdate {
        self.clock += f64::from(dt.max(0.0));
        let t_now = self.clock;
        let mut events = Vec::new();
        let updated = !self.queue.is_empty();
        for e in &mut self.queue {
            // 1. First update.
            let start = *e.start.get_or_insert_with(|| {
                e.fade_in_start = t_now;
                e.end = if e.looped {
                    -1.0
                } else {
                    t_now + f64::from(e.motion.duration)
                };
                t_now
            });
            // 2. Curves.
            let m = &e.motion;
            let elapsed = (t_now - start) as f32;
            let local = if e.looped && m.duration > 0.0 {
                elapsed % m.duration
            } else {
                elapsed.min(m.duration)
            };
            let fade_in = fade_or_default(m.fade_in);
            let fade_out = fade_or_default(m.fade_out);
            let weights = |fi_time: f32, fo_time: f32| -> f32 {
                let fi = if fi_time > 0.0 {
                    ease(((t_now - e.fade_in_start) as f32) / fi_time)
                } else {
                    1.0
                };
                let fo = if fo_time > 0.0 && e.end > 0.0 {
                    ease(((e.end - t_now) as f32) / fo_time)
                } else {
                    1.0
                };
                fi * fo
            };
            let weight = weights(fade_in, fade_out);
            apply_motion(
                m,
                local,
                m.beziers_restricted || self.force_restricted_beziers,
                weight,
                &|c: &crate::motion::Curve| {
                    weights(c.fade_in.unwrap_or(fade_in), c.fade_out.unwrap_or(fade_out))
                },
                p,
                eye_blink,
                lip_sync,
                out,
            );
            // User data crossed since the previous update (motion3-json.md §3 step 6).
            let wrapped = local < e.last_t;
            for (time, value) in &m.user_data {
                let crossed = if wrapped {
                    *time > e.last_t || *time <= local
                } else {
                    *time > e.last_t && *time <= local
                };
                if crossed {
                    events.push(value.clone());
                }
            }
            e.last_t = local;
            // 3. Finished?
            if e.end > 0.0 && e.end < t_now {
                e.finished = true;
            }
            // 4. Pending fade-out.
            if let Some(fo) = e.pending_fade_out.take() {
                let end = t_now + f64::from(fo);
                if !(e.end > 0.0 && e.end <= end) {
                    e.end = end;
                }
            }
        }
        self.queue.retain(|e| !e.finished);
        MotionUpdate { events, updated }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_motion(
    m: &Motion,
    local: f32,
    restricted: bool,
    weight: f32,
    curve_weight: &dyn Fn(&crate::motion::Curve) -> f32,
    p: &mut Params,
    eye_blink: &[usize],
    lip_sync: &[usize],
    out: &mut MotionOutputs,
) {
    let value = |target: Target, id: &str| {
        m.curves
            .iter()
            .find(|c| c.target == target && c.id == id)
            .map(|c| c.eval(local, restricted))
    };
    let eye = value(Target::Model, "EyeBlink");
    let lip = value(Target::Model, "LipSync");
    if let Some(o) = value(Target::Model, "Opacity") {
        out.model_opacity = o;
    }
    let mut with_curve = Vec::new();
    for c in m.curves.iter().filter(|c| c.target == Target::Parameter) {
        let mut v = c.eval(local, restricted);
        let real = p.index.get(&c.id).copied();
        // Special curves modify grouped parameters that have their own curve.
        if let Some(i) = real {
            if let Some(e) = eye.filter(|_| eye_blink.contains(&i)) {
                v *= e;
            }
            if let Some(l) = lip.filter(|_| lip_sync.contains(&i)) {
                v += l;
            }
            with_curve.push(i);
        }
        let has_own = c.fade_in.is_some() || c.fade_out.is_some();
        p.set(&c.id, v, if has_own { curve_weight(c) } else { weight });
    }
    for (v, group) in [(eye, eye_blink), (lip, lip_sync)] {
        let Some(v) = v else { continue };
        for &i in group.iter().filter(|i| !with_curve.contains(i)) {
            p.set_index(i, v, weight);
        }
    }
    // PartOpacity curves set the part-ID parameter, unweighted (§3 step 5).
    for c in m.curves.iter().filter(|c| c.target == Target::PartOpacity) {
        p.set(&c.id, c.eval(local, restricted), 1.0);
    }
}

struct ExpEntry {
    exp: Arc<Expression>,
    start: Option<f64>,
    fade_in_start: f64,
    end: f64,
    pending_fade_out: Option<f32>,
}

/// The expression manager. spec: viewer.md §3.3
#[derive(Default)]
pub struct ExpressionPlayer {
    queue: Vec<ExpEntry>,
    clock: f64,
    /// Tracked parameter IDs with their (add, mul, ow) state, in first-seen order.
    tracked: Vec<(String, [f32; 3])>,
    /// Animation ▸ "Use Old Fading Method": a plain queue applying each
    /// expression in turn instead of the composite blend (§3.3, §3.7).
    pub old_fading: bool,
}

/// exp3 fade default when absent (viewer.md §3.3).
pub const DEFAULT_EXPRESSION_FADE: f32 = 1.0;

impl ExpressionPlayer {
    pub fn start(&mut self, exp: Arc<Expression>) {
        for e in &mut self.queue {
            e.pending_fade_out = Some(e.exp.fade_out.unwrap_or(DEFAULT_EXPRESSION_FADE));
        }
        for p in &exp.params {
            if !self.tracked.iter().any(|(id, _)| *id == p.id) {
                self.tracked.push((p.id.clone(), [0.0, 1.0, 0.0]));
            }
        }
        self.queue.push(ExpEntry {
            exp,
            start: None,
            fade_in_start: 0.0,
            end: -1.0,
            pending_fade_out: None,
        });
    }

    /// Returns to the neutral face by queueing an empty expression, which
    /// fades in over the default time and then replaces the others.
    pub fn reset(&mut self) {
        if !self.queue.is_empty() {
            self.start(Arc::new(Expression::default()));
        }
    }

    pub fn update(&mut self, dt: f32, p: &mut Params) {
        if self.queue.is_empty() {
            return;
        }
        self.clock += f64::from(dt.max(0.0));
        if self.old_fading {
            self.update_old(p);
            return;
        }
        let t_now = self.clock;
        // Current values: the same for every entry this frame.
        let current: Vec<f32> = self.tracked.iter().map(|(id, _)| p.get(id)).collect();
        let mut sum_fi = 0.0f32;
        let n = self.queue.len();
        let mut newest_w = 0.0;
        for (i, e) in self.queue.iter_mut().enumerate() {
            let start = *e.start.get_or_insert_with(|| {
                e.fade_in_start = t_now;
                t_now
            });
            let _ = start;
            let fi_time = e.exp.fade_in.unwrap_or(DEFAULT_EXPRESSION_FADE);
            let fo_time = e.exp.fade_out.unwrap_or(DEFAULT_EXPRESSION_FADE);
            let fi = if fi_time > 0.0 {
                ease(((t_now - e.fade_in_start) as f32) / fi_time)
            } else {
                1.0
            };
            let fo = if fo_time > 0.0 && e.end >= 0.0 {
                ease(((e.end - t_now) as f32) / fo_time)
            } else {
                1.0
            };
            let w = fi * fo;
            for (k, (id, st)) in self.tracked.iter_mut().enumerate() {
                let c = current[k];
                let (a, m, o) = match e.exp.params.iter().find(|x| x.id == *id) {
                    Some(x) => match x.blend {
                        ExpBlend::Add => (x.value, 1.0, c),
                        ExpBlend::Multiply => (0.0, x.value, c),
                        ExpBlend::Overwrite => (0.0, 1.0, x.value),
                    },
                    None => (0.0, 1.0, c),
                };
                if i == 0 {
                    *st = [a, m, o];
                } else {
                    st[0] += (a - st[0]) * w;
                    st[1] += (m - st[1]) * w;
                    // Overwrite restarts from c at every entry (reference quirk).
                    st[2] = c + (o - c) * w;
                }
            }
            sum_fi += fi;
            if let Some(f) = e.pending_fade_out.take() {
                let end = t_now + f64::from(f);
                if !(e.end > 0.0 && e.end <= end) {
                    e.end = end;
                }
            }
            if i + 1 == n {
                newest_w = w;
            }
        }
        // 2. The newest entry fully faded in replaces the older ones.
        if n > 1 && newest_w >= 1.0 {
            self.queue.drain(..n - 1);
        }
        // 3–4.
        let weight = sum_fi.min(1.0);
        for (id, st) in &mut self.tracked {
            p.set(id, (st[2] + st[0]) * st[1], weight);
            st[0] = 0.0;
            st[1] = 1.0;
        }
    }
}

impl ExpressionPlayer {
    /// The old fading method: an ordinary queue (§3.2 rules); each entry, in
    /// queue order, applies its parameters in file order with weight
    /// ease(fade-in)·ease(fade-out). spec: viewer.md §3.3
    fn update_old(&mut self, p: &mut Params) {
        let t_now = self.clock;
        for e in &mut self.queue {
            e.start.get_or_insert_with(|| {
                e.fade_in_start = t_now;
                t_now
            });
            let fi_time = e.exp.fade_in.unwrap_or(DEFAULT_EXPRESSION_FADE);
            let fo_time = e.exp.fade_out.unwrap_or(DEFAULT_EXPRESSION_FADE);
            let fi = if fi_time > 0.0 {
                ease(((t_now - e.fade_in_start) as f32) / fi_time)
            } else {
                1.0
            };
            let fo = if fo_time > 0.0 && e.end >= 0.0 {
                ease(((e.end - t_now) as f32) / fo_time)
            } else {
                1.0
            };
            let w = fi * fo;
            for x in &e.exp.params {
                match x.blend {
                    ExpBlend::Add => p.add(&x.id, x.value, w),
                    ExpBlend::Multiply => p.mul(&x.id, x.value, w),
                    ExpBlend::Overwrite => p.set(&x.id, x.value, w),
                }
            }
            if let Some(f) = e.pending_fade_out.take() {
                let end = t_now + f64::from(f);
                if !(e.end > 0.0 && e.end <= end) {
                    e.end = end;
                }
            }
        }
        self.queue.retain(|e| !(e.end > 0.0 && e.end < t_now));
    }
}

/// Breath: ParamBreath += 0.5 + 0.5·sin(2π·T/3.2345), weight 1 (§3.1 step 7).
#[derive(Default)]
pub struct Breath {
    clock: f64,
}

impl Breath {
    pub fn update(&mut self, dt: f32, p: &mut Params) {
        self.clock += f64::from(dt.max(0.0));
        let v = 0.5 + 0.5 * (std::f64::consts::TAU * self.clock / 3.2345).sin();
        if p.index.contains_key("ParamBreath") {
            p.add("ParamBreath", v as f32, 1.0);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BlinkState {
    #[default]
    First,
    Interval,
    Closing,
    Closed,
    Opening,
}

/// Small deterministic generator for the blink interval (xorshift64*).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn from_time() -> Self {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Self::new(t ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Automatic eye blink. Its clock advances only on frames where it runs
/// (§3.1 step 4). spec: viewer.md §3.5
#[derive(Clone, Debug)]
pub struct EyeBlink {
    clock: f64,
    state: BlinkState,
    state_start: f64,
    next_blink: f64,
    rng: Rng,
}

impl Default for EyeBlink {
    fn default() -> Self {
        Self::with_rng(Rng::from_time())
    }
}

impl EyeBlink {
    const CLOSING: f64 = 0.1;
    const CLOSED: f64 = 0.05;
    const OPENING: f64 = 0.15;
    /// Nominal interval 4 s; the gap is r·(2·4 − 1), uniform over 0–7 s.
    const INTERVAL: f64 = 4.0;

    pub fn with_rng(rng: Rng) -> Self {
        Self {
            clock: 0.0,
            state: BlinkState::First,
            state_start: 0.0,
            next_blink: 0.0,
            rng,
        }
    }

    fn schedule(&mut self) {
        self.next_blink = self.clock + self.rng.next_f64() * (2.0 * Self::INTERVAL - 1.0);
    }

    /// Advances the blink and writes E (0 = closed, 1 = open) to every
    /// eye-blink parameter with weight 1.
    pub fn update(&mut self, dt: f32, p: &mut Params, eye_blink: &[usize]) {
        self.clock += f64::from(dt.max(0.0));
        let now = self.clock;
        if self.state == BlinkState::First {
            self.schedule();
            self.state = BlinkState::Interval;
        }
        let e = match self.state {
            BlinkState::First | BlinkState::Interval => {
                if self.next_blink < now {
                    self.state = BlinkState::Closing;
                    self.state_start = now;
                }
                1.0
            }
            BlinkState::Closing => {
                let mut u = (now - self.state_start) / Self::CLOSING;
                if u >= 1.0 {
                    u = 1.0;
                    self.state = BlinkState::Closed;
                    self.state_start = now;
                }
                1.0 - u
            }
            BlinkState::Closed => {
                if (now - self.state_start) / Self::CLOSED >= 1.0 {
                    self.state = BlinkState::Opening;
                    self.state_start = now;
                }
                0.0
            }
            BlinkState::Opening => {
                let mut u = (now - self.state_start) / Self::OPENING;
                if u >= 1.0 {
                    u = 1.0;
                    self.schedule();
                    self.state = BlinkState::Interval;
                }
                u
            }
        };
        for &i in eye_blink {
            p.set_index(i, e as f32, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::ExpParam;

    struct Fixture {
        values: Vec<f32>,
        ranges: Vec<ParamRange>,
        index: HashMap<String, usize>,
        virtuals: HashMap<String, f32>,
    }

    impl Fixture {
        fn new() -> Self {
            let r = ParamRange {
                min: -10.0,
                max: 10.0,
                repeat: false,
            };
            Self {
                values: vec![0.0, 1.0, 1.0],
                ranges: vec![
                    r,
                    ParamRange {
                        min: 0.0,
                        max: 1.0,
                        repeat: false,
                    },
                    ParamRange {
                        min: 0.0,
                        max: 1.0,
                        repeat: false,
                    },
                ],
                index: HashMap::from([("A".into(), 0), ("EyeL".into(), 1), ("EyeR".into(), 2)]),
                virtuals: HashMap::new(),
            }
        }
        fn params(&mut self) -> Params<'_> {
            Params {
                values: &mut self.values,
                ranges: &self.ranges,
                index: &self.index,
                virtuals: &mut self.virtuals,
            }
        }
    }

    #[test]
    fn repeat_wraps_and_virtuals_are_created() {
        let r = ParamRange {
            min: -180.0,
            max: 180.0,
            repeat: true,
        };
        assert_eq!(r.fit(190.0), -170.0);
        let mut f = Fixture::new();
        let mut p = f.params();
        assert_eq!(p.get("PartArm"), 0.0);
        p.set("PartArm", 1.0, 1.0);
        p.set("A", 99.0, 1.0);
        assert_eq!(f.virtuals["PartArm"], 1.0);
        assert_eq!(f.values[0], 10.0);
    }

    #[test]
    fn motion_queue_fades_and_eye_blink() {
        let m = Motion::parse(
            r#"{"Meta":{"Duration":1,"Fps":30,"FadeInTime":0.5,"FadeOutTime":0},"Curves":[
            {"Target":"Parameter","Id":"A","Segments":[0,10,0,1,10]},
            {"Target":"Parameter","Id":"EyeL","Segments":[0,0.8,0,1,0.8]},
            {"Target":"Model","Id":"EyeBlink","Segments":[0,0.5,0,1,0.5]},
            {"Target":"PartOpacity","Id":"PartArm","Segments":[0,1,0,1,1]}]}"#,
        )
        .unwrap();
        let mut f = Fixture::new();
        let mut out = MotionOutputs::default();
        let mut pl = MotionPlayer::default();
        pl.start(Arc::new(m), false);
        // First update at clock 0 sets the start; then 0.25 s in: ease(0.5) = 0.5.
        pl.update(0.0, &mut f.params(), &[1, 2], &[], &mut out);
        f.values = vec![0.0, 1.0, 1.0];
        let u = pl.update(0.25, &mut f.params(), &[1, 2], &[], &mut out);
        assert!(u.updated);
        assert!((f.values[0] - 5.0).abs() < 1e-4, "{:?}", f.values);
        // Fully faded in: EyeL has its own curve: 0.8 × 0.5; EyeR is set to E.
        f.values = vec![0.0, 1.0, 1.0];
        pl.update(0.35, &mut f.params(), &[1, 2], &[], &mut out);
        assert!((f.values[1] - 0.4).abs() < 1e-4, "{:?}", f.values);
        assert!((f.values[2] - 0.5).abs() < 1e-4);
        assert_eq!(f.virtuals["PartArm"], 1.0);
        // Ends after its duration.
        pl.update(1.0, &mut f.params(), &[1, 2], &[], &mut out);
        assert!(!pl.is_playing());
    }

    #[test]
    fn expression_manager_blend() {
        let exp = |v: f32, blend| {
            Arc::new(Expression {
                fade_in: Some(0.0),
                fade_out: Some(0.0),
                params: vec![ExpParam {
                    id: "A".into(),
                    value: v,
                    blend,
                }],
            })
        };
        let mut f = Fixture::new();
        let mut ep = ExpressionPlayer::default();
        // A lone Add expression with zero fade: A = (c + V) · 1.
        ep.start(exp(2.0, ExpBlend::Add));
        ep.update(0.1, &mut f.params());
        assert_eq!(f.values[0], 2.0);
        // Multiply on top: the newest entry (weight 1) replaces the older one.
        f.values[0] = 3.0;
        ep.start(exp(0.5, ExpBlend::Multiply));
        ep.update(0.1, &mut f.params());
        // Entry 0 (Add 2): (2, 1, 3); entry 1 (Mul 0.5, w=1): add→0, mul→0.5, ow→3 → (3+0)·0.5.
        assert_eq!(f.values[0], 1.5);
        assert_eq!(ep.queue.len(), 1);
        // Overwrite.
        f.values[0] = 3.0;
        ep.start(exp(-4.0, ExpBlend::Overwrite));
        ep.update(0.1, &mut f.params());
        assert_eq!(f.values[0], -4.0);
    }

    #[test]
    fn expression_fades_in_with_default_time() {
        let e = Arc::new(Expression {
            fade_in: None,
            fade_out: None,
            params: vec![ExpParam {
                id: "A".into(),
                value: 4.0,
                blend: ExpBlend::Add,
            }],
        });
        let mut f = Fixture::new();
        let mut ep = ExpressionPlayer::default();
        ep.start(e);
        ep.update(0.0, &mut f.params());
        assert_eq!(f.values[0], 0.0); // W = ease(0) = 0
        f.values[0] = 0.0;
        ep.update(0.5, &mut f.params());
        assert!((f.values[0] - 2.0).abs() < 1e-4); // W = ease(0.5) = 0.5 → 0·0.5 + 4·0.5
    }

    #[test]
    fn eye_blink_cycle() {
        let mut f = Fixture::new();
        let mut b = EyeBlink::with_rng(Rng::new(7));
        // First update schedules within 0–7 s and writes "open".
        b.update(0.0, &mut f.params(), &[1, 2]);
        assert!(b.next_blink >= 0.0 && b.next_blink < 7.0);
        assert_eq!(f.values[1], 1.0);
        let gap = b.next_blink;
        // Run past the scheduled time: closing starts on the first frame after it.
        let mut seen = Vec::new();
        let dt = 0.01;
        for _ in 0..((gap + 0.5) / 0.01) as usize {
            b.update(dt, &mut f.params(), &[1, 2]);
            seen.push(f.values[1]);
        }
        let min = seen.iter().copied().fold(f32::MAX, f32::min);
        assert_eq!(min, 0.0, "the eyes close fully");
        assert_eq!(*seen.last().unwrap(), 1.0, "and open again");
        assert_eq!(f.values[1], f.values[2]);
        // Closing → closed → opening takes 0.1 + 0.05 + 0.15 s (± a frame each).
        let shut = seen.iter().filter(|v| **v < 1.0).count() as f64 * f64::from(dt);
        assert!((0.28..=0.34).contains(&shut), "{shut}");
        // The clock only moves when updated: no blink without updates.
        let clock = b.clock;
        assert!((clock - seen.len() as f64 * f64::from(dt)).abs() < 1e-6);
    }

    #[test]
    fn old_fading_method_applies_in_order() {
        let mut f = Fixture::new();
        let mut ep = ExpressionPlayer {
            old_fading: true,
            ..ExpressionPlayer::default()
        };
        ep.start(Arc::new(Expression {
            fade_in: Some(0.0),
            fade_out: Some(0.0),
            params: vec![
                ExpParam {
                    id: "A".into(),
                    value: 2.0,
                    blend: ExpBlend::Add,
                },
                ExpParam {
                    id: "A".into(),
                    value: 3.0,
                    blend: ExpBlend::Multiply,
                },
            ],
        }));
        f.values[0] = 1.0;
        ep.update(0.1, &mut f.params());
        // (1 + 2) · 3, in file order.
        assert_eq!(f.values[0], 9.0);
        // Half faded in with the default 1 s: add 2·0.5 then ×(1 + 2·0.5).
        let mut ep = ExpressionPlayer {
            old_fading: true,
            ..ExpressionPlayer::default()
        };
        ep.start(Arc::new(Expression {
            fade_in: None,
            fade_out: None,
            params: vec![ExpParam {
                id: "A".into(),
                value: 4.0,
                blend: ExpBlend::Overwrite,
            }],
        }));
        f.values[0] = 0.0;
        ep.update(0.0, &mut f.params());
        f.values[0] = 0.0;
        ep.update(0.5, &mut f.params());
        assert!((f.values[0] - 2.0).abs() < 1e-5, "{}", f.values[0]);
    }

    #[test]
    fn old_playback_forces_restricted_beziers() {
        // An unrestricted bezier whose handles reach past the segment.
        let m = Arc::new(
            Motion::parse(
                r#"{"Meta":{"Duration":1,"Fps":30,"FadeInTime":0,"FadeOutTime":0,"AreBeziersRestricted":false},"Curves":[
            {"Target":"Parameter","Id":"A","Segments":[0,0,1,0.9,10,0.95,-10,1,0]}]}"#,
            )
            .unwrap(),
        );
        let run = |force: bool| {
            let mut f = Fixture::new();
            let mut pl = MotionPlayer {
                force_restricted_beziers: force,
                ..MotionPlayer::default()
            };
            pl.start(m.clone(), false);
            let mut out = MotionOutputs::default();
            pl.update(0.0, &mut f.params(), &[], &[], &mut out);
            pl.update(0.3, &mut f.params(), &[], &[], &mut out);
            f.values[0]
        };
        let curve = &m.curves[0];
        assert_eq!(run(false), curve.eval(0.3, false));
        assert_eq!(run(true), curve.eval(0.3, true));
        assert_ne!(run(false), run(true));
    }
}
