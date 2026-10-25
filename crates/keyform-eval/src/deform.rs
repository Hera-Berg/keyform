//! Warp and rotation deformer local→canvas maps.
//!
//! spec: 30-algorithms/deformers.md

/// A warp deformer's deformed grid (canvas space).
#[derive(Clone, Debug, PartialEq)]
pub struct WarpMap {
    pub cols: usize,
    pub rows: usize,
    pub quad: bool,
    /// (cols+1)·(rows+1) points, x fastest, x,y interleaved.
    pub points: Vec<f32>,
}

type V2 = [f32; 2];

fn add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: V2, s: f32) -> V2 {
    [a[0] * s, a[1] * s]
}

impl WarpMap {
    fn p(&self, i: usize, j: usize) -> V2 {
        let k = 2 * (j * (self.cols + 1) + i);
        [self.points[k], self.points[k + 1]]
    }

    /// Affine approximation A(u, v) from the four corners. spec: deformers.md §1.3
    fn affine(&self) -> (V2, V2, V2) {
        let p00 = self.p(0, 0);
        let p10 = self.p(self.cols, 0);
        let p01 = self.p(0, self.rows);
        let p11 = self.p(self.cols, self.rows);
        let c = mul(add(add(p00, p10), add(p01, p11)), 0.25);
        let d1 = sub(p11, p00);
        let d2 = sub(p10, p01);
        let x = mul(add(d1, d2), 0.5);
        let y = mul(sub(d1, d2), 0.5);
        let o = sub(c, mul(add(x, y), 0.5));
        (o, x, y)
    }

    /// Maps warp-local (u, v) to canvas.
    pub fn map(&self, u: f32, v: f32) -> V2 {
        let (cols, rows) = (self.cols as f32, self.rows as f32);
        if (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v) {
            // §1.2 inside.
            let x = u * cols;
            let y = v * rows;
            let i = (x.floor() as usize).min(self.cols - 1);
            let j = (y.floor() as usize).min(self.rows - 1);
            let s = x - i as f32;
            let t = y - j as f32;
            let a = self.p(i, j);
            let b = self.p(i + 1, j);
            let c = self.p(i, j + 1);
            let d = self.p(i + 1, j + 1);
            if self.quad {
                let w = [(1.0 - s) * (1.0 - t), s * (1.0 - t), (1.0 - s) * t, s * t];
                return [
                    a[0] * w[0] + b[0] * w[1] + c[0] * w[2] + d[0] * w[3],
                    a[1] * w[0] + b[1] * w[1] + c[1] * w[2] + d[1] * w[3],
                ];
            }
            return if s + t < 1.0 {
                add(add(mul(a, 1.0 - s - t), mul(b, s)), mul(c, t))
            } else {
                add(add(mul(d, s + t - 1.0), mul(c, 1.0 - s)), mul(b, 1.0 - t))
            };
        }

        let (o, ax, ay) = self.affine();
        let aff = |u: f32, v: f32| add(add(o, mul(ax, u)), mul(ay, v));
        // §1.3 far region.
        if u <= -2.0 || u >= 3.0 || v <= -2.0 || v >= 3.0 {
            return aff(u, v);
        }
        let quad = |q00: V2, q10: V2, q01: V2, q11: V2, s: f32, t: f32| -> V2 {
            if s + t <= 1.0 {
                add(add(q00, mul(sub(q10, q00), s)), mul(sub(q01, q00), t))
            } else {
                add(
                    add(q11, mul(sub(q01, q11), 1.0 - s)),
                    mul(sub(q10, q11), 1.0 - t),
                )
            }
        };
        let x = u * cols;
        let y = v * rows;
        let i = (x.floor().max(0.0) as usize).min(self.cols - 1);
        let j = (y.floor().max(0.0) as usize).min(self.rows - 1);
        let (fi, fj) = (i as f32, j as f32);
        let (c, r) = (self.cols, self.rows);
        // Border ring regions, in the spec's order.
        if u <= 0.0 && v <= 0.0 {
            quad(
                aff(-2.0, -2.0),
                aff(0.0, -2.0),
                aff(-2.0, 0.0),
                self.p(0, 0),
                (u + 2.0) / 2.0,
                (v + 2.0) / 2.0,
            )
        } else if u <= 0.0 && v >= 1.0 {
            quad(
                aff(-2.0, 1.0),
                self.p(0, r),
                aff(-2.0, 3.0),
                aff(0.0, 3.0),
                (u + 2.0) / 2.0,
                (v - 1.0) / 2.0,
            )
        } else if u <= 0.0 {
            quad(
                aff(-2.0, fj / rows),
                self.p(0, j),
                aff(-2.0, (fj + 1.0) / rows),
                self.p(0, j + 1),
                (u + 2.0) / 2.0,
                y - fj,
            )
        } else if u >= 1.0 && v <= 0.0 {
            quad(
                aff(1.0, -2.0),
                aff(3.0, -2.0),
                self.p(c, 0),
                aff(3.0, 0.0),
                (u - 1.0) / 2.0,
                (v + 2.0) / 2.0,
            )
        } else if u >= 1.0 && v >= 1.0 {
            quad(
                self.p(c, r),
                aff(3.0, 1.0),
                aff(1.0, 3.0),
                aff(3.0, 3.0),
                (u - 1.0) / 2.0,
                (v - 1.0) / 2.0,
            )
        } else if u >= 1.0 {
            quad(
                self.p(c, j),
                aff(3.0, fj / rows),
                self.p(c, j + 1),
                aff(3.0, (fj + 1.0) / rows),
                (u - 1.0) / 2.0,
                y - fj,
            )
        } else if v <= 0.0 {
            quad(
                aff(fi / cols, -2.0),
                aff((fi + 1.0) / cols, -2.0),
                self.p(i, 0),
                self.p(i + 1, 0),
                x - fi,
                (v + 2.0) / 2.0,
            )
        } else {
            quad(
                self.p(i, r),
                self.p(i + 1, r),
                aff(fi / cols, 3.0),
                aff((fi + 1.0) / cols, 3.0),
                x - fi,
                (v - 1.0) / 2.0,
            )
        }
    }
}

