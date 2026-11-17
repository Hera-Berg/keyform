//! Audio output for animation playback: the scene's sound tracks, mixed
//! from the playhead, through the default output device (cpal).
//! spec: 60-animation/animation-model.md §2 (sound tracks)

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// A playing sound; dropping it stops the output.
pub struct AudioOut {
    _stream: cpal::Stream,
}

impl AudioOut {
    /// Plays mono `samples` at `rate` Hz from sample `start`.
    pub fn play(samples: Arc<Vec<f32>>, rate: u32, start: usize) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("No audio output device.")?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!(
                "Unsupported audio output format {:?}.",
                config.sample_format()
            ));
        }
        let channels = usize::from(config.channels());
        let out_rate = f64::from(config.sample_rate().0);
        let step = f64::from(rate) / out_rate;
        // Position in source samples, as fixed point (1/65536).
        let pos = Arc::new(AtomicU64::new((start as u64) << 16));
        let p = pos.clone();
        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _| {
                    let mut at = p.load(Ordering::Relaxed) as f64 / 65536.0;
                    for frame in data.chunks_mut(channels) {
                        let v = samples.get(at as usize).copied().unwrap_or(0.0);
                        frame.fill(v);
                        at += step;
                    }
                    p.store((at * 65536.0) as u64, Ordering::Relaxed);
                },
                |_| {},
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self { _stream: stream })
    }
}

/// The scene's unmuted sound tracks mixed into one mono buffer at `rate`
/// Hz, sample 0 = scene frame 0 (nearest-sample resampling).
pub fn mix_scene(st: &crate::animation::AnimState, rate: u32) -> Option<Vec<f32>> {
    use keyform_model::animation::TrackKind;
    let s = st.scene()?;
    let len = ((f64::from(s.duration) + 1.0) / s.fps * f64::from(rate)).ceil() as usize;
    let mut out = vec![0.0f32; len];
    let mut any = false;
    for t in s.tracks.iter().filter(|t| !t.mute) {
        let TrackKind::Sound(snd) = &t.kind else {
            continue;
        };
        let Some(pcm) = snd.resource.as_ref().and_then(|g| st.sounds.get(g)) else {
            continue;
        };
        any = true;
        let vol = snd.volume.as_ref().map_or(1.0, |v| {
            keyform_anim::scene::eval_curve(&v.curve, 0.0) as f32 / 100.0
        });
        for (i, o) in out.iter_mut().enumerate() {
            let scene_t = i as f64 / f64::from(rate);
            let frame = scene_t * s.fps;
            let local = frame - f64::from(t.start) + f64::from(t.offset);
            if local < 0.0 {
                continue;
            }
            let src_t = local / s.fps * snd.speed + f64::from(snd.start_frame) / s.fps;
            if let Some(x) = pcm.samples.get((src_t * f64::from(pcm.rate)) as usize) {
                *o += x * vol;
            }
        }
    }
    if !any {
        return None;
    }
    for o in &mut out {
        *o = o.clamp(-1.0, 1.0);
    }
    Some(out)
}
