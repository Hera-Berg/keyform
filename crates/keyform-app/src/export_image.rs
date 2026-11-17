//! File ▸ Export Image/Video ▸ Image: the model at the current parameter
//! values as a PNG (transparent) or JPEG (white). spec: 50-ui/dialogs.md §20

use std::path::Path;

use eframe::egui;
use keyform_eval::Evaluator;
use keyform_render::{CpuRenderer, RenderOptions, RgbaImage, View};

use crate::document::{Document, Mode};
use crate::log::Log;

/// Largest image accepted: 200 MiB before compression (4 bytes per pixel).
pub const MAX_BYTES: u64 = 200 * 1024 * 1024;
/// Images wider or taller than this get a note before saving.
const LARGE_SIDE: u32 = 2048;
const PRESETS: [f32; 6] = [25.0, 50.0, 75.0, 100.0, 150.0, 200.0];

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ExportImageState {
    #[serde(skip)]
    pub open: bool,
    /// Scale in percent (remembered).
    pub percent: f32,
    /// JPEG quality 0–100.
    pub jpeg_quality: u8,
}

impl Default for ExportImageState {
    fn default() -> Self {
        Self {
            open: false,
            percent: 100.0,
            jpeg_quality: 90,
        }
    }
}

/// Output size at `percent` of the canvas: ⌊W·m⌋ × ⌊H·m⌋.
pub fn output_size(doc: &Document, percent: f32) -> (u32, u32) {
    let m = percent / 100.0;
    let (w, h) = (
        (doc.model.canvas.width as f32 * m).floor(),
        (doc.model.canvas.height as f32 * m).floor(),
    );
    (w.max(0.0) as u32, h.max(0.0) as u32)
}

/// Whether OK may be pressed: non-empty and within the limit.
pub fn size_ok(w: u32, h: u32) -> bool {
    w >= 1 && h >= 1 && 4 * u64::from(w) * u64::from(h) <= MAX_BYTES
}

/// Renders the export image: the export camera, 4 samples per pixel used
/// like 4× MSAA, transparent or white background.
pub fn render(doc: &Document, percent: f32, opaque_white: bool) -> Result<RgbaImage, String> {
    let view = View::for_export(&doc.model, percent / 100.0).ok_or("The image size is 0.")?;
    // The current parameter values, on a fresh evaluation of the model.
    let values = match doc.mode {
        Mode::Modeling => &doc.values,
        Mode::Viewer | Mode::Animation => &doc.live,
    };
    let frame = Evaluator::new(&doc.model).evaluate(values);
    let r = CpuRenderer::new(&doc.model);
    r.render(
        &frame,
        &RenderOptions {
            view,
            background: opaque_white.then_some([1.0, 1.0, 1.0, 1.0]),
            supersample: 2,
            msaa: true,
        },
    )
    .map_err(|e| e.to_string())
}

fn write(img: &RgbaImage, path: &Path, quality: u8) -> Result<(), String> {
    let jpeg = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    if jpeg {
        let rgb: Vec<u8> = img
            .data
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        let (w, h) = (
            u16::try_from(img.width).map_err(|_| "JPEG images are limited to 65535 pixels")?,
            u16::try_from(img.height).map_err(|_| "JPEG images are limited to 65535 pixels")?,
        );
        jpeg_encoder::Encoder::new_file(path, quality.min(100))
            .and_then(|e| e.encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb))
            .map_err(|e| e.to_string())
    } else {
        let png = img.encode_png().map_err(|e| e.to_string())?;
        keyform_formats::cmo3::write_atomic(path, &png).map_err(|e| e.to_string())
    }
}

