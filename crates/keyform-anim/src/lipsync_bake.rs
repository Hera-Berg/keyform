//! Sound level analysis and Bake Lip-sync.
//! spec: 60-animation/animation-model.md §4.1

use keyform_model::animation::{Curve, CurveType};

use crate::AnimError;

/// The raw PCM of a WAV: sample rate, bytes per frame and the data bytes.
#[derive(Clone, Debug)]
pub struct RawWav {
    pub rate: u32,
    pub frame_bytes: usize,
    pub data: Vec<u8>,
}

impl RawWav {
    /// Reads the `fmt ` and `data` chunks (any PCM layout; the analysis
    /// itself reads 16-bit values as §4.1 describes).
    pub fn parse(bytes: &[u8]) -> Result<Self, AnimError> {
        let bad = |m: &str| AnimError::Invalid(format!("wav: {m}"));
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err(bad("not a RIFF/WAVE file"));
        }
        let (mut rate, mut block, mut data) = (0u32, 0usize, None);
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let id = &bytes[at..at + 4];
            let len =
                u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
                    as usize;
            let body = at + 8;
            let end = body.saturating_add(len).min(bytes.len());
            match id {
                b"fmt " if end - body >= 14 => {
                    rate = u32::from_le_bytes([
                        bytes[body + 4],
                        bytes[body + 5],
                        bytes[body + 6],
                        bytes[body + 7],
                    ]);
                    block = usize::from(u16::from_le_bytes([bytes[body + 12], bytes[body + 13]]));
                }
                b"data" => data = Some(bytes[body..end].to_vec()),
                _ => {}
            }
            at = body + len + (len & 1);
        }
        let data = data.ok_or_else(|| bad("no data chunk"))?;
        if rate == 0 || block == 0 {
            return Err(bad("no format"));
        }
        Ok(Self {
            rate,
            frame_bytes: block,
            data,
        })
    }

    /// Seconds of sound.
    pub fn duration(&self) -> f64 {
        (self.data.len() / self.frame_bytes) as f64 / f64::from(self.rate)
    }
}

/// In-place radix-2 FFT of (re, im) with n a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// The level analyser: keeps the 512-slot buffer between calls (a short
/// window leaves the previous call's tail in place, §4.1 step 3).
pub struct LevelAnalyser {
    buf: [f64; 512],
}

impl Default for LevelAnalyser {
    fn default() -> Self {
        Self { buf: [0.0; 512] }
    }
}

impl LevelAnalyser {
    /// The level at time `t` seconds. spec: §4.1 "Level at time t"
    pub fn level(&mut self, w: &RawWav, t: f64) -> f64 {
        let start = (f64::from(w.rate) * (t + 0.1)).floor().max(0.0) as usize * w.frame_bytes;
        // 2048 bytes stepping 4 bytes per sample (16-bit, first channel
        // of 4-byte frames: a mono file is read every other sample).
        let mut n = 0;
        let mut sum_abs = 0.0;
        for k in 0..512 {
            let at = start + 4 * k;
            if at + 2 > w.data.len() || 4 * k >= 2048 {
                break;
            }
            let s = f64::from(i16::from_le_bytes([w.data[at], w.data[at + 1]])) / 32767.0;
            self.buf[k] = s;
            sum_abs += s.abs();
            n += 1;
        }
        if n == 0 {
            return 0.0;
        }
        let m = sum_abs / n as f64;
        let mut re = self.buf;
        let mut im = [0.0f64; 512];
        fft(&mut re, &mut im);
        // Packed real layout: slot 0 = DC + Nyquist, slot 1 = DC − Nyquist,
        // then (re, im) of bins 1…255.
        let (dc, nyq) = (re[0], re[256]);
        let mut total = (dc + nyq).abs() + (dc - nyq).abs();
        for k in 1..256 {
            total += re[k].abs() + im[k].abs();
        }
        let f = (0.75 * total / 256.0).clamp(0.0, 1.0);
        (f + 5.0 * m) / 2.0
    }
}

