//! The document view: the model rendered by the GPU renderer, with canvas
//! navigation. spec: 50-ui/canvas-and-tools.md §1–2

use eframe::egui::{self, Color32, Pos2, Rect, Sense};
use eframe::egui_wgpu;
use keyform_render::gpu::GpuRenderer;
use keyform_render::{RenderOptions, View};

use crate::document::Document;
use crate::theme::Tokens;

/// GPU resources of one document's view.
#[derive(Default)]
pub struct ViewGpu {
    pub renderer: Option<GpuRenderer>,
    target: Option<(wgpu::Texture, egui::TextureId, [u32; 2])>,
    /// Pan axis lock while Shift is held: None = undecided.
    pan_axis: Option<Option<bool>>,
}

/// Live information for the status bar.
#[derive(Default, Clone, Copy)]
pub struct CanvasStatus {
    pub cursor: Option<[f32; 2]>,
    pub zoom: f32,
    pub rotation: f32,
}

/// Supersampling of the view texture (egui's linear filter box-filters it down).
const SUPERSAMPLE: u32 = 2;

pub fn view_of(doc: &Document, rect: Rect, ppp: f32) -> View {
    let w = (rect.width() * ppp).round().max(1.0) as u32;
    let h = (rect.height() * ppp).round().max(1.0) as u32;
    let scale = doc.view.zoom * ppp;
    View {
        width: w,
        height: h,
        origin: [
            doc.view.center[0] - w as f32 / 2.0 / scale,
            doc.view.center[1] - h as f32 / 2.0 / scale,
        ],
        scale,
        rotation: doc.view.rotation,
        mirror: doc.view.mirror,
    }
}

pub fn fit(doc: &mut Document, rect: Rect) {
    let (cw, ch) = (
        doc.model.canvas.width.max(1) as f32,
        doc.model.canvas.height.max(1) as f32,
    );
    doc.view.zoom = (rect.width() / cw).min(rect.height() / ch) * 0.92;
    doc.view.center = [cw / 2.0, ch / 2.0];
    doc.view.rotation = 0.0;
    doc.view.fit_pending = false;
}

/// Cursor tracking input: while a button is held over the view, its target
/// follows the pointer (left = primary, right = secondary); release → (0, 0).
/// Space/R drags navigate instead. spec: 70-advanced/viewer.md §3.6
fn track_cursor(ui: &egui::Ui, response: &egui::Response, doc: &mut Document, rect: Rect) {
    let (space, rkey, primary, secondary, pos) = ui.input(|i| {
        (
            i.key_down(egui::Key::Space),
            i.key_down(egui::Key::R),
            i.pointer.primary_down(),
            i.pointer.secondary_down(),
            i.pointer.interact_pos(),
        )
    });
    let held = response.is_pointer_button_down_on() && !space && !rkey;
    let target = pos.map(|p| {
        let u = (p.x - rect.min.x) / rect.width();
        let v = (p.y - rect.min.y) / rect.height();
        keyform_anim::tracking::target_from_view(u, v)
    });
    let t = &mut doc.tracking;
    match target {
        Some(xy) if held && primary => t.primary.set_target(xy),
        _ => t.primary.set_target([0.0, 0.0]),
    }
    match target {
        Some(xy) if held && secondary => t.secondary.set_target(xy),
        _ => t.secondary.set_target([0.0, 0.0]),
    }
}

