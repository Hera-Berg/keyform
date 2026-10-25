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
    /// Percent.
    pub blend_ratio: f32,
    pub smoothing: i32,
    /// Analysis frames per second (default 60, viewer.md §3.8).
    pub sample_rate: f32,
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
            blend_ratio: num(&s["PostProcessing"], "BlendRatio", 50.0),
            smoothing: num(&s["PostProcessing"], "Smoothing", 0.0) as i32,
            sample_rate: num(&s["PostProcessing"], "SampleRate", 60.0).max(1.0),
        });
    }
    Ok(out)
}

impl Setting {
    /// Analyses `pcm` into one parameter map per frame at `sample_rate`
    /// (motion-sync.md §3).
    pub fn frames(&self, pcm: &Pcm) -> Vec<FrameMap> {
        let weights = audio::analyse(pcm, self.sample_rate);
        // Element index of each audio parameter, by id.
        let elem = |id: &str| {
            audio::ELEMENTS
                .iter()
                .position(|e| e.eq_ignore_ascii_case(id))
        };
        let mut state: Vec<Option<f32>> = vec![None; self.cubism.len()];
        let mut out = Vec::with_capacity(weights.len());
        for w in &weights {
            // 1. Scaled, clamped element weights (disabled ones ignored).
            let ws: Vec<(f32, &Mapping)> = self
                .audio
                .iter()
                .filter(|a| a.enabled)
                .filter_map(|a| {
                    let raw = w[elem(&a.id)?];
                    let m = self.mappings.iter().find(|m| m.id == a.id)?;
                    Some(((raw * a.scale).clamp(a.min.min(a.max), a.max.max(a.min)), m))
                })
                .collect();
            let target = |m: &Mapping, id: &str| {
                m.targets.iter().find(|(t, _)| t == id).map_or(0.0, |t| t.1)
            };
            let total: f32 = ws.iter().map(|(x, _)| x).sum();
            let strongest = ws
                .iter()
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, m)| *m);
            let r = (self.blend_ratio / 100.0).clamp(0.0, 1.0);
            let mut frame = Vec::with_capacity(self.cubism.len());
            for (k, p) in self.cubism.iter().enumerate() {
                // 2. TODO(Q-0014): blend between the strongest element's
                // shape and the weighted mix.
                let one = strongest.map_or(0.0, |m| target(m, &p.id));
                let mix = if total > 0.0 {
                    ws.iter().map(|(x, m)| x * target(m, &p.id)).sum::<f32>() / total
                } else {
                    one
                };
                let x = one + (mix - one) * r;
                // 3. TODO(Q-0014): smoothing and damper.
                let s = (self.smoothing + p.smooth).max(0) as f32;
                let y = match state[k] {
                    None => x,
                    Some(prev) if (x - prev).abs() < p.damper => prev,
                    Some(prev) => prev + (x - prev) / (1.0 + s),
                };
                let y = y.clamp(p.min.min(p.max), p.max.max(p.min));
                state[k] = Some(y);
                frame.push((p.id.clone(), y));
            }
            out.push(frame);
        }
        out
    }
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
        assert_eq!(f.len(), 15);
        assert!(f.iter().all(|m| m[0].1 == 0.0));
    }
}
