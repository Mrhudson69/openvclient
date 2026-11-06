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

/// Cartoon, day: the website's cream paper, ink outlines and pop colours.
/// Text roles still meet 4.5:1 on `bg` and `card`.
pub const CARTOON_DAY: Tokens = Tokens {
    bg: Color32::from_rgb(0xff, 0xf7, 0xe6),
    card: Color32::WHITE,
    muted: Color32::from_rgb(0xff, 0xef, 0xcc),
    fg: Color32::from_rgb(0x1a, 0x16, 0x12),
    muted_fg: Color32::from_rgb(0x4a, 0x40, 0x36),
    border: Color32::from_rgb(0xd9, 0xc7, 0xa3),
    secondary: Color32::from_rgb(0xff, 0xe9, 0x8f),
    accent: Color32::from_rgb(0x15, 0x80, 0x3d),
    on_accent: Color32::WHITE,
    destructive: Color32::from_rgb(0xb9, 0x1c, 0x1c),
    warning: Color32::from_rgb(0x92, 0x56, 0x05),
    info: Color32::from_rgb(0x1d, 0x4e, 0xd8),
    ring: Color32::from_rgb(0x1d, 0x4e, 0xd8),
    selection: Color32::from_rgb(0xff, 0xe9, 0x8f),
};

/// Cartoon, night: the website's indigo night palette.
pub const CARTOON_NIGHT: Tokens = Tokens {
    bg: Color32::from_rgb(0x14, 0x12, 0x1f),
    card: Color32::from_rgb(0x24, 0x20, 0x3a),
    muted: Color32::from_rgb(0x1c, 0x19, 0x30),
    fg: Color32::from_rgb(0xf4, 0xf1, 0xff),
    muted_fg: Color32::from_rgb(0xc9, 0xc3, 0xe0),
    border: Color32::from_rgb(0x3a, 0x35, 0x58),
    secondary: Color32::from_rgb(0x33, 0x2e, 0x52),
    accent: Color32::from_rgb(0x22, 0xc5, 0x5e),
    on_accent: Color32::from_rgb(0x0a, 0x09, 0x10),
    destructive: Color32::from_rgb(0xff, 0x6b, 0x6b),
    warning: Color32::from_rgb(0xff, 0xdb, 0x58),
    info: Color32::from_rgb(0x9d, 0xba, 0xf6),
    ring: Color32::from_rgb(0xff, 0xdb, 0x58),
    selection: Color32::from_rgb(0x3a, 0x33, 0x5e),
};

/// Pop colours of the cartoon theme (fills under ink text, never text colours).
pub mod pop {
    use egui::Color32;
    pub const YELLOW: Color32 = Color32::from_rgb(0xff, 0xdb, 0x58);
    pub const GREEN: Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);
    pub const BLUE_L: Color32 = Color32::from_rgb(0x9d, 0xba, 0xf6);
    pub const CORAL_L: Color32 = Color32::from_rgb(0xff, 0x9b, 0x9b);
    pub const TEAL_L: Color32 = Color32::from_rgb(0x8f, 0xe3, 0xdd);
    pub const YELLOW_L: Color32 = Color32::from_rgb(0xff, 0xe9, 0x8f);
    pub const PINK_L: Color32 = Color32::from_rgb(0xff, 0xc2, 0xe6);
    /// Text on pop fills, in both day and night.
    pub const INK: Color32 = Color32::from_rgb(0x1a, 0x16, 0x12);
}

static CARTOON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True while the Cartoon theme is selected.
pub fn is_cartoon() -> bool {
    CARTOON.load(std::sync::atomic::Ordering::Relaxed)
}

/// Switch between the original themes and the cartoon ones. Both keep a day
/// and a night variant; egui picks between them from the theme preference.
pub fn set_cartoon(ctx: &egui::Context, on: bool) {
    CARTOON.store(on, std::sync::atomic::Ordering::Relaxed);
    if on {
        ctx.set_visuals_of(egui::Theme::Dark, cartoon_visuals(CARTOON_NIGHT, true));
        ctx.set_visuals_of(egui::Theme::Light, cartoon_visuals(CARTOON_DAY, false));
    } else {
        ctx.set_visuals_of(egui::Theme::Dark, visuals(DARK, true));
        ctx.set_visuals_of(egui::Theme::Light, visuals(LIGHT, false));
    }
}

