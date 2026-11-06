//! Home: connection overview with health, quick actions and shortcuts.

use egui::RichText;
use egui_material_icons::icons::*;

use crate::app::{AppCtx, Cmd};
use crate::conns::ConnState;
use crate::theme::{self, ico};

pub fn ui(ui: &mut egui::Ui, cx: &mut AppCtx) {
    let t = theme::tokens(ui);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(theme::space::LG);
        ui.horizontal(|ui| {
            ui.add_space(theme::space::SM);
            ui.vertical(|ui| {
                ui.label(RichText::new("Welcome to OpenVClient").size(24.0).strong());
                ui.label(RichText::new("Databases, caches and containers in one fast workspace.").color(t.muted_fg));
            });
        });
        ui.add_space(theme::space::XL);

        // Connection overview cards.
        let mut profiles = cx.store.ws.profiles.clone();
        profiles.sort_by(|a, b| b.last_connected_at.cmp(&a.last_connected_at).then(a.name.cmp(&b.name)));
        section(ui, "CONNECTIONS", &format!("{} saved · {} connected", profiles.len(), cx.conns.connected_ids().len()));
        if profiles.is_empty() {
            card(ui, |ui| {
                ui.label(RichText::new("No connections yet").strong());
                ui.label(RichText::new("Add PostgreSQL, MySQL/MariaDB, SQLite, Redis or Docker. Passwords go to the system keychain.").color(t.muted_fg));
                ui.add_space(theme::space::SM);
                if theme::primary_button(ui, ico(ICON_ADD, "Add your first connection")).clicked() {
                    cx.cmd(Cmd::EditConnection(None));
                }
            });
        }
        // Fixed-size cards in an even grid that reflows with the window width.
        let (card_w, card_h, gap) = (290.0_f32, 112.0_f32, theme::space::MD);
        let cols = (((ui.available_width() + gap) / (card_w + gap)).floor() as usize).max(1);
        for row in profiles.iter().take(48).collect::<Vec<_>>().chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, p) in row.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(gap);
                    }
                    connection_card(ui, cx, p, card_w, card_h);
                }
            });
            ui.add_space(gap);
        }

        ui.add_space(theme::space::XL);
        section(ui, "CONNECTORS IN THIS BUILD", "");
        ui.horizontal_wrapped(|ui| {
            let infos: Vec<openvclient_core::DriverInfo> = cx.conns.registry.all().iter().map(|d| d.info().clone()).collect();
            for i in infos {
                let r = ui.add(egui::Button::new(ico(crate::views::driver_icon(i.id), i.name))).on_hover_text(i.description);
                if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    cx.cmd(Cmd::NewConnectionOf(i.id));
                }
            }
        });

        ui.add_space(theme::space::XL);
        section(ui, "KEYBOARD", "");
        card(ui, |ui| {
            egui::Grid::new("keys").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                for (k, what) in [
                    ("⌘K", "Search everything"),
                    ("⌘N", "New connection"),
                    ("⌘T", "New SQL editor for the focused connection"),
                    ("⌘⏎", "Run statement at cursor (or selection)"),
                    ("⌘⇧⏎", "Run whole script"),
                    ("⌘E", "Explain statement"),
                    ("⌘.", "Cancel running query"),
                    ("⌘S", "Save SQL to file"),
                    ("⌃Space", "Autocomplete"),
                    ("⌘W", "Close tab"),
                    ("F5", "Refresh the current view"),
                    ("⌘⇧H", "Home"),
                ] {
                    ui.label(RichText::new(theme::kbd(k)).monospace().strong());
                    ui.label(RichText::new(what).color(t.muted_fg));
                    ui.end_row();
                }
            });
        });
        ui.add_space(theme::space::SM);
        ui.label(RichText::new("Drag a tab to the edge of another to split the workspace.").small().color(t.muted_fg));
    });
}

fn connection_card(ui: &mut egui::Ui, cx: &mut AppCtx, p: &openvclient_core::ConnectionProfile, w: f32, h: f32) {
    let t = theme::tokens(ui);
    let state = cx.conns.state(p.id);
    let (dot, status) = match state {
        Some(ConnState::Connected(l)) => (t.accent, l.latency.map(|d| format!("Connected · {}", openvclient_core::elapsed_ms(d))).unwrap_or_else(|| "Connected".into())),
        Some(ConnState::Connecting { .. }) => (t.info, "Connecting…".into()),
        Some(ConnState::Failed(e)) => (t.destructive, e.message.clone()),
        None => (t.muted_fg, "Not connected · click to connect".into()),
    };
    let product = match state {
        Some(ConnState::Connected(l)) => l.handle.base().server_info().product,
        _ => crate::views::driver_label(cx, &p.driver),
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = resp.hovered();
    ui.painter().rect(rect, 10.0, if hovered { t.muted } else { t.card }, egui::Stroke::new(if hovered { 1.5 } else { 1.0 }, if hovered { t.ring } else { t.border }), egui::StrokeKind::Inside);
    let env = theme::env_color(ui, p.environment);
    // Environment stripe on the left edge (also stated in text by the badge).
    ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 10.0), egui::vec2(3.0, h - 20.0)), 2.0, env);
    let inner = rect.shrink2(egui::vec2(14.0, 12.0));
    // A child Ui draws inside the allocated card without claiming more layout space.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    {
        let ui = &mut child;
        ui.spacing_mut().item_spacing = egui::vec2(theme::space::SM, 4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(String::from(crate::views::driver_icon(&p.driver))).size(theme::icon::LG).color(env));
            // Name left-aligned and truncated; the badge keeps a fixed slot on the right.
            let name_w = (ui.available_width() - 56.0).max(40.0);
            ui.allocate_ui_with_layout(egui::vec2(name_w, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(&p.name).strong().size(14.5)).truncate());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::badge(ui, p.environment.short(), env);
            });
        });
        ui.add(egui::Label::new(RichText::new(p.summary()).small().color(t.muted_fg)).truncate());
        ui.add(egui::Label::new(RichText::new(product).small().color(t.muted_fg)).truncate());
        ui.horizontal(|ui| {
            theme::dot(ui, dot);
            ui.add(egui::Label::new(RichText::new(status).small()).truncate());
        });
    }
    let resp = resp.on_hover_text(format!("{} · {}", p.name, p.environment.label()));
    if resp.clicked() {
        cx.selected_conn = Some(p.id);
        match cx.conns.session(p.id) {
            Some(h) if h.sql().is_some() => cx.cmd(Cmd::NewSql { conn: p.id, database: None, text: String::new() }),
            Some(h) if h.kv().is_some() => cx.cmd(Cmd::OpenRedis { conn: p.id, db: p.param("db").parse().unwrap_or(0) }),
            Some(_) => cx.cmd(Cmd::OpenDocker { conn: p.id, section: crate::views::docker::Section::Containers }),
            None => cx.cmd(Cmd::Connect(p.id)),
        }
    }
}

fn section(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).small().strong().color(theme::muted(ui)));
        if !detail.is_empty() {
            ui.label(RichText::new(detail).small().color(theme::muted(ui)));
        }
    });
    ui.add_space(theme::space::XS);
}

fn card(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    let t = theme::tokens(ui);
    egui::Frame::new().fill(t.card).stroke(egui::Stroke::new(1.0, t.border)).corner_radius(10).inner_margin(14).show(ui, |ui| {
        ui.set_max_width(720.0);
        body(ui);
    });
}
