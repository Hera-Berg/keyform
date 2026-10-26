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
    /// Rectangle selection in progress: start (screen) and whether it
    /// selects points (of the selected objects) rather than objects.
    rect: Option<(Pos2, bool)>,
    /// The texture page shown in mesh edit mode (texture index).
    mesh_texture: Option<(usize, egui::TextureHandle)>,
}

/// Pick radius for points, in screen points.
const POINT_PICK: f32 = 6.0;

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
    let [x0, y0, x1, y1] = doc.view.fit_box.take().unwrap_or([
        0.0,
        0.0,
        doc.model.canvas.width as f32,
        doc.model.canvas.height as f32,
    ]);
    let (cw, ch) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    doc.view.zoom = (rect.width() / cw).min(rect.height() / ch) * 0.92;
    doc.view.center = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
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
    } else if doc.mesh_edit.is_some() {
        mesh_tool(ui, &response, doc, gpu, rect, ppp);
    } else {
        arrow_tool(ui, &response, doc, gpu, rect, ppp);
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

    if doc.mesh_edit.is_some() {
        draw_mesh_edit(ui.ctx(), &painter, doc, gpu, rect, &to_screen, tokens);
    } else {
        draw_selection(&painter, doc, &to_screen, tokens);
    }
    if let (Some((a, _)), Some(b)) = (gpu.rect, ui.input(|i| i.pointer.interact_pos())) {
        let r = Rect::from_two_pos(a, b);
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.0, tokens.accent),
            egui::StrokeKind::Inside,
        );
        painter.rect_filled(r, 0.0, tokens.accent.gamma_multiply(0.08));
    }

    status.zoom = doc.view.zoom;
    status.rotation = doc.view.rotation;
    status.cursor = response
        .hover_pos()
        .map(|p| view.to_canvas([(p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp]));
    response
}

/// Arrow tool (canvas-and-tools.md §3):
/// - click selects the top-most object, Shift toggles it; clicking an
///   object that is already selected keeps the selection;
/// - pressing on a point of a selected object selects the point (Shift
///   toggles) and a drag moves the selected points;
/// - dragging a selected object moves every selected object;
/// - dragging on empty space draws a rectangle: it selects points of the
///   selected objects, or objects when nothing is selected (Shift adds);
/// - a click on empty space clears the selection.
///
/// Space/R drags navigate instead.
fn arrow_tool(
    ui: &egui::Ui,
    response: &egui::Response,
    doc: &mut Document,
    gpu: &mut ViewGpu,
    rect: Rect,
    ppp: f32,
) {
    use crate::document::SelectionSet;
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
    let press = ui.input(|i| i.pointer.press_origin());
    if response.drag_started_by(primary) || response.clicked_by(primary) {
        let Some(press) = press else { return };
        let q = to_canvas(press);
        let radius = POINT_PICK / doc.view.zoom.max(1e-6);
        let mut set = doc.selection.clone();
        let mut drag_points = false;
        // Rotation deformer widget of the primary selection: handle tip
        // rotates, circle edge scales (the origin is a point).
        let widget = doc
            .primary()
            .and_then(|o| Some((o, doc.rotation_widget(o)?)));
        if let (Some((o, (centre, tip, r))), true) = (widget, response.drag_started_by(primary)) {
            let dist = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
            let handle = if dist(q, tip) <= radius {
                Some(crate::document::Handle::Rotate)
            } else if dist(q, centre) > radius && (dist(q, centre) - r).abs() <= radius {
                Some(crate::document::Handle::Scale)
            } else {
                None
            };
            if let Some(h) = handle {
                if let Err(e) = doc.drag_begin_handle(o, h, q) {
                    doc.notice = Some(e);
                }
                return;
            }
        }
        if let Some(pt) = doc.point_hit(q, radius) {
            // A point of a selected object.
            if shift {
                if let Some(i) = set.points.iter().position(|p| *p == pt) {
                    set.points.remove(i);
                } else {
                    set.points.push(pt);
                }
            } else if !set.points.contains(&pt) {
                set.points = vec![pt];
            }
            drag_points = true;
        } else if let Some(hit) = doc.hit_test(q) {
            if shift {
                if let Some(i) = set.objects.iter().position(|o| *o == hit) {
                    set.objects.remove(i);
                    set.points.retain(|p| p.0 != hit);
                } else {
                    set.objects.push(hit);
                }
            } else if !set.objects.contains(&hit) {
                set = SelectionSet::of(Some(hit));
            }
        } else if response.drag_started_by(primary) {
            gpu.rect = Some((press, !set.objects.is_empty()));
        } else if !shift {
            set = SelectionSet::default();
        }
        doc.set_selection(set);
        if response.drag_started_by(primary) && gpu.rect.is_none() && !shift {
            let points = drag_points && !doc.selection.points.is_empty();
            if points || !doc.selection.objects.is_empty() {
                if let Err(e) = doc.drag_begin(points) {
                    doc.notice = Some(e);
                }
            }
        }
    }
    if let Some((start, points)) = gpu.rect {
        if !response.dragged_by(primary) {
            gpu.rect = None;
            if let Some(end) = ui.input(|i| i.pointer.interact_pos()) {
                let r = Rect::from_two_pos(start, end);
                let quad = [
                    r.left_top(),
                    r.right_top(),
                    r.right_bottom(),
                    r.left_bottom(),
                ]
                .map(to_canvas);
                let mut set = if shift {
                    doc.selection.clone()
                } else if points {
                    SelectionSet {
                        objects: doc.selection.objects.clone(),
                        points: Vec::new(),
                    }
                } else {
                    SelectionSet::default()
                };
                if points {
                    for p in doc.points_in(&quad) {
                        if !set.points.contains(&p) {
                            set.points.push(p);
                        }
                    }
                } else {
                    for o in doc.objects_in(&quad) {
                        if !set.objects.contains(&o) {
                            set.objects.push(o);
                        }
                    }
                }
                doc.set_selection(set);
            }
        }
        return;
    }
    if doc.is_dragging() {
        if response.dragged_by(primary) {
            if let (Some(a), Some(b)) = (press, ui.input(|i| i.pointer.interact_pos())) {
                let (ca, cb) = (to_canvas(a), to_canvas(b));
                doc.drag_update([cb[0] - ca[0], cb[1] - ca[1]], shift);
            }
        } else {
            doc.drag_end();
        }
    }
}

