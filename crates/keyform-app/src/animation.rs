//! The Animation workspace: a `.can3` document played on its linked model,
//! with a timeline. spec: 60-animation/animation-model.md §1–5,
//! 50-ui/palettes.md §7–8, 60-animation/motion3-json.md §2

use std::path::{Path, PathBuf};

use eframe::egui;
use keyform_anim::scene::{eval_attr, step_frame};
use keyform_formats::can3::Can3Document;
use keyform_model::animation::{Attr, IntCurve, ModelTrack, Scene, TrackKind};

use crate::document::{Document, Mode};
use crate::log::Log;

/// Which attribute of a model track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrRef {
    Param(usize),
    Part(usize),
}

/// An open animation and its playhead.
pub struct AnimState {
    pub doc: Can3Document,
    pub path: PathBuf,
    pub scene: usize,
    /// Current frame (fractional while playing).
    pub frame: f64,
    pub playing: bool,
    /// Loop the work area while playing.
    pub looping: bool,
    /// The selected attribute: (track, attribute).
    pub selected: Option<(usize, AttrRef)>,
    /// Timeline zoom: screen px per frame.
    pub px_per_frame: f32,
    /// Show only attributes with keys.
    pub keyed_only: bool,
    /// The selected key of the selected attribute.
    pub key: Option<usize>,
    /// Animation snapshots for undo / redo (the Animation workspace's own
    /// steps).
    pub undo: Vec<(String, UndoStep)>,
    pub redo: Vec<(String, UndoStep)>,
    /// A key drag: the animation before it.
    drag: Option<keyform_model::animation::Animation>,
    /// Changed since opening or the last save.
    pub dirty: bool,
    /// Never saved: the first save asks for a location.
    pub untitled: bool,
    /// The scene name being edited (Rename).
    pub rename: Option<String>,
    /// Decoded WAVs of the sound resources, by resource GUID.
    pub sounds: std::collections::HashMap<String, keyform_anim::audio::Pcm>,
    /// Decoded images of the image tracks' resources, by resource GUID.
    pub images:
        std::collections::HashMap<String, std::sync::Arc<keyform_formats::image::RgbaImage>>,
    /// The same WAVs as stored PCM (for the level analysis, §4.1).
    pub raw_sounds: std::collections::HashMap<String, keyform_anim::lipsync_bake::RawWav>,
    /// The sound playing with the playhead.
    pub audio: Option<crate::audio_out::AudioOut>,
    /// Sound was started (or failed to start) for this run of playback.
    audio_tried: bool,
    /// The Scene settings dialog's draft (the scene without its tracks).
    pub settings: Option<Scene>,
    /// The physics baking dialog, while open.
    pub physics_bake: Option<PhysicsBakeDraft>,
    /// The Form Edit tool is active on the canvas.
    pub form_tool: bool,
    /// A Form Edit vertex drag in progress.
    pub form_drag: Option<crate::form_edit::FormDrag>,
    /// The art mesh last form-edited (its vertices are drawn).
    pub form_mesh: Option<usize>,
    pub ctrl_tool: crate::param_ctrl_ui::CtrlTool,
    pub ctrl_drag: Option<crate::param_ctrl_ui::CtrlDrag>,
    /// Controllers with a target follow it during playback.
    pub ctrl_follow: bool,
    /// The text of a new event (Events ▸ Add).
    pub event_text: String,
}

/// Options of [`Document::anim_frame_opts`].
#[derive(Default)]
pub struct FrameOpts {
    /// Model canvas space (no track placement).
    pub unplaced: bool,
    /// Without form animation.
    pub no_form: bool,
    /// Added to one art mesh's local form: (art mesh, offsets).
    pub extra_local: Option<(usize, Vec<f32>)>,
    /// Another scene than the current one (nested scene tracks).
    pub scene: Option<usize>,
    /// Parameter values replacing the animated ones (target follow).
    pub values: Vec<(usize, f32)>,
}

/// One Animation-workspace undo step: the typed animation before a curve
/// or settings edit, or the whole document before a structural edit
/// (scenes, tracks).
pub enum UndoStep {
    Anim(keyform_model::animation::Animation),
    Doc(Box<keyform_formats::can3::Can3Snapshot>),
}

impl AnimState {
    pub fn scene(&self) -> Option<&Scene> {
        self.doc.animation.scenes.get(self.scene)
    }

    /// A resource file: next to the animation by name, else as stored.
    fn resource_path(&self, stored: &str) -> PathBuf {
        let p = PathBuf::from(stored.replace('\\', "/"));
        if let (Some(dir), Some(name)) = (self.path.parent(), p.file_name()) {
            let near = dir.join(name);
            if near.exists() {
                return near;
            }
        }
        p
    }

    /// Decodes the WAVs of the sound tracks' resources (once each).
    /// Decodes the PNGs of the image tracks' resources (once each).
    pub fn load_images(&mut self, log: &mut Log) {
        let wanted: Vec<String> = self
            .doc
            .animation
            .scenes
            .iter()
            .flat_map(|s| s.tracks.iter())
            .filter_map(|t| match &t.kind {
                TrackKind::Image(it) => it.resource.clone(),
                _ => None,
            })
            .collect();
        for g in wanted {
            if self.images.contains_key(&g) {
                continue;
            }
            let Some(r) = self.doc.animation.resources.iter().find(|r| r.guid == g) else {
                continue;
            };
            let path = self.resource_path(&r.path);
            match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|d| {
                    keyform_formats::image::RgbaImage::decode_png(&d).map_err(|e| e.to_string())
                }) {
                Ok(img) => {
                    self.images.insert(g, std::sync::Arc::new(img));
                }
                Err(e) => log.warn(format!("{}: {e}", path.display())),
            }
        }
    }

    pub fn load_sounds(&mut self, log: &mut Log) {
        self.load_images(log);
        let wanted: Vec<String> = self
            .doc
            .animation
            .scenes
            .iter()
            .flat_map(|s| s.tracks.iter())
            .filter_map(|t| match &t.kind {
                TrackKind::Sound(st) => st.resource.clone(),
                _ => None,
            })
            .collect();
        for g in wanted {
            if self.sounds.contains_key(&g) {
                continue;
            }
            let Some(r) = self.doc.animation.resources.iter().find(|r| r.guid == g) else {
                continue;
            };
            let path = self.resource_path(&r.path);
            match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|d| keyform_anim::audio::read_wav(&d).map_err(|e| e.to_string()))
            {
                Ok(pcm) => {
                    if let Ok(raw) = std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|d| {
                            keyform_anim::lipsync_bake::RawWav::parse(&d).map_err(|e| e.to_string())
                        })
                    {
                        self.raw_sounds.insert(g.clone(), raw);
                    }
                    self.sounds.insert(g, pcm);
                }
                Err(e) => log.warn(format!("{}: {e}", path.display())),
            }
        }
    }

    /// Bake Lip-sync from sound track `sound` (an index into the scene's
    /// tracks) into the model track's soundLevel: its level table keyed
    /// every 2nd frame over the sound track, reduced to turning points, with
    /// the scene's default parameter curve type (one undo step). Returns
    /// the number of keys. spec: animation-model.md §4.1
    pub fn bake_lip_sync(&mut self, sound: usize) -> Result<usize, String> {
        let scene = self.scene().ok_or("No scene.")?.clone();
        let (ti, _) = self
            .model_track()
            .map(|(i, m)| (i, m.clone()))
            .ok_or("This scene has no model track.")?;
        let st = scene.tracks.get(sound).ok_or("No such sound track.")?;
        let TrackKind::Sound(snd) = &st.kind else {
            return Err("That track is not a sound track.".into());
        };
        let g = snd
            .resource
            .as_ref()
            .ok_or("The sound track has no sound.")?;
        let raw = self
            .raw_sounds
            .get(g)
            .ok_or("The sound could not be read.")?;
        let table = keyform_anim::lipsync_bake::level_table(raw);
        let model = &scene.tracks[ti];
        // Scene frames → the model track's own frames.
        let shift = model.offset - model.start;
        let start = st.start + shift;
        let end = st.start + st.duration.max(1) - 1 + shift;
        let kind = scene.default_param_curve;
        let before = self.doc.animation.clone();
        let legacy = before.smooth_legacy;
        let TrackKind::Model(mt) = &mut self.doc.animation.scenes[self.scene].tracks[ti].kind
        else {
            return Err("no model track".into());
        };
        let Some(ls) = mt.lip_sync.as_mut() else {
            return Err("The model track has no lip-sync effect.".into());
        };
        let (lo, hi) = keyform_anim::scene::clamp_range(&ls.sound_level);
        let c = &mut ls.sound_level.curve;
        c.fixed = false;
        let o = keyform_anim::lipsync_bake::BakeOptions {
            start,
            end,
            fps: scene.fps,
            kind,
            range: (lo, hi),
            legacy,
        };
        keyform_anim::lipsync_bake::bake(c, &table, &o);
        keyform_anim::scene::refresh(c, lo, hi, legacy);
        let n = c.keys.len();
        self.undo
            .push(("Bake Lip-sync".into(), UndoStep::Anim(before)));
        self.redo.clear();
        self.dirty = true;
        Ok(n)
    }

    /// Replaces the current scene's settings (everything but its tracks)
    /// as one undo step. spec: dialogs.md §5, animation-model.md §1
    pub fn apply_settings(&mut self, s: &Scene) {
        let before = self.doc.animation.clone();
        let Some(cur) = self.doc.animation.scenes.get_mut(self.scene) else {
            return;
        };
        let tracks = std::mem::take(&mut cur.tracks);
        *cur = Scene {
            tracks,
            ..s.clone()
        };
        if self.doc.animation != before {
            self.undo
                .push(("Scene settings".into(), UndoStep::Anim(before)));
            self.redo.clear();
            self.dirty = true;
        }
    }

    /// Adds (or replaces) an event at `frame` on the model track's user
    /// data, or removes the one there (`text` = None). One undo step.
    /// spec: motion3-json.md §2 ("UserData")
    pub fn set_event(&mut self, frame: i32, text: Option<String>) {
        let Some((ti, _)) = self.model_track() else {
            return;
        };
        let before = self.doc.animation.clone();
        let ud = &mut self.doc.animation.scenes[self.scene].tracks[ti].user_data;
        ud.retain(|e| e.0 != frame);
        if let Some(t) = text {
            ud.push((frame, t));
            ud.sort_by_key(|e| e.0);
        }
        if self.doc.animation != before {
            self.undo.push(("Event".into(), UndoStep::Anim(before)));
            self.redo.clear();
            self.dirty = true;
        }
    }

    /// The selected attribute.
    pub fn attr(&self) -> Option<&Attr> {
        let (ti, which) = self.selected?;
        let TrackKind::Model(m) = &self.scene()?.tracks.get(ti)?.kind else {
            return None;
        };
        match which {
            AttrRef::Param(k) => m.params.get(k),
            AttrRef::Part(k) => m.parts.get(k),
        }
    }

    fn attr_mut(&mut self) -> Option<&mut Attr> {
        let (ti, which) = self.selected?;
        let s = self.doc.animation.scenes.get_mut(self.scene)?;
        let TrackKind::Model(m) = &mut s.tracks.get_mut(ti)?.kind else {
            return None;
        };
        match which {
            AttrRef::Param(k) => m.params.get_mut(k),
            AttrRef::Part(k) => m.parts.get_mut(k),
        }
    }

    /// Applies `f` to the selected attribute's curve as one undo step
    /// `name`, then the §3.3 clean-up. Returns whether it applied.
    pub fn edit(
        &mut self,
        name: &str,
        f: impl FnOnce(&mut keyform_model::animation::Curve, f64),
    ) -> bool {
        let before = self.doc.animation.clone();
        let legacy = self.doc.animation.smooth_legacy;
        let frame = self.frame.round();
        let Some(a) = self.attr_mut() else {
            return false;
        };
        let range = keyform_anim::scene::clamp_range(a);
        f(&mut a.curve, frame);
        keyform_anim::scene::refresh(&mut a.curve, range.0, range.1, legacy);
        if self.doc.animation == before {
            return false;
        }
        self.undo.push((name.to_owned(), UndoStep::Anim(before)));
        self.redo.clear();
        self.dirty = true;
        true
    }

    /// Undo (`redo` = false) or redo; returns the step's name.
    /// "Add key at playhead" on the selected attribute: a key with the
    /// curve's current value; the new key becomes the selected key, so its
    /// value can be typed at once. Returns its index.
    pub fn add_key_at_playhead(
        &mut self,
        default_type: keyform_model::animation::CurveType,
    ) -> Option<usize> {
        use keyform_anim::scene::{eval_curve, set_key};
        let a = self.attr()?;
        let v = eval_curve(&a.curve, self.frame);
        let f0 = self.frame.round() as i32;
        self.edit("Add key", |c, f| {
            set_key(c, f as i32, v, default_type);
        });
        self.key = self
            .attr()
            .and_then(|attr| attr.curve.keys.iter().position(|k| k.frame == f0));
        self.key
    }

    pub fn step(&mut self, redo: bool) -> Option<String> {
        let (name, step) = if redo {
            self.redo.pop()?
        } else {
            self.undo.pop()?
        };
        let back = match step {
            UndoStep::Anim(a) => UndoStep::Anim(std::mem::replace(&mut self.doc.animation, a)),
            UndoStep::Doc(s) => {
                let cur = self.doc.snapshot();
                self.doc.restore(*s);
                self.selected = None;
                self.scene = self
                    .scene
                    .min(self.doc.animation.scenes.len().saturating_sub(1));
                self.load_sounds(&mut Log::default());
                UndoStep::Doc(Box::new(cur))
            }
        };
        let to = if redo { &mut self.undo } else { &mut self.redo };
        to.push((name.clone(), back));
        self.dirty = true;
        self.key = None;
        Some(name)
    }

    /// The model track's parameter controllers (empty without one).
    pub fn ctrls(&self) -> &keyform_model::animation::ControllerSet {
        static EMPTY: keyform_model::animation::ControllerSet =
            keyform_model::animation::ControllerSet {
                points: Vec::new(),
                controllers: Vec::new(),
            };
        self.model_track().map_or(&EMPTY, |(_, m)| &m.controllers)
    }

    pub fn ctrls_mut(&mut self) -> Option<&mut keyform_model::animation::ControllerSet> {
        let (ti, _) = self.model_track()?;
        let s = self.scene;
        match &mut self
            .doc
            .animation
            .scenes
            .get_mut(s)?
            .tracks
            .get_mut(ti)?
            .kind
        {
            TrackKind::Model(m) => Some(&mut m.controllers),
            _ => None,
        }
    }

    /// The first model track of scene `si`.
    pub fn model_track_in(&self, si: usize) -> Option<(usize, &ModelTrack)> {
        self.doc
            .animation
            .scenes
            .get(si)?
            .tracks
            .iter()
            .enumerate()
            .find_map(|(i, t)| match &t.kind {
                TrackKind::Model(m) => Some((i, &**m)),
                _ => None,
            })
    }

    /// The first model track of the current scene.
    pub fn model_track(&self) -> Option<(usize, &ModelTrack)> {
        self.scene()?
            .tracks
            .iter()
            .enumerate()
            .find_map(|(i, t)| match &t.kind {
                TrackKind::Model(m) => Some((i, &**m)),
                _ => None,
            })
    }
}

