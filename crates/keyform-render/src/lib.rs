//! Keyform rendering. [`CpuRenderer`] is the deterministic reference
//! rasterizer used for headless rendering and tests.
//!
//! spec: 40-rendering/

pub mod blend;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod image;

use std::collections::HashMap;

use keyform_eval::Frame;
use keyform_model::{AlphaComposition, ColorComposition, MeshIndex, Model};

pub use image::{RgbaImage, Texture};

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("image: {0}")]
    Image(String),
    #[error("invalid render options: {0}")]
    Options(String),
    #[error("gpu: {0}")]
    Gpu(String),
}

/// Maps canvas pixels to output pixels: `out = (canvas − origin) · scale`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub width: u32,
    pub height: u32,
    pub origin: [f32; 2],
    pub scale: f32,
}

impl View {
    /// The whole canvas scaled to fit `max_side` pixels on its longer side.
    pub fn fit_canvas(m: &Model, max_side: u32) -> Self {
        let (w, h) = (m.canvas.width.max(1) as f32, m.canvas.height.max(1) as f32);
        let scale = max_side as f32 / w.max(h);
        Self {
            width: (w * scale).round().max(1.0) as u32,
            height: (h * scale).round().max(1.0) as u32,
            origin: [0.0, 0.0],
            scale,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    pub view: View,
    /// Premultiplied background; `None` = transparent.
    pub background: Option<[f32; 4]>,
    /// Samples per axis (1 = none). The image is rendered larger and box-filtered.
    pub supersample: u32,
}

/// Prepared textures for one model.
pub struct CpuRenderer<'m> {
    model: &'m Model,
    textures: Vec<Option<Texture>>,
    pub warnings: Vec<String>,
}

type V2 = [f32; 2];

struct Target {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

impl<'m> CpuRenderer<'m> {
    pub fn new(model: &'m Model) -> Self {
        let mut warnings = Vec::new();
        let textures = model
            .textures
            .iter()
            .map(|t| match t.png.as_deref().map(RgbaImage::decode_png) {
                Some(Ok(img)) => Some(Texture::from_image(&img)),
                Some(Err(e)) => {
                    warnings.push(format!("texture {:?}: {e}", t.name));
                    None
                }
                None => {
                    warnings.push(format!("texture {:?} has no image data", t.name));
                    None
                }
            })
            .collect();
        Self {
            model,
            textures,
            warnings,
        }
    }

    pub fn render(&self, frame: &Frame, opts: &RenderOptions) -> Result<RgbaImage, RenderError> {
        let ss = opts.supersample.clamp(1, 8) as usize;
        let v = opts.view;
        if v.width == 0
            || v.height == 0
            || v.width > 16384
            || v.height > 16384
            || v.scale.is_nan()
            || v.scale <= 0.0
        {
            return Err(RenderError::Options(format!(
                "{}×{} at scale {}",
                v.width, v.height, v.scale
            )));
        }
        let (w, h) = (v.width as usize * ss, v.height as usize * ss);
        let mut t = Target {
            w,
            h,
            px: vec![opts.background.unwrap_or([0.0; 4]); w * h],
        };
        let scale = v.scale * ss as f32;
        let to_px =
            |p: &[f32]| -> V2 { [(p[0] - v.origin[0]) * scale, (p[1] - v.origin[1]) * scale] };

        let m = self.model;
        let mut mask_cache: HashMap<(Vec<MeshIndex>, bool), Vec<f32>> = HashMap::new();
        let mut written = vec![0u32; w * h];
        let mut stamp = 0u32;
        for &mi in &frame.render_order {
            let (Some(am), Some(ms)) = (m.art_meshes.get(mi), frame.meshes.get(mi)) else {
                continue;
            };
            if !ms.is_drawn() {
                continue;
            }
            let Some(tex) = am
                .texture
                .and_then(|ti| self.textures.get(ti))
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let pts: Vec<V2> = ms.positions.chunks_exact(2).map(to_px).collect();
            let mask = if am.clip.is_empty() {
                None
            } else {
                let key = (am.clip.clone(), am.invert_clip);
                if !mask_cache.contains_key(&key) {
                    let mk = self.build_mask(frame, &am.clip, am.invert_clip, w, h, &to_px);
                    mask_cache.insert(key.clone(), mk);
                }
                mask_cache.get(&key)
            };
            stamp = stamp.wrapping_add(1);
            let simple = blend::is_simple(am.color_composition, am.alpha_composition);
            let shade = Shade {
                alpha: ms.opacity,
                mul: ms.multiply,
                scr: ms.screen,
                cc: am.color_composition,
                ac: am.alpha_composition,
                simple,
            };
            for tri in am.indices.chunks_exact(3) {
                let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
                let (Some(&p0), Some(&p1), Some(&p2)) = (pts.get(i0), pts.get(i1), pts.get(i2))
                else {
                    continue;
                };
                // spec: blending.md §4 — front face has positive signed area in y-down canvas space.
                let area = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
                if am.culling && area <= 0.0 {
                    continue;
                }
                let uv = |i: usize| -> V2 {
                    [
                        am.uvs.get(2 * i).copied().unwrap_or(0.0),
                        am.uvs.get(2 * i + 1).copied().unwrap_or(0.0),
                    ]
                };
                raster_triangle(
                    &mut t,
                    [p0, p1, p2],
                    [uv(i0), uv(i1), uv(i2)],
                    |x, y, u, vv, t| {
                        let idx = y * t.w + x;
                        if !simple {
                            // One blend per pixel per mesh (blending.md §2).
                            if written[idx] == stamp {
                                return;
                            }
                            written[idx] = stamp;
                        }
                        let k = mask.map_or(1.0, |m| m[idx]);
                        shade.apply(tex.sample(u, vv), k, &mut t.px[idx]);
                    },
                );
            }
        }

        Ok(finish_image(
            &t.px,
            w,
            v.width as usize,
            v.height as usize,
            ss,
        ))
    }