/// Cartoon outline colour: ink by day, a lavender line by night (ink would vanish).
pub fn outline(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(0x5b, 0x54, 0x80) } else { pop::INK }
}

/// Hard offset shadow colour of the cartoon theme.
pub fn hard_shadow(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(0x0a, 0x09, 0x10) } else { pop::INK }
}

pub fn tokens(ui: &egui::Ui) -> Tokens {
    tokens_for(ui.visuals().dark_mode)
}

pub fn tokens_for(dark: bool) -> Tokens {
    match (is_cartoon(), dark) {
        (true, true) => CARTOON_NIGHT,
        (true, false) => CARTOON_DAY,
        (false, true) => DARK,
        (false, false) => LIGHT,
    }
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

/// Cartoon look: thick ink outlines, hard offset shadows, rounder corners and
/// pop-colour hover/press states, as on openvclient.xyz.
fn cartoon_visuals(t: Tokens, dark: bool) -> Visuals {
    let mut v = visuals(t, dark);
    let ink = outline(dark);
    let shadow = hard_shadow(dark);
    v.extreme_bg_color = if dark { Color32::from_rgb(0x10, 0x0e, 0x1a) } else { Color32::WHITE };
    v.text_edit_bg_color = Some(v.extreme_bg_color);
    v.faint_bg_color = if dark { Color32::from_rgb(0x1a, 0x17, 0x29) } else { Color32::from_rgb(0xff, 0xf3, 0xda) };
    v.window_stroke = Stroke::new(2.0, ink);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow { offset: [6, 6], blur: 0, spread: 0, color: shadow };
    v.popup_shadow = Shadow { offset: [4, 4], blur: 0, spread: 0, color: shadow };
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(8);
    }
    let press = if dark { Color32::from_rgb(0x4a, 0x41, 0x7a) } else { pop::YELLOW };
    // Slider rails and other tracks paint with bg_fill; buttons use weak_bg_fill.
    v.widgets.inactive.bg_fill = if dark { Color32::from_rgb(0x3a, 0x35, 0x58) } else { Color32::from_rgb(0xf0, 0xdf, 0xbb) };
    v.widgets.inactive.weak_bg_fill = t.card;
    v.widgets.inactive.bg_stroke = Stroke::new(1.5, ink);
    v.widgets.hovered.bg_fill = t.secondary;
    v.widgets.hovered.weak_bg_fill = t.secondary;
    v.widgets.hovered.bg_stroke = Stroke::new(2.0, ink);
    v.widgets.active.bg_fill = press;
    v.widgets.active.weak_bg_fill = press;
    v.widgets.active.bg_stroke = Stroke::new(2.0, ink);
    v.widgets.open.weak_bg_fill = t.secondary;
    v.widgets.open.bg_stroke = Stroke::new(1.5, ink);
    v.selection.bg_fill = t.selection;
    v.selection.stroke = Stroke::new(2.0, t.ring);
    v
}

/// Card background: flat in the original themes; outlined with a hard shadow
/// that grows on hover in the cartoon theme.
pub fn paint_card(ui: &egui::Ui, rect: egui::Rect, hovered: bool) {
    let t = tokens(ui);
    if is_cartoon() {
        let dark = ui.visuals().dark_mode;
        let lift = if hovered { 5.0 } else { 3.0 };
        ui.painter().rect_filled(rect.translate(egui::vec2(lift, lift)), 12.0, hard_shadow(dark));
        ui.painter().rect(rect, 12.0, if hovered { t.secondary } else { t.card }, Stroke::new(2.0, outline(dark)), egui::StrokeKind::Inside);
    } else {
        ui.painter().rect(rect, 10.0, if hovered { t.muted } else { t.card }, Stroke::new(if hovered { 1.5 } else { 1.0 }, if hovered { t.ring } else { t.border }), egui::StrokeKind::Inside);
    }
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
    if is_cartoon() {
        // Sticker button: green fill, ink text, hard shadow that the press pushes into.
        let dark = ui.visuals().dark_mode;
        let slot = ui.painter().add(egui::Shape::Noop);
        let r = ui.add(egui::Button::new(egui::RichText::new(text.into()).color(pop::INK).strong()).fill(pop::GREEN));
        let off = if r.is_pointer_button_down_on() { 1.0 } else { 3.0 };
        ui.painter().set(slot, egui::Shape::rect_filled(r.rect.translate(egui::vec2(off, off)), 8.0, hard_shadow(dark)));
        return r;
    }
    ui.add(egui::Button::new(egui::RichText::new(text.into()).color(t.on_accent).strong()).fill(t.accent))
}

