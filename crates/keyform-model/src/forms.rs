//! Keyform contents per object kind. spec: 20-model/object-model.md §7
//!
//! The same types hold stored keyforms and evaluated forms, so draw order is
//! a float here (stored keyforms hold integers 0…1000).

use crate::{Color, SCREEN_DEFAULT, WHITE};

#[derive(Clone, Debug, PartialEq)]
pub struct PartForm {
    pub draw_order: f32,
    pub opacity: f32,
    pub multiply: Color,
    pub screen: Color,
}

impl Default for PartForm {
    fn default() -> Self {
        Self {
            draw_order: 500.0,
            opacity: 1.0,
            multiply: WHITE,
            screen: SCREEN_DEFAULT,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WarpForm {
    /// (cols+1)·(rows+1) grid points, x fastest, x,y interleaved.
    pub positions: Vec<f32>,
    pub opacity: f32,
    pub multiply: Color,
    pub screen: Color,
}

impl Default for WarpForm {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            opacity: 1.0,
            multiply: WHITE,
            screen: SCREEN_DEFAULT,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RotationForm {
    /// Degrees.
    pub angle: f32,
    /// In the target deformer's local space.
    pub origin: [f32; 2],
    pub scale: f32,
    pub reflect_x: bool,
    pub reflect_y: bool,
    pub opacity: f32,
    pub multiply: Color,
    pub screen: Color,
}

impl Default for RotationForm {
    fn default() -> Self {
        Self {
            angle: 0.0,
            origin: [0.0, 0.0],
            scale: 1.0,
            reflect_x: false,
            reflect_y: false,
            opacity: 1.0,
            multiply: WHITE,
            screen: SCREEN_DEFAULT,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArtMeshForm {
    /// Vertex positions in the target deformer's local space, x,y interleaved.
    pub positions: Vec<f32>,
    pub draw_order: f32,
    pub opacity: f32,
    pub multiply: Color,
    pub screen: Color,
}

impl Default for ArtMeshForm {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            draw_order: 500.0,
            opacity: 1.0,
            multiply: WHITE,
            screen: SCREEN_DEFAULT,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlueForm {
    pub intensity: f32,
}

impl Default for GlueForm {
    fn default() -> Self {
        Self { intensity: 1.0 }
    }
}
