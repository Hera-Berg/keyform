//! Keyform editor application.
//!
//! Usage: `keyform [FILE…] [--screenshot OUT.png [--frames N]] [--size WxH] [--play MOTION]
//! [--select OBJECT_ID] [ID=VALUE …]`

mod app;
mod auto_mesh_dialog;
mod canvas;
mod create_deformer;
mod document;
mod export_image;
mod fonts;
mod glue_tool;
mod icons;
mod log;
mod mesh_edit;
mod palettes;
mod param_dialog;
mod psd_dialog;
mod texture_atlas;
mod theme;
mod viewer_settings;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut files = Vec::new();
    let mut screenshot = None;
    let mut frames = 10;
    let mut size = [1200.0f32, 900.0];
    let mut values = Vec::new();
    let mut play = None;
    let mut select = None;
    let mut mesh_edit = false;
    let mut auto_mesh = false;
    let mut atlas = false;
    let mut project = false;
    let mut glue_tool = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--play" => play = args.next().and_then(|n| n.parse::<usize>().ok()),
            "--select" => select = args.next(),
            "--mesh-edit" => mesh_edit = true,
            "--auto-mesh" => auto_mesh = true,
            "--atlas" => atlas = true,
            "--project" => project = true,
            "--glue-tool" => {
                use keyform_edit::glue_brush::GlueBrush;
                glue_tool = match args.next().as_deref() {
                    Some("A") => Some(GlueBrush::A),
                    Some("B") => Some(GlueBrush::B),
                    Some("AB") => Some(GlueBrush::AB),
                    Some("BA") => Some(GlueBrush::BA),
                    _ => Some(GlueBrush::Remerge),
                }
            }
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
    // Ask for transparent texture borders where the GPU has them
    // (40-rendering/appearance.md §5); the renderer falls back otherwise.
    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(c) = &mut wgpu_options.wgpu_setup {
        let base = c.device_descriptor.clone();
        c.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut d = base(adapter);
            d.required_features |=
                adapter.features() & wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER;
            d
        });
    }
    let options = eframe::NativeOptions {
        wgpu_options,
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
            a.initial_motion = play;
            a.initial_select = select;
            a.initial_mesh_edit = mesh_edit;
            a.initial_auto_mesh = auto_mesh;
            a.initial_atlas = atlas;
            a.initial_project = project;
            a.initial_glue_tool = glue_tool;
            Ok(Box::new(a))
        }),
    )
}
