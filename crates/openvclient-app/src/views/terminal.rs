//! Interactive terminal tab (SSH shell with a PTY), rendered from a vt100
//! screen model.

use egui::{Color32, FontId, Key, RichText, Sense, Stroke, pos2, vec2};
use egui_material_icons::icons::*;
use openvclient_core::resource::{TermEvent, TermInput, Terminal};
use uuid::Uuid;

use crate::app::AppCtx;
use crate::bg::{Job, poll};
use crate::theme::{self, ico};

const SCROLLBACK: usize = 5000;
const FONT: f32 = 13.0;

fn nz(v: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(v.max(1)).unwrap_or(std::num::NonZeroU16::MIN)
}

pub struct TerminalTab {
    pub conn: Uuid,
    term: Option<Terminal>,
    job: Option<Job<openvclient_core::Result<Terminal>>>,
    parser: vt100::Parser,
    size: (u16, u16),
    closed: Option<String>,
    error: Option<openvclient_core::Error>,
    scroll: usize,
    focused_once: bool,
}

/// The xterm 16-colour palette, then the 6×6×6 cube and grey ramp.
fn idx_color(i: u8, dark: bool) -> Color32 {
    const BASE: [(u8, u8, u8); 16] = [
        (0x1e, 0x29, 0x3b),
        (0xef, 0x44, 0x44),
        (0x22, 0xc5, 0x5e),
        (0xea, 0xb3, 0x08),
        (0x3b, 0x82, 0xf6),
        (0xa8, 0x55, 0xf7),
        (0x06, 0xb6, 0xd4),
        (0xcb, 0xd5, 0xe1),
        (0x64, 0x74, 0x8b),
        (0xf8, 0x71, 0x71),
        (0x4a, 0xde, 0x80),
        (0xfa, 0xcc, 0x15),
        (0x60, 0xa5, 0xfa),
        (0xc0, 0x84, 0xfc),
        (0x22, 0xd3, 0xee),
        (0xf8, 0xfa, 0xfc),
    ];
    match i {
        0..=15 => {
            let (r, g, b) = BASE[i as usize];
            // Keep "white" readable on the light theme.
            if !dark && (i == 7 || i == 15) { Color32::from_rgb(0x33, 0x41, 0x55) } else { Color32::from_rgb(r, g, b) }
        }
        16..=231 => {
            let n = i - 16;
            let c = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(c(n / 36), c((n / 6) % 6), c(n % 6))
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            Color32::from_rgb(v, v, v)
        }
    }
}

impl TerminalTab {
    pub fn new(conn: Uuid) -> Self {
        Self { conn, term: None, job: None, parser: vt100::Parser::new(nz(24), nz(80), SCROLLBACK), size: (24, 80), closed: None, error: None, scroll: 0, focused_once: false }
    }

    fn start(&mut self, cx: &AppCtx) {
        let Some(s) = cx.conns.session(self.conn).and_then(|h| h.resource()).cloned() else {
            return;
        };
        let (rows, cols) = self.size;
        self.closed = None;
        self.error = None;
        self.parser = vt100::Parser::new(nz(rows), nz(cols), SCROLLBACK);
        self.job = Some(cx.bg.spawn("Opening terminal", async move { s.open_terminal(cols, rows).await }));
    }

    fn send(&self, bytes: impl Into<Vec<u8>>) {
        if let Some(t) = &self.term {
            let _ = t.input.send(TermInput::Data(bytes.into()));
        }
    }