/// The model file a `.can3`'s first model track links to: the stored path,
/// else a file of that name next to the `.can3`.
fn linked_model(doc: &Can3Document, can3: &Path) -> Option<PathBuf> {
    let guid = doc.animation.scenes.iter().find_map(|s| {
        s.tracks.iter().find_map(|t| match &t.kind {
            TrackKind::Model(m) => m.resource.clone(),
            _ => None,
        })
    })?;
    let r = doc.animation.resources.iter().find(|r| r.guid == guid)?;
    let dir = can3.parent().unwrap_or(Path::new("."));
    let stored = PathBuf::from(r.path.replace('\\', "/"));
    let file = stored
        .file_name()
        .map(|f| f.to_owned())
        .or_else(|| Some(std::ffi::OsString::from(&r.name)))?;
    [dir.join(&file), stored].into_iter().find(|p| p.exists())
}

impl Document {
    /// Opens a `.can3` with its linked model in the Animation workspace.
    pub fn open_animation(path: &Path, log: &mut Log) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc = Can3Document::read(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        for w in &doc.warnings {
            log.warn(format!("{name}: {w}"));
        }
        let model = linked_model(&doc, path).ok_or_else(|| {
            format!("{name}: the linked model was not found (put it next to the animation file)")
        })?;
        let mut d = Document::open(&model, log)?;
        d.title = name;
        d.mode = Mode::Animation;
        let frame = doc
            .animation
            .scenes
            .first()
            .map_or(0.0, |s| f64::from(s.work_start));
        let mut doc = doc;
        doc.form_info = Some(form_info(&d.model));
        d.anim = Some(AnimState {
            doc,
            path: path.to_path_buf(),
            scene: 0,
            frame,
            playing: false,
            looping: true,
            selected: None,
            px_per_frame: 6.0,
            keyed_only: true,
            key: None,
            undo: Vec::new(),
            redo: Vec::new(),
            drag: None,
            dirty: false,
            untitled: false,
            rename: None,
            sounds: Default::default(),
            raw_sounds: Default::default(),
            images: Default::default(),
            audio: None,
            audio_tried: false,
            settings: None,
            physics_bake: None,
            form_tool: false,
            form_drag: None,
            form_mesh: None,
            ctrl_tool: Default::default(),
            ctrl_drag: None,
            ctrl_follow: false,
            event_text: String::new(),
        });
        if let Some(a) = d.anim.as_mut() {
            a.load_sounds(log);
        }
        Ok(d)
    }

    /// A new animation on the model `model` (a `.cmo3`) with one scene of
    /// 120 frames at 30 fps; unsaved (the first save asks for a location).
    /// spec: animation-model.md §1–2
    pub fn new_animation(model: &Path, log: &mut Log) -> Result<Self, String> {
        let mut d = Document::open(model, log)?;
        let stem = model
            .file_stem()
            .map_or_else(|| "Animation".into(), |s| s.to_string_lossy().into_owned());
        let path = model.with_file_name(format!("{stem}_animation.can3"));
        let abs = std::fs::canonicalize(model).unwrap_or_else(|_| model.to_path_buf());
        let doc = Can3Document::new_animation(
            &stem,
            &path.to_string_lossy(),
            &d.model,
            &abs.to_string_lossy(),
            "Scene1",
            120,
            30.0,
        )
        .map_err(|e| e.to_string())?;
        d.title = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        d.mode = Mode::Animation;
        let mut doc = doc;
        doc.form_info = Some(form_info(&d.model));
        d.anim = Some(AnimState {
            doc,
            path,
            scene: 0,
            frame: 0.0,
            playing: false,
            looping: true,
            selected: None,
            px_per_frame: 6.0,
            keyed_only: false,
            key: None,
            undo: Vec::new(),
            redo: Vec::new(),
            drag: None,
            dirty: true,
            untitled: true,
            rename: None,
            sounds: Default::default(),
            raw_sounds: Default::default(),
            images: Default::default(),
            audio: None,
            audio_tried: false,
            settings: None,
            physics_bake: None,
            form_tool: false,
            form_drag: None,
            form_mesh: None,
            ctrl_tool: Default::default(),
            ctrl_drag: None,
            ctrl_follow: false,
            event_text: String::new(),
        });
        Ok(d)
    }

    /// Imports a motion3.json as a new scene: its length, frame rate and
    /// fades, the curves on the model track's matching attributes
    /// (parameters and parts by ID, Model Opacity / EyeBlink / LipSync)
    /// and its events. Returns the curves that found no attribute.
    /// spec: motion3-json.md §4
    /// The model parameters physics writes (each once, in rig order).
    pub fn physics_outputs(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for st in self.model.physics.settings.iter().filter(|s| s.enabled) {
            for o in &st.outputs {
                if !out.contains(&o.parameter) {
                    out.push(o.parameter);
                }
            }
        }
        out
    }

    /// Bake Animation from Physics: simulates the scene twice at its frame
    /// rate (the first pass resets the simulation unless the scene loops;
    /// the second continues from it and its values are kept) and writes the
    /// physics outputs `params` (model parameter indices) as keys on the
    /// model track: every frame, or reduced to turning points ("optimal
    /// curves"), with the scene's default parameter curve type. One undo
    /// step; returns the number of keys written.
    /// spec: 60-animation/timeline-editing.md §12
    pub fn bake_physics(&mut self, params: &[usize], every_frame: bool) -> Result<usize, String> {
        let st = self.anim.as_ref().ok_or("No animation is open.")?;
        let scene = st.scene().ok_or("No scene.")?.clone();
        let (ti, m) = st.model_track().ok_or("This scene has no model track.")?;
        if self.model.physics.settings.is_empty() {
            return Err("The model has no physics.".into());
        }
        let track = &scene.tracks[ti];
        let shift = track.offset - track.start;
        let attrs: Vec<(usize, usize)> = params
            .iter()
            .filter_map(|&pi| {
                m.params
                    .iter()
                    .position(|a| param_index(&self.model, a) == Some(pi))
                    .map(|k| (pi, k))
            })
            .collect();
        if attrs.is_empty() {
            return Err("None of the chosen parameters is on the model track.".into());
        }
        let dt = (1.0 / scene.fps) as f32;
        let frames = 0..=scene.duration;
        let mut sim = keyform_physics::PhysicsSim::new(&self.model.physics);
        let mut series = vec![Vec::new(); attrs.len()];
        for pass in 0..2 {
            if pass == 0 && !scene.loop_motion {
                sim.reset(&self.model.physics);
            }
            for f in frames.clone() {
                let (mut values, _) = self.anim_values_at(f64::from(f)).ok_or("no values")?;
                sim.step(&self.model.physics, &self.model.parameters, &mut values, dt);
                if pass == 1 {
                    for (j, &(pi, _)) in attrs.iter().enumerate() {
                        series[j].push(f64::from(values[pi]));
                    }
                }
            }
        }
        let kind = scene.default_param_curve;
        let st = self.anim.as_mut().ok_or("No animation is open.")?;
        let before = st.doc.animation.clone();
        let legacy = before.smooth_legacy;
        let TrackKind::Model(mt) = &mut st.doc.animation.scenes[st.scene].tracks[ti].kind else {
            return Err("no model track".into());
        };
        let mut n = 0;
        for (j, &(_, k)) in attrs.iter().enumerate() {
            let a = &mut mt.params[k];
            let (lo, hi) = keyform_anim::scene::clamp_range(a);
            let v = &series[j];
            let kept: Vec<usize> = if every_frame {
                (0..v.len()).collect()
            } else {
                keyform_anim::lipsync_bake::turning_points(v)
            };
            let c = &mut a.curve;
            c.fixed = false;
            c.keys.clear();
            c.types.clear();
            for i in kept {
                keyform_anim::scene::set_key(c, i as i32 + shift, v[i], kind);
            }
            c.types = vec![kind; c.keys.len()];
            keyform_anim::scene::refresh(c, lo, hi, legacy);
            n += c.keys.len();
        }
        st.undo
            .push(("Bake Animation from Physics".into(), UndoStep::Anim(before)));
        st.redo.clear();
        st.dirty = true;
        Ok(n)
    }

    /// Bake Animation from Motion-sync: analyses sound track `sound` with
    /// the model's motion-sync setting `setting` (motion-sync.md §3) and
    /// writes the rows, resampled from the setting's SampleRate to the
    /// scene's frame rate, as keys on the model track's parameter curves
    /// over the sound (one undo step). Keys inside that range are replaced.
    /// Returns the number of keys written. spec: motion-sync.md §3 step 5
    // TODO(Q-0039): key placement, curve type, scene extension and the
    // lip-sync warning of the reference.
    pub fn bake_motion_sync(&mut self, setting: usize, sound: usize) -> Result<usize, String> {
        use keyform_anim::motionsync::Setting;
        let ms = self
            .model
            .motion_sync
            .get(setting)
            .ok_or("The model has no motion-sync settings.")?;
        let eng = Setting::from_model(ms);
        let st = self.anim.as_ref().ok_or("No animation is open.")?;
        let scene = st.scene().ok_or("No scene.")?.clone();
        let (ti, m) = st.model_track().ok_or("This scene has no model track.")?;
        let track = scene.tracks.get(sound).ok_or("No such sound track.")?;
        let TrackKind::Sound(snd) = &track.kind else {
            return Err("That track is not a sound track.".into());
        };
        let g = snd
            .resource
            .as_ref()
            .ok_or("The sound track has no sound.")?;
        let pcm = st.sounds.get(g).ok_or("The sound could not be read.")?;
        let frames = eng.frames(pcm);
        if frames.is_empty() {
            return Err("The sound is too short.".into());
        }
        let model_track = &scene.tracks[ti];
        let shift = model_track.offset - model_track.start;
        let (start, end) = (track.start, track.start + track.duration.max(1) - 1);
        let attrs: Vec<(String, usize)> = eng
            .cubism
            .iter()
            .filter_map(|c| {
                let pi = self.model.parameters.iter().position(|p| p.id == c.id)?;
                let k = m
                    .params
                    .iter()
                    .position(|a| param_index(&self.model, a) == Some(pi))?;
                Some((c.id.clone(), k))
            })
            .collect();
        if attrs.is_empty() {
            return Err("None of the setting's parameters is on the model track.".into());
        }
        let value = |id: &str, i: usize| frames[i].iter().find(|(p, _)| p == id).map(|x| x.1);
        let rate = f64::from(eng.sample_rate);
        let last = frames.len() - 1;
        let kind = scene.default_param_curve;
        let st = self.anim.as_mut().ok_or("No animation is open.")?;
        let before = st.doc.animation.clone();
        let legacy = before.smooth_legacy;
        let TrackKind::Model(mt) = &mut st.doc.animation.scenes[st.scene].tracks[ti].kind else {
            return Err("no model track".into());
        };
        let mut n = 0;
        for (id, k) in &attrs {
            let a = &mut mt.params[*k];
            let (lo, hi) = keyform_anim::scene::clamp_range(a);
            let c = &mut a.curve;
            c.fixed = false;
            let (from, to) = (start + shift, end + shift);
            c.keys.retain(|key| key.frame < from || key.frame > to);
            for f in start..=end {
                // Scene frame → row position at SampleRate (linear blend of
                // the two nearest rows, as §3 step 3).
                let x = f64::from(f - start) / scene.fps * rate;
                let i0 = (x.floor() as usize).min(last);
                let i1 = (i0 + 1).min(last);
                let t = (x - x.floor()) as f32;
                let (Some(a0), Some(a1)) = (value(id, i0), value(id, i1)) else {
                    continue;
                };
                keyform_anim::scene::set_key(c, f + shift, f64::from(a0 + (a1 - a0) * t), kind);
                n += 1;
            }
            c.types = vec![kind; c.keys.len()];
            keyform_anim::scene::refresh(c, lo, hi, legacy);
        }
        st.undo.push((
            "Bake Animation from Motion-sync".into(),
            UndoStep::Anim(before),
        ));
        st.redo.clear();
        st.dirty = true;
        Ok(n)
    }

