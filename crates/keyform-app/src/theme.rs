//! Look and feel: flat dark (default) and light themes.
//!
//! spec: 50-ui/theme.md §1–3 (colour values are design tokens)

use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};

fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Design tokens used outside egui's visuals.
#[derive(Clone, Copy)]
pub struct Tokens {
    pub background: Color32,
    pub foreground: Color32,
    pub secondary_text: Color32,
    pub accent: Color32,
    pub list_background: Color32,
    pub canvas_viewport: Color32,
    pub border: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub info: Color32,
    pub parameter_row: Color32,
    pub parameter_row_selected: Color32,
}

pub const DARK: fn() -> Tokens = || Tokens {
    background: hex(0x323232),
    foreground: hex(0xe0e0e0),
    secondary_text: hex(0xa0a0a0),
    accent: hex(0x3d87e8),
    list_background: hex(0x282828),
    canvas_viewport: hex(0x1e1e1e),
    border: hex(0x4e4e4e),
    warning: hex(0xd7bf20),
    error: hex(0xff3052),
    info: hex(0x20d5d7),
    parameter_row: hex(0x282828),
    parameter_row_selected: hex(0x3c3c3c),
};

pub const LIGHT: fn() -> Tokens = || Tokens {
    background: hex(0xf2f2f2),
    foreground: Color32::BLACK,
    secondary_text: hex(0x606060),
    accent: hex(0x1070e0),
    list_background: hex(0xffffff),
    canvas_viewport: hex(0xd0d0d0),
    border: hex(0xcecece),
    warning: hex(0x9a8400),
    error: hex(0xd0002a),
    info: hex(0x00888a),
    parameter_row: hex(0xffffff),
    parameter_row_selected: hex(0xd5d5d5),
};

pub fn apply(ctx: &egui::Context, dark: bool) -> Tokens {
    let t = if dark { DARK() } else { LIGHT() };
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let (button, title, selection, focused) = if dark {
        (hex(0x414141), hex(0x1e1e1e), hex(0x515151), hex(0x25518b))
    } else {
        (hex(0xe4e4e4), hex(0xe0e0e0), hex(0xd5d5d5), hex(0x6f9fe0))
    };
    v.panel_fill = t.background;
    v.window_fill = t.background;
    v.extreme_bg_color = t.list_background;
    v.faint_bg_color = t.list_background;
    v.code_bg_color = t.list_background;
    v.hyperlink_color = t.accent;
    v.warn_fg_color = t.warning;
    v.error_fg_color = t.error;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.selection.bg_fill = if dark { hex(0x3e71b4) } else { hex(0xa9c8f0) };
    v.selection.stroke = Stroke::new(1.0, t.foreground);
    v.override_text_color = Some(t.foreground);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(2);
        w.fg_stroke.color = t.foreground;
    }
    v.widgets.noninteractive.bg_fill = t.background;
    v.widgets.noninteractive.weak_bg_fill = t.background;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.inactive.bg_fill = button;
    v.widgets.inactive.weak_bg_fill = button;
    v.widgets.hovered.bg_fill = selection;
    v.widgets.hovered.weak_bg_fill = selection;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.accent);
    v.widgets.active.bg_fill = focused;
    v.widgets.active.weak_bg_fill = focused;
    v.widgets.open.bg_fill = title;
    v.widgets.open.weak_bg_fill = title;
    ctx.set_visuals(v);

    // Dense metrics, 12 px UI font (theme.md §3).
    let mut style = (*ctx.global_style()).clone();
    use egui::{FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(15.0)),
        (TextStyle::Body, FontId::proportional(12.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
        (TextStyle::Button, FontId::proportional(12.0)),
        (TextStyle::Small, FontId::proportional(10.0)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(6.0, 3.0);
    style.spacing.button_padding = egui::vec2(5.0, 2.0);
    style.spacing.interact_size.y = 18.0;
    ctx.set_global_style(style);
    t
}
