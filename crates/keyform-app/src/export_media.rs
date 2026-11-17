//! File ▸ Export Image/Video in the Animation workspace: the scene's frames
//! as a still image sequence (PNG / JPEG), an animated GIF or a video
//! (through the platform encoder: `ffmpeg`). spec: 50-ui/dialogs.md §14,
//! 60-animation/animation-model.md §5

use std::io::Write;
use std::path::{Path, PathBuf};

use eframe::egui;
use keyform_render::{CpuRenderer, RenderOptions, RgbaImage, View};

use crate::document::Document;
use crate::log::Log;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Png,
    Jpeg,
    Gif,
    Video,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameRange {
    All,
    WorkArea,
    Current,
}

#[derive(Clone, Debug)]
pub struct MediaState {
    pub open: bool,
    pub kind: MediaKind,
    pub range: FrameRange,
    /// Scale in percent of the scene canvas.
    pub percent: f32,
    /// Output frame rate (GIF, video); 0 = the scene's.
    pub fps: f64,
    pub transparent: bool,
    pub looping: bool,
    pub jpeg_quality: u8,
}

impl Default for MediaState {
    fn default() -> Self {
        Self {
            open: false,
            kind: MediaKind::Gif,
            range: FrameRange::WorkArea,
            percent: 25.0,
            fps: 15.0,
            transparent: false,
            looping: true,
            jpeg_quality: 90,
        }
    }
}

/// Scene frames to render, sampled at `fps` (or every frame at the
/// scene's rate).
pub fn frames(doc: &Document, range: FrameRange, fps: f64) -> Vec<f64> {
    let Some(st) = doc.anim.as_ref() else {
        return Vec::new();
    };
    let Some(s) = st.scene() else {
        return Vec::new();
    };
    let (a, b) = match range {
        FrameRange::All => (f64::from(s.start_frame), f64::from(s.duration)),
        FrameRange::WorkArea => (f64::from(s.work_start), f64::from(s.work_end)),
        FrameRange::Current => return vec![st.frame.round()],
    };
    let step = if fps > 0.0 { s.fps / fps } else { 1.0 };
    let mut out = Vec::new();
    let mut f = a;
    while f <= b + 1e-9 {
        out.push(f);
        f += step.max(1e-3);
    }
    out
}

/// One scene frame rendered over the scene canvas (the model placed by
/// its track's visual effect), `percent` of the canvas size.
pub fn render_frame(
    doc: &Document,
    frame: f64,
    percent: f32,
    white: bool,
) -> Result<RgbaImage, String> {
    let (sw, sh) = doc.stage_size();
    let k = percent / 100.0;
    let view = View {
        width: ((sw * k).floor() as u32).max(1),
        height: ((sh * k).floor() as u32).max(1),
        origin: [0.0, 0.0],
        scale: k,
        rotation: 0.0,
        mirror: false,
    };
    let f = doc.anim_frame_at(frame).ok_or("No animation is open.")?;
    // The track is drawn offscreen (transparent), then composited with its
    // group opacity over the background. spec: animation-model.md §5.1
    let mut img = CpuRenderer::new(&doc.model)
        .render(
            &f,
            &RenderOptions {
                view,
                background: None,
                supersample: 2,
                msaa: true,
            },
        )
        .map_err(|e| e.to_string())?;
    keyform_render::composite_group(
        &mut img,
        doc.anim_group_opacity(frame),
        white.then_some([1.0, 1.0, 1.0]),
    );
    Ok(img)
}

fn jpeg(img: &RgbaImage, path: &Path, quality: u8) -> Result<(), String> {
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
}