    /// Mask image from clip sources' texture alpha, accumulated with "over";
    /// opacity and colors of the sources do not count (clipping.md §1). Hidden
    /// sources still mask in the editor (§3).
    fn build_mask(
        &self,
        frame: &Frame,
        clip: &[MeshIndex],
        invert: bool,
        w: usize,
        h: usize,
        to_px: &dyn Fn(&[f32]) -> V2,
    ) -> Vec<f32> {
        let mut t = Target {
            w,
            h,
            px: vec![[0.0; 4]; w * h],
        };
        let m = self.model;
        for &ci in clip {
            let (Some(am), Some(ms)) = (m.art_meshes.get(ci), frame.meshes.get(ci)) else {
                continue;
            };
            let Some(tex) = am
                .texture
                .and_then(|ti| self.textures.get(ti))
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let pts: Vec<V2> = ms.positions.chunks_exact(2).map(to_px).collect();
            for tri in am.indices.chunks_exact(3) {
                let ix = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
                let (Some(&p0), Some(&p1), Some(&p2)) =
                    (pts.get(ix[0]), pts.get(ix[1]), pts.get(ix[2]))
                else {
                    continue;
                };
                let uv = |i: usize| -> V2 {
                    [
                        am.uvs.get(2 * i).copied().unwrap_or(0.0),
                        am.uvs.get(2 * i + 1).copied().unwrap_or(0.0),
                    ]
                };
                raster_triangle(
                    &mut t,
                    [p0, p1, p2],
                    [uv(ix[0]), uv(ix[1]), uv(ix[2])],
                    |x, y, u, v, t| {
                        let a = tex.sample(u, v)[3];
                        let p = &mut t.px[y * t.w + x][3];
                        *p = a + *p * (1.0 - a);
                    },
                );
            }
        }
        t.px.iter()
            .map(|p| if invert { 1.0 - p[3] } else { p[3] })
            .collect()
    }
}

pub(crate) fn check_view(v: &View) -> Result<(), RenderError> {
    if v.width == 0
        || v.height == 0
        || v.width > 16384
        || v.height > 16384
        || v.scale.is_nan()
        || v.scale <= 0.0
    {
        return Err(RenderError::Options(format!(
            "{}×{} at scale {}",
            v.width, v.height, v.scale
        )));
    }
    Ok(())
}

/// Box-filters a premultiplied `ss`× supersampled buffer of width `w` down to
/// `ow`×`oh` and converts to straight 8-bit RGBA.
pub(crate) fn finish_image(
    px: &[[f32; 4]],
    w: usize,
    ow: usize,
    oh: usize,
    ss: usize,
) -> RgbaImage {
    let mut out = RgbaImage::new(ow as u32, oh as u32);
    let n = (ss * ss) as f32;
    for y in 0..oh {
        for x in 0..ow {
            let mut acc = [0.0f32; 4];
            for sy in 0..ss {
                for sx in 0..ss {
                    let p = px[(y * ss + sy) * w + x * ss + sx];
                    for i in 0..4 {
                        acc[i] += p[i].clamp(0.0, 1.0);
                    }
                }
            }
            let a = acc[3] / n;
            let o = 4 * (y * ow + x);
            for i in 0..3 {
                let c = if a > 0.0 { acc[i] / n / a } else { 0.0 };
                out.data[o + i] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            out.data[o + 3] = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    out
}

struct Shade {
    alpha: f32,
    mul: [f32; 3],
    scr: [f32; 3],
    cc: ColorComposition,
    ac: AlphaComposition,
    simple: bool,
}

impl Shade {
    /// spec: appearance.md §4, clipping.md §2, blending.md §2–3
    fn apply(&self, texel: [f32; 4], k: f32, dst: &mut [f32; 4]) {
        let mut c = texel;
        for i in 0..3 {
            c[i] *= self.mul[i];
            c[i] = c[i] + self.scr[i] * c[3] - c[i] * self.scr[i];
        }
        if self.simple {
            let mut src = c.map(|x| x * self.alpha);
            for i in 0..3 {
                src[i] = src[i].min(src[3]);
            }
            let src = src.map(|x| x * k);
            *dst = blend::blend_simple(self.cc, src, *dst);
        } else {
            *dst = blend::blend_advanced(self.cc, self.ac, c, self.alpha * k, *dst);
        }
    }
}

/// Rasterizes a triangle in output pixels, calling `f(x, y, u, v)` for every
/// covered pixel centre with perspective-free UV interpolation.
fn raster_triangle(
    t: &mut Target,
    p: [V2; 3],
    uv: [V2; 3],
    mut f: impl FnMut(usize, usize, f32, f32, &mut Target),
) {
    let area =
        (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
    if area.abs() < 1e-12 || !area.is_finite() {
        return;
    }
    let minx = p
        .iter()
        .map(|q| q[0])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0);
    let maxx = p
        .iter()
        .map(|q| q[0])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(t.w as f32);
    let miny = p
        .iter()
        .map(|q| q[1])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0);
    let maxy = p
        .iter()
        .map(|q| q[1])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(t.h as f32);
    if minx >= maxx || miny >= maxy {
        return;
    }
    let inv = 1.0 / area;
    let edge =
        |a: V2, b: V2, x: f32, y: f32| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
    // Sample points are nudged off the exact pixel centre so that shared edges
    // through centres are owned by exactly one triangle.
    const NUDGE: [f32; 2] = [0.5 + 1.0 / 4096.0, 0.5 + 1.0 / 8192.0];
    for y in miny as usize..maxy as usize {
        let sy = y as f32 + NUDGE[1];
        for x in minx as usize..maxx as usize {
            let sx = x as f32 + NUDGE[0];
            let w0 = edge(p[1], p[2], sx, sy) * inv;
            let w1 = edge(p[2], p[0], sx, sy) * inv;
            let w2 = edge(p[0], p[1], sx, sy) * inv;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let u = w0 * uv[0][0] + w1 * uv[1][0] + w2 * uv[2][0];
            let v = w0 * uv[0][1] + w1 * uv[1][1] + w2 * uv[2][1];
            f(x, y, u, v, t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_edge_is_covered_once() {
        let mut t = Target {
            w: 8,
            h: 8,
            px: vec![[0.0; 4]; 64],
        };
        let quad = [[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 8.0]];
        let mut hits = vec![0u32; 64];
        for tri in [[0, 1, 2], [0, 2, 3]] {
            raster_triangle(
                &mut t,
                tri.map(|i| quad[i]),
                [[0.0; 2]; 3],
                |x, y, _, _, _| hits[y * 8 + x] += 1,
            );
        }
        assert!(hits.iter().all(|&h| h == 1), "{hits:?}");
    }
}
