//! Keyform data model: what a model is made of, independent of file formats,
//! GPU and UI. Cross references are indices into the model's object lists.
//!
//! spec: 20-model/object-model.md

use std::sync::Arc;

pub mod forms;
pub mod grid;
pub mod physics;

pub use forms::*;
pub use grid::*;
pub use physics::*;

pub type Color = [f32; 4];
pub const WHITE: Color = [1.0, 1.0, 1.0, 1.0];
/// Default screen color: black, opaque alpha (spec: object-model.md §7).
pub const SCREEN_DEFAULT: Color = [0.0, 0.0, 0.0, 1.0];

/// GUID text (UUID). Compared by value, never by identity.
pub type Guid = String;

/// A new random (version 4) UUID for objects and keyforms created by
/// editing. Uses the standard library's randomly keyed hasher, the clock
/// and a counter as entropy (no cryptographic strength needed).
pub fn new_guid() -> Guid {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let half = |salt: u64| {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(salt);
        h.write_u64(n);
        h.write_u64(t);
        h.finish()
    };
    let (a, b) = (half(0x5157), half(0xA11CE));
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&a.to_le_bytes());
    bytes[8..].copy_from_slice(&b.to_le_bytes());
    bytes[6] = (bytes[6] & 0x0F) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3F) | 0x80; // RFC 4122 variant
    let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// Target deformer of every deformer, art mesh, art path and glue attached
/// to the canvas root. Writers must use it for root-attached objects.
/// spec: 20-model/object-model.md §1
pub const ROOT_DEFORMER_GUID: &str = "71fae776-e218-4aee-873e-78e8ac0cb48a";
/// "NOT INITIALIZED": the target of parts and aliases (they use none).
pub const NOT_INITIALIZED_GUID: &str = "00000000-0000-0000-0000-000000000000";

pub type ParamIndex = usize;
pub type PartIndex = usize;
pub type DeformerIndex = usize;
pub type MeshIndex = usize;
pub type GlueIndex = usize;

/// The model. Object lists hold shared objects: cloning a model is cheap
/// and an edit copies only the objects it changes (`Arc::make_mut`), which
/// makes undo snapshots inexpensive (spec: 50-ui/undo.md).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub guid: Guid,
    pub name: String,
    pub canvas: Canvas,
    pub parameters: Vec<Arc<Parameter>>,
    pub parameter_groups: Vec<Arc<ParameterGroup>>,
    /// Parameters and groups at the top of the Parameter palette, in order.
    pub root_parameter_entries: Vec<ParamEntry>,
    pub parts: Vec<Arc<Part>>,
    /// Top of the part tree (`__RootPart__`).
    pub root_part: Option<PartIndex>,
    pub deformers: Vec<Arc<Deformer>>,
    pub art_meshes: Vec<Arc<ArtMesh>>,
    pub glues: Vec<Arc<Glue>>,
    pub textures: Vec<Arc<Texture>>,
    pub physics: PhysicsRig,
    pub export: ExportSettings,
    /// Parameters of the "eye blink" and "lip sync" effect groups.
    pub eye_blink_parameters: Vec<ParamIndex>,
    pub lip_sync_parameters: Vec<ParamIndex>,
    /// SDK target (`targetVersionNo`), 0 when unselected. spec: 10-formats/cmo3.md §2
    pub target_version: i32,
}