/// The settings dialog; on OK asks for a file and exports.
pub fn dialog(
    ctx: &egui::Context,
    st: &mut ExportImageState,
    doc: Option<&Document>,
    log: &mut Log,
) {
    if !st.open {
        return;
    }
    let Some(doc) = doc else {
        st.open = false;
        return;
    };
    let (w, h) = output_size(doc, st.percent);
    let bytes = 4 * u64::from(w) * u64::from(h);
    let mut open = true;
    let mut go = false;
    let mut cancel = false;
    egui::Window::new("Image Output Settings")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("Number of Pixels");
            ui.horizontal(|ui| {
                ui.label("Magnification");
                ui.add(
                    egui::DragValue::new(&mut st.percent)
                        .range(0.0..=10000.0)
                        .suffix(" %"),
                );
                egui::ComboBox::from_id_salt("export-scale")
                    .selected_text(format!("{} %", st.percent))
                    .show_ui(ui, |ui| {
                        for p in PRESETS {
                            ui.selectable_value(&mut st.percent, p, format!("{p} %"));
                        }
                    });
            });
            ui.label(format!("Size  {w} × {h}"));
            ui.weak(format!(
                "[ Original Size {} × {} ]",
                doc.model.canvas.width, doc.model.canvas.height
            ));
            ui.label(format!(
                "Size before compression  {:.2} MB",
                bytes as f64 / 1024.0 / 1024.0
            ));
            ui.weak(format!(
                "Note: Max output file size {} MB (Size before compression) can be saved.",
                MAX_BYTES / 1024 / 1024
            ));
            ui.horizontal(|ui| {
                ui.label("JPEG quality");
                ui.add(egui::Slider::new(&mut st.jpeg_quality, 0..=100));
            });
            if w > LARGE_SIDE || h > LARGE_SIDE {
                // TODO: the reference shows a dismissable hint here; its
                // wording is not specified.
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Images larger than 2048 pixels may not open in every application.",
                );
            }
            ui.separator();
            ui.horizontal(|ui| {
                go = ui
                    .add_enabled(size_ok(w, h), egui::Button::new("OK"))
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
    if !open || cancel {
        st.open = false;
    }
    if !go {
        return;
    }
    st.open = false;
    let stem = doc
        .path
        .file_stem()
        .map_or("model".into(), |s| s.to_string_lossy().into_owned());
    let mut fd = rfd::FileDialog::new()
        .add_filter("PNG Image (*.png)", &["png"])
        .add_filter("JPEG Image (*.jpg)", &["jpg", "jpeg"])
        .set_file_name(format!("{stem}.png"));
    if let Some(dir) = doc.path.parent() {
        fd = fd.set_directory(dir);
    }
    let Some(mut path) = fd.save_file() else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension("png");
    }
    let jpeg = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    match render(doc, st.percent, jpeg).and_then(|img| write(&img, &path, st.jpeg_quality)) {
        Ok(()) => log.info(format!("exported {w} × {h} image to {}", path.display())),
        Err(e) => log.error(format!("image export failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// Against a reference editor export kept locally (black box, not
    /// committed): test-data/blackbox/ref/hiyori_default_25.png.
    #[test]
    fn matches_reference_export_when_available() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (model, reference) = (
            root.join("hiyori_pro_t11.cmo3"),
            root.join("test-data/blackbox/ref/hiyori_default_25.png"),
        );
        let (Ok(_), Ok(want)) = (std::fs::metadata(&model), std::fs::read(&reference)) else {
            return;
        };
        let mut log = Log::default();
        let doc = Document::open(&model, &mut log).unwrap();
        let got = render(&doc, 25.0, false).unwrap();
        let want = RgbaImage::decode_png(&want).unwrap();
        assert_eq!((got.width, got.height), (want.width, want.height));
        let pre = |p: &[u8]| {
            let a = u32::from(p[3]);
            [p[0], p[1], p[2]]
                .map(|c| u32::from(c) * a / 255)
                .into_iter()
                .chain([a])
        };
        let total: u64 = got
            .data
            .chunks_exact(4)
            .zip(want.data.chunks_exact(4))
            .map(|(a, b)| {
                pre(a)
                    .zip(pre(b))
                    .map(|(x, y)| x.abs_diff(y))
                    .max()
                    .unwrap_or(0) as u64
            })
            .sum();
        let mean = total as f64 / f64::from(got.width * got.height);
        assert!(mean < 0.3, "mean difference {mean}");
    }

    #[test]
    fn size_limit_is_200_mib() {
        assert!(size_ok(7240, 7240));
        assert!(!size_ok(7241, 7241));
        assert!(!size_ok(0, 100));
    }
}
