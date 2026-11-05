//! Keyform's own monochrome line icons, drawn as vectors (≈1.5 px stroke at
//! 16 px, icon colour). spec: 50-ui/theme.md §4 — new icons, not copies.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Visible,
    Hidden,
    Part,
    ArtMesh,
    WarpDeformer,
    RotationDeformer,
    Glue,
    Folder,
    BlendShape,
    Play,
    Stop,
    /// Key diamond with a plus: add a key.
    KeyAdd,
    /// Key diamond with a minus: delete a key.
    KeyDelete,
    /// Closed padlock (locked); open shackle = unlocked.
    Locked,
    Unlocked,
}

pub const ICON_COLOR: Color32 = Color32::from_rgb(0xa8, 0xa8, 0xa8);

/// Paints `icon` into `rect` (square).
pub fn paint(p: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let u = s / 16.0;
    let at = |x: f32, y: f32| Pos2::new(c.x + (x - 8.0) * u, c.y + (y - 8.0) * u);
    let st = Stroke::new(1.4 * u.max(0.75), color);
    let line = |pts: &[(f32, f32)]| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| at(x, y)).collect();
        p.add(egui::Shape::line(v, st));
    };
    match icon {
        Icon::Visible | Icon::Hidden => {
            // Eye: almond outline with a pupil; hidden = closed lid.
            if icon == Icon::Visible {
                let top: Vec<Pos2> = (0..=12)
                    .map(|i| {
                        let t = i as f32 / 12.0;
                        at(2.0 + 12.0 * t, 8.0 - 4.5 * (std::f32::consts::PI * t).sin())
                    })
                    .collect();
                let bot: Vec<Pos2> = (0..=12)
                    .map(|i| {
                        let t = i as f32 / 12.0;
                        at(2.0 + 12.0 * t, 8.0 + 4.5 * (std::f32::consts::PI * t).sin())
                    })
                    .collect();
                p.add(egui::Shape::line(top, st));
                p.add(egui::Shape::line(bot, st));
                p.circle_filled(at(8.0, 8.0), 2.0 * u, color);
            } else {
                let lid: Vec<Pos2> = (0..=12)
                    .map(|i| {
                        let t = i as f32 / 12.0;
                        at(2.0 + 12.0 * t, 8.0 + 3.0 * (std::f32::consts::PI * t).sin())
                    })
                    .collect();
                p.add(egui::Shape::line(
                    lid,
                    Stroke::new(st.width, color.gamma_multiply(0.6)),
                ));
            }
        }
        Icon::Part | Icon::Folder => {
            line(&[
                (1.5, 4.0),
                (6.0, 4.0),
                (7.5, 5.5),
                (14.5, 5.5),
                (14.5, 13.0),
                (1.5, 13.0),
                (1.5, 4.0),
            ]);
        }
        Icon::ArtMesh => {
            line(&[(8.0, 2.0), (14.5, 13.5), (1.5, 13.5), (8.0, 2.0)]);
            line(&[(4.75, 7.75), (8.0, 13.5), (11.25, 7.75), (4.75, 7.75)]);
        }
        Icon::WarpDeformer => {
            // A slightly bent 3×3 grid.
            for i in 0..4 {
                let k = 2.0 + i as f32 * 4.0;
                line(&[(2.0, k), (8.0, k + 0.8), (14.0, k)]);
                line(&[(k, 2.0), (k + 0.8, 8.0), (k, 14.0)]);
            }
        }
        Icon::RotationDeformer => {
            p.circle_stroke(at(8.0, 8.0), 5.0 * u, st);
            line(&[(8.0, 8.0), (8.0, 1.0)]);
            p.circle_filled(at(8.0, 8.0), 1.4 * u, color);
        }
        Icon::Glue => {
            line(&[(2.0, 3.0), (2.0, 13.0)]);
            line(&[(14.0, 3.0), (14.0, 13.0)]);
            for y in [5.0, 8.0, 11.0] {
                line(&[(3.0, y), (13.0, y)]);
            }
        }
        Icon::BlendShape => {
            line(&[(8.0, 2.0), (14.0, 8.0), (8.0, 14.0), (2.0, 8.0), (8.0, 2.0)]);
        }
        Icon::Play => {
            p.add(egui::Shape::convex_polygon(
                vec![at(4.0, 2.5), at(13.5, 8.0), at(4.0, 13.5)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Stop => {
            p.rect_filled(Rect::from_min_max(at(3.5, 3.5), at(12.5, 12.5)), 1.0, color);
        }
        Icon::Locked | Icon::Unlocked => {
            // Body.
            line(&[
                (4.0, 8.0),
                (12.0, 8.0),
                (12.0, 14.0),
                (4.0, 14.0),
                (4.0, 8.0),
            ]);
            // Shackle: closed, or lifted and open on the right.
            if icon == Icon::Locked {
                line(&[(5.5, 8.0), (5.5, 5.0), (8.0, 2.5), (10.5, 5.0), (10.5, 8.0)]);
            } else {
                let dim = Stroke::new(st.width, color.gamma_multiply(0.6));
                let v: Vec<Pos2> = [(5.5, 8.0), (5.5, 4.0), (8.0, 1.5), (10.5, 4.0), (10.5, 5.0)]
                    .iter()
                    .map(|&(x, y)| at(x, y))
                    .collect();
                p.add(egui::Shape::line(v, dim));
            }
        }
        Icon::KeyAdd | Icon::KeyDelete => {
            // Key mark (diamond) at the left, sign at the right.
            line(&[(5.0, 3.5), (9.5, 8.0), (5.0, 12.5), (0.5, 8.0), (5.0, 3.5)]);
            line(&[(11.0, 8.0), (15.5, 8.0)]);
            if icon == Icon::KeyAdd {
                line(&[(13.25, 5.75), (13.25, 10.25)]);
            }
        }
    }
}

/// A 16 px icon widget (not interactive).
pub fn icon(ui: &mut egui::Ui, icon: Icon) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
    paint(ui.painter(), rect, icon, ICON_COLOR);
    r
}

/// A small icon button.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(22.0, 18.0), Sense::click());
    if r.hovered() {
        ui.painter()
            .rect_filled(rect, 2.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let color = if r.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ICON_COLOR
    };
    paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(14.0)),
        icon,
        color,
    );
    r.on_hover_text(tooltip)
}