/// Mesh edit mode input (canvas-and-tools.md §5). Select: click a vertex
/// to select it (Shift toggles), drag selected vertices to move them, drag
/// on empty space for a rectangle (Shift adds). Add Point: press to add a
/// vertex (on an edge it splits the edge) and drag to place it. Eraser:
/// press or drag over vertices and edges; Alt erases edges only.
fn mesh_tool(
    ui: &egui::Ui,
    response: &egui::Response,
    doc: &mut Document,
    gpu: &mut ViewGpu,
    rect: Rect,
    ppp: f32,
) {
    use crate::mesh_edit::MeshTool;
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (space, rkey, shift, alt) = ui.input(|i| {
        (
            !typing && i.key_down(egui::Key::Space),
            !typing && i.key_down(egui::Key::R),
            i.modifiers.shift,
            i.modifiers.alt,
        )
    });
    if space || rkey {
        return;
    }
    let view = view_of(doc, rect, ppp);
    let to_canvas = |p: Pos2| view.to_canvas([(p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp]);
    let radius = POINT_PICK / doc.view.zoom.max(1e-6);
    let Some(s) = doc.mesh_edit.as_mut() else {
        return;
    };
    let primary = egui::PointerButton::Primary;
    let press = ui.input(|i| i.pointer.press_origin());
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let started = response.drag_started_by(primary) || response.clicked_by(primary);
    match s.tool {
        MeshTool::Select => {
            if started {
                let Some(press) = press else { return };
                let q = to_canvas(press);
                let mut sel = s.selected.clone();
                if let Some(k) = s.point_at(q, radius) {
                    if shift {
                        match sel.iter().position(|&p| p == k) {
                            Some(i) => {
                                sel.remove(i);
                            }
                            None => sel.push(k),
                        }
                    } else if !sel.contains(&k) {
                        sel = vec![k];
                    }
                    s.set_selected(sel);
                    if response.drag_started_by(primary) && !shift {
                        s.drag_begin(radius);
                    }
                } else if response.drag_started_by(primary) {
                    gpu.rect = Some((press, true));
                } else if !shift {
                    s.set_selected(Vec::new());
                }
            }
            if let Some((start, _)) = gpu.rect {
                if !response.dragged_by(primary) {
                    gpu.rect = None;
                    if let Some(end) = pointer {
                        let r = Rect::from_two_pos(start, end);
                        let quad = [
                            r.left_top(),
                            r.right_top(),
                            r.right_bottom(),
                            r.left_bottom(),
                        ]
                        .map(to_canvas);
                        let mut sel = if shift {
                            s.selected.clone()
                        } else {
                            Vec::new()
                        };
                        for k in s.points_in(&quad) {
                            if !sel.contains(&k) {
                                sel.push(k);
                            }
                        }
                        s.set_selected(sel);
                    }
                }
                return;
            }
        }
        MeshTool::AddPoint => {
            if started {
                let Some(press) = press else { return };
                let q = to_canvas(press);
                match s.point_at(q, radius) {
                    // Pressing an existing vertex picks it up instead.
                    Some(k) => s.set_selected(vec![k]),
                    None => {
                        s.add_point(q, radius);
                    }
                }
                if response.drag_started_by(primary) {
                    s.drag_begin(radius);
                }
            }
        }
        MeshTool::Brush => {
            if response.drag_started_by(primary) {
                if let Some(press) = press {
                    s.brush_begin(to_canvas(press), radius);
                }
            }
        }
        MeshTool::Erase => {
            if response.dragged_by(primary) || response.clicked_by(primary) {
                if let Some(p) = pointer {
                    s.erase(to_canvas(p), radius, alt);
                }
            }
            return;
        }
    }
    if s.is_dragging() {
        if response.dragged_by(primary) {
            if let (Some(a), Some(b)) = (press, pointer) {
                let (ca, cb) = (to_canvas(a), to_canvas(b));
                s.drag_update([cb[0] - ca[0], cb[1] - ca[1]]);
            }
        } else {
            s.drag_end();
        }
    }
}

/// Mesh edit mode view: the model dimmed, the edited mesh's texture
/// undeformed on its working triangulation, the graph edges and vertices.
/// spec: canvas-and-tools.md §5 ("deformation does not affect the texture"),
/// main-window.md §6 ("dims other meshes")
fn draw_mesh_edit(
    ctx: &egui::Context,
    painter: &egui::Painter,
    doc: &Document,
    gpu: &mut ViewGpu,
    rect: Rect,
    to_screen: &dyn Fn([f32; 2]) -> Pos2,
    tokens: &Tokens,
) {
    let Some(s) = doc.mesh_edit.as_ref() else {
        return;
    };
    painter.rect_filled(rect, 0.0, tokens.canvas_viewport.gamma_multiply(0.75));
    let g = &s.graph;
    if let Some(t) = s.texture {
        if gpu.mesh_texture.as_ref().is_none_or(|(i, _)| *i != t) {
            gpu.mesh_texture = doc.model.textures.get(t).and_then(|tex| {
                let img = keyform_formats::image::RgbaImage::decode_png(tex.png.as_ref()?).ok()?;
                let c = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.data,
                );
                Some((
                    t,
                    ctx.load_texture("mesh-edit", c, egui::TextureOptions::LINEAR),
                ))
            });
        }
    }
    if let Some((_, tex)) = gpu
        .mesh_texture
        .as_ref()
        .filter(|(i, _)| Some(*i) == s.texture)
    {
        let mut mesh = egui::Mesh::with_texture(tex.id());
        for k in 0..g.len() {
            let uv = Pos2::new(s.uvs[2 * k], s.uvs[2 * k + 1]);
            mesh.vertices.push(egui::epaint::Vertex {
                pos: to_screen(g.point(k)),
                uv,
                color: Color32::WHITE,
            });
        }
        mesh.indices.extend_from_slice(&s.preview);
        painter.add(egui::Shape::mesh(mesh));
    }
    // Triangulation (thin), then the graph's own edges on top.
    let thin = egui::Stroke::new(1.0, tokens.accent.gamma_multiply(0.35));
    for t in s.preview.chunks_exact(3) {
        let p = [t[0], t[1], t[2]].map(|v| to_screen(g.point(v as usize)));
        painter.line_segment([p[0], p[1]], thin);
        painter.line_segment([p[1], p[2]], thin);
        painter.line_segment([p[2], p[0]], thin);
    }
    // Brush outline at the cursor.
    if s.tool == crate::mesh_edit::MeshTool::Brush {
        if let Some(c) = ctx
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
        {
            let o = to_screen([0.0, 0.0]);
            let r = (to_screen([s.brush_radius, 0.0]) - o).length();
            painter.circle_stroke(c, r, egui::Stroke::new(1.0, tokens.accent));
            painter.circle_stroke(
                c,
                r * s.brush_hardness,
                egui::Stroke::new(1.0, tokens.accent.gamma_multiply(0.4)),
            );
        }
    }
    // Mirror Edit axis.
    if let Some(m) = s.mirror {
        let stroke = egui::Stroke::new(1.0, tokens.warning);
        let far = 1e6;
        let (a, b) = if m.vertical {
            ([m.axis, -far], [m.axis, far])
        } else {
            ([-far, m.axis], [far, m.axis])
        };
        painter.line_segment([to_screen(a), to_screen(b)], stroke);
    }
    let edge = egui::Stroke::new(1.0, tokens.accent);
    for (e, &pr) in g.mesh.edges.iter().zip(&g.mesh.edge_priority) {
        if pr != keyform_model::EDGE_AUTO_TRIANGULATION {
            let [a, b] = e.map(|v| to_screen(g.point(v as usize)));
            painter.line_segment([a, b], edge);
        }
    }
    for k in 0..g.len() {
        let c = to_screen(g.point(k));
        if s.selected.contains(&k) {
            painter.circle_filled(c, 3.5, tokens.accent);
            painter.circle_stroke(c, 3.5, egui::Stroke::new(1.0, Color32::WHITE));
        } else {
            painter.rect_filled(
                Rect::from_center_size(c, egui::vec2(4.0, 4.0)),
                0.0,
                Color32::WHITE,
            );
            painter.rect_stroke(
                Rect::from_center_size(c, egui::vec2(4.0, 4.0)),
                0.0,
                egui::Stroke::new(1.0, tokens.accent),
                egui::StrokeKind::Outside,
            );
        }
    }
}

