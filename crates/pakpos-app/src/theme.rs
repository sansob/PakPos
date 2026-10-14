//! PakPos visual identity: deep navy surfaces with a warm orange accent.
//!
//! Navy is the dominant colour (header, sidebar, dark surfaces); orange is
//! reserved for primary actions and selection so it stays meaningful.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Visuals};
use pakpos_core::storage::Theme;
use pakpos_core::HttpMethod;

pub const NAVY_950: Color32 = Color32::from_rgb(0x0A, 0x13, 0x2E);
pub const NAVY_900: Color32 = Color32::from_rgb(0x0F, 0x1B, 0x3F);
pub const NAVY_850: Color32 = Color32::from_rgb(0x13, 0x22, 0x4D);
pub const NAVY_800: Color32 = Color32::from_rgb(0x18, 0x29, 0x5B);
pub const NAVY_700: Color32 = Color32::from_rgb(0x1E, 0x31, 0x70);
pub const NAVY_600: Color32 = Color32::from_rgb(0x2A, 0x41, 0x8E);
pub const ORANGE: Color32 = Color32::from_rgb(0xF7, 0x94, 0x1D);
pub const INK: Color32 = Color32::from_rgb(0x0B, 0x14, 0x30);

pub const GREEN: Color32 = Color32::from_rgb(0x3D, 0xBE, 0x8B);
pub const RED: Color32 = Color32::from_rgb(0xF2, 0x5F, 0x5C);
pub const YELLOW: Color32 = Color32::from_rgb(0xF2, 0xC1, 0x4E);
pub const BLUE: Color32 = Color32::from_rgb(0x5B, 0x9B, 0xF8);
pub const PURPLE: Color32 = Color32::from_rgb(0xA7, 0x8B, 0xFA);
pub const TEAL: Color32 = Color32::from_rgb(0x2B, 0xB3, 0xC0);
pub const LIME: Color32 = Color32::from_rgb(0x9B, 0xD8, 0x5C);
pub const PINK: Color32 = Color32::from_rgb(0xE3, 0x6D, 0xB8);

/// Colours derived from the current theme, used by custom widgets.
#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub surface_alt: Color32,
    pub sidebar: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub header: Color32,
    pub header_text: Color32,
}

impl Palette {
    pub fn of(theme: Theme) -> Self {
        match theme {
            Theme::Dark => Palette {
                dark: true,
                bg: NAVY_950,
                surface: NAVY_900,
                surface_alt: NAVY_850,
                sidebar: NAVY_900,
                border: Color32::from_rgb(0x26, 0x36, 0x6B),
                text: Color32::from_rgb(0xE6, 0xEA, 0xF5),
                text_weak: Color32::from_rgb(0x95, 0xA2, 0xC8),
                header: NAVY_700,
                header_text: Color32::WHITE,
            },
            Theme::Light => Palette {
                dark: false,
                bg: Color32::from_rgb(0xF3, 0xF5, 0xFB),
                surface: Color32::WHITE,
                surface_alt: Color32::from_rgb(0xEC, 0xF0, 0xF9),
                sidebar: Color32::from_rgb(0xF8, 0xF9, 0xFD),
                border: Color32::from_rgb(0xD6, 0xDC, 0xEC),
                text: Color32::from_rgb(0x15, 0x1F, 0x3D),
                text_weak: Color32::from_rgb(0x5D, 0x69, 0x8C),
                header: NAVY_700,
                header_text: Color32::WHITE,
            },
        }
    }
}

pub fn method_color(m: HttpMethod) -> Color32 {
    match m {
        HttpMethod::Get => GREEN,
        HttpMethod::Post => ORANGE,
        HttpMethod::Put => BLUE,
        HttpMethod::Patch => PURPLE,
        HttpMethod::Delete => RED,
        HttpMethod::Head => TEAL,
        HttpMethod::Options => PINK,
    }
}

pub fn status_color(status: u16) -> Color32 {
    match status {
        100..=199 => BLUE,
        200..=299 => GREEN,
        300..=399 => TEAL,
        400..=499 => YELLOW,
        _ => RED,
    }
}