    /// Translate egui input into terminal bytes.
    fn keyboard(&mut self, ui: &egui::Ui) {
        let app_cursor = self.parser.screen().application_cursor();
        let bracketed = self.parser.screen().bracketed_paste();
        let events = ui.input(|i| i.events.clone());
        for e in events {
            match e {
                egui::Event::Text(t) => self.send(t.into_bytes()),
                egui::Event::Paste(t) => {
                    if bracketed {
                        self.send([b"\x1b[200~".as_slice(), t.as_bytes(), b"\x1b[201~"].concat());
                    } else {
                        self.send(t.into_bytes());
                    }
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let arrow = |c: u8| if app_cursor { vec![0x1b, b'O', c] } else { vec![0x1b, b'[', c] };
                    let bytes: Option<Vec<u8>> = if modifiers.ctrl && !modifiers.alt {
                        // Ctrl+letter → control code (Ctrl+C = 0x03 …).
                        let name = key.name();
                        (name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic()).then(|| vec![name.as_bytes()[0].to_ascii_lowercase() & 0x1f])
                    } else {
                        match key {
                            Key::Enter => Some(vec![b'\r']),
                            Key::Backspace => Some(vec![0x7f]),
                            Key::Tab => Some(if modifiers.shift { b"\x1b[Z".to_vec() } else { vec![b'\t'] }),
                            Key::Escape => Some(vec![0x1b]),
                            Key::ArrowUp => Some(arrow(b'A')),
                            Key::ArrowDown => Some(arrow(b'B')),
                            Key::ArrowRight => Some(arrow(b'C')),
                            Key::ArrowLeft => Some(arrow(b'D')),
                            Key::Home => Some(b"\x1b[H".to_vec()),
                            Key::End => Some(b"\x1b[F".to_vec()),
                            Key::PageUp if modifiers.shift => {
                                self.scroll = (self.scroll + self.size.0 as usize).min(SCROLLBACK);
                                None
                            }
                            Key::PageDown if modifiers.shift => {
                                self.scroll = self.scroll.saturating_sub(self.size.0 as usize);
                                None
                            }
                            Key::PageUp => Some(b"\x1b[5~".to_vec()),
                            Key::PageDown => Some(b"\x1b[6~".to_vec()),
                            Key::Delete => Some(b"\x1b[3~".to_vec()),
                            Key::Insert => Some(b"\x1b[2~".to_vec()),
                            Key::F1 => Some(b"\x1bOP".to_vec()),
                            Key::F2 => Some(b"\x1bOQ".to_vec()),
                            Key::F3 => Some(b"\x1bOR".to_vec()),
                            Key::F4 => Some(b"\x1bOS".to_vec()),
                            Key::F5 => Some(b"\x1b[15~".to_vec()),
                            Key::F10 => Some(b"\x1b[21~".to_vec()),
                            _ => None,
                        }
                    };
                    if let Some(b) = bytes {
                        self.scroll = 0;
                        self.send(b);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut AppCtx) {
        let t = theme::tokens(ui);
        let dark = ui.visuals().dark_mode;
        if cx.conns.session(self.conn).is_none() {
            crate::views::unsupported(ui, "Not connected. Connect from the sidebar, then reopen the terminal.");
            return;
        }
        if self.term.is_none() && self.job.is_none() && self.closed.is_none() && self.error.is_none() {
            self.start(cx);
        }
        if let Some(r) = poll(&mut self.job) {
            match r {
                Ok(term) => self.term = Some(term),
                Err(e) => self.error = Some(e),
            }
        }
        // Drain output.
        let mut got = false;
        if let Some(term) = &self.term {
            while let Ok(ev) = term.output.try_recv() {
                match ev {
                    TermEvent::Data(d) => {
                        self.parser.process(&d);
                        got = true;
                    }
                    TermEvent::Closed(why) => self.closed = Some(why),
                }
            }
        }
        if self.closed.is_some() {
            self.term = None;
        }
        if got {
            self.scroll = 0;
        }

        ui.horizontal(|ui| {
            ui.label(RichText::new(String::from(ICON_TERMINAL)).color(t.accent));
            ui.label(RichText::new(format!("{} · {}×{}", cx.profile_name(self.conn), self.size.1, self.size.0)).strong());
            if self.job.is_some() {
                ui.spinner();
                ui.label(RichText::new("Opening shell…").color(t.muted_fg));
            }
            if let Some(why) = &self.closed {
                theme::badge(ui, why, t.warning);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if (self.closed.is_some() || self.error.is_some()) && theme::primary_button(ui, ico(ICON_REFRESH, "Restart shell")).clicked() {
                    self.start(cx);
                }
                if ui.button(ico(ICON_CONTENT_COPY, "Copy screen")).on_hover_text("Copy the visible screen as text").clicked() {
                    ui.ctx().copy_text(self.parser.screen().contents());
                    cx.toasts.ok("Copied terminal screen");
                }
                if self.term.is_some() && ui.button(ico(ICON_CONTENT_PASTE, "Paste")).clicked() {
                    // Paste arrives through the normal paste event; this button just focuses.
                    self.focused_once = false;
                }
                ui.label(RichText::new(theme::kbd("Shift+PgUp/PgDn scrolls")).small().color(t.muted_fg));
            });
        });
        if let Some(e) = &self.error {
            crate::views::error_box(ui, e);
            return;
        }

        // Size the grid to the available area.
        let font = FontId::monospace(FONT);
        let (cw, ch) = ui.fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let avail = ui.available_size();
        let cols = ((avail.x - 12.0) / cw).floor().clamp(20.0, 500.0) as u16;
        let rows = ((avail.y - 12.0) / ch).floor().clamp(5.0, 300.0) as u16;
        if (rows, cols) != self.size {
            self.size = (rows, cols);
            self.parser.screen_mut().set_size(nz(rows), nz(cols));
            if let Some(term) = &self.term {
                let _ = term.input.send(TermInput::Resize { cols, rows });
            }
        }

        let (rect, resp) = ui.allocate_exact_size(avail, Sense::click());
        let id = resp.id;
        if resp.clicked() || !self.focused_once {
            ui.memory_mut(|m| m.request_focus(id));
            self.focused_once = true;
        }
        let has_focus = ui.memory(|m| m.has_focus(id));
        if has_focus {
            // Keep Tab, arrows and Esc for the terminal instead of egui focus moves.
            ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
            self.keyboard(ui);
        }
        if resp.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy.abs() > 0.5 {
                let lines = (dy / ch).round() as i64;
                self.scroll = (self.scroll as i64 + lines).clamp(0, SCROLLBACK as i64) as usize;
            }
        }
        self.parser.screen_mut().set_scrollback(self.scroll);
        self.scroll = self.parser.screen().scrollback();

        let bg = if dark { Color32::from_rgb(0x0b, 0x12, 0x20) } else { Color32::from_rgb(0xf8, 0xfa, 0xfc) };
        let fg = if dark { Color32::from_rgb(0xe2, 0xe8, 0xf0) } else { Color32::from_rgb(0x0f, 0x17, 0x2a) };
        let painter = ui.painter_at(rect);
        painter.rect(rect, 6.0, bg, Stroke::new(1.0, if has_focus { t.ring } else { t.border }), egui::StrokeKind::Inside);
        let origin = rect.min + vec2(6.0, 6.0);
        let screen = self.parser.screen();
        let (srows, scols) = screen.size();
        let (srows, scols) = (srows.get(), scols.get());
        let color = |c: vt100::Color, default: Color32| match c {
            vt100::Color::Default => default,
            vt100::Color::Idx(i) => idx_color(i, dark),
            vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        };
        for r in 0..srows {
            let y = origin.y + r as f32 * ch;
            let mut job = egui::text::LayoutJob::default();
            for c in 0..scols {
                let Some(cell) = screen.cell(r, c) else { continue };
                if cell.is_wide_continuation() {
                    continue;
                }
                let (mut f, mut b) = (color(cell.fgcolor(), fg), color(cell.bgcolor(), Color32::TRANSPARENT));
                if cell.inverse() {
                    std::mem::swap(&mut f, &mut b);
                    if f == Color32::TRANSPARENT {
                        f = bg;
                    }
                    if b == Color32::TRANSPARENT {
                        b = fg;
                    }
                }
                if cell.dim() {
                    f = f.gamma_multiply(0.6);
                }
                if b != Color32::TRANSPARENT {
                    let w = if cell.is_wide() { 2.0 } else { 1.0 };
                    painter.rect_filled(egui::Rect::from_min_size(pos2(origin.x + c as f32 * cw, y), vec2(cw * w, ch)), 0.0, b);
                }
                let text = if cell.has_contents() { cell.contents().to_string() } else { " ".to_string() };
                let fmt = egui::TextFormat {
                    font_id: font.clone(),
                    color: if cell.bold() && matches!(cell.fgcolor(), vt100::Color::Default) { t.fg } else { f },
                    underline: if cell.underline() { Stroke::new(1.0, f) } else { Stroke::NONE },
                    italics: cell.italic(),
                    ..Default::default()
                };
                job.append(&text, 0.0, fmt);
            }
            let galley = ui.fonts_mut(|fo| fo.layout_job(job));
            painter.galley(pos2(origin.x, y), galley, fg);
        }
        // Cursor (only on the live screen).
        if self.scroll == 0 && !screen.hide_cursor() && self.term.is_some() {
            let (cr, cc) = screen.cursor_position();
            let cur = egui::Rect::from_min_size(pos2(origin.x + cc as f32 * cw, origin.y + cr as f32 * ch), vec2(cw, ch));
            if has_focus {
                painter.rect_filled(cur, 1.0, t.accent.gamma_multiply(0.7));
            } else {
                painter.rect_stroke(cur, 1.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
            }
        }
        if self.scroll > 0 {
            painter.text(rect.right_top() + vec2(-10.0, 8.0), egui::Align2::RIGHT_TOP, format!("scrollback −{}", self.scroll), FontId::proportional(11.0), t.warning);
        }
        if self.term.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
}