/// The level table: ⌊60·duration⌋ bytes, entry k = ⌊level(k/60)·255⌋
/// (clamped to 255; the reference wraps, e.g. a level of 1.02 reads back
/// as 0.02). spec: §4.1 "Level table"
pub fn level_table(w: &RawWav) -> Vec<u8> {
    let count = (60.0 * w.duration()).floor().max(0.0) as usize;
    let mut a = LevelAnalyser::default();
    (0..count)
        .map(|k| {
            (a.level(w, k as f64 / 60.0) * 255.0)
                .floor()
                .clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// The table at time t: entry ⌊60·t⌋ / 255, 0 outside.
// TODO(Q-0032): the reference's levels are up to one byte lower than the
// §4.1 formula gives (a spectral-term factor of about 0.995).
pub fn table_level(table: &[u8], t: f64) -> f64 {
    if t < 0.0 {
        return 0.0;
    }
    table
        .get((60.0 * t).floor() as usize)
        .map_or(0.0, |&b| f64::from(b) / 255.0)
}

/// How Bake Lip-sync writes its keys.
#[derive(Clone, Copy, Debug)]
pub struct BakeOptions {
    /// The sound track's first and last frame, in the curve's frames.
    pub start: i32,
    pub end: i32,
    pub fps: f64,
    /// The attribute's default curve type.
    pub kind: CurveType,
    /// The attribute's range (clamp).
    pub range: (f64, f64),
    /// `isSmoothCurveLegacyMode`.
    pub legacy: bool,
}

/// Bake Lip-sync into `c` (the soundLevel curve): keys every 2nd frame
/// over [start, end] at the table level of time (f − start)·(1/fps) (3
/// decimals, as observed), then reduced to the first and last key and the
/// frame before every turning point or flat-run edge of the per-frame
/// curve. spec: animation-model.md §4.1 "Bake Lip-sync"
pub fn bake(c: &mut Curve, table: &[u8], o: &BakeOptions) {
    use crate::scene::{eval_curve, refresh, set_key};
    let (lo, hi) = o.range;
    // 1. Old keys in range go; even offsets get a key.
    let mut tmp = Curve {
        base: c.base,
        ..Default::default()
    };
    for (i, k) in c.keys.iter().enumerate() {
        if k.frame < o.start || k.frame > o.end {
            set_key(&mut tmp, k.frame, k.value, c.type_at(i));
        }
    }
    for f in (o.start..=o.end).step_by(2) {
        // Time = frames × (1/fps): the reference's rounding (frame 62 at
        // 30 fps reads table entry 123, not 124).
        let v = table_level(table, f64::from(f - o.start) * (1.0 / o.fps));
        set_key(&mut tmp, f, (v * 1000.0).round() / 1000.0, o.kind);
    }
    if tmp.keys.is_empty() {
        *c = tmp;
        return;
    }
    refresh(&mut tmp, lo, hi, o.legacy);
    // 2. The curve at every frame; keep the turning points.
    let (f0, f1) = (tmp.keys[0].frame, tmp.keys[tmp.keys.len() - 1].frame);
    // On a key frame the key's own value (no Bézier round-off).
    let v: Vec<f64> = (f0..=f1)
        .map(|f| match tmp.keys.binary_search_by_key(&f, |k| k.frame) {
            Ok(i) => tmp.keys[i].value,
            Err(_) => eval_curve(&tmp, f64::from(f)),
        })
        .map(|x| x.clamp(lo.min(hi), hi.max(lo)))
        .collect();
    let kept = turning_points(&v);
    // 3. Replace the keys.
    *c = Curve {
        base: c.base,
        ..Default::default()
    };
    for i in kept {
        set_key(c, f0 + i as i32, v[i], o.kind);
    }
    c.types = vec![o.kind; c.keys.len()];
}

/// Indices kept by the reduction: the first, the last, and the one before
/// every change of direction (rising / falling / flat).
pub fn turning_points(v: &[f64]) -> Vec<usize> {
    let sign = |x: f64| {
        if x > 0.0 {
            1
        } else if x < 0.0 {
            -1
        } else {
            0
        }
    };
    let mut kept = vec![0usize];
    for i in 2..v.len() {
        if sign(v[i] - v[i - 1]) != sign(v[i - 1] - v[i - 2]) && kept.last() != Some(&(i - 1)) {
            kept.push(i - 1);
        }
    }
    if v.len() > 1 && kept.last() != Some(&(v.len() - 1)) {
        kept.push(v.len() - 1);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_matches_dft() {
        let x: Vec<f64> = (0..16).map(|i| ((i * 7) % 5) as f64 - 2.0).collect();
        let (mut re, mut im) = (x.clone(), vec![0.0; 16]);
        fft(&mut re, &mut im);
        for k in 0..16 {
            let (mut r, mut i) = (0.0, 0.0);
            for (j, v) in x.iter().enumerate() {
                let a = -2.0 * std::f64::consts::PI * (j * k) as f64 / 16.0;
                r += v * a.cos();
                i += v * a.sin();
            }
            assert!((r - re[k]).abs() < 1e-9 && (i - im[k]).abs() < 1e-9);
        }
    }

    #[test]
    fn bake_keeps_turning_points() {
        // A level table rising then falling: keys at the edges and the peak.
        let table: Vec<u8> = (0..60)
            .map(|k| {
                if k < 30 {
                    (k * 8) as u8
                } else {
                    ((59 - k) * 8) as u8
                }
            })
            .collect();
        let mut c = Curve::default();
        let o = BakeOptions {
            start: 0,
            end: 59,
            fps: 60.0,
            kind: CurveType::Linear,
            range: (0.0, 1.0),
            legacy: false,
        };
        bake(&mut c, &table, &o);
        let frames: Vec<i32> = c.keys.iter().map(|k| k.frame).collect();
        assert_eq!(frames.first(), Some(&0));
        assert!(frames.len() < 10, "{frames:?}");
        assert!(c
            .keys
            .iter()
            .any(|k| (k.value - 232.0 / 255.0).abs() < 0.05));
    }

    /// 2.4 s of mono 16-bit PCM at 44.1 kHz: 2-frame (30 fps) segments
    /// from 0.1 s on, loud (rising level) alternating with silence.
    fn alternating_wav() -> Vec<u8> {
        let rate = 44100u32;
        let samples: Vec<i16> = (0..(2.4 * f64::from(rate)) as usize)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                let j = ((t - 0.1) * 15.0).floor();
                if j < 0.0 || j as i64 % 2 == 1 {
                    return 0;
                }
                let a = 0.02 + 0.006 * j;
                let w = 2.0 * std::f64::consts::PI * t;
                ((a * ((440.0 * w).sin() + 0.3 * (1320.0 * w).sin())) * 32767.0) as i16
            })
            .collect();
        let n = (samples.len() * 2) as u32;
        let mut d = Vec::new();
        d.extend_from_slice(b"RIFF");
        d.extend_from_slice(&(36 + n).to_le_bytes());
        d.extend_from_slice(b"WAVEfmt ");
        for v in [16u32, 1 | (1 << 16), rate, rate * 2, 2 | (16 << 16)] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(b"data");
        d.extend_from_slice(&n.to_le_bytes());
        for x in &samples {
            d.extend_from_slice(&x.to_le_bytes());
        }
        d
    }

    /// Bake Lip-sync of the alternating WAV against the reference (5.3.04,
    /// Animation ▸ Track ▸ Apply Lip-sync from the Audio File, saved and
    /// read back): every 2nd frame is a turning point, so all keys stay.
    /// Values agree within one level byte (Q-0032); frame 62 reads table
    /// entry 123 through the (1/fps) rounding.
    #[test]
    fn bake_matches_reference() {
        const REF: [(i32, f64); 36] = [
            (0, 0.071),
            (2, 0.004),
            (4, 0.098),
            (6, 0.0),
            (8, 0.153),
            (10, 0.0),
            (12, 0.2),
            (14, 0.0),
            (16, 0.212),
            (18, 0.0),
            (20, 0.286),
            (22, 0.0),
            (24, 0.325),
            (26, 0.0),
            (28, 0.325),
            (30, 0.0),
            (32, 0.416),
            (34, 0.0),
            (36, 0.455),
            (38, 0.0),
            (40, 0.439),
            (42, 0.0),
            (44, 0.549),
            (46, 0.0),
            (48, 0.584),
            (50, 0.051),
            (52, 0.592),
            (54, 0.0),
            (56, 0.663),
            (58, 0.059),
            (60, 0.714),
            (62, 0.573),
            (64, 0.663),
            (66, 0.0),
            (68, 0.808),
            (70, 0.0),
        ];
        let w = RawWav::parse(&alternating_wav()).unwrap();
        assert_eq!(w.frame_bytes, 2);
        // The new sound track's duration: ⌊2.4 s × 30⌋ = 72 frames.
        let table = level_table(&w);
        let mut c = Curve::default();
        let o = BakeOptions {
            start: 0,
            end: 71,
            fps: 30.0,
            kind: CurveType::Smooth,
            range: (0.0, 1.0),
            legacy: false,
        };
        bake(&mut c, &table, &o);
        let got: Vec<(i32, f64)> = c.keys.iter().map(|k| (k.frame, k.value)).collect();
        assert_eq!(got.len(), REF.len(), "{got:?}");
        for ((f, v), (rf, rv)) in got.iter().zip(REF) {
            assert_eq!(*f, rf);
            assert!((v - rv).abs() <= 0.0045, "frame {f}: {v} vs {rv}");
        }
        assert!(c.types.iter().all(|t| *t == CurveType::Smooth));
    }
}
