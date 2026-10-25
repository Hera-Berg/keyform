//! The Bezier control net of a warp deformer (edit levels 2 and 3): built
//! from the fine grid, edited, and used to regenerate the grid.
//! spec: 35-editing/warp-deformer-editing.md §1–3

type V2 = [f32; 2];

fn add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: V2, k: f32) -> V2 {
    [a[0] * k, a[1] * k]
}

/// Cubic Bézier B(P0, P1, P2, P3; u).
fn bez(p0: V2, p1: V2, p2: V2, p3: V2, u: f32) -> V2 {
    let v = 1.0 - u;
    add(
        add(mul(p0, v * v * v), mul(p1, 3.0 * v * v * u)),
        add(mul(p2, 3.0 * v * u * u), mul(p3, u * u * u)),
    )
}

/// First control point of the cubic through P0…P3 at 0, ⅓, ⅔, 1.
fn cubic_handle(p0: V2, p1: V2, p2: V2, p3: V2) -> V2 {
    add(
        add(mul(p0, -5.0 / 6.0), mul(p1, 3.0)),
        add(mul(p2, -1.5), mul(p3, 1.0 / 3.0)),
    )
}

/// An anchor and its handles (absolute positions; a handle equal to the
/// anchor has no effect).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Anchor {
    pub p: V2,
    pub n: V2,
    pub s: V2,
    pub w: V2,
    pub e: V2,
}

/// Handle directions, in point-index order after the anchors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    N,
    S,
    W,
    E,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    pub cols: usize,
    pub rows: usize,
    /// (cols+1)·(rows+1) anchors, row-major (x fastest).
    pub anchors: Vec<Anchor>,
}

/// A fine grid: (cols+1)·(rows+1) points, x fastest, x,y interleaved.
pub struct Grid<'a> {
    pub cols: usize,
    pub rows: usize,
    pub points: &'a [f32],
}

impl Grid<'_> {
    fn p(&self, i: usize, j: usize) -> V2 {
        let k = 2 * (j * (self.cols + 1) + i);
        [self.points[k], self.points[k + 1]]
    }

    /// Interpolates up to four consecutive samples at local position `x`
    /// (in sample steps from the first one): linear, quadratic-as-cubic or
    /// cubic through equally spaced points. spec: §2 step 1
    fn interp(pts: &[V2], x: f32) -> V2 {
        match pts.len() {
            0 => [0.0, 0.0],
            1 => pts[0],
            2 => add(pts[0], mul(sub(pts[1], pts[0]), x)),
            3 => {
                let c1 = mul(sub(mul(pts[1], 4.0), pts[2]), 1.0 / 3.0);
                let c2 = mul(sub(mul(pts[1], 4.0), pts[0]), 1.0 / 3.0);
                bez(pts[0], c1, c2, pts[2], x / 2.0)
            }
            _ => {
                let c1 = cubic_handle(pts[0], pts[1], pts[2], pts[3]);
                let c2 = cubic_handle(pts[3], pts[2], pts[1], pts[0]);
                bez(pts[0], c1, c2, pts[3], x / 3.0)
            }
        }
    }

    /// The neighbourhood start and size along an axis of `n` cells.
    fn window(n: usize, x: f32) -> (usize, usize) {
        let size = (n + 1).min(4);
        let k = (x.floor().max(0.0) as usize).min(n.saturating_sub(1));
        let start = k.saturating_sub(1).min(n + 1 - size);
        (start, size)
    }

    /// The smooth surface G(u, v) through the grid (§2 step 1).
    pub fn eval(&self, u: f32, v: f32) -> V2 {
        let (x, y) = (u * self.cols as f32, v * self.rows as f32);
        let (i0, ni) = Self::window(self.cols, x);
        let (j0, nj) = Self::window(self.rows, y);
        let row: Vec<V2> = (i0..i0 + ni)
            .map(|i| {
                let col: Vec<V2> = (j0..j0 + nj).map(|j| self.p(i, j)).collect();
                Self::interp(&col, y - j0 as f32)
            })
            .collect();
        Self::interp(&row, x - i0 as f32)
    }
}

impl Net {
    fn idx(&self, i: usize, j: usize) -> usize {
        j * (self.cols + 1) + i
    }

    pub fn anchor(&self, i: usize, j: usize) -> &Anchor {
        &self.anchors[self.idx(i, j)]
    }