/// Connector group, used for the cartoon stickers' colour.
fn connector_group(driver: &str) -> Color32 {
    match driver {
        "mongodb" | "redis" | "elasticsearch" | "rabbitmq" | "kafka" | "cassandra" => pop::CORAL_L,
        "s3" | "azure_blob" | "b2" | "idrive_e2" | "wasabi" | "r2" | "do_spaces" | "gcs" | "oci" | "linode" | "scaleway" | "hetzner" | "storj" | "minio" | "alibaba_oss" => pop::TEAL_L,
        "ssh" | "ftp" | "docker" => pop::YELLOW_L,
        "jdbc" => pop::PINK_L,
        _ => pop::BLUE_L,
    }
}

/// Two-letter sticker label for a connector, as on the website's connector board.
pub fn connector_abbr(driver: &str) -> String {
    let a = match driver {
        "postgres" => "PG",
        "mysql" => "MY",
        "mariadb" => "MA",
        "sqlserver" => "MS",
        "sqlite" => "SL",
        "clickhouse" => "CH",
        "kingbase" => "KB",
        "gaussdb" => "GA",
        "mongodb" => "MG",
        "redis" => "RD",
        "elasticsearch" => "ES",
        "rabbitmq" => "MQ",
        "kafka" => "KF",
        "cassandra" => "CA",
        "oracle" => "OR",
        "db2" => "D2",
        "s3" => "S3",
        "azure_blob" => "AZ",
        "b2" => "B2",
        "idrive_e2" => "E2",
        "r2" => "R2",
        "wasabi" => "WA",
        "do_spaces" => "DO",
        "gcs" => "GC",
        "oci" => "OC",
        "linode" => "LI",
        "scaleway" => "SC",
        "hetzner" => "HZ",
        "storj" => "ST",
        "minio" => "MI",
        "alibaba_oss" => "AL",
        "ssh" => "SH",
        "ftp" => "FT",
        "docker" => "DK",
        other => return other.chars().filter(|c| c.is_ascii_alphanumeric()).take(2).collect::<String>().to_uppercase(),
    };
    a.to_string()
}

/// Cartoon connector logo: a pop-colour sticker with an ink outline, a hard
/// shadow and the connector's two-letter code, centred on `center`.
pub fn paint_connector_sticker(painter: &egui::Painter, center: egui::Pos2, size: f32, driver: &str, dark: bool) {
    let s = size;
    let rect = egui::Rect::from_center_size(center - egui::vec2(s * 0.04, s * 0.04), egui::vec2(s * 0.92, s * 0.92));
    let r = (s * 0.26).round() as u8;
    let line = (s * 0.075).clamp(1.2, 3.0);
    painter.rect_filled(rect.translate(egui::vec2(s * 0.09, s * 0.09)), r, hard_shadow(dark));
    painter.rect(rect, r, connector_group(driver), Stroke::new(line, if dark { hard_shadow(true) } else { pop::INK }), egui::StrokeKind::Inside);
    let font = FontId::new((s * 0.42).max(7.0), FontFamily::Monospace);
    let label = connector_abbr(driver);
    // Faux bold: the mono face has one weight, so draw it twice a hair apart.
    for dx in [0.0, s * 0.025] {
        painter.text(rect.center() + egui::vec2(dx, s * 0.01), egui::Align2::CENTER_CENTER, &label, font.clone(), pop::INK);
    }
}

/// Destructive button; always behind a confirmation in the caller.
pub fn danger_button(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    let t = tokens(ui);
    ui.add(egui::Button::new(egui::RichText::new(text.into()).color(Color32::WHITE).strong()).fill(if ui.visuals().dark_mode { Color32::from_rgb(0xb9, 0x1c, 0x1c) } else { t.destructive }))
}

/// The OpenVClient logo mark ("Vee"), matching the current theme: the light
/// variant has an ink outline, the dark one a cream outline and coral shadow.
/// Textures are decoded once and cached in egui's memory.
pub fn brand_mark(ui: &mut egui::Ui, size: f32) -> egui::Response {
    let tex = brand_texture(ui.ctx(), ui.visuals().dark_mode);
    ui.add(egui::Image::new(egui::load::SizedTexture::new(tex.id(), egui::vec2(size, size))))
}

