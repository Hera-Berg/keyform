//! An open model and its runtime state (parameters, playback, physics).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use keyform_anim::{Expression, ExpressionPlayer, Motion, MotionPlayer, Pose, PoseState, Targets};
use keyform_eval::{EvalOptions, Evaluator, Frame, Overrides};
use keyform_formats::cmo3::Cmo3Document;
use keyform_formats::moc3::{moc3_to_model, Moc3Raw};
use keyform_formats::runtime_json::Model3;
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

pub struct MotionEntry {
    pub group: String,
    pub file: String,
    pub motion: Arc<Motion>,
}

pub struct ExpressionEntry {
    pub name: String,
    pub file: String,
    pub expression: Arc<Expression>,
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

#[allow(dead_code)] // read by viewer-settings saving (Phase 2 milestone 5)
pub enum Source {
    /// A `.cmo3` project (kept for saving in later phases).
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
    #[allow(dead_code)]
    pub source: Source,
    pub model: Model,
    /// Values set by the user (sliders).
    pub values: Vec<f32>,
    /// Values after motion, expression and physics this frame.
    pub live: Vec<f32>,
    pub param_index: HashMap<String, usize>,
    pub part_index: HashMap<String, usize>,
    pub physics_enabled: bool,
    pub sim: PhysicsSim,
    pub motions: Vec<MotionEntry>,
    pub expressions: Vec<ExpressionEntry>,
    pub pose: Option<Pose>,
    pose_state: PoseState,
    pub motion_player: MotionPlayer,
    pub expression_player: ExpressionPlayer,
    pub looping: bool,
    /// Playback clock (seconds).
    pub time: f64,
    pub frame: Option<Frame>,
    pub view: ViewState,
    pub selection: Option<Selection>,
    pub hit_areas: Vec<(String, String)>,
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
                m.textures.push(Texture {
                    guid: String::new(),
                    name: format!("texture_{i:02}"),
                    width: w,
                    height: h,
                    png: Some(png),
                    locked: true,
                });
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
            // Effect groups from model3.json (runtime-json.md §2).
            for (gname, ids) in &m3.groups {
                let list: Vec<usize> = ids.iter().filter_map(|i| pindex.get(i).copied()).collect();
                match gname.as_str() {
                    "EyeBlink" => model.eye_blink_parameters = list,
                    "LipSync" => model.lip_sync_parameters = list,
                    _ => {}
                }
            }
            for (group, files) in &m3.motions {
                for f in files {
                    match std::fs::read_to_string(dir.join(f))
                        .map_err(|e| e.to_string())
                        .and_then(|t| Motion::parse(&t).map_err(|e| e.to_string()))
                    {
                        Ok(m) => motions.push(MotionEntry {
                            group: group.clone(),
                            file: f.clone(),
                            motion: Arc::new(m),
                        }),
                        Err(e) => log.error(format!("{f}: {e}")),
                    }
                }
            }
            for (ename, f) in &m3.expressions {
                match std::fs::read_to_string(dir.join(f))
                    .map_err(|e| e.to_string())
                    .and_then(|t| Expression::parse(&t).map_err(|e| e.to_string()))
                {
                    Ok(e) => expressions.push(ExpressionEntry {
                        name: ename.clone(),
                        file: f.clone(),
                        expression: Arc::new(e),
                    }),
                    Err(e) => log.error(format!("{f}: {e}")),
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
            hit_areas = m3.hit_areas.clone();
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
        let pose_state = pose.as_ref().map(PoseState::new).unwrap_or_default();
        log.info(format!(
            "opened {name}: {} parameters, {} parts, {} art meshes, {} physics settings, {} motions, {} expressions",
            model.parameters.len(),
            model.parts.len(),
            model.art_meshes.len(),
            model.physics.settings.len(),
            motions.len(),
            expressions.len()
        ));
        Ok(Self {
            path: path.to_path_buf(),
            title: name,
            source,
            live: values.clone(),
            values,
            param_index,
            part_index,
            physics_enabled: !model.physics.settings.is_empty(),
            sim,
            motions,
            expressions,
            pose,
            pose_state,
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
            model,
        })
    }

    pub fn reset_parameters(&mut self) {
        self.values = self.model.parameters.iter().map(|p| p.default).collect();
        self.sim.reset(&self.model.physics);
    }

    pub fn play_motion(&mut self, i: usize) {
        if let Some(m) = self.motions.get(i) {
            self.motion_player
                .start(m.motion.clone(), self.time, self.looping);
        }
    }

    pub fn play_expression(&mut self, i: usize) {
        if let Some(e) = self.expressions.get(i) {
            self.expression_player
                .start(e.expression.clone(), self.time);
        }
    }

    /// Whether anything animates (the UI then repaints continuously).
    pub fn is_animating(&self) -> bool {
        self.motion_player.is_playing() || self.physics_enabled || self.pose.is_some()
    }

    /// Advances playback by `dt` seconds and evaluates the model.
    /// Order: user values → motions → expressions → physics → pose → evaluation
    /// (spec: 70-advanced/physics.md, "after animation, before evaluation").
    pub fn tick(&mut self, dt: f32, log: &mut Log) {
        self.time += f64::from(dt);
        self.live.clone_from(&self.values);
        let mut part_opacity: HashMap<String, f32> = HashMap::new();
        let mut model_opacity = 1.0f32;
        let events = {
            let mut t = Targets {
                params: &mut self.live,
                param_index: &self.param_index,
                part_opacity: &mut part_opacity,
                eye_blink: &self.model.eye_blink_parameters,
                lip_sync: &self.model.lip_sync_parameters,
                model_opacity: &mut model_opacity,
            };
            self.motion_player.update(self.time, &mut t)
        };
        for e in events {
            log.info(format!("motion event: {e}"));
        }
        self.expression_player
            .update(self.time, &mut self.live, &self.param_index);
        if self.physics_enabled {
            self.sim.step(
                &self.model.physics,
                &self.model.parameters,
                &mut self.live,
                dt,
            );
        }
        let mut ov = Overrides {
            part_opacity: vec![None; self.model.parts.len()],
            model_opacity,
        };
        if let Some(pose) = &self.pose {
            for (id, o) in self
                .pose_state
                .update(pose, dt, &|id| part_opacity.get(id).copied())
            {
                if let Some(&pi) = self.part_index.get(&id) {
                    ov.part_opacity[pi] = Some(o);
                }
            }
        }
        for (id, o) in &part_opacity {
            if let Some(&pi) = self.part_index.get(id) {
                ov.part_opacity[pi].get_or_insert(*o);
            }
        }
        let ev = Evaluator::new(&self.model);
        self.frame = Some(ev.evaluate_full(&self.live, EvalOptions::default(), &ov));
    }
}