/// Exports to `out` (a folder for sequences, a file otherwise). Returns
/// the files written.
pub fn export(doc: &Document, st: &MediaState, out: &Path) -> Result<Vec<PathBuf>, String> {
    let scene = doc
        .anim
        .as_ref()
        .and_then(|a| a.scene())
        .ok_or("No animation is open.")?;
    let name = scene.name.clone();
    let scene_fps = scene.fps;
    let fps = match st.kind {
        MediaKind::Gif | MediaKind::Video if st.fps > 0.0 => st.fps,
        _ => scene_fps,
    };
    let list = frames(
        doc,
        st.range,
        if matches!(st.kind, MediaKind::Gif | MediaKind::Video) {
            fps
        } else {
            0.0
        },
    );
    if list.is_empty() {
        return Err("There are no frames to export.".into());
    }
    match st.kind {
        MediaKind::Png | MediaKind::Jpeg => {
            std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
            let mut files = Vec::new();
            for f in &list {
                let png = st.kind == MediaKind::Png;
                let img = render_frame(doc, *f, st.percent, !png || !st.transparent)?;
                let p = out.join(format!(
                    "{name}_{:04}.{}",
                    *f as i64,
                    if png { "png" } else { "jpg" }
                ));
                if png {
                    let bytes = img.encode_png().map_err(|e| e.to_string())?;
                    std::fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))?;
                } else {
                    jpeg(&img, &p, st.jpeg_quality)?;
                }
                files.push(p);
            }
            Ok(files)
        }
        MediaKind::Gif => {
            let first = render_frame(doc, list[0], st.percent, !st.transparent)?;
            let (w, h) = (first.width, first.height);
            if w < 16 || h < 16 {
                return Err("GIF images must be at least 16 × 16.".into());
            }
            let (gw, gh) = (
                u16::try_from(w).map_err(|_| "too large for GIF")?,
                u16::try_from(h).map_err(|_| "too large for GIF")?,
            );
            let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
            let mut enc = gif::Encoder::new(std::io::BufWriter::new(file), gw, gh, &[])
                .map_err(|e| e.to_string())?;
            if st.looping {
                enc.set_repeat(gif::Repeat::Infinite)
                    .map_err(|e| e.to_string())?;
            }
            let delay = (100.0 / fps).round().max(1.0) as u16;
            for (i, f) in list.iter().enumerate() {
                let mut img = if i == 0 {
                    first.clone()
                } else {
                    render_frame(doc, *f, st.percent, !st.transparent)?
                };
                // Straight alpha for the encoder; transparent pixels stay 0.
                for p in img.data.chunks_exact_mut(4) {
                    if p[3] > 0 && p[3] < 255 {
                        let a = u32::from(p[3]);
                        for c in &mut p[..3] {
                            *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
                        }
                    }
                    if st.transparent {
                        p[3] = if p[3] >= 128 { 255 } else { 0 };
                    }
                }
                let mut frame = gif::Frame::from_rgba_speed(gw, gh, &mut img.data, 10);
                frame.delay = delay;
                frame.dispose = gif::DisposalMethod::Background;
                enc.write_frame(&frame).map_err(|e| e.to_string())?;
            }
            Ok(vec![out.to_path_buf()])
        }
        MediaKind::Video => {
            let first = render_frame(doc, list[0], st.percent, true)?;
            // Even sizes for the usual encoders.
            let (w, h) = (first.width & !1, first.height & !1);
            let mut child = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    "rgba",
                    "-s",
                ])
                .arg(format!("{w}x{h}"))
                .arg("-r")
                .arg(format!("{fps}"))
                .args(["-i", "-", "-pix_fmt", "yuv420p"])
                .arg(out)
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|_| "No video encoder was found (install ffmpeg).".to_string())?;
            {
                let stdin = child.stdin.as_mut().ok_or("ffmpeg has no input")?;
                for (i, f) in list.iter().enumerate() {
                    let img = if i == 0 {
                        first.clone()
                    } else {
                        render_frame(doc, *f, st.percent, true)?
                    };
                    for y in 0..h {
                        let row = (y * img.width * 4) as usize;
                        stdin
                            .write_all(&img.data[row..row + (w * 4) as usize])
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            let status = child.wait().map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("ffmpeg failed ({status})"));
            }
            Ok(vec![out.to_path_buf()])
        }
    }
}

