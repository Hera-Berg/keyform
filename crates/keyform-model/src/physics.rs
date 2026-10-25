//! Physics settings (pendulum rigs). spec: 70-advanced/physics.md §1

use crate::ParamIndex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicsKind {
    /// `SRC_TO_X` / `"X"`
    X,
    /// `SRC_TO_Y` / `"Y"`
    Y,
    /// `SRC_TO_G_ANGLE` / `"Angle"`
    Angle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsInput {
    pub parameter: ParamIndex,
    /// Percent, 0–100.
    pub weight: f32,
    pub kind: PhysicsKind,
    pub reflect: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsOutput {
    pub parameter: ParamIndex,
    /// Particle index, ≥ 1.
    pub vertex_index: usize,
    /// Translation scale (x, y) and angle scale; the one matching `kind` is used.
    pub translation_scale: [f32; 2],
    pub angle_scale: f32,
    pub weight: f32,
    pub kind: PhysicsKind,
    pub reflect: bool,
}

impl PhysicsOutput {
    /// The scale written as physics3.json `Scale` (§4).
    pub fn scale(&self) -> f32 {
        match self.kind {
            PhysicsKind::X => self.translation_scale[0],
            PhysicsKind::Y => self.translation_scale[1],
            PhysicsKind::Angle => self.angle_scale,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsVertex {
    pub position: [f32; 2],
    pub mobility: f32,
    pub delay: f32,
    pub acceleration: f32,
    pub radius: f32,
}

/// (minimum, default, maximum).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Normalization {
    pub minimum: f32,
    pub default: f32,
    pub maximum: f32,
}

impl Default for Normalization {
    fn default() -> Self {
        Self {
            minimum: -10.0,
            default: 0.0,
            maximum: 10.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsSetting {
    pub guid: String,
    pub id: String,
    pub name: String,
    /// Editor preview only; disabled settings are still exported (§4).
    pub enabled: bool,
    pub inputs: Vec<PhysicsInput>,
    pub outputs: Vec<PhysicsOutput>,
    pub vertices: Vec<PhysicsVertex>,
    pub position: Normalization,
    pub angle: Normalization,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsRig {
    /// Editor y-down coordinates; default (0, 1).
    pub gravity: [f32; 2],
    pub wind: [f32; 2],
    /// FPS hint; `None` = unset (not exported).
    pub fps: Option<f32>,
    pub settings: Vec<PhysicsSetting>,
}

impl Default for PhysicsRig {
    fn default() -> Self {
        Self {
            gravity: [0.0, 1.0],
            wind: [0.0, 0.0],
            fps: None,
            settings: Vec::new(),
        }
    }
}
