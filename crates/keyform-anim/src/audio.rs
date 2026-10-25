//! Audio for the motion-sync preview: a small WAV reader and Keyform's own
//! vowel analyser. The reference uses a third-party engine that Keyform
//! does not have (spec: 70-advanced/motion-sync.md §4); this analyser is
//! documented in `docs/motion-sync-analyser.md`.

use crate::AnimError;

/// Mono PCM samples in −1…1.
#[derive(Clone, Debug, PartialEq)]
pub struct Pcm {
    pub rate: u32,
    pub samples: Vec<f32>,
}

/// Longest sound accepted (samples), ~20 minutes at 48 kHz.
const MAX_SAMPLES: usize = 60_000_000;

fn bad(msg: &str) -> AnimError {
    AnimError::Invalid(format!("wav: {msg}"))
}

/// Reads RIFF/WAVE: PCM 8/16/24/32-bit integer or 32-bit float, any number
/// of channels (mixed to mono). Untrusted input: sizes are checked.
pub fn read_wav(data: &[u8]) -> Result<Pcm, AnimError> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(bad("not a RIFF/WAVE file"));
    }
    let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut pos = 12;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let len = u32_at(data, pos + 4) as usize;
        let body = pos + 8;
        let end = body.checked_add(len).ok_or_else(|| bad("chunk size"))?;
        if id == b"fmt " {
            if len < 16 || end > data.len() {
                return Err(bad("fmt chunk"));
            }
            let c = &data[body..end];
            let mut format = u16_at(c, 0);
            if format == 0xFFFE && len >= 26 {
                // WAVE_FORMAT_EXTENSIBLE: the sub-format's first two bytes.
                format = u16_at(c, 24);
            }
            fmt = Some((format, u16_at(c, 2), u32_at(c, 4), u16_at(c, 14)));
        } else if id == b"data" {
            let (format, channels, rate, bits) = fmt.ok_or_else(|| bad("data before fmt"))?;
            if channels == 0 || rate == 0 || rate > 384_000 {
                return Err(bad("format"));
            }
            let body = &data[body..end.min(data.len())];
            let bytes = usize::from(bits / 8);
            let frame = bytes * usize::from(channels);
            if frame == 0 {
                return Err(bad("sample size"));
            }
            let n = body.len() / frame;
            if n > MAX_SAMPLES {
                return Err(bad("too long"));
            }
            let sample = |b: &[u8]| -> Option<f32> {
                Some(match (format, bits) {
                    (1, 8) => (f32::from(b[0]) - 128.0) / 128.0,
                    (1, 16) => f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0,
                    (1, 24) => {
                        (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0
                    }
                    (1, 32) => {
                        i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0
                    }
                    (3, 32) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                    _ => return None,
                })
            };
            let mut samples = Vec::with_capacity(n);
            for f in body.chunks_exact(frame) {
                let mut acc = 0.0;
                for ch in f.chunks_exact(bytes) {
                    acc += sample(ch).ok_or_else(|| bad("unsupported sample format"))?;
                }
                let v = acc / f32::from(channels);
                samples.push(if v.is_finite() {
                    v.clamp(-1.0, 1.0)
                } else {
                    0.0
                });
            }
            return Ok(Pcm { rate, samples });
        }
        // Chunks are word-aligned.
        pos = end + (len & 1);
    }
    Err(bad("no data chunk"))
}

/// The audio elements of the default mouth preset (motion-sync.md §1).
pub const ELEMENTS: [&str; 6] = ["Silence", "A", "I", "U", "E", "O"];

/// Formant prototypes (F1, F2 in Hz) of the five vowels: textbook
/// averages, not tuned to any product.
const VOWELS: [(f32, f32); 5] = [
    (750.0, 1200.0), // A
    (300.0, 2300.0), // I
    (350.0, 1300.0), // U
    (480.0, 1900.0), // E
    (500.0, 850.0),  // O
];

/// Below this level (dBFS) a frame is silent; voicing ramps up over the
/// next `VOICE_RAMP_DB`.
const SILENCE_DB: f32 = -45.0;
const VOICE_RAMP_DB: f32 = 15.0;