/// The output settings dialog; on OK asks for the destination and exports.
pub fn dialog(ctx: &egui::Context, st: &mut MediaState, doc: Option<&Document>, log: &mut Log) {
    if !st.open {
        return;
    }
    let Some(doc) = doc.filter(|d| d.anim.is_some()) else {
        st.open = false;
        return;
    };
    let mut open = true;
    let (mut go, mut cancel) = (false, false);
    let scene_fps = doc
        .anim
        .as_ref()
        .and_then(|a| a.scene())
        .map_or(30.0, |s| s.fps);
    egui::Window::new("Image/Video Output Settings")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Grid::new("media").num_columns(2).show(ui, |ui| {
                ui.label("Output");
                egui::ComboBox::from_id_salt("media-kind")
                    .selected_text(match st.kind {
                        MediaKind::Png => "Still images (PNG)",
                        MediaKind::Jpeg => "Still images (JPEG)",
                        MediaKind::Gif => "GIF",
                        MediaKind::Video => "Video (MP4)",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.kind, MediaKind::Png, "Still images (PNG)");
                        ui.selectable_value(&mut st.kind, MediaKind::Jpeg, "Still images (JPEG)");
                        ui.selectable_value(&mut st.kind, MediaKind::Gif, "GIF");
                        ui.selectable_value(&mut st.kind, MediaKind::Video, "Video (MP4)");
                    });
                ui.end_row();
                ui.label("Range");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut st.range, FrameRange::All, "Output All");
                    ui.radio_value(&mut st.range, FrameRange::WorkArea, "Output Work Area");
                    ui.radio_value(&mut st.range, FrameRange::Current, "Selected frame");
                });
                ui.end_row();
                ui.label("Size");
                ui.add(
                    egui::DragValue::new(&mut st.percent)
                        .range(1.0..=400.0)
                        .suffix(" %"),
                );
                ui.end_row();
                if matches!(st.kind, MediaKind::Gif | MediaKind::Video) {
                    ui.label("Frame rate");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut st.fps)
                                .range(1.0..=120.0)
                                .suffix(" fps"),
                        );
                        if st.kind == MediaKind::Gif {
                            ui.weak("(16 fps or less recommended)");
                        } else if ui.small_button("scene").clicked() {
                            st.fps = scene_fps;
                        }
                    });
                    ui.end_row();
                }
                if matches!(st.kind, MediaKind::Png | MediaKind::Gif) {
                    ui.label("");
                    ui.checkbox(&mut st.transparent, "Make Background Transparent");
                    ui.end_row();
                }
                if st.kind == MediaKind::Gif {
                    ui.label("");
                    ui.checkbox(&mut st.looping, "Loop");
                    ui.end_row();
                }
                if st.kind == MediaKind::Jpeg {
                    ui.label("JPEG quality");
                    ui.add(egui::Slider::new(&mut st.jpeg_quality, 0..=100));
                    ui.end_row();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                go = ui.button("OK").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
    if cancel || !open {
        st.open = false;
        return;
    }
    if !go {
        return;
    }
    st.open = false;
    let name = doc
        .anim
        .as_ref()
        .and_then(|a| a.scene())
        .map_or_else(String::new, |s| s.name.clone());
    let dest = match st.kind {
        MediaKind::Png | MediaKind::Jpeg => rfd::FileDialog::new().pick_folder(),
        MediaKind::Gif => rfd::FileDialog::new()
            .set_file_name(format!("{name}.gif"))
            .add_filter("GIF", &["gif"])
            .save_file(),
        MediaKind::Video => rfd::FileDialog::new()
            .set_file_name(format!("{name}.mp4"))
            .add_filter("Video", &["mp4"])
            .save_file(),
    };
    let Some(dest) = dest else { return };
    match export(doc, st, &dest) {
        Ok(files) => log.info(format!(
            "Exported {} file(s) to {}",
            files.len(),
            dest.display()
        )),
        Err(e) => log.error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GIF and a PNG sequence of hiyori_m01's first frames.
    #[test]
    fn export_scene_media() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let can3 = home.join("Downloads/hiyori_en/hiyori_pro/hiyori_pro_t04.can3");
        let model = root.join("hiyori_pro_t11.cmo3");
        if !can3.exists() || !model.exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kf-media-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::os::unix::fs::symlink(&can3, dir.join("a.can3"));
        let _ = std::os::unix::fs::symlink(&model, dir.join("hiyori_pro_t11.cmo3"));
        let mut log = Log::default();
        let doc = Document::open_animation(&dir.join("a.can3"), &mut log).unwrap();
        // 10 fps over the work area 0..141 (30 fps): every 3rd frame.
        let f = frames(&doc, FrameRange::WorkArea, 10.0);
        assert_eq!(f.len(), 48);
        assert_eq!(f[1], 3.0);
        let mut st = MediaState {
            percent: 5.0,
            fps: 10.0,
            ..Default::default()
        };
        st.range = FrameRange::Current;
        st.kind = MediaKind::Png;
        let files = export(&doc, &st, &dir.join("png")).unwrap();
        assert_eq!(files.len(), 1);
        st.kind = MediaKind::Gif;
        st.range = FrameRange::WorkArea;
        st.fps = 1.0;
        let gif_path = dir.join("a.gif");
        export(&doc, &st, &gif_path).unwrap();
        let data = std::fs::read(&gif_path).unwrap();
        assert!(data.starts_with(b"GIF89a"));
        // A one-frame video when the encoder is there.
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_ok()
        {
            st.kind = MediaKind::Video;
            st.range = FrameRange::Current;
            st.percent = 10.0;
            let v = dir.join("a.mp4");
            export(&doc, &st, &v).unwrap();
            assert!(std::fs::metadata(&v).unwrap().len() > 0);
        }
        // Two frames differ (the scene moves).
        let a = render_frame(&doc, 0.0, 5.0, true).unwrap();
        let b = render_frame(&doc, 60.0, 5.0, true).unwrap();
        assert_ne!(a.data, b.data);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