/// Short coloured badge for a request (method, or protocol for non-HTTP).
pub fn badge(r: &pakpos_core::Request) -> (&'static str, Color32) {
    match r.kind {
        pakpos_core::model::RequestKind::Http => (method_short(r.method), method_color(r.method)),
        pakpos_core::model::RequestKind::WebSocket => ("WS", TEAL),
        pakpos_core::model::RequestKind::Grpc => ("gRPC", LIME),
        pakpos_core::model::RequestKind::Sse => ("SSE", PURPLE),
        pakpos_core::model::RequestKind::SocketIo => ("SIO", PINK),
        pakpos_core::model::RequestKind::Mqtt => ("MQTT", YELLOW),
    }
}

/// Short method label used in the sidebar and tabs.
pub fn method_short(m: HttpMethod) -> &'static str {
    match m {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Delete => "DEL",
        HttpMethod::Head => "HEAD",
        HttpMethod::Options => "OPT",
    }
}

/// Height of every button, combo box and single-line input.
pub const CONTROL_HEIGHT: f32 = 28.0;

/// Applies the PakPos style to the egui context.
pub fn apply(ctx: &egui::Context, theme: Theme, font_scale_percent: u32) {
    let p = Palette::of(theme);
    ctx.set_theme(if p.dark { egui::Theme::Dark } else { egui::Theme::Light });
    let mut visuals = if p.dark { Visuals::dark() } else { Visuals::light() };

    let radius = CornerRadius::same(6);
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.surface;
    visuals.window_stroke = Stroke::new(1.0, p.border);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.window_shadow =
        Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if p.dark { 110 } else { 40 }) };
    visuals.popup_shadow =
        Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(if p.dark { 90 } else { 30 }) };
    visuals.extreme_bg_color = if p.dark { INK } else { Color32::WHITE };
    visuals.text_edit_bg_color = Some(if p.dark { INK } else { Color32::WHITE });
    visuals.faint_bg_color = p.surface_alt;
    visuals.code_bg_color = if p.dark { INK } else { Color32::from_rgb(0xF6, 0xF8, 0xFD) };
    visuals.hyperlink_color = ORANGE;
    visuals.selection.bg_fill = if p.dark {
        Color32::from_rgba_unmultiplied(0xF7, 0x94, 0x1D, 70)
    } else {
        Color32::from_rgba_unmultiplied(0xF7, 0x94, 0x1D, 60)
    };
    visuals.selection.stroke = Stroke::new(1.0, ORANGE);
    visuals.warn_fg_color = YELLOW;
    visuals.error_fg_color = RED;
    visuals.striped = true;

    let w = &mut visuals.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;

    let (inactive, hovered, active) = if p.dark {
        (NAVY_800, NAVY_700, NAVY_600)
    } else {
        (Color32::from_rgb(0xE9, 0xED, 0xF7), Color32::from_rgb(0xDD, 0xE3, 0xF3), Color32::from_rgb(0xCF, 0xD7, 0xEE))
    };
    w.inactive.bg_fill = inactive;
    w.inactive.weak_bg_fill = inactive;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = radius;

    w.hovered.bg_fill = hovered;
    w.hovered.weak_bg_fill = hovered;
    w.hovered.bg_stroke = Stroke::new(1.0, ORANGE.gamma_multiply(0.7));
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = active;
    w.active.weak_bg_fill = active;
    w.active.bg_stroke = Stroke::new(1.0, ORANGE);
    w.active.fg_stroke = Stroke::new(2.0, p.text);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

    w.open.bg_fill = active;
    w.open.weak_bg_fill = active;
    w.open.bg_stroke = Stroke::new(1.0, ORANGE);
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = radius;

    ctx.set_visuals(visuals);

    let scale = font_scale_percent.clamp(70, 200) as f32 / 100.0;
    ctx.global_style_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::new(19.0 * scale, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.0 * scale, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0 * scale, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(11.5 * scale, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.0 * scale, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        // One control height everywhere: buttons, combo boxes and single-line inputs.
        style.spacing.button_padding = egui::vec2(12.0, 5.0);
        style.spacing.interact_size.y = CONTROL_HEIGHT;
        style.spacing.window_margin = Margin::same(14);
        style.spacing.menu_margin = Margin::same(6);
        style.spacing.combo_width = 120.0;
    });
}