impl Model {
    /// True if `other` shares every object with `self` (nothing was edited
    /// between two snapshots). Cheap: compares pointers, not contents.
    pub fn same_objects(&self, other: &Model) -> bool {
        fn same<T>(a: &[Arc<T>], b: &[Arc<T>]) -> bool {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| Arc::ptr_eq(x, y))
        }
        same(&self.parameters, &other.parameters)
            && same(&self.parameter_groups, &other.parameter_groups)
            && same(&self.parts, &other.parts)
            && same(&self.deformers, &other.deformers)
            && same(&self.art_meshes, &other.art_meshes)
            && same(&self.glues, &other.glues)
            && same(&self.textures, &other.textures)
            && self.guid == other.guid
            && self.root_part == other.root_part
            && self.root_parameter_entries == other.root_parameter_entries
            && self.physics == other.physics
            && self.export == other.export
            && self.canvas == other.canvas
            && self.name == other.name
            && self.eye_blink_parameters == other.eye_blink_parameters
            && self.lip_sync_parameters == other.lip_sync_parameters
            && self.target_version == other.target_version
    }

    /// True if `other` holds the same data. Shared objects are compared by
    /// pointer first, so this is cheap when few objects differ.
    pub fn same_data(&self, other: &Model) -> bool {
        fn same<T: PartialEq>(a: &[Arc<T>], b: &[Arc<T>]) -> bool {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| Arc::ptr_eq(x, y) || **x == **y)
        }
        same(&self.parameters, &other.parameters)
            && same(&self.parameter_groups, &other.parameter_groups)
            && same(&self.parts, &other.parts)
            && same(&self.deformers, &other.deformers)
            && same(&self.art_meshes, &other.art_meshes)
            && same(&self.glues, &other.glues)
            && same(&self.textures, &other.textures)
            && self.guid == other.guid
            && self.root_part == other.root_part
            && self.root_parameter_entries == other.root_parameter_entries
            && self.physics == other.physics
            && self.export == other.export
            && self.canvas == other.canvas
            && self.name == other.name
            && self.eye_blink_parameters == other.eye_blink_parameters
            && self.lip_sync_parameters == other.lip_sync_parameters
            && self.target_version == other.target_version
    }

    /// GUID to write as `targetDeformerGuid` for an object attached to
    /// `target` (the ROOT GUID when attached to the canvas).
    pub fn target_deformer_guid(&self, target: Option<DeformerIndex>) -> &str {
        match target.and_then(|t| self.deformers.get(t)) {
            Some(d) if !d.info.guid.is_empty() => &d.info.guid,
            _ => ROOT_DEFORMER_GUID,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
        }
    }
}

/// Export-for-runtime settings stored with the model.
/// spec: 70-advanced/export-dialog.md §1
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExportSettings {
    /// moc3 header version chosen in the export dialog, if stored.
    pub moc_version: Option<u8>,
    /// Export origin as a fraction of the canvas size.
    pub origin_normalized: [f32; 2],
    /// Pixels per unit; `None` = default, the shorter canvas side.
    /// spec: 70-advanced/export-dialog.md §1.2
    pub pixels_per_unit: Option<f32>,
    pub export_invisible_parts: bool,
    pub export_invisible_art_meshes: bool,
    pub export_sketch_parts: bool,
    /// "Apply color leakage prevention to texture"; on for a model's first
    /// export, then remembered. spec: export-dialog.md §1
    pub extend_edge_color: bool,
    /// "Export Calculate FPS to Physics settings file" (the stored option is
    /// its inverse, `ignoreFPSOnPhysicsJson`). spec: export-dialog.md §1
    pub physics_fps: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            moc_version: None,
            origin_normalized: [0.5, 0.5],
            pixels_per_unit: None,
            export_invisible_parts: false,
            export_invisible_art_meshes: false,
            export_sketch_parts: false,
            extend_edge_color: true,
            physics_fps: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    pub guid: Guid,
    pub id: String,
    pub name: String,
    pub description: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub repeat: bool,
    pub kind: ParameterKind,
    pub decimal_places: i32,
    /// Snap epsilon εₛ. spec: object-model.md §3
    pub snap_epsilon: f32,
    pub combined: bool,
    pub group: Option<usize>,
}

impl Parameter {
    pub fn new(id: &str, min: f32, max: f32, default: f32) -> Self {
        Self {
            guid: Guid::new(),
            id: id.to_owned(),
            name: id.to_owned(),
            description: String::new(),
            min,
            max,
            default,
            repeat: false,
            kind: ParameterKind::Normal,
            decimal_places: 3,
            snap_epsilon: 0.001,
            combined: false,
            group: None,
        }
    }

    /// Interpolation epsilon εᵢ = 1.5·εₛ. spec: object-model.md §3
    pub fn interp_epsilon(&self) -> f32 {
        1.5 * self.snap_epsilon
    }