pub fn canvas_ui(
    ui: &mut egui::Ui,
    doc: &mut Document,
    gpu: &mut ViewGpu,
    rs: Option<&egui_wgpu::RenderState>,
    tokens: &Tokens,
    status: &mut CanvasStatus,
) -> egui::Response {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    if rect.width() < 2.0 || rect.height() < 2.0 {
        return response;
    }
    let ppp = ui.ctx().pixels_per_point();
    if doc.view.fit_pending {
        fit(doc, rect);
    }
    navigate(ui, &response, doc, gpu, rect, ppp);
    if doc.mode == crate::document::Mode::Viewer {
        track_cursor(ui, &response, doc, rect);
    } else {
        arrow_tool(ui, &response, doc, rect, ppp);
    }

    let view = view_of(doc, rect, ppp);
    let to_screen = |p: [f32; 2]| -> Pos2 {
        let q = view.to_px(p);
        rect.min + egui::vec2(q[0], q[1]) / ppp
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, tokens.canvas_viewport);
    // Canvas area (white), drawn as a quad so it follows rotation.
    let (cw, ch) = (
        doc.model.canvas.width as f32,
        doc.model.canvas.height as f32,
    );
    let corners = [[0.0, 0.0], [cw, 0.0], [cw, ch], [0.0, ch]].map(to_screen);
    painter.add(egui::Shape::convex_polygon(
        corners.to_vec(),
        Color32::WHITE,
        egui::Stroke::new(1.0, tokens.border),
    ));

    if let (Some(rs), Some(frame)) = (rs, doc.frame.as_ref()) {
        if gpu.renderer.is_none() {
            let r = GpuRenderer::with_device(
                rs.device.clone(),
                rs.queue.clone(),
                &doc.model,
                "UI device".into(),
            );
            gpu.renderer = Some(r);
        }
        let size = [view.width * SUPERSAMPLE, view.height * SUPERSAMPLE];
        let max = rs.device.limits().max_texture_dimension_2d;
        if size[0] <= max && size[1] <= max {
            if gpu.target.as_ref().is_none_or(|t| t.2 != size) {
                let tex = rs.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("view"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: GpuRenderer::TARGET_FORMAT,
                    usage: GpuRenderer::TARGET_USAGE,
                    view_formats: &[],
                });
                let tv = tex.create_view(&Default::default());
                let id = match gpu.target.take() {
                    Some((_, id, _)) => {
                        rs.renderer.write().update_egui_texture_from_wgpu_texture(
                            &rs.device,
                            &tv,
                            wgpu::FilterMode::Linear,
                            id,
                        );
                        id
                    }
                    None => rs.renderer.write().register_native_texture(
                        &rs.device,
                        &tv,
                        wgpu::FilterMode::Linear,
                    ),
                };
                gpu.target = Some((tex, id, size));
            }
            if let (Some(r), Some((tex, id, _))) = (&gpu.renderer, &gpu.target) {
                let opts = RenderOptions {
                    msaa: false,
                    view,
                    background: None,
                    supersample: SUPERSAMPLE,
                };
                match r.render_to(&doc.model, frame, &opts, tex) {
                    Ok(()) => {
                        painter.image(
                            *id,
                            rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    Err(e) => {
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            e.to_string(),
                            egui::FontId::proportional(12.0),
                            tokens.error,
                        );
                    }
                }
            }
        }
    }

    draw_selection(&painter, doc, &to_screen, tokens);

    status.zoom = doc.view.zoom;
    status.rotation = doc.view.rotation;
    status.cursor = response
        .hover_pos()
        .map(|p| view.to_canvas([(p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp]));
    response
}

/// Arrow tool: click selects the top-most object (Shift toggles it), a
/// drag moves the selected object in its current keyform; Space/R drags
/// navigate instead. spec: canvas-and-tools.md §3
fn arrow_tool(ui: &egui::Ui, response: &egui::Response, doc: &mut Document, rect: Rect, ppp: f32) {
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (space, rkey, shift) = ui.input(|i| {
        (
            !typing && i.key_down(egui::Key::Space),
            !typing && i.key_down(egui::Key::R),
            i.modifiers.shift,
        )
    });
    if space || rkey {
        return;
    }
    let view = view_of(doc, rect, ppp);
    let to_canvas = |p: Pos2| view.to_canvas([(p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp]);
    let primary = egui::PointerButton::Primary;
    if response.drag_started_by(primary) || response.clicked_by(primary) {
        let Some(press) = ui.input(|i| i.pointer.press_origin()) else {
            return;
        };
        let hit = doc.hit_test(to_canvas(press));
        if shift {
            if hit.is_some() {
                let sel = if doc.selection == hit { None } else { hit };
                doc.select(sel);
            }
        } else if hit != doc.selection {
            doc.select(hit);
        }
        if response.drag_started_by(primary) && !shift {
            if let Some(o) = hit {
                if let Err(e) = doc.drag_begin(o) {
                    doc.notice = Some(e);
                }
            }
        }
    }
    if doc.is_dragging() {
        if response.dragged_by(primary) {
            if let (Some(a), Some(b)) = (
                ui.input(|i| i.pointer.press_origin()),
                ui.input(|i| i.pointer.interact_pos()),
            ) {
                let (ca, cb) = (to_canvas(a), to_canvas(b));
                doc.drag_update([cb[0] - ca[0], cb[1] - ca[1]]);
            }
        } else {
            doc.drag_end();
        }
    }
}

/// Outline of the selected object over the model.
fn draw_selection(
    painter: &egui::Painter,
    doc: &Document,
    to_screen: &dyn Fn([f32; 2]) -> Pos2,
    tokens: &Tokens,
) {
    use crate::document::Selection;
    let (Some(sel), Some(frame)) = (doc.selection, doc.frame.as_ref()) else {
        return;
    };
    let stroke = egui::Stroke::new(1.0, tokens.accent.gamma_multiply(0.85));
    match sel {
        Selection::ArtMesh(a) => {
            let (Some(st), Some(mesh)) = (frame.meshes.get(a), doc.model.art_meshes.get(a)) else {
                return;
            };
            let pt = |i: u32| {
                let i = i as usize;
                to_screen([
                    st.positions.get(2 * i).copied().unwrap_or(0.0),
                    st.positions.get(2 * i + 1).copied().unwrap_or(0.0),
                ])
            };
            for t in mesh.indices.chunks_exact(3) {
                let (p, q, r) = (pt(t[0]), pt(t[1]), pt(t[2]));
                painter.line_segment([p, q], stroke);
                painter.line_segment([q, r], stroke);
                painter.line_segment([r, p], stroke);
            }
        }
        Selection::Deformer(d) => match frame.deformers.get(d).and_then(|s| s.map.as_ref()) {
            Some(keyform_eval::DeformerMap::Warp(w)) => {
                let p = |i: usize, j: usize| {
                    let k = 2 * (j * (w.cols + 1) + i);
                    to_screen([w.points[k], w.points[k + 1]])
                };
                for j in 0..=w.rows {
                    for i in 0..=w.cols {
                        if i < w.cols {
                            painter.line_segment([p(i, j), p(i + 1, j)], stroke);
                        }
                        if j < w.rows {
                            painter.line_segment([p(i, j), p(i, j + 1)], stroke);
                        }
                        painter.circle_filled(p(i, j), 2.0, tokens.accent);
                    }
                }
            }
            Some(keyform_eval::DeformerMap::Rotation(r)) => {
                let o = to_screen(r.origin);
                let (s, c) = r.angle.to_radians().sin_cos();
                // Handle pointing along the deformed local "up" (−y).
                let len = 40.0;
                let tip = o + egui::vec2(s * len, -c * len);
                painter.circle_stroke(o, 6.0, stroke);
                painter.line_segment([o, tip], stroke);
                painter.circle_filled(tip, 3.0, tokens.accent);
            }
            None => {}
        },
        Selection::Part(_) | Selection::Glue(_) => {}
    }
}

/// spec: canvas-and-tools.md §2
fn navigate(
    ui: &egui::Ui,
    response: &egui::Response,
    doc: &mut Document,
    gpu: &mut ViewGpu,
    rect: Rect,
    ppp: f32,
) {
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (space, rkey, shift, scroll, pointer) = ui.input(|i| {
        (
            !typing && i.key_down(egui::Key::Space),
            !typing && i.key_down(egui::Key::R),
            i.modifiers.shift,
            i.smooth_scroll_delta.y + (i.zoom_delta() - 1.0) * 200.0,
            i.pointer.hover_pos(),
        )
    });
    // Wheel: zoom at the cursor, keeping the canvas point under it fixed.
    if response.hovered() && scroll != 0.0 {
        if let Some(p) = pointer {
            let px = [(p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp];
            let before = view_of(doc, rect, ppp).to_canvas(px);
            doc.view.zoom = (doc.view.zoom * (scroll * 0.0025).exp()).clamp(0.01, 64.0);
            let after = view_of(doc, rect, ppp).to_canvas(px);
            doc.view.center[0] += before[0] - after[0];
            doc.view.center[1] += before[1] - after[1];
        }
    }
    let drag = response.drag_delta();
    let middle = response.dragged_by(egui::PointerButton::Middle);
    let primary = response.dragged_by(egui::PointerButton::Primary);
    if (middle || (primary && space)) && drag != egui::Vec2::ZERO {
        let mut d = drag;
        // Shift locks the pan to the axis of the first movement.
        if shift {
            let axis = gpu.pan_axis.get_or_insert(None);
            if axis.is_none() && d.x.abs() != d.y.abs() {
                *axis = Some(d.x.abs() > d.y.abs());
            }
            match axis {
                Some(true) => d.y = 0.0,
                Some(false) => d.x = 0.0,
                None => d = egui::Vec2::ZERO,
            }
        } else {
            gpu.pan_axis = None;
        }
        let v = view_of(doc, rect, ppp);
        let c = rect.center() - rect.min;
        let a = v.to_canvas([c.x * ppp, c.y * ppp]);
        let b = v.to_canvas([(c.x - d.x) * ppp, (c.y - d.y) * ppp]);
        doc.view.center[0] += b[0] - a[0];
        doc.view.center[1] += b[1] - a[1];
    } else if primary && rkey {
        // Rotate the view about its centre by the angle the cursor sweeps.
        if let Some(p) = pointer {
            let c = rect.center();
            let a0 = (p - drag - c).angle();
            let a1 = (p - c).angle();
            let mut r = doc.view.rotation + (a1 - a0).to_degrees();
            if shift {
                r = (r / 15.0).round() * 15.0;
            }
            doc.view.rotation = r.rem_euclid(360.0);
        }
    }
    if !response.dragged() {
        gpu.pan_axis = None;
    }
    if response.double_clicked() && (space || rkey) {
        if rkey {
            doc.view.rotation = 0.0;
        } else {
            fit(doc, rect);
        }
    }
}