/// A rotation deformer's deformed transform (canvas space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotationMap {
    pub origin: V2,
    /// Effective angle in degrees: deformed angle + base angle.
    pub angle: f32,
    /// Total scale along the rotation chain.
    pub scale: f32,
    pub reflect_x: bool,
    pub reflect_y: bool,
}

impl RotationMap {
    /// spec: deformers.md §2.1
    pub fn map(&self, p: V2) -> V2 {
        let (s, c) = self.angle.to_radians().sin_cos();
        let rx = if self.reflect_x { -1.0 } else { 1.0 };
        let ry = if self.reflect_y { -1.0 } else { 1.0 };
        let (x, y) = (rx * p[0], ry * p[1]);
        [
            self.scale * (c * x - s * y) + self.origin[0],
            self.scale * (s * x + c * y) + self.origin[1],
        ]
    }

    /// Canvas → local. spec: deformers.md §2.2
    pub fn inverse(&self, q: V2) -> V2 {
        let (s, c) = self.angle.to_radians().sin_cos();
        let (dx, dy) = (q[0] - self.origin[0], q[1] - self.origin[1]);
        let x = c * dx + s * dy;
        let y = -s * dx + c * dy;
        let rx = if self.reflect_x { -1.0 } else { 1.0 };
        let ry = if self.reflect_y { -1.0 } else { 1.0 };
        [x / (self.scale * rx), y / (self.scale * ry)]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DeformerMap {
    Warp(WarpMap),
    Rotation(RotationMap),
}

impl DeformerMap {
    pub fn map(&self, p: V2) -> V2 {
        match self {
            DeformerMap::Warp(w) => w.map(p[0], p[1]),
            DeformerMap::Rotation(r) => r.map(p),
        }
    }
}

/// Angle (degrees) a parent map turns the local "up" direction at `o`.
/// spec: deformers.md §3.2 step 2
pub fn inherited_rotation(parent: &DeformerMap, o: V2) -> f32 {
    let h = match parent {
        DeformerMap::Rotation(_) => -10.0,
        DeformerMap::Warp(_) => -0.1,
    };
    let base = parent.map(o);
    let mut k = 1.0f32;
    let mut d = [0.0f32, 0.0];
    for _ in 0..10 {
        let fwd = sub(parent.map([o[0], o[1] + k * h]), base);
        if fwd != [0.0, 0.0] {
            d = fwd;
            break;
        }
        let back = sub(parent.map([o[0], o[1] - k * h]), base);
        if back != [0.0, 0.0] {
            d = mul(back, -1.0);
            break;
        }
        k *= 0.1;
    }
    if d == [0.0, 0.0] {
        return 0.0;
    }
    let delta = (d[1].atan2(d[0]) - h.signum().atan2(0.0)).to_degrees();
    // Wrap to (−180, 180].
    let mut a = delta % 360.0;
    if a > 180.0 {
        a -= 360.0;
    } else if a <= -180.0 {
        a += 360.0;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(cols: usize, rows: usize, f: impl Fn(f32, f32) -> V2) -> WarpMap {
        let mut points = Vec::new();
        for j in 0..=rows {
            for i in 0..=cols {
                let p = f(i as f32 / cols as f32, j as f32 / rows as f32);
                points.extend_from_slice(&p);
            }
        }
        WarpMap {
            cols,
            rows,
            quad: false,
            points,
        }
    }

    fn close(a: V2, b: V2) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn affine_grid_maps_affinely_everywhere() {
        // An affine grid: inside, ring and far region must all agree with the affine map.
        let f = |u: f32, v: f32| [100.0 + 200.0 * u + 30.0 * v, 50.0 - 10.0 * u + 300.0 * v];
        for quad in [false, true] {
            let mut w = grid(4, 3, f);
            w.quad = quad;
            for &u in &[-3.0, -2.0, -1.5, -0.25, 0.0, 0.3, 0.999, 1.0, 1.7, 2.9, 3.5] {
                for &v in &[-2.5, -1.0, 0.0, 0.5, 0.99, 1.0, 2.2, 4.0] {
                    assert!(
                        close(w.map(u, v), f(u, v)),
                        "quad={quad} u={u} v={v}: {:?} vs {:?}",
                        w.map(u, v),
                        f(u, v)
                    );
                }
            }
        }
    }

    #[test]
    fn warp_hits_grid_points_and_is_continuous_at_border() {
        let w = grid(2, 2, |u, v| {
            [u * 100.0 + (v * 3.0).sin() * 10.0, v * 100.0 + u * u * 20.0]
        });
        assert!(close(w.map(0.5, 0.5), w.p(1, 1)));
        assert!(close(w.map(0.0, 0.0), w.p(0, 0)));
        // Continuity across the grid border (steps of 1e-5 move ≤ ~0.003 px here).
        let near = |a: V2, b: V2| (a[0] - b[0]).abs() < 1e-2 && (a[1] - b[1]).abs() < 1e-2;
        for &t in &[0.1f32, 0.5, 0.9] {
            assert!(near(w.map(1.0 - 1e-5, t), w.map(1.0, t)));
            assert!(near(w.map(t, -1e-5), w.map(t, 1e-5)));
            assert!(near(w.map(-1e-5, t), w.map(1e-5, t)));
            assert!(near(w.map(t, 1.0 - 1e-5), w.map(t, 1.0)));
        }
    }

    #[test]
    fn rotation_clockwise_on_screen() {
        let r = RotationMap {
            origin: [10.0, 20.0],
            angle: 90.0,
            scale: 2.0,
            reflect_x: false,
            reflect_y: false,
        };
        // Local (0, −100) is "up"; +90° turns it to the right on a y-down canvas.
        let q = r.map([0.0, -100.0]);
        assert!(close(q, [210.0, 20.0]), "{q:?}");
        assert!(close(r.inverse(q), [0.0, -100.0]));
        let rr = RotationMap {
            reflect_x: true,
            reflect_y: true,
            ..r
        };
        assert!(close(rr.inverse(rr.map([3.0, 4.0])), [3.0, 4.0]));
    }

    #[test]
    fn inherited_rotation_from_rotation_parent() {
        let parent = DeformerMap::Rotation(RotationMap {
            origin: [0.0, 0.0],
            angle: 30.0,
            scale: 1.0,
            reflect_x: false,
            reflect_y: false,
        });
        assert!((inherited_rotation(&parent, [5.0, 5.0]) - 30.0).abs() < 1e-3);
        let warp = DeformerMap::Warp(grid(1, 1, |u, v| [v * 100.0, -u * 100.0])); // rotated −90°
        assert!((inherited_rotation(&warp, [0.5, 0.5]) + 90.0).abs() < 1e-3);
    }
}
