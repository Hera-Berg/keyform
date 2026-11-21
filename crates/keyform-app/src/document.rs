//! An open model and its runtime state (parameters, playback, physics).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use keyform_anim::{
    Breath, Expression, ExpressionPlayer, EyeBlink, Motion, MotionOutputs, MotionPlayer,
    ParamRange, Params, Pose, PoseState, Rng, Tracking,
};
use keyform_edit::{History, StepKind};
use keyform_eval::{EvalOptions, Evaluator, Frame, Overrides};
use keyform_formats::cmo3::Cmo3Document;
use keyform_formats::moc3::{moc3_to_model, Moc3Raw};
use keyform_formats::runtime_json::{Model3, Model3Group, MotionRef};
use keyform_model::{DeformerKind, Model, Texture};
use keyform_physics::PhysicsSim;

use crate::log::Log;

/// Which object the user selected (Parts / Deformer palettes).
pub use keyform_edit::ObjectRef as Selection;

/// Key operations of the Parameter palette. spec: 50-ui/palettes.md §3,
/// 35-editing/keys.md
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOp {
    /// At the current value.
    Add,
    /// At the current value.
    Delete,
    /// Min and max.
    AddTwo,
    /// Min, default and max.
    AddThree,
    /// Unbind, keeping the shape at the current value.
    DeleteAll,
}

/// What the document view does. spec: 50-ui/main-window.md §4 (a model
/// view uses the Modeling workspace; runtime models open in the viewer)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Editing: the model shows exactly the slider values.
    Modeling,
    /// Playback preview (viewer.md §3).
    Viewer,
    /// An animation (`.can3`) played on the model at the playhead.
    Animation,
}

/// An object being dragged on the canvas: edits are previewed live on top of
/// `base` and committed as one undo step at the end.
struct MoveTarget {
    object: Selection,
    cell: usize,
    /// The parent deformer's map at drag start.
    parent: Option<keyform_eval::DeformerMap>,
    /// Points to move with their soft-selection weights; all if `None`.
    points: Option<Vec<(usize, f32)>>,
    /// Warp net level when the points are Bezier net points.
    net_level: Option<u8>,
    /// The blend-shape entry edited instead of the grid keyform.
    blend: Option<usize>,
}

enum DragKind {
    Move(Vec<MoveTarget>),
    /// Rotation-deformer handle: angle about `centre` (canvas).
    Rotate {
        target: Selection,
        cell: usize,
        centre: [f32; 2],
        start: [f32; 2],
    },
    /// Rotation-deformer circle: scale about `centre`.
    Scale {
        target: Selection,
        cell: usize,
        centre: [f32; 2],
        start: [f32; 2],
    },
}

/// A paste waiting for its pairing (multi-paste dialog).
#[derive(Clone, Debug)]
pub struct PastePairing {
    pub name: String,
    pub w: f32,
    pub mirror: keyform_edit::form_tools::PasteMirror,
    /// Candidate copies of each target.
    pub candidates: Vec<(Selection, Vec<usize>)>,
    /// The chosen copy of each target.
    pub pairs: Vec<(Selection, Option<usize>)>,
}

struct Drag {
    base: Model,
    kind: DragKind,
}

/// Which part of a rotation deformer's on-canvas widget was grabbed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Rotate,
    Scale,
}

/// Angle increment of Shift-rotation (Environment ▸ Modeling default).
/// Environment Settings that act on documents (dialogs.md §11, defaults
/// settings-keys.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prefs {
    /// Shift-rotation increment of rotation deformers, degrees.
    pub rotation_step: f32,
    /// Double-clicking an art mesh starts mesh edit.
    pub mesh_edit_double_click: bool,
    /// Pasted objects' names get a number too.
    pub paste_numbered_names: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            rotation_step: 45.0,
            mesh_edit_double_click: true,
            paste_numbered_names: false,
        }
    }
}

/// Binding more parameters than this to one object shows a warning
/// (palettes.md §3).
const BINDING_WARN: usize = 3;

/// Animation menu toggles, persisted per user. spec: 70-advanced/viewer.md §3.7
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PlaybackOptions {
    pub expression: bool,
    pub eye_blink: bool,
    pub physics: bool,
    pub pose: bool,
    /// Motion-sync lip sync while a motion with a sound plays (§3.1 step 11,
    /// §3.8).
    pub motion_sync: bool,
    /// Start a random motion whenever the queue is empty (§3.1 step 2).
    pub idle_motion: bool,
    pub track_cursor: bool,
    pub breath: bool,
    /// Repeat parameters wrap instead of clamping (viewer.md §3.8).
    pub parameter_repeat: bool,
    /// Restricted Bezier evaluation for every motion.
    pub old_playback: bool,
    /// Expressions through the old fading method (§3.3).
    pub old_fading: bool,
}

impl Default for PlaybackOptions {
    fn default() -> Self {
        Self {
            expression: true,
            eye_blink: true,
            physics: true,
            pose: true,
            motion_sync: true,
            idle_motion: false,
            track_cursor: true,
            breath: true,
            parameter_repeat: false,
            old_playback: false,
            old_fading: false,
        }
    }
}

/// Selected objects (last = primary) and selected points of them.
/// spec: 50-ui/canvas-and-tools.md §3
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionSet {
    pub objects: Vec<Selection>,
    /// (object, point index): mesh vertices, warp grid points, rotation origin.
    pub points: Vec<(Selection, usize)>,
    /// Soft-selection weight per entry of `points` (0…1); empty = all 1.
    /// spec: canvas-and-tools.md §4 "Soft Selection"
    pub point_weights: Vec<f32>,
}

/// `sel` (made on `old`) for `new`: objects matched by GUID; points kept
/// only when their object's index, kind and point count are unchanged.
pub fn remap_selection(old: &Model, new: &Model, sel: &SelectionSet) -> SelectionSet {
    fn guid(m: &Model, o: Selection) -> Option<&str> {
        Some(match o {
            Selection::Part(i) => m.parts.get(i)?.info.guid.as_str(),
            Selection::Deformer(i) => m.deformers.get(i)?.info.guid.as_str(),
            Selection::ArtMesh(i) => m.art_meshes.get(i)?.info.guid.as_str(),
            Selection::Glue(i) => m.glues.get(i)?.info.guid.as_str(),
        })
    }
    fn find(m: &Model, o: Selection, g: &str) -> Option<Selection> {
        let pos = |v: &[&str]| v.iter().position(|x| *x == g);
        match o {
            Selection::Part(_) => pos(&m
                .parts
                .iter()
                .map(|x| x.info.guid.as_str())
                .collect::<Vec<_>>())
            .map(Selection::Part),
            Selection::Deformer(_) => pos(&m
                .deformers
                .iter()
                .map(|x| x.info.guid.as_str())
                .collect::<Vec<_>>())
            .map(Selection::Deformer),
            Selection::ArtMesh(_) => pos(&m
                .art_meshes
                .iter()
                .map(|x| x.info.guid.as_str())
                .collect::<Vec<_>>())
            .map(Selection::ArtMesh),
            Selection::Glue(_) => pos(&m
                .glues
                .iter()
                .map(|x| x.info.guid.as_str())
                .collect::<Vec<_>>())
            .map(Selection::Glue),
        }
    }
    fn points(m: &Model, o: Selection) -> usize {
        match o {
            Selection::ArtMesh(i) => m.art_meshes.get(i).map_or(0, |a| a.positions.len() / 2),
            Selection::Deformer(i) => m.deformers.get(i).map_or(0, |d| match &d.kind {
                keyform_model::DeformerKind::Warp(w) => {
                    w.grid.keyforms.first().map_or(0, |f| f.positions.len() / 2)
                }
                keyform_model::DeformerKind::Rotation(_) => 1,
            }),
            _ => 0,
        }
    }
    let map = |o: Selection| guid(old, o).and_then(|g| find(new, o, g));
    let objects: Vec<Selection> = sel.objects.iter().filter_map(|&o| map(o)).collect();
    let mut out = SelectionSet {
        objects,
        points: Vec::new(),
        point_weights: Vec::new(),
    };
    for (k, &(o, i)) in sel.points.iter().enumerate() {
        if map(o) == Some(o) && points(old, o) == points(new, o) {
            out.points.push((o, i));
            if let Some(w) = sel.point_weights.get(k) {
                out.point_weights.push(*w);
            }
        }
    }
    if out.point_weights.len() != out.points.len() {
        out.point_weights.clear();
    }
    out
}

impl SelectionSet {
    /// Soft-selection weight of point entry `i` (1 when unweighted).
    pub fn weight(&self, i: usize) -> f32 {
        self.point_weights.get(i).copied().unwrap_or(1.0)
    }

    /// Keeps the point entries for which `keep` holds, with their weights.
    pub fn retain_points(&mut self, keep: impl Fn(&(Selection, usize)) -> bool) {
        let mut w = Vec::new();
        let weighted = !self.point_weights.is_empty();
        let mut pts = Vec::new();
        for (i, p) in self.points.iter().enumerate() {
            if keep(p) {
                pts.push(*p);
                if weighted {
                    w.push(self.weight(i));
                }
            }
        }
        self.points = pts;
        self.point_weights = w;
    }

    /// Sets the point entries (unweighted).
    pub fn set_points(&mut self, pts: Vec<(Selection, usize)>) {
        self.points = pts;
        self.point_weights.clear();
    }

    pub fn of(o: Option<Selection>) -> Self {
        Self {
            objects: o.into_iter().collect(),
            points: Vec::new(),
            point_weights: Vec::new(),
        }
    }
}

