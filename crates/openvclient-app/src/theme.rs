//! Visual identity, built on semantic tokens (see design-system/openvclient/MASTER.md).
//! Views ask for roles (`tokens(ui).muted_fg`), never raw hex values.

use egui::{Color32, CornerRadius, FontFamily, FontId, Shadow, Stroke, TextStyle, Visuals};
use openvclient_core::Environment;

/// One theme's semantic colours. Text roles meet 4.5:1 on `bg` and `card`.
#[derive(Clone, Copy)]
pub struct Tokens {
    pub bg: Color32,
    pub card: Color32,
    pub muted: Color32,
    pub fg: Color32,
    pub muted_fg: Color32,
    pub border: Color32,
    pub secondary: Color32,
    /// Primary action (Run, Connect, Save). Text-safe on `bg`.
    pub accent: Color32,
    pub on_accent: Color32,
    pub destructive: Color32,
    pub warning: Color32,
    pub info: Color32,
    pub ring: Color32,
    pub selection: Color32,
}

/// Slate dark, from the skill's developer-tool palette ("code dark + run green").
pub const DARK: Tokens = Tokens {
    bg: Color32::from_rgb(0x0f, 0x17, 0x2a),
    card: Color32::from_rgb(0x1b, 0x23, 0x36),
    muted: Color32::from_rgb(0x27, 0x2f, 0x42),
    fg: Color32::from_rgb(0xf8, 0xfa, 0xfc),
    muted_fg: Color32::from_rgb(0x94, 0xa3, 0xb8),
    border: Color32::from_rgb(0x33, 0x41, 0x55),
    secondary: Color32::from_rgb(0x33, 0x41, 0x55),
    accent: Color32::from_rgb(0x22, 0xc5, 0x5e),
    on_accent: Color32::from_rgb(0x0f, 0x17, 0x2a),
    destructive: Color32::from_rgb(0xf8, 0x71, 0x71),
    warning: Color32::from_rgb(0xfb, 0xbf, 0x24),
    info: Color32::from_rgb(0x60, 0xa5, 0xfa),
    ring: Color32::from_rgb(0x93, 0xc5, 0xfd),
    selection: Color32::from_rgb(0x1e, 0x3a, 0x5f),
};

/// Light counterpart: same hues, darkened until text roles pass 4.5:1 on white.
pub const LIGHT: Tokens = Tokens {
    bg: Color32::from_rgb(0xf8, 0xfa, 0xfc),
    card: Color32::WHITE,
    muted: Color32::from_rgb(0xf1, 0xf5, 0xf9),
    fg: Color32::from_rgb(0x0f, 0x17, 0x2a),
    muted_fg: Color32::from_rgb(0x47, 0x55, 0x69),
    border: Color32::from_rgb(0xcb, 0xd5, 0xe1),
    secondary: Color32::from_rgb(0xe2, 0xe8, 0xf0),
    accent: Color32::from_rgb(0x15, 0x80, 0x3d),
    on_accent: Color32::WHITE,
    destructive: Color32::from_rgb(0xb9, 0x1c, 0x1c),
    warning: Color32::from_rgb(0xa1, 0x62, 0x07),
    info: Color32::from_rgb(0x1d, 0x4e, 0xd8),
    ring: Color32::from_rgb(0x1d, 0x4e, 0xd8),
    selection: Color32::from_rgb(0xdb, 0xea, 0xfe),
};

pub fn tokens(ui: &egui::Ui) -> Tokens {
    tokens_for(ui.visuals().dark_mode)
}

pub fn tokens_for(dark: bool) -> Tokens {
    if dark { DARK } else { LIGHT }
}

/// Spacing scale for a dense tool (skill density 9: 8–32 px).
pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

/// Icon sizes as tokens.
pub mod icon {
    pub const SM: f32 = 14.0;
    pub const MD: f32 = 16.0;
    pub const LG: f32 = 20.0;
}

pub fn env_color(ui: &egui::Ui, e: Environment) -> Color32 {
    let t = tokens(ui);
    match e {
        Environment::Development => t.accent,
        Environment::Testing => t.info,
        Environment::Staging => t.warning,
        Environment::Production => t.destructive,
    }
}

pub fn muted(ui: &egui::Ui) -> Color32 {
    tokens(ui).muted_fg
}

pub struct SyntaxColors {
    pub keyword: Color32,
    pub ident: Color32,
    pub quoted: Color32,
    pub string: Color32,
    pub number: Color32,
    pub comment: Color32,
    pub operator: Color32,
    pub param: Color32,
}

/// Every syntax colour clears 4.5:1 on the editor background of its theme.
pub fn syntax(dark: bool) -> SyntaxColors {
    if dark {
        SyntaxColors {
            keyword: Color32::from_rgb(0xc0, 0x84, 0xfc),
            ident: Color32::from_rgb(0xe2, 0xe8, 0xf0),
            quoted: Color32::from_rgb(0x7d, 0xd3, 0xfc),
            string: Color32::from_rgb(0x86, 0xef, 0xac),
            number: Color32::from_rgb(0xfd, 0xba, 0x74),
            comment: Color32::from_rgb(0x94, 0xa3, 0xb8),
            operator: Color32::from_rgb(0x67, 0xe8, 0xf9),
            param: Color32::from_rgb(0xfc, 0xd3, 0x4d),
        }
    } else {
        SyntaxColors {
            keyword: Color32::from_rgb(0x6b, 0x21, 0xa8),
            ident: Color32::from_rgb(0x0f, 0x17, 0x2a),
            quoted: Color32::from_rgb(0x07, 0x59, 0x85),
            string: Color32::from_rgb(0x16, 0x65, 0x34),
            number: Color32::from_rgb(0x9a, 0x34, 0x12),
            comment: Color32::from_rgb(0x47, 0x55, 0x69),
            operator: Color32::from_rgb(0x15, 0x5e, 0x75),
            param: Color32::from_rgb(0x85, 0x4d, 0x0e),
        }
    }
}

