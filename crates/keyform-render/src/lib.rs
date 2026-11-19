//! Keyform rendering. [`CpuRenderer`] is the deterministic reference
//! rasterizer used for headless rendering and tests.
//!
//! spec: 40-rendering/

pub mod blend;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod image;

use std::collections::HashMap;

use keyform_eval::draw_order::DrawItem;
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
    /// View rotation in degrees about the output centre (clockwise on screen).
    pub rotation: f32,
    /// Mirror the view horizontally (View ▸ reflect).
    pub mirror: bool,
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
            rotation: 0.0,
            mirror: false,
        }
    }

    /// The model-image export camera at magnification `mag`: an image of
    /// ⌊W·mag⌋ × ⌊H·mag⌋ pixels, uniform scale k = min(w/W, h/H), canvas
    /// centre at the image centre, standard pixel centres. `None` if the
    /// image would be empty. spec: 50-ui/dialogs.md §20
    pub fn for_export(m: &Model, mag: f32) -> Option<Self> {
        let (cw, ch) = (m.canvas.width as f32, m.canvas.height as f32);
        let (w, h) = ((cw * mag).floor(), (ch * mag).floor());
        if !(w >= 1.0 && h >= 1.0 && cw > 0.0 && ch > 0.0) {
            return None;
        }
        let k = (w / cw).min(h / ch);
        Some(Self {
            width: w as u32,
            height: h as u32,
            origin: [cw / 2.0 - w / (2.0 * k), ch / 2.0 - h / (2.0 * k)],
            scale: k,
            rotation: 0.0,
            mirror: false,
        })
    }

    /// Canvas pixels → output pixels (before supersampling).
    pub fn to_px(&self, p: [f32; 2]) -> [f32; 2] {
        let mut x = (p[0] - self.origin[0]) * self.scale;
        let y = (p[1] - self.origin[1]) * self.scale;
        let (cx, cy) = (self.width as f32 / 2.0, self.height as f32 / 2.0);
        if self.mirror {
            x = 2.0 * cx - x;
        }
        if self.rotation == 0.0 {
            return [x, y];
        }
        let (s, c) = self.rotation.to_radians().sin_cos();
        let (dx, dy) = (x - cx, y - cy);
        [cx + c * dx - s * dy, cy + s * dx + c * dy]
    }

    /// Output pixels → canvas pixels (inverse of [`Self::to_px`]).
    pub fn to_canvas(&self, q: [f32; 2]) -> [f32; 2] {
        let (cx, cy) = (self.width as f32 / 2.0, self.height as f32 / 2.0);
        let (s, c) = self.rotation.to_radians().sin_cos();
        let (dx, dy) = (q[0] - cx, q[1] - cy);
        let mut x = cx + c * dx + s * dy;
        let y = cy - s * dx + c * dy;
        if self.mirror {
            x = 2.0 * cx - x;
        }
        [
            x / self.scale + self.origin[0],
            y / self.scale + self.origin[1],
        ]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    pub view: View,
    /// Premultiplied background; `None` = transparent.
    pub background: Option<[f32; 4]>,
    /// Samples per axis (1 = none). The image is rendered larger and box-filtered.
    pub supersample: u32,
    /// With supersampling, behave like multisampling: textures are sampled
    /// once per output pixel, at its centre and with its level of detail;
    /// only coverage is supersampled (image export, dialogs.md §20).
    pub msaa: bool,
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
            .texture_sources()
            .into_iter()
            .map(|t| {
                let t = t?;
                match t.png.map(RgbaImage::decode_png) {
                    Some(Ok(img)) => {
                        Some(Texture::new(&image::pad_image(&img, t.uv_pad), t.sampling))
                    }
                    Some(Err(e)) => {
                        warnings.push(format!("texture {:?}: {e}", t.name));
                        None
                    }
                    None => {
                        warnings.push(format!("texture {:?} has no image data", t.name));
                        None
                    }
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
        let t = Target {
            w,
            h,
            px: vec![opts.background.unwrap_or([0.0; 4]); w * h],
        };
        let ssf = ss as f32;
        let to_px = |p: &[f32]| -> V2 {
            let q = v.to_px([p[0], p[1]]);
            [q[0] * ssf, q[1] * ssf]
        };

        let m = self.model;
        let mut mask_cache: HashMap<(Vec<MeshIndex>, bool), Vec<f32>> = HashMap::new();
        let mut written = vec![0u32; w * h];
        let mut stamp = 0u32;
        // Offscreen parts (offscreen.md): a buffer per active offscreen
        // ancestor, outermost first; `t` is the bottom of the stack.
        let mut stack: Vec<(usize, Target)> = Vec::new();
        let mut base = t;
        for item in &frame.items {
            let (parent, drawn) = match *item {
                DrawItem::Mesh(mi) => match (m.art_meshes.get(mi), frame.meshes.get(mi)) {
                    (Some(am), Some(ms)) => (am.info.parent_part, ms.is_drawn()),
                    _ => continue,
                },
                DrawItem::ArtPath(k) => match (m.art_paths.get(k), frame.art_paths.get(k)) {
                    (Some(a), Some(st)) => (a.info.parent_part, st.is_drawn()),
                    _ => continue,
                },
                DrawItem::AliasMesh(k, mi) => {
                    match (m.aliases.get(k), frame.alias_mesh_state(k, mi)) {
                        (Some(a), Some(st)) if mi < m.art_meshes.len() => {
                            (a.info.parent_part, st.is_drawn())
                        }
                        _ => continue,
                    }
                }
                DrawItem::AliasArtPath(k, p) => {
                    match (m.aliases.get(k), frame.alias_path_state(k, p)) {
                        (Some(a), Some(st)) => (a.info.parent_part, st.is_drawn()),
                        _ => continue,
                    }
                }
                DrawItem::Alias(_) => continue,
            };
            if !drawn {
                continue;
            }
            let chain = offscreen_chain(m, parent);
            while !stack.is_empty()
                && !chain.starts_with(&stack.iter().map(|(p, _)| *p).collect::<Vec<_>>())
            {
                let (p, buf) = stack.pop().expect("non-empty");
                let dst = stack.last_mut().map_or(&mut base, |(_, b)| b);
                self.composite_part(frame, p, &buf, dst, &mut mask_cache, &to_px, ss, opts.msaa);
            }
            for &p in &chain[stack.len()..] {
                stack.push((
                    p,
                    Target {
                        w,
                        h,
                        px: vec![[0.0; 4]; w * h],
                    },
                ));
            }
            let t = stack.last_mut().map_or(&mut base, |(_, b)| b);
            let (mi, alias_state) = match *item {
                DrawItem::Mesh(mi) => (mi, None),
                DrawItem::AliasMesh(k, mi) => (mi, frame.alias_mesh_state(k, mi)),
                DrawItem::ArtPath(k) => {
                    draw_stroke(t, &frame.art_paths[k], &to_px);
                    continue;
                }
                DrawItem::AliasArtPath(k, p) => {
                    if let Some(st) = frame.alias_path_state(k, p) {
                        draw_stroke(t, &st, &to_px);
                    }
                    continue;
                }
                DrawItem::Alias(_) => continue,
            };
            // TODO(Q-0046): alias clip lists, offscreen and compositions.
            let am = &m.art_meshes[mi];
            let ms = alias_state.as_ref().unwrap_or(&frame.meshes[mi]);
            let Some(tex) = m
                .texture_slot(am)
                .and_then(|ti| self.textures.get(ti))
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let pts: Vec<V2> = ms.positions.chunks_exact(2).map(to_px).collect();
            // The "illegal" look (blending.md §5): no mask, culling or
            // colour effects.
            let illegal = m.illegal_look(am);
            let mask = if am.clip.is_empty() || illegal {
                None
            } else {
                let key = (am.clip.clone(), am.invert_clip);
                if !mask_cache.contains_key(&key) {
                    let mk = self.build_mask(
                        frame,
                        &am.clip,
                        am.invert_clip,
                        w,
                        h,
                        &to_px,
                        ss,
                        opts.msaa,
                    );
                    mask_cache.insert(key.clone(), mk);
                }
                mask_cache.get(&key)
            };
            stamp = stamp.wrapping_add(1);
            let simple = illegal || blend::is_simple(am.color_composition, am.alpha_composition);
            let shade = Shade {
                alpha: ms.opacity,
                mul: ms.multiply,
                scr: ms.screen,
                cc: am.color_composition,
                ac: am.alpha_composition,
                simple,
                illegal,
            };
            for tri in am.indices.chunks_exact(3) {
                let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
                let (Some(&p0), Some(&p1), Some(&p2)) = (pts.get(i0), pts.get(i1), pts.get(i2))
                else {
                    continue;
                };
                // spec: blending.md §4 — front face has positive signed area in
                // y-down canvas space, so a mirrored or rotated view keeps the
                // same triangles visible.
                let cp = |i: usize| [ms.positions[2 * i], ms.positions[2 * i + 1]];
                let (c0, c1, c2) = (cp(i0), cp(i1), cp(i2));
                let area = (c1[0] - c0[0]) * (c2[1] - c0[1]) - (c1[1] - c0[1]) * (c2[0] - c0[0]);
                if am.culling && !illegal && area <= 0.0 {
                    continue;
                }
                let uv = |i: usize| -> V2 {
                    [
                        am.uvs.get(2 * i).copied().unwrap_or(0.0),
                        am.uvs.get(2 * i + 1).copied().unwrap_or(0.0),
                    ]
                };
                let tri_uv = [uv(i0), uv(i1), uv(i2)];
                let g = Sampler::new([p0, p1, p2], tri_uv, ss, opts.msaa);
                raster_triangle(t, [p0, p1, p2], tri_uv, |x, y, u, vv, t| {
                    let idx = y * t.w + x;
                    if !simple {
                        // One blend per pixel per mesh (blending.md §2).
                        if written[idx] == stamp {
                            return;
                        }
                        written[idx] = stamp;
                    }
                    let k = mask.map_or(1.0, |m| m[idx]);
                    shade.apply(g.sample(tex, x, y, u, vv), k, &mut t.px[idx]);
                });
            }
        }
        while let Some((p, buf)) = stack.pop() {
            let dst = stack.last_mut().map_or(&mut base, |(_, b)| b);
            self.composite_part(frame, p, &buf, dst, &mut mask_cache, &to_px, ss, opts.msaa);
        }
        let t = base;

        Ok(finish_image(
            &t.px,
            w,
            v.width as usize,
            v.height as usize,
            ss,
        ))
    }

    /// Composites an offscreen part's buffer into `dst`: the part's
    /// multiply / screen colours, opacity (own × visibility × ancestors up to
    /// the next offscreen ancestor), clip mask and blend modes.
    /// spec: offscreen.md §1 step 3, appearance.md §2
    #[allow(clippy::too_many_arguments)]
    fn composite_part(
        &self,
        frame: &Frame,
        p: usize,
        buf: &Target,
        dst: &mut Target,
        mask_cache: &mut HashMap<(Vec<MeshIndex>, bool), Vec<f32>>,
        to_px: &dyn Fn(&[f32]) -> V2,
        ss: usize,
        msaa: bool,
    ) {
        let m = self.model;
        let (Some(part), Some(st)) = (m.parts.get(p), frame.parts.get(p)) else {
            return;
        };
        let mut alpha = st.form.opacity.clamp(0.0, 1.0) * if st.visible { 1.0 } else { 0.0 };
        let mut q = part.info.parent_part;
        let mut steps = 0;
        while let Some(a) = q {
            steps += 1;
            if steps > m.parts.len() || m.parts[a].offscreen {
                break;
            }
            alpha *= frame.parts.get(a).map_or(1.0, |s| s.form.opacity);
            q = m.parts[a].info.parent_part;
        }
        if alpha <= 0.0 {
            return;
        }
        let mask = if part.clip.is_empty() {
            None
        } else {
            let key = (part.clip.clone(), part.invert_clip);
            if !mask_cache.contains_key(&key) {
                let mk = self.build_mask(
                    frame,
                    &part.clip,
                    part.invert_clip,
                    buf.w,
                    buf.h,
                    to_px,
                    ss,
                    msaa,
                );
                mask_cache.insert(key.clone(), mk);
            }
            mask_cache.get(&key).cloned()
        };
        let c = |c: keyform_model::Color| [c[0], c[1], c[2]];
        let shade = Shade {
            alpha,
            mul: c(st.form.multiply),
            scr: c(st.form.screen),
            cc: part.color_composition,
            ac: part.alpha_composition,
            simple: blend::is_simple(part.color_composition, part.alpha_composition),
            illegal: false,
        };
        for (i, px) in buf.px.iter().enumerate() {
            if px[3] <= 0.0 {
                continue;
            }
            let k = mask.as_ref().map_or(1.0, |m| m[i]);
            shade.apply(*px, k, &mut dst.px[i]);
        }
    }

    /// Mask image from clip sources' texture alpha, accumulated with "over";
    /// opacity and colors of the sources do not count (clipping.md §1). Hidden
    /// sources still mask in the editor (§3).
    #[allow(clippy::too_many_arguments)]
    fn build_mask(
        &self,
        frame: &Frame,
        clip: &[MeshIndex],
        invert: bool,
        w: usize,
        h: usize,
        to_px: &dyn Fn(&[f32]) -> V2,
        ss: usize,
        msaa: bool,
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
            let Some(tex) = m
                .texture_slot(am)
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
                let tri_uv = [uv(ix[0]), uv(ix[1]), uv(ix[2])];
                let g = Sampler::new([p0, p1, p2], tri_uv, ss, msaa);
                raster_triangle(&mut t, [p0, p1, p2], tri_uv, |x, y, u, v, t| {
                    let a = g.sample(tex, x, y, u, v)[3];
                    let p = &mut t.px[y * t.w + x][3];
                    *p = a + *p * (1.0 - a);
                });
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

/// Composites a rendered (straight-alpha) group image with `opacity`
/// (clamped to 0–1) over an opaque `background`, or leaves it transparent
/// with its alpha scaled. spec: 60-animation/animation-model.md §5.1
pub fn composite_group(img: &mut RgbaImage, opacity: f32, background: Option<[f32; 3]>) {
    let op = opacity.clamp(0.0, 1.0);
    for p in img.data.chunks_exact_mut(4) {
        let a = f32::from(p[3]) / 255.0 * op;
        match background {
            Some(bg) => {
                for i in 0..3 {
                    let c = f32::from(p[i]) / 255.0 * a + bg[i] * (1.0 - a);
                    p[i] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                p[3] = 255;
            }
            None => p[3] = (a * 255.0).round() as u8,
        }
    }
}

/// The offscreen parts above a drawable, outermost first.
pub(crate) fn offscreen_chain(m: &Model, mut p: Option<usize>) -> Vec<usize> {
    let mut out = Vec::new();
    let mut steps = 0;
    while let Some(q) = p {
        steps += 1;
        if steps > m.parts.len() {
            break;
        }
        if m.parts.get(q).is_some_and(|x| x.offscreen) {
            out.push(q);
        }
        p = m.parts.get(q).and_then(|x| x.info.parent_part);
    }
    out.reverse();
    out
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
    /// The "illegal" look (blending.md §5).
    illegal: bool,
}

impl Shade {
    /// spec: appearance.md §4, clipping.md §2, blending.md §2–3
    fn apply(&self, texel: [f32; 4], k: f32, dst: &mut [f32; 4]) {
        if self.illegal {
            // texture × (1, 0, 0, 0.5), premultiplied source-over.
            let src = [texel[0], 0.0, 0.0, texel[3] * 0.5];
            for i in 0..4 {
                dst[i] = src[i] + dst[i] * (1.0 - src[3]);
            }
            return;
        }
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

/// Texture sampling for one triangle: its constant UV gradients and the
/// sample position rule (per subsample, or per output pixel for MSAA).
struct Sampler {
    /// d(u, v)/dx and d(u, v)/dy per buffer pixel.
    ddx: V2,
    ddy: V2,
    ss: usize,
    msaa: bool,
}

impl Sampler {
    fn new(p: [V2; 3], uv: [V2; 3], ss: usize, msaa: bool) -> Self {
        let area =
            (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
        let inv = if area.abs() > 1e-12 { 1.0 / area } else { 0.0 };
        // Barycentric weight derivatives (see `raster_triangle`'s edges).
        let dw = |a: V2, b: V2| [-(b[1] - a[1]) * inv, (b[0] - a[0]) * inv];
        let (w0, w1, w2) = (dw(p[1], p[2]), dw(p[2], p[0]), dw(p[0], p[1]));
        let d = |k: usize, c: usize| w0[k] * uv[0][c] + w1[k] * uv[1][c] + w2[k] * uv[2][c];
        Self {
            ddx: [d(0, 0), d(0, 1)],
            ddy: [d(1, 0), d(1, 1)],
            ss,
            msaa,
        }
    }

    fn sample(&self, tex: &Texture, x: usize, y: usize, u: f32, v: f32) -> [f32; 4] {
        if !self.msaa || self.ss <= 1 {
            return tex.sample_grad(u, v, self.ddx, self.ddy);
        }
        // Move to the output pixel's centre and use its footprint.
        let ss = self.ss as f32;
        let cx = ((x / self.ss) as f32 + 0.5) * ss - (x as f32 + 0.5);
        let cy = ((y / self.ss) as f32 + 0.5) * ss - (y as f32 + 0.5);
        let (u, v) = (
            u + self.ddx[0] * cx + self.ddy[0] * cy,
            v + self.ddx[1] * cx + self.ddy[1] * cy,
        );
        let k = ss;
        tex.sample_grad(
            u,
            v,
            [self.ddx[0] * k, self.ddx[1] * k],
            [self.ddy[0] * k, self.ddy[1] * k],
        )
    }
}

/// Draws an art-path stroke: vertex colours (straight RGB, alpha) blended
/// across each triangle, multiply / screen and opacity applied, normal
/// source-over. spec: art-path.md §3, appearance.md §4
pub(crate) fn draw_stroke(
    t: &mut Target,
    st: &keyform_eval::artpath::ArtPathState,
    to_px: &dyn Fn(&[f32]) -> V2,
) {
    let s = &st.stroke;
    let pts: Vec<V2> = s.positions.chunks_exact(2).map(to_px).collect();
    let bary = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    for tri in s.indices.chunks_exact(3) {
        let i = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        let (Some(&p0), Some(&p1), Some(&p2)) = (pts.get(i[0]), pts.get(i[1]), pts.get(i[2]))
        else {
            continue;
        };
        let c = [s.colors[i[0]], s.colors[i[1]], s.colors[i[2]]];
        raster_triangle(t, [p0, p1, p2], bary, |x, y, u, v, t| {
            let w0 = 1.0 - u - v;
            let k: [f32; 4] = std::array::from_fn(|j| w0 * c[0][j] + u * c[1][j] + v * c[2][j]);
            let a = (k[3] * st.opacity).clamp(0.0, 1.0);
            let mut src = [0.0f32; 4];
            for j in 0..3 {
                let rgb = k[j].clamp(0.0, 1.0) * st.multiply[j];
                src[j] = (rgb + st.screen[j] - rgb * st.screen[j]) * a;
            }
            src[3] = a;
            let d = &mut t.px[y * t.w + x];
            for j in 0..4 {
                d[j] = src[j] + d[j] * (1.0 - a);
            }
        });
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
    fn view_mapping_inverts() {
        let v = View {
            width: 300,
            height: 200,
            origin: [10.0, -5.0],
            scale: 0.75,
            rotation: 33.0,
            mirror: true,
        };
        for p in [[0.0, 0.0], [123.0, 45.0], [-50.0, 400.0]] {
            let q = v.to_canvas(v.to_px(p));
            assert!(
                (q[0] - p[0]).abs() < 1e-3 && (q[1] - p[1]).abs() < 1e-3,
                "{p:?} → {q:?}"
            );
        }
    }

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