    /// Builds the net of `cols`×`rows` cells from the fine grid (§2).
    pub fn from_grid(g: &Grid, cols: usize, rows: usize) -> Self {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let (du, dv) = (1.0 / cols as f32, 1.0 / rows as f32);
        // Two fine cells per net cell in a direction → quadratic handles.
        let quad_u = g.cols == 2 * cols;
        let quad_v = g.rows == 2 * rows;
        let mut anchors = Vec::with_capacity((cols + 1) * (rows + 1));
        for j in 0..=rows {
            for i in 0..=cols {
                let (u, v) = (i as f32 * du, j as f32 * dv);
                let p0 = g.eval(u, v);
                let handle = |su: f32, sv: f32, quad: bool| -> V2 {
                    if quad {
                        let p1 = g.eval(u + su / 2.0, v + sv / 2.0);
                        let p2 = g.eval(u + su, v + sv);
                        mul(sub(mul(p1, 4.0), p2), 1.0 / 3.0)
                    } else {
                        let p1 = g.eval(u + su / 3.0, v + sv / 3.0);
                        let p2 = g.eval(u + 2.0 * su / 3.0, v + 2.0 * sv / 3.0);
                        let p3 = g.eval(u + su, v + sv);
                        cubic_handle(p0, p1, p2, p3)
                    }
                };
                anchors.push(Anchor {
                    p: p0,
                    n: if j > 0 { handle(0.0, -dv, quad_v) } else { p0 },
                    s: if j < rows {
                        handle(0.0, dv, quad_v)
                    } else {
                        p0
                    },
                    w: if i > 0 { handle(-du, 0.0, quad_u) } else { p0 },
                    e: if i < cols {
                        handle(du, 0.0, quad_u)
                    } else {
                        p0
                    },
                });
            }
        }
        Self {
            cols,
            rows,
            anchors,
        }
    }

    /// Patch point X(s, t) of cell (ci, cj) (§1).
    pub fn eval(&self, ci: usize, cj: usize, s: f32, t: f32) -> V2 {
        let nw = *self.anchor(ci, cj);
        let ne = *self.anchor(ci + 1, cj);
        let sw = *self.anchor(ci, cj + 1);
        let se = *self.anchor(ci + 1, cj + 1);
        let h = || {
            let w = bez(nw.p, nw.s, sw.n, sw.p, t);
            let e = bez(ne.p, ne.s, se.n, se.p, t);
            let hw = add(
                w,
                add(mul(sub(nw.e, nw.p), 1.0 - t), mul(sub(sw.e, sw.p), t)),
            );
            let he = add(
                e,
                add(mul(sub(ne.w, ne.p), 1.0 - t), mul(sub(se.w, se.p), t)),
            );
            bez(w, hw, he, e, s)
        };
        let v = || {
            let n = bez(nw.p, nw.e, ne.w, ne.p, s);
            let so = bez(sw.p, sw.e, se.w, se.p, s);
            let hn = add(
                n,
                add(mul(sub(nw.s, nw.p), 1.0 - s), mul(sub(ne.s, ne.p), s)),
            );
            let hs = add(
                so,
                add(mul(sub(sw.n, sw.p), 1.0 - s), mul(sub(se.n, se.p), s)),
            );
            bez(n, hn, hs, so, t)
        };
        if t == 0.0 || t == 1.0 {
            v()
        } else if s == 0.0 || s == 1.0 {
            h()
        } else {
            let wh = (t * (1.0 - t)).powi(2);
            let wv = (s * (1.0 - s)).powi(2);
            mul(add(mul(h(), wh), mul(v(), wv)), 1.0 / (wh + wv))
        }
    }

    /// Inverse within cell (ci, cj) from a starting guess: Gauss–Newton on
    /// |X(s, t) − p|², tolerance 0.001 on the squared residual. Returns
    /// (s, t) and the residual offset p − X(s, t). spec: §1 "Inverse"
    pub fn locate(&self, ci: usize, cj: usize, p: V2, guess: (f32, f32)) -> ((f32, f32), V2) {
        let (mut s, mut t) = guess;
        let h = 1e-3f32;
        for _ in 0..20 {
            let x = self.eval(ci, cj, s, t);
            let r = sub(p, x);
            if r[0] * r[0] + r[1] * r[1] <= 0.001 {
                break;
            }
            let ds = mul(
                sub(self.eval(ci, cj, s + h, t), self.eval(ci, cj, s - h, t)),
                0.5 / h,
            );
            let dt = mul(
                sub(self.eval(ci, cj, s, t + h), self.eval(ci, cj, s, t - h)),
                0.5 / h,
            );
            let det = ds[0] * dt[1] - ds[1] * dt[0];
            if det.abs() < 1e-12 {
                break;
            }
            let a = (r[0] * dt[1] - r[1] * dt[0]) / det;
            let b = (ds[0] * r[1] - ds[1] * r[0]) / det;
            s = (s + a).clamp(-0.5, 1.5);
            t = (t + b).clamp(-0.5, 1.5);
        }
        let x = self.eval(ci, cj, s, t);
        ((s, t), sub(p, x))
    }

    /// Net points as a flat list: anchors, then for each anchor its N, S,
    /// W, E handles where they point into the net.
    pub fn points(&self) -> Vec<(usize, Option<Dir>)> {
        let mut out: Vec<(usize, Option<Dir>)> =
            (0..self.anchors.len()).map(|k| (k, None)).collect();
        for j in 0..=self.rows {
            for i in 0..=self.cols {
                let k = self.idx(i, j);
                for (d, ok) in [
                    (Dir::N, j > 0),
                    (Dir::S, j < self.rows),
                    (Dir::W, i > 0),
                    (Dir::E, i < self.cols),
                ] {
                    if ok {
                        out.push((k, Some(d)));
                    }
                }
            }
        }
        out
    }

