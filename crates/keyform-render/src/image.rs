//! Premultiplied textures for sampling, with mipmaps.
//!
//! spec: 40-rendering/appearance.md §5 — the premultiplied texture is padded
//! (transparent, right and bottom) to a multiple of `mipmapLevel`, a mip
//! chain is built with a 2×2 box filter on premultiplied texels, and
//! minification is trilinear with anisotropic filtering. Magnification is
//! bilinear. Outside the texture is transparent (clamp-to-border).

pub use keyform_formats::image::RgbaImage;
use keyform_model::TextureSampling;

/// Most samples taken along the major axis (the usual device maximum, 16×).
pub const MAX_ANISOTROPY: f32 = 16.0;

/// One mip level: premultiplied RGBA, 0…1.
pub struct Level {
    pub width: usize,
    pub height: usize,
    pub texels: Vec<[f32; 4]>,
}

impl Level {
    fn texel(&self, x: i64, y: i64) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            [0.0; 4]
        } else {
            self.texels[y as usize * self.width + x as usize]
        }
    }

    /// Bilinear sample at normalized (u, v) of this level, v = 0 at the top.
    fn bilinear(&self, u: f32, v: f32) -> [f32; 4] {
        let x = u * self.width as f32 - 0.5;
        let y = v * self.height as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (
            self.texel(x0, y0),
            self.texel(x0 + 1, y0),
            self.texel(x0, y0 + 1),
            self.texel(x0 + 1, y0 + 1),
        );
        let mut out = [0.0; 4];
        for i in 0..4 {
            let top = a[i] + (b[i] - a[i]) * fx;
            let bot = c[i] + (d[i] - c[i]) * fx;
            out[i] = top + (bot - top) * fy;
        }
        out
    }

    /// The next level: half size (at least 1), 2×2 box filter.
    fn half(&self) -> Level {
        let (w, h) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let mut texels = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = (2 * x as i64, 2 * y as i64);
                let clampx = |v: i64| v.min(self.width as i64 - 1);
                let clampy = |v: i64| v.min(self.height as i64 - 1);
                let mut acc = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let t = self.texel(clampx(sx + dx), clampy(sy + dy));
                    for i in 0..4 {
                        acc[i] += t[i];
                    }
                }
                texels.push(acc.map(|c| c * 0.25));
            }
        }
        Level {
            width: w,
            height: h,
            texels,
        }
    }
}

/// A sampled texture: its mip chain (level 0 = padded original).
pub struct Texture {
    pub levels: Vec<Level>,
    /// Original size / padded size: scales UVs onto the original area.
    pub uv_scale: [f32; 2],
    pub sampling: TextureSampling,
}

/// Premultiplied level 0, padded to a multiple of `pad_to`.
pub fn premultiplied_level(img: &RgbaImage, pad_to: u32) -> (Level, [f32; 2]) {
    let (w, h) = (img.width as usize, img.height as usize);
    let pad = |n: usize| {
        if pad_to > 1 {
            n.div_ceil(pad_to as usize) * pad_to as usize
        } else {
            n
        }
    };
    let (pw, ph) = (pad(w).max(1), pad(h).max(1));
    let mut texels = vec![[0.0f32; 4]; pw * ph];
    for (i, p) in img.data.chunks_exact(4).enumerate() {
        let (x, y) = (i % w.max(1), i / w.max(1));
        let a = f32::from(p[3]) / 255.0;
        texels[y * pw + x] = [
            f32::from(p[0]) / 255.0 * a,
            f32::from(p[1]) / 255.0 * a,
            f32::from(p[2]) / 255.0 * a,
            a,
        ];
    }
    let scale = [w as f32 / pw as f32, h as f32 / ph as f32];
    (
        Level {
            width: pw,
            height: ph,
            texels,
        },
        scale,
    )
}

impl Texture {
    /// Premultiplies at upload (spec: 10-formats/cmo3.md §3), pads and
    /// builds the mip chain.
    pub fn new(img: &RgbaImage, sampling: TextureSampling) -> Self {
        let (base, uv_scale) = premultiplied_level(img, sampling.pad_to);
        let mut levels = vec![base];
        if sampling.mipmaps {
            while levels.last().is_some_and(|l| l.width > 1 || l.height > 1) {
                let next = levels.last().map(Level::half);
                levels.extend(next);
            }
        }
        Self {
            levels,
            uv_scale,
            sampling,
        }
    }

    /// Bilinear texture without mipmaps.
    pub fn from_image(img: &RgbaImage) -> Self {
        Self::new(
            img,
            TextureSampling {
                mipmaps: false,
                anisotropy: false,
                pad_to: 0,
            },
        )
    }