/// The mark painted into `rect` without taking layout space (e.g. inside a tab).
pub fn paint_brand_mark(ui: &egui::Ui, rect: egui::Rect) {
    let tex = brand_texture(ui.ctx(), ui.visuals().dark_mode);
    ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
}

fn brand_texture(ctx: &egui::Context, dark: bool) -> egui::TextureHandle {
    let key = egui::Id::unique(("openvclient_brand_mark", dark));
    ctx.data_mut(|d| d.get_temp::<egui::TextureHandle>(key)).unwrap_or_else(|| {
        let bytes: &[u8] = if dark { include_bytes!("../assets/brand/mark-dark.png") } else { include_bytes!("../assets/brand/mark-light.png") };
        let img = image::load_from_memory(bytes).map(|i| i.to_rgba8()).unwrap_or_default();
        let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        let tex = ctx.load_texture("openvclient-brand-mark", color, egui::TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(key, tex.clone()));
        tex
    })
}

/// "OpenVClient" with the brand-green V, as in the logo.
pub fn wordmark(ui: &mut egui::Ui, size: f32) -> egui::Response {
    let t = tokens(ui);
    let mut job = egui::text::LayoutJob::default();
    let fmt = |c: Color32| egui::TextFormat { font_id: egui::FontId::proportional(size), color: c, ..Default::default() };
    job.append("Open", 0.0, fmt(t.fg));
    job.append("V", 0.0, fmt(t.accent));
    job.append("Client", 0.0, fmt(t.fg));
    ui.add(egui::Label::new(job).selectable(false))
}