fn visuals(t: Tokens, dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    let r = CornerRadius::same(6);
    v.panel_fill = t.bg;
    v.window_fill = t.card;
    v.extreme_bg_color = if dark { Color32::from_rgb(0x0b, 0x11, 0x20) } else { Color32::WHITE };
    v.faint_bg_color = t.muted;
    v.code_bg_color = t.muted;
    v.text_edit_bg_color = Some(v.extreme_bg_color);
    v.weak_text_color = Some(t.muted_fg);
    v.warn_fg_color = t.warning;
    v.error_fg_color = t.destructive;
    v.hyperlink_color = t.info;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if dark { 120 } else { 40 }) };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(if dark { 110 } else { 35 }) };
    v.selection.bg_fill = t.selection;
    // Visible focus ring on every focused control.
    v.selection.stroke = Stroke::new(1.5, t.ring);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_fill = t.bg;
    v.widgets.noninteractive.weak_bg_fill = t.bg;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.fg);
    v.widgets.inactive.bg_fill = t.muted;
    v.widgets.inactive.weak_bg_fill = t.muted;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.fg);
    v.widgets.hovered.bg_fill = t.secondary;
    v.widgets.hovered.weak_bg_fill = t.secondary;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.muted_fg);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, t.fg);
    v.widgets.active.bg_fill = t.secondary;
    v.widgets.active.weak_bg_fill = t.secondary;
    v.widgets.active.bg_stroke = Stroke::new(1.5, t.ring);
    v.widgets.active.fg_stroke = Stroke::new(2.0, t.fg);
    v.widgets.open.weak_bg_fill = t.secondary;
    v.widgets.open.bg_stroke = Stroke::new(1.0, t.border);
    v.striped = true;
    v
}

/// JetBrains Mono for code and grids, IBM Plex Sans for UI text (both SIL OFL,
/// licences in assets/fonts). egui's bundled fonts stay as fallbacks.
fn install_fonts(ctx: &egui::Context) {
    let mut defs = egui::FontDefinitions::default();
    defs.font_data.insert("plex".into(), std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/IBMPlexSans.ttf"))));
    defs.font_data.insert("jbmono".into(), std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/JetBrainsMono.ttf"))));
    defs.families.entry(FontFamily::Proportional).or_default().insert(0, "plex".into());
    defs.families.entry(FontFamily::Monospace).or_default().insert(0, "jbmono".into());
    // Plex lacks ⌘ ⇧ ⌃ ▲ ▼; JetBrains Mono has them.
    defs.families.entry(FontFamily::Proportional).or_default().insert(1, "jbmono".into());
    ctx.set_fonts(defs);
    // Added after set_fonts, which would otherwise replace it.
    egui_material_icons::initialize(ctx);
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_visuals_of(egui::Theme::Dark, visuals(DARK, true));
    ctx.set_visuals_of(egui::Theme::Light, visuals(LIGHT, false));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(space::SM, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.menu_margin = egui::Margin::same(6);
        // Body stays ≥13 px: dense, but above the 12 px floor.
        s.text_styles.insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(13.5, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(12.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace));
        s.text_styles.insert(TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional));
        // Short, consistent motion; egui has no reduced-motion signal, so keep it minimal.
        s.animation_time = 0.12;
    });
}

/// A small pill label, e.g. the PROD badge. Text plus colour, never colour alone.
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::new(11.0, FontFamily::Proportional), color);
    let size = galley.size() + egui::vec2(10.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(4), color.gamma_multiply(0.16));
    ui.painter().rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, color.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

/// Status dot; callers always pair it with a text label or tooltip.
pub fn dot(ui: &mut egui::Ui, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    resp
}

/// Shortcut text for this platform: `⌘⇧K` on macOS, `Ctrl+Shift+K` elsewhere.
/// Enter is spelled out because the bundled fonts have no Return glyph.
pub fn kbd(s: &str) -> String {
    let s = s.replace('⏎', "Enter");
    if cfg!(target_os = "macos") {
        // A thin space keeps "⌘Enter" readable.
        return s.replace("⌘Enter", "⌘ Enter").replace("⇧Enter", "⇧ Enter");
    }
    s.replace('⌘', "Ctrl+").replace('⇧', "Shift+").replace('⌃', "Ctrl+").replace("Ctrl+Ctrl+", "Ctrl+")
}

/// Icon (vector font) followed by text, for buttons and labels.
pub fn ico(icon: egui_material_icons::MaterialIcon, text: &str) -> String {
    let glyph = String::from(icon);
    if text.is_empty() { glyph } else { format!("{glyph}  {text}") }
}

/// Primary call-to-action button (Run, Connect, Save).
pub fn primary_button(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    let t = tokens(ui);
    ui.add(egui::Button::new(egui::RichText::new(text.into()).color(t.on_accent).strong()).fill(t.accent))
}

/// Destructive button; always behind a confirmation in the caller.
pub fn danger_button(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    let t = tokens(ui);
    ui.add(egui::Button::new(egui::RichText::new(text.into()).color(Color32::WHITE).strong()).fill(if ui.visuals().dark_mode {
        Color32::from_rgb(0xb9, 0x1c, 0x1c)
    } else {
        t.destructive
    }))
}
