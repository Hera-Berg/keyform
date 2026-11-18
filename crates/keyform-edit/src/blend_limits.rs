//! Limit Settings for Blend Shape Weights: per object, per blend-shape
//! parameter, a piecewise-linear weight limit over another parameter.
//! Layout and rules observed in the reference (Q-0037); stored as
//! `BlendConstraint` points. spec: 30-algorithms/blend-shapes.md §1–2

use std::sync::Arc;

use keyform_model::{BlendConstraint, DeformerKind, Model, ParamIndex};

use crate::keys::ObjectRef;

/// Built-in graph shapes (presets of the Weight Limit Graph).
pub const PRESETS: [&str; 10] = [
    "Line1",
    "Line2",
    "Line3",
    "Fold line1",
    "Fold line2",
    "Fold line3",
    "Fold line4",
    "Fold line5",
    "Step1",
    "Step2",
];

/// Points (parameter value, weight 0–1) of preset `i` over [min, max].
/// Line1, Fold line1 and Step1 were observed (the step sits 1 % of the
/// range before the middle); the others follow their pictured shapes.
pub fn preset_points(i: usize, min: f32, max: f32) -> Vec<(f32, f32)> {
    let mid = (min + max) / 2.0;
    let eps = (max - min) * 0.01;
    match i {
        0 => vec![(min, 1.0), (max, 1.0)],
        1 => vec![(min, 0.0), (max, 1.0)],
        2 => vec![(min, 1.0), (max, 0.0)],
        3 => vec![(min, 0.0), (mid, 1.0), (max, 0.0)],
        4 => vec![(min, 0.0), (mid, 0.0), (max, 1.0)],
        5 => vec![(min, 1.0), (mid, 0.0), (max, 0.0)],
        6 => vec![(min, 0.0), (mid, 1.0), (max, 1.0)],
        7 => vec![(min, 1.0), (mid, 1.0), (max, 0.0)],
        8 => vec![(min, 0.0), (mid - eps, 0.0), (mid, 1.0), (max, 1.0)],
        _ => vec![(min, 1.0), (mid - eps, 1.0), (mid, 0.0), (max, 0.0)],
    }
}

/// The Edit Graph rules: at most 5 rows, no duplicate parameter values,
/// values within [min, max], weights within 0–1. Returns the first broken
/// rule's text.
pub fn validate(points: &[(f32, f32)], min: f32, max: f32) -> Result<(), &'static str> {
    if points.len() > 5 {
        return Err("5 rows or fewer");
    }
    for (i, p) in points.iter().enumerate() {
        if points[..i].iter().any(|q| q.0 == p.0) {
            return Err("No duplicate parameters");
        }
        if p.0 < min || p.0 > max {
            return Err("Parameter values are above minimum and below maximum values");
        }
        if !(0.0..=1.0).contains(&p.1) {
            return Err("Weight limit is above 0% and below 100%");
        }
    }
    Ok(())
}

fn constraints_mut(m: &mut Model, o: ObjectRef) -> Option<&mut Vec<BlendConstraint>> {
    Some(match o {
        ObjectRef::Part(i) => &mut Arc::make_mut(m.parts.get_mut(i)?).blend_shapes.constraints,
        ObjectRef::ArtMesh(i) => {
            &mut Arc::make_mut(m.art_meshes.get_mut(i)?)
                .blend_shapes
                .constraints
        }
        ObjectRef::Glue(i) => &mut Arc::make_mut(m.glues.get_mut(i)?).blend_shapes.constraints,
        ObjectRef::Deformer(i) => match &mut Arc::make_mut(m.deformers.get_mut(i)?).kind {
            DeformerKind::Warp(w) => &mut w.blend_shapes.constraints,
            DeformerKind::Rotation(r) => &mut r.blend_shapes.constraints,
        },
    })
}

fn constraints(m: &Model, o: ObjectRef) -> Vec<BlendConstraint> {
    match o {
        ObjectRef::Part(i) => m.parts.get(i).map(|x| x.blend_shapes.constraints.clone()),
        ObjectRef::ArtMesh(i) => m
            .art_meshes
            .get(i)
            .map(|x| x.blend_shapes.constraints.clone()),
        ObjectRef::Glue(i) => m.glues.get(i).map(|x| x.blend_shapes.constraints.clone()),
        ObjectRef::Deformer(i) => m.deformers.get(i).map(|d| match &d.kind {
            DeformerKind::Warp(w) => w.blend_shapes.constraints.clone(),
            DeformerKind::Rotation(r) => r.blend_shapes.constraints.clone(),
        }),
    }
    .unwrap_or_default()
}

/// Constraint parameters of `o` for blend-shape parameter `b`, in order.
pub fn constraint_params(m: &Model, o: ObjectRef, b: ParamIndex) -> Vec<ParamIndex> {
    let mut v = Vec::new();
    for c in constraints(m, o).iter().filter(|c| c.blend_parameter == b) {
        if !v.contains(&c.constraint_parameter) {
            v.push(c.constraint_parameter);
        }
    }
    v
}

/// The graph points of (`b`, `q`) on `o`, sorted by value.
pub fn points(m: &Model, o: ObjectRef, b: ParamIndex, q: ParamIndex) -> Vec<(f32, f32)> {
    let mut v: Vec<(f32, f32)> = constraints(m, o)
        .iter()
        .filter(|c| c.blend_parameter == b && c.constraint_parameter == q)
        .map(|c| (c.value, c.weight))
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// Replaces the graph of (`b`, `q`) on `o` (`None` deletes it). Returns
/// whether anything changed.
pub fn set_points(
    m: &mut Model,
    o: ObjectRef,
    b: ParamIndex,
    q: ParamIndex,
    pts: Option<&[(f32, f32)]>,
) -> bool {
    let before = points(m, o, b, q);
    let Some(list) = constraints_mut(m, o) else {
        return false;
    };
    let at = list
        .iter()
        .position(|c| c.blend_parameter == b && c.constraint_parameter == q);
    list.retain(|c| !(c.blend_parameter == b && c.constraint_parameter == q));
    if let Some(pts) = pts {
        let mut sorted = pts.to_vec();
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
        let new: Vec<BlendConstraint> = sorted
            .iter()
            .map(|&(value, weight)| BlendConstraint {
                blend_parameter: b,
                constraint_parameter: q,
                value,
                weight,
            })
            .collect();
        let at = at.unwrap_or(list.len()).min(list.len());
        list.splice(at..at, new);
    }
    let after = points(m, o, b, q);
    before != after || (pts.is_none() && !before.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_rules() {
        assert_eq!(
            preset_points(0, -30.0, 30.0),
            vec![(-30.0, 1.0), (30.0, 1.0)]
        );
        assert_eq!(
            preset_points(3, -30.0, 30.0),
            vec![(-30.0, 0.0), (0.0, 1.0), (30.0, 0.0)]
        );
        // Observed: Step1 over Angle X = (−30, 0), (−0.6, 0), (0, 100 %), (30, 100 %).
        let s = preset_points(8, -30.0, 30.0);
        assert!((s[1].0 + 0.6).abs() < 1e-5 && s[2] == (0.0, 1.0));
        assert!(validate(&s, -30.0, 30.0).is_ok());
        assert!(validate(&[(0.0, 1.0), (0.0, 0.5)], -1.0, 1.0).is_err());
        assert!(validate(&[(2.0, 1.0)], -1.0, 1.0).is_err());
        assert!(validate(&[(0.0, 1.5)], -1.0, 1.0).is_err());
        assert!(validate(&[(0.0, 0.0); 6], -1.0, 1.0).is_err());
    }
}