/// Outlines of the selected objects, their points, and the selected points.
fn draw_selection(
    painter: &egui::Painter,
    doc: &Document,
    to_screen: &dyn Fn([f32; 2]) -> Pos2,
    tokens: &Tokens,
) {
    use crate::document::Selection;
    let Some(frame) = doc.frame.as_ref() else {
        return;
    };
    let stroke = egui::Stroke::new(1.0, tokens.accent.gamma_multiply(0.85));
    for &sel in &doc.selection.objects {
        match sel {
            Selection::ArtMesh(a) => {
                let (Some(st), Some(mesh)) = (frame.meshes.get(a), doc.model.art_meshes.get(a))
                else {
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
                Some(keyform_eval::DeformerMap::Warp(_)) if doc.uses_net(sel) => {
                    // Bezier net (levels 2–3): patch edges, anchors and handles.
                    if let Some(net) = doc.canvas_net(d) {
                        let thin = egui::Stroke::new(1.0, tokens.accent.gamma_multiply(0.5));
                        for cj in 0..net.rows {
                            for ci in 0..net.cols {
                                let edge = |f: &dyn Fn(f32) -> (f32, f32)| {
                                    let pts: Vec<Pos2> = (0..=16)
                                        .map(|k| {
                                            let (s, t) = f(k as f32 / 16.0);
                                            to_screen(net.eval(ci, cj, s, t))
                                        })
                                        .collect();
                                    painter.add(egui::Shape::line(pts, stroke));
                                };
                                edge(&|u| (u, 0.0));
                                edge(&|u| (0.0, u));
                                if ci + 1 == net.cols {
                                    edge(&|u| (1.0, u));
                                }
                                if cj + 1 == net.rows {
                                    edge(&|u| (u, 1.0));
                                }
                            }
                        }
                        for (k, dir) in net.points() {
                            let p = to_screen(net.point(k, dir));
                            match dir {
                                None => {
                                    painter.rect_filled(
                                        Rect::from_center_size(p, egui::vec2(6.0, 6.0)),
                                        0.0,
                                        tokens.accent,
                                    );
                                }
                                Some(_) => {
                                    painter.line_segment([to_screen(net.point(k, None)), p], thin);
                                    painter.circle_filled(p, 2.5, tokens.accent);
                                }
                            }
                        }
                    }
                }
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
                Some(keyform_eval::DeformerMap::Rotation(_)) => {
                    if let Some((origin, tip, radius)) = doc.rotation_widget(sel) {
                        let (o, t) = (to_screen(origin), to_screen(tip));
                        let edge = to_screen([origin[0] + radius, origin[1]]);
                        painter.circle_stroke(o, (edge - o).length(), stroke);
                        painter.line_segment([o, t], stroke);
                        painter.circle_filled(t, 4.0, tokens.accent);
                        painter.circle_filled(o, 3.0, tokens.accent);
                    }
                }
                None => {}
            },
            Selection::Part(_) | Selection::Glue(_) => {}
        }
    }
    // Selected points on top.
    for &(o, i) in &doc.selection.points {
        if let Some(p) = doc.object_points(o).get(i) {
            let c = to_screen(*p);
            painter.circle_filled(c, 3.5, tokens.accent);
            painter.circle_stroke(c, 3.5, egui::Stroke::new(1.0, Color32::WHITE));
        }
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
