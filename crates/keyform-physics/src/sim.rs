//! Pendulum simulation. spec: 70-advanced/physics.md §2–3

use std::borrow::Borrow;

use keyform_model::{Parameter, PhysicsKind, PhysicsRig, PhysicsSetting};

type V2 = [f32; 2];

fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}
fn scale(a: V2, s: f32) -> V2 {
    [a[0] * s, a[1] * s]
}
fn normalize(a: V2) -> V2 {
    let l = (a[0] * a[0] + a[1] * a[1]).sqrt();
    if l > 0.0 {
        [a[0] / l, a[1] / l]
    } else {
        [0.0, 0.0]
    }
}

/// θ(b) − θ(a), wrapped into (−π, π].
fn delta(a: V2, b: V2) -> f32 {
    let mut d = b[1].atan2(b[0]) - a[1].atan2(a[0]);
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d <= -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Particle {
    position: V2,
    prev: V2,
    velocity: V2,
    force: V2,
    prev_gravity: V2,
}

/// Runtime state of every setting's particles.
#[derive(Clone, Debug, Default)]
pub struct PhysicsSim {
    particles: Vec<Vec<Particle>>,
    /// Largest |value| an output produced (editor "exceeded range" hint).
    pub output_extremes: Vec<Vec<[f32; 2]>>,
}

impl PhysicsSim {
    pub fn new(rig: &PhysicsRig) -> Self {
        let mut s = Self::default();
        s.reset(rig);
        s
    }

    /// spec §2
    pub fn reset(&mut self, rig: &PhysicsRig) {
        self.particles = rig
            .settings
            .iter()
            .map(|st| {
                let mut v: Vec<Particle> = Vec::with_capacity(st.vertices.len());
                for (i, vx) in st.vertices.iter().enumerate() {
                    let position = if i == 0 {
                        [0.0, 0.0]
                    } else {
                        add(v[i - 1].position, [0.0, vx.radius])
                    };
                    v.push(Particle {
                        position,
                        prev: position,
                        velocity: [0.0; 2],
                        force: [0.0; 2],
                        prev_gravity: rig.gravity,
                    });
                }
                v
            })
            .collect();
        self.output_extremes = rig
            .settings
            .iter()
            .map(|s| vec![[f32::INFINITY, f32::NEG_INFINITY]; s.outputs.len()])
            .collect();
    }

    /// Advances `dt` seconds, reading inputs from and writing outputs to
    /// `values` (indexed like `params`). Disabled settings are skipped.
    pub fn step(
        &mut self,
        rig: &PhysicsRig,
        params: &[impl Borrow<Parameter>],
        values: &mut [f32],
        dt: f32,
    ) {
        if self.particles.len() != rig.settings.len() {
            self.reset(rig);
        }
        for (si, st) in rig.settings.iter().enumerate() {
            if !st.enabled || self.particles[si].len() != st.vertices.len() {
                continue;
            }
            step_setting(
                st,
                rig,
                params,
                values,
                dt,
                &mut self.particles[si],
                &mut self.output_extremes[si],
            );
        }
    }
}

fn normalized_input(st: &PhysicsSetting, kind: PhysicsKind, p: &Parameter, v: f32) -> f32 {
    let n = if kind == PhysicsKind::Angle {
        st.angle
    } else {
        st.position
    };
    let (lo, hi) = (p.min.min(p.max), p.min.max(p.max));
    if v > hi || v < lo {
        return 0.0;
    }
    let mid = (p.min + p.max) / 2.0;
    let d = v - mid;
    if d > 0.0 {
        let den = hi - mid;
        if den == 0.0 {
            0.0
        } else {
            d * (n.maximum - n.default) / den + n.default
        }
    } else if d < 0.0 {
        let den = lo - mid;
        if den == 0.0 {
            0.0
        } else {
            d * (n.minimum - n.default) / den + n.default
        }
    } else {
        n.default
    }
}

fn step_setting(
    st: &PhysicsSetting,
    rig: &PhysicsRig,
    params: &[impl Borrow<Parameter>],
    values: &mut [f32],
    dt: f32,
    ps: &mut [Particle],
    extremes: &mut [[f32; 2]],
) {
    // §3.1 inputs.
    let (mut tr, mut angle) = ([0.0f32, 0.0f32], 0.0f32);
    for inp in &st.inputs {
        let (Some(p), Some(&v)) = (
            params.get(inp.parameter).map(Borrow::<Parameter>::borrow),
            values.get(inp.parameter),
        ) else {
            continue;
        };
        let mut n = normalized_input(st, inp.kind, p, v);
        // Not reflected means negated (§3.1).
        if !inp.reflect {
            n = -n;
        }
        let w = n * inp.weight / 100.0;
        match inp.kind {
            PhysicsKind::X => tr[0] += w,
            PhysicsKind::Y => tr[1] += w,
            PhysicsKind::Angle => angle += w,
        }
    }
    // §3.2 rotate by −A, using the already updated x for y (reproduced on purpose).
    let r = (-angle).to_radians();
    tr[0] = tr[0] * r.cos() - tr[1] * r.sin();
    tr[1] = tr[0] * r.sin() + tr[1] * r.cos();

    // §3.3 particles.
    if dt != 0.0 && !ps.is_empty() {
        ps[0].position = tr;
        let base = delta([0.0, 1.0], rig.gravity);
        let a = angle.to_radians() + base;
        let dir = normalize([a.sin(), a.cos()]);
        let threshold = 0.001 * st.position.maximum;
        for i in 1..ps.len() {
            let q = ps[i - 1];
            let vx = &st.vertices[i];
            let p = &mut ps[i];
            p.force = add(scale(dir, vx.acceleration), rig.wind);
            p.prev = p.position;
            let mut d = sub(p.position, q.position);
            let rho = delta(p.prev_gravity, dir) / 5.0;
            d[0] = rho.cos() * d[0] - d[1] * rho.sin();
            d[1] = rho.sin() * d[0] + d[1] * rho.cos();
            p.position = add(q.position, d);
            let tau = vx.delay * dt * 30.0;
            p.position = add(
                p.position,
                add(scale(p.velocity, tau), scale(p.force, tau * tau)),
            );
            p.position = add(
                q.position,
                scale(normalize(sub(p.position, q.position)), vx.radius),
            );
            if p.position[0].abs() < threshold {
                p.position[0] = 0.0;
            }
            if tau != 0.0 {
                p.velocity = scale(scale(sub(p.position, p.prev), 1.0 / tau), vx.mobility);
            }
            p.force = [0.0, 0.0];
            p.prev_gravity = dir;
        }
    }

    // §3.4 outputs.
    for (oi, out) in st.outputs.iter().enumerate() {
        let k = out.vertex_index;
        if k < 1 || k >= ps.len() {
            continue;
        }
        let t = sub(ps[k].position, ps[k - 1].position);
        let mut value = match out.kind {
            PhysicsKind::X => t[0],
            PhysicsKind::Y => t[1],
            PhysicsKind::Angle => {
                let parent = if k >= 2 {
                    sub(ps[k - 1].position, ps[k - 2].position)
                } else {
                    [0.0, 1.0]
                };
                delta(parent, t)
            }
        };
        if out.reflect {
            value = -value;
        }
        value *= out.scale();
        if let Some(e) = extremes.get_mut(oi) {
            e[0] = e[0].min(value);
            e[1] = e[1].max(value);
        }
        let (Some(p), Some(slot)) = (
            params.get(out.parameter).map(Borrow::<Parameter>::borrow),
            values.get_mut(out.parameter),
        ) else {
            continue;
        };
        let c = value.clamp(p.min.min(p.max), p.min.max(p.max));
        let w = out.weight / 100.0;
        *slot = if w >= 1.0 {
            c
        } else {
            *slot * (1.0 - w) + c * w
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::*;

    fn rig() -> (PhysicsRig, Vec<Parameter>) {
        let params = vec![
            Parameter::new("In", -30.0, 30.0, 0.0),
            Parameter::new("Out", -30.0, 30.0, 0.0),
        ];
        let v = |r: f32| PhysicsVertex {
            position: [0.0, r],
            mobility: 0.95,
            delay: 0.9,
            acceleration: 1.5,
            radius: r,
        };
        let setting = PhysicsSetting {
            guid: String::new(),
            id: "PhysicsSetting1".into(),
            name: "Hair".into(),
            enabled: true,
            inputs: vec![PhysicsInput {
                parameter: 0,
                weight: 100.0,
                kind: PhysicsKind::X,
                reflect: false,
            }],
            outputs: vec![PhysicsOutput {
                parameter: 1,
                vertex_index: 1,
                translation_scale: [0.0, 0.0],
                angle_scale: 30.0,
                weight: 100.0,
                kind: PhysicsKind::Angle,
                reflect: false,
            }],
            vertices: vec![v(0.0), v(3.0)],
            position: Normalization::default(),
            angle: Normalization::default(),
        };
        (
            PhysicsRig {
                settings: vec![setting],
                ..PhysicsRig::default()
            },
            params,
        )
    }

    #[test]
    fn normalisation() {
        let (r, p) = rig();
        let st = &r.settings[0];
        assert_eq!(normalized_input(st, PhysicsKind::X, &p[0], 30.0), 10.0);
        assert_eq!(normalized_input(st, PhysicsKind::X, &p[0], -15.0), -5.0);
        assert_eq!(normalized_input(st, PhysicsKind::X, &p[0], 0.0), 0.0);
        assert_eq!(normalized_input(st, PhysicsKind::X, &p[0], 31.0), 0.0);
    }

    #[test]
    fn rest_is_stable_and_motion_swings_then_settles() {
        let (r, p) = rig();
        let mut sim = PhysicsSim::new(&r);
        let mut vals = vec![0.0, 0.0];
        for _ in 0..120 {
            sim.step(&r, &p, &mut vals, 1.0 / 60.0);
        }
        assert!(vals[1].abs() < 1e-3, "at rest: {vals:?}");
        // Step input: the pendulum swings (output ≠ 0), then settles back
        // toward hanging straight down.
        vals[0] = 30.0;
        let mut peak = 0.0f32;
        for _ in 0..30 {
            sim.step(&r, &p, &mut vals, 1.0 / 60.0);
            peak = peak.max(vals[1].abs());
        }
        assert!(peak > 1.0, "no swing: {peak}");
        for _ in 0..1200 {
            sim.step(&r, &p, &mut vals, 1.0 / 60.0);
        }
        assert!(vals[1].abs() < 0.5, "did not settle: {vals:?}");
        // dt = 0 changes nothing.
        let before = vals.clone();
        sim.step(&r, &p, &mut vals, 0.0);
        assert_eq!(before, vals);
    }
}
