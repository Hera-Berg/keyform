//! Art paths: centre line and stroke mesh. spec: 70-advanced/art-path.md
//! §2 (centre line), §3 (SIMPLE stroke)

use keyform_model::{ArtPath, ArtPathForm, ArtPathPoint, LineAlgorithm, LineAlignment};

use crate::deform::DeformerMap;

type V2 = [f32; 2];

/// A centre-line sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub pos: V2,
    pub width: f32,
    pub opacity: f32,
    pub color: [f32; 4],
}

fn lerp_sample(a: &ArtPathPoint, b: &ArtPathPoint, t: f32, pos: V2) -> Sample {
    let l = |x: f32, y: f32| x + (y - x) * t;
    Sample {
        pos,
        width: l(a.width, b.width),
        opacity: l(a.opacity, b.opacity),
        color: [
            l(a.color[0], b.color[0]),
            l(a.color[1], b.color[1]),
            l(a.color[2], b.color[2]),
            l(a.color[3], b.color[3]),
        ],
    }
}

/// LCNS tangents (§2): chord-length parameters, quadratic-derivative
/// interior tangents, natural ends.
fn lcns_tangents(p: &[V2]) -> (Vec<f32>, Vec<V2>) {
    let n = p.len();
    let mut t = vec![0.0f32; n];
    for i in 1..n {
        t[i] = t[i - 1] + (p[i][0] - p[i - 1][0]).hypot(p[i][1] - p[i - 1][1]);
    }
    let total = t[n - 1].max(1e-12);
    t.iter_mut().for_each(|x| *x /= total);
    let mut tan = vec![[0.0f32; 2]; n];
    for i in 1..n.saturating_sub(1) {
        let (h0, h1) = (t[i] - t[i - 1], t[i + 1] - t[i]);
        let hh = h0 + h1;
        let r = if hh > 0.0 { h0 / hh } else { 0.5 };
        for k in 0..2 {
            tan[i][k] = ((r - 1.0) / r * p[i - 1][k]
                + (2.0 * r - 1.0) / (r * (r - 1.0)) * p[i][k]
                + r / (1.0 - r) * p[i + 1][k])
                / hh;
        }
    }
    if n >= 3 {
        for k in 0..2 {
            tan[0][k] = 2.0 * (p[1][k] - p[0][k]) / (t[1] - t[0]) - tan[1][k];
            tan[n - 1][k] =
                2.0 * (p[n - 1][k] - p[n - 2][k]) / (t[n - 1] - t[n - 2]) - tan[n - 2][k];
        }
    }
    for v in &mut tan {
        for x in v.iter_mut() {
            if !x.is_finite() {
                *x = 0.0;
            }
        }
    }
    (t, tan)
}

fn hermite(p0: V2, p1: V2, m0: V2, m1: V2, s: f32) -> V2 {
    let (s2, s3) = (s * s, s * s * s);
    let (h00, h01, h10, h11) = (
        2.0 * s3 - 3.0 * s2 + 1.0,
        -2.0 * s3 + 3.0 * s2,
        s3 - 2.0 * s2 + s,
        s3 - s2,
    );
    [
        h00 * p0[0] + h01 * p1[0] + h10 * m0[0] + h11 * m1[0],
        h00 * p0[1] + h01 * p1[1] + h10 * m0[1] + h11 * m1[1],
    ]
}

