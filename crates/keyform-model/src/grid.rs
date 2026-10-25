//! Keyform grids and blend shapes. spec: 20-model/object-model.md §5–6

use crate::ParamIndex;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExtendedInterpolationKind {
    #[default]
    Linear,
    Ellipse,
    SnsCurve,
}

/// Links an object to one parameter. spec: object-model.md §5
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub parameter: ParamIndex,
    /// Ascending key values.
    pub keys: Vec<f32>,
    /// Stored interpolation type name; only LINEAR is evaluated
    /// (spec: 30-algorithms/keyform-interpolation.md §6).
    pub interpolation: String,
    pub extended: ExtendedInterpolationKind,
    pub insert_point_count: i32,
    pub extended_scale: f32,
}

impl Binding {
    pub fn new(parameter: ParamIndex, keys: Vec<f32>) -> Self {
        Self {
            parameter,
            keys,
            interpolation: "LINEAR".into(),
            extended: ExtendedInterpolationKind::Linear,
            insert_point_count: 1,
            extended_scale: 1.0,
        }
    }
}

/// The cartesian product of an object's bindings with one keyform per cell.
/// Cell index = Σ iⱼ·strideⱼ, first binding fastest.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyformGrid<F> {
    pub bindings: Vec<Binding>,
    /// Exactly [`Self::cell_count`] keyforms, in cell order.
    pub keyforms: Vec<F>,
}

impl<F> KeyformGrid<F> {
    /// A grid without bindings: one unconditional "default keyform".
    pub fn single(form: F) -> Self {
        Self {
            bindings: Vec::new(),
            keyforms: vec![form],
        }
    }

    pub fn cell_count(&self) -> usize {
        self.bindings.iter().map(|b| b.keys.len()).product()
    }

    /// Cell index of the key-index tuple `idx` (one entry per binding).
    pub fn cell_index(&self, idx: &[usize]) -> Option<usize> {
        if idx.len() != self.bindings.len() {
            return None;
        }
        let mut cell = 0;
        let mut stride = 1;
        for (b, &i) in self.bindings.iter().zip(idx) {
            if i >= b.keys.len() {
                return None;
            }
            cell += i * stride;
            stride *= b.keys.len();
        }
        Some(cell)
    }

    pub fn is_consistent(&self) -> bool {
        self.keyforms.len() == self.cell_count()
            && self
                .bindings
                .iter()
                .all(|b| !b.keys.is_empty() && b.keys.windows(2).all(|w| w[0] < w[1]))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlendShape<F> {
    pub parameter: ParamIndex,
    /// `None` = unset: the parameter's max is used (spec: blend-shapes.md §1).
    pub key: Option<f32>,
    pub form: F,
}

/// Scales a blend shape's influence by another parameter's value.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendConstraint {
    pub blend_parameter: ParamIndex,
    pub constraint_parameter: ParamIndex,
    pub value: f32,
    pub weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlendShapes<F> {
    pub shapes: Vec<BlendShape<F>>,
    pub constraints: Vec<BlendConstraint>,
}

impl<F> Default for BlendShapes<F> {
    fn default() -> Self {
        Self {
            shapes: Vec::new(),
            constraints: Vec::new(),
        }
    }
}

impl<F> BlendShapes<F> {
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_addressing_first_binding_fastest() {
        let g = KeyformGrid {
            bindings: vec![
                Binding::new(0, vec![-1.0, 0.0, 1.0]),
                Binding::new(1, vec![0.0, 1.0]),
            ],
            keyforms: (0..6).collect::<Vec<_>>(),
        };
        assert!(g.is_consistent());
        assert_eq!(g.cell_index(&[0, 0]), Some(0));
        assert_eq!(g.cell_index(&[2, 0]), Some(2));
        assert_eq!(g.cell_index(&[1, 1]), Some(4));
        assert_eq!(g.cell_index(&[3, 0]), None);
        assert_eq!(KeyformGrid::single(()).cell_count(), 1);
    }
}