/// Element weights (Silence, A, I, U, E, O) at `rate` frames per second.
pub fn analyse(pcm: &Pcm, rate: f32) -> Vec<[f32; 6]> {
    if pcm.samples.is_empty() || rate <= 0.0 || pcm.rate == 0 {
        return Vec::new();
    }
    let sr = pcm.rate as f32;
    let duration = pcm.samples.len() as f32 / sr;
    let frames = (duration * rate).ceil() as usize;
    // ~30 ms analysis window centred on each frame time.
    let win = ((0.03 * sr) as usize).max(64);
    let order = (2 + pcm.rate / 1000).min(48) as usize;
    let mut out = Vec::with_capacity(frames);
    let mut buf = vec![0.0f32; win];
    for f in 0..frames {
        let centre = (f as f32 / rate * sr) as isize;
        let start = centre - win as isize / 2;
        let mut energy = 0.0f32;
        for (i, b) in buf.iter_mut().enumerate() {
            let k = start + i as isize;
            let s = if k >= 0 && (k as usize) < pcm.samples.len() {
                pcm.samples[k as usize]
            } else {
                0.0
            };
            // Hann window; pre-emphasis applied below.
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (win - 1) as f32).cos();
            *b = s * w;
            energy += s * s;
        }
        let rms = (energy / win as f32).sqrt();
        let db = 20.0 * rms.max(1e-9).log10();
        let voiced = ((db - SILENCE_DB) / VOICE_RAMP_DB).clamp(0.0, 1.0);
        let mut w = [0.0f32; 6];
        w[0] = 1.0 - voiced;
        if voiced > 0.0 {
            for i in (1..win).rev() {
                buf[i] -= 0.97 * buf[i - 1];
            }
            if let Some((f1, f2)) = formants(&buf, order, sr) {
                // Similarity in log-frequency space.
                let mut sim = [0.0f32; 5];
                let mut sum = 0.0;
                for (k, (v1, v2)) in VOWELS.iter().enumerate() {
                    let d1 = (f1 / v1).ln() / 0.25;
                    let d2 = (f2 / v2).ln() / 0.20;
                    sim[k] = (-(d1 * d1 + d2 * d2)).exp();
                    sum += sim[k];
                }
                if sum > 0.0 {
                    for k in 0..5 {
                        w[k + 1] = voiced * sim[k] / sum;
                    }
                } else {
                    w[0] = 1.0;
                }
            } else {
                w[0] = 1.0;
            }
        }
        out.push(w);
    }
    out
}