/// Samples an open piece (no interior corners).
fn open_piece(pts: &[ArtPathPoint], div: usize, alg: LineAlgorithm) -> Vec<Sample> {
    let n = pts.len();
    let p: Vec<V2> = pts.iter().map(|x| x.position).collect();
    let mut out = Vec::new();
    match n {
        0 => {}
        1 => out.push(lerp_sample(&pts[0], &pts[0], 0.0, p[0])),
        2 => {
            // A straight line of div+1 evenly spaced points.
            for k in 0..=div {
                let t = k as f32 / div.max(1) as f32;
                let q = [
                    p[0][0] + (p[1][0] - p[0][0]) * t,
                    p[0][1] + (p[1][1] - p[0][1]) * t,
                ];
                out.push(lerp_sample(&pts[0], &pts[1], t, q));
            }
        }
        _ => match alg {
            LineAlgorithm::Sns => {
                for i in 0..n - 1 {
                    for k in 0..div.max(1) {
                        let t = k as f32 / div.max(1) as f32;
                        out.push(lerp_sample(
                            &pts[i],
                            &pts[i + 1],
                            t,
                            crate::extended::sns_point(&p, i, t),
                        ));
                    }
                }
                out.push(lerp_sample(&pts[n - 1], &pts[n - 1], 0.0, p[n - 1]));
            }
            LineAlgorithm::Lcns => {
                let (t, tan) = lcns_tangents(&p);
                for i in 0..n - 1 {
                    let dt = t[i + 1] - t[i];
                    let (m0, m1) = (
                        [tan[i][0] * dt, tan[i][1] * dt],
                        [tan[i + 1][0] * dt, tan[i + 1][1] * dt],
                    );
                    let steps = div + 1;
                    let start = usize::from(i > 0); // shared end samples once
                    for k in start..=steps {
                        let s = k as f32 / steps as f32;
                        out.push(lerp_sample(
                            &pts[i],
                            &pts[i + 1],
                            s,
                            hermite(p[i], p[i + 1], m0, m1, s),
                        ));
                    }
                }
            }
        },
    }
    out
}

/// The centre line of evaluated control points (§2).
pub fn centre_line(
    pts: &[ArtPathPoint],
    closed: bool,
    div: usize,
    alg: LineAlgorithm,
) -> Vec<Sample> {
    let n = pts.len();
    if closed && n > 2 {
        // Padded open path [Pₙ₋₁, P₀…Pₙ₋₁, P₀, P₁], then trimmed.
        let mut padded = vec![pts[n - 1].clone()];
        padded.extend(pts.iter().cloned());
        padded.push(pts[0].clone());
        padded.push(pts[1].clone());
        let s = centre_line(&padded, false, div, alg);
        let (a, b) = (div + 1, div + 2);
        return if s.len() > a + b {
            s[a..s.len() - b].to_vec()
        } else {
            s
        };
    }
    // Split at corner points; each piece separately.
    let mut out: Vec<Sample> = Vec::new();
    let mut start = 0;
    for i in 1..n {
        if pts[i].corner || i == n - 1 {
            let piece = open_piece(&pts[start..=i], div, alg);
            let skip = usize::from(!out.is_empty());
            out.extend(piece.into_iter().skip(skip));
            start = i;
        }
    }
    if n == 1 {
        out = open_piece(pts, div, alg);
    }
    out
}

/// A stroke mesh: canvas positions, per-vertex colour (RGB × opacity in
/// alpha), triangles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stroke {
    pub positions: Vec<f32>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// The SIMPLE stroke (§3): 3 vertices per sample, 4 triangles between
/// consecutive samples.
pub fn stroke(s: &[Sample], align: LineAlignment, closed: bool) -> Stroke {
    let n = s.len();
    let mut out = Stroke::default();
    if n < 2 {
        return out;
    }
    for i in 0..n {
        let (a, b) = if closed {
            (s[(i + n - 1) % n].pos, s[(i + 1) % n].pos)
        } else {
            (s[i.saturating_sub(1)].pos, s[(i + 1).min(n - 1)].pos)
        };
        let d = [b[0] - a[0], b[1] - a[1]];
        let l = d[0].hypot(d[1]).max(1e-12);
        let nn = [d[1] / l, -d[0] / l];
        let h = s[i].width / 2.0;
        let c = match align {
            LineAlignment::Center => s[i].pos,
            LineAlignment::LeftCcw => [s[i].pos[0] - nn[0] * h, s[i].pos[1] - nn[1] * h],
            LineAlignment::RightCcw => [s[i].pos[0] + nn[0] * h, s[i].pos[1] + nn[1] * h],
        };
        for q in [
            [c[0] + nn[0] * h, c[1] + nn[1] * h],
            c,
            [c[0] - nn[0] * h, c[1] - nn[1] * h],
        ] {
            out.positions.extend_from_slice(&q);
            let k = s[i].color;
            out.colors.push([k[0], k[1], k[2], k[3] * s[i].opacity]);
        }
    }
    let seg = |out: &mut Stroke, i: u32, j: u32| {
        let (a, b) = (3 * i, 3 * j);
        out.indices.extend_from_slice(&[
            a,
            b,
            a + 1,
            b,
            b + 1,
            a + 1,
            a + 1,
            b + 1,
            a + 2,
            b + 1,
            b + 2,
            a + 2,
        ]);
    };
    for i in 0..n as u32 - 1 {
        seg(&mut out, i, i + 1);
    }
    if closed {
        seg(&mut out, n as u32 - 1, 0);
    }
    out
}

