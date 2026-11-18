//! Motion-sync: motionsync3.json settings, the analysed per-frame parameter
//! maps, and their application in the preview.
//! spec: 70-advanced/motion-sync.md §1–3, 70-advanced/viewer.md §3.8

use serde_json::Value;

use crate::audio::{self, Pcm};
use crate::player::Params;
use crate::AnimError;

#[derive(Clone, Debug, PartialEq)]
pub struct CubismParameter {
    pub id: String,
    pub min: f32,
    pub max: f32,
    pub damper: f32,
    pub smooth: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioParameter {
    pub id: String,
    pub min: f32,
    pub max: f32,
    pub scale: f32,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mapping {
    /// Audio parameter id.
    pub id: String,
    /// (Cubism parameter id, target value).
    pub targets: Vec<(String, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Setting {
    pub id: String,
    pub name: String,
    pub cubism: Vec<CubismParameter>,
    pub audio: Vec<AudioParameter>,
    pub mappings: Vec<Mapping>,
    /// Blend ratio 0–1 (stored and exported as e.g. 0.5, observed; the
    /// spec calls it a percentage). Used by Keyform's analysis engine
    /// (motion-sync.md §4). TODO(Q-0039)
    pub blend_ratio: f32,
    /// 0–100. Used by Keyform's analysis engine.
    pub smoothing: i32,
    /// Percent. Used by Keyform's analysis engine.
    pub audio_level_effect_ratio: f32,
    /// Analysis frames per second (default 60, viewer.md §3.8).
    pub sample_rate: f32,
}

impl Setting {
    /// The engine view of a model's motion-sync setting (.cmo3).
    pub fn from_model(s: &keyform_model::MotionSyncSetting) -> Self {
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            cubism: s
                .cubism
                .iter()
                .map(|c| CubismParameter {
                    id: c.parameter_id.clone(),
                    min: c.min,
                    max: c.max,
                    damper: c.damper,
                    smooth: c.smooth,
                })
                .collect(),
            audio: s
                .audio
                .iter()
                .map(|a| AudioParameter {
                    id: a.audio_id.clone(),
                    min: a.min,
                    max: a.max,
                    scale: a.scale,
                    enabled: a.enabled,
                })
                .collect(),
            mappings: s
                .mappings
                .iter()
                .map(|m| Mapping {
                    id: m.audio_id.clone(),
                    targets: m.targets.clone(),
                })
                .collect(),
            blend_ratio: s.blend,
            smoothing: s.smoothing,
            audio_level_effect_ratio: 0.0,
            sample_rate: s.sample_rate.max(1.0),
        }
    }
}

/// Settings stored at the 5.0.00 formula (version 0) are corrected on
/// open: the blend value is halved and the version set to 1. Returns
/// whether anything changed. spec: motion-sync.md §2 "Pre-5.0.01 settings"
pub fn correct_v0(settings: &mut [keyform_model::MotionSyncSetting]) -> bool {
    let mut changed = false;
    for s in settings.iter_mut().filter(|s| s.version == 0) {
        s.blend *= 0.5;
        s.version = 1;
        changed = true;
    }
    changed
}

/// One analysis frame: parameter id → value.
pub type FrameMap = Vec<(String, f32)>;

fn num(v: &Value, k: &str, d: f32) -> f32 {
    v[k].as_f64().map_or(d, |x| x as f32)
}

fn text(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_owned()
}

/// Parses motionsync3.json (Version 1). spec: motion-sync.md §2
pub fn parse(json: &str) -> Result<Vec<Setting>, AnimError> {
    let v: Value = serde_json::from_str(json).map_err(|e| AnimError::Json(e.to_string()))?;
    if v["Version"].as_i64() != Some(1) {
        return Err(AnimError::Invalid(format!(
            "motionsync3.json version {} is not supported",
            v["Version"]
        )));
    }
    let names: Vec<(String, String)> = v["Meta"]["Dictionary"]
        .as_array()
        .map(|d| d.iter().map(|e| (text(e, "Id"), text(e, "Name"))).collect())
        .unwrap_or_default();
    let list = v["Settings"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();
    for s in list.iter().take(256) {
        let id = text(s, "Id");
        let arr = |k: &str| s[k].as_array().cloned().unwrap_or_default();
        out.push(Setting {
            name: names
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| id.clone()),
            id,
            cubism: arr("CubismParameters")
                .iter()
                .map(|p| CubismParameter {
                    id: text(p, "Id"),
                    min: num(p, "Min", 0.0),
                    max: num(p, "Max", 1.0),
                    damper: num(p, "Damper", 0.0),
                    smooth: num(p, "Smooth", 0.0) as i32,
                })
                .collect(),
            audio: arr("AudioParameters")
                .iter()
                .map(|p| AudioParameter {
                    id: text(p, "Id"),
                    min: num(p, "Min", 0.0),
                    max: num(p, "Max", 1.0),
                    scale: num(p, "Scale", 1.0),
                    enabled: p["Enabled"].as_bool().unwrap_or(true),
                })
                .collect(),
            mappings: arr("Mappings")
                .iter()
                .map(|m| Mapping {
                    id: text(m, "Id"),
                    targets: m["Targets"]
                        .as_array()
                        .map(|t| {
                            t.iter()
                                .map(|x| (text(x, "Id"), num(x, "Value", 0.0)))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect(),
            blend_ratio: num(&s["PostProcessing"], "BlendRatio", 0.5),
            smoothing: num(&s["PostProcessing"], "Smoothing", 0.0) as i32,
            audio_level_effect_ratio: num(&s["PostProcessing"], "AudioLevelEffectRatio", 0.0),
            sample_rate: num(&s["PostProcessing"], "SampleRate", 60.0).max(1.0),
        });
    }
    Ok(out)
}

impl Setting {
    /// Analyses `pcm` into one parameter map per frame at `sample_rate`
    /// (motion-sync.md §3): engine input conversion, 100 fps engine rows,
    /// resampling, then the per-parameter post-processing. A parameter
    /// missing from a frame is not written in that frame.
    pub fn frames(&self, pcm: &Pcm) -> Vec<FrameMap> {
        let input = audio::to_engine_pcm(pcm);
        let analysis = audio::analyse(&input, audio::ENGINE_FPS);
        let rows = self.engine(&analysis);
        let rows = resample(&rows, audio::ENGINE_FPS / self.sample_rate);
        self.post_process(&rows)
    }

    /// The Silence element's target for each Cubism parameter.
    fn silence_targets(&self) -> Vec<Option<f32>> {
        let silence = self
            .mappings
            .iter()
            .find(|m| m.id.eq_ignore_ascii_case("Silence"));
        self.cubism
            .iter()
            .map(|p| silence.and_then(|m| m.targets.iter().find(|(t, _)| *t == p.id).map(|t| t.1)))
            .collect()
    }

    /// Keyform's analysis engine: one row per analysis frame with a value
    /// per Cubism parameter (NaN = no value). The blending, `Smoothing` and
    /// `AudioLevelEffectRatio` rules are Keyform's own design
    /// (motion-sync.md §4, docs/motion-sync-analyser.md).
    fn engine(&self, analysis: &[audio::Analysis]) -> Vec<Vec<f32>> {
        let elem = |id: &str| {
            audio::ELEMENTS
                .iter()
                .position(|e| e.eq_ignore_ascii_case(id))
        };
        // Enabled elements with a mapping: (element index, Scale, mapping).
        // Audio Min/Max are not engine inputs.
        let elements: Vec<(usize, f32, &Mapping)> = self
            .audio
            .iter()
            .filter(|a| a.enabled)
            .filter_map(|a| {
                let m = self.mappings.iter().find(|m| m.id == a.id)?;
                Some((elem(&a.id)?, a.scale, m))
            })
            .collect();
        let silence = self.silence_targets();
        let r = self.blend_ratio.clamp(0.0, 1.0);
        let level_effect = (self.audio_level_effect_ratio / 100.0).clamp(0.0, 1.0);
        let smoothing = self.smoothing.clamp(0, 100) as f32;
        let mut state: Vec<Option<f32>> = vec![None; self.cubism.len()];
        let mut rows = Vec::with_capacity(analysis.len());
        for a in analysis {
            let mut row = Vec::with_capacity(self.cubism.len());
            for (k, p) in self.cubism.iter().enumerate() {
                // (weight, target) of the elements with a target here.
                let ws: Vec<(f32, f32)> = elements
                    .iter()
                    .filter_map(|&(e, scale, m)| {
                        let t = m.targets.iter().find(|(t, _)| *t == p.id)?.1;
                        Some(((a.weights[e] * scale).max(0.0), t))
                    })
                    .collect();
                let total: f32 = ws.iter().map(|w| w.0).sum();
                let mut x = if ws.is_empty() {
                    f32::NAN
                } else if total <= 0.0 {
                    silence[k].unwrap_or(f32::NAN)
                } else {
                    let mix = ws.iter().map(|(w, t)| w * t).sum::<f32>() / total;
                    let one = ws
                        .iter()
                        .max_by(|a, b| a.0.total_cmp(&b.0))
                        .map_or(mix, |w| w.1);
                    one + (mix - one) * r
                };
                // Quiet frames pull the shape toward Silence.
                if let Some(s0) = silence[k] {
                    x = s0 + (x - s0) * (1.0 - level_effect + level_effect * a.level);
                }
                if x.is_nan() {
                    state[k] = None;
                } else {
                    let y = state[k].map_or(x, |prev| {
                        ((100.0 - smoothing) * x + smoothing * prev) / 100.0
                    });
                    state[k] = Some(y);
                    x = y;
                }
                row.push(x);
            }
            rows.push(row);
        }
        rows
    }

    /// Per-parameter post-processing (motion-sync.md §3 step 4): row 0 is
    /// the Silence targets; later rows are smoothed by `Smooth` and held by
    /// the `Damper` dead band; "no value" writes nothing and resets.
    pub fn post_process(&self, rows: &[Vec<f32>]) -> Vec<FrameMap> {
        let silence = self.silence_targets();
        let mut smoothed = silence.clone();
        let mut output = silence.clone();
        let mut out = Vec::with_capacity(rows.len());
        for (r, row) in rows.iter().enumerate() {
            let mut frame = FrameMap::new();
            for (k, p) in self.cubism.iter().enumerate() {
                if r == 0 {
                    if let Some(v) = silence[k] {
                        frame.push((p.id.clone(), v));
                    }
                    continue;
                }
                let x = row.get(k).copied().unwrap_or(f32::NAN);
                if x.is_nan() {
                    smoothed[k] = None;
                    output[k] = None;
                    continue;
                }
                let v = match (smoothed[k], output[k]) {
                    (Some(prev), Some(last)) => {
                        let s = p.smooth.clamp(0, 100) as f32;
                        let v = ((100.0 - s) * x + s * prev) / 100.0;
                        smoothed[k] = Some(v);
                        if (v - last).abs() < p.damper {
                            last
                        } else {
                            v
                        }
                    }
                    _ => {
                        smoothed[k] = Some(x);
                        x
                    }
                };
                output[k] = Some(v);
                frame.push((p.id.clone(), v));
            }
            out.push(frame);
        }
        out
    }
}

/// Resamples engine rows (100 fps) by step `q` = 100 / SampleRate:
/// ⌊rows / q⌋ rows, row k blending source rows ⌊kq⌋ and the next one
/// (motion-sync.md §3 step 3).
pub fn resample(rows: &[Vec<f32>], q: f32) -> Vec<Vec<f32>> {
    if rows.is_empty() || q.is_nan() || q <= 0.0 {
        return Vec::new();
    }
    let q = f64::from(q);
    let n = (rows.len() as f64 / q).floor() as usize;
    let last = rows.len() - 1;
    (0..n)
        .map(|k| {
            let t = k as f64 * q;
            let i = (t.floor() as usize).min(last);
            let u = (t - t.floor()) as f32;
            let (a, b) = (&rows[i], &rows[(i + 1).min(last)]);
            a.iter().zip(b).map(|(x, y)| x + (y - x) * u).collect()
        })
        .collect()
}

/// Applies the analysed frames at audio position `t_us` (µs since the
/// sound started). spec: viewer.md §3.8 steps 1–4
pub fn apply(frames: &[FrameMap], rate: f32, t_us: f64, p: &mut Params) {
    if frames.is_empty() || t_us < 0.0 {
        return;
    }
    let f = t_us * f64::from(rate) / 1e6;
    let (i0, i1) = (f.floor() as usize, f.ceil() as usize);
    if i1 >= frames.len() {
        return;
    }
    // Parameters not in the model are skipped.
    let real = |p: &Params, id: &str| p.index.contains_key(id);
    if i0 == i1 {
        for (id, v) in &frames[i0] {
            if real(p, id) {
                p.set(id, *v, 1.0);
            }
        }
        return;
    }
    let u = (f - i0 as f64) as f32;
    let (a, b) = (&frames[i0], &frames[i1]);
    for (id, v0) in a {
        if !real(p, id) {
            continue;
        }
        let v = match b.iter().find(|(k, _)| k == id) {
            Some((_, v1)) => v0 + (v1 - v0) * u,
            None => *v0,
        };
        p.set(id, v, 1.0);
    }
    for (id, v1) in b {
        if real(p, id) && !a.iter().any(|(k, _)| k == id) {
            let cur = p.get(id);
            p.set(id, cur * (1.0 - u) + v1 * u, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::ParamRange;
    use std::collections::HashMap;

    const JSON: &str = r#"{"Version":1,"Meta":{"SettingCount":1,"Dictionary":[{"Id":"S","Name":"Mouth"}]},
      "Settings":[{"Id":"S","AnalysisType":"CRI","UseCase":"Mouth",
        "CubismParameters":[{"Name":"Open","Id":"ParamMouthOpenY","Min":0,"Max":1,"Damper":0,"Smooth":0}],
        "AudioParameters":[{"Name":"A","Id":"A","Min":0,"Max":1,"Scale":1,"Enabled":true},
                           {"Name":"Silence","Id":"Silence","Min":0,"Max":1,"Scale":1,"Enabled":true}],
        "Mappings":[{"Type":"Shape","Id":"A","Targets":[{"Id":"ParamMouthOpenY","Value":1}]},
                    {"Type":"Shape","Id":"Silence","Targets":[{"Id":"ParamMouthOpenY","Value":0}]}],
        "PostProcessing":{"BlendRatio":100,"Smoothing":0,"SampleRate":30}}]}"#;

    #[test]
    fn parses_and_applies_with_interpolation() {
        let s = parse(JSON).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].name.as_str(), s[0].sample_rate), ("Mouth", 30.0));
        assert!(parse(r#"{"Version":2}"#).is_err());
        let frames = vec![
            vec![("ParamMouthOpenY".to_owned(), 0.0)],
            vec![("ParamMouthOpenY".to_owned(), 1.0)],
            vec![("Missing".to_owned(), 1.0)],
        ];
        let mut values = vec![0.3];
        let ranges = vec![ParamRange {
            min: 0.0,
            max: 1.0,
            repeat: false,
        }];
        let index = HashMap::from([("ParamMouthOpenY".to_owned(), 0)]);
        let mut virtuals = HashMap::new();
        let mut p = Params {
            values: &mut values,
            ranges: &ranges,
            index: &index,
            virtuals: &mut virtuals,
        };
        // Between frames 0 and 1 (30 fps): 1/60 s → u = 0.5.
        apply(&frames, 30.0, 1e6 / 60.0, &mut p);
        assert!((p.values[0] - 0.5).abs() < 1e-6);
        // Frame 1 → 2: the parameter is only in frame 1, so v₀ is set.
        apply(&frames, 30.0, 1e6 * 1.5 / 30.0, &mut p);
        assert_eq!(p.values[0], 1.0);
        // Past the end: nothing.
        p.values[0] = 0.2;
        apply(&frames, 30.0, 1e6, &mut p);
        assert_eq!(p.values[0], 0.2);
        assert!(virtuals.is_empty(), "unknown ids are skipped");
    }

    #[test]
    fn silence_closes_the_mouth() {
        let s = &parse(JSON).unwrap()[0];
        let pcm = Pcm {
            rate: 16000,
            samples: vec![0.0; 8000],
        };
        let f = s.frames(&pcm);
        // 0.5 s → 50 engine rows → 15 rows at 30 fps.
        assert_eq!(f.len(), 15);
        assert!(f.iter().all(|m| m[0].1 == 0.0));
    }

    #[test]
    fn resampling() {
        let rows: Vec<Vec<f32>> = (0..10).map(|i| vec![i as f32]).collect();
        // 100 → 30 fps: q = 10/3, ⌊10 / q⌋ = 3 rows at 0, 3.33, 6.67.
        let r = resample(&rows, 100.0 / 30.0);
        assert_eq!(r.len(), 3);
        assert!((r[1][0] - 10.0 / 3.0).abs() < 1e-5);
        assert!((r[2][0] - 20.0 / 3.0).abs() < 1e-5);
        // Upsampling holds the last row at the end.
        let r = resample(&rows, 0.5);
        assert_eq!(r.len(), 20);
        assert_eq!(r[19][0], 9.0);
        assert_eq!(r[3][0], 1.5);
    }

    #[test]
    fn post_processing_formulas() {
        let mut s = parse(JSON).unwrap().remove(0);
        s.cubism[0].smooth = 50;
        s.cubism[0].damper = 0.1;
        let rows: Vec<Vec<f32>> = [9.0, 1.0, 1.0, 1.0, f32::NAN, 0.6, 0.62]
            .iter()
            .map(|&v| vec![v])
            .collect();
        let f = s.post_process(&rows);
        let v = |r: usize| f[r].first().map(|x| x.1);
        // Row 0: the Silence target, whatever the row says.
        assert_eq!(v(0), Some(0.0));
        // Smooth 50 from the Silence state: 0.5, then 0.75 and 0.875.
        assert_eq!(v(1), Some(0.5));
        assert_eq!(v(2), Some(0.75));
        // 0.875 − 0.75 ≥ 0.1 → written.
        assert_eq!(v(3), Some(0.875));
        // No value: nothing written, state reset.
        assert_eq!(v(4), None);
        // After a reset the first value is taken as is.
        assert_eq!(v(5), Some(0.6));
        // Smoothed 0.61 is within the 0.1 dead band of 0.6: held.
        assert_eq!(v(6), Some(0.6));
    }
}
