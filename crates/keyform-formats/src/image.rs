//! Straight-alpha RGBA images and PNG I/O (untrusted input).

use std::io::Cursor;

use crate::error::FormatError;

/// Straight-alpha 8-bit RGBA image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl RgbaImage {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; width as usize * height as usize * 4],
        }
    }

    /// Largest accepted decoded image side (textures are ≤ 16384, spec
    /// 70-advanced/texture-atlas.md §1).
    pub const MAX_SIDE: u32 = 16384;

    /// Decodes a PNG of any colour type / bit depth to 8-bit RGBA.
    pub fn decode_png(bytes: &[u8]) -> Result<Self, FormatError> {
        let mut dec = png::Decoder::new(Cursor::new(bytes));
        dec.set_transformations(
            png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
        );
        let mut reader = dec
            .read_info()
            .map_err(|e| FormatError::Invalid(e.to_string()))?;
        let (w, h) = (reader.info().width, reader.info().height);
        if w == 0 || h == 0 || w > Self::MAX_SIDE || h > Self::MAX_SIDE {
            return Err(FormatError::Invalid(format!(
                "image size {w}×{h} unsupported"
            )));
        }
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| FormatError::Invalid(e.to_string()))?;
        buf.truncate(info.buffer_size());
        let data = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::GrayscaleAlpha => buf
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            png::ColorType::Rgb => buf
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::Indexed => {
                return Err(FormatError::Invalid("unexpanded palette".into()))
            }
        };
        if data.len() != w as usize * h as usize * 4 {
            return Err(FormatError::Invalid("decoded size mismatch".into()));
        }
        Ok(Self {
            width: w,
            height: h,
            data,
        })
    }

    pub fn encode_png(&self) -> Result<Vec<u8>, FormatError> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc
                .write_header()
                .map_err(|e| FormatError::Invalid(e.to_string()))?;
            w.write_image_data(&self.data)
                .map_err(|e| FormatError::Invalid(e.to_string()))?;
        }
        Ok(out)
    }
}
