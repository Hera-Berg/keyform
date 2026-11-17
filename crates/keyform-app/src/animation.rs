//! The Animation workspace: a `.can3` document played on its linked model,
//! with a timeline. spec: 60-animation/animation-model.md §1–5,
//! 50-ui/palettes.md §7–8, 60-animation/motion3-json.md §2

use std::path::{Path, PathBuf};

use eframe::egui;
use keyform_anim::scene::{eval_attr, param_value, step_frame};
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
    pub undo: Vec<(String, keyform_model::animation::Animation)>,
    pub redo: Vec<(String, keyform_model::animation::Animation)>,
    /// A key drag: the animation before it.
    drag: Option<keyform_model::animation::Animation>,
    /// Changed since opening or the last save.
    pub dirty: bool,
    /// Never saved: the first save asks for a location.
    pub untitled: bool,
    /// The scene name being edited (Rename).
    pub rename: Option<String>,
}

impl AnimState {
    pub fn scene(&self) -> Option<&Scene> {
        self.doc.animation.scenes.get(self.scene)
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
        self.undo.push((name.to_owned(), before));
        self.redo.clear();
        self.dirty = true;
        true
    }

    /// Undo (`redo` = false) or redo; returns the step's name.
    pub fn step(&mut self, redo: bool) -> Option<String> {
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let (name, a) = from.pop()?;
        let cur = std::mem::replace(&mut self.doc.animation, a);
        to.push((name.clone(), cur));
        self.dirty = true;
        self.key = None;
        Some(name)
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
        });
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
        });
        Ok(d)
    }

    /// Runs a structural change (scenes, tracks) on the animation. These
    /// aren't undoable yet: the animation's undo steps are cleared.
    pub fn anim_structure(
        &mut self,
        f: impl FnOnce(&mut Can3Document, &keyform_model::Model) -> keyform_formats::Result<()>,
    ) -> Result<(), String> {
        let model = self.model.clone();
        let st = self.anim.as_mut().ok_or("No animation is open.")?;
        f(&mut st.doc, &model).map_err(|e| e.to_string())?;
        st.undo.clear();
        st.redo.clear();
        st.dirty = true;
        st.selected = None;
        st.key = None;
        st.scene = st
            .scene
            .min(st.doc.animation.scenes.len().saturating_sub(1));
        Ok(())
    }

    /// Parameter values and part opacities of the current frame: each
    /// model parameter takes its attribute's value (frame-stepped by its
    /// group, else the track), with eye blink / lip sync. spec: §3.1a, §4, §5
    pub fn anim_values(&self) -> Option<(Vec<f32>, Vec<Option<f32>>)> {
        self.anim_values_at(self.anim.as_ref()?.frame)
    }

    /// [`Document::anim_values`] at scene frame `frame`.
    pub fn anim_values_at(&self, frame: f64) -> Option<(Vec<f32>, Vec<Option<f32>>)> {
        let st = self.anim.as_ref()?;
        let scene = st.scene()?;
        let (ti, m) = st.model_track()?;
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
            values[pi] = model.parameters[pi].clamp(param_value(m, k, xf) as f32);
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

    /// Advances the playhead while playing (work area, looping).
    pub fn anim_tick(&mut self, dt: f32) {
        let Some(st) = self.anim.as_mut() else { return };
        let Some(s) = st.scene() else { return };
        let (a, b, fps) = (f64::from(s.work_start), f64::from(s.work_end), s.fps);
        let (lo, hi) = (f64::from(s.start_frame), f64::from(s.duration));
        if st.playing {
            st.frame += f64::from(dt) * fps;
            if st.frame > b {
                if st.looping {
                    st.frame = a + (st.frame - b) % (b - a + 1.0).max(1.0);
                } else {
                    st.frame = b;
                    st.playing = false;
                }
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
/// A structural change chosen in the timeline header.
enum SceneOp {
    Add,
    Rename(String),
    Delete,
    AddTrack,
}

pub fn timeline_ui(ui: &mut egui::Ui, doc: &mut Document, tokens: &crate::theme::Tokens) {
    let mut op = None;
    if let Some(st) = doc.anim.as_mut() {
        ui.horizontal(|ui| {
            ui.menu_button("Scene", |ui| {
                if ui.button("Create new scene").clicked() {
                    op = Some(SceneOp::Add);
                    ui.close();
                }
                if ui.button("Rename…").clicked() {
                    st.rename = st.scene().map(|s| s.name.clone());
                    ui.close();
                }
                if ui
                    .add_enabled(
                        st.doc.animation.scenes.len() > 1,
                        egui::Button::new("Delete scene"),
                    )
                    .clicked()
                {
                    op = Some(SceneOp::Delete);
                    ui.close();
                }
                ui.separator();
                if ui.button("Add model track").clicked() {
                    op = Some(SceneOp::AddTrack);
                    ui.close();
                }
            });
            if let Some(name) = st.rename.as_mut() {
                let r = ui.add(egui::TextEdit::singleline(name).desired_width(140.0));
                if ui.button("OK").clicked()
                    || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    op = Some(SceneOp::Rename(name.clone()));
                }
                if ui.button("Cancel").clicked() {
                    st.rename = None;
                }
            }
        });
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
            SceneOp::Add => doc.anim_structure(|d, m| {
                d.add_scene(&format!("Scene{}", n + 1), m, frames, fps)
                    .map(|_| ())
            }),
            SceneOp::Rename(name) => doc.anim_structure(|d, _| d.rename_scene(scene, &name)),
            SceneOp::Delete => doc.anim_structure(|d, _| d.delete_scene(scene)),
            SceneOp::AddTrack => doc.anim_structure(|d, m| d.add_model_track(scene, m, &m.name)),
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
            .on_hover_text("Play / Pause (Space)")
            .clicked()
        {
            st.playing = !st.playing;
        }
        if ui
            .button("⏹")
            .on_hover_text("Stop: back to the work area start")
            .clicked()
        {
            st.playing = false;
            st.frame = st.scene().map_or(0.0, |s| f64::from(s.work_start));
        }
        ui.checkbox(&mut st.looping, "Loop");
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
        ui.checkbox(&mut st.keyed_only, "Keyed only");
        ui.add(egui::Slider::new(&mut st.px_per_frame, 1.0..=40.0).text("zoom"));
    });
    let Some(scene) = st.scene().cloned() else {
        return;
    };
    let Some((ti, m)) = st.model_track().map(|(i, m)| (i, m.clone())) else {
        ui.weak("This scene has no model track.");
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
        ui.weak("Select an attribute in the timeline to edit its curve.");
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
        if ui.button("Add key at playhead").clicked() {
            let v = eval_curve(&a.curve, st.frame);
            st.edit("Add key", |c, f| {
                set_key(c, f as i32, v, default_type);
            });
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
            if ui.button("Delete key").clicked() {
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
                st.undo.push(("Move key".into(), base));
                st.redo.clear();
                st.dirty = true;
            }
        }
    }
    resp.context_menu(|ui| {
        let Some(k) = st.key else {
            ui.weak("Click a key first.");
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
        if ui.button("Delete key").clicked() {
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
        doc.anim_structure(|d, m| d.add_scene("Two", m, 60, 30.0).map(|_| ()))
            .unwrap();
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
        let _ = std::fs::remove_dir_all(&dir);
    }
}