    pub fn point(&self, k: usize, d: Option<Dir>) -> V2 {
        let a = &self.anchors[k];
        match d {
            None => a.p,
            Some(Dir::N) => a.n,
            Some(Dir::S) => a.s,
            Some(Dir::W) => a.w,
            Some(Dir::E) => a.e,
        }
    }

    /// Moves a net point to `to`; moving an anchor carries its handles.
    pub fn set_point(&mut self, k: usize, d: Option<Dir>, to: V2) {
        let a = &mut self.anchors[k];
        match d {
            None => {
                let delta = sub(to, a.p);
                for q in [&mut a.p, &mut a.n, &mut a.s, &mut a.w, &mut a.e] {
                    *q = add(*q, delta);
                }
            }
            Some(Dir::N) => a.n = to,
            Some(Dir::S) => a.s = to,
            Some(Dir::W) => a.w = to,
            Some(Dir::E) => a.e = to,
        }
    }
}

/// Where each fine point sits in a net: its cell, (s, t) and residual.
pub struct Located {
    cells: Vec<(usize, usize, f32, f32, V2)>,
}

impl Located {
    /// Locates every point of the fine grid in `net` (§3), starting from
    /// the uniform cell coordinates.
    pub fn new(net: &Net, g: &Grid) -> Self {
        let mut cells = Vec::with_capacity((g.cols + 1) * (g.rows + 1));
        for j in 0..=g.rows {
            for i in 0..=g.cols {
                let (x, y) = (
                    i as f32 / g.cols as f32 * net.cols as f32,
                    j as f32 / g.rows as f32 * net.rows as f32,
                );
                let ci = (x.floor() as usize).min(net.cols - 1);
                let cj = (y.floor() as usize).min(net.rows - 1);
                let guess = (x - ci as f32, y - cj as f32);
                let ((s, t), r) = net.locate(ci, cj, g.p(i, j), guess);
                cells.push((ci, cj, s, t, r));
            }
        }
        Self { cells }
    }

    /// Fine points regenerated from an edited net: X_new(s, t) + r.
    pub fn regenerate(&self, net: &Net) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.cells.len() * 2);
        for &(ci, cj, s, t, r) in &self.cells {
            let x = add(net.eval(ci, cj, s, t), r);
            out.extend_from_slice(&x);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(cols: usize, rows: usize, f: impl Fn(f32, f32) -> V2) -> Vec<f32> {
        let mut v = Vec::new();
        for j in 0..=rows {
            for i in 0..=cols {
                v.extend_from_slice(&f(i as f32 / cols as f32, j as f32 / rows as f32));
            }
        }
        v
    }

    #[test]
    fn surface_interpolates_the_grid_and_affine_grids_are_exact() {
        let pts = grid(5, 5, |u, v| [100.0 * u + 20.0 * v, 80.0 * v - 10.0 * u]);
        let g = Grid {
            cols: 5,
            rows: 5,
            points: &pts,
        };
        for (u, v) in [(0.0, 0.0), (0.4, 0.6), (1.0, 1.0), (0.33, 0.9)] {
            let p = g.eval(u, v);
            let want = [100.0 * u + 20.0 * v, 80.0 * v - 10.0 * u];
            assert!(
                (p[0] - want[0]).abs() < 1e-3 && (p[1] - want[1]).abs() < 1e-3,
                "{p:?}"
            );
        }
        // The net of an affine grid reproduces it everywhere.
        let net = Net::from_grid(&g, 2, 2);
        let x = net.eval(1, 0, 0.5, 0.25);
        let want = g.eval(0.75, 0.125);
        assert!((x[0] - want[0]).abs() < 1e-3 && (x[1] - want[1]).abs() < 1e-3);
    }

    #[test]
    fn unedited_net_keeps_the_grid_and_anchor_moves_are_local() {
        // A bent 6×4 grid.
        let pts = grid(6, 4, |u, v| {
            [100.0 * u + 5.0 * (3.0 * v).sin(), 60.0 * v + 8.0 * u * u]
        });
        let g = Grid {
            cols: 6,
            rows: 4,
            points: &pts,
        };
        let net = Net::from_grid(&g, 2, 2);
        let loc = Located::new(&net, &g);
        let same = loc.regenerate(&net);
        for (a, b) in same.iter().zip(&pts) {
            assert!((a - b).abs() < 1e-3, "{a} vs {b}");
        }
        // Move the centre anchor right by 10: the centre fine point follows
        // fully, the corners stay.
        let mut edited = net.clone();
        let c = edited.idx(1, 1);
        let to = add(edited.anchors[c].p, [10.0, 0.0]);
        edited.set_point(c, None, to);
        let out = loc.regenerate(&edited);
        let fine = |i: usize, j: usize| {
            let k = 2 * (j * 7 + i);
            [out[k] - pts[k], out[k + 1] - pts[k + 1]]
        };
        assert!((fine(3, 2)[0] - 10.0).abs() < 1e-2, "{:?}", fine(3, 2));
        assert!(fine(0, 0)[0].abs() < 1e-3 && fine(6, 4)[0].abs() < 1e-3);
        assert_eq!(net.points().len(), 9 + 12 * 2);
    }
}
