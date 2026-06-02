//! Visual theme for the envctl "GPU control center" dashboard.
//!
//! A single place that owns the dark slate palette, the accent color, the
//! semantic status colors, and the egui `Style`/`Visuals` tuning. Screens pull
//! the semantic constants from here so color usage stays consistent.
use eframe::egui::{
    self, Color32, FontFamily, FontId, Margin, Rounding, Shadow, Stroke, TextStyle,
};

// ── Surfaces ────────────────────────────────────────────────────────────────
/// App background — deepest layer.
pub const BG: Color32 = Color32::from_rgb(0x0e, 0x11, 0x17);
/// Panels (nav / central).
pub const PANEL: Color32 = Color32::from_rgb(0x12, 0x16, 0x1f);
/// Raised surfaces: cards, table rows, inputs.
pub const SURFACE: Color32 = Color32::from_rgb(0x19, 0x1f, 0x2b);
/// Hovered / active surface.
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x22, 0x2a, 0x39);
/// Hairline borders.
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x33, 0x44);

// ── Text ──────────────────────────────────────────────────────────────────--
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xea, 0xf2);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8b, 0x95, 0xa7);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x5e, 0x68, 0x79);

// ── Accent (primary actions / active nav) ─────────────────────────────────--
pub const ACCENT: Color32 = Color32::from_rgb(0x3d, 0x9b, 0xff);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1f, 0x3a, 0x57);
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);

// ── Semantic status colors ─────────────────────────────────────────────────-
/// healthy / present / ok
pub const HEALTHY: Color32 = Color32::from_rgb(0x4a, 0xd6, 0x8c);
/// missing / medium severity / warning
pub const WARN: Color32 = Color32::from_rgb(0xf2, 0xb0, 0x4b);
/// unhealthy / high severity / refused / error
pub const DANGER: Color32 = Color32::from_rgb(0xf2, 0x5f, 0x5f);
/// low severity / informational
pub const INFO: Color32 = Color32::from_rgb(0x6f, 0x9c, 0xc4);
/// neutral / not-applicable
#[allow(dead_code)]
pub const NEUTRAL: Color32 = TEXT_FAINT;

/// A telemetry "load" gradient: green → amber → red as a fraction rises.
pub fn load_color(frac: f32) -> Color32 {
    let f = frac.clamp(0.0, 1.0);
    if f < 0.6 {
        HEALTHY
    } else if f < 0.85 {
        WARN
    } else {
        DANGER
    }
}

/// Install the full theme onto the egui context.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // ── Type scale (built-in proportional + monospace fonts) ────────────────
    use FontFamily::{Monospace, Proportional};
    style.text_styles = [
        (TextStyle::Small, FontId::new(11.0, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Heading, FontId::new(21.0, Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, Monospace)),
    ]
    .into();

    // ── Spacing: generous, breathable ───────────────────────────────────────
    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 8.0);
    s.button_padding = egui::vec2(12.0, 6.0);
    s.menu_margin = Margin::same(8.0);
    s.window_margin = Margin::same(12.0);
    s.indent = 18.0;
    s.interact_size.y = 26.0;
    s.scroll.bar_width = 10.0;

    // ── Visuals ─────────────────────────────────────────────────────────────
    let mut v = egui::Visuals::dark();
    v.dark_mode = true;
    v.override_text_color = Some(TEXT);
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.faint_bg_color = SURFACE;
    v.extreme_bg_color = BG;
    v.code_bg_color = BG;
    v.warn_fg_color = WARN;
    v.error_fg_color = DANGER;
    v.hyperlink_color = ACCENT;

    let round = Rounding::same(8.0);
    v.window_rounding = round;
    v.menu_rounding = round;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_shadow = Shadow {
        offset: egui::vec2(0.0, 6.0),
        blur: 18.0,
        spread: 0.0,
        color: Color32::from_black_alpha(120),
    };
    v.popup_shadow = v.window_shadow;

    // Selection (used by selectable_value / text selection).
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    // Widget states.
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = SURFACE;
    w.noninteractive.weak_bg_fill = SURFACE;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_MUTED);
    w.noninteractive.rounding = round;

    w.inactive.bg_fill = SURFACE;
    w.inactive.weak_bg_fill = SURFACE;
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.rounding = round;

    w.hovered.bg_fill = SURFACE_HOVER;
    w.hovered.weak_bg_fill = SURFACE_HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    w.hovered.fg_stroke = Stroke::new(1.5, TEXT);
    w.hovered.rounding = round;
    w.hovered.expansion = 1.0;

    w.active.bg_fill = ACCENT_DIM;
    w.active.weak_bg_fill = ACCENT_DIM;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.5, TEXT);
    w.active.rounding = round;
    w.active.expansion = 1.0;

    w.open.bg_fill = SURFACE;
    w.open.weak_bg_fill = SURFACE;
    w.open.bg_stroke = Stroke::new(1.0, BORDER);
    w.open.fg_stroke = Stroke::new(1.0, TEXT);
    w.open.rounding = round;

    v.slider_trailing_fill = true;
    style.visuals = v;

    ctx.set_style(style);
}

/// A framed "card" surface for grouped content (GPU cards, forms).
pub fn card() -> egui::Frame {
    egui::Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::same(14.0))
        .outer_margin(Margin::symmetric(0.0, 4.0))
}

/// A subtle inset panel (used for the form / settings).
pub fn inset() -> egui::Frame {
    egui::Frame::none()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::same(16.0))
}

/// Section heading text style helper.
pub fn section(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into())
        .size(13.0)
        .color(TEXT_MUTED)
        .strong()
}