/// Is `q` inside the convex quadrilateral `c` (either winding)?
/// Whether `q` is inside the closed polygon `poly` (even-odd rule; a
/// rectangle drag's quad or a lasso path).
pub fn in_polygon(poly: &[[f32; 2]], q: [f32; 2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > q[1]) != (b[1] > q[1]) {
            let x = a[0] + (q[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if q[0] < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// The undoable state of a document (spec: 50-ui/undo.md §2). The edit
/// level joins it when edit levels are implemented.
#[derive(Clone)]
pub struct EditState {
    pub model: Model,
    pub selection: SelectionSet,
    pub edit_level: u8,
}

pub struct MotionEntry {
    pub group: String,
    /// The model3.json entry (file, sound, fades).
    pub reference: MotionRef,
    /// `None` when the file could not be loaded (kept so saving keeps it).
    pub motion: Option<Arc<Motion>>,
}

pub struct ExpressionEntry {
    pub name: String,
    pub file: String,
    pub expression: Expression,
    /// Edited since loading: written on save.
    pub dirty: bool,
    /// The file could not be loaded: never overwritten.
    pub broken: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewState {
    /// Canvas point shown at the centre of the view.
    pub center: [f32; 2],
    /// Screen points per canvas pixel.
    pub zoom: f32,
    pub rotation: f32,
    pub mirror: bool,
    /// Fit the canvas into the view on the next frame.
    pub fit_pending: bool,
    /// Fit this canvas box (min x, min y, max x, max y) instead.
    pub fit_box: Option<[f32; 4]>,
}

pub enum Source {
    /// A `.cmo3` project.
    Project(Box<Cmo3Document>),
    /// A runtime model (`.moc3`, optionally described by a model3.json).
    Runtime {
        model3: Option<Box<Model3>>,
        dir: PathBuf,
    },
}

pub struct Document {
    pub path: PathBuf,
    pub title: String,
    pub source: Source,
    pub model: Model,
    /// Values set by the user (sliders).
    pub values: Vec<f32>,
    /// Values after motion, expression and physics this frame.
    pub live: Vec<f32>,
    pub param_index: HashMap<String, usize>,
    pub part_index: HashMap<String, usize>,
    pub sim: PhysicsSim,
    pub motions: Vec<MotionEntry>,
    pub expressions: Vec<ExpressionEntry>,
    pub pose: Option<Pose>,
    /// Created (reset) when the pose is first applied (viewer.md §3.4).
    pose_state: Option<PoseState>,
    /// Real parameter values saved after the motion update (viewer.md §3.1).
    saved: Vec<f32>,
    /// Values the user set by hand (Parameter palette); applied each frame.
    pub manual: Vec<bool>,
    /// Virtual parameters (part-ID parameters written by PartOpacity curves).
    pub virtuals: HashMap<String, f32>,
    ranges: Vec<ParamRange>,
    motion_out: MotionOutputs,
    breath: Breath,
    /// Cursor tracking state (viewer.md §3.6).
    pub tracking: Tracking,
    /// Motion-sync settings of a runtime model (motionsync3.json).
    pub motion_sync: Vec<keyform_anim::motionsync::Setting>,
    /// Analysed frames per motion index (`None` = no usable sound).
    motion_sync_frames: HashMap<usize, Option<(f32, Vec<keyform_anim::motionsync::FrameMap>)>>,
    /// The motion most recently started (its model3.json entry).
    playing_motion: Option<usize>,
    /// Index of the last motion started by idle playback.
    idle_last: Option<usize>,
    idle_rng: Rng,
    eye_blink: EyeBlink,
    pub motion_player: MotionPlayer,
    pub expression_player: ExpressionPlayer,
    pub looping: bool,
    /// Playback clock (seconds).
    pub time: f64,
    pub frame: Option<Frame>,
    /// Opacity the rendered frame is composited with (an animation track's
    /// group opacity, animation-model.md §5.1); 1 elsewhere.
    pub group_opacity: f32,
    pub view: ViewState,
    pub selection: SelectionSet,
    pub hit_areas: Vec<(String, String)>,
    /// Parts palette order comes from part child lists (projects) or from
    /// parent links (runtime models, which store no palette order).
    pub palette_from_children: bool,
    pub history: History<EditState>,
    pub mode: Mode,
    /// Edit level 1 (refine), 2 (normal) or 3 (rough): which helpers are
    /// shown and edited (main-window.md §1).
    pub edit_level: u8,
    drag: Option<Drag>,
    /// A message for the log from a canvas tool (taken by the app).
    pub notice: Option<String>,
    /// Environment Settings that act on documents (set by the app).
    pub prefs: Prefs,
    /// The guide selected in Guide Settings (drawn 3 px wide).
    pub guide_selected: Option<usize>,
    /// This open document's identity for the session (same-document paste,
    /// object-clipboard.md §3 fileContentInstanceGuid).
    pub instance: u64,
    /// A parameter dialog requested by a palette (taken by the app).
    pub param_request: Option<crate::param_dialog::Request>,
    /// Result of the mask check, recomputed when the model changes
    /// (dialogs.md §21).
    pub mask_issues: keyform_edit::MaskIssues,
    /// History revision at the last save (the unmodified state).
    saved_revision: u64,
    /// Mesh edit mode on one art mesh (main-window.md §6).
    pub mesh_edit: Option<crate::mesh_edit::MeshEdit>,
    /// Parameter values pushed by an external application (they override
    /// the sliders until cleared). spec: external-api.md §4, §5
    pub external: Vec<(usize, f32)>,
    /// Mesh editor settings kept between mesh-edit sessions.
    mesh_prefs: Option<crate::mesh_edit::MeshPrefs>,
    /// Glue weight brush tool, when active (canvas-and-tools.md "Glue").
    pub glue_tool: Option<crate::glue_tool::GlueTool>,
    /// Glue brush settings, kept while the tool is off.
    pub glue_prefs: crate::glue_tool::GluePrefs,
    /// Lasso Tool [L] instead of the Arrow tool's rectangle [A]: dragging
    /// on empty space draws a lasso. spec: canvas-and-tools.md §3
    pub lasso: bool,
    /// Soft Selection tool, when active.
    pub soft_select: Option<crate::soft_select::SoftSelect>,
    /// Soft Selection brush settings (size 100 px, weight 100 %).
    pub soft_prefs: crate::glue_tool::GluePrefs,
    /// Control Point edit tool [P] (deform paths), when active.
    pub path_tool: Option<crate::path_tool::PathTool>,
    /// ArtPath tool group (art-path.md §5) and its settings.
    pub art_tool: Option<crate::artpath_tool::ArtTool>,
    pub art_prefs: crate::artpath_tool::ArtPrefs,
    /// Its Tool Details settings, kept while the tool is off.
    pub path_prefs: crate::path_tool::PathPrefs,
    /// Temporary path handler, while active (deform-paths.md §6).
    pub temp_path: Option<crate::path_tool::TempPath>,
    /// A paste whose copy ↔ target pairing is ambiguous, waiting for the
    /// multi-paste dialog (form-tools.md §2).
    pub paste_pairing: Option<PastePairing>,
    /// The animation of a `.can3` document (Animation workspace).
    pub anim: Option<crate::animation::AnimState>,
    /// Copy Form data (form-tools.md §1).
    pub form_clipboard: Vec<keyform_edit::form_tools::CopiedForm>,
    /// Never saved yet: the first save asks for a location.
    pub untitled: bool,
}

pub fn load_motion(dir: &Path, file: &str, log: &mut Log) -> Option<Arc<Motion>> {
    match std::fs::read_to_string(dir.join(file))
        .map_err(|e| e.to_string())
        .and_then(|t| Motion::parse(&t).map_err(|e| e.to_string()))
    {
        Ok(m) => Some(Arc::new(m)),
        Err(e) => {
            log.error(format!("{file}: {e}"));
            None
        }
    }
}

/// `path` relative to `dir` with forward slashes (model3.json references),
/// or `None` if it isn't inside `dir`.
pub fn relative_to(dir: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(dir).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn load_runtime_textures(m: &mut Model, dir: &Path, files: &[String], log: &mut Log) {
    for (i, t) in files.iter().enumerate() {
        match std::fs::read(dir.join(t)) {
            Ok(png) => {
                let (w, h) = keyform_formats::image::RgbaImage::decode_png(&png)
                    .map_or((0, 0), |i| (i.width, i.height));
                m.textures.push(std::sync::Arc::new(Texture {
                    sampling: Default::default(),
                    guid: String::new(),
                    name: format!("texture_{i:02}"),
                    width: w,
                    height: h,
                    png: Some(png),
                    locked: true,
                    entries: Vec::new(),
                }));
            }
            Err(e) => log.warn(format!("texture {t}: {e}")),
        }
    }
}

impl Document {
    /// Opens a `.cmo3`, `.moc3` or `.model3.json`.
    pub fn open(path: &Path, log: &mut Log) -> Result<Self, String> {
        Self::open_with(path, None, log)
    }

    /// A new, unsaved project (e.g. a new model from a PSD); `path` is
    /// the suggested location, asked for on the first save.
    pub fn new_project(path: &Path, doc: Cmo3Document, log: &mut Log) -> Result<Self, String> {
        let mut d = Self::open_with(path, Some(doc), log)?;
        d.untitled = true;
        // Unsaved: closing asks to save it.
        d.saved_revision = u64::MAX;
        Ok(d)
    }

    fn open_with(
        path: &Path,
        preloaded: Option<Cmo3Document>,
        log: &mut Log,
    ) -> Result<Self, String> {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let lower = name.to_lowercase();
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut motion_sync = Vec::new();
        let (source, mut model, mut motions, mut expressions, mut pose, mut hit_areas) = (
            None,
            Model::default(),
            Vec::new(),
            Vec::new(),
            None,
            Vec::new(),
        );
        let mut source = source;
        if lower.ends_with(".cmo3") {
            let doc = match preloaded {
                Some(d) => d,
                None => Cmo3Document::read(&read(path)?).map_err(|e| e.to_string())?,
            };
            for w in &doc.warnings {
                log.warn(format!("{name}: {w}"));
            }
            model = doc.model.clone();
            motion_sync = model
                .motion_sync
                .iter()
                .map(keyform_anim::motionsync::Setting::from_model)
                .collect();
            source = Some(Source::Project(Box::new(doc)));
        } else if lower.ends_with(".moc3") {
            let raw = Moc3Raw::read(&read(path)?).map_err(|e| e.to_string())?;
            model = moc3_to_model(&raw).map_err(|e| e.to_string())?;
            log.info(format!(
                "{name}: moc3 version {}, no model3.json — textures are not loaded",
                raw.version
            ));
            source = Some(Source::Runtime {
                model3: None,
                dir: dir.clone(),
            });
        } else if lower.ends_with(".model3.json") {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{name}: {e}"))?;
            let m3 = Model3::parse(&text).map_err(|e| e.to_string())?;
            let raw = Moc3Raw::read(&read(&dir.join(&m3.moc))?)
                .map_err(|e| format!("{}: {e}", m3.moc))?;
            model = moc3_to_model(&raw).map_err(|e| format!("{}: {e}", m3.moc))?;
            load_runtime_textures(&mut model, &dir, &m3.textures, log);
            let pindex: HashMap<String, usize> = model
                .parameters
                .iter()
                .enumerate()
                .map(|(i, p)| (p.id.clone(), i))
                .collect();
            if let Some(ph) = &m3.physics {
                match std::fs::read_to_string(dir.join(ph)) {
                    Ok(t) => match keyform_physics::json3::parse(&t, &|id| pindex.get(id).copied())
                    {
                        Ok((rig, warns)) => {
                            for w in warns {
                                log.warn(format!("{ph}: {w}"));
                            }
                            model.physics = rig;
                        }
                        Err(e) => log.error(format!("{ph}: {e}")),
                    },
                    Err(e) => log.error(format!("{ph}: {e}")),
                }
            }
            if let Some(di) = &m3.display_info {
                match std::fs::read_to_string(dir.join(di))
                    .map_err(|e| e.to_string())
                    .and_then(|t| {
                        keyform_formats::runtime_json::apply_display_info(&mut model, &t)
                            .map_err(|e| e.to_string())
                    }) {
                    Ok(n) => log.info(format!("{di}: {n} display names")),
                    Err(e) => log.warn(format!("{di}: {e}")),
                }
            }
            // Effect groups from model3.json (runtime-json.md §2).
            for g in m3.groups.iter().flatten() {
                let list: Vec<usize> = g
                    .ids
                    .iter()
                    .filter_map(|i| pindex.get(i).copied())
                    .collect();
                match g.name.as_str() {
                    "EyeBlink" => model.eye_blink_parameters = list,
                    "LipSync" => model.lip_sync_parameters = list,
                    _ => {}
                }
            }
            for (group, refs) in m3.motions.iter().flatten() {
                for r in refs {
                    let motion = load_motion(&dir, &r.file, log);
                    motions.push(MotionEntry {
                        group: group.clone(),
                        reference: r.clone(),
                        motion,
                    });
                }
            }
            for (ename, f) in m3.expressions.iter().flatten() {
                match std::fs::read_to_string(dir.join(f))
                    .map_err(|e| e.to_string())
                    .and_then(|t| Expression::parse(&t).map_err(|e| e.to_string()))
                {
                    Ok(e) => expressions.push(ExpressionEntry {
                        name: ename.clone(),
                        file: f.clone(),
                        expression: e,
                        dirty: false,
                        broken: false,
                    }),
                    Err(e) => {
                        log.error(format!("{f}: {e}"));
                        expressions.push(ExpressionEntry {
                            name: ename.clone(),
                            file: f.clone(),
                            expression: Expression::default(),
                            dirty: false,
                            broken: true,
                        });
                    }
                }
            }
            if let Some(p) = &m3.pose {
                match std::fs::read_to_string(dir.join(p))
                    .map_err(|e| e.to_string())
                    .and_then(|t| Pose::parse(&t).map_err(|e| e.to_string()))
                {
                    Ok(p) => pose = Some(p),
                    Err(e) => log.error(format!("{p}: {e}")),
                }
            }
            hit_areas = m3.hit_areas.clone().unwrap_or_default();
            if let Some(f) = &m3.motion_sync {
                match std::fs::read_to_string(dir.join(f))
                    .map_err(|e| e.to_string())
                    .and_then(|t| keyform_anim::motionsync::parse(&t).map_err(|e| e.to_string()))
                {
                    Ok(s) => motion_sync = s,
                    Err(e) => log.error(format!("{f}: {e}")),
                }
            }
            source = Some(Source::Runtime {
                model3: Some(Box::new(m3)),
                dir: dir.clone(),
            });
        }
        let Some(source) = source else {
            return Err(format!(
                "{name}: unsupported file type (open a .cmo3, .moc3 or .model3.json)"
            ));
        };
        let values: Vec<f32> = model.parameters.iter().map(|p| p.default).collect();
        let param_index = model
            .parameters
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id.clone(), i))
            .collect();
        let part_index = model
            .parts
            .iter()
            .enumerate()
            .map(|(i, p)| (p.info.id.clone(), i))
            .collect();
        let sim = PhysicsSim::new(&model.physics);
        let saved_init = values.clone();
        let n_params = values.len();
        let ranges: Vec<ParamRange> = model
            .parameters
            .iter()
            .map(|p| ParamRange {
                min: p.min,
                max: p.max,
                repeat: p.repeat,
            })
            .collect();
        log.info(format!(
            "opened {name}: {} parameters, {} parts, {} art meshes, {} physics settings, {} motions, {} expressions",
            model.parameters.len(),
            model.parts.len(),
            model.art_meshes.len(),
            model.physics.settings.len(),
            motions.len(),
            expressions.len()
        ));
        let palette_from_children = matches!(source, Source::Project(_));
        let mut doc = Self {
            path: path.to_path_buf(),
            title: name,
            source,
            live: values.clone(),
            values,
            param_index,
            part_index,
            sim,
            motions,
            expressions,
            pose,
            pose_state: None,
            saved: saved_init,
            manual: vec![false; n_params],
            virtuals: HashMap::new(),
            ranges,
            motion_out: MotionOutputs::default(),
            breath: Breath::default(),
            tracking: Tracking::new(model.parameters.iter().map(|p| p.id.as_str())),
            motion_sync,
            motion_sync_frames: HashMap::new(),
            playing_motion: None,
            idle_last: None,
            idle_rng: Rng::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(1, |d| d.as_nanos() as u64),
            ),
            eye_blink: EyeBlink::default(),
            motion_player: MotionPlayer::default(),
            expression_player: ExpressionPlayer::default(),
            looping: true,
            time: 0.0,
            frame: None,
            group_opacity: 1.0,
            view: ViewState {
                center: [0.0, 0.0],
                zoom: 1.0,
                rotation: 0.0,
                mirror: false,
                fit_pending: true,
                fit_box: None,
            },
            selection: SelectionSet::default(),
            hit_areas,
            palette_from_children,
            history: History::new(EditState {
                model: model.clone(),
                selection: SelectionSet::default(),
                edit_level: 2,
            }),
            saved_revision: 0,
            mesh_edit: None,
            external: Vec::new(),
            mesh_prefs: None,
            glue_tool: None,
            glue_prefs: Default::default(),
            lasso: false,
            soft_select: None,
            path_tool: None,
            art_tool: None,
            art_prefs: Default::default(),
            paste_pairing: None,
            anim: None,
            path_prefs: Default::default(),
            temp_path: None,
            soft_prefs: Default::default(),
            form_clipboard: Vec::new(),
            untitled: false,
            drag: None,
            edit_level: 2,
            notice: None,
            prefs: Prefs::default(),
            guide_selected: None,
            instance: {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            },
            param_request: None,
            mask_issues: keyform_edit::mask_issues(&model),
            mode: if palette_from_children {
                Mode::Modeling
            } else {
                Mode::Viewer
            },
            model,
        };
        // Motion-sync settings from before 5.0.01 (motion-sync.md §2).
        if doc.model.motion_sync.iter().any(|s| s.version == 0) {
            doc.edit("Motion-sync formula update", |m| {
                keyform_anim::motionsync::correct_v0(&mut m.motion_sync);
            });
            log.warn(
                "The motion-sync formula has been optimized since Cubism 5.0.01. The blending \
                 ratio value is automatically adjusted to maintain compatibility.",
            );
        }
        Ok(doc)
    }

    /// Model edits are possible only on projects (runtime models cannot be
    /// saved back).
    pub fn editable(&self) -> bool {
        matches!(self.source, Source::Project(_))
    }

    /// Changed since opening or the last save (title asterisk).
    pub fn is_modified(&self) -> bool {
        self.history.revision() != self.saved_revision
            || self.anim.as_ref().is_some_and(|a| a.dirty)
    }

    pub(crate) fn edit_state(&self) -> EditState {
        EditState {
            model: self.model.clone(),
            selection: self.selection.clone(),
            edit_level: self.edit_level,
        }
    }

    /// Applies `f` to the model as one undo step named `name`. A step that
    /// changes nothing is discarded (undo.md §2). Returns whether it changed
    /// anything.
    pub fn edit(&mut self, name: &str, f: impl FnOnce(&mut Model)) -> bool {
        if !self.editable() {
            return false;
        }
        let before = self.model.clone();
        f(&mut self.model);
        let unchanged = self.model.same_data(&before);
        if unchanged {
            // Keep sharing objects with the history.
            self.model = before;
            return false;
        }
        self.history
            .commit(name, StepKind::Real, self.edit_state(), false);
        self.model_changed();
        true
    }

    /// The value the Parameter palette shows for `p`.
    pub fn shown_value(&self, p: usize) -> f32 {
        if self.mode == Mode::Modeling || self.manual.get(p).copied().unwrap_or(false) {
            self.values[p]
        } else {
            self.live.get(p).copied().unwrap_or(0.0)
        }
    }

    /// Runs a key operation on the selected object as one undo step.
    /// Returns a warning to show, or the reason it could not run.
    pub fn key_op(&mut self, op: KeyOp, p: usize) -> Result<Option<String>, String> {
        use keyform_edit::{add_key, delete_all_keys, keys_of, remove_key, KeyError};
        const NO_OBJECT: &str = "Please execute after selecting an object that has keyforms.";
        let Some(o) = self.primary() else {
            return Err(NO_OBJECT.into());
        };
        let Some(param) = self.model.parameters.get(p).cloned() else {
            return Err("No such parameter.".into());
        };
        let v = param.clamp(self.shown_value(p));
        if param.kind == keyform_model::ParameterKind::BlendShape {
            // Blend-shape keys: entries at the max (Add 2/3 keys, as
            // observed) or at the current value; 0 is the implicit base.
            // spec: blend-shapes.md §1, §3
            let (name, key) = match op {
                KeyOp::AddTwo | KeyOp::AddThree => ("Add Blend Shape", Some(param.max)),
                // (Add 3 keys also adds the middle of the range, below.)
                KeyOp::Add => ("Add Blend Shape", Some(v)),
                KeyOp::Delete => ("Delete Blend Shape", Some(v)),
                KeyOp::DeleteAll => ("Delete Blend Shapes", None),
            };
            let adding = matches!(op, KeyOp::Add | KeyOp::AddTwo | KeyOp::AddThree);
            let mut failure = None;
            self.edit(name, |m| {
                if op == KeyOp::AddThree {
                    // Observed: keys at 0, the middle of the range and the max.
                    let mid = (param.min + param.max) / 2.0;
                    let _ = keyform_edit::keys::add_blend_shape(m, o, p, mid);
                }
                if adding {
                    if let Err(e) =
                        keyform_edit::keys::add_blend_shape(m, o, p, key.unwrap_or(param.max))
                    {
                        if !(matches!(op, KeyOp::AddTwo | KeyOp::AddThree)
                            && e == KeyError::TooClose)
                        {
                            failure = Some(match e {
                                KeyError::TooClose => "Cannot set an existing value.".to_owned(),
                                _ => NO_OBJECT.to_owned(),
                            });
                        }
                    }
                } else if !keyform_edit::keys::remove_blend_shape(m, o, p, key) {
                    failure = Some("There is no key at the current value.".to_owned());
                }
            });
            return failure.map_or(Ok(None), Err);
        }
        let keys = keys_of(&self.model, o, p);
        // Keys can't be added or removed while extended interpolation is
        // set on that binding (extended-interpolation.md §5).
        if keyform_edit::form_tools::extended_kind(&self.model, o, p)
            .is_some_and(|k| k != keyform_model::ExtendedInterpolationKind::Linear)
        {
            return Err(
                "Keys cannot be added or removed while extended interpolation is set. Select Linear Interpolation first."
                    .into(),
            );
        }
        let message = |e: KeyError| match e {
            KeyError::RepeatEnds => {
                "Unable to delete maximum and minimum key value when Repeat is active.".to_owned()
            }
            KeyError::TooClose => "Cannot set an existing value.".to_owned(),
            KeyError::NoKey => "There is no key at the current value.".to_owned(),
            KeyError::Missing => NO_OBJECT.to_owned(),
        };
        let values: Vec<f32> = match op {
            KeyOp::Add if keys.contains(&v) => return Err("Cannot set an existing value.".into()),
            KeyOp::Add | KeyOp::Delete | KeyOp::DeleteAll => vec![v],
            KeyOp::AddTwo => vec![param.min, param.max],
            KeyOp::AddThree => vec![param.min, param.default, param.max],
        };
        if matches!(op, KeyOp::Delete | KeyOp::DeleteAll) && keys.is_empty() {
            return Err(NO_OBJECT.into());
        }
        // Binding-count note (palettes.md §3). TODO: the reference asks with
        // OK/Cancel; Keyform proceeds and warns.
        let bindings = keyform_edit::keys::binding_count(&self.model, o);
        let warning = (matches!(op, KeyOp::Add | KeyOp::AddTwo | KeyOp::AddThree)
            && keys.is_empty()
            && bindings >= BINDING_WARN)
            .then(|| {
                format!(
                    "Notes on Parameter Mapping: You are attempting to map {} parameters to the \
                     object. It is recommended to map no more than three parameters to a single object.",
                    bindings + 1
                )
            });
        let name = match op {
            KeyOp::Add | KeyOp::AddTwo | KeyOp::AddThree => "Add Key",
            KeyOp::Delete => "Delete Key",
            KeyOp::DeleteAll => "Delete All Keys",
        };
        let mut failure = None;
        self.edit(name, |m| {
            for x in values {
                let r = match op {
                    KeyOp::Delete => remove_key(m, o, p, x),
                    KeyOp::DeleteAll => delete_all_keys(m, o, p, x),
                    _ => add_key(m, o, p, x),
                };
                if let Err(e) = r {
                    // "Add 2/3 keys" skip keys that already exist.
                    if !(matches!(op, KeyOp::AddTwo | KeyOp::AddThree) && e == KeyError::TooClose) {
                        failure = Some(message(e));
                    }
                }
            }
        });
        match failure {
            Some(f) => Err(f),
            None => Ok(warning),
        }
    }

    /// The top-most visible, unlocked art mesh under canvas point `q`
    /// (canvas-and-tools.md §3).
    /// The selected object under canvas point `q`, if any: selected objects
    /// take priority over unselected ones on top, so a drag inside a
    /// selected deformer moves it rather than grabbing a mesh above it.
    /// Meshes by their triangles, warp deformers by their grid's bounds,
    /// rotation deformers near their origin (`radius`).
    pub fn selected_hit(&self, q: [f32; 2], radius: f32) -> Option<Selection> {
        for &o in self.selection.objects.iter().rev() {
            match o {
                Selection::ArtMesh(_) => {
                    if self.hit_test_mesh(o, q) {
                        return Some(o);
                    }
                }
                Selection::Deformer(d) => {
                    let pts = self.object_points(o);
                    let rotation = matches!(
                        self.model.deformers.get(d).map(|x| &x.kind),
                        Some(keyform_model::DeformerKind::Rotation(_))
                    );
                    if rotation {
                        if pts
                            .first()
                            .is_some_and(|p| (p[0] - q[0]).hypot(p[1] - q[1]) <= radius * 3.0)
                        {
                            return Some(o);
                        }
                    } else if pts.len() >= 2 {
                        let (mut x0, mut y0, mut x1, mut y1) =
                            (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                        for p in &pts {
                            x0 = x0.min(p[0]);
                            y0 = y0.min(p[1]);
                            x1 = x1.max(p[0]);
                            y1 = y1.max(p[1]);
                        }
                        if (x0..=x1).contains(&q[0]) && (y0..=y1).contains(&q[1]) {
                            return Some(o);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Whether `q` is inside art mesh `o`'s triangles.
    fn hit_test_mesh(&self, o: Selection, q: [f32; 2]) -> bool {
        let (Selection::ArtMesh(mi), Some(frame)) = (o, self.frame.as_ref()) else {
            return false;
        };
        let (Some(st), Some(mesh)) = (frame.meshes.get(mi), self.model.art_meshes.get(mi)) else {
            return false;
        };
        let side = |a: [f32; 2], b: [f32; 2]| {
            (q[0] - b[0]) * (a[1] - b[1]) - (a[0] - b[0]) * (q[1] - b[1])
        };
        let pt = |i: u32| {
            let i = i as usize;
            [
                st.positions.get(2 * i).copied().unwrap_or(0.0),
                st.positions.get(2 * i + 1).copied().unwrap_or(0.0),
            ]
        };
        mesh.indices.chunks_exact(3).any(|t| {
            let (a, b, c) = (pt(t[0]), pt(t[1]), pt(t[2]));
            let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
            !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
        })
    }

    pub fn hit_test(&self, q: [f32; 2]) -> Option<Selection> {
        let frame = self.frame.as_ref()?;
        // Which side of edge b→a the point q is on.
        let side = |a: [f32; 2], b: [f32; 2]| {
            (q[0] - b[0]) * (a[1] - b[1]) - (a[0] - b[0]) * (q[1] - b[1])
        };
        let inside = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
            let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
            let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
            let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
            !(neg && pos)
        };
        for &mi in frame.render_order.iter().rev() {
            let (Some(st), Some(mesh)) = (frame.meshes.get(mi), self.model.art_meshes.get(mi))
            else {
                continue;
            };
            if st.hidden || st.outside || st.opacity <= 0.0 || mesh.info.locked {
                continue;
            }
            let pt = |i: u32| {
                let i = i as usize;
                [
                    st.positions.get(2 * i).copied().unwrap_or(0.0),
                    st.positions.get(2 * i + 1).copied().unwrap_or(0.0),
                ]
            };
            let hit = mesh
                .indices
                .chunks_exact(3)
                .any(|t| inside(pt(t[0]), pt(t[1]), pt(t[2])));
            if hit {
                return Some(Selection::ArtMesh(mi));
            }
        }
        None
    }

    /// Starts dragging the selection in the current keyforms: the selected
    /// points (`points`), or the selected objects whole. An object whose
    /// parent deformer also moves is carried by it and not moved itself.
    pub fn drag_begin(&mut self, points: bool) -> Result<(), String> {
        if !self.editable() || self.mode != Mode::Modeling {
            return Err("Editing is available in the Modeling workspace of a project.".into());
        }
        let objects = self.selection.objects.clone();
        let moving: Vec<Selection> = if points {
            let mut v: Vec<Selection> = self.selection.points.iter().map(|p| p.0).collect();
            v.dedup();
            v
        } else {
            objects
        };
        let carried = |o: Selection| {
            let mut d = keyform_edit::parent_deformer(&self.model, o);
            let mut guard = 0;
            while let Some(p) = d {
                if !points && moving.contains(&Selection::Deformer(p)) {
                    return true;
                }
                d = self.model.deformers.get(p).and_then(|x| x.target);
                guard += 1;
                if guard > self.model.deformers.len() {
                    break;
                }
            }
            false
        };
        let mut targets = Vec::new();
        for &o in &moving {
            if carried(o) {
                continue;
            }
            let cell = keyform_edit::current_cell(&self.model, o, &self.values)
                .map_err(|e| e.to_string())?;
            let parent = keyform_edit::parent_deformer(&self.model, o).and_then(|d| {
                self.frame
                    .as_ref()
                    .and_then(|f| f.deformers.get(d))
                    .and_then(|s| s.map.clone())
            });
            let which = points.then(|| {
                self.selection
                    .points
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.0 == o)
                    .map(|(k, p)| (p.1, self.selection.weight(k)))
                    .collect()
            });
            let net_level = (points && self.uses_net(o)).then_some(self.edit_level);
            targets.push(MoveTarget {
                object: o,
                cell,
                parent,
                points: which,
                net_level,
                blend: keyform_edit::keys::blend_target(&self.model, o, &self.values).map(|t| t.1),
            });
        }
        if targets.is_empty() {
            return Ok(());
        }
        self.drag = Some(Drag {
            base: self.model.clone(),
            kind: DragKind::Move(targets),
        });
        Ok(())
    }

    /// A rotation deformer's widget in canvas space: origin, handle tip and
    /// circle radius. spec: canvas-and-tools.md §3
    pub fn rotation_widget(&self, o: Selection) -> Option<([f32; 2], [f32; 2], f32)> {
        let Selection::Deformer(d) = o else {
            return None;
        };
        let DeformerKind::Rotation(r) = &self.model.deformers.get(d)?.kind else {
            return None;
        };
        let Some(keyform_eval::DeformerMap::Rotation(map)) =
            self.frame.as_ref()?.deformers.get(d)?.map.clone()
        else {
            return None;
        };
        let (s, c) = map.angle.to_radians().sin_cos();
        let o = map.origin;
        Some((
            o,
            [o[0] + s * r.handle_length, o[1] - c * r.handle_length],
            r.circle_radius,
        ))
    }

    /// Rotation deformers drawn as a needle (origin and handle tip) while
    /// not selected: every visible, unlocked one in the Modeling workspace,
    /// as the reference shows them (observed for Q-0048: at every edit level,
    /// whatever the Show Bone flag or the Parts palette filter; not in the
    /// Animation workspace or image exports).
    /// TODO(Q-0048): observed in the black box; canvas-and-tools.md doesn't
    /// describe it yet.
    pub fn idle_rotation_gizmos(&self) -> Vec<([f32; 2], [f32; 2])> {
        if self.mode != Mode::Modeling {
            return Vec::new();
        }
        (0..self.model.deformers.len())
            .filter(|&d| {
                let o = Selection::Deformer(d);
                let info = &self.model.deformers[d].info;
                info.visible && !info.locked && !self.selection.objects.contains(&o)
            })
            .filter_map(|d| {
                self.rotation_widget(Selection::Deformer(d))
                    .map(|(o, t, _)| (o, t))
            })
            .collect()
    }

    /// Starts rotating or scaling a rotation deformer from canvas point `at`.
    pub fn drag_begin_handle(
        &mut self,
        o: Selection,
        handle: Handle,
        at: [f32; 2],
    ) -> Result<(), String> {
        if !self.editable() || self.mode != Mode::Modeling {
            return Err("Editing is available in the Modeling workspace of a project.".into());
        }
        let (centre, _, _) = self.rotation_widget(o).ok_or("not a rotation deformer")?;
        let cell =
            keyform_edit::current_cell(&self.model, o, &self.values).map_err(|e| e.to_string())?;
        let kind = match handle {
            Handle::Rotate => DragKind::Rotate {
                target: o,
                cell,
                centre,
                start: at,
            },
            Handle::Scale => DragKind::Scale {
                target: o,
                cell,
                centre,
                start: at,
            },
        };
        self.drag = Some(Drag {
            base: self.model.clone(),
            kind,
        });
        Ok(())
    }

    /// Shows the drag at `total` canvas pixels from its start; `shift`
    /// snaps rotations.
    pub fn drag_update(&mut self, total: [f32; 2], shift: bool) {
        let Some(d) = &self.drag else { return };
        let mut m = d.base.clone();
        let r = match &d.kind {
            DragKind::Move(targets) => targets.iter().try_for_each(|t| {
                // A blend-shape key: edit its entry (swapped into the cell).
                if let Some(e) = t.blend {
                    keyform_edit::keys::swap_blend_form(&mut m, t.object, t.cell, e);
                }
                let r = match (t.net_level, t.points.as_deref()) {
                    (Some(level), Some(which)) => {
                        let idx: Vec<usize> = which.iter().map(|w| w.0).collect();
                        keyform_edit::translate_net(
                            &mut m,
                            t.object,
                            t.cell,
                            t.parent.as_ref(),
                            total,
                            &idx,
                            level,
                        )
                    }
                    _ => keyform_edit::forms::translate_weighted(
                        &mut m,
                        t.object,
                        t.cell,
                        t.parent.as_ref(),
                        total,
                        t.points.as_deref(),
                    ),
                }
                .map(|_| ());
                if let Some(e) = t.blend {
                    keyform_edit::keys::swap_blend_form(&mut m, t.object, t.cell, e);
                }
                r
            }),
            DragKind::Rotate {
                target,
                cell,
                centre,
                start,
            } => {
                let cur = [start[0] + total[0], start[1] + total[1]];
                let ang = |p: [f32; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
                let mut delta = (ang(cur) - ang(*start)).to_degrees();
                delta = (delta + 180.0).rem_euclid(360.0) - 180.0;
                keyform_edit::rotate(
                    &mut m,
                    *target,
                    *cell,
                    delta,
                    shift.then_some(self.prefs.rotation_step),
                )
                .map(|_| ())
            }
            DragKind::Scale {
                target,
                cell,
                centre,
                start,
            } => {
                let cur = [start[0] + total[0], start[1] + total[1]];
                let dist = |p: [f32; 2]| (p[0] - centre[0]).hypot(p[1] - centre[1]);
                let d0 = dist(*start);
                if d0 <= 1e-3 {
                    return;
                }
                keyform_edit::scale(&mut m, *target, *cell, dist(cur) / d0).map(|_| ())
            }
        };
        match r {
            Ok(()) => {
                self.model = m;
                self.model_changed();
            }
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    /// Ends a drag: one undo step "Move" if anything moved.
    pub fn drag_end(&mut self) {
        let Some(d) = self.drag.take() else { return };
        if self.model.same_data(&d.base) {
            self.model = d.base;
            self.model_changed();
            return;
        }
        let name = match d.kind {
            DragKind::Move(_) => "Move",
            DragKind::Rotate { .. } => "Rotate",
            DragKind::Scale { .. } => "Scale",
        };
        self.history
            .commit(name, StepKind::Real, self.edit_state(), false);
        self.mask_issues = keyform_edit::mask_issues(&self.model);
    }

    /// Exports the runtime file set (moc3, textures, physics3, cdi3,
    /// model3) into `dir`. Returns the files written and warnings.
    pub fn export_runtime(&self, dir: &Path) -> Result<(Vec<PathBuf>, Vec<String>), String> {
        use keyform_formats::moc3::ExportOptions;
        use keyform_formats::runtime_json::{export_runtime, RuntimeExportOptions};
        let name = self
            .path
            .file_stem()
            .map_or("model".into(), |s| s.to_string_lossy().into_owned());
        let ro = RuntimeExportOptions {
            moc3: ExportOptions::for_model(&self.model),
            export_physics: true,
            export_motion_sync: true,
            extend_edge_color: self.model.export.extend_edge_color,
            export_display_info: true,
            export_user_data: true,
        };
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let (files, rep) =
            export_runtime(&self.model, &ro, dir, &name).map_err(|e| e.to_string())?;
        Ok((files, rep.warnings))
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Like [`Self::edit`], for operations that can fail: the model only
    /// changes (one undo step) if `f` succeeds.
    pub fn try_edit<T>(
        &mut self,
        name: &str,
        f: impl FnOnce(&mut Model) -> Result<T, String>,
    ) -> Result<T, String> {
        if !self.editable() {
            return Err("Only projects can be edited.".into());
        }
        let mut m = self.model.clone();
        let r = f(&mut m)?;
        if !m.same_data(&self.model) {
            // Objects keep their selection across the edit (matched by GUID:
            // structural edits renumber them); deleted ones drop out.
            self.selection = remap_selection(&self.model, &m, &self.selection);
            self.model = m;
            self.history
                .commit(name, StepKind::Real, self.edit_state(), false);
            self.model_changed();
        }
        Ok(r)
    }

    /// Edit ▸ Delete: deletes the selected deformers (their children move
    /// to the parent), empty parts, art meshes (with the glue joining them)
    /// and glue. Returns a note about glue removed with its meshes.
    pub fn delete_selection(&mut self) -> Result<Option<String>, String> {
        #[derive(PartialEq)]
        enum K {
            Deformer,
            Part,
            Mesh,
            Glue,
        }
        let m = &self.model;
        let mut guids = Vec::new();
        for &o in &self.selection.objects {
            match o {
                Selection::Deformer(d) => {
                    guids.push((K::Deformer, m.deformers[d].info.guid.clone()))
                }
                Selection::Part(p) => guids.push((K::Part, m.parts[p].info.guid.clone())),
                Selection::ArtMesh(a) => guids.push((K::Mesh, m.art_meshes[a].info.guid.clone())),
                Selection::Glue(g) => guids.push((K::Glue, m.glues[g].info.guid.clone())),
            }
        }
        if guids.is_empty() {
            return Ok(None);
        }
        let values = self.values.clone();
        self.try_edit("Delete", |m| {
            let mut glues = Vec::new();
            // Glue and meshes first: deformers and parts may then be empty.
            for (k, g) in &guids {
                match k {
                    K::Glue => {
                        if let Some(i) = m.glues.iter().position(|x| x.info.guid == *g) {
                            keyform_edit::objects::delete_glue(m, i)?;
                        }
                    }
                    K::Mesh => {
                        if let Some(i) = m.art_meshes.iter().position(|x| x.info.guid == *g) {
                            glues.extend(keyform_edit::objects::delete_art_mesh(m, i)?);
                        }
                    }
                    _ => {}
                }
            }
            for (k, g) in &guids {
                match k {
                    K::Deformer => {
                        if let Some(d) = m.deformers.iter().position(|x| x.info.guid == *g) {
                            keyform_edit::objects::delete_deformer(m, d, &values)?;
                        }
                    }
                    K::Part => {
                        if let Some(p) = m.parts.iter().position(|x| x.info.guid == *g) {
                            keyform_edit::objects::delete_part(m, p)?;
                        }
                    }
                    _ => {}
                }
            }
            Ok((!glues.is_empty())
                .then(|| format!("Glue removed with its ArtMesh: {}", glues.join(", "))))
        })
    }

    /// Whether Edit ▸ Copy / Cut must first ask "Do you want to copy part's
    /// child objects?" (object-clipboard.md §1).
    pub fn copy_needs_children_prompt(&self) -> bool {
        keyform_edit::clipboard::needs_children_prompt(&self.model, &self.selection.objects)
    }

    /// Edit ▸ Copy (object-clipboard.md §1): the selection, with the
    /// descendants of selected parts when `with_children`.
    pub fn copy_objects(
        &self,
        with_children: bool,
    ) -> Option<keyform_edit::clipboard::ObjectClipboard> {
        if !self.editable() || self.mode != Mode::Modeling {
            return None;
        }
        keyform_edit::clipboard::copy_objects(
            &self.model,
            &self.selection.objects,
            self.instance,
            with_children,
        )
    }

    /// Edit ▸ Cut: Copy, then delete the originals, as one undo step "Cut";
    /// nothing is selected afterwards.
    pub fn cut_objects(
        &mut self,
        with_children: bool,
    ) -> Result<Option<keyform_edit::clipboard::ObjectClipboard>, String> {
        let Some(clip) = self.copy_objects(with_children) else {
            return Ok(None);
        };
        let guids: Vec<(Selection, String)> = keyform_edit::clipboard::palette_order(&self.model)
            .into_iter()
            .filter(|o| {
                clip.objects.iter().any(|c| match (&c.object, o) {
                    (keyform_edit::clipboard::CopiedObject::Part(p), Selection::Part(i)) => {
                        self.model.parts[*i].info.guid == p.info.guid
                    }
                    (
                        keyform_edit::clipboard::CopiedObject::Deformer(d),
                        Selection::Deformer(i),
                    ) => self.model.deformers[*i].info.guid == d.info.guid,
                    (
                        keyform_edit::clipboard::CopiedObject::ArtMesh { mesh, .. },
                        Selection::ArtMesh(i),
                    ) => self.model.art_meshes[*i].info.guid == mesh.info.guid,
                    (
                        keyform_edit::clipboard::CopiedObject::Glue { glue, .. },
                        Selection::Glue(i),
                    ) => self.model.glues[*i].info.guid == glue.info.guid,
                    _ => false,
                })
            })
            .filter_map(|o| Some((o, crate::palettes::info(&self.model, o)?.guid.clone())))
            .collect();
        let values = self.values.clone();
        self.try_edit("Cut", |m| {
            // Glue and meshes, then deformers, then parts from the deepest.
            for (o, g) in &guids {
                match o {
                    Selection::Glue(_) => {
                        if let Some(i) = m.glues.iter().position(|x| x.info.guid == *g) {
                            keyform_edit::objects::delete_glue(m, i)?;
                        }
                    }
                    Selection::ArtMesh(_) => {
                        if let Some(i) = m.art_meshes.iter().position(|x| x.info.guid == *g) {
                            keyform_edit::objects::delete_art_mesh(m, i)?;
                        }
                    }
                    _ => {}
                }
            }
            for (o, g) in &guids {
                if let Selection::Deformer(_) = o {
                    if let Some(d) = m.deformers.iter().position(|x| x.info.guid == *g) {
                        keyform_edit::objects::delete_deformer(m, d, &values)?;
                    }
                }
            }
            for (o, g) in guids.iter().rev() {
                if let Selection::Part(_) = o {
                    if let Some(p) = m.parts.iter().position(|x| x.info.guid == *g) {
                        keyform_edit::objects::delete_part(m, p)?;
                    }
                }
            }
            Ok(())
        })?;
        self.select(None);
        Ok(Some(clip))
    }

    /// Edit ▸ Paste (object-clipboard.md §4): one undo step "Paste" that
    /// also selects the pasted objects. Returns how many were pasted and
    /// the objects that couldn't be.
    pub fn paste_objects(
        &mut self,
        clip: &keyform_edit::clipboard::ObjectClipboard,
    ) -> Result<(usize, Vec<String>), String> {
        if self.mode != Mode::Modeling {
            return Err("Please select Modeling View".into());
        }
        if !self.editable() {
            return Err("Only projects can be edited.".into());
        }
        let mut m = self.model.clone();
        let r = keyform_edit::clipboard::paste_objects(
            &mut m,
            clip,
            &self.selection.objects,
            self.instance,
            self.prefs.paste_numbered_names,
        )?;
        let n = r.objects.len();
        if n > 0 {
            let mut set = SelectionSet::of(None);
            set.objects = r.objects;
            self.selection = set;
            self.model = m;
            self.history
                .commit("Paste", StepKind::Real, self.edit_state(), false);
            self.model_changed();
        }
        Ok((n, r.warnings))
    }

    /// Edit ▸ Copy Parameter Value: every parameter's current value.
    pub fn copy_parameter_values(&self) -> Vec<(String, f32)> {
        keyform_edit::clipboard::copy_parameter_values(&self.model, &self.values)
    }

    /// Edit ▸ Paste Parameter Value: sets every copied parameter (by ID)
    /// as if by hand. Returns how many were set.
    pub fn paste_parameter_values(&mut self, copied: &[(String, f32)]) -> usize {
        let mut values = self.values.clone();
        let n = keyform_edit::clipboard::paste_parameter_values(&self.model, &mut values, copied);
        for (p, v) in values.into_iter().enumerate() {
            if copied
                .iter()
                .any(|(id, _)| self.model.parameters.get(p).is_some_and(|x| x.id == *id))
            {
                self.set_manual(p, v);
            }
        }
        n
    }

    /// Generate and Register Glue: binds the one selected vertex on each of
    /// two art meshes (spec: 30-algorithms/glue.md "Editing").
    pub fn glue_selected_vertices(&mut self) -> Result<(), String> {
        let pts: Vec<(usize, usize)> = self
            .selection
            .points
            .iter()
            .filter_map(|&(o, k)| match o {
                Selection::ArtMesh(a) => Some((a, k)),
                _ => None,
            })
            .collect();
        let [p, q] = pts[..] else {
            return Err(keyform_edit::objects::GLUE_SELECT_TWO.into());
        };
        if p.0 == q.0 {
            return Err(keyform_edit::objects::GLUE_SELECT_TWO.into());
        }
        let sel = self.selection.clone();
        let g = self.try_edit("Glue", |m| keyform_edit::objects::bind_vertices(m, p, q))?;
        let mut sel = sel;
        sel.objects.retain(|o| !matches!(o, Selection::Glue(_)));
        sel.objects.push(Selection::Glue(g));
        self.set_selection(sel);
        Ok(())
    }

    /// Project palette ▸ layer ▸ Create ArtMesh: an art mesh for the layer,
    /// in the selected part (else the root part), then selected.
    /// spec: 50-ui/palettes.md §6
    pub fn create_art_mesh_from_layer(&mut self, image: usize, layer: &str) -> Result<(), String> {
        let part = match self.primary() {
            Some(Selection::Part(p)) => Some(p),
            _ => None,
        };
        let a = self.try_edit("Create ArtMesh", |m| {
            keyform_formats::psd_import::create_art_mesh_from_layer(
                m,
                image,
                layer,
                part,
                &Default::default(),
            )
            .map_err(|e| e.to_string())
        })?;
        self.select(Some(Selection::ArtMesh(a)));
        Ok(())
    }

    /// Edit Form ▸ Copy Form: the selected objects' forms (selected
    /// points weigh 1, others 0, when points are selected).
    /// spec: form-tools.md §1
    pub fn copy_form(&mut self) -> Result<usize, String> {
        let Some(frame) = self.frame.as_ref() else {
            return Err("The model is not evaluated yet.".into());
        };
        let mut out = Vec::new();
        for &o in &self.selection.objects {
            let pts: Vec<usize> = self
                .selection
                .points
                .iter()
                .filter(|(x, _)| *x == o)
                .map(|(_, k)| *k)
                .collect();
            let weights: Option<Vec<f32>> = (!self.selection.points.is_empty()).then(|| {
                // Points past the end weigh 0 (copy_form).
                let n = pts.iter().max().map_or(0, |m| m + 1);
                (0..n)
                    .map(|k| if pts.contains(&k) { 1.0 } else { 0.0 })
                    .collect()
            });
            match keyform_edit::form_tools::copy_form(
                &self.model,
                frame,
                &self.values,
                o,
                weights.as_deref(),
            ) {
                Some(c) => out.push(c),
                None => {
                    if let Some(i) = crate::palettes::info(&self.model, o) {
                        self.notice =
                            Some(format!("{}: this object's form cannot be copied.", i.id));
                    }
                }
            }
        }
        if out.is_empty() {
            return Err("Select the objects whose forms to copy.".into());
        }
        let n = out.len();
        self.form_clipboard = out;
        Ok(n)
    }

    /// Paste Form (w = 1) / Blend Form (w = rate) into the selected objects
    /// (or the copied objects when nothing is selected). One undo step.
    pub fn paste_form(
        &mut self,
        name: &str,
        w: f32,
        mirror: keyform_edit::form_tools::PasteMirror,
    ) -> Result<usize, String> {
        if self.form_clipboard.is_empty() {
            return Err("There is no copied form.".into());
        }
        let targets: Vec<Selection> = if self.selection.objects.is_empty() {
            self.form_clipboard.iter().map(|c| c.object).collect()
        } else {
            self.selection.objects.clone()
        };
        use keyform_edit::form_tools as ft;
        let copies = self.form_clipboard.clone();
        if ft::pairing_ambiguous(&self.model, &copies, &targets) {
            self.paste_pairing = Some(PastePairing {
                name: name.to_owned(),
                w,
                mirror,
                candidates: ft::paste_candidates(&self.model, &copies, &targets),
                pairs: ft::default_pairing(&self.model, &copies, &targets),
            });
            return Ok(0);
        }
        let pairs = ft::default_pairing(&self.model, &copies, &targets);
        self.paste_paired(name, w, mirror, &pairs)
    }

    /// Paste / Blend with a chosen copy for each target. One undo step.
    pub fn paste_paired(
        &mut self,
        name: &str,
        w: f32,
        mirror: keyform_edit::form_tools::PasteMirror,
        pairs: &[(Selection, Option<usize>)],
    ) -> Result<usize, String> {
        let copies = self.form_clipboard.clone();
        let values = self.values.clone();
        let sel = self.selection.clone();
        let n = self.try_edit(name, |m| {
            keyform_edit::form_tools::paste_form_paired(m, &values, &copies, pairs, w, mirror)
                .map_err(|e| e.to_string())
        })?;
        self.set_selection(sel);
        if n == 0 {
            return Err("The copied forms don't fit the selected objects.".into());
        }
        Ok(n)
    }

    /// Unbind Glue Vertices [Ctrl+Shift+G] in the Modeling view: removes
    /// the pair of the one selected vertex on each of two art meshes.
    /// spec: 30-algorithms/glue.md "Bind / unbind"
    pub fn unglue_selected_vertices(&mut self) -> Result<(), String> {
        let pts: Vec<(usize, usize)> = self
            .selection
            .points
            .iter()
            .filter_map(|&(o, k)| match o {
                Selection::ArtMesh(a) => Some((a, k)),
                _ => None,
            })
            .collect();
        let [p, q] = pts[..] else {
            return Err(keyform_edit::objects::GLUE_SELECT_TWO.into());
        };
        if p.0 == q.0 {
            return Err(keyform_edit::objects::GLUE_SELECT_TWO.into());
        }
        let sel = self.selection.clone();
        let removed = self.try_edit("Unbind Glue", |m| {
            keyform_edit::objects::unbind_vertices(m, p, q)
        })?;
        self.set_selection(sel);
        if removed {
            Ok(())
        } else {
            Err("The selected vertices are not glued.".into())
        }
    }

    /// Start Mesh editing on the selected art mesh (canvas-and-tools.md §5).
    pub fn start_mesh_edit(&mut self) -> Result<(), String> {
        if !self.editable() || self.mode != Mode::Modeling {
            return Err("Mesh editing needs a model project in the Modeling workspace.".into());
        }
        if self.mesh_edit.is_some() || self.drag.is_some() {
            return Ok(());
        }
        let meshes: Vec<usize> = self
            .selection
            .objects
            .iter()
            .filter_map(|o| match o {
                Selection::ArtMesh(a) => Some(*a),
                _ => None,
            })
            .collect();
        // One art mesh, or two (gluing between them; glue.md "Bind /
        // unbind"). The primary selection is edited; the other is the
        // partner.
        let (a, b) = match meshes[..] {
            [a] => (a, None),
            [x, y] => match self.primary() {
                Some(Selection::ArtMesh(p)) if p == y => (y, Some(x)),
                _ => (x, Some(y)),
            },
            _ => return Err("Select one ArtMesh (or two, to glue them) to edit its mesh.".into()),
        };
        let mut s = self.new_mesh_edit(a)?;
        if let Some(b) = b {
            s.partner = Some(Box::new(self.new_mesh_edit(b)?));
        }
        self.mesh_edit = Some(s);
        Ok(())
    }

    /// A mesh-edit session on art mesh `a` with the remembered settings,
    /// the model image's rectangle (Reset Mesh) and its glue state.
    fn new_mesh_edit(&mut self, a: usize) -> Result<crate::mesh_edit::MeshEdit, String> {
        let m = &self.model;
        let canvas = [m.canvas.width as f32, m.canvas.height as f32];
        let prefs = *self
            .mesh_prefs
            .get_or_insert_with(|| crate::mesh_edit::MeshPrefs::new(canvas));
        let mut s = crate::mesh_edit::MeshEdit::start(a, &m.art_meshes[a], prefs)?;
        s.glued = keyform_edit::automesh::is_glued(m, a);
        // spec: canvas-and-tools.md §5.1 "Reset Mesh": the model image's
        // (0, 0)–(width, height) mapped to the canvas, margin 0.
        s.model_rect = m.art_meshes[a]
            .model_image
            .and_then(|k| m.model_images.get(k))
            .and_then(|mi| {
                let (w, h) = keyform_formats::image::png_size(mi.png.as_ref()?)?;
                let t = mi.local_to_canvas;
                let map = |x: f32, y: f32| [t[0] * x + t[1] * y + t[2], t[3] * x + t[4] * y + t[5]];
                let (w, h) = (w as f32, h as f32);
                Some([map(0.0, 0.0), map(w, 0.0), map(w, h), map(0.0, h)])
            });
        Ok(s)
    }

    /// Finish Mesh editing: the mesh is re-triangulated and its keyforms
    /// remapped as one undo step. Returns a warning to show, if any.
    pub fn finish_mesh_edit(&mut self) -> Result<Option<String>, String> {
        let Some(s) = self.mesh_edit.take() else {
            return Ok(None);
        };
        self.mesh_prefs = Some(s.prefs);
        if !s.changed() {
            return Ok(None);
        }
        let a = s.mesh;
        // Finishing merges exactly coincident vertices (§5.1).
        let (graph, merged) = s.finished_graph();
        let generator = s.generator;
        let changed_self = s.graph_changed();
        let partner = s
            .partner
            .as_ref()
            .map(|p| (p.mesh, p.finished_graph().0, p.graph_changed()));
        let (glue_add, glue_remove) = (s.glue_add.clone(), s.glue_remove.clone());
        let values = self.values.clone();
        let t = self.try_edit("Mesh Edit", |m| {
            // Deform paths on (or driving) the mesh are re-anchored and
            // rebound (deform-paths.md §1 "Topology observers").
            let t = if changed_self || generator.is_some() || partner.is_none() {
                keyform_edit::deform_path::set_mesh_keep_paths(m, &values, a, &graph)
                    .map_err(|e| e.to_string())?
            } else {
                Default::default()
            };
            if let Some(g) = generator {
                std::sync::Arc::make_mut(&mut m.art_meshes[a]).generator = g;
            }
            if let Some((b, pg, pchanged)) = &partner {
                if *pchanged {
                    keyform_edit::deform_path::set_mesh_keep_paths(m, &values, *b, pg)
                        .map_err(|e| e.to_string())?;
                }
                // Glue pairs by vertex UID (glue.md "Bind / unbind").
                let idx = |m: &keyform_model::Model, x: usize, uid: i64| {
                    m.art_meshes[x].point_uids.iter().position(|&u| u == uid)
                };
                for &(ua, ub) in &glue_add {
                    let (Some(i), Some(j)) = (idx(m, a, ua), idx(m, *b, ub)) else {
                        continue;
                    };
                    match keyform_edit::objects::bind_vertices(m, (a, i), (*b, j)) {
                        Ok(_) => {}
                        Err(e) if e == keyform_edit::objects::GLUE_SAME_POINTS => {}
                        Err(e) => return Err(e),
                    }
                }
                for &(ua, ub) in &glue_remove {
                    let (Some(i), Some(j)) = (idx(m, a, ua), idx(m, *b, ub)) else {
                        continue;
                    };
                    keyform_edit::objects::unbind_vertices(m, (a, i), (*b, j))?;
                }
            }
            Ok(t)
        });
        let t = match t {
            Ok(t) => t,
            Err(e) => {
                // Keep editing so nothing is lost.
                let mut s = self.new_mesh_edit(a)?;
                s.change(|g, _| *g = graph);
                self.mesh_edit = Some(s);
                return Err(e);
            }
        };
        self.select(Some(Selection::ArtMesh(a)));
        let mut notes: Vec<String> = merged.map(str::to_owned).into_iter().collect();
        if t.unresolved > 0 {
            notes.push(format!(
                "{} crossing edges could not be kept in the mesh.",
                t.unresolved
            ));
        }
        Ok((!notes.is_empty()).then(|| notes.join(" ")))
    }

    /// The primary (last selected) object.
    pub fn primary(&self) -> Option<Selection> {
        self.selection.objects.last().copied()
    }

    /// Selects one object (or nothing) as a selection-only undo step
    /// (undo.md §3).
    pub fn select(&mut self, sel: Option<Selection>) {
        self.set_selection(SelectionSet::of(sel));
    }

    /// Ctrl / Shift + click on palette rows: each object is added to or
    /// removed from the selection (with its points). Applied after a plain
    /// click's selection, so the toggles aren't overwritten.
    pub fn toggle_objects(&mut self, toggles: &[Selection]) {
        if toggles.is_empty() {
            return;
        }
        let mut set = self.selection.clone();
        for &o in toggles {
            if let Some(i) = set.objects.iter().position(|x| *x == o) {
                set.objects.remove(i);
                set.retain_points(|p| p.0 != o);
            } else {
                set.objects.push(o);
            }
        }
        self.set_selection(set);
    }

    /// Edit ▸ Select All: every point of the selected objects (vertices,
    /// warp grid / net points, rotation origin); with nothing selected, every
    /// visible, unlocked art mesh.
    pub fn select_all(&mut self) {
        let objects = self.selection.objects.clone();
        if objects.is_empty() {
            let all = self
                .model
                .art_meshes
                .iter()
                .enumerate()
                .filter(|(_, a)| a.info.visible && !a.info.locked)
                .map(|(i, _)| Selection::ArtMesh(i))
                .collect();
            self.set_selection(SelectionSet {
                objects: all,
                points: Vec::new(),
                point_weights: Vec::new(),
            });
            return;
        }
        let mut pts = Vec::new();
        for o in &objects {
            for k in 0..self.object_points(*o).len() {
                pts.push((*o, k));
            }
        }
        self.set_selection(SelectionSet {
            objects,
            points: pts,
            point_weights: Vec::new(),
        });
    }

    /// Replaces the selection as a selection-only undo step.
    pub fn set_selection(&mut self, set: SelectionSet) {
        if set == self.selection {
            return;
        }
        self.selection = set;
        self.history
            .commit("Select", StepKind::SelectionOnly, self.edit_state(), false);
    }

    /// Sets the edit level (recorded in undo history, main-window.md §1).
    pub fn set_edit_level(&mut self, level: u8) {
        let level = level.clamp(1, 3);
        if level == self.edit_level {
            return;
        }
        self.edit_level = level;
        // Point indices mean different things per level.
        self.selection.set_points(Vec::new());
        self.history.commit(
            "Edit Level",
            StepKind::SelectionOnly,
            self.edit_state(),
            false,
        );
    }

    /// Whether `o` is edited through its Bezier net at the current level.
    pub fn uses_net(&self, o: Selection) -> bool {
        matches!(o, Selection::Deformer(d)
            if self.edit_level >= 2
                && matches!(self.model.deformers.get(d).map(|x| &x.kind), Some(DeformerKind::Warp(_))))
    }

    /// The Bezier net of warp `d` at the current level, in canvas space
    /// (built from the deformed grid, for display and picking).
    pub fn canvas_net(&self, d: usize) -> Option<keyform_edit::warp_net::Net> {
        use keyform_edit::warp_net::{Grid, Net};
        let DeformerKind::Warp(w) = &self.model.deformers.get(d)?.kind else {
            return None;
        };
        let Some(keyform_eval::DeformerMap::Warp(map)) =
            self.frame.as_ref()?.deformers.get(d)?.map.as_ref()
        else {
            return None;
        };
        let (nc, nr) = w.net_division(self.edit_level);
        let g = Grid {
            cols: map.cols,
            rows: map.rows,
            points: &map.points,
        };
        Some(Net::from_grid(&g, nc as usize, nr as usize))
    }

    /// Canvas positions of an object's editable points: mesh vertices, warp
    /// grid points (level 1) or Bezier net points (levels 2–3), the
    /// rotation origin.
    pub fn object_points(&self, o: Selection) -> Vec<[f32; 2]> {
        if let (true, Selection::Deformer(d)) = (self.uses_net(o), o) {
            return self
                .canvas_net(d)
                .map(|n| {
                    n.points()
                        .into_iter()
                        .map(|(k, dir)| n.point(k, dir))
                        .collect()
                })
                .unwrap_or_default();
        }
        let Some(f) = self.frame.as_ref() else {
            return Vec::new();
        };
        match o {
            Selection::ArtMesh(i) => f
                .meshes
                .get(i)
                .map(|m| m.positions.chunks_exact(2).map(|p| [p[0], p[1]]).collect())
                .unwrap_or_default(),
            Selection::Deformer(d) => match f.deformers.get(d).and_then(|s| s.map.as_ref()) {
                Some(keyform_eval::DeformerMap::Warp(w)) => {
                    w.points.chunks_exact(2).map(|p| [p[0], p[1]]).collect()
                }
                Some(keyform_eval::DeformerMap::Rotation(r)) => vec![r.origin],
                None => Vec::new(),
            },
            Selection::Part(_) | Selection::Glue(_) => Vec::new(),
        }
    }

    /// The point of a selected object nearest to `q` within `radius`.
    pub fn point_hit(&self, q: [f32; 2], radius: f32) -> Option<(Selection, usize)> {
        let mut best: Option<(f32, Selection, usize)> = None;
        for &o in &self.selection.objects {
            for (i, p) in self.object_points(o).iter().enumerate() {
                let d = (p[0] - q[0]).hypot(p[1] - q[1]);
                if d <= radius && best.is_none_or(|b| d < b.0) {
                    best = Some((d, o, i));
                }
            }
        }
        best.map(|(_, o, i)| (o, i))
    }

    /// Visible, unlocked art meshes with a vertex inside the canvas quad.
    pub fn objects_in(&self, quad: &[[f32; 2]]) -> Vec<Selection> {
        let Some(f) = self.frame.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for &mi in f.render_order.iter().rev() {
            let (Some(st), Some(mesh)) = (f.meshes.get(mi), self.model.art_meshes.get(mi)) else {
                continue;
            };
            if st.hidden || st.outside || mesh.info.locked {
                continue;
            }
            if st
                .positions
                .chunks_exact(2)
                .any(|p| in_polygon(quad, [p[0], p[1]]))
            {
                out.push(Selection::ArtMesh(mi));
            }
        }
        out
    }

    /// Points of the selected objects inside the canvas quad.
    pub fn points_in(&self, quad: &[[f32; 2]]) -> Vec<(Selection, usize)> {
        let mut out = Vec::new();
        for &o in &self.selection.objects {
            for (i, p) in self.object_points(o).iter().enumerate() {
                if in_polygon(quad, *p) {
                    out.push((o, i));
                }
            }
        }
        out
    }

    /// Undoes one step; returns its name.
    pub fn undo(&mut self) -> Option<String> {
        let name = self.history.undo_name()?.to_owned();
        let state = self.history.undo()?.clone();
        self.restore(state);
        Some(name)
    }

    /// Redoes one step; returns its name.
    pub fn redo(&mut self) -> Option<String> {
        let name = self.history.redo_name()?.to_owned();
        let state = self.history.redo()?.clone();
        self.restore(state);
        Some(name)
    }

    fn restore(&mut self, state: EditState) {
        self.model = state.model;
        let mut sel = state.selection;
        sel.objects.retain(|s| self.selection_valid(*s));
        sel.retain_points(|(s, _)| self.selection_valid(*s));
        self.selection = sel;
        self.edit_level = state.edit_level;
        self.model_changed();
    }

    fn selection_valid(&self, s: Selection) -> bool {
        let m = &self.model;
        match s {
            Selection::Part(i) => i < m.parts.len(),
            Selection::Deformer(i) => i < m.deformers.len(),
            Selection::ArtMesh(i) => i < m.art_meshes.len(),
            Selection::Glue(i) => i < m.glues.len(),
        }
    }

    /// Rebuilds what is derived from the model after an edit, undo or redo.
    pub(crate) fn model_changed(&mut self) {
        if matches!(self.source, Source::Project(_)) {
            let ms: Vec<keyform_anim::motionsync::Setting> = self
                .model
                .motion_sync
                .iter()
                .map(keyform_anim::motionsync::Setting::from_model)
                .collect();
            if ms != self.motion_sync {
                self.motion_sync = ms;
                self.motion_sync_frames.clear();
            }
        }
        if self.drag.is_none() {
            self.mask_issues = keyform_edit::mask_issues(&self.model);
        }
        let m = &self.model;
        self.param_index = m
            .parameters
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id.clone(), i))
            .collect();
        self.part_index = m
            .parts
            .iter()
            .enumerate()
            .map(|(i, p)| (p.info.id.clone(), i))
            .collect();
        self.ranges = m
            .parameters
            .iter()
            .map(|p| ParamRange {
                min: p.min,
                max: p.max,
                repeat: p.repeat,
            })
            .collect();
        let n = m.parameters.len();
        if self.values.len() != n {
            let defaults = m.parameters.iter().map(|p| p.default);
            self.values = defaults.collect();
            self.saved.clone_from(&self.values);
            self.live.clone_from(&self.values);
            self.manual = vec![false; n];
            self.tracking = Tracking::new(m.parameters.iter().map(|p| p.id.as_str()));
        }
        for (i, p) in m.parameters.iter().enumerate() {
            self.values[i] = p.clamp(self.values[i]);
            self.saved[i] = p.clamp(self.saved[i]);
        }
    }

    /// Saves the project to its file, or to `to` (Save As), atomically.
    pub fn save_project(&mut self, to: Option<&Path>) -> Result<PathBuf, String> {
        let Source::Project(doc) = &mut self.source else {
            return Err(format!("{}: not a model project", self.title));
        };
        let path = to.unwrap_or(&self.path).to_path_buf();
        doc.model = self.model.clone();
        doc.save_to(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        self.saved_revision = self.history.revision();
        self.untitled = false;
        if to.is_some() {
            self.title = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            self.path.clone_from(&path);
        }
        Ok(path)
    }

    /// The current model as `.cmo3` bytes, without touching the open file.
    pub fn project_bytes(&self) -> Result<Vec<u8>, String> {
        let Source::Project(doc) = &self.source else {
            return Err(format!("{}: not a model project", self.title));
        };
        let mut d = (**doc).clone();
        d.model = self.model.clone();
        d.save_bytes().map_err(|e| e.to_string())
    }

    /// Back to defaults: parameters, playback state, physics, pose.
    pub fn reset_parameters(&mut self) {
        self.values = self.model.parameters.iter().map(|p| p.default).collect();
        self.saved.clone_from(&self.values);
        self.manual.iter_mut().for_each(|m| *m = false);
        self.virtuals.clear();
        self.pose_state = None;
        self.motion_out = MotionOutputs::default();
        self.sim.reset(&self.model.physics);
    }

    /// Sets a parameter by hand (slider); it then overrides playback (§3.1 step 8).
    pub fn set_manual(&mut self, p: usize, v: f32) {
        if p < self.values.len() {
            self.values[p] = v;
            self.manual[p] = true;
        }
    }

    /// Releases a hand-set parameter back to playback.
    pub fn release_manual(&mut self, p: usize) {
        if let Some(m) = self.manual.get_mut(p) {
            *m = false;
        }
        if let (Some(v), Some(d)) = (self.saved.get_mut(p), self.model.parameters.get(p)) {
            *v = d.default;
        }
    }

    /// Re-applies the pose after editing (viewer.md §3.4 reset).
    pub fn pose_changed(&mut self) {
        self.pose_state = None;
    }

    pub fn play_motion(&mut self, i: usize) {
        if let Some(m) = self.motions.get(i).and_then(|m| m.motion.clone()) {
            self.motion_player.start(m, self.looping);
            self.playing_motion = Some(i);
        }
    }

    pub fn play_expression(&mut self, i: usize) {
        if let Some(e) = self.expressions.get(i) {
            self.expression_player.start(Arc::new(e.expression.clone()));
        }
    }

    /// Whether viewer settings can be saved (runtime models only).
    pub fn can_save_settings(&self) -> bool {
        matches!(self.source, Source::Runtime { .. })
    }

    /// Saves the viewer settings: model3.json (created next to a bare moc3
    /// if missing), edited expressions, and the pose. Returns the files
    /// written. spec: 70-advanced/viewer.md §1
    pub fn save_settings(&mut self) -> Result<Vec<PathBuf>, String> {
        let Source::Runtime { model3, dir } = &mut self.source else {
            return Err("Viewer settings belong to runtime models (.model3.json / .moc3).".into());
        };
        let dir = dir.clone();
        let mut written = Vec::new();
        let write =
            |path: PathBuf, text: String, written: &mut Vec<PathBuf>| -> Result<(), String> {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
                written.push(path);
                Ok(())
            };
        let stem = self
            .title
            .trim_end_matches(".model3.json")
            .trim_end_matches(".moc3")
            .to_owned();
        let (m3_path, m3) = match model3 {
            Some(m) => (self.path.clone(), m),
            None => {
                let moc = self.title.clone();
                *model3 = Some(Box::new(Model3::new(&moc, Vec::new())));
                let m = model3.as_mut().ok_or("internal")?;
                (dir.join(format!("{stem}.model3.json")), m)
            }
        };

        // Motions, grouped in order of first appearance.
        let mut groups: Vec<(String, Vec<MotionRef>)> = Vec::new();
        for e in &self.motions {
            match groups.iter_mut().find(|(g, _)| *g == e.group) {
                Some((_, refs)) => refs.push(e.reference.clone()),
                None => groups.push((e.group.clone(), vec![e.reference.clone()])),
            }
        }
        m3.motions = if groups.is_empty() && m3.motions.is_none() {
            None
        } else {
            Some(groups)
        };

        // Expressions: write the edited ones.
        for e in &mut self.expressions {
            if e.dirty && !e.broken {
                write(
                    dir.join(&e.file),
                    keyform_anim::json::to_string(&e.expression.to_json()),
                    &mut written,
                )?;
                e.dirty = false;
            }
        }
        let exps: Vec<(String, String)> = self
            .expressions
            .iter()
            .map(|e| (e.name.clone(), e.file.clone()))
            .collect();
        m3.expressions = if exps.is_empty() && m3.expressions.is_none() {
            None
        } else {
            Some(exps)
        };

        // Pose.
        match &self.pose {
            Some(p) if !p.groups.is_empty() => {
                let file = m3
                    .pose
                    .clone()
                    .unwrap_or_else(|| format!("{stem}.pose3.json"));
                write(
                    dir.join(&file),
                    keyform_anim::json::to_string(&p.to_json()),
                    &mut written,
                )?;
                m3.pose = Some(file);
            }
            _ => m3.pose = None,
        }

        // Hit areas and effect groups.
        m3.hit_areas = if self.hit_areas.is_empty() && m3.hit_areas.is_none() {
            None
        } else {
            Some(self.hit_areas.clone())
        };
        let ids = |list: &[usize]| -> Vec<String> {
            list.iter()
                .filter_map(|&p| self.model.parameters.get(p))
                .map(|p| p.id.clone())
                .collect()
        };
        let mut g = m3.groups.clone().unwrap_or_default();
        for (name, list) in [
            ("EyeBlink", ids(&self.model.eye_blink_parameters)),
            ("LipSync", ids(&self.model.lip_sync_parameters)),
        ] {
            match g.iter().position(|x| x.name == name) {
                Some(i) if list.is_empty() => {
                    g.remove(i);
                }
                Some(i) => g[i].ids = list,
                None if !list.is_empty() => g.push(Model3Group {
                    target: "Parameter".into(),
                    name: name.into(),
                    ids: list,
                }),
                None => {}
            }
        }
        m3.groups = if g.is_empty() && m3.groups.is_none() {
            None
        } else {
            Some(g)
        };

        write(m3_path.clone(), m3.to_string_pretty(), &mut written)?;
        if self.path != m3_path {
            // A bare moc3 now has a model3.json: further saves go there.
            self.path = m3_path;
        }
        Ok(written)
    }

    /// Whether anything animates (the UI then repaints continuously).
    pub fn is_animating(&self, o: &PlaybackOptions) -> bool {
        if self.mode == Mode::Modeling {
            return false;
        }
        if self.mode == Mode::Animation {
            return self.anim.as_ref().is_some_and(|a| a.playing);
        }
        self.motion_player.is_playing()
            || o.physics && !self.model.physics.settings.is_empty()
            || o.pose && self.pose.is_some()
            || o.breath
            || o.eye_blink && !self.model.eye_blink_parameters.is_empty()
            || o.track_cursor
            || o.idle_motion && !self.motions.is_empty()
    }

    /// Idle playback: a random motion, never the previous one twice in a
    /// row (the next index is used instead). spec: viewer.md §3.1 step 2
    fn start_idle_motion(&mut self) {
        let n = self.motions.len();
        if n == 0 {
            return;
        }
        let mut i = ((self.idle_rng.next_f64() * n as f64) as usize).min(n - 1);
        if Some(i) == self.idle_last {
            i = (i + 1) % n;
        }
        self.idle_last = Some(i);
        if let Some(m) = self.motions[i].motion.clone() {
            self.motion_player.start(m, false);
            self.playing_motion = Some(i);
        }
    }

    /// Analysed motion-sync frames of motion `i` (computed on first use):
    /// the setting named by its model3.json `MotionSync` entry applied to
    /// its `Sound`. spec: viewer.md §3.8
    fn motion_sync_frames(
        &mut self,
        i: usize,
        log: &mut Log,
    ) -> Option<&(f32, Vec<keyform_anim::motionsync::FrameMap>)> {
        if !self.motion_sync_frames.contains_key(&i) {
            let computed = (|| {
                let entry = self.motions.get(i)?;
                let name = entry.reference.motion_sync.as_deref()?;
                let setting = self
                    .motion_sync
                    .iter()
                    .find(|s| s.name == name || s.id == name)?;
                let sound = entry.reference.sound.as_deref()?;
                let Source::Runtime { dir, .. } = &self.source else {
                    return None;
                };
                let pcm = std::fs::read(dir.join(sound))
                    .map_err(|e| e.to_string())
                    .and_then(|d| keyform_anim::audio::read_wav(&d).map_err(|e| e.to_string()));
                match pcm {
                    Ok(pcm) => Some((setting.sample_rate, setting.frames(&pcm))),
                    Err(e) => {
                        log.warn(format!("{sound}: {e}"));
                        None
                    }
                }
            })();
            self.motion_sync_frames.insert(i, computed);
        }
        self.motion_sync_frames.get(&i).and_then(Option::as_ref)
    }

    /// Advances playback by `dt` seconds and evaluates the model, in the
    /// frame order of spec 70-advanced/viewer.md §3.1.
    pub fn tick(&mut self, dt: f32, o: &PlaybackOptions, log: &mut Log) {
        if self.mode == Mode::Animation {
            // The scene at the playhead (animation-model.md §5).
            self.anim_tick(dt);
            let at = self.anim.as_ref().map_or(0.0, |a| a.frame);
            if let Some((values, _)) = self.anim_values_at(at) {
                self.live = values;
            }
            // Placed in the scene by the track's visual effect; controllers
            // with a target follow it when asked (§2.3).
            let follow = self.anim.as_ref().is_some_and(|a| a.ctrl_follow);
            let opts = crate::animation::FrameOpts {
                values: if follow {
                    self.solve_ctrl_targets(at)
                        .into_iter()
                        .map(|(pi, v)| (pi, v as f32))
                        .collect()
                } else {
                    Vec::new()
                },
                ..Default::default()
            };
            let mut opts = opts;
            opts.values.extend(self.external.iter().copied());
            let f = self.anim_frame_opts(at, &opts).unwrap_or_else(|| {
                let ov = Overrides {
                    part_opacity: vec![None; self.model.parts.len()],
                    model_opacity: 1.0,
                    ..Default::default()
                };
                Evaluator::new(&self.model).evaluate_full(&self.live, EvalOptions::default(), &ov)
            });
            self.frame = Some(f);
            self.group_opacity = self.anim_group_opacity(at);
            return;
        }
        self.group_opacity = 1.0;
        if self.mode == Mode::Modeling {
            // The model exactly at the slider values, nothing animated.
            self.live.clone_from(&self.values);
            self.apply_external();
            let ov = Overrides {
                part_opacity: vec![None; self.model.parts.len()],
                model_opacity: 1.0,
                ..Default::default()
            };
            let ev = Evaluator::new(&self.model);
            self.frame = Some(ev.evaluate_full(&self.live, EvalOptions::default(), &ov));
            return;
        }
        self.time += f64::from(dt);
        // Parameter repeat (§3.8): repeat parameters wrap only when it is on.
        for (r, p) in self.ranges.iter_mut().zip(&self.model.parameters) {
            r.repeat = p.repeat && o.parameter_repeat;
        }
        self.motion_player.force_restricted_beziers = o.old_playback;
        self.expression_player.old_fading = o.old_fading;
        if o.idle_motion && !self.motion_player.is_playing() {
            self.start_idle_motion();
        }
        // Motion-sync data of the playing motion, analysed on first use.
        let sync_motion = self
            .playing_motion
            .filter(|_| o.motion_sync && !self.motion_sync.is_empty());
        if let Some(i) = sync_motion {
            let _ = self.motion_sync_frames(i, log);
        }
        // 1. Restore the values saved after the previous motion update.
        self.live.clone_from(&self.saved);
        let mut p = Params {
            values: &mut self.live,
            ranges: &self.ranges,
            index: &self.param_index,
            virtuals: &mut self.virtuals,
        };
        // 2. Motion queue.
        let upd = self.motion_player.update(
            dt,
            &mut p,
            &self.model.eye_blink_parameters,
            &self.model.lip_sync_parameters,
            &mut self.motion_out,
        );
        for e in upd.events {
            log.info(format!("motion event: {e}"));
        }
        // 3. Save.
        self.saved.copy_from_slice(p.values);
        // 4. Eye blink only when no motion was updated.
        if o.eye_blink && !upd.updated {
            self.eye_blink
                .update(dt, &mut p, &self.model.eye_blink_parameters);
        }
        // 5. Expressions.
        if o.expression {
            self.expression_player.update(dt, &mut p);
        }
        // 6. Cursor tracking.
        if o.track_cursor {
            self.tracking.update(dt, &mut p);
        }
        // 7. Breath.
        if o.breath {
            self.breath.update(dt, &mut p);
        }
        // 8. Parameters adjusted by hand.
        for (i, &m) in self.manual.iter().enumerate() {
            if m {
                p.set_index(i, self.values[i], 1.0);
            }
        }
        // 9. Physics.
        if o.physics {
            self.sim
                .step(&self.model.physics, &self.model.parameters, p.values, dt);
        }
        // 10. Pose.
        let mut ov = Overrides {
            part_opacity: vec![None; self.model.parts.len()],
            model_opacity: self.motion_out.model_opacity,
            ..Default::default()
        };
        if let Some(pose) = self.pose.as_ref().filter(|_| o.pose) {
            let st = self
                .pose_state
                .get_or_insert_with(|| PoseState::reset(pose, &mut p));
            for (id, o) in st.update(pose, dt, &mut p) {
                if let Some(&pi) = self.part_index.get(&id) {
                    ov.part_opacity[pi] = Some(o);
                }
            }
        }
        // 11. Motion-sync lip sync at the sound position (viewer.md §3.8).
        if let (Some(i), Some(t)) = (sync_motion, self.motion_player.newest_elapsed()) {
            if let Some(Some((rate, frames))) = self.motion_sync_frames.get(&i) {
                keyform_anim::motionsync::apply(frames, *rate, t * 1e6, &mut p);
            }
        }
        self.apply_external();
        let ev = Evaluator::new(&self.model);
        self.frame = Some(ev.evaluate_full(&self.live, EvalOptions::default(), &ov));
    }

    /// Pushed external values win over everything else.
    fn apply_external(&mut self) {
        for &(i, v) in &self.external {
            if let Some(x) = self.live.get_mut(i) {
                *x = v;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                copy_dir(&p, &to.join(e.file_name()));
            } else {
                std::fs::copy(&p, to.join(e.file_name())).unwrap();
            }
        }
    }

    /// Saving viewer settings on a sample runtime folder (a scratch copy):
    /// unchanged → identical model3.json; edits land in place.
    #[test]
    fn save_settings_roundtrip_and_edit() {
        let Ok(dirs) = std::env::var("KEYFORM_SAMPLES") else {
            return;
        };
        let Some(model3) = std::env::split_paths(&dirs)
            .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
            .map(|e| e.path())
            .find(|p| p.to_string_lossy().ends_with(".model3.json"))
        else {
            return;
        };
        let scratch =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch/viewer-save");
        let _ = std::fs::remove_dir_all(&scratch);
        copy_dir(model3.parent().unwrap(), &scratch);
        let path = scratch.join(model3.file_name().unwrap());
        let before = std::fs::read_to_string(&path).unwrap();
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        doc.save_settings().unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "unchanged save must not alter model3.json"
        );

        doc.hit_areas.push(("HitAreaTest".into(), "Test".into()));
        if let Some(e) = doc.expressions.first_mut() {
            e.expression.fade_in = Some(0.25);
            e.dirty = true;
        }
        doc.live = doc.values.clone();
        if let Some(v) = doc.live.first_mut() {
            *v += 1.0;
        }
        let n = doc.expressions.len();
        // "New from current values" equivalent.
        doc.expressions.push(ExpressionEntry {
            name: "New.exp3.json".into(),
            file: "expressions/New.exp3.json".into(),
            expression: Expression {
                fade_in: None,
                fade_out: None,
                params: vec![],
            },
            dirty: true,
            broken: false,
        });
        let written = doc.save_settings().unwrap();
        assert!(written
            .iter()
            .any(|p| p.ends_with("expressions/New.exp3.json")));
        let m3 = Model3::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(m3
            .hit_areas
            .unwrap()
            .contains(&("HitAreaTest".into(), "Test".into())));
        assert_eq!(m3.expressions.unwrap().len(), n + 1);
        if n > 0 {
            let first = &doc.expressions[0];
            let e = Expression::parse(&std::fs::read_to_string(scratch.join(&first.file)).unwrap())
                .unwrap();
            assert_eq!(e.fade_in, Some(0.25));
        }
        // The edited model3.json reopens.
        Document::open(&path, &mut log).unwrap();
    }

    /// Edit → undo/redo → save → reopen on a sample project (scratch copy).
    #[test]
    fn project_edit_undo_save() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dirs = std::env::var("KEYFORM_SAMPLES").unwrap_or_else(|_| root.display().to_string());
        let Some(src) = std::env::split_paths(&dirs)
            .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "cmo3"))
        else {
            return;
        };
        let scratch = root.join("target/test-scratch/project-save");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("model.cmo3");
        std::fs::copy(&src, &path).unwrap();
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        assert!(doc.editable() && !doc.is_modified());
        let original = doc.model.clone();

        doc.select(Some(Selection::ArtMesh(0)));
        assert!(!doc.is_modified(), "selection is not a modification");
        assert!(!doc.edit("Nothing", |_| {}), "empty step is discarded");
        assert!(doc.edit("Change Name", |m| {
            Arc::make_mut(&mut m.art_meshes[0]).info.name = "Renamed".into();
        }));
        assert!(doc.edit("Change Visibility", |m| {
            let v = &mut Arc::make_mut(&mut m.parts[0]).info.visible;
            *v = !*v;
        }));
        assert!(doc.is_modified());
        let edited = doc.model.clone();

        assert_eq!(doc.undo().as_deref(), Some("Change Visibility"));
        assert_eq!(doc.undo().as_deref(), Some("Change Name"));
        assert!(doc.model.same_data(&original));
        assert!(!doc.is_modified());
        assert_eq!(doc.primary(), Some(Selection::ArtMesh(0)));
        assert_eq!(doc.redo().as_deref(), Some("Change Name"));
        assert_eq!(doc.redo().as_deref(), Some("Change Visibility"));
        assert!(doc.model.same_data(&edited));

        doc.save_project(None).unwrap();
        assert!(!doc.is_modified());
        let back = Document::open(&path, &mut log).unwrap();
        assert!(back.model.same_data(&edited));
        // Undo after saving marks the document modified again.
        doc.undo();
        assert!(doc.is_modified());
    }

    /// Parameter palette key operations: undoable, saved, reloaded.
    #[test]
    fn key_operations_undo_and_save() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dirs = std::env::var("KEYFORM_SAMPLES").unwrap_or_else(|_| root.display().to_string());
        let Some(src) = std::env::split_paths(&dirs)
            .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "cmo3"))
        else {
            return;
        };
        let scratch = root.join("target/test-scratch/key-ops");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("model.cmo3");
        std::fs::copy(&src, &path).unwrap();
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        assert!(doc.key_op(KeyOp::Add, 0).is_err(), "nothing selected");
        // An unbound normal parameter of art mesh 0.
        doc.select(Some(Selection::ArtMesh(0)));
        let p = (0..doc.model.parameters.len())
            .find(|&p| {
                let q = &doc.model.parameters[p];
                !q.repeat
                    && q.kind == keyform_model::ParameterKind::Normal
                    && keyform_edit::keys_of(&doc.model, Selection::ArtMesh(0), p).is_empty()
            })
            .unwrap();
        let (lo, def, hi) = {
            let q = &doc.model.parameters[p];
            (q.min, q.default, q.max)
        };
        doc.key_op(KeyOp::AddThree, p).unwrap();
        let keys = keyform_edit::keys_of(&doc.model, Selection::ArtMesh(0), p);
        let mut want = vec![lo, def, hi];
        want.dedup();
        assert_eq!(keys, want);
        assert_eq!(doc.history.undo_name(), Some("Add Key"));
        // At the default value a key exists: Add refuses, Delete works.
        doc.set_manual(p, def);
        assert_eq!(
            doc.key_op(KeyOp::Add, p),
            Err("Cannot set an existing value.".into())
        );
        doc.key_op(KeyOp::Delete, p).unwrap();
        assert_eq!(
            keyform_edit::keys_of(&doc.model, Selection::ArtMesh(0), p).len(),
            want.len() - 1
        );
        doc.save_project(None).unwrap();
        let back = Document::open(&path, &mut log).unwrap();
        assert!(back.model.same_data(&doc.model));
        // Undo both steps: the original grid comes back and saves too.
        doc.undo();
        doc.undo();
        assert!(keyform_edit::keys_of(&doc.model, Selection::ArtMesh(0), p).is_empty());
        doc.save_project(None).unwrap();
        let back = Document::open(&path, &mut log).unwrap();
        assert!(back.model.same_data(&doc.model));
    }

    /// Arrow tool: hit-test a mesh, drag it in its current keyform, undo.
    #[test]
    fn drag_moves_current_keyform() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dirs = std::env::var("KEYFORM_SAMPLES").unwrap_or_else(|_| root.display().to_string());
        let Some(path) = std::env::split_paths(&dirs)
            .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "cmo3"))
        else {
            return;
        };
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        assert_eq!(doc.mode, Mode::Modeling);
        doc.tick(0.0, &PlaybackOptions::default(), &mut log);
        // The centroid of some visible mesh's first triangle hits a mesh.
        let frame = doc.frame.clone().unwrap();
        let (_, q) = frame
            .render_order
            .iter()
            .rev()
            .find_map(|&mi| {
                let st = &frame.meshes[mi];
                let t = doc.model.art_meshes[mi].indices.get(..3)?;
                (!st.hidden && !st.outside && st.opacity > 0.0).then(|| {
                    let p = |i: u32| {
                        [
                            st.positions[2 * i as usize],
                            st.positions[2 * i as usize + 1],
                        ]
                    };
                    let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                    (mi, [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0])
                })
            })
            .unwrap();
        let hit = doc.hit_test(q).unwrap();
        let Selection::ArtMesh(h) = hit else { panic!() };
        // Put every parameter on a key of the hit mesh (defaults may not be).
        for b in doc.model.art_meshes[h].grid.bindings.clone() {
            doc.set_manual(b.parameter, b.keys[0]);
        }
        doc.tick(0.0, &PlaybackOptions::default(), &mut log);
        let before = doc.frame.as_ref().unwrap().meshes[h].positions.clone();
        doc.select(Some(hit));
        doc.drag_begin(false).unwrap();
        doc.drag_update([5.0, 0.0], false);
        doc.drag_update([10.0, -4.0], false);
        doc.drag_end();
        assert_eq!(doc.history.undo_name(), Some("Move"));
        doc.tick(0.0, &PlaybackOptions::default(), &mut log);
        let after = &doc.frame.as_ref().unwrap().meshes[h].positions;
        // Every vertex moved by (10, −4) on the canvas (glue may bend a few).
        let moved = before
            .chunks(2)
            .zip(after.chunks(2))
            .filter(|(a, b)| {
                ((b[0] - a[0]) - 10.0).abs() < 0.05 && ((b[1] - a[1]) + 4.0).abs() < 0.05
            })
            .count();
        assert!(
            moved * 10 >= before.len() / 2 * 9,
            "{moved} of {}",
            before.len() / 2
        );
        doc.undo();
        doc.tick(0.0, &PlaybackOptions::default(), &mut log);
        assert_eq!(doc.frame.as_ref().unwrap().meshes[h].positions, before);
    }

    /// Motion-sync end to end on a scratch copy of a sample runtime folder:
    /// a vowel sound opens the mouth while the motion plays.
    #[test]
    fn motion_sync_opens_the_mouth() {
        let Ok(dirs) = std::env::var("KEYFORM_SAMPLES") else {
            return;
        };
        let Some(model3) = std::env::split_paths(&dirs)
            .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
            .map(|e| e.path())
            .find(|p| p.to_string_lossy().ends_with(".model3.json"))
        else {
            return;
        };
        let scratch =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch/motion-sync");
        let _ = std::fs::remove_dir_all(&scratch);
        copy_dir(model3.parent().unwrap(), &scratch);
        let path = scratch.join(model3.file_name().unwrap());
        let mut m3 = Model3::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let Some((_, refs)) = m3.motions.as_mut().and_then(|g| g.first_mut()) else {
            return;
        };
        let Some(r) = refs.first_mut() else { return };
        r.sound = Some("vowel.wav".into());
        r.motion_sync = Some("Mouth".into());
        m3.motion_sync = Some("test.motionsync3.json".into());
        std::fs::write(&path, m3.to_string_pretty()).unwrap();
        std::fs::write(
            scratch.join("test.motionsync3.json"),
            r#"{"Version":1,"Meta":{"SettingCount":1,"Dictionary":[{"Id":"S","Name":"Mouth"}]},
            "Settings":[{"Id":"S","AnalysisType":"CRI","UseCase":"Mouth",
            "CubismParameters":[{"Name":"Open","Id":"ParamMouthOpenY","Min":0,"Max":1,"Damper":0,"Smooth":0}],
            "AudioParameters":[{"Name":"A","Id":"A","Min":0,"Max":1,"Scale":1,"Enabled":true},
              {"Name":"Silence","Id":"Silence","Min":0,"Max":1,"Scale":1,"Enabled":true}],
            "Mappings":[{"Type":"Shape","Id":"A","Targets":[{"Id":"ParamMouthOpenY","Value":1}]},
              {"Type":"Shape","Id":"Silence","Targets":[{"Id":"ParamMouthOpenY","Value":0}]}],
            "PostProcessing":{"BlendRatio":100,"Smoothing":0,"SampleRate":30}}]}"#,
        )
        .unwrap();
        // 0.5 s of a loud 220 Hz buzz with formants near "a", 16 kHz mono.
        let rate = 16000u32;
        let samples: Vec<i16> = (0..rate / 2)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let s = (std::f32::consts::TAU * 750.0 * t).sin() * 0.6
                    + (std::f32::consts::TAU * 1200.0 * t).sin() * 0.3
                    + (std::f32::consts::TAU * 220.0 * t).sin() * 0.1;
                (s * 0.8 * 32767.0) as i16
            })
            .collect();
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
        for v in samples {
            wav.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(scratch.join("vowel.wav"), wav).unwrap();

        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        assert_eq!(doc.motion_sync.len(), 1);
        let Some(&mouth) = doc.param_index.get("ParamMouthOpenY") else {
            return;
        };
        let o = PlaybackOptions::default();
        doc.play_motion(0);
        doc.tick(0.0, &o, &mut log);
        let mut peak = 0.0f32;
        for _ in 0..12 {
            doc.tick(1.0 / 60.0, &o, &mut log);
            peak = peak.max(doc.live[mouth]);
        }
        assert!(peak > 0.5, "mouth peak {peak}");
        // With the toggle off nothing opens it (the motion itself may).
        let off = PlaybackOptions {
            motion_sync: false,
            ..o
        };
        let mut doc = Document::open(&path, &mut log).unwrap();
        doc.play_motion(0);
        doc.tick(0.0, &off, &mut log);
        let mut peak_off = 0.0f32;
        for _ in 0..12 {
            doc.tick(1.0 / 60.0, &off, &mut log);
            peak_off = peak_off.max(doc.live[mouth]);
        }
        eprintln!("motion-sync mouth peak {peak}, toggle off {peak_off}");
        assert!(peak_off < peak, "{peak_off} vs {peak}");
    }

    #[test]
    fn quad_test() {
        let q = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert!(in_polygon(&q, [5.0, 5.0]));
        assert!(!in_polygon(&q, [11.0, 5.0]));
        let rev = [q[3], q[2], q[1], q[0]];
        assert!(in_polygon(&rev, [1.0, 9.0]));
        // A concave lasso (a "C"): the notch is outside.
        let c = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 3.0],
            [3.0, 3.0],
            [3.0, 7.0],
            [10.0, 7.0],
            [10.0, 10.0],
            [0.0, 10.0],
        ];
        assert!(in_polygon(&c, [1.0, 5.0]));
        assert!(!in_polygon(&c, [6.0, 5.0]));
        assert!(in_polygon(&c, [6.0, 1.0]));
        assert!(!in_polygon(&c[..2], [1.0, 0.0]), "degenerate");
    }

    /// Point selection: only the selected vertex moves; rectangles select.
    #[test]
    fn point_drag_moves_only_selected_points() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("hiyori_pro_t11.cmo3");
        if !path.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        let o = PlaybackOptions::default();
        doc.tick(0.0, &o, &mut log);
        // A visible mesh whose grid is on keys at the defaults.
        let f = doc.frame.clone().unwrap();
        let a = (0..doc.model.art_meshes.len())
            .find(|&a| {
                let st = &f.meshes[a];
                !st.hidden
                    && !st.outside
                    && keyform_edit::current_cell(&doc.model, Selection::ArtMesh(a), &doc.values)
                        .is_ok()
            })
            .unwrap();
        let sel = Selection::ArtMesh(a);
        doc.select(Some(sel));
        let pts = doc.object_points(sel);
        let hit = doc.point_hit(pts[0], 0.5).unwrap();
        assert_eq!(hit.0, sel);
        doc.set_selection(SelectionSet {
            objects: vec![sel],
            points: vec![hit],
            point_weights: Vec::new(),
        });
        doc.drag_begin(true).unwrap();
        doc.drag_update([0.0, 7.0], false);
        doc.drag_end();
        doc.tick(0.0, &o, &mut log);
        let after = doc.object_points(sel);
        let moved: Vec<usize> = (0..pts.len())
            .filter(|&i| (after[i][1] - pts[i][1]).abs() > 1e-3)
            .collect();
        assert!(moved.contains(&hit.1));
        // Glue may pull a few neighbours; most vertices stay.
        assert!(moved.len() * 4 <= pts.len().max(4), "{moved:?}");
        // A rectangle around the mesh's first vertex finds the mesh.
        let p = after[0];
        let quad = [
            [p[0] - 1.0, p[1] - 1.0],
            [p[0] + 1.0, p[1] - 1.0],
            [p[0] + 1.0, p[1] + 1.0],
            [p[0] - 1.0, p[1] + 1.0],
        ];
        assert!(doc.objects_in(&quad).contains(&sel));
        assert!(doc.points_in(&quad).iter().any(|x| x.0 == sel));
    }

    /// Rotation deformer widget: the handle rotates, the circle scales.
    #[test]
    fn rotation_handles_rotate_and_scale() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("hiyori_pro_t11.cmo3");
        if !path.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        let o = PlaybackOptions::default();
        doc.tick(0.0, &o, &mut log);
        let d = (0..doc.model.deformers.len())
            .find(|&d| {
                matches!(doc.model.deformers[d].kind, DeformerKind::Rotation(_))
                    && keyform_edit::current_cell(&doc.model, Selection::Deformer(d), &doc.values)
                        .is_ok()
            })
            .unwrap();
        let sel = Selection::Deformer(d);
        let cell = keyform_edit::current_cell(&doc.model, sel, &doc.values).unwrap();
        let angle = |doc: &Document| match &doc.model.deformers[d].kind {
            DeformerKind::Rotation(r) => (r.grid.keyforms[cell].angle, r.grid.keyforms[cell].scale),
            _ => unreachable!(),
        };
        let (a0, s0) = angle(&doc);
        let (centre, tip, radius) = doc.rotation_widget(sel).unwrap();
        // Quarter turn clockwise on screen (y down): +90°.
        let v = [tip[0] - centre[0], tip[1] - centre[1]];
        let turned = [centre[0] - v[1], centre[1] + v[0]];
        doc.drag_begin_handle(sel, Handle::Rotate, tip).unwrap();
        doc.drag_update([turned[0] - tip[0], turned[1] - tip[1]], false);
        doc.drag_end();
        assert!(
            (angle(&doc).0 - (a0 + 90.0)).abs() < 1e-3,
            "{:?}",
            angle(&doc)
        );
        assert_eq!(doc.history.undo_name(), Some("Rotate"));
        // Shift snaps to 45°.
        doc.tick(0.0, &o, &mut log);
        let (centre, tip, _) = doc.rotation_widget(sel).unwrap();
        let v = [tip[0] - centre[0], tip[1] - centre[1]];
        let (s, c) = 40f32.to_radians().sin_cos();
        let t = [
            centre[0] + c * v[0] - s * v[1],
            centre[1] + s * v[0] + c * v[1],
        ];
        doc.drag_begin_handle(sel, Handle::Rotate, tip).unwrap();
        doc.drag_update([t[0] - tip[0], t[1] - tip[1]], true);
        doc.drag_end();
        assert_eq!(angle(&doc).0.rem_euclid(45.0), 0.0, "{:?}", angle(&doc));
        // Dragging the circle out to twice its radius doubles the scale.
        let start = [centre[0] + radius, centre[1]];
        doc.drag_begin_handle(sel, Handle::Scale, start).unwrap();
        doc.drag_update([radius, 0.0], false);
        doc.drag_end();
        assert!((angle(&doc).1 - 2.0 * s0).abs() < 1e-4);
        assert_eq!(doc.history.undo_name(), Some("Scale"));
    }

    /// Edit level 2: moving a Bezier net anchor reshapes the warp grid.
    #[test]
    fn warp_net_anchor_drag() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("hiyori_pro_t11.cmo3");
        if !path.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        let o = PlaybackOptions::default();
        doc.tick(0.0, &o, &mut log);
        let d = (0..doc.model.deformers.len())
            .find(|&d| match &doc.model.deformers[d].kind {
                DeformerKind::Warp(w) => {
                    w.cols >= 2
                        && w.rows >= 2
                        && keyform_edit::current_cell(
                            &doc.model,
                            Selection::Deformer(d),
                            &doc.values,
                        )
                        .is_ok()
                }
                _ => false,
            })
            .unwrap();
        let sel = Selection::Deformer(d);
        doc.select(Some(sel));
        doc.set_edit_level(1);
        doc.set_edit_level(2);
        assert_eq!(doc.history.undo_name(), Some("Edit Level"));
        let net = doc.canvas_net(d).unwrap();
        // The centre anchor of the 2×2 net.
        let centre = (net.cols + 1) * (net.rows / 2) + net.cols / 2;
        let grid_before = doc.object_points(sel);
        assert_eq!(grid_before.len(), net.points().len());
        doc.set_selection(SelectionSet {
            objects: vec![sel],
            points: vec![(sel, centre)],
            point_weights: Vec::new(),
        });
        doc.drag_begin(true).unwrap();
        doc.drag_update([12.0, 0.0], false);
        doc.drag_end();
        doc.tick(0.0, &o, &mut log);
        let DeformerKind::Warp(w) = &doc.model.deformers[d].kind else {
            unreachable!()
        };
        let Some(keyform_eval::DeformerMap::Warp(map)) =
            doc.frame.as_ref().unwrap().deformers[d].map.clone()
        else {
            unreachable!()
        };
        // Grid corners stay, the middle point moves most of the 12 px.
        let base = Document::open(&path, &mut log).unwrap();
        let mut b = base;
        b.tick(0.0, &o, &mut log);
        let Some(keyform_eval::DeformerMap::Warp(map0)) =
            b.frame.as_ref().unwrap().deformers[d].map.clone()
        else {
            unreachable!()
        };
        let at = |m: &keyform_eval::WarpMap, i: usize, j: usize| {
            let k = 2 * (j * (m.cols + 1) + i);
            [m.points[k], m.points[k + 1]]
        };
        let (c, r) = (w.cols as usize, w.rows as usize);
        assert!((at(&map, 0, 0)[0] - at(&map0, 0, 0)[0]).abs() < 1e-2);
        assert!((at(&map, c, r)[0] - at(&map0, c, r)[0]).abs() < 1e-2);
        let mid = at(&map, c / 2, r / 2)[0] - at(&map0, c / 2, r / 2)[0];
        assert!(mid > 6.0 && mid < 12.5, "middle moved {mid}");
        doc.undo();
        doc.tick(0.0, &o, &mut log);
        let Some(keyform_eval::DeformerMap::Warp(back)) =
            doc.frame.as_ref().unwrap().deformers[d].map.clone()
        else {
            unreachable!()
        };
        assert_eq!(back.points, map0.points);
    }

    /// Create a deformer around a mesh, delete it, undo/redo, save.
    #[test]
    fn create_and_delete_deformer_in_document() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let src = root.join("hiyori_pro_t11.cmo3");
        if !src.exists() {
            return;
        }
        let scratch = root.join("target/test-scratch/create-delete");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("model.cmo3");
        std::fs::copy(&src, &path).unwrap();
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        let n0 = doc.model.deformers.len();
        let a = Selection::ArtMesh(0);
        let parent = doc.model.art_meshes[0].target;
        let values = doc.values.clone();
        let d = doc
            .try_edit("Create Rotation Deformer", |m| {
                keyform_edit::objects::create_rotation_deformer(
                    m,
                    &keyform_edit::objects::NewDeformer {
                        name: "R".into(),
                        part: m.art_meshes[0].info.parent_part,
                        parent,
                        children: &[a],
                        values: &values,
                    },
                )
            })
            .unwrap();
        assert_eq!(doc.model.deformers.len(), n0 + 1);
        assert_eq!(doc.model.art_meshes[0].target, Some(d));
        doc.save_project(None).unwrap();
        doc.select(Some(Selection::Deformer(d)));
        doc.delete_selection().unwrap();
        assert_eq!(doc.model.deformers.len(), n0);
        assert_eq!(doc.model.art_meshes[0].target, parent);
        doc.save_project(None).unwrap();
        let back = Document::open(&path, &mut log).unwrap();
        assert!(back.model.same_data(&doc.model));
        // Undo brings the deformer back, and that saves too.
        assert_eq!(doc.undo().as_deref(), Some("Delete"));
        assert_eq!(doc.model.deformers.len(), n0 + 1);
        doc.save_project(None).unwrap();
        let back = Document::open(&path, &mut log).unwrap();
        assert_eq!(back.model.deformers.len(), n0 + 1);
        // A failing operation changes nothing.
        let before = doc.history.len();
        assert!(doc.try_edit("Bad", |_| Err::<(), _>("no".into())).is_err());
        assert_eq!(doc.history.len(), before);
    }

    /// Glue two meshes' selected vertices, delete a mesh (taking its glue),
    /// undo, save and reopen.
    #[test]
    fn glue_and_delete_meshes_in_document() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let src = root.join("taka.cmo3");
        if !src.exists() {
            return;
        }
        let scratch = root.join("target/test-scratch/glue-delete");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("model.cmo3");
        std::fs::copy(&src, &path).unwrap();
        let mut log = Log::default();
        let mut doc = Document::open(&path, &mut log).unwrap();
        let (n_glue, n_mesh) = (doc.model.glues.len(), doc.model.art_meshes.len());
        // One point on one mesh is not enough.
        doc.set_selection(SelectionSet {
            objects: vec![Selection::ArtMesh(0), Selection::ArtMesh(1)],
            points: vec![(Selection::ArtMesh(0), 0)],
            point_weights: Vec::new(),
        });
        assert!(doc.glue_selected_vertices().is_err());
        doc.set_selection(SelectionSet {
            objects: vec![Selection::ArtMesh(0), Selection::ArtMesh(1)],
            points: vec![(Selection::ArtMesh(0), 0), (Selection::ArtMesh(1), 0)],
            point_weights: Vec::new(),
        });
        doc.glue_selected_vertices().unwrap();
        assert_eq!(doc.model.glues.len(), n_glue + 1);
        let g = &doc.model.glues[n_glue];
        assert_eq!(g.pairs, vec![(Some(0), Some(0))]);
        assert!(g.info.id.starts_with("Glue__"));
        doc.save_project(None).unwrap();
        // Deleting mesh 1 takes the glue with it.
        doc.select(Some(Selection::ArtMesh(1)));
        let note = doc.delete_selection().unwrap();
        assert!(note.is_some_and(|n| n.contains("Glue__")));
        assert_eq!(doc.model.art_meshes.len(), n_mesh - 1);
        assert_eq!(doc.model.glues.len(), n_glue);
        doc.save_project(None).unwrap();
        let back = Document::open(&path, &mut log).unwrap();
        assert!(back.model.same_data(&doc.model));
        assert_eq!(doc.undo().as_deref(), Some("Delete"));
        assert_eq!(doc.model.glues.len(), n_glue + 1);
        doc.save_project(None).unwrap();
        let back = Document::open(&path, &mut log).unwrap();
        assert_eq!(back.model.art_meshes.len(), n_mesh);
        assert_eq!(back.model.glues.len(), n_glue + 1);
    }

    /// New model from a PSD: an unsaved, modified project that saves to a
    /// chosen path and reopens equal.
    #[test]
    fn new_model_from_psd_document() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let psd_path = root.join("blitty.psd");
        if !psd_path.exists() {
            return;
        }
        let data = std::fs::read(&psd_path).unwrap();
        let psd = keyform_formats::psd::Psd::parse(&data).unwrap();
        let (doc, _) = keyform_formats::psd_import::new_model_from_psd(
            "blitty.psd",
            &psd,
            &Default::default(),
        )
        .unwrap();
        let mut log = Log::default();
        let scratch = root.join("target/test-scratch/new-model");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("blitty.cmo3");
        let mut d = Document::new_project(&path, doc, &mut log).unwrap();
        assert!(d.untitled && d.is_modified());
        assert_eq!(d.model.parameters.len(), 27);
        d.save_project(Some(&path)).unwrap();
        assert!(!d.untitled && !d.is_modified());
        let back = Document::open(&path, &mut log).unwrap();
        assert!(back.model.same_data(&d.model));
    }
}