/// LPC by autocorrelation (Levinson–Durbin), then the formant peaks of
/// its envelope.
fn formants(x: &[f32], order: usize, sr: f32) -> Option<(f32, f32)> {
    let n = x.len();
    if n <= order {
        return None;
    }
    let mut r = vec![0.0f64; order + 1];
    for (k, rk) in r.iter_mut().enumerate() {
        *rk = (0..n - k)
            .map(|i| f64::from(x[i]) * f64::from(x[i + k]))
            .sum();
    }
    if r[0] <= 1e-12 {
        return None;
    }
    r[0] *= 1.0 + 1e-9;
    let mut a = vec![0.0f64; order + 1];
    a[0] = 1.0;
    let mut e = r[0];
    for i in 1..=order {
        let acc: f64 = (1..i).map(|j| a[j] * r[i - j]).sum();
        let k = -(r[i] + acc) / e;
        let prev = a.clone();
        for j in 1..i {
            a[j] = prev[j] + k * prev[i - j];
        }
        a[i] = k;
        e *= 1.0 - k * k;
        if e <= 0.0 {
            return None;
        }
    }
    // Envelope 1/|A(e^{jω})| on a 10 Hz grid.
    let step = 10.0f32;
    let mut env = Vec::new();
    let mut f = 0.0f32;
    while f <= 4000.0f32.min(sr / 2.0) {
        let w = f64::from(std::f32::consts::TAU * f / sr);
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (k, ak) in a.iter().enumerate() {
            re += ak * (w * k as f64).cos();
            im -= ak * (w * k as f64).sin();
        }
        env.push((f, 1.0 / (re * re + im * im).sqrt().max(1e-12)));
        f += step;
    }
    let peaks: Vec<(f32, f64)> = env
        .windows(3)
        .filter(|t| t[1].1 > t[0].1 && t[1].1 >= t[2].1)
        .map(|t| t[1])
        .collect();
    // F1: the strongest peak in 200–1000 Hz; F2: the strongest peak at
    // least 300 Hz above F1, up to 3 kHz.
    let strongest = |lo: f32, hi: f32| {
        peaks
            .iter()
            .filter(|(f, _)| (lo..=hi).contains(f))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|p| p.0)
    };
    let f1 = strongest(200.0, 1000.0)?;
    let f2 = strongest((f1 + 300.0).max(700.0), 3000.0)?;
    Some((f1, f2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav16(rate: u32, samples: &[f32]) -> Vec<u8> {
        let mut d = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        d.extend_from_slice(&16u32.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&rate.to_le_bytes());
        d.extend_from_slice(&(rate * 2).to_le_bytes());
        d.extend_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&16u16.to_le_bytes());
        d.extend_from_slice(b"data");
        d.extend_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
        for s in samples {
            d.extend_from_slice(&((s * 32767.0) as i16).to_le_bytes());
        }
        d
    }

    /// A crude vowel: a 120 Hz pulse train through two resonators.
    fn vowel(rate: u32, secs: f32, f1: f32, f2: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        let sr = rate as f32;
        let reson = |x: &[f32], f: f32, bw: f32| {
            let r = (-std::f32::consts::PI * bw / sr).exp();
            let c = 2.0 * r * (std::f32::consts::TAU * f / sr).cos();
            let (mut y1, mut y2) = (0.0f32, 0.0f32);
            x.iter()
                .map(|&v| {
                    let y = v + c * y1 - r * r * y2;
                    y2 = y1;
                    y1 = y;
                    y
                })
                .collect::<Vec<_>>()
        };
        let pulses: Vec<f32> = (0..n)
            .map(|i| {
                if i % (rate as usize / 120) == 0 {
                    1.0
                } else {
                    0.0
                }
            })
            .collect();
        let y = reson(&reson(&pulses, f1, 80.0), f2, 100.0);
        let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
        y.iter().map(|v| 0.5 * v / peak).collect()
    }

    #[test]
    fn reads_wav_and_tells_vowels_from_silence() {
        let rate = 16000;
        let mut s = vec![0.0; rate as usize / 4];
        s.extend(vowel(rate, 0.5, 750.0, 1200.0)); // "a"
        s.extend(vowel(rate, 0.5, 300.0, 2300.0)); // "i"
        let pcm = read_wav(&wav16(rate, &s)).unwrap();
        assert_eq!(pcm.rate, rate);
        assert_eq!(pcm.samples.len(), s.len());
        let w = analyse(&pcm, 30.0);
        let argmax = |f: &[f32; 6]| (0..6).max_by(|&a, &b| f[a].total_cmp(&f[b])).unwrap();
        assert_eq!(argmax(&w[2]), 0, "silence: {:?}", w[2]);
        assert_eq!(argmax(&w[15]), 1, "a: {:?}", w[15]);
        assert_eq!(argmax(&w[30]), 2, "i: {:?}", w[30]);
        // All five vowels, each half a second.
        for (k, (f1, f2)) in VOWELS.iter().enumerate() {
            let pcm = Pcm {
                rate,
                samples: vowel(rate, 0.5, *f1, *f2),
            };
            let w = analyse(&pcm, 30.0);
            assert_eq!(argmax(&w[7]), k + 1, "{}: {:?}", ELEMENTS[k + 1], w[7]);
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(read_wav(b"nope").is_err());
        let mut d = wav16(8000, &[0.0; 10]);
        d.truncate(30);
        assert!(read_wav(&d).is_err());
    }
}
