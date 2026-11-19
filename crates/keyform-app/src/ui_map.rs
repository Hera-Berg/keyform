//! UI map for scripted end-to-end tests: with `KEYFORM_UI_MAP=/path.json`
//! Keyform writes, every frame, the AccessKit tree egui builds (role, label,
//! value, state and bounds of each widget) plus where the canvas shows each
//! art mesh and its vertices. A test harness reads it to find where to
//! click, then drives the app with real mouse and keyboard input on the
//! display. Off unless the variable is set.

use std::sync::Mutex;

use eframe::egui;
use serde_json::{json, Value};

static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CANVAS: Mutex<Option<Value>> = Mutex::new(None);

pub fn enabled() -> bool {
    ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Installs the dump plugin when `KEYFORM_UI_MAP` is set.
pub fn install(ctx: &egui::Context) {
    let Some(path) = std::env::var_os("KEYFORM_UI_MAP") else {
        return;
    };
    ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
    ctx.enable_accesskit();
    ctx.add_plugin(UiMap {
        path: path.into(),
        frame: 0,
    });
}

/// Canvas facts of this frame (window points): the view rect, the canvas →
/// window mapping of three points, and each art mesh's vertices.
pub fn set_canvas(
    rect: egui::Rect,
    to_screen: &dyn Fn([f32; 2]) -> egui::Pos2,
    meshes: Vec<(String, Vec<[f32; 2]>)>,
    art_paths: Vec<(String, Vec<[f32; 2]>)>,
) {
    let pt = |p: egui::Pos2| json!([p.x, p.y]);
    let list = |v: Vec<(String, Vec<[f32; 2]>)>| -> Vec<Value> {
        v.into_iter()
            .map(|(id, pts)| {
                let s: Vec<egui::Pos2> = pts.iter().map(|p| to_screen(*p)).collect();
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for p in &s {
                    x0 = x0.min(p.x);
                    y0 = y0.min(p.y);
                    x1 = x1.max(p.x);
                    y1 = y1.max(p.y);
                }
                json!({ "id": id, "bbox": [x0, y0, x1, y1], "points": s.iter().map(|p| pt(*p)).collect::<Vec<_>>() })
            })
            .collect()
    };
    let v = json!({
        "rect": [rect.min.x, rect.min.y, rect.max.x, rect.max.y],
        "origin": pt(to_screen([0.0, 0.0])),
        "x_axis": pt(to_screen([1000.0, 0.0])),
        "y_axis": pt(to_screen([0.0, 1000.0])),
        "meshes": list(meshes),
        "art_paths": list(art_paths),
    });
    if let Ok(mut c) = CANVAS.lock() {
        *c = Some(v);
    }
}

struct UiMap {
    path: std::path::PathBuf,
    frame: u64,
}

impl egui::Plugin for UiMap {
    fn debug_name(&self) -> &'static str {
        "keyform-ui-map"
    }

    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        let Some(update) = output.platform_output.accesskit_update.as_ref() else {
            return;
        };
        self.frame += 1;
        // Clipped interact rects and layers of this pass's widgets (keyed by
        // the AccessKit node id): what is actually visible and on top.
        let widgets: std::collections::HashMap<
            egui::accesskit::NodeId,
            (egui::Rect, egui::LayerId, bool),
        > = ctx.viewport(|vp| {
            vp.prev_pass
                .widgets
                .layers()
                .flat_map(|(_, ws)| ws.iter())
                .map(|w| {
                    (
                        w.id.accesskit_id(),
                        (w.interact_rect, w.layer_id, w.enabled),
                    )
                })
                .collect()
        });
        let mut nodes = Vec::new();
        for (id, n) in &update.nodes {
            let Some(b) = n.bounds() else { continue };
            let label = n.label().unwrap_or_default().to_owned();
            let value = n.value().unwrap_or_default().to_owned();
            let interactive = matches!(
                n.role(),
                egui::accesskit::Role::TextInput
                    | egui::accesskit::Role::MultilineTextInput
                    | egui::accesskit::Role::SpinButton
                    | egui::accesskit::Role::Slider
                    | egui::accesskit::Role::CheckBox
                    | egui::accesskit::Role::ComboBox
                    | egui::accesskit::Role::Button
            );
            if label.is_empty() && value.is_empty() && !interactive {
                continue;
            }
            // Visible part (clipped to its scroll area / panel) and whether a
            // higher layer (window, popup) covers its centre.
            let (visible, covered) = match widgets.get(id) {
                Some((r, layer, _)) => {
                    let covered = r.is_positive()
                        && ctx.layer_id_at(r.center()).is_some_and(|top| top != *layer);
                    (
                        r.is_positive()
                            .then_some([r.min.x, r.min.y, r.max.x, r.max.y]),
                        covered,
                    )
                }
                // Nodes without a widget (custom-painted rows) clip their own
                // bounds before they are published.
                None => (
                    Some([b.x0 as f32, b.y0 as f32, b.x1 as f32, b.y1 as f32]),
                    false,
                ),
            };
            nodes.push(json!({
                "role": format!("{:?}", n.role()),
                "label": label,
                "value": value,
                "rect": [b.x0, b.y0, b.x1, b.y1],
                "visible": visible,
                "covered": covered,
                "disabled": n.is_disabled(),
                "toggled": n.toggled().map(|t| format!("{t:?}")),
                "selected": n.is_selected(),
            }));
        }
        let canvas = CANVAS.lock().ok().and_then(|c| c.clone());
        let v = json!({
            "frame": self.frame,
            "ppp": ctx.pixels_per_point(),
            "nodes": nodes,
            "canvas": canvas,
        });
        let tmp = self.path.with_extension("tmp");
        if std::fs::write(&tmp, v.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    /// The UI map is a test hook: off unless KEYFORM_UI_MAP is set.
    #[test]
    fn off_by_default() {
        if std::env::var_os("KEYFORM_UI_MAP").is_some() {
            return;
        }
        let ctx = eframe::egui::Context::default();
        super::install(&ctx);
        assert!(!super::enabled());
    }
}