#[cfg(test)]
mod remap_tests {
    use super::*;

    /// Deleting an earlier mesh keeps the later one selected at its new
    /// index; a deleted object drops out; points survive only unchanged.
    #[test]
    fn selection_follows_objects() {
        let mut a = Model::default();
        for id in ["A", "B", "C"] {
            let mut p = keyform_model::Part::new(id);
            p.info.guid = keyform_model::new_guid();
            a.parts.push(std::sync::Arc::new(p));
        }
        let sel = SelectionSet {
            objects: vec![Selection::Part(0), Selection::Part(2)],
            points: Vec::new(),
            point_weights: Vec::new(),
        };
        let mut b = a.clone();
        b.parts.remove(0);
        let r = remap_selection(&a, &b, &sel);
        assert_eq!(r.objects, vec![Selection::Part(1)]);
        assert_eq!(remap_selection(&a, &a, &sel), sel);
    }
}

/// Regression tests for workflow bugs found by the end-to-end session
/// (tools/e2e/session.py), on a document made from the committed original
/// test PSD.
#[cfg(test)]
pub(crate) mod e2e_regressions {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::path::Path;

    pub(crate) fn character(scratch_name: &str) -> (Document, std::path::PathBuf) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data = std::fs::read(root.join("test-data/e2e/character.psd")).unwrap();
        let psd = keyform_formats::psd::Psd::parse(&data).unwrap();
        let (cmo3, _) = keyform_formats::psd_import::new_model_from_psd(
            "character.psd",
            &psd,
            &Default::default(),
        )
        .unwrap();
        let scratch = root.join("target/test-scratch").join(scratch_name);
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("character.cmo3");
        let mut log = Log::default();
        let mut d = Document::new_project(&path, cmo3, &mut log).unwrap();
        d.save_project(Some(&path)).unwrap();
        d.tick(0.0, &PlaybackOptions::default(), &mut log);
        (d, path)
    }

    fn mesh(d: &Document, id: &str) -> Selection {
        Selection::ArtMesh(
            d.model
                .art_meshes
                .iter()
                .position(|a| a.info.id == id)
                .unwrap(),
        )
    }

    /// Ctrl/Shift+click toggles rows into the selection after a plain click.
    #[test]
    fn palette_toggles_add_to_selection() {
        let (mut d, _) = character("e2e-toggle");
        let (face, mouth, iris) = (mesh(&d, "Face"), mesh(&d, "Mouth"), mesh(&d, "IrisL"));
        d.select(Some(face));
        d.toggle_objects(&[mouth, iris]);
        assert_eq!(d.selection.objects, vec![face, mouth, iris]);
        d.toggle_objects(&[mouth]);
        assert_eq!(d.selection.objects, vec![face, iris]);
    }

    /// Edit ▸ Select All: points of the selected object; all meshes when
    /// nothing is selected. (Escape = Deselect is `select(None)`.)
    #[test]
    fn select_all_and_deselect() {
        let (mut d, _) = character("e2e-select-all");
        d.select_all();
        assert_eq!(d.selection.objects.len(), d.model.art_meshes.len());
        let lid = mesh(&d, "EyelidL");
        d.select(Some(lid));
        d.select_all();
        assert_eq!(d.selection.points.len(), d.object_points(lid).len());
        assert!(!d.selection.points.is_empty());
        d.select(None);
        assert!(d.selection.objects.is_empty() && d.selection.points.is_empty());
    }

    /// An Inspector edit (a keyform change through try_edit) keeps the
    /// selection; a structural edit keeps it by GUID.
    #[test]
    fn edits_keep_the_selection() {
        let (mut d, _) = character("e2e-keep-selection");
        let mouth = mesh(&d, "Mouth");
        d.select(Some(mouth));
        let Selection::ArtMesh(mi) = mouth else {
            unreachable!()
        };
        d.try_edit("Change Keyform", |m| {
            std::sync::Arc::make_mut(&mut m.art_meshes[mi])
                .grid
                .keyforms[0]
                .opacity = 0.5;
            Ok(())
        })
        .unwrap();
        assert_eq!(d.selection.objects, vec![mouth]);
        // Deleting an earlier mesh renumbers it; it stays selected.
        let first = d
            .model
            .art_meshes
            .iter()
            .position(|a| a.info.id != "Mouth")
            .unwrap();
        let guid = d.model.art_meshes[mi].info.guid.clone();
        d.try_edit("Delete", |m| {
            keyform_edit::objects::delete_art_mesh(m, first).map(|_| ())
        })
        .unwrap();
        let now = d
            .model
            .art_meshes
            .iter()
            .position(|a| a.info.guid == guid)
            .unwrap();
        assert_eq!(d.selection.objects, vec![Selection::ArtMesh(now)]);
    }

    /// A drag that starts inside a selected warp deformer keeps the warp
    /// (selected objects take priority over the mesh on top).
    #[test]
    fn selected_deformer_wins_under_the_pointer() {
        let (mut d, _) = character("e2e-drag-priority");
        let face = mesh(&d, "Face");
        let values = d.values.clone();
        let w = d
            .try_edit("Create Warp Deformer", |m| {
                let nd = keyform_edit::objects::NewDeformer {
                    name: "FaceWarp".into(),
                    part: None,
                    parent: None,
                    children: &[face],
                    values: &values,
                };
                keyform_edit::objects::create_warp_deformer(m, &nd, 5, 5, (2, 2))
            })
            .unwrap();
        d.tick(0.0, &PlaybackOptions::default(), &mut Log::default());
        let warp = Selection::Deformer(w);
        d.select(Some(warp));
        let pts = d.object_points(face);
        let c = [
            pts.iter().map(|p| p[0]).sum::<f32>() / pts.len() as f32,
            pts.iter().map(|p| p[1]).sum::<f32>() / pts.len() as f32,
        ];
        assert!(d.hit_test(c).is_some(), "a mesh is under the point");
        assert_eq!(d.selected_hit(c, 5.0), Some(warp));
        d.select(None);
        assert_eq!(d.selected_hit(c, 5.0), None);
    }

    /// Moving a slider in the Animation workspace keys the value at the
    /// playhead: a new key on a frame without one, an updated key on a frame
    /// with one (reference behaviour, no toggle).
    #[test]
    fn slider_keys_at_the_playhead() {
        let (d0, path) = character("e2e-slider-key");
        let p = d0
            .model
            .parameters
            .iter()
            .position(|q| q.id == "ParamAngleX")
            .unwrap();
        let mut log = Log::default();
        let mut a = Document::new_animation(&path, &mut log).unwrap();
        a.anim.as_mut().unwrap().frame = 15.0;
        assert!(a.anim_key_param(p, 12.0));
        let keys = |a: &Document| a.anim.as_ref().unwrap().attr().unwrap().curve.keys.clone();
        assert_eq!(keys(&a).len(), 1);
        assert_eq!((keys(&a)[0].frame, keys(&a)[0].value as f32), (15, 12.0));
        assert!(a.anim_key_param(p, -5.0));
        assert_eq!(keys(&a).len(), 1, "updated, not added");
        assert_eq!(keys(&a)[0].value as f32, -5.0);
        a.anim.as_mut().unwrap().frame = 40.0;
        a.anim_key_param(p, 3.0);
        assert_eq!(keys(&a).len(), 2);
    }

    /// The Deformer palette opens the selected object's ancestors (it was
    /// collapsed, hiding a new deformer's children).
    #[test]
    fn deformer_tree_opens_to_the_selection() {
        let (mut d, _) = character("e2e-tree-open");
        let face = mesh(&d, "Face");
        let values = d.values.clone();
        let mut parent = None;
        for name in ["Outer", "Inner"] {
            let w = d
                .try_edit("Create Warp Deformer", |m| {
                    let nd = keyform_edit::objects::NewDeformer {
                        name: name.into(),
                        part: None,
                        parent,
                        children: &[face],
                        values: &values,
                    };
                    keyform_edit::objects::create_warp_deformer(m, &nd, 3, 3, (2, 2))
                })
                .unwrap();
            parent = Some(w);
        }
        let Selection::ArtMesh(f) = face else {
            unreachable!()
        };
        let inner = d.model.art_meshes[f].target.unwrap();
        let outer = d.model.deformers[inner].target.expect("nested warps");
        let open = crate::palettes::open_deformers(&d.model, Some(face));
        assert_eq!(open, [inner, outer].into_iter().collect());
        assert!(crate::palettes::open_deformers(&d.model, None).is_empty());
    }

    /// Filtering the Parts palette finds meshes inside collapsed folders:
    /// a folder matches when something inside it (at any depth) does.
    #[test]
    fn parts_filter_reaches_nested_folders() {
        let (mut d, _) = character("e2e-parts-filter");
        let Selection::ArtMesh(face) = mesh(&d, "Face") else {
            unreachable!()
        };
        let (outer, inner) = d
            .try_edit("folders", |m| {
                let outer = keyform_edit::objects::create_part(m, "Head folder", None);
                let inner = keyform_edit::objects::create_part(m, "Left Eye", Some(outer));
                keyform_edit::objects::move_to_part(
                    m,
                    &[keyform_edit::keys::ObjectRef::ArtMesh(face)],
                    inner,
                )?;
                Ok((outer, inner))
            })
            .unwrap();
        let m = &d.model;
        assert!(crate::palettes::part_matches(m, outer, "face"));
        assert!(crate::palettes::part_matches(m, inner, "face"));
        assert!(
            crate::palettes::part_matches(m, outer, "left eye"),
            "folder names match too"
        );
        assert!(!crate::palettes::part_matches(m, inner, "mouth"));
    }

    /// Selecting an object whose Deformer palette row is scrolled out of
    /// view scrolls the palette to it.
    #[test]
    fn deformer_palette_scrolls_to_the_selection() {
        let (mut d, _) = character("e2e-deformer-scroll");
        let last = d.model.art_meshes.len() - 1;
        let name = d.model.art_meshes[last].info.display_name().to_owned();
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let screen =
            eframe::egui::Rect::from_min_size(Default::default(), eframe::egui::vec2(300.0, 200.0));
        let mut row_y = Vec::new();
        for frame in 0..5 {
            if frame == 1 {
                d.select(Some(Selection::ArtMesh(last)));
            }
            let input = eframe::egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| crate::palettes::deformer_ui(ui, &mut d));
            out.textures_delta.clear();
            let update = out.platform_output.accesskit_update.expect("accesskit");
            let y = update
                .nodes
                .iter()
                .find(|(_, n)| n.label().is_some_and(|l| l == name))
                .and_then(|(_, n)| n.bounds())
                .map(|b| b.y0);
            row_y.push(y);
        }
        let first = row_y[0].expect("row present");
        assert!(first > 200.0, "the last row starts out of view (y {first})");
        let end = row_y[4].expect("row present");
        assert!(
            (0.0..190.0).contains(&end),
            "scrolled into view (y {end}, frames {row_y:?})"
        );
    }

    /// Rotation deformers show a needle while not selected (reference
    /// look), hidden ones don't, and the selected one keeps its full
    /// widget instead.
    /// Edit ▸ Copy / Paste of an art mesh (object-clipboard.md): the copy
    /// is selected, keeps the name, gets the next ID, sits above the
    /// selected original; one undo step removes it; the result saves and
    /// reopens. Cut is one step too.
    #[test]
    fn copy_paste_objects_and_undo() {
        let (mut d, path) = character("e2e-copy-paste");
        let brow = mesh(&d, "BrowR");
        let Selection::ArtMesh(orig) = brow else {
            unreachable!()
        };
        d.select(Some(brow));
        let clip = d.copy_objects(true).unwrap();
        let n = d.model.art_meshes.len();
        assert_eq!(d.paste_objects(&clip).unwrap().0, 1);
        assert_eq!(d.model.art_meshes.len(), n + 1);
        let new = d.model.art_meshes.last().unwrap();
        assert_eq!(new.info.id, "BrowR2");
        assert_eq!(new.info.name, d.model.art_meshes[orig].info.name);
        assert_eq!(new.target, d.model.art_meshes[orig].target, "same parent");
        assert_eq!(d.selection.objects, vec![Selection::ArtMesh(n)]);
        d.prefs.paste_numbered_names = true;
        d.paste_objects(&clip).unwrap();
        let numbered = d.model.art_meshes[n + 1].info.display_name().to_owned();
        assert_ne!(numbered, d.model.art_meshes[orig].info.display_name());
        assert_eq!(d.model.art_meshes[n + 1].info.id, "BrowR3");
        d.undo();
        d.undo();
        assert_eq!(d.model.art_meshes.len(), n, "one undo step per paste");
        d.redo();
        d.save_project(None).unwrap();
        let back = Document::open(&path, &mut Log::default()).unwrap();
        assert_eq!(back.model.art_meshes.len(), n + 1);
        assert!(back.model.art_meshes.iter().any(|a| a.info.id == "BrowR2"));
        // Cut: one step, nothing selected afterwards.
        d.select(Some(Selection::ArtMesh(n)));
        d.cut_objects(true).unwrap().unwrap();
        assert_eq!(d.model.art_meshes.len(), n);
        assert!(d.selection.objects.is_empty());
        d.undo();
        assert_eq!(d.model.art_meshes.len(), n + 1);
    }

    /// Pasting into another open document (object-clipboard.md §4): the
    /// mesh comes with its image, its parent deformer is matched by ID, and
    /// the result saves and reopens.
    #[test]
    fn paste_into_another_document() {
        let (mut a, _) = character("e2e-paste-a");
        let (mut b, path_b) = character("e2e-paste-b");
        let brow = mesh(&a, "BrowR");
        a.select(Some(brow));
        let clip = a.copy_objects(true).unwrap();
        let n = b.model.art_meshes.len();
        let images = b.model.model_images.len();
        let (pasted, warnings) = b.paste_objects(&clip).unwrap();
        assert_eq!((pasted, warnings.len()), (1, 0), "{warnings:?}");
        let new = b.model.art_meshes.last().unwrap();
        assert_eq!(new.model_image, Some(images), "the image was brought along");
        assert_eq!(b.model.model_images.len(), images + 1);
        b.save_project(None).unwrap();
        let back = Document::open(&path_b, &mut Log::default()).unwrap();
        assert_eq!(back.model.art_meshes.len(), n + 1);
    }

    /// Edit ▸ Copy / Paste Parameter Value sets every parameter back.
    #[test]
    fn copy_paste_parameter_values() {
        let (mut d, _) = character("e2e-param-values");
        d.set_manual(0, d.model.parameters[0].max);
        let copied = d.copy_parameter_values();
        d.reset_parameters();
        assert_ne!(d.values[0], d.model.parameters[0].max);
        assert_eq!(d.paste_parameter_values(&copied), d.model.parameters.len());
        assert_eq!(d.values[0], d.model.parameters[0].max);
    }

    /// Show ▸ Guide ▸ Add Guide to Center and guide moves are undoable
    /// model edits that save (view-aids.md §1).
    #[test]
    fn guides_are_saved_with_the_model() {
        let (mut d, path) = character("e2e-guides");
        let g = crate::view_aids::center_guides(&d, keyform_model::Guide::DEFAULT_COLOR);
        assert_eq!(g.len(), 2);
        let (w, h) = (d.model.canvas.width as f32, d.model.canvas.height as f32);
        assert_eq!((g[0].vertical, g[0].position), (false, h / 2.0));
        assert_eq!((g[1].vertical, g[1].position), (true, w / 2.0));
        d.try_edit("Add Guide", |m| {
            m.guides.extend(g);
            Ok(())
        })
        .unwrap();
        d.save_project(None).unwrap();
        let back = Document::open(&path, &mut Log::default()).unwrap();
        assert_eq!(back.model.guides, d.model.guides);
        d.undo();
        assert!(d.model.guides.is_empty());
    }

    /// Animation ▸ Scene ▸ New Scene (main menu) runs through the pending
    /// op, also with the Timeline palette hidden.
    #[test]
    fn animation_menu_new_scene() {
        let (_, path) = character("e2e-anim-menu");
        let mut log = Log::default();
        let mut a = Document::new_animation(&path, &mut log).unwrap();
        let n = a.anim.as_ref().unwrap().doc.animation.scenes.len();
        a.anim.as_mut().unwrap().pending = Some(crate::animation::SceneOp::Add);
        crate::animation::run_pending(&mut a);
        assert_eq!(a.anim.as_ref().unwrap().doc.animation.scenes.len(), n + 1);
        assert!(a.anim.as_ref().unwrap().pending.is_none());
    }

    #[test]
    fn idle_rotation_gizmos_for_unselected_rotations() {
        let (mut d, _) = character("e2e-idle-gizmos");
        let face = mesh(&d, "Face");
        let values = d.values.clone();
        let r = d
            .try_edit("Create Rotation Deformer", |m| {
                let nd = keyform_edit::objects::NewDeformer {
                    name: "Head".into(),
                    part: None,
                    parent: None,
                    children: &[face],
                    values: &values,
                };
                keyform_edit::objects::create_rotation_deformer(m, &nd)
            })
            .unwrap();
        d.tick(0.0, &PlaybackOptions::default(), &mut Log::default());
        d.select(None);
        assert_eq!(d.idle_rotation_gizmos().len(), 1);
        d.select(Some(Selection::Deformer(r)));
        assert!(
            d.idle_rotation_gizmos().is_empty(),
            "the selected one has its widget"
        );
        d.select(None);
        std::sync::Arc::make_mut(&mut d.model.deformers[r])
            .info
            .visible = false;
        assert!(
            d.idle_rotation_gizmos().is_empty(),
            "hidden ones aren't drawn"
        );
        let info = &mut std::sync::Arc::make_mut(&mut d.model.deformers[r]).info;
        info.visible = true;
        info.locked = true;
        assert!(
            d.idle_rotation_gizmos().is_empty(),
            "locked ones aren't drawn"
        );
    }

    /// File ▸ Save as… in an animation saves the .can3 under the new name
    /// (it saved the model). The menu routing is covered by the e2e
    /// session's "Save as model.can3" step.
    #[test]
    fn save_as_in_an_animation_writes_the_can3() {
        let (_, path) = character("e2e-save-as");
        let model_bytes = std::fs::read(&path).unwrap();
        let mut log = Log::default();
        let mut a = Document::new_animation(&path, &mut log).unwrap();
        let to = path.with_file_name("renamed.can3");
        let p = a.save_animation_to(Some(&to)).unwrap();
        assert_eq!(p, to);
        assert_eq!(a.title, "renamed.can3");
        keyform_formats::can3::Can3Document::read(&std::fs::read(&to).unwrap()).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            model_bytes,
            "model untouched"
        );
    }

    /// "Add key at playhead" selects the new key.
    #[test]
    fn add_key_selects_the_new_key() {
        let (_, path) = character("e2e-add-key");
        let mut log = Log::default();
        let mut a = Document::new_animation(&path, &mut log).unwrap();
        let st = a.anim.as_mut().unwrap();
        let p = st.model_track().map(|(_, m)| m.params.len()).unwrap();
        assert!(p > 0);
        st.selected = Some((
            st.model_track().unwrap().0,
            crate::animation::AttrRef::Param(0),
        ));
        st.frame = 30.0;
        let k = st.add_key_at_playhead(Default::default()).unwrap();
        assert_eq!(st.key, Some(k));
        assert_eq!(st.attr().unwrap().curve.keys[k].frame, 30);
    }
}
