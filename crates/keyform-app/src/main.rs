//! Keyform editor application.
//!
//! Usage: `keyform [FILE…] [--screenshot OUT.png [--frames N]] [--size WxH] [ID=VALUE …]`

mod app;
mod canvas;
mod document;
mod fonts;
mod icons;
mod log;
mod palettes;
mod theme;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut files = Vec::new();
    let mut screenshot = None;
    let mut frames = 10;
    let mut size = [1200.0f32, 900.0];
    let mut values = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--frames" => frames = args.next().and_then(|n| n.parse().ok()).unwrap_or(frames),
            "--size" => {
                if let Some((w, h)) = args.next().as_deref().and_then(|s| s.split_once('x')) {
                    size = [w.parse().unwrap_or(size[0]), h.parse().unwrap_or(size[1])];
                }
            }
            _ => match a.split_once('=') {
                Some((id, v))
                    if !id.contains(std::path::MAIN_SEPARATOR) && v.parse::<f32>().is_ok() =>
                {
                    values.push((id.to_owned(), v.parse().unwrap_or(0.0)));
                }
                _ => files.push(PathBuf::from(a)),
            },
        }
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_min_inner_size([640.0, 480.0])
            .with_title("Keyform"),
        renderer: eframe::Renderer::Wgpu,
        // spec: 50-ui/main-window.md — one top-level window, default 1200 × 900.
        persist_window: screenshot.is_none(),
        ..Default::default()
    };
    let shot = screenshot.map(|path| app::ScreenshotRequest {
        path,
        after_frames: frames,
    });
    eframe::run_native(
        "Keyform",
        options,
        Box::new(move |cc| {
            let mut a = app::KeyformApp::new(cc, files, shot);
            a.initial_values = values;
            Ok(Box::new(a))
        }),
    )
}