    pub fn import_motion(&mut self, path: &Path) -> Result<Vec<String>, String> {
        use keyform_anim::motion::Target;
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let motion =
            keyform_anim::Motion::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let fps = f64::from(motion.fps.max(1.0));
        let end = (f64::from(motion.duration) * fps).round() as i32;
        let name = path.file_name().map_or_else(
            || "Motion".into(),
            |n| {
                n.to_string_lossy()
                    .trim_end_matches(".motion3.json")
                    .to_owned()
            },
        );
        self.anim_structure("Import Motion", |d, m| {
            d.add_scene(&name, m, end + 1, fps).map(|_| ())
        })?;
        let model = self.model.clone();
        let st = self.anim.as_mut().ok_or("No animation is open.")?;
        st.scene = st.doc.animation.scenes.len() - 1;
        let (curves, events) = keyform_anim::motion_import::import_motion(&motion, fps);
        let legacy = st.doc.animation.smooth_legacy;
        let si = st.scene;
        let ti = st
            .model_track()
            .map(|(i, _)| i)
            .ok_or("The new scene has no model track.")?;
        let scene = &mut st.doc.animation.scenes[si];
        scene.fade_in_ms = motion.fade_in.map_or(-1, |s| (s * 1000.0).round() as i32);
        scene.fade_out_ms = motion.fade_out.map_or(-1, |s| (s * 1000.0).round() as i32);
        scene.bezier_restricted = motion.beziers_restricted;
        let track = &mut scene.tracks[ti];
        track.user_data = events;
        let mut missing = Vec::new();
        for c in curves {
            let attr: Option<&mut Attr> = match (c.target, c.id.as_str()) {
                (Target::Model, "Opacity") => track.opacity.as_mut(),
                (Target::Model, id) => match &mut track.kind {
                    TrackKind::Model(m) if id == "EyeBlink" => {
                        m.eye_blink.as_mut().map(|e| &mut e.eye_open)
                    }
                    TrackKind::Model(m) if id == "LipSync" => {
                        m.lip_sync.as_mut().map(|e| &mut e.sound_level)
                    }
                    _ => None,
                },
                (Target::Parameter, id) => match &mut track.kind {
                    TrackKind::Model(m) => m.params.iter_mut().find(|a| {
                        keyform_anim::scene::param_id(&a.id) == Some(id)
                            || a.guid.as_deref().is_some_and(|g| {
                                model.parameters.iter().any(|p| p.guid == g && p.id == id)
                            })
                    }),
                    _ => None,
                },
                (Target::PartOpacity, id) => match &mut track.kind {
                    TrackKind::Model(m) => m
                        .parts
                        .iter_mut()
                        .find(|a| keyform_anim::scene::part_id(&a.id) == Some(id)),
                    _ => None,
                },
            };
            let Some(a) = attr else {
                missing.push(c.id.clone());
                continue;
            };
            let (lo, hi) = keyform_anim::scene::clamp_range(a);
            a.curve.keys = c.curve.keys;
            a.curve.types = c.curve.types;
            keyform_anim::scene::refresh(&mut a.curve, lo, hi, legacy);
        }
        st.dirty = true;
        Ok(missing)
    }

    /// Runs a structural change (scenes, tracks) on the animation. These
    /// aren't undoable yet: the animation's undo steps are cleared.
    pub fn anim_structure(
        &mut self,
        name: &str,
        f: impl FnOnce(&mut Can3Document, &keyform_model::Model) -> keyform_formats::Result<()>,
    ) -> Result<(), String> {
        let model = self.model.clone();
        let st = self.anim.as_mut().ok_or("No animation is open.")?;
        let before = st.doc.snapshot();
        if let Err(e) = f(&mut st.doc, &model) {
            st.doc.restore(before);
            return Err(e.to_string());
        }
        st.undo
            .push((name.to_owned(), UndoStep::Doc(Box::new(before))));
        st.redo.clear();
        st.dirty = true;
        st.selected = None;
        st.key = None;
        st.scene = st
            .scene
            .min(st.doc.animation.scenes.len().saturating_sub(1));
        let mut log = Log::default();
        st.load_sounds(&mut log);
        if let Some((_, msg)) = log.entries.last() {
            self.notice = Some(msg.clone());
        }
        Ok(())
    }

    /// Parameter values and part opacities of the current frame: each
    /// model parameter takes its attribute's value (frame-stepped by its
    /// group, else the track), with eye blink / lip sync. spec: §3.1a, §4, §5
    pub fn anim_values_at(&self, frame: f64) -> Option<(Vec<f32>, Vec<Option<f32>>)> {
        self.anim_values_in(self.anim.as_ref()?.scene, frame)
    }

    /// [`Self::anim_values_at`] for scene `si`.
    pub fn anim_values_in(&self, si: usize, frame: f64) -> Option<(Vec<f32>, Vec<Option<f32>>)> {
        let st = self.anim.as_ref()?;
        let scene = st.doc.animation.scenes.get(si)?;
        let (ti, m) = st.model_track_in(si)?;
        let track = &scene.tracks[ti];
        let x = frame - f64::from(track.start) + f64::from(track.offset);
        let model = &self.model;
        let mut values: Vec<f32> = model.parameters.iter().map(|p| p.default).collect();
        let group_step = |pi: usize| -> Option<&IntCurve> {
            let g = model.parameters.get(pi)?.group?;
            let guid = &model.parameter_groups.get(g)?.guid;
            m.groups
                .iter()
                .find(|x| &x.guid == guid)?
                .frame_step
                .as_ref()
        };
        for (k, a) in m.params.iter().enumerate() {
            let Some(pi) = param_index(model, a) else {
                continue;
            };
            let step = group_step(pi).or(track.frame_step.as_ref());
            let xf = step_frame(x, step);
            values[pi] =
                model.parameters[pi].clamp(keyform_anim::scene::param_value(m, k, xf) as f32);
        }
        let mut parts = vec![None; model.parts.len()];
        for a in &m.parts {
            let id = keyform_anim::scene::part_id(&a.id);
            let pi = model.parts.iter().position(|p| {
                a.guid.as_deref() == Some(p.info.guid.as_str()) || Some(p.info.id.as_str()) == id
            });
            if let Some(pi) = pi {
                parts[pi] = Some(eval_attr(a, step_frame(x, track.frame_step.as_ref())) as f32);
            }
        }
        Some((values, parts))
    }

    /// The model track's group opacity at scene frame `frame`: the track is
    /// drawn offscreen and composited with it. spec: animation-model.md §5.1
    pub fn anim_group_opacity(&self, frame: f64) -> f32 {
        self.anim
            .as_ref()
            .map_or(1.0, |a| self.anim_group_opacity_in(a.scene, frame))
    }

    /// [`Self::anim_group_opacity`] for scene `si`.
    pub fn anim_group_opacity_in(&self, si: usize, frame: f64) -> f32 {
        let Some(st) = self.anim.as_ref() else {
            return 1.0;
        };
        let (Some((ti, _)), Some(s)) = (st.model_track_in(si), st.doc.animation.scenes.get(si))
        else {
            return 1.0;
        };
        let track = &s.tracks[ti];
        let x = frame - f64::from(track.start) + f64::from(track.offset);
        keyform_anim::scene::placement(track, x).opacity
    }

    /// The evaluated model at scene frame `frame`, placed in the scene by
    /// its track's visual effect (positions in scene pixels; the track's
    /// opacity is applied when compositing, [`Self::anim_group_opacity`]).
    /// spec: animation-model.md §5
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn anim_frame_at(&self, frame: f64) -> Option<keyform_eval::Frame> {
        self.anim_frame_opts(frame, &FrameOpts::default())
    }

    /// [`Self::anim_frame_at`] with options (form edit needs the frame
    /// before placement, without form animation, or with an extra local
    /// offset).
    pub fn anim_frame_opts(&self, frame: f64, o: &FrameOpts) -> Option<keyform_eval::Frame> {
        let st = self.anim.as_ref()?;
        let si = o.scene.unwrap_or(st.scene);
        let (mut values, parts) = self.anim_values_in(si, frame)?;
        for &(pi, v) in &o.values {
            if let Some(x) = values.get_mut(pi) {
                *x = self.model.parameters[pi].clamp(v);
            }
        }
        let (ti, _) = st.model_track_in(si)?;
        let track = &st.doc.animation.scenes.get(si)?.tracks[ti];
        let x = frame - f64::from(track.start) + f64::from(track.offset);
        let pl = keyform_anim::scene::placement(track, x);
        let mut ov = keyform_eval::Overrides {
            part_opacity: parts,
            model_opacity: 1.0,
            ..Default::default()
        };
        // Form animation, Adjustment Mode 2: D added to the object's local
        // (interpolated) form, before its deformer.
        // spec: form-animation-and-controllers.md §1 step 3
        if let TrackKind::Model(m) = &track.kind {
            if m.form.apply && !o.no_form {
                let fx = step_frame(x, track.frame_step.as_ref());
                for o in &m.form.local {
                    let Some(d) = keyform_anim::scene::form_difference(o, fx) else {
                        continue;
                    };
                    if let Some(mi) = self
                        .model
                        .art_meshes
                        .iter()
                        .position(|a| a.info.guid == o.guid)
                    {
                        ov.local_mesh.insert(mi, d);
                    } else if let Some(di) = self
                        .model
                        .deformers
                        .iter()
                        .position(|w| w.info.guid == o.guid)
                    {
                        ov.local_warp.insert(di, d);
                    }
                }
            }
        }
        if let Some((mi, d)) = &o.extra_local {
            let e = ov
                .local_mesh
                .entry(*mi)
                .or_insert_with(|| vec![0.0; d.len()]);
            if e.len() == d.len() {
                for (a, b) in e.iter_mut().zip(d) {
                    *a += b;
                }
            }
        }
        let mut f = keyform_eval::Evaluator::new(&self.model).evaluate_full(
            &values,
            keyform_eval::EvalOptions::default(),
            &ov,
        );
        // Form animation, Adjustment Mode 1: D added to the object's final
        // canvas form (form-animation-and-controllers.md §1 step 3).
        if let TrackKind::Model(m) = &track.kind {
            if m.form.apply && !o.no_form {
                let fx = step_frame(x, track.frame_step.as_ref());
                for o in &m.form.global {
                    let Some(mi) = self
                        .model
                        .art_meshes
                        .iter()
                        .position(|a| a.info.guid == o.guid)
                    else {
                        continue;
                    };
                    let Some(d) = keyform_anim::scene::form_difference(o, fx) else {
                        continue;
                    };
                    let pos = &mut f.meshes[mi].positions;
                    if d.len() == pos.len() {
                        for (p, dv) in pos.iter_mut().zip(d) {
                            *p += dv;
                        }
                    }
                }
            }
        }
        if !o.unplaced && pl != keyform_anim::scene::Placement::IDENTITY {
            let place = |pos: &mut Vec<f32>| {
                for q in pos.chunks_exact_mut(2) {
                    let p = pl.apply([q[0], q[1]]);
                    q[0] = p[0];
                    q[1] = p[1];
                }
            };
            for m in &mut f.meshes {
                place(&mut m.positions);
            }
            for p in f.pre_glue.values_mut() {
                place(p);
            }
        }
        Some(f)
    }

    /// The canvas the view frames: the scene canvas in the Animation
    /// workspace, else the model canvas.
    pub fn stage_size(&self) -> (f32, f32) {
        if self.mode == Mode::Animation {
            if let Some(s) = self.anim.as_ref().and_then(|a| a.scene()) {
                if s.canvas.0 > 0 && s.canvas.1 > 0 {
                    return (s.canvas.0 as f32, s.canvas.1 as f32);
                }
            }
        }
        (
            self.model.canvas.width as f32,
            self.model.canvas.height as f32,
        )
    }

    /// Advances the playhead while playing (work area, looping).
    pub fn anim_tick(&mut self, dt: f32) {
        let Some(st) = self.anim.as_mut() else { return };
        let Some(s) = st.scene() else { return };
        let (a, b, fps) = (f64::from(s.work_start), f64::from(s.work_end), s.fps);
        let (lo, hi) = (f64::from(s.start_frame), f64::from(s.duration));
        let mut wrapped = false;
        if st.playing {
            st.frame += f64::from(dt) * fps;
            if st.frame > b {
                if st.looping {
                    st.frame = a + (st.frame - b) % (b - a + 1.0).max(1.0);
                    wrapped = true;
                } else {
                    st.frame = b;
                    st.playing = false;
                }
            }
        }
        // Sound tracks play with the playhead (restarted on a loop).
        if !st.playing || wrapped {
            st.audio = None;
            st.audio_tried = false;
        }
        if st.playing && !st.audio_tried && !st.sounds.is_empty() {
            st.audio_tried = true;
            const RATE: u32 = 44100;
            if let Some(mix) = crate::audio_out::mix_scene(st, RATE) {
                let start = (st.frame / fps * f64::from(RATE)) as usize;
                // No device (or an unsupported one): playback stays silent.
                st.audio =
                    crate::audio_out::AudioOut::play(std::sync::Arc::new(mix), RATE, start).ok();
            }
        }
        st.frame = st.frame.clamp(lo, hi.max(lo));
    }

