//! Deform paths: control points and curves attached to an art mesh that
//! drag its vertices (and those of sub meshes) in the current keyform. An
//! editing helper only: never exported, never evaluated.
//! spec: 35-editing/deform-paths.md §1

use crate::Guid;

/// How effects are computed (`CControllerExtension$BindMethod`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BindMethod {
    DistanceSimple,
    DistanceAndDirection,
    LineSimple,
    #[default]
    LineAndDirection,
}

impl BindMethod {
    pub const NAMES: [(&'static str, BindMethod); 4] = [
        ("DISTANCE_SIMPLE", Self::DistanceSimple),
        ("DISTANCE_AND_DIRECTION", Self::DistanceAndDirection),
        ("LINE_SIMPLE", Self::LineSimple),
        ("LINE_AND_DIRECTION", Self::LineAndDirection),
    ];

    pub fn from_name(s: &str) -> Option<Self> {
        Self::NAMES.iter().find(|(n, _)| *n == s).map(|(_, b)| *b)
    }

    pub fn name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(_, b)| *b == self)
            .map_or("LINE_AND_DIRECTION", |(n, _)| n)
    }

    pub fn is_line(self) -> bool {
        matches!(self, Self::LineSimple | Self::LineAndDirection)
    }

    pub fn with_direction(self) -> bool {
        matches!(self, Self::DistanceAndDirection | Self::LineAndDirection)
    }
}

/// A position as barycentric weights over a triangle of the main mesh.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointInTriangle {
    pub index: [i32; 3],
    pub weight: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControlPoint {
    /// `ctrlPtId`.
    pub guid: Guid,
    /// Index into [`DeformPath::curves`].
    pub curve: Option<usize>,
    /// Fold (corner) point.
    pub corner: bool,
    pub anchor: PointInTriangle,
    /// Canvas position at the last bind (fallback).
    pub pos_default: [f32; 2],
    /// Canvas position when the anchor was set (fallback).
    pub recovery: [f32; 2],
    /// Runtime value, stored (§4).
    pub total_effect: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControlCurve {
    /// `curveId`.
    pub guid: Guid,
    /// Ordered control points (indices into [`DeformPath::points`]).
    pub points: Vec<usize>,
    pub open: bool,
    /// Canvas px (default 200).
    pub width: f32,
    /// Percent (default 50).
    pub hardness: f32,
}

impl ControlCurve {
    pub fn new(guid: Guid, points: Vec<usize>) -> Self {
        Self {
            guid,
            points,
            open: true,
            width: 200.0,
            hardness: 50.0,
        }
    }
}

/// What an effect follows.
#[derive(Clone, Debug, PartialEq)]
pub enum Effector {
    /// A control point (index into [`DeformPath::points`]).
    Point(usize),
    /// A point on a curve (index into [`DeformPath::curves`]) at curve
    /// parameter `t`, `distance` from the vertex, at canvas `pos`.
    Curve {
        curve: usize,
        t: f32,
        distance: f32,
        pos: [f32; 2],
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub effector: Effector,
    pub weight: f32,
}

/// A bound vertex and its effects.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetPoint {
    /// The art mesh (drawable GUID): the main mesh or a sub mesh.
    pub mesh: Guid,
    /// Vertex UID in that mesh.
    pub uid: i64,
    /// Vertex index at the last bind (`_index`).
    pub index: usize,
    /// GUID of the keyform the reference is bound to (`keyForm`), empty
    /// when unknown (the writer then uses the mesh's first keyform).
    pub form: Guid,
    pub effects: Vec<Effect>,
}

/// One deform path (`CControllerExtension`) of an art mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct DeformPath {
    /// Extension GUID.
    pub guid: Guid,
    pub edit_level: u8,
    /// Sub meshes (drawable GUIDs).
    pub sub_meshes: Vec<Guid>,
    pub max_bind: i32,
    pub bind_method: BindMethod,
    pub points: Vec<ControlPoint>,
    pub curves: Vec<ControlCurve>,
    pub targets: Vec<TargetPoint>,
}

impl DeformPath {
    pub fn new(guid: Guid, edit_level: u8) -> Self {
        Self {
            guid,
            edit_level,
            sub_meshes: Vec::new(),
            max_bind: 3,
            bind_method: BindMethod::LineAndDirection,
            points: Vec::new(),
            curves: Vec::new(),
            targets: Vec::new(),
        }
    }
}
