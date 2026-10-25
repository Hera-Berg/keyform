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
use keyform_model::{Model, Texture};
use keyform_physics::PhysicsSim;

use crate::log::Log;

/// Which object the user selected (Parts / Deformer palettes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Selection {
    Part(usize),
    Deformer(usize),
    ArtMesh(usize),
    Glue(usize),
}

/// Animation menu toggles, persisted per user. spec: 70-advanced/viewer.md §3.7
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PlaybackOptions {
    pub expression: bool,
    pub eye_blink: bool,
    pub physics: bool,
    pub pose: bool,
    /// Motion-sync lip sync (§3.1 step 11). TODO(Q-0013): stored only.
    pub motion_sync: bool,
    /// Start a random motion whenever the queue is empty (§3.1 step 2).
    pub idle_motion: bool,
    pub track_cursor: bool,
    pub breath: bool,
    /// "Parameter repeat". TODO(Q-0013): behaviour not specified; stored only.
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

/// The undoable state of a document (spec: 50-ui/undo.md §2). The edit
/// level joins it when edit levels are implemented.
#[derive(Clone)]
pub struct EditState {
    pub model: Model,
    pub selection: Option<Selection>,
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
    pub selection: Option<Selection>,
    pub hit_areas: Vec<(String, String)>,
    /// Parts palette order comes from part child lists (projects) or from
    /// parent links (runtime models, which store no palette order).
    pub palette_from_children: bool,
    pub history: History<EditState>,
    /// History revision at the last save (the unmodified state).
    saved_revision: u64,
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
                    guid: String::new(),
                    name: format!("texture_{i:02}"),
                    width: w,
                    height: h,
                    png: Some(png),
                    locked: true,
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
            },
            selection: None,
            hit_areas,
            palette_from_children,
            history: History::new(EditState {
                model: model.clone(),
                selection: None,
            }),
            saved_revision: 0,
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
            selection: self.selection,
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

    /// Changes the selection as a selection-only undo step (undo.md §3).
    pub fn select(&mut self, sel: Option<Selection>) {
        if sel == self.selection {
            return;
        }
        self.selection = sel;
        self.history
            .commit("Select", StepKind::SelectionOnly, self.edit_state(), false);
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
        self.selection = state.selection.filter(|s| self.selection_valid(*s));
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
        }
    }

    /// Advances playback by `dt` seconds and evaluates the model, in the
    /// frame order of spec 70-advanced/viewer.md §3.1.
    pub fn tick(&mut self, dt: f32, o: &PlaybackOptions, log: &mut Log) {
        self.time += f64::from(dt);
        self.motion_player.force_restricted_beziers = o.old_playback;
        self.expression_player.old_fading = o.old_fading;
        if o.idle_motion && !self.motion_player.is_playing() {
            self.start_idle_motion();
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
        assert_eq!(doc.selection, Some(Selection::ArtMesh(0)));
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
}