    /// Saves the animation's `.can3` (edited curves in place), atomically;
    /// `to` saves under a new path (the first save of a new animation).
    pub fn save_animation_to(&mut self, to: Option<&Path>) -> Result<PathBuf, String> {
        if let Some(p) = to {
            let st = self.anim.as_mut().ok_or("No animation is open.")?;
            st.doc
                .set_file(&p.to_string_lossy())
                .map_err(|e| e.to_string())?;
            st.path = p.to_path_buf();
            st.untitled = false;
            self.title = p
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        }
        self.save_animation()
    }

    /// Saves the animation's `.can3` (edited curves in place), atomically.
    pub fn save_animation(&mut self) -> Result<PathBuf, String> {
        let st = self.anim.as_mut().ok_or("No animation is open.")?;
        let bytes = st
            .doc
            .save_bytes()
            .map_err(|e| format!("{}: {e}", st.path.display()))?;
        keyform_formats::cmo3::write_atomic(&st.path, &bytes)
            .map_err(|e| format!("{}: {e}", st.path.display()))?;
        st.dirty = false;
        Ok(st.path.clone())
    }

    /// Exports the scenes marked for export as motion3.json into `dir`.
    /// Writes each scene with a model track as `<scene>.exp3.json` (keyed
    /// parameters at the first frame, Add). spec: viewer.md §1.1
    pub fn export_expressions(&self, dir: &Path) -> Result<Vec<PathBuf>, String> {
        let st = self.anim.as_ref().ok_or("No animation is open.")?;
        let mut out = Vec::new();
        for s in &st.doc.animation.scenes {
            let Some(ti) = s
                .tracks
                .iter()
                .position(|t| matches!(t.kind, TrackKind::Model(_)))
            else {
                continue;
            };
            let default_of = |id: &str| {
                self.model
                    .parameters
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.default)
            };
            let Some(e) = keyform_anim::expression::from_scene(s, ti, &default_of) else {
                continue;
            };
            let path = dir.join(format!("{}.exp3.json", s.name));
            let text =
                keyform_anim::json::to_string(&keyform_anim::expression::scene_exp3_json(&e));
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
            out.push(path);
        }
        Ok(out)
    }

    pub fn export_motions(&self, dir: &Path) -> Result<Vec<PathBuf>, String> {
        use keyform_anim::motion_export::{export_motion, ExportOptions};
        let st = self.anim.as_ref().ok_or("No animation is open.")?;
        let model = &self.model;
        let o = ExportOptions {
            legacy_smooth: st.doc.animation.smooth_legacy,
            ..Default::default()
        };
        let mut out = Vec::new();
        for s in st.doc.animation.scenes.iter().filter(|s| s.export_motion) {
            let Some(ti) = s
                .tracks
                .iter()
                .position(|t| matches!(t.kind, TrackKind::Model(_)))
            else {
                continue;
            };
            // IDs from the model when the attribute's GUID resolves.
            let id_of = |a: &Attr| -> Option<String> {
                let g = a.guid.as_deref()?;
                model
                    .parameters
                    .iter()
                    .find(|p| p.guid == g)
                    .map(|p| p.id.clone())
                    .or_else(|| {
                        model
                            .parts
                            .iter()
                            .find(|p| p.info.guid == g)
                            .map(|p| p.info.id.clone())
                    })
            };
            let Some(m) = export_motion(s, ti, &o, &|_| None, &id_of) else {
                continue;
            };
            let p = dir.join(format!("{}.motion3.json", s.name));
            std::fs::write(&p, keyform_anim::json::to_string(&m.to_json()))
                .map_err(|e| format!("{}: {e}", p.display()))?;
            out.push(p);
        }
        Ok(out)
    }
}

/// The model parameter an attribute animates: by GUID, else by ID.
/// The model objects form keys can be saved for (art meshes and warp
/// deformers), by GUID. spec: form-animation-and-controllers.md §3.1
pub fn form_info(m: &keyform_model::Model) -> keyform_formats::can3_build::FormInfo {
    use keyform_formats::can3_build::{FormTarget, FormTargetKind};
    use keyform_model::DeformerKind;
    let parent = |t: Option<usize>| {
        t.and_then(|di| m.deformers.get(di)).map(|d| {
            let class = match d.kind {
                DeformerKind::Warp(_) => "CWarpDeformerSource",
                DeformerKind::Rotation(_) => "CRotationDeformerSource",
            };
            (d.info.guid.clone(), class.to_owned())
        })
    };
    let mut out = keyform_formats::can3_build::FormInfo::new();
    for a in &m.art_meshes {
        out.insert(
            a.info.guid.clone(),
            FormTarget {
                id: a.info.id.clone(),
                name: a.info.name.clone(),
                kind: FormTargetKind::ArtMesh {
                    indices: a.indices.clone(),
                    draw_order: 500,
                },
                parent: parent(a.target),
            },
        );
    }
    for d in &m.deformers {
        if let DeformerKind::Warp(w) = &d.kind {
            out.insert(
                d.info.guid.clone(),
                FormTarget {
                    id: d.info.id.clone(),
                    name: d.info.name.clone(),
                    kind: FormTargetKind::Warp {
                        col: w.cols,
                        row: w.rows,
                    },
                    parent: parent(d.target),
                },
            );
        }
    }
    out
}

fn param_index(m: &keyform_model::Model, a: &Attr) -> Option<usize> {
    if let Some(g) = a.guid.as_deref() {
        if let Some(i) = m.parameters.iter().position(|p| p.guid == g) {
            return Some(i);
        }
    }
    let id = keyform_anim::scene::param_id(&a.id)?;
    m.parameters.iter().position(|p| p.id == id)
}

const LABEL_W: f32 = 180.0;
const ROW_H: f32 = 18.0;

/// The timeline: scene, transport, ruler with work area and playhead, and
/// one row of keys per attribute. spec: palettes.md §7
/// Scene settings: name, size, frame rate, length, work area, fades,
/// curve defaults, target. spec: dialogs.md §5, animation-model.md §1
fn settings_dialog(ctx: &egui::Context, st: &mut AnimState) {
    use keyform_model::animation::CurveType;
    let Some(mut s) = st.settings.take() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new(crate::i18n::tr("Scene settings"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Grid::new("scene-settings")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label(crate::i18n::tr("Scene Name"));
                    ui.text_edit_singleline(&mut s.name);
                    ui.end_row();
                    ui.label(crate::i18n::tr("Size"));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut s.canvas.0)
                                .range(16..=16384)
                                .suffix(" px"),
                        );
                        ui.label(crate::i18n::tr("×"));
                        ui.add(
                            egui::DragValue::new(&mut s.canvas.1)
                                .range(16..=16384)
                                .suffix(" px"),
                        );
                    });
                    ui.end_row();
                    ui.label(crate::i18n::tr("Frame rate"));
                    ui.add(
                        egui::DragValue::new(&mut s.fps)
                            .range(1.0..=240.0)
                            .suffix(" fps"),
                    );
                    ui.end_row();
                    ui.label(crate::i18n::tr("Last frame"));
                    ui.add(egui::DragValue::new(&mut s.duration).range(1..=100_000));
                    ui.end_row();
                    ui.label(crate::i18n::tr("Work area"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut s.work_start).range(0..=s.duration));
                        ui.label(crate::i18n::tr("–"));
                        ui.add(egui::DragValue::new(&mut s.work_end).range(0..=s.duration));
                    });
                    ui.end_row();
                    ui.label(crate::i18n::tr("Fade in / out (ms, −1 = unset)"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut s.fade_in_ms).range(-1..=60_000));
                        ui.add(egui::DragValue::new(&mut s.fade_out_ms).range(-1..=60_000));
                    });
                    ui.end_row();
                    ui.label(crate::i18n::tr(""));
                    ui.checkbox(&mut s.loop_motion, crate::i18n::tr("Loop motion"));
                    ui.end_row();
                    ui.label(crate::i18n::tr(""));
                    ui.checkbox(
                        &mut s.bezier_restricted,
                        crate::i18n::tr("Restriction for SDK (handles move vertically only)"),
                    );
                    ui.end_row();
                    ui.label(crate::i18n::tr(""));
                    ui.checkbox(
                        &mut s.export_motion,
                        crate::i18n::tr("Include in motion export"),
                    );
                    ui.end_row();
                    let ct = |ui: &mut egui::Ui, id: &str, v: &mut CurveType| {
                        egui::ComboBox::from_id_salt(id)
                            .selected_text(v.name())
                            .show_ui(ui, |ui| {
                                for (n, t) in CurveType::NAMES {
                                    ui.selectable_value(v, t, n);
                                }
                            });
                    };
                    ui.label(crate::i18n::tr("New parameter keys"));
                    ct(ui, "set-param-curve", &mut s.default_param_curve);
                    ui.end_row();
                    ui.label(crate::i18n::tr("New part keys"));
                    ct(ui, "set-part-curve", &mut s.default_part_curve);
                    ui.end_row();
                    ui.label(crate::i18n::tr("Target"));
                    let label = match s.target.as_str() {
                        "FOR_VIDEO" => "Video",
                        "FOR_UNITY_SDK" => "SDK (Unity)",
                        "FOR_AE_PLUGIN" => "AE plug-in",
                        _ => "SDK (Others)",
                    };
                    egui::ComboBox::from_id_salt("set-target")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for (v, n) in [
                                ("FOR_VIDEO", "Video"),
                                ("FOR_UNITY_SDK", "SDK (Unity)"),
                                ("FOR_SDK", "SDK (Others)"),
                                ("FOR_AE_PLUGIN", "AE plug-in"),
                            ] {
                                ui.selectable_value(&mut s.target, v.to_owned(), n);
                            }
                        });
                    ui.end_row();
                });
            ui.separator();
            ui.horizontal(|ui| {
                ok = ui.button(crate::i18n::tr("OK")).clicked();
                cancel = ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
    s.work_end = s.work_end.min(s.duration).max(s.work_start);
    if ok {
        st.apply_settings(&s);
    } else if open && !cancel {
        st.settings = Some(s);
    }
}

/// A structural change chosen in the timeline header.
enum SceneOp {
    Add,
    Rename(String),
    Delete,
    AddTrack,
    AddSound(PathBuf),
    AddImage(PathBuf),
    AddSceneTrack(usize),
    BakeLipSync(usize),
    BakeMotionSync(usize, usize),
    ImportMotion(PathBuf),
    OpenPhysicsBake,
    DeleteFormEdit(usize),
    BakePhysics(Vec<usize>, bool),
}

/// The "Settings of Baking Animation from Physics" dialog's draft:
/// (parameter index, name, chosen) per physics output, and the key
/// insertion method. spec: timeline-editing.md §12
pub struct PhysicsBakeDraft {
    pub params: Vec<(usize, String, bool)>,
    pub every_frame: bool,
}

fn physics_bake_dialog(ctx: &egui::Context, st: &mut AnimState) -> Option<SceneOp> {
    let d = st.physics_bake.as_mut()?;
    let mut open = true;
    let mut op = None;
    let mut close = false;
    egui::Window::new(crate::i18n::tr("Settings of Baking Animation from Physics"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(crate::i18n::tr("Parameters to apply physics to:"));
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Select all")).clicked() {
                    d.params.iter_mut().for_each(|p| p.2 = true);
                }
                if ui.button(crate::i18n::tr("Unselect all")).clicked() {
                    d.params.iter_mut().for_each(|p| p.2 = false);
                }
            });
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for (_, name, on) in &mut d.params {
                        ui.checkbox(on, name.as_str());
                    }
                });
            ui.separator();
            ui.label(crate::i18n::tr("Keyframe Insertion Method"));
            ui.radio_value(
                &mut d.every_frame,
                false,
                "Automatically create optimal curves",
            );
            ui.radio_value(
                &mut d.every_frame,
                true,
                crate::i18n::tr("Insert key to every frame"),
            );
            ui.label(crate::i18n::tr(
                "Frame rate used for baking: the scene frame rate.",
            ));
            ui.separator();
            ui.horizontal(|ui| {
                let chosen: Vec<usize> = d.params.iter().filter(|p| p.2).map(|p| p.0).collect();
                if ui
                    .add_enabled(!chosen.is_empty(), egui::Button::new(crate::i18n::tr("OK")))
                    .clicked()
                {
                    op = Some(SceneOp::BakePhysics(chosen, d.every_frame));
                    close = true;
                }
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    close = true;
                }
            });
        });
    if close || !open {
        st.physics_bake = None;
    }
    op
}

