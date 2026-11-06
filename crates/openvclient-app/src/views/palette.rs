//! Global search (⌘K): connections, loaded objects, open tabs and commands.

use egui::{Key, RichText};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::*;
use openvclient_core::sql::ObjectKind;
use uuid::Uuid;

use crate::app::{AppCtx, Cmd};
use crate::theme;

#[derive(Default)]
pub struct Palette {
    open: bool,
    query: String,
    selected: usize,
    focus: bool,
}

impl Palette {
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus = true;
    }

    #[cfg(test)]
    pub fn set_query(&mut self, q: &str) {
        self.query = q.to_string();
    }
}

struct Entry {
    icon: MaterialIcon,
    label: String,
    detail: String,
    score: i32,
    action: Action,
}

enum Action {
    Cmd(Cmd),
    Connect(Uuid),
}

/// Subsequence match; consecutive and word-start hits score higher.
fn fuzzy(needle: &str, hay: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    let h: Vec<char> = hay.to_lowercase().chars().collect();
    let mut score = 0;
    let mut hi = 0;
    let mut last: Option<usize> = None;
    for nc in needle.to_lowercase().chars() {
        let pos = (hi..h.len()).find(|&i| h[i] == nc)?;
        score += 1;
        if last == Some(pos.wrapping_sub(1)) {
            score += 4;
        }
        if pos == 0 || !h[pos - 1].is_alphanumeric() {
            score += 3;
        }
        last = Some(pos);
        hi = pos + 1;
    }
    if hay.to_lowercase().contains(&needle.to_lowercase()) {
        score += 10;
    }
    Some(score - (h.len() as i32 / 12))
}

fn entries(cx: &AppCtx, tabs: &[(u64, String)], q: &str) -> Vec<Entry> {
    let mut v = Vec::new();
    let mut add = |icon, label: String, detail: String, action| {
        if let Some(s) = fuzzy(q, &format!("{label} {detail}")) {
            v.push(Entry { icon, label, detail, score: s, action });
        }
    };
    for p in &cx.store.ws.profiles {
        let connected = cx.conns.is_connected(p.id);
        let detail = format!("{} · {} · {}", p.environment.short(), p.summary(), if connected { "connected" } else { "connect" });
        let action = if connected { Action::Cmd(Cmd::NewSql { conn: p.id, database: None, text: String::new() }) } else { Action::Connect(p.id) };
        add(crate::views::driver_icon(&p.driver), p.name.clone(), detail, action);
    }
    for (conn, n) in cx.sidebar.loaded_objects() {
        let action = if n.kind.has_rows() {
            Cmd::OpenTable { conn, kind: n.kind, path: n.path.clone() }
        } else if matches!(n.kind, ObjectKind::Database | ObjectKind::Schema) {
            Cmd::NewSql { conn, database: n.path.database.clone(), text: String::new() }
        } else {
            Cmd::OpenObject { conn, kind: n.kind, path: n.path.clone() }
        };
        add(crate::views::kind_icon(n.kind), n.label.clone(), format!("{} · {} · {}", n.kind.label(), n.path.display(), cx.profile_name(conn)), Action::Cmd(action));
    }
    for (id, title) in tabs {
        add(ICON_TAB, title.clone(), "open tab".into(), Action::Cmd(Cmd::FocusTab(*id)));
    }
    add(ICON_ADD, "New connection".into(), theme::kbd("command · ⌘N"), Action::Cmd(Cmd::EditConnection(None)));
    add(ICON_DASHBOARD, "Home".into(), theme::kbd("command · ⌘⇧H"), Action::Cmd(Cmd::OpenHome));
    if let Some(c) = cx.selected_conn.filter(|c| cx.conns.is_connected(*c)) {
        add(ICON_MONITORING, format!("Monitor {}", cx.profile_name(c)), "command".into(), Action::Cmd(Cmd::OpenMonitor(c)));
    }
    v.sort_by_key(|x| std::cmp::Reverse(x.score));
    v.truncate(60);
    v
}

