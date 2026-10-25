//! Premultiplied textures for sampling.

pub use keyform_formats::image::RgbaImage;

/// Premultiplied RGBA texture in linear f32 for sampling.
pub struct Texture {
    pub width: usize,
    pub height: usize,
    /// Premultiplied, 0…1.
    pub texels: Vec<[f32; 4]>,
}

impl Texture {
    /// Premultiplies at upload (spec: 10-formats/cmo3.md §3).
    pub fn from_image(img: &RgbaImage) -> Self {
        let texels = img
            .data
            .chunks_exact(4)
            .map(|p| {
                let a = f32::from(p[3]) / 255.0;
                [
                    f32::from(p[0]) / 255.0 * a,
                    f32::from(p[1]) / 255.0 * a,
                    f32::from(p[2]) / 255.0 * a,
                    a,
                ]
            })
            .collect();
        Self {
            width: img.width as usize,
            height: img.height as usize,
            texels,
        }
    }

    /// Bilinear sample at normalized (u, v), v = 0 at the top row; transparent
    /// outside (clamp-to-border).
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let x = u * self.width as f32 - 0.5;
        let y = v * self.height as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let get = |xi: i64, yi: i64| -> [f32; 4] {
            if xi < 0 || yi < 0 || xi >= self.width as i64 || yi >= self.height as i64 {
                [0.0; 4]
            } else {
                self.texels[yi as usize * self.width + xi as usize]
            }
        };
        let (a, b, c, d) = (
            get(x0, y0),
            get(x0 + 1, y0),
            get(x0, y0 + 1),
            get(x0 + 1, y0 + 1),
        );
        let mut out = [0.0; 4];
        for i in 0..4 {
            let top = a[i] + (b[i] - a[i]) * fx;
            let bot = c[i] + (d[i] - c[i]) * fx;
            out[i] = top + (bot - top) * fy;
        }
        out
    }
}