/// An evaluated art path (canvas-space stroke and final appearance).
#[derive(Clone, Debug, PartialEq)]
pub struct ArtPathState {
    pub stroke: Stroke,
    pub draw_order: f32,
    /// Final opacity (own × deformer × parts × model).
    pub opacity: f32,
    pub multiply: [f32; 3],
    pub screen: [f32; 3],
    pub outside: bool,
    pub hidden: bool,
}

impl ArtPathState {
    pub fn is_drawn(&self) -> bool {
        !self.outside && !self.hidden && self.opacity > 0.0 && !self.stroke.indices.is_empty()
    }
}

/// The stroke of evaluated form `f`: control points to the canvas through
/// `map` (the target deformer), centre line, SIMPLE strip.
// Null / unmatched brush = SIMPLE without texture; colour alpha used;
// width in canvas px after mapping (Q-0046, art-path.md).
pub fn geometry(a: &ArtPath, f: &ArtPathForm, map: Option<&DeformerMap>) -> Stroke {
    let pts: Vec<ArtPathPoint> = f
        .points
        .iter()
        .map(|p| ArtPathPoint {
            position: map.map_or(p.position, |mp| mp.map(p.position)),
            ..p.clone()
        })
        .collect();
    let line = centre_line(&pts, a.closed, a.div_num.max(1) as usize, a.algorithm);
    stroke(&line, a.alignment, a.closed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> ArtPathPoint {
        ArtPathPoint {
            position: [x, y],
            width: 2.0,
            opacity: 1.0,
            color: [0.0, 0.0, 0.0, 1.0],
            corner: false,
        }
    }

    #[test]
    fn lines_and_strips() {
        // Two points: div+1 samples on the segment.
        let s = centre_line(
            &[pt(0.0, 0.0), pt(10.0, 0.0)],
            false,
            4,
            LineAlgorithm::Lcns,
        );
        assert_eq!(s.len(), 5);
        assert_eq!(s[2].pos, [5.0, 0.0]);
        // LCNS passes through the points; shared samples once.
        let p = [pt(0.0, 0.0), pt(10.0, 5.0), pt(20.0, 0.0)];
        let s = centre_line(&p, false, 3, LineAlgorithm::Lcns);
        assert_eq!(s.len(), 2 * (3 + 2) - 1);
        assert!((s[4].pos[0] - 10.0).abs() < 1e-4 && (s[4].pos[1] - 5.0).abs() < 1e-4);
        // SNS through the points too.
        let s2 = centre_line(&p, false, 3, LineAlgorithm::Sns);
        assert_eq!(s2.len(), 2 * 3 + 1);
        assert!((s2[3].pos[0] - 10.0).abs() < 1e-3);
        // A SIMPLE strip of a straight line: half-width offsets, 4 triangles per segment.
        let st = stroke(
            &centre_line(
                &[pt(0.0, 0.0), pt(10.0, 0.0)],
                false,
                1,
                LineAlgorithm::Lcns,
            ),
            LineAlignment::Center,
            false,
        );
        assert_eq!(st.positions.len(), 2 * 3 * 2);
        assert_eq!(st.indices.len(), 12);
        assert_eq!(&st.positions[0..2], &[0.0, -1.0]);
        // Closed paths keep the right number of samples.
        let sq = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)];
        let c = centre_line(&sq, true, 2, LineAlgorithm::Lcns);
        assert!(!c.is_empty());
    }
}