    /// Clamps to [min, max] unless the parameter repeats.
    pub fn clamp(&self, v: f32) -> f32 {
        if self.repeat {
            v
        } else {
            v.clamp(self.min.min(self.max), self.max.max(self.min))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterKind {
    Normal,
    /// Blend-shape parameter (stored as `MORPH_TARGET`).
    BlendShape,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParameterGroup {
    pub guid: Guid,
    pub id: String,
    pub name: String,
    pub parent: Option<usize>,
    pub children: Vec<ParamEntry>,
}

/// An entry of a parameter-group folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamEntry {
    Parameter(ParamIndex),
    Group(usize),
}

/// Fields shared by every parameter-controllable object. spec: object-model.md §4
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectInfo {
    pub guid: Guid,
    pub id: String,
    /// Display name; may be empty (the UI then shows the id).
    pub name: String,
    pub parent_part: Option<PartIndex>,
    pub visible: bool,
    pub locked: bool,
}

impl ObjectInfo {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            visible: true,
            ..Self::default()
        }
    }

    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
}

/// A child in the part tree, in Parts-palette order (top first).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartChild {
    Part(PartIndex),
    Deformer(DeformerIndex),
    ArtMesh(MeshIndex),
    Glue(GlueIndex),
    /// Something Keyform does not model yet (art path, alias, …).
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorComposition {
    #[default]
    Normal,
    /// Legacy "Add (Before 5.3)".
    Add,
    /// Legacy "Multiply (Before 5.3)".
    Multiply,
    AddR2Tsl,
    AddR2,
    Darken,
    MultiplyR2,
    ColorBurnTsl,
    LinearBurnTsl,
    Lighten,
    Screen,
    ColorDodgeTsl,
    Overlay,
    SoftLight,
    HardLight,
    LinearLightTsl,
    HslHue,
    HslColor,
}

impl ColorComposition {
    /// Stored names (spec: 40-rendering/blending.md §1).
    pub const NAMES: [(&'static str, ColorComposition); 18] = [
        ("NORMAL", Self::Normal),
        ("ADD", Self::Add),
        ("MULTIPLY", Self::Multiply),
        ("ADD_R2_TSL", Self::AddR2Tsl),
        ("ADD_R2", Self::AddR2),
        ("DARKEN", Self::Darken),
        ("MULTIPLY_R2", Self::MultiplyR2),
        ("COLORBURN_TSL", Self::ColorBurnTsl),
        ("LINEARBURN_TSL", Self::LinearBurnTsl),
        ("LIGHTEN", Self::Lighten),
        ("SCREEN", Self::Screen),
        ("COLORDODGE_TSL", Self::ColorDodgeTsl),
        ("OVERLAY", Self::Overlay),
        ("SOFTLIGHT", Self::SoftLight),
        ("HARDLIGHT", Self::HardLight),
        ("LINEARLIGHT_TSL", Self::LinearLightTsl),
        ("HSL_HUE", Self::HslHue),
        ("HSL_COLOR", Self::HslColor),
    ];

    pub fn from_name(s: &str) -> Option<Self> {
        Self::NAMES.iter().find(|(n, _)| *n == s).map(|(_, c)| *c)
    }

    pub fn name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(_, c)| *c == self)
            .map_or("NORMAL", |(n, _)| n)
    }

    /// moc3 color-blend code (spec: 10-formats/moc3.md §7).
    pub fn moc3_code(self) -> u8 {
        use ColorComposition::*;
        match self {
            Normal => 0,
            Add => 1,
            Multiply => 2,
            AddR2Tsl => 3,
            AddR2 => 4,
            Darken => 5,
            MultiplyR2 => 6,
            ColorBurnTsl => 7,
            LinearBurnTsl => 8,
            Lighten => 9,
            Screen => 10,
            ColorDodgeTsl => 11,
            Overlay => 12,
            SoftLight => 13,
            HardLight => 14,
            LinearLightTsl => 15,
            HslHue => 16,
            HslColor => 17,
        }
    }

    pub fn from_moc3_code(c: u8) -> Option<Self> {
        Self::NAMES
            .iter()
            .map(|(_, m)| *m)
            .find(|m| m.moc3_code() == c)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlphaComposition {
    #[default]
    Over,
    Atop,
    Out,
    Conjoint,
    Disjoint,
}

impl AlphaComposition {
    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "OVER" => Self::Over,
            "ATOP" => Self::Atop,
            "OUT" => Self::Out,
            "CONJOINT" => Self::Conjoint,
            "DISJOINT" => Self::Disjoint,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Over => "OVER",
            Self::Atop => "ATOP",
            Self::Out => "OUT",
            Self::Conjoint => "CONJOINT",
            Self::Disjoint => "DISJOINT",
        }
    }

    pub fn moc3_code(self) -> u8 {
        self as u8
    }