/// Cartoon version of a line icon, drawn in code in the website's style (ink
/// outline, pop fill). Folders, containers, databases, tables and files get
/// their own drawings; any other icon becomes a small sticker tile with the
/// glyph in ink. Decorative: callers always draw the item's name next to it.
pub fn paint_cartoon_icon(painter: &egui::Painter, center: egui::Pos2, size: f32, icon: egui_material_icons::MaterialIcon, dark: bool) {
    use egui::{Pos2, Rect, Shape, pos2, vec2};
    use egui_material_icons::icons::*;
    let g = String::from(icon);
    let is = |i: egui_material_icons::MaterialIcon| String::from(i) == g;
    let ink = if dark { hard_shadow(true) } else { pop::INK };
    let line = Stroke::new((size * 0.09).clamp(1.2, 2.4), ink);
    let s = size;
    let o = center - vec2(s / 2.0, s / 2.0);
    let at = |x: f32, y: f32| -> Pos2 { o + vec2(x * s, y * s) };
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(at(x0, y0), at(x1, y1));
    let rad = (s * 0.12).round() as u8;

    if is(ICON_FOLDER) || is(ICON_FOLDER_OPEN) || is(ICON_FOLDER_SHARED) || is(ICON_FOLDER_ZIP) {
        // Folder: a tab at the back, then the body.
        painter.rect(rect(0.08, 0.16, 0.5, 0.42), rad, pop::YELLOW, line, egui::StrokeKind::Middle);
        painter.rect(rect(0.08, 0.28, 0.92, 0.84), rad, pop::YELLOW, line, egui::StrokeKind::Middle);
        if is(ICON_FOLDER_ZIP) {
            painter.line_segment([at(0.5, 0.34), at(0.5, 0.78)], Stroke::new(line.width, ink));
        }
        return;
    }
    if is(ICON_INVENTORY_2) || is(ICON_INVENTORY) {
        // Container / bucket: a teal box with a lid and a handle slot.
        painter.rect(rect(0.14, 0.36, 0.86, 0.86), rad, pop::TEAL_L, line, egui::StrokeKind::Middle);
        painter.rect(rect(0.08, 0.16, 0.92, 0.38), rad, Color32::from_rgb(0x4e, 0xcd, 0xc4), line, egui::StrokeKind::Middle);
        painter.line_segment([at(0.4, 0.54), at(0.6, 0.54)], Stroke::new(line.width * 1.2, ink));
        return;
    }
    if is(ICON_DATABASE) || is(ICON_STORAGE) || is(ICON_DNS) {
        // Database: a blue cylinder.
        let (l, r, top, bot, ry) = (0.16, 0.84, 0.26, 0.76, 0.12);
        painter.add(Shape::ellipse_filled(at(0.5, bot), vec2(0.34 * s, ry * s), pop::BLUE_L));
        painter.rect_filled(rect(l, top, r, bot), 0, pop::BLUE_L);
        painter.add(Shape::ellipse_stroke(at(0.5, bot), vec2(0.34 * s, ry * s), line));
        painter.rect_filled(rect(l + 0.02, top, r - 0.02, bot), 0, pop::BLUE_L);
        painter.line_segment([at(l, top), at(l, bot)], line);
        painter.line_segment([at(r, top), at(r, bot)], line);
        painter.add(Shape::ellipse_filled(at(0.5, top), vec2(0.34 * s, ry * s), Color32::from_rgb(0xc4, 0xd6, 0xfa)));
        painter.add(Shape::ellipse_stroke(at(0.5, top), vec2(0.34 * s, ry * s), line));
        return;
    }
    if is(ICON_TABLE) || is(ICON_TABLE_VIEW) || is(ICON_TABLE_CHART) || is(ICON_VIEW_COLUMN) {
        // Table: a white card with a coloured header row and grid lines.
        let card = rect(0.1, 0.16, 0.9, 0.84);
        painter.rect_filled(card, rad, Color32::WHITE);
        painter.rect_filled(rect(0.1, 0.16, 0.9, 0.38), egui::CornerRadius { nw: rad, ne: rad, sw: 0, se: 0 }, if is(ICON_TABLE_CHART) { pop::GREEN } else { pop::BLUE_L });
        let thin = Stroke::new(line.width * 0.6, ink);
        painter.line_segment([at(0.1, 0.61), at(0.9, 0.61)], thin);
        painter.line_segment([at(0.5, 0.38), at(0.5, 0.84)], thin);
        painter.line_segment([at(0.1, 0.38), at(0.9, 0.38)], line);
        painter.rect_stroke(card, rad, line, egui::StrokeKind::Middle);
        return;
    }
    let page = [
        (ICON_DESCRIPTION, Color32::WHITE),
        (ICON_DRAFT, Color32::WHITE),
        (ICON_IMAGE, pop::PINK_L),
        (ICON_DATA_OBJECT, pop::TEAL_L),
        (ICON_PICTURE_AS_PDF, pop::CORAL_L),
        (ICON_MOVIE, pop::CORAL_L),
        (ICON_AUDIO_FILE, pop::PINK_L),
        (ICON_CODE, pop::BLUE_L),
    ];
    if let Some((_, band)) = page.iter().find(|(i, _)| is(*i)) {
        // File: a page with a folded corner; the band colour gives the type.
        let pts = vec![at(0.2, 0.1), at(0.6, 0.1), at(0.8, 0.3), at(0.8, 0.9), at(0.2, 0.9)];
        painter.add(Shape::convex_polygon(pts.clone(), Color32::WHITE, Stroke::NONE));
        if *band != Color32::WHITE {
            painter.rect_filled(rect(0.2, 0.56, 0.8, 0.9), egui::CornerRadius::ZERO, *band);
        }
        painter.add(Shape::closed_line(pts, line));
        painter.add(Shape::convex_polygon(vec![at(0.6, 0.1), at(0.8, 0.3), at(0.6, 0.3)], pop::YELLOW_L, line));
        if *band == Color32::WHITE {
            let thin = Stroke::new(line.width * 0.6, ink);
            painter.line_segment([at(0.32, 0.5), at(0.68, 0.5)], thin);
            painter.line_segment([at(0.32, 0.66), at(0.68, 0.66)], thin);
        }
        return;
    }
    // Anything else: a little sticker tile with the glyph in ink.
    let tile = rect(0.06, 0.06, 0.94, 0.94);
    let fill = if is(ICON_KEY) || is(ICON_LOCK) {
        pop::YELLOW_L
    } else if is(ICON_FUNCTIONS) || is(ICON_SCHEDULE) || is(ICON_BOLT) {
        pop::PINK_L
    } else if is(ICON_PERSON) || is(ICON_GROUP) {
        pop::CORAL_L
    } else {
        pop::TEAL_L
    };
    painter.rect(tile, rad + 1, fill, Stroke::new(line.width * 0.8, ink), egui::StrokeKind::Inside);
    painter.text(tile.center(), egui::Align2::CENTER_CENTER, g, FontId::proportional(s * 0.66), pop::INK);
    let _ = pos2;
}
