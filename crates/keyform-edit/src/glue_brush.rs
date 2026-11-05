//! Glue weight brushes: paint the per-pair glue weights (wA, wB).
//! spec: 30-algorithms/glue.md "Weight brushes (Glue tool group)"

use std::sync::Arc;

use keyform_model::Model;

/// The tools of the Glue group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GlueBrush {
    /// Weight brush A.
    A,
    /// Weight brush B.
    B,
    /// Weight brush A:B.
    AB,
    /// Brush weight B:A.
    BA,
    /// Re-merge Glue Vertices Tool.
    Remerge,
}

impl GlueBrush {
    pub const ALL: [GlueBrush; 5] = [Self::A, Self::B, Self::AB, Self::BA, Self::Remerge];

    pub fn label(self) -> &'static str {
        match self {
            Self::A => "Weight brush A",
            Self::B => "Weight brush B",
            Self::AB => "Weight brush A:B",
            Self::BA => "Brush weight B:A",
            Self::Remerge => "Re-merge Glue Vertices Tool",
        }
    }

    /// Status text of a Shift + drag.
    pub fn shift_status(self) -> &'static str {
        match self {
            Self::A | Self::B => "Decrease weight",
            Self::AB => "Increase weight B",
            Self::BA => "Increase weight A",
            Self::Remerge => "Weaken adhesion",
        }
    }
}

/// Amount of one drag event for a vertex at brush falloff `f` (0…1):
/// 0.2 × pressure (1 for a mouse) × level × f.
pub fn amount(f: f32, level: f32, pressure: f32) -> f32 {
    0.2 * pressure * level * f
}

/// Brush falloff at distance `d` for radius `r` and `hardness` (0…1):
/// 1 at full hardness, else clamp((1 − d/r) / (1 − hardness), 0, 1).
/// `None` outside the radius.
pub fn falloff(d: f32, r: f32, hardness: f32) -> Option<f32> {
    if r <= 0.0 || d > r {
        return None;
    }
    let h = hardness.clamp(0.0, 1.0);
    Some(if h >= 1.0 {
        1.0
    } else {
        ((1.0 - d / r) / (1.0 - h)).clamp(0.0, 1.0)
    })
}

/// New stored (wA, wB) of one pair. `a` and `b` are the amounts of A's
/// and B's vertex, `None` when that vertex is outside the brush.
pub fn brush_pair(
    tool: GlueBrush,
    shift: bool,
    w: [f32; 2],
    a: Option<f32>,
    b: Option<f32>,
) -> [f32; 2] {
    let c = |v: f32| v.clamp(0.0, 1.0);
    if tool == GlueBrush::Remerge {
        // Both vertices must be under the brush; the larger amount.
        let (Some(a), Some(b)) = (a, b) else { return w };
        let m = a.max(b);
        if shift {
            return [c(w[0] - m), c(w[1] - m)];
        }
        let sum = w[0] + w[1];
        let target = if sum.abs() < 1e-6 {
            [0.5, 0.5]
        } else {
            [w[0] / sum, w[1] / sum]
        };
        let toward = |v: f32, t: f32| v + (t - v).clamp(-m, m);
        return [c(toward(w[0], target[0])), c(toward(w[1], target[1]))];
    }
    // wA changes by A's vertex amount and wB by B's, each only when that
    // vertex is under the brush. UI "weight A" is A's holding strength:
    // raising it lowers wA.
    let s = if shift { -1.0 } else { 1.0 };
    let (da, db) = (s * a.unwrap_or(0.0), s * b.unwrap_or(0.0));
    match tool {
        GlueBrush::A => [c(w[0] - da), w[1]],
        GlueBrush::B => [w[0], c(w[1] - db)],
        GlueBrush::AB => [c(w[0] - da), c(w[1] + db)],
        GlueBrush::BA => [c(w[0] + da), c(w[1] - db)],
        GlueBrush::Remerge => unreachable!(),
    }
}

/// One drag event of `tool` on glue objects `glues`. `amount_at(mesh,
/// vertex)` is the brush amount of a vertex (`None` outside the brush).
/// Returns whether any weight changed.
pub fn apply(
    m: &mut Model,
    glues: &[usize],
    tool: GlueBrush,
    shift: bool,
    amount_at: impl Fn(usize, usize) -> Option<f32>,
) -> bool {
    let mut changed = false;
    for &g in glues {
        let Some(glue) = m.glues.get(g) else { continue };
        let (Some(ma), Some(mb)) = (glue.mesh_a, glue.mesh_b) else {
            continue;
        };
        let mut weights = glue.weights.clone();
        let mut any = false;
        for (k, &(ia, ib)) in glue.pairs.iter().enumerate() {
            let (Some(ia), Some(ib), Some(w)) = (ia, ib, weights.get_mut(k)) else {
                continue;
            };
            let n = brush_pair(tool, shift, *w, amount_at(ma, ia), amount_at(mb, ib));
            if n != *w {
                *w = n;
                any = true;
            }
        }
        if any {
            Arc::make_mut(&mut m.glues[g]).weights = weights;
            changed = true;
        }
    }
    changed
}