    pub fn from_moc3_code(c: u8) -> Option<Self> {
        [
            Self::Over,
            Self::Atop,
            Self::Out,
            Self::Conjoint,
            Self::Disjoint,
        ]
        .get(c as usize)
        .copied()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub info: ObjectInfo,
    pub children: Vec<PartChild>,
    pub draw_order_group: bool,
    pub sketch: bool,
    pub offscreen: bool,
    pub clip: Vec<MeshIndex>,
    pub invert_clip: bool,
    /// The stored clip list names drawables that no longer exist
    /// (spec: 50-ui/dialogs.md §21 "deleted mask").
    pub clip_missing: bool,
    pub color_composition: ColorComposition,
    pub alpha_composition: AlphaComposition,
    /// Editor default draw order (500 unless changed). spec: object-model.md §11
    pub default_order: i32,
    pub grid: KeyformGrid<PartForm>,
    pub blend_shapes: BlendShapes<PartForm>,
}

impl Part {
    pub fn new(id: &str) -> Self {
        Self {
            info: ObjectInfo::new(id),
            children: Vec::new(),
            draw_order_group: false,
            sketch: false,
            offscreen: false,
            clip: Vec::new(),
            invert_clip: false,
            clip_missing: false,
            color_composition: ColorComposition::Normal,
            alpha_composition: AlphaComposition::Over,
            default_order: 500,
            grid: KeyformGrid::single(PartForm::default()),
            blend_shapes: BlendShapes::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Deformer {
    pub info: ObjectInfo,
    pub target: Option<DeformerIndex>,
    pub kind: DeformerKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DeformerKind {
    Warp(WarpDeformer),
    Rotation(RotationDeformer),
}

#[derive(Clone, Debug, PartialEq)]
pub struct WarpDeformer {
    pub cols: u32,
    pub rows: u32,
    /// Bilinear cells instead of two triangles. spec: 30-algorithms/deformers.md §1.2
    pub quad: bool,
    pub grid: KeyformGrid<WarpForm>,
    pub blend_shapes: BlendShapes<WarpForm>,
}

impl WarpDeformer {
    pub fn point_count(&self) -> usize {
        (self.cols as usize + 1) * (self.rows as usize + 1)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RotationDeformer {
    /// Degrees, added to every keyform angle. spec: deformers.md §2.1
    pub base_angle: f32,
    pub grid: KeyformGrid<RotationForm>,
    pub blend_shapes: BlendShapes<RotationForm>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArtMesh {
    pub info: ObjectInfo,
    pub target: Option<DeformerIndex>,
    /// Rest positions in canvas pixels, x,y interleaved.
    pub positions: Vec<f32>,
    /// Atlas UVs, 2 per vertex, v = 0 at the top row.
    pub uvs: Vec<f32>,
    /// Triangle list.
    pub indices: Vec<u32>,
    /// Index into [`Model::textures`].
    pub texture: Option<usize>,
    pub clip: Vec<MeshIndex>,
    pub invert_clip: bool,
    /// The stored clip list names drawables that no longer exist
    /// (spec: 50-ui/dialogs.md §21 "deleted mask").
    pub clip_missing: bool,
    pub culling: bool,
    pub color_composition: ColorComposition,
    pub alpha_composition: AlphaComposition,
    pub user_data: String,
    /// Stable per-vertex UIDs from the editable mesh (glue binding).
    pub point_uids: Vec<i64>,
    pub grid: KeyformGrid<ArtMeshForm>,
    pub blend_shapes: BlendShapes<ArtMeshForm>,
}

impl ArtMesh {
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 2
    }

    /// Index of the vertex with editable-mesh UID `uid`.
    pub fn vertex_of_uid(&self, uid: i64) -> Option<usize> {
        self.point_uids.iter().position(|&u| u == uid)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Glue {
    pub info: ObjectInfo,
    pub mesh_a: Option<MeshIndex>,
    pub mesh_b: Option<MeshIndex>,
    /// Bound pairs as vertex indices; `None` when the UID didn't resolve.
    pub pairs: Vec<(Option<usize>, Option<usize>)>,
    /// (wA, wB) per pair. spec: 30-algorithms/glue.md
    pub weights: Vec<[f32; 2]>,
    pub grid: KeyformGrid<GlueForm>,
    pub blend_shapes: BlendShapes<GlueForm>,
}

/// A texture image (atlas page) with its PNG bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub guid: Guid,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Straight-alpha PNG data, if the image is stored.
    pub png: Option<Vec<u8>>,
    /// The stored page is used as is and never re-rendered from the model
    /// images. spec: 70-advanced/texture-atlas.md §1, §6
    pub locked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_attached_objects_use_the_root_guid() {
        let mut m = Model::default();
        assert_eq!(m.target_deformer_guid(None), ROOT_DEFORMER_GUID);
        m.deformers.push(Arc::new(Deformer {
            info: ObjectInfo {
                guid: "abc".into(),
                ..ObjectInfo::new("Warp")
            },
            target: None,
            kind: DeformerKind::Rotation(RotationDeformer {
                base_angle: 0.0,
                grid: KeyformGrid::single(RotationForm::default()),
                blend_shapes: BlendShapes::default(),
            }),
        }));
        assert_eq!(m.target_deformer_guid(Some(0)), "abc");
        assert_eq!(m.target_deformer_guid(Some(7)), ROOT_DEFORMER_GUID);
    }
}