pub fn timeline_ui(ui: &mut egui::Ui, doc: &mut Document, tokens: &crate::theme::Tokens) {
    let mut op = None;
    let mut ctrl_op = None;
    let param_ids: Vec<String> = doc.model.parameters.iter().map(|p| p.id.clone()).collect();
    let doc_motion_sync_names: Vec<String> = doc
        .model
        .motion_sync
        .iter()
        .map(|s| s.name.clone())
        .collect();
    if let Some(st) = doc.anim.as_mut() {
        ui.horizontal(|ui| {
            ui.menu_button(crate::i18n::tr("Scene"), |ui| {
                if ui.button(crate::i18n::tr("Scene settings…")).clicked() {
                    st.settings = st.scene().map(|s| Scene { tracks: Vec::new(), ..s.clone() });
                    ui.close();
                }
                if ui.button(crate::i18n::tr("Create new scene")).clicked() {
                    op = Some(SceneOp::Add);
                    ui.close();
                }
                if ui.button(crate::i18n::tr("Rename…")).clicked() {
                    st.rename = st.scene().map(|s| s.name.clone());
                    ui.close();
                }
                if ui
                    .add_enabled(
                        st.doc.animation.scenes.len() > 1,
                        egui::Button::new(crate::i18n::tr("Delete scene")),
                    )
                    .clicked()
                {
                    op = Some(SceneOp::Delete);
                    ui.close();
                }
                ui.separator();
                if ui.button(crate::i18n::tr("Add model track")).clicked() {
                    op = Some(SceneOp::AddTrack);
                    ui.close();
                }
                if ui
                    .button(crate::i18n::tr("Add sound track (WAV)…"))
                    .on_hover_text(crate::i18n::tr("Currently only wav files are supported (16 bit or less PCM, 44100 Hz recommended). Use Bake Lip-sync to drive the mouth from it."))
                    .clicked()
                {
                    ui.close();
                    if let Some(p) = crate::file_dialog::FileDialog::new().add_filter("WAV", &["wav"]).pick_file() {
                        op = Some(SceneOp::AddSound(p));
                    }
                }
                let others: Vec<(usize, String)> = st
                    .doc
                    .animation
                    .scenes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != st.scene)
                    .map(|(i, s)| (i, s.name.clone()))
                    .collect();
                ui.add_enabled_ui(!others.is_empty(), |ui| {
                    ui.menu_button(crate::i18n::tr("Add scene track"), |ui| {
                        for (i, name) in &others {
                            if ui.button(name).clicked() {
                                op = Some(SceneOp::AddSceneTrack(*i));
                                ui.close();
                            }
                        }
                    });
                });
                if ui.button(crate::i18n::tr("Add image track…")).clicked() {
                    ui.close();
                    if let Some(p) = crate::file_dialog::FileDialog::new().add_filter("Image", &["png"]).pick_file() {
                        op = Some(SceneOp::AddImage(p));
                    }
                }
                if ui.button(crate::i18n::tr("Import motion3.json…")).clicked() {
                    ui.close();
                    if let Some(p) = crate::file_dialog::FileDialog::new().add_filter("Motion", &["json"]).pick_file() {
                        op = Some(SceneOp::ImportMotion(p));
                    }
                }
                ui.separator();
                let sound_tracks: Vec<(usize, String)> = st
                    .scene()
                    .map(|s| {
                        s.tracks
                            .iter()
                            .enumerate()
                            .filter(|(_, t)| matches!(t.kind, TrackKind::Sound(_)))
                            .map(|(i, t)| (i, t.name.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                if ui.button(crate::i18n::tr("Bake Animation from Physics…")).clicked() {
                    op = Some(SceneOp::OpenPhysicsBake);
                    ui.close();
                }
                let ms_settings = &doc_motion_sync_names;
                ui.add_enabled_ui(!sound_tracks.is_empty() && !ms_settings.is_empty(), |ui| {
                    ui.menu_button(crate::i18n::tr("Bake Animation from Motion-sync"), |ui| {
                        for (si, sname) in ms_settings.iter().enumerate() {
                            for (i, name) in &sound_tracks {
                                if ui.button(format!("{sname} ← {name}")).clicked() {
                                    op = Some(SceneOp::BakeMotionSync(si, *i));
                                    ui.close();
                                }
                            }
                        }
                    });
                });
                ui.add_enabled_ui(!sound_tracks.is_empty(), |ui| {
                    ui.menu_button(crate::i18n::tr("Bake Lip-sync"), |ui| {
                        for (i, name) in &sound_tracks {
                            if ui.button(name).on_hover_text(crate::i18n::tr("Write this sound's level as keys on the lip-sync volume")).clicked() {
                                op = Some(SceneOp::BakeLipSync(*i));
                                ui.close();
                            }
                        }
                    });
                });
            });
            // Form Edit (form-animation-and-controllers.md §1).
            let (ti, mode) = match st.model_track() {
                Some((ti, m)) => (Some(ti), Some((m.form.local_mode, m.form.apply))),
                None => (None, None),
            };
            if let (Some(ti), Some((mut local, mut apply))) = (ti, mode) {
                ui.toggle_value(&mut st.form_tool, crate::i18n::tr("Form Edit"))
                    .on_hover_text(crate::i18n::tr("Drag art mesh vertices on the canvas to make a form key at the playhead"));
                let (l0, a0) = (local, apply);
                egui::ComboBox::from_id_salt("form_mode")
                    .selected_text(if local { "Adjustment Mode 2" } else { "Adjustment Mode 1" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut local, false, crate::i18n::tr("Adjustment Mode 1"))
                            .on_hover_text(crate::i18n::tr("Added to the final form"));
                        ui.selectable_value(&mut local, true, crate::i18n::tr("Adjustment Mode 2"))
                            .on_hover_text(crate::i18n::tr("Added to the local form; the deformer still moves it"));
                    });
                ui.checkbox(&mut apply, crate::i18n::tr("Apply form edit"));
                if (local, apply) != (l0, a0) {
                    let before = st.doc.animation.clone();
                    let s = st.scene;
                    if let TrackKind::Model(m) = &mut st.doc.animation.scenes[s].tracks[ti].kind {
                        m.form.local_mode = local;
                        m.form.apply = apply;
                    }
                    st.undo.push(("Form Edit settings".into(), UndoStep::Anim(before)));
                    st.redo.clear();
                    st.dirty = true;
                }
                if let Some(mi) = st.form_mesh {
                    if ui.button(crate::i18n::tr("Delete Form Edit")).clicked() {
                        op = Some(SceneOp::DeleteFormEdit(mi));
                    }
                }
            }
            ui.menu_button(crate::i18n::tr("Controllers"), |ui| {
                if let Some(o) = crate::param_ctrl_ui::menu(ui, st, &param_ids) {
                    ctrl_op = Some(o);
                }
            });
            ui.menu_button(crate::i18n::tr("Events"), |ui| {
                let f = st.frame.round() as i32;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut st.event_text).hint_text(crate::i18n::tr("event text")).desired_width(140.0));
                    if ui.button(format!("Add at frame {f}")).clicked() && !st.event_text.is_empty() {
                        let t = std::mem::take(&mut st.event_text);
                        st.set_event(f, Some(t));
                    }
                });
                let events = st.model_track().map(|(ti, _)| st.scene().map(|s| s.tracks[ti].user_data.clone()).unwrap_or_default()).unwrap_or_default();
                if events.is_empty() {
                    ui.weak(crate::i18n::tr("No events."));
                }
                for (ef, text) in events {
                    ui.horizontal(|ui| {
                        if ui.small_button(crate::i18n::tr("✕")).on_hover_text(crate::i18n::tr("Delete Event")).clicked() {
                            st.set_event(ef, None);
                        }
                        if ui.link(format!("{ef}: {text}")).clicked() {
                            st.frame = f64::from(ef);
                            st.playing = false;
                        }
                    });
                }
            });
            if let Some(name) = st.rename.as_mut() {
                let r = ui.add(egui::TextEdit::singleline(name).desired_width(140.0));
                if ui.button(crate::i18n::tr("OK")).clicked()
                    || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    op = Some(SceneOp::Rename(name.clone()));
                }
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    st.rename = None;
                }
            }
        });
    }
    if let Some(o) = ctrl_op {
        if let Err(e) = crate::param_ctrl_ui::run_file_op(doc, o) {
            doc.notice = Some(e);
        }
    }
    if let Some(st) = doc.anim.as_mut() {
        settings_dialog(ui.ctx(), st);
        if let Some(o) = physics_bake_dialog(ui.ctx(), st) {
            op = Some(o);
        }
    }
    if let Some(op) = op {
        let scene = doc.anim.as_ref().map_or(0, |a| a.scene);
        let frames = doc
            .anim
            .as_ref()
            .and_then(|a| a.scene())
            .map_or(120, |s| s.duration);
        let fps = doc
            .anim
            .as_ref()
            .and_then(|a| a.scene())
            .map_or(30.0, |s| s.fps);
        let n = doc
            .anim
            .as_ref()
            .map_or(0, |a| a.doc.animation.scenes.len());
        let r = match op {
            SceneOp::Add => doc.anim_structure("Add Scene", |d, m| {
                d.add_scene(&format!("Scene{}", n + 1), m, frames, fps)
                    .map(|_| ())
            }),
            SceneOp::Rename(name) => {
                doc.anim_structure("Rename Scene", |d, _| d.rename_scene(scene, &name))
            }
            SceneOp::Delete => doc.anim_structure("Delete Scene", |d, _| d.delete_scene(scene)),
            SceneOp::AddTrack => {
                doc.anim_structure("Add Track", |d, m| d.add_model_track(scene, m, &m.name))
            }
            SceneOp::AddSound(p) => {
                let abs = std::fs::canonicalize(&p).unwrap_or(p);
                match std::fs::read(&abs)
                    .map_err(|e| e.to_string())
                    .and_then(|b| {
                        keyform_anim::lipsync_bake::RawWav::parse(&b).map_err(|e| e.to_string())
                    }) {
                    Ok(w) => doc.anim_structure("Add Sound Track", |d, _| {
                        d.add_sound_track(scene, &abs.to_string_lossy(), w.duration())
                    }),
                    Err(e) => Err(e),
                }
            }
            SceneOp::AddSceneTrack(n) => {
                doc.anim_structure("Add Scene Track", |d, _| d.add_scene_track(scene, n))
            }
            SceneOp::AddImage(p) => {
                let abs = std::fs::canonicalize(&p).unwrap_or(p);
                match std::fs::read(&abs)
                    .map_err(|e| e.to_string())
                    .and_then(|b| {
                        keyform_formats::image::RgbaImage::decode_png(&b).map_err(|e| e.to_string())
                    }) {
                    Ok(img) => doc.anim_structure("Add Image Track", |d, _| {
                        d.add_image_track(scene, &abs.to_string_lossy(), (img.width, img.height))
                    }),
                    Err(e) => Err(e),
                }
            }
            SceneOp::ImportMotion(p) => doc.import_motion(&p).map(|missing| {
                if !missing.is_empty() {
                    doc.notice = Some(format!("No attribute for: {}", missing.join(", ")));
                }
            }),
            SceneOp::BakeLipSync(i) => doc
                .anim
                .as_mut()
                .map_or(Err("No animation is open.".into()), |a| {
                    a.bake_lip_sync(i).map(|_| ())
                }),
            SceneOp::BakeMotionSync(s, i) => doc.bake_motion_sync(s, i).map(|_| ()),
            SceneOp::OpenPhysicsBake => {
                let outs = doc.physics_outputs();
                if outs.is_empty() {
                    Err("The model has no physics.".into())
                } else {
                    let params = outs
                        .into_iter()
                        .map(|pi| {
                            let p = &doc.model.parameters[pi];
                            (
                                pi,
                                if p.name.is_empty() {
                                    p.id.clone()
                                } else {
                                    p.name.clone()
                                },
                                true,
                            )
                        })
                        .collect();
                    if let Some(a) = doc.anim.as_mut() {
                        a.physics_bake = Some(PhysicsBakeDraft {
                            params,
                            every_frame: false,
                        });
                    }
                    Ok(())
                }
            }
            SceneOp::BakePhysics(params, every) => doc.bake_physics(&params, every).map(|_| ()),
            SceneOp::DeleteFormEdit(mi) => {
                doc.delete_form_edit(mi);
                Ok(())
            }
        };
        match r {
            Ok(()) => {
                if let Some(a) = doc.anim.as_mut() {
                    a.rename = None;
                    if matches!(a.doc.animation.scenes.len(), k if k > n) {
                        a.scene = a.doc.animation.scenes.len() - 1;
                    }
                }
            }
            Err(e) => doc.notice = Some(e),
        }
    }
    let Some(st) = doc.anim.as_mut() else { return };
    ui.horizontal(|ui| {
        let names: Vec<String> = st
            .doc
            .animation
            .scenes
            .iter()
            .map(|s| s.name.clone())
            .collect();
        egui::ComboBox::from_id_salt("anim-scene")
            .selected_text(names.get(st.scene).cloned().unwrap_or_default())
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    if ui.selectable_value(&mut st.scene, i, n).changed() {
                        st.selected = None;
                        if let Some(s) = st.doc.animation.scenes.get(i) {
                            st.frame = f64::from(s.work_start);
                        }
                    }
                }
            });
        if ui
            .button(if st.playing { "⏸" } else { "▶" })
            .on_hover_text(crate::i18n::tr("Play / Pause (Space)"))
            .clicked()
        {
            st.playing = !st.playing;
        }
        if ui
            .button(crate::i18n::tr("⏹"))
            .on_hover_text(crate::i18n::tr("Stop: back to the work area start"))
            .clicked()
        {
            st.playing = false;
            st.frame = st.scene().map_or(0.0, |s| f64::from(s.work_start));
        }
        ui.checkbox(&mut st.looping, crate::i18n::tr("Loop"));
        let mut f = st.frame.round() as i32;
        let max = st.scene().map_or(0, |s| s.duration);
        if ui
            .add(egui::DragValue::new(&mut f).range(0..=max).prefix("frame "))
            .changed()
        {
            st.frame = f64::from(f);
            st.playing = false;
        }
        if let Some(s) = st.scene() {
            ui.weak(format!(
                "{:.2} s · {} fps · work area {}–{}",
                st.frame / s.fps,
                s.fps,
                s.work_start,
                s.work_end
            ));
        }
        ui.checkbox(&mut st.keyed_only, crate::i18n::tr("Keyed only"));
        ui.add(egui::Slider::new(&mut st.px_per_frame, 1.0..=40.0).text("zoom"));
    });
    let Some(scene) = st.scene().cloned() else {
        return;
    };
    let Some((ti, m)) = st.model_track().map(|(i, m)| (i, m.clone())) else {
        ui.weak(crate::i18n::tr("This scene has no model track."));
        return;
    };
    let rows: Vec<(AttrRef, &Attr)> = m
        .params
        .iter()
        .enumerate()
        .map(|(k, a)| (AttrRef::Param(k), a))
        .chain(
            m.parts
                .iter()
                .enumerate()
                .map(|(k, a)| (AttrRef::Part(k), a)),
        )
        .filter(|(_, a)| !st.keyed_only || !a.curve.keys.is_empty())
        .collect();
    let ppf = st.px_per_frame;
    let width = LABEL_W + ppf * (scene.duration as f32 + 2.0);
    // Curve editor beside the timeline when an attribute is selected.
    let st_selected = st.selected.is_some();
    if st_selected {
        egui::Panel::right("curve-editor")
            .resizable(true)
            .default_size(ui.available_width() * 0.45)
            .show(ui, |ui| curve_ui(ui, doc, tokens));
    }
    let Some(st) = doc.anim.as_mut() else { return };
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(
                    width.max(ui.available_width()),
                    ROW_H * (rows.len() as f32 + 1.0),
                ),
                egui::Sense::click_and_drag(),
            );
            let p = ui.painter_at(rect);
            let x_of = |f: f64| rect.min.x + LABEL_W + (f as f32) * ppf;
            // Ruler with the work area.
            let ruler = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), ROW_H));
            p.rect_filled(ruler, 0.0, tokens.list_background);
            // The ruler as an accessible slider: frame 0 … duration.
            ui.ctx()
                .accesskit_node_builder(ui.id().with("timeline-ruler"), |b| {
                    b.set_role(egui::accesskit::Role::Slider);
                    b.set_label("Frame".to_owned());
                    b.set_value(format!("{:.0}/{}", st.frame, scene.duration));
                    b.set_bounds(egui::accesskit::Rect {
                        x0: x_of(0.0).into(),
                        y0: ruler.min.y.into(),
                        x1: x_of(f64::from(scene.duration)).into(),
                        y1: ruler.max.y.into(),
                    });
                });
            p.rect_filled(
                egui::Rect::from_x_y_ranges(
                    x_of(f64::from(scene.work_start))..=x_of(f64::from(scene.work_end)),
                    ruler.y_range(),
                ),
                0.0,
                tokens.accent.gamma_multiply(0.25),
            );
            let step = if ppf >= 8.0 {
                5
            } else if ppf >= 3.0 {
                10
            } else {
                30
            };
            for f in (0..=scene.duration).step_by(step) {
                let x = x_of(f64::from(f));
                p.line_segment(
                    [
                        egui::pos2(x, ruler.bottom() - 5.0),
                        egui::pos2(x, ruler.bottom()),
                    ],
                    egui::Stroke::new(1.0, tokens.secondary_text),
                );
                if (f as usize).is_multiple_of(step * 2) {
                    p.text(
                        egui::pos2(x + 2.0, ruler.top()),
                        egui::Align2::LEFT_TOP,
                        f.to_string(),
                        egui::FontId::proportional(10.0),
                        tokens.secondary_text,
                    );
                }
            }
            // Rows.
            for (r, (which, a)) in rows.iter().enumerate() {
                let y = rect.min.y + ROW_H * (r as f32 + 1.0);
                let row = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x, y),
                    egui::vec2(rect.width(), ROW_H),
                );
                let selected = st.selected == Some((ti, *which));
                // Painted rows get an accessible node each (name, bounds,
                // selected): screen readers and scripted tests find them.
                ui.ctx()
                    .accesskit_node_builder(ui.id().with(("timeline-row", r)), |b| {
                        b.set_role(egui::accesskit::Role::Row);
                        b.set_label(if a.name.is_empty() {
                            a.id.clone()
                        } else {
                            a.name.clone()
                        });
                        b.set_bounds(egui::accesskit::Rect {
                            x0: row.min.x.into(),
                            y0: row.min.y.into(),
                            x1: (row.min.x + LABEL_W).into(),
                            y1: row.max.y.into(),
                        });
                        if selected {
                            b.set_selected(true);
                        }
                    });
                if selected {
                    p.rect_filled(row, 0.0, tokens.accent.gamma_multiply(0.15));
                } else if r % 2 == 1 {
                    p.rect_filled(row, 0.0, tokens.list_background.gamma_multiply(0.5));
                }
                let label = if a.name.is_empty() {
                    a.id.clone()
                } else {
                    a.name.clone()
                };
                p.text(
                    egui::pos2(rect.min.x + 4.0, y + ROW_H / 2.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(11.0),
                    tokens.foreground,
                );
                for k in &a.curve.keys {
                    let c = egui::pos2(x_of(f64::from(k.frame)), y + ROW_H / 2.0);
                    let d = 4.0;
                    let pts = vec![
                        c + egui::vec2(0.0, -d),
                        c + egui::vec2(d, 0.0),
                        c + egui::vec2(0.0, d),
                        c + egui::vec2(-d, 0.0),
                    ];
                    p.add(egui::Shape::convex_polygon(
                        pts,
                        if selected {
                            tokens.accent
                        } else {
                            tokens.foreground
                        },
                        egui::Stroke::NONE,
                    ));
                }
            }
            // Events (user data markers) on the ruler.
            for (ef, text) in &scene.tracks[ti].user_data {
                let x = x_of(f64::from(*ef));
                let tip = [
                    egui::pos2(x, ruler.bottom()),
                    egui::pos2(x - 4.0, ruler.top() + 4.0),
                    egui::pos2(x + 4.0, ruler.top() + 4.0),
                ];
                p.add(egui::Shape::convex_polygon(
                    tip.to_vec(),
                    tokens.warning,
                    egui::Stroke::NONE,
                ));
                p.text(
                    egui::pos2(x + 5.0, ruler.top()),
                    egui::Align2::LEFT_TOP,
                    text,
                    egui::FontId::proportional(10.0),
                    tokens.warning,
                );
            }
            // Playhead.
            let xh = x_of(st.frame);
            p.line_segment(
                [egui::pos2(xh, rect.top()), egui::pos2(xh, rect.bottom())],
                egui::Stroke::new(1.5, tokens.error),
            );
            // Scrub on the ruler / key area; select rows by their label.
            if let Some(pos) = resp.interact_pointer_pos() {
                if pos.x > rect.min.x + LABEL_W && (resp.dragged() || resp.clicked()) {
                    let f = ((pos.x - rect.min.x - LABEL_W) / ppf).round().max(0.0);
                    st.frame = f64::from(f).min(f64::from(scene.duration));
                    st.playing = false;
                }
                if resp.clicked() && pos.y > ruler.bottom() {
                    let r = ((pos.y - ruler.bottom()) / ROW_H) as usize;
                    if let Some((which, _)) = rows.get(r) {
                        st.selected = Some((ti, *which));
                    }
                }
            }
        });
}