pub fn show(ctx: &egui::Context, cx: &mut AppCtx, tabs: &[(u64, String)]) {
    if !cx.palette.open {
        return;
    }
    let items = entries(cx, tabs, &cx.palette.query.clone());
    let (down, up, enter, esc) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::Escape),
        )
    });
    let p = &mut cx.palette;
    if down {
        p.selected = (p.selected + 1).min(items.len().saturating_sub(1));
    }
    if up {
        p.selected = p.selected.saturating_sub(1);
    }
    let mut chosen = enter.then_some(p.selected);
    let mut close = esc;
    let resp = egui::Modal::new(egui::Id::unique("palette")).show(ctx, |ui| {
        ui.set_width(620.0);
        let t = theme::tokens(ui);
        let r =
            ui.add(egui::TextEdit::singleline(&mut cx.palette.query).hint_text("Search connections, tables, open tabs, commands…").font(egui::FontId::proportional(16.0)).desired_width(f32::INFINITY));
        if cx.palette.focus {
            r.request_focus();
            cx.palette.focus = false;
        }
        if r.changed() {
            cx.palette.selected = 0;
        }
        ui.add_space(theme::space::SM);
        if items.is_empty() {
            ui.label(RichText::new("No matches. Objects appear here once their folder has been opened in the sidebar.").color(t.muted_fg));
        }
        let row_h = 30.0;
        let moved = down || up;
        let sel = cx.palette.selected;
        egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show_rows(ui, row_h, items.len(), |ui, range| {
            for i in range {
                let e = &items[i];
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click());
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                let selected = i == sel;
                if selected {
                    ui.painter().rect_filled(rect, 6.0, t.selection);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 6.0, t.muted);
                }
                let p = ui.painter().with_clip_rect(rect.shrink(2.0));
                let cy = rect.center().y;
                p.text(egui::pos2(rect.left() + 18.0, cy), egui::Align2::CENTER_CENTER, String::from(e.icon), egui::FontId::proportional(theme::icon::MD), t.muted_fg);
                let label = p.layout_no_wrap(e.label.clone(), egui::FontId::proportional(14.0), t.fg);
                let lw = label.size().x.min(rect.width() * 0.55);
                p.with_clip_rect(egui::Rect::from_min_max(egui::pos2(rect.left() + 34.0, rect.top()), egui::pos2(rect.left() + 34.0 + lw, rect.bottom()))).galley(
                    egui::pos2(rect.left() + 34.0, cy - label.size().y / 2.0),
                    label,
                    t.fg,
                );
                let dx = rect.left() + 34.0 + lw + 12.0;
                let detail_clip = egui::Rect::from_min_max(egui::pos2(dx, rect.top()), egui::pos2(rect.right() - 8.0, rect.bottom()));
                p.with_clip_rect(detail_clip).text(egui::pos2(dx, cy), egui::Align2::LEFT_CENTER, &e.detail, egui::FontId::proportional(12.0), t.muted_fg);
                if selected && moved {
                    resp.scroll_to_me(None);
                }
                if resp.clicked() {
                    chosen = Some(i);
                }
            }
        });
        ui.add_space(theme::space::XS);
        ui.label(RichText::new("↑↓ to move · Enter to open · Esc to close").small().color(t.muted_fg));
    });
    if resp.should_close() {
        close = true;
    }
    if let Some(i) = chosen {
        if let Some(e) = items.into_iter().nth(i) {
            match e.action {
                Action::Cmd(c) => cx.cmd(c),
                Action::Connect(id) => {
                    cx.selected_conn = Some(id);
                    cx.cmd(Cmd::Connect(id));
                }
            }
        }
        close = true;
    }
    if close {
        cx.palette.open = false;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fuzzy_prefers_contiguous() {
        let a = super::fuzzy("ord", "orders").unwrap();
        let b = super::fuzzy("ord", "o_r_d_x").unwrap();
        assert!(a > b);
        assert!(super::fuzzy("xyz", "orders").is_none());
    }
}
