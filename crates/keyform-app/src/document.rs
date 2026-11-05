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
}

/// An object being dragged on the canvas: edits are previewed live on top of
/// `base` and committed as one undo step at the end.
struct MoveTarget {
    object: Selection,
    cell: usize,
    /// The parent deformer's map at drag start.
    parent: Option<keyform_eval::DeformerMap>,
    /// Points to move; all if `None`.
    points: Option<Vec<usize>>,
    /// Warp net level when the points are Bezier net points.
    net_level: Option<u8>,
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
const ROTATE_SNAP: f32 = 45.0;

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
}

impl SelectionSet {
    pub fn of(o: Option<Selection>) -> Self {
        Self {
            objects: o.into_iter().collect(),
            points: Vec::new(),
        }
    }
}

/// Is `q` inside the convex quadrilateral `c` (either winding)?
pub fn in_quad(c: &[[f32; 2]; 4], q: [f32; 2]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let (a, b) = (c[i], c[(i + 1) % 4]);
        let d = (b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0]);
        if d != 0.0 {
            if sign != 0.0 && d.signum() != sign {
                return false;
            }
            sign = d.signum();
        }
    }
    true
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
    /// A parameter dialog requested by a palette (taken by the app).
    pub param_request: Option<crate::param_dialog::Request>,
    /// Result of the mask check, recomputed when the model changes
    /// (dialogs.md §21).
    pub mask_issues: keyform_edit::MaskIssues,
    /// History revision at the last save (the unmodified state).
    saved_revision: u64,
    /// Mesh edit mode on one art mesh (main-window.md §6).
    pub mesh_edit: Option<crate::mesh_edit::MeshEdit>,
    /// Mesh editor settings kept between mesh-edit sessions.
    mesh_prefs: Option<crate::mesh_edit::MeshPrefs>,
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
            let doc = Cmo3Document::read(&read(path)?).map_err(|e| e.to_string())?;
            for w in &doc.warnings {
                log.warn(format!("{name}: {w}"));
            }
            model = doc.model.clone();
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
        Ok(Self {
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
            mesh_prefs: None,
            drag: None,
            edit_level: 2,
            notice: None,
            param_request: None,
            mask_issues: keyform_edit::mask_issues(&model),
            mode: if palette_from_children {
                Mode::Modeling
            } else {
                Mode::Viewer
            },
            model,
        })
    }

    /// Model edits are possible only on projects (runtime models cannot be
    /// saved back).
    pub fn editable(&self) -> bool {
        matches!(self.source, Source::Project(_))
    }

    /// Changed since opening or the last save (title asterisk).
    pub fn is_modified(&self) -> bool {
        self.history.revision() != self.saved_revision
    }

    fn edit_state(&self) -> EditState {
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
        if param.kind == keyform_model::ParameterKind::BlendShape {
            return Err("The keyforms for Blend Shape parameters cannot be edited.".into());
        }
        let v = param.clamp(self.shown_value(p));
        let keys = keys_of(&self.model, o, p);
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
                    .filter(|p| p.0 == o)
                    .map(|p| p.1)
                    .collect()
            });
            let net_level = (points && self.uses_net(o)).then_some(self.edit_level);
            targets.push(MoveTarget {
                object: o,
                cell,
                parent,
                points: which,
                net_level,
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
                match (t.net_level, t.points.as_deref()) {
                    (Some(level), Some(which)) => keyform_edit::translate_net(
                        &mut m,
                        t.object,
                        t.cell,
                        t.parent.as_ref(),
                        total,
                        which,
                        level,
                    ),
                    _ => keyform_edit::translate(
                        &mut m,
                        t.object,
                        t.cell,
                        t.parent.as_ref(),
                        total,
                        t.points.as_deref(),
                    ),
                }
                .map(|_| ())
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
                keyform_edit::rotate(&mut m, *target, *cell, delta, shift.then_some(ROTATE_SNAP))
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
            self.model = m;
            self.selection = SelectionSet::default();
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
        let [a] = meshes[..] else {
            return Err("Select one ArtMesh to edit its mesh.".into());
        };
        let s = self.new_mesh_edit(a)?;
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
        let t = self.try_edit("Mesh Edit", |m| {
            let t = keyform_edit::mesh::set_mesh(m, a, &graph).map_err(|e| e.to_string())?;
            if let Some(g) = generator {
                std::sync::Arc::make_mut(&mut m.art_meshes[a]).generator = g;
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
        self.selection.points.clear();
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
    pub fn objects_in(&self, quad: &[[f32; 2]; 4]) -> Vec<Selection> {
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
                .any(|p| in_quad(quad, [p[0], p[1]]))
            {
                out.push(Selection::ArtMesh(mi));
            }
        }
        out
    }

    /// Points of the selected objects inside the canvas quad.
    pub fn points_in(&self, quad: &[[f32; 2]; 4]) -> Vec<(Selection, usize)> {
        let mut out = Vec::new();
        for &o in &self.selection.objects {
            for (i, p) in self.object_points(o).iter().enumerate() {
                if in_quad(quad, *p) {
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
        sel.points.retain(|(s, _)| self.selection_valid(*s));
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
    fn model_changed(&mut self) {
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
        if to.is_some() {
            self.title = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            self.path.clone_from(&path);
        }
        Ok(path)
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
        if self.mode == Mode::Modeling {
            // The model exactly at the slider values, nothing animated.
            self.live.clone_from(&self.values);
            let ov = Overrides {
                part_opacity: vec![None; self.model.parts.len()],
                model_opacity: 1.0,
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
        let ev = Evaluator::new(&self.model);
        self.frame = Some(ev.evaluate_full(&self.live, EvalOptions::default(), &ov));
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
        assert!(in_quad(&q, [5.0, 5.0]));
        assert!(!in_quad(&q, [11.0, 5.0]));
        let rev = [q[3], q[2], q[1], q[0]];
        assert!(in_quad(&rev, [1.0, 9.0]));
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
        });
        assert!(doc.glue_selected_vertices().is_err());
        doc.set_selection(SelectionSet {
            objects: vec![Selection::ArtMesh(0), Selection::ArtMesh(1)],
            points: vec![(Selection::ArtMesh(0), 0), (Selection::ArtMesh(1), 0)],
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
}
