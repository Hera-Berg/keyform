//! The animation document (`.can3`): scenes, a track tree per scene,
//! effects and their animatable attributes with keyframe curves.
//! spec: 60-animation/animation-model.md, 10-formats/can3-schema.md

use crate::Guid;

/// The type of the curve segment starting at a key (`CCurveType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CurveType {
    Linear,
    Bezier,
    #[default]
    Smooth,
    Step,
    InverseStep,
}

impl CurveType {
    pub const NAMES: [(&'static str, CurveType); 5] = [
        ("LINEAR", CurveType::Linear),
        ("BEZIER", CurveType::Bezier),
        ("SMOOTH", CurveType::Smooth),
        ("STEP", CurveType::Step),
        ("INVERSE_STEP", CurveType::InverseStep),
    ];

    pub fn from_name(s: &str) -> Option<Self> {
        Self::NAMES.iter().find(|(n, _)| *n == s).map(|(_, t)| *t)
    }

    pub fn name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(_, t)| *t == self)
            .map_or("SMOOTH", |(n, _)| n)
    }
}

/// A Bézier handle (`CBezierCtrlPt`): a fractional frame and a value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Handle {
    pub frame: f32,
    pub value: f64,
}

/// A key point (`CBezierPt`): anchor plus optional handles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Key {
    pub frame: i32,
    pub value: f64,
    pub corner: bool,
    pub prev: Option<Handle>,
    pub next: Option<Handle>,
}

/// A float attribute's value sequence: constant (`CFixedSequence`) or a
/// keyframe curve (`CMutableSequence`; `types[i]` is the segment starting
/// at key i).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curve {
    pub base: f64,
    pub keys: Vec<Key>,
    pub types: Vec<CurveType>,
    /// Stored as a `CFixedSequence` (no keys possible).
    pub fixed: bool,
}

impl Curve {
    pub fn constant(v: f64) -> Self {
        Self {
            base: v,
            ..Default::default()
        }
    }

    pub fn type_at(&self, i: usize) -> CurveType {
        self.types.get(i).copied().unwrap_or_default()
    }
}

/// An integer attribute's curve (`CIntSequence`): (frame, value) keys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IntCurve {
    pub base: i32,
    pub keys: Vec<(i32, i32)>,
}

/// A 2-D point attribute's curve (`CXY_TNSSequence`): (frame, point)
/// keys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointCurve {
    pub base: [f32; 2],
    pub keys: Vec<(i32, [f32; 2])>,
}

/// A track's visual effect (`CMvEffect_VisualDefault`): where and how the
/// track is placed in the scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Visual {
    /// Position in the scene (scene pixels).
    pub xy: PointCurve,
    /// The point of the track that sits at `xy` (track pixels).
    pub anchor: PointCurve,
    /// Scale in percent.
    pub scale_x: Attr,
    pub scale_y: Attr,
    /// Rotation in degrees.
    pub rotate: Attr,
    pub shear: Attr,
}

/// A float attribute (`CMvAttrF`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attr {
    /// Attribute ID (`live2dParam_<ParameterId>`, `live2DPartsOpacity_<PartId>`,
    /// `opacity`, `effectLevel`, …).
    pub id: String,
    pub name: String,
    /// The parameter or part GUID it animates, if any.
    pub guid: Option<Guid>,
    pub curve: Curve,
    pub range: (f64, f64),
    /// Repeat range for repeat parameters.
    pub repeat: Option<(f64, f64)>,
    pub active: bool,
    pub shy: bool,
    pub freeze: bool,
    /// Per-curve fades in ms (`optionParam` KEY_ATTR_FADE_IN/OUT); < 0 = unset.
    pub fade_in_ms: i32,
    pub fade_out_ms: i32,
}

/// A parameter group of a model track (`CMvParameter_Group`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParamGroup {
    pub id: String,
    pub guid: Guid,
    pub frame_step: Option<IntCurve>,
    pub shy: bool,
    pub freeze: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EyeBlinkEffect {
    pub active: bool,
    /// Attribute IDs of the target parameters.
    pub targets: Vec<String>,
    pub invert: bool,
    pub relative: bool,
    /// `effectLevel` (0–100 %).
    pub level: Attr,
    /// `eyeOpen` (0–1).
    pub eye_open: Attr,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LipSyncEffect {
    pub active: bool,
    pub targets: Vec<String>,
    /// The sound track the level comes from.
    pub sync_track: Option<Guid>,
    pub invert: bool,
    pub relative: bool,
    pub sound_level: Attr,
    pub scale: Attr,
    pub base: Attr,
    /// `lipSyncLevel` (0–100 %).
    pub level: Attr,
}

/// A Live2D model track's effects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelTrack {
    /// The linked model resource.
    pub resource: Option<Guid>,
    /// Parameter attributes (`Effects:Live2DParam`), in stored order.
    pub params: Vec<Attr>,
    pub groups: Vec<ParamGroup>,
    /// Part opacity attributes.
    pub parts: Vec<Attr>,
    pub eye_blink: Option<EyeBlinkEffect>,
    pub lip_sync: Option<LipSyncEffect>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TrackKind {
    Group,
    Model(Box<ModelTrack>),
    /// Kept as stored; not evaluated yet (image, sound, text, …).
    Other(String),
}

/// A track (`ICMvTrack_Source`).
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub name: String,
    pub guid: Guid,
    /// Index of the parent track in [`Scene::tracks`].
    pub parent: Option<usize>,
    pub start: i32,
    pub offset: i32,
    pub duration: i32,
    pub visible: bool,
    pub mute: bool,
    pub guide: bool,
    pub repeat: bool,
    pub kind: TrackKind,
    /// The visual effect's opacity attribute (`Model / Opacity` on export).
    pub opacity: Option<Attr>,
    /// The visual effect's frame step (`frameStep`).
    pub frame_step: Option<IntCurve>,
    /// The visual effect's placement attributes.
    pub visual: Option<Visual>,
    /// User data markers: (frame, text).
    pub user_data: Vec<(i32, String)>,
}

/// A scene (`CSceneSource` with its `CMvMovieInfo`).
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub name: String,
    pub guid: Guid,
    /// Output (movie) size.
    pub width: i32,
    pub height: i32,
    /// The scene canvas (`CImageCanvas`), the space tracks are placed in.
    pub canvas: (i32, i32),
    /// Index of the last frame (frame count = duration + 1).
    pub duration: i32,
    pub fps: f64,
    pub work_start: i32,
    pub work_end: i32,
    /// Motion fades in ms; < 0 = unset.
    pub fade_in_ms: i32,
    pub fade_out_ms: i32,
    pub bezier_restricted: bool,
    pub loop_motion: bool,
    pub start_frame: i32,
    pub export_motion: bool,
    pub default_param_curve: CurveType,
    pub default_part_curve: CurveType,
    /// Target (`Animation` enum constant, e.g. `FOR_SDK`).
    pub target: String,
    pub tracks: Vec<Track>,
}

/// A resource (`CResourceData`), e.g. a linked model file.
#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub guid: Guid,
    pub name: String,
    /// The stored source path (as written, often absolute).
    pub path: String,
}

/// The animation document (`CAnimation`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Animation {
    pub name: String,
    pub scenes: Vec<Scene>,
    pub resources: Vec<Resource>,
    /// `isSmoothCurveLegacyMode` (animation-model.md §3.2 step 4).
    pub smooth_legacy: bool,
}