    /// Bilinear sample of level 0 at normalized (u, v), v = 0 at the top
    /// row; transparent outside (clamp-to-border).
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        self.levels[0].bilinear(u * self.uv_scale[0], v * self.uv_scale[1])
    }

    fn trilinear(&self, u: f32, v: f32, lod: f32) -> [f32; 4] {
        let top = (self.levels.len() - 1) as f32;
        let lod = lod.clamp(0.0, top);
        let l0 = lod.floor() as usize;
        let f = lod - l0 as f32;
        let a = self.levels[l0].bilinear(u, v);
        if f <= 0.0 || l0 + 1 >= self.levels.len() {
            return a;
        }
        let b = self.levels[l0 + 1].bilinear(u, v);
        let mut out = [0.0; 4];
        for i in 0..4 {
            out[i] = a[i] + (b[i] - a[i]) * f;
        }
        out
    }

    /// Sample with the screen-space UV gradients of one output pixel
    /// (`ddx` = (du/dx, dv/dx), `ddy` = (du/dy, dv/dy), normalized UV
    /// units): trilinear with anisotropic taps along the major axis.
    pub fn sample_grad(&self, u: f32, v: f32, ddx: [f32; 2], ddy: [f32; 2]) -> [f32; 4] {
        let (u, v) = (u * self.uv_scale[0], v * self.uv_scale[1]);
        let s = self.uv_scale;
        let (ddx, ddy) = (
            [ddx[0] * s[0], ddx[1] * s[1]],
            [ddy[0] * s[0], ddy[1] * s[1]],
        );
        if !self.sampling.mipmaps || self.levels.len() == 1 {
            return self.levels[0].bilinear(u, v);
        }
        let (w, h) = (self.levels[0].width as f32, self.levels[0].height as f32);
        let px = (ddx[0] * w).hypot(ddx[1] * h);
        let py = (ddy[0] * w).hypot(ddy[1] * h);
        let (pmax, pmin, major) = if px >= py {
            (px, py, ddx)
        } else {
            (py, px, ddy)
        };
        let n = if self.sampling.anisotropy && pmin > 0.0 {
            // Tolerance: an isotropic footprint must not round up to 2.
            (pmax / pmin - 1e-3).ceil().clamp(1.0, MAX_ANISOTROPY)
        } else if self.sampling.anisotropy && pmax > 0.0 {
            MAX_ANISOTROPY
        } else {
            1.0
        };
        let rho = pmax / n;
        if rho <= 1.0 && n <= 1.0 {
            // Magnification: bilinear on level 0.
            return self.levels[0].bilinear(u, v);
        }
        let lod = rho.max(1e-12).log2();
        let taps = n as usize;
        if taps <= 1 {
            return self.trilinear(u, v, lod);
        }
        let mut acc = [0.0f32; 4];
        for i in 0..taps {
            let t = (i as f32 + 0.5) / n - 0.5;
            let c = self.trilinear(u + major[0] * t, v + major[1] * t, lod);
            for k in 0..4 {
                acc[k] += c[k];
            }
        }
        acc.map(|c| c / n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(n: u32) -> RgbaImage {
        let mut data = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let on = (x + y) % 2 == 0;
                data.extend_from_slice(&[if on { 255 } else { 0 }, 0, 0, 255]);
            }
        }
        RgbaImage {
            width: n,
            height: n,
            data,
        }
    }

    #[test]
    fn mips_average_premultiplied_and_minification_blurs() {
        let t = Texture::new(&checker(8), TextureSampling::default());
        assert_eq!(t.levels.len(), 4);
        // Level 1 of a 1-px checker is flat 50 % red.
        assert!(t.levels[1].texels.iter().all(|p| (p[0] - 0.5).abs() < 1e-6));
        // Magnified: the original texel.
        let mag = t.sample_grad(0.5 / 8.0, 0.5 / 8.0, [0.01, 0.0], [0.0, 0.01]);
        assert!((mag[0] - 1.0).abs() < 1e-6);
        // Minified 2× isotropically: level 1, flat.
        let min = t.sample_grad(0.5, 0.5, [2.0 / 8.0, 0.0], [0.0, 2.0 / 8.0]);
        assert!((min[0] - 0.5).abs() < 1e-6, "{min:?}");
    }

    #[test]
    fn padding_keeps_uvs_on_the_original_area() {
        let t = Texture::new(
            &checker(6),
            TextureSampling {
                mipmaps: true,
                anisotropy: false,
                pad_to: 4,
            },
        );
        assert_eq!((t.levels[0].width, t.levels[0].height), (8, 8));
        // Texel (0, 0) of the original is red.
        let c = t.sample(0.5 / 6.0, 0.5 / 6.0);
        assert!((c[0] - 1.0).abs() < 1e-6, "{c:?}");
    }
}