/// The curve editor of the selected attribute: the curve over the scene,
/// keys (drag to move, double-click to add, right-click for the curve
/// type or Delete), the playhead. spec: palettes.md §8, animation-model.md
/// §3
pub fn curve_ui(ui: &mut egui::Ui, doc: &mut Document, tokens: &crate::theme::Tokens) {
    use keyform_anim::scene::{delete_key, eval_curve, move_key, set_key};
    use keyform_model::animation::CurveType;
    let Some(st) = doc.anim.as_mut() else { return };
    let Some(a) = st.attr().cloned() else {
        ui.weak(crate::i18n::tr(
            "Select an attribute in the timeline to edit its curve.",
        ));
        return;
    };
    let Some(scene) = st.scene().cloned() else {
        return;
    };
    let default_type = match st.selected {
        Some((_, AttrRef::Part(_))) => scene.default_part_curve,
        _ => scene.default_param_curve,
    };
    ui.horizontal(|ui| {
        ui.strong(if a.name.is_empty() { &a.id } else { &a.name });
        ui.weak(format!("range {} … {}", a.range.0, a.range.1));
        if ui.button(crate::i18n::tr("Add key at playhead")).clicked() {
            st.add_key_at_playhead(default_type);
        }
        if let Some(k) = st.key.filter(|&k| k < a.curve.keys.len()) {
            let mut v = a.curve.keys[k].value;
            ui.label(format!("key @{}", a.curve.keys[k].frame));
            if ui.add(egui::DragValue::new(&mut v).speed(0.01)).changed() {
                st.edit("Change key value", |c, _| {
                    if let Some(x) = c.keys.get_mut(k) {
                        x.value = v;
                    }
                });
            }
            if ui.button(crate::i18n::tr("Delete key")).clicked() {
                st.edit("Delete key", |c, _| delete_key(c, k));
                st.key = None;
            }
        }
    });
    let (lo, hi) = (a.range.0.min(a.range.1), a.range.0.max(a.range.1));
    let (lo, hi) = if hi > lo {
        (lo, hi)
    } else {
        (lo - 1.0, hi + 1.0)
    };
    let size = egui::vec2(ui.available_width(), ui.available_height().max(80.0));
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, tokens.list_background);
    let n = f64::from(scene.duration.max(1));
    let x_of = |f: f64| rect.left() + 8.0 + ((f / n) as f32) * (rect.width() - 16.0);
    let y_of =
        |v: f64| rect.bottom() - 8.0 - (((v - lo) / (hi - lo)) as f32) * (rect.height() - 16.0);
    let f_of = |x: f32| f64::from(x - rect.left() - 8.0) / f64::from(rect.width() - 16.0) * n;
    let v_of = |y: f32| {
        lo + f64::from(rect.bottom() - 8.0 - y) / f64::from(rect.height() - 16.0) * (hi - lo)
    };
    // Work area and zero line.
    p.rect_filled(
        egui::Rect::from_x_y_ranges(
            x_of(f64::from(scene.work_start))..=x_of(f64::from(scene.work_end)),
            rect.y_range(),
        ),
        0.0,
        tokens.accent.gamma_multiply(0.06),
    );
    if lo < 0.0 && hi > 0.0 {
        p.line_segment(
            [
                egui::pos2(rect.left(), y_of(0.0)),
                egui::pos2(rect.right(), y_of(0.0)),
            ],
            egui::Stroke::new(1.0, tokens.border),
        );
    }
    // The curve, 4 samples per frame.
    let pts: Vec<egui::Pos2> = (0..=(4 * scene.duration.max(1)))
        .map(|i| {
            let f = f64::from(i) / 4.0;
            egui::pos2(x_of(f), y_of(eval_curve(&a.curve, f).clamp(lo, hi)))
        })
        .collect();
    p.add(egui::Shape::line(
        pts,
        egui::Stroke::new(1.5, tokens.accent),
    ));
    // Keys and handles.
    let hit = |pos: egui::Pos2| {
        a.curve.keys.iter().position(|k| {
            (egui::pos2(x_of(f64::from(k.frame)), y_of(k.value)) - pos).length() < 7.0
        })
    };
    for (i, k) in a.curve.keys.iter().enumerate() {
        let c = egui::pos2(x_of(f64::from(k.frame)), y_of(k.value));
        if st.key == Some(i) {
            for h in [k.prev, k.next].into_iter().flatten() {
                let q = egui::pos2(x_of(f64::from(h.frame)), y_of(h.value));
                p.line_segment([c, q], egui::Stroke::new(1.0, tokens.secondary_text));
                p.circle_filled(q, 2.5, tokens.secondary_text);
            }
        }
        let fill = if st.key == Some(i) {
            tokens.accent
        } else {
            tokens.foreground
        };
        p.circle_filled(c, 4.0, fill);
    }
    let xh = x_of(st.frame);
    p.line_segment(
        [egui::pos2(xh, rect.top()), egui::pos2(xh, rect.bottom())],
        egui::Stroke::new(1.0, tokens.error),
    );
    // Interaction.
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.drag_started() {
            st.key = hit(pos);
            if st.key.is_some() {
                st.drag = Some(st.doc.animation.clone());
            }
        }
        if resp.dragged() {
            if let (Some(k), Some(base)) = (st.key, st.drag.clone()) {
                // Re-apply the move from the drag's start.
                st.doc.animation = base;
                let f = f_of(pos.x).round().clamp(0.0, n) as i32;
                let v = v_of(pos.y).clamp(lo, hi);
                let legacy = st.doc.animation.smooth_legacy;
                if let Some(at) = st.attr_mut() {
                    if move_key(&mut at.curve, k, f, v) {
                        let range = keyform_anim::scene::clamp_range(at);
                        keyform_anim::scene::refresh(&mut at.curve, range.0, range.1, legacy);
                        st.key = at.curve.keys.iter().position(|x| x.frame == f);
                    }
                }
            }
        }
        if resp.clicked() {
            st.key = hit(pos);
            if st.key.is_none() {
                st.frame = f_of(pos.x).round().clamp(0.0, n);
                st.playing = false;
            }
        }
        if resp.double_clicked() && hit(pos).is_none() {
            let f = f_of(pos.x).round().clamp(0.0, n) as i32;
            let v = v_of(pos.y);
            st.edit("Add key", |c, _| {
                set_key(c, f, v, default_type);
            });
        }
    }
    if resp.drag_stopped() {
        if let Some(base) = st.drag.take() {
            if base != st.doc.animation {
                st.undo.push(("Move key".into(), UndoStep::Anim(base)));
                st.redo.clear();
                st.dirty = true;
            }
        }
    }
    resp.context_menu(|ui| {
        let Some(k) = st.key else {
            ui.weak(crate::i18n::tr("Click a key first."));
            return;
        };
        for (name, t) in [
            ("Linear", CurveType::Linear),
            ("Bezier", CurveType::Bezier),
            ("Smooth", CurveType::Smooth),
            ("Step", CurveType::Step),
            ("Inverse step", CurveType::InverseStep),
        ] {
            if ui.button(name).clicked() {
                st.edit("Change curve type", |c, _| {
                    if let Some(x) = c.types.get_mut(k) {
                        *x = t;
                    }
                });
                ui.close();
            }
        }
        ui.separator();
        if ui.button(crate::i18n::tr("Delete key")).clicked() {
            st.edit("Delete key", |c, _| delete_key(c, k));
            st.key = None;
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// hiyori_pro_t04.can3 opens on its linked model; playing scene
    /// hiyori_m01 gives the parameter values the reference's exported
    /// hiyori_m01.motion3.json gives at the same times.
    #[test]
    fn play_scene_like_reference_motion() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let can3 = home.join("Downloads/hiyori_en/hiyori_pro/hiyori_pro_t04.can3");
        let model = root.join("hiyori_pro_t11.cmo3");
        let motion =
            home.join("Downloads/hiyori_en/hiyori_pro/runtime/motion/hiyori_m01.motion3.json");
        if !can3.exists() || !model.exists() || !motion.exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kf-anim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::os::unix::fs::symlink(&can3, dir.join("a.can3"));
        let _ = std::os::unix::fs::symlink(&model, dir.join("hiyori_pro_t11.cmo3"));
        let mut log = Log::default();
        let mut doc = Document::open_animation(&dir.join("a.can3"), &mut log).unwrap();
        assert_eq!(doc.mode, Mode::Animation);
        let reference =
            keyform_anim::Motion::parse(&std::fs::read_to_string(&motion).unwrap()).unwrap();
        let fps = 30.0;
        let mut worst = 0.0f32;
        for f in 0..=141 {
            doc.anim.as_mut().unwrap().frame = f64::from(f);
            doc.tick(0.0, &Default::default(), &mut log);
            for c in reference
                .curves
                .iter()
                .filter(|c| c.target == keyform_anim::Target::Parameter)
            {
                let Some(pi) = doc.model.parameters.iter().position(|p| p.id == c.id) else {
                    continue;
                };
                let p = &doc.model.parameters[pi];
                let v = p.clamp(c.eval(f as f32 / fps, false));
                let d = (doc.live[pi] - v).abs() / (p.max - p.min).max(1e-6);
                worst = worst.max(d);
            }
        }
        eprintln!("worst difference over 142 frames: {worst} of the range");
        // The motion rounds times and values to 3 decimals: at a key
        // frame (109/30 → 3.633) the runtime is already a hair into the
        // next segment.
        assert!(worst < 5e-3, "{worst}");
        // Playing advances the playhead by fps × dt.
        let a = doc.anim.as_mut().unwrap();
        a.frame = 0.0;
        a.playing = true;
        doc.tick(0.5, &Default::default(), &mut log);
        assert!((doc.anim.as_ref().unwrap().frame - 15.0).abs() < 1e-6);
        // Key edits on the selected attribute are undoable steps.
        let a = doc.anim.as_mut().unwrap();
        a.playing = false;
        a.selected = Some((a.model_track().unwrap().0, AttrRef::Param(0)));
        let before = a.attr().unwrap().curve.keys.len();
        a.frame = 3.0;
        assert!(a.edit("Add key", |c, f| {
            keyform_anim::scene::set_key(
                c,
                f as i32,
                5.0,
                keyform_model::animation::CurveType::Linear,
            );
        }));
        assert_eq!(a.attr().unwrap().curve.keys.len(), before + 1);
        assert!(a.dirty);
        assert_eq!(a.step(false).as_deref(), Some("Add key"));
        assert_eq!(a.attr().unwrap().curve.keys.len(), before);
        assert_eq!(a.step(true).as_deref(), Some("Add key"));
        assert_eq!(a.attr().unwrap().curve.keys.len(), before + 1);
        // Motion export writes the scenes marked for export.
        let out = doc.export_motions(&dir).unwrap();
        assert_eq!(out.len(), 10);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// File ▸ New ▸ Animation on hiyori: one scene, a scene added, saved
    /// under a chosen path, reopened.
    #[test]
    fn new_animation_save_reopen() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = root.join("hiyori_pro_t11.cmo3");
        if !model.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        assert_eq!(doc.mode, Mode::Animation);
        assert!(doc.is_modified());
        let a = doc.anim.as_ref().unwrap();
        assert!(a.untitled);
        assert_eq!(a.doc.animation.scenes.len(), 1);
        doc.anim_structure("Add Scene", |d, m| {
            d.add_scene("Two", m, 60, 30.0).map(|_| ())
        })
        .unwrap();
        // Structural edits undo and redo like any other step.
        {
            let a = doc.anim.as_mut().unwrap();
            assert_eq!(a.step(false).as_deref(), Some("Add Scene"));
            assert_eq!(a.doc.animation.scenes.len(), 1);
            assert_eq!(a.step(true).as_deref(), Some("Add Scene"));
            assert_eq!(a.doc.animation.scenes.len(), 2);
            a.undo.clear();
        }
        // Scene settings and an event, as undoable steps.
        {
            let a = doc.anim.as_mut().unwrap();
            a.scene = 0;
            let mut s = a.scene().unwrap().clone();
            s.fps = 24.0;
            s.fade_in_ms = 300;
            a.apply_settings(&s);
            a.set_event(12, Some("blink".into()));
            assert_eq!(a.undo.len(), 2);
            assert_eq!(a.step(false).as_deref(), Some("Event"));
            assert_eq!(a.step(true).as_deref(), Some("Event"));
        }
        doc.tick(0.0, &Default::default(), &mut log);
        let dir = std::env::temp_dir().join(format!("kf-newanim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("made.can3");
        doc.save_animation_to(Some(&out)).unwrap();
        assert!(!doc.is_modified());
        let back = Document::open_animation(&out, &mut log).unwrap();
        let names: Vec<_> = back
            .anim
            .as_ref()
            .unwrap()
            .doc
            .animation
            .scenes
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(names, vec!["Scene1".to_string(), "Two".to_string()]);
        let s0 = &back.anim.as_ref().unwrap().doc.animation.scenes[0];
        assert_eq!((s0.fps, s0.fade_in_ms), (24.0, 300));
        assert_eq!(s0.tracks[1].user_data, vec![(12, "blink".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_wav(path: &Path, samples: &[i16], rate: u32) {
        let mut d = Vec::new();
        let data_len = (samples.len() * 2) as u32;
        d.extend_from_slice(b"RIFF");
        d.extend_from_slice(&(36 + data_len).to_le_bytes());
        d.extend_from_slice(b"WAVEfmt ");
        d.extend_from_slice(&16u32.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&rate.to_le_bytes());
        d.extend_from_slice(&(rate * 2).to_le_bytes());
        d.extend_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&16u16.to_le_bytes());
        d.extend_from_slice(b"data");
        d.extend_from_slice(&data_len.to_le_bytes());
        for x in samples {
            d.extend_from_slice(&x.to_le_bytes());
        }
        std::fs::write(path, d).unwrap();
    }

    /// A sound track drives lip sync: silent first second, loud second
    /// second; the mouth follows only after Bake Lip-sync (§4.1);
    /// the animation saves and reloads with its sound.
    #[test]
    fn sound_track_lip_sync() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = root.join("hiyori_pro_t11.cmo3");
        if !model.exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kf-sound-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("voice.wav");
        let rate = 44100;
        let samples: Vec<i16> = (0..2 * rate)
            .map(|i| {
                if i < rate {
                    0
                } else {
                    ((i as f32 * 0.05).sin() * 20000.0) as i16
                }
            })
            .collect();
        write_wav(&wav, &samples, rate);
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        doc.anim_structure("Add Sound Track", |d, _| {
            d.add_sound_track(0, &wav.to_string_lossy(), 2.0)
        })
        .unwrap();
        let a = doc.anim.as_ref().unwrap();
        assert_eq!(a.sounds.len(), 1, "{:?}", log.entries);
        let (_, m) = a.model_track().unwrap();
        // §4.1: adding a sound track doesn't link lip sync, and there's no
        // live envelope: the mouth only follows after Bake Lip-sync.
        assert!(m.lip_sync.as_ref().unwrap().sync_track.is_none());
        let mouth = doc
            .model
            .parameters
            .iter()
            .position(|p| p.id == "ParamMouthOpenY")
            .unwrap();
        let quiet0 = doc.anim_values_at(10.0).unwrap().0[mouth];
        let loud0 = doc.anim_values_at(45.0).unwrap().0[mouth];
        assert_eq!(quiet0, loud0);
        // The playback mix: silent first second, loud second (0 dB = gain 1).
        let mix = crate::audio_out::mix_scene(doc.anim.as_ref().unwrap(), 44100).unwrap();
        assert!(mix[..44000].iter().all(|x| *x == 0.0));
        assert!(mix[50000..60000].iter().any(|x| x.abs() > 0.3));
        // Bake from the sound track (index 2): keys reduced to turning
        // points; the mouth follows (frame 45 = 1.5 s, loud).
        let sound = doc.anim.as_ref().unwrap().scene().unwrap().tracks.len() - 1;
        let n = doc.anim.as_mut().unwrap().bake_lip_sync(sound).unwrap();
        assert!((2..30).contains(&n), "{n}");
        let quiet = doc.anim_values_at(10.0).unwrap().0[mouth];
        let loud = doc.anim_values_at(45.0).unwrap().0[mouth];
        assert!(loud > quiet + 0.3, "{quiet} → {loud}");
        let out = dir.join("talk.can3");
        doc.save_animation_to(Some(&out)).unwrap();
        let back = Document::open_animation(&out, &mut log).unwrap();
        let b = back.anim.as_ref().unwrap();
        assert_eq!(b.sounds.len(), 1);
        let (_, m2) = b.model_track().unwrap();
        assert_eq!(
            m2.lip_sync.as_ref().unwrap().sound_level.curve.keys.len(),
            n
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Physics baking: a head turn drives the physics outputs, which get
    /// keys (every frame, or reduced to turning points) as one undo step.
    #[test]
    fn bake_motion_sync_writes_mouth_keys() {
        // ms.cmo3 (reference-made motion-sync setting) + a burst WAV.
        let Some(dir) = std::env::var_os("KEYFORM_BB_P5") else {
            return;
        };
        let dir = Path::new(&dir);
        let (model, wav) = (dir.join("ms.cmo3"), dir.join("../q32_35/burst.wav"));
        if !wav.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        let scene = doc.anim.as_ref().unwrap().scene;
        let b = std::fs::read(&wav).unwrap();
        let w = keyform_anim::lipsync_bake::RawWav::parse(&b).unwrap();
        let abs = std::fs::canonicalize(&wav).unwrap();
        doc.anim_structure("Add Sound Track", |d, _| {
            d.add_sound_track(scene, &abs.to_string_lossy(), w.duration())
        })
        .unwrap();
        let a = doc.anim.as_ref().unwrap();
        let sound = a
            .scene()
            .unwrap()
            .tracks
            .iter()
            .position(|t| matches!(t.kind, TrackKind::Sound(_)))
            .unwrap();
        let n = doc.bake_motion_sync(0, sound).unwrap();
        assert!(n > 10, "{n} keys");
        let a = doc.anim.as_ref().unwrap();
        assert_eq!(
            a.undo.last().map(|u| u.0.as_str()),
            Some("Bake Animation from Motion-sync")
        );
        let (_, m) = a.model_track().unwrap();
        let open = m
            .params
            .iter()
            .find(|p| p.id.ends_with("ParamMouthOpenY"))
            .unwrap();
        let peak = open
            .curve
            .keys
            .iter()
            .map(|k| k.value)
            .fold(0.0f64, f64::max);
        assert!(peak > 0.05, "mouth never opens: {peak}");
    }

    #[test]
    fn bake_physics_writes_outputs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = root.join("hiyori_pro_t11.cmo3");
        if !model.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        let outs = doc.physics_outputs();
        assert!(!outs.is_empty());
        // Animate an input: ParamAngleX 0 → 30 → 0.
        {
            let a = doc.anim.as_mut().unwrap();
            let (ti, _) = a.model_track().unwrap();
            let s = a.scene;
            let TrackKind::Model(m) = &mut a.doc.animation.scenes[s].tracks[ti].kind else {
                panic!()
            };
            let at = m
                .params
                .iter_mut()
                .find(|p| p.id == "live2dParam_ParamAngleX")
                .unwrap();
            for (f, v) in [(0, 0.0), (15, 30.0), (30, 0.0)] {
                keyform_anim::scene::set_key(
                    &mut at.curve,
                    f,
                    v,
                    keyform_model::animation::CurveType::Smooth,
                );
            }
            keyform_anim::scene::refresh(&mut at.curve, -30.0, 30.0, false);
        }
        let dur = doc.anim.as_ref().unwrap().scene().unwrap().duration;
        let n_all = doc.bake_physics(&outs[..1], true).unwrap();
        assert_eq!(n_all, (dur + 1) as usize);
        assert_eq!(
            doc.anim.as_mut().unwrap().step(false).as_deref(),
            Some("Bake Animation from Physics")
        );
        let n = doc.bake_physics(&outs, false).unwrap();
        assert!(n >= 2 * outs.len() && n < n_all * outs.len(), "{n}");
        // The outputs now move with the head turn.
        let pi = outs[0];
        let (a, b) = (
            doc.anim_values_at(0.0).unwrap().0[pi],
            doc.anim_values_at(20.0).unwrap().0[pi],
        );
        assert!((a - b).abs() > 1e-3, "{a} {b}");
    }

    /// A global form key moves its art mesh by target − original in the
    /// scene (before the track's placement scales it).
    #[test]
    fn global_form_animation() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = root.join("hiyori_pro_t11.cmo3");
        if !model.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        let before = doc.anim_frame_at(0.0).unwrap();
        let n = doc.model.art_meshes[0].vertex_count();
        let guid = doc.model.art_meshes[0].info.guid.clone();
        let a = doc.anim.as_mut().unwrap();
        let (ti, _) = a.model_track().unwrap();
        let TrackKind::Model(m) = &mut a.doc.animation.scenes[0].tracks[ti].kind else {
            panic!()
        };
        let key = keyform_model::animation::FormKey {
            frame: 0,
            original: vec![0.0; 2 * n],
            target: (0..n).flat_map(|_| [100.0, 0.0]).collect(),
        };
        m.form.global.push(keyform_model::animation::FormObject {
            guid,
            keys: vec![key],
            time_remap: None,
        });
        m.form.apply = true;
        let after = doc.anim_frame_at(0.0).unwrap();
        let (p0, p1) = (&before.meshes[0].positions, &after.meshes[0].positions);
        // 100 model px × the track's fit scale, to the right.
        let dx = p1[0] - p0[0];
        assert!(dx > 1.0 && (p1[1] - p0[1]).abs() < 1e-3, "{dx}");
        assert!(after.meshes[1].positions == before.meshes[1].positions);
    }

    /// Importing hiyori_m01.motion3.json into a new scene and exporting it
    /// again gives the same motion. spec: motion3-json.md §2, §4
    #[test]
    fn motion_import_round_trip() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let model = root.join("hiyori_pro_t11.cmo3");
        let motion =
            home.join("Downloads/hiyori_en/hiyori_pro/runtime/motion/hiyori_m01.motion3.json");
        if !model.exists() || !motion.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        let missing = doc.import_motion(&motion).unwrap();
        assert!(missing.is_empty(), "{missing:?}");
        let dir = std::env::temp_dir().join(format!("kf-mimport-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let files = doc.export_motions(&dir).unwrap();
        let mine = files
            .iter()
            .find(|p| p.ends_with("hiyori_m01.motion3.json"))
            .unwrap();
        let parse =
            |p: &Path| keyform_anim::Motion::parse(&std::fs::read_to_string(p).unwrap()).unwrap();
        let (a, b) = (parse(mine), parse(&motion));
        assert_eq!(a.duration, b.duration);
        assert_eq!(a.curves.len(), b.curves.len());
        let mut worst = 0.0f32;
        // The new scene lists parameters in model order: match by ID.
        for y in &b.curves {
            let x = a
                .curves
                .iter()
                .find(|x| x.id == y.id && x.target == y.target)
                .unwrap();
            assert_eq!(x.segments.len(), y.segments.len(), "{}", x.id);
            for t in 0..=140 {
                let t = t as f32 / 30.0;
                worst = worst.max((x.eval(t, false) - y.eval(t, false)).abs());
            }
        }
        eprintln!("worst value difference after the round trip: {worst}");
        assert!(worst < 0.01, "{worst}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Black-box check files (not run by default): `KEYFORM_BB_DIR` = output
/// folder, `KEYFORM_BB_MODEL` = a .cmo3 within the reference's FREE limits.
/// Writes place_kf.can3 (placement with shear and a 3-key xy curve) and
/// sound_kf.can3 + burst.wav (a sound track to Bake Lip-sync from), plus
/// Keyform's renders of place_kf at frames 5 and 20.
#[cfg(test)]
mod placement_reference {
    use super::*;
    use std::path::Path;

    /// The alpha centroid (pixels) of a straight-alpha image.
    fn centroid(img: &keyform_formats::image::RgbaImage) -> [f64; 2] {
        let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
        for (i, p) in img.data.chunks_exact(4).enumerate() {
            let w = f64::from(p[3]);
            sx += w * (i % img.width as usize) as f64;
            sy += w * (i / img.width as usize) as f64;
            sw += w;
        }
        [sx / sw, sy / sw]
    }

    /// hiyori_free placed with scale 25/20 %, rotate −10°, shear 0.3 and a
    /// 3-key xy curve (blackbox_files::set_placement): the rendered alpha
    /// centroid at 50 % matches the reference's image-sequence export
    /// (5.3.04) at frames 0 and 20. spec: animation-model.md §5.1
    #[test]
    fn placement_matches_reference() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dirs = std::env::var("KEYFORM_SAMPLES").unwrap_or_else(|_| root.display().to_string());
        let Some(model) = std::env::split_paths(&dirs)
            .map(|d| d.join("hiyori_free_t08.cmo3"))
            .find(|p| p.exists())
        else {
            return;
        };
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        {
            let a = doc.anim.as_mut().unwrap();
            let ti = a.model_track().unwrap().0;
            let s = a.scene;
            crate::animation::blackbox_files::set_placement(
                a.doc.animation.scenes[s].tracks[ti]
                    .visual
                    .as_mut()
                    .unwrap(),
            );
        }
        for (f, r) in [(0.0, [138.51, 181.92]), (20.0, [269.50, 392.91])] {
            let img = crate::export_media::render_frame(&doc, f, 50.0, false).unwrap();
            assert_eq!((img.width, img.height), (456, 640));
            let c = centroid(&img);
            assert!(
                (c[0] - r[0]).abs() < 0.1 && (c[1] - r[1]).abs() < 0.1,
                "frame {f}: {c:?} vs {r:?}"
            );
        }
    }
}

#[cfg(test)]
mod blackbox_files {
    use super::*;
    use std::path::Path;

    pub(crate) fn burst_samples() -> Vec<i16> {
        // 1.2 s mono at 44.1 kHz: 440 Hz bursts of different loudness.
        let rate = 44100.0;
        let amps = [0.0, 0.05, 0.2, 0.1, 0.3, 0.0];
        (0..(1.2 * rate) as usize)
            .map(|i| {
                let t = i as f64 / rate;
                let a = amps[((t / 0.2) as usize).min(amps.len() - 1)];
                let v = a
                    * ((2.0 * std::f64::consts::PI * 440.0 * t).sin()
                        + 0.3 * (2.0 * std::f64::consts::PI * 1320.0 * t).sin());
                (v * 32767.0) as i16
            })
            .collect()
    }

    pub(crate) fn wav_bytes(samples: &[i16], rate: u32) -> Vec<u8> {
        let mut d = Vec::new();
        let n = (samples.len() * 2) as u32;
        d.extend_from_slice(b"RIFF");
        d.extend_from_slice(&(36 + n).to_le_bytes());
        d.extend_from_slice(b"WAVEfmt ");
        d.extend_from_slice(&16u32.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&rate.to_le_bytes());
        d.extend_from_slice(&(rate * 2).to_le_bytes());
        d.extend_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&16u16.to_le_bytes());
        d.extend_from_slice(b"data");
        d.extend_from_slice(&n.to_le_bytes());
        for x in samples {
            d.extend_from_slice(&x.to_le_bytes());
        }
        d
    }

    /// 2.4 s mono: 2-frame (at 30 fps) segments from 0.1 s on, loud ones
    /// (varying level) alternating with silence, so every baked key is a
    /// turning point and survives the reduction.
    pub(crate) fn alternating_samples() -> Vec<i16> {
        let rate = 44100.0;
        (0..(2.4 * rate) as usize)
            .map(|i| {
                let t = i as f64 / rate;
                let j = ((t - 0.1) * 15.0).floor();
                if j < 0.0 || j as i64 % 2 == 1 {
                    return 0;
                }
                let a = 0.02 + 0.006 * j;
                let v = a
                    * ((2.0 * std::f64::consts::PI * 440.0 * t).sin()
                        + 0.3 * (2.0 * std::f64::consts::PI * 1320.0 * t).sin());
                (v * 32767.0) as i16
            })
            .collect()
    }

    /// The placement used by place_kf.can3.
    pub(crate) fn set_placement(v: &mut keyform_model::animation::Visual) {
        v.scale_x.curve.base = 25.0;
        v.scale_y.curve.base = 20.0;
        v.rotate.curve.base = -10.0;
        v.shear.curve.base = 0.3;
        v.xy.keys = vec![
            (0, [300.0, 400.0]),
            (10, [600.0, 700.0]),
            (30, [350.0, 900.0]),
        ];
        v.xy.corners = vec![false; 3];
    }

    #[test]
    fn write_blackbox_files() {
        let (Some(dir), Some(model)) = (
            std::env::var_os("KEYFORM_BB_DIR"),
            std::env::var_os("KEYFORM_BB_MODEL"),
        ) else {
            return;
        };
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        let model = Path::new(&model);
        let mut log = Log::default();
        // Placement.
        let mut doc = Document::new_animation(model, &mut log).unwrap();
        {
            let a = doc.anim.as_mut().unwrap();
            let ti = a.model_track().unwrap().0;
            let s = a.scene;
            let t = &mut a.doc.animation.scenes[s].tracks[ti];
            set_placement(t.visual.as_mut().unwrap());
        }
        doc.save_animation_to(Some(&dir.join("place_kf.can3")))
            .unwrap();
        std::fs::create_dir_all(dir.join("kf")).unwrap();
        for f in 0..=30 {
            let img = crate::export_media::render_frame(&doc, f64::from(f), 50.0, false).unwrap();
            std::fs::write(
                dir.join(format!("kf/place_{f:03}.png")),
                img.encode_png().unwrap(),
            )
            .unwrap();
        }
        // Sound.
        let wav = dir.join("burst.wav");
        std::fs::write(&wav, wav_bytes(&burst_samples(), 44100)).unwrap();
        let mut doc = Document::new_animation(model, &mut log).unwrap();
        doc.anim_structure("Add Sound Track", |d, _| {
            d.add_sound_track(0, &wav.to_string_lossy(), 1.2)
        })
        .unwrap();
        doc.save_animation_to(Some(&dir.join("sound_kf.can3")))
            .unwrap();
        let wav = dir.join("alt.wav");
        std::fs::write(&wav, wav_bytes(&alternating_samples(), 44100)).unwrap();
        let mut doc = Document::new_animation(model, &mut log).unwrap();
        doc.anim_structure("Add Sound Track", |d, _| {
            d.add_sound_track(0, &wav.to_string_lossy(), 2.4)
        })
        .unwrap();
        doc.save_animation_to(Some(&dir.join("sound2_kf.can3")))
            .unwrap();
        // An image track (listed below the model track: drawn behind it).
        let png = dir.join("red.png");
        let img = keyform_formats::image::RgbaImage {
            width: 400,
            height: 400,
            data: [230u8, 20, 20, 255].repeat(400 * 400),
        };
        std::fs::write(&png, img.encode_png().unwrap()).unwrap();
        let mut doc = Document::new_animation(model, &mut log).unwrap();
        doc.anim_structure("Add Image Track", |d, _| {
            d.add_image_track(0, &png.to_string_lossy(), (400, 400))
        })
        .unwrap();
        {
            let a = doc.anim.as_mut().unwrap();
            let (ti, _) = a.model_track().unwrap();
            let s = a.scene;
            set_placement(
                a.doc.animation.scenes[s].tracks[ti]
                    .visual
                    .as_mut()
                    .unwrap(),
            );
        }
        doc.save_animation_to(Some(&dir.join("image_kf.can3")))
            .unwrap();
        let img = crate::export_media::render_frame(&doc, 0.0, 50.0, true).unwrap();
        std::fs::write(dir.join("image_kf_0.png"), img.encode_png().unwrap()).unwrap();
        eprintln!("{:?}", log.entries);
    }
}

#[cfg(test)]
mod blackbox_compare {
    use super::*;
    use std::path::Path;

    /// Prints the reference's baked soundLevel keys (sound_ref.can3) next
    /// to Keyform's bake of sound_kf.can3 (`KEYFORM_BB_DIR`).
    #[test]
    fn compare_bake() {
        let Some(dir) = std::env::var_os("KEYFORM_BB_DIR") else {
            return;
        };
        let dir = Path::new(&dir);
        let mut log = Log::default();
        let show = |doc: &Document| {
            let a = doc.anim.as_ref().unwrap();
            let (_, m) = a.model_track().unwrap();
            let e = m.lip_sync.as_ref().unwrap();
            let c = &e.sound_level.curve;
            eprintln!(
                "sync {:?} range {:?} fixed {} types {:?}",
                e.sync_track,
                e.sound_level.range,
                c.fixed,
                c.types.len()
            );
            for (k, t) in c.keys.iter().zip(&c.types) {
                eprintln!(
                    "  {:3} {:.6} {:?} {:?} {:?}",
                    k.frame, k.value, t, k.prev, k.next
                );
            }
        };
        let name = std::env::var("KEYFORM_BB_SOUND").unwrap_or_else(|_| "sound".into());
        if let Ok(r) = Document::open_animation(&dir.join(format!("{name}_ref.can3")), &mut log) {
            eprintln!("REFERENCE");
            show(&r);
        }
        let mut d =
            Document::open_animation(&dir.join(format!("{name}_kf.can3")), &mut log).unwrap();
        let a = d.anim.as_mut().unwrap();
        let sound = a
            .scene()
            .unwrap()
            .tracks
            .iter()
            .position(|t| matches!(t.kind, TrackKind::Sound(_)))
            .unwrap();
        a.bake_lip_sync(sound).unwrap();
        eprintln!("KEYFORM");
        show(&d);
        eprintln!("{:?}", log.entries);
    }
}