/// Message when no glue is selected for the weight brushes.
pub const NO_SELECTED_GLUE: &str = "No Selected Glue";

/// The glue objects a brush paints: the selected glue only. `None` (the
/// brush does nothing) when one of them is locked.
pub fn target_glues(m: &Model, selected_glue: &[usize]) -> Option<Vec<usize>> {
    if selected_glue
        .iter()
        .any(|&g| m.glues.get(g).is_some_and(|x| x.info.locked))
    {
        return None;
    }
    Some(selected_glue.to_vec())
}

/// Mean (UI weight A, UI weight B) of the pairs of `glues`, for the
/// "Glue Weight A : B" panel; UI weight = 1 − stored weight.
pub fn ui_weights(m: &Model, glues: &[usize]) -> Option<[f32; 2]> {
    let mut sum = [0.0f32; 2];
    let mut n = 0;
    for &g in glues {
        for w in m.glues.get(g).map(|x| &x.weights[..]).unwrap_or(&[]) {
            sum[0] += 1.0 - w[0];
            sum[1] += 1.0 - w[1];
            n += 1;
        }
    }
    (n > 0).then(|| [sum[0] / n as f32, sum[1] / n as f32])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_rules() {
        let w = [0.5, 0.5];
        let a = Some(0.1);
        assert_eq!(brush_pair(GlueBrush::A, false, w, a, None), [0.4, 0.5]);
        assert_eq!(brush_pair(GlueBrush::A, true, w, a, None), [0.6, 0.5]);
        // A's brush ignores B's vertex.
        assert_eq!(brush_pair(GlueBrush::A, false, w, None, a), w);
        assert_eq!(brush_pair(GlueBrush::B, false, w, None, a), [0.5, 0.4]);
        // A:B and B:A: each weight by its own vertex's amount.
        assert_eq!(brush_pair(GlueBrush::AB, false, w, a, None), [0.4, 0.5]);
        assert_eq!(brush_pair(GlueBrush::AB, false, w, None, a), [0.5, 0.6]);
        let r = brush_pair(GlueBrush::AB, false, w, Some(0.1), Some(0.2));
        assert!((r[0] - 0.4).abs() < 1e-6 && (r[1] - 0.7).abs() < 1e-6);
        assert_eq!(brush_pair(GlueBrush::BA, false, w, a, a), [0.6, 0.4]);
        assert_eq!(brush_pair(GlueBrush::BA, true, w, a, a), [0.4, 0.6]);
        assert_eq!(brush_pair(GlueBrush::A, false, w, None, None), w);
        // Clamped to [0, 1].
        assert_eq!(
            brush_pair(GlueBrush::A, false, [0.05, 0.5], a, None),
            [0.0, 0.5]
        );
        // Re-merge needs both vertices; moves towards w / (wA + wB) by the
        // larger amount.
        assert_eq!(
            brush_pair(GlueBrush::Remerge, false, [0.2, 0.2], a, None),
            [0.2, 0.2]
        );
        let r = brush_pair(GlueBrush::Remerge, false, [0.2, 0.2], a, Some(0.05));
        assert!((r[0] - 0.3).abs() < 1e-6 && (r[1] - 0.3).abs() < 1e-6);
        let r = brush_pair(GlueBrush::Remerge, false, [0.0, 0.0], Some(1.0), Some(1.0));
        assert_eq!(r, [0.5, 0.5]);
        let r = brush_pair(GlueBrush::Remerge, true, [0.2, 0.5], a, a);
        assert!((r[0] - 0.1).abs() < 1e-6 && (r[1] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn falloff_and_amount() {
        assert_eq!(falloff(0.0, 10.0, 0.3), Some(1.0));
        assert_eq!(falloff(11.0, 10.0, 0.3), None);
        assert!((falloff(8.5, 10.0, 0.5).unwrap() - 0.3).abs() < 1e-6);
        assert_eq!(falloff(9.9, 10.0, 1.0), Some(1.0));
        assert!((amount(0.5, 1.0, 1.0) - 0.1).abs() < 1e-6);
    }
}
