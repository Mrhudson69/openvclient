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

        // Connection overview cards; favourites are pinned in their own section first.
        let mut profiles = cx.store.ws.profiles.clone();
        profiles.sort_by(|a, b| b.last_connected_at.cmp(&a.last_connected_at).then(a.name.cmp(&b.name)));
        let (card_w, card_h, gap) = (290.0_f32, 112.0_f32, theme::space::MD);
        let favs: Vec<_> = cx.store.ws.favorites.iter().filter_map(|id| profiles.iter().find(|p| p.id == *id).cloned()).collect();
        if !favs.is_empty() {
            section(ui, "FAVOURITES", &format!("{} pinned", favs.len()));
            card_grid(ui, cx, &favs, card_w, card_h, gap);
            ui.add_space(theme::space::LG);
        }
        let all = profiles.len();
        profiles.retain(|p| !cx.store.is_favorite(p.id));
        let title = if favs.is_empty() { "CONNECTIONS" } else { "OTHER CONNECTIONS" };
        section(ui, title, &format!("{all} saved · {} connected", cx.conns.connected_ids().len()));
        if favs.is_empty() && !profiles.is_empty() {
            ui.label(RichText::new(format!("{}  Star a connection to pin it here.", String::from(ICON_STAR))).small().color(t.muted_fg));
            ui.add_space(theme::space::XS);
        }
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
        card_grid(ui, cx, &profiles, card_w, card_h, gap);

        ui.add_space(theme::space::XL);
        section(ui, "CONNECTORS IN THIS BUILD", "");
        ui.horizontal_wrapped(|ui| {
            let infos: Vec<openvclient_core::DriverInfo> = cx.conns.registry.all().iter().map(|d| d.info().clone()).collect();
            for i in infos {
                let r = if theme::is_cartoon() {
                    // Room on the left for the connector sticker, painted over the button.
                    let r = ui.add(egui::Button::new(format!("        {}", i.name))).on_hover_text(i.description);
                    theme::paint_connector_sticker(ui.painter(), egui::pos2(r.rect.left() + 17.0, r.rect.center().y), 18.0, i.id, ui.visuals().dark_mode);
                    r
                } else {
                    ui.add(egui::Button::new(ico(crate::views::driver_icon(i.id), i.name))).on_hover_text(i.description)
                };
                if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    cx.cmd(Cmd::NewConnectionOf(i.id));
                }
            }
        });

        ui.add_space(theme::space::SM);
        ui.horizontal(|ui| {
            ui.label(RichText::new(theme::kbd("Keyboard shortcuts: ⌘/ (also in Settings)")).small().color(t.muted_fg));
            if ui.small_button("Show").clicked() {
                cx.show_shortcuts = true;
            }
        });
    });
}

/// Fixed-size cards in an even grid that reflows with the window width.
fn card_grid(ui: &mut egui::Ui, cx: &mut AppCtx, profiles: &[openvclient_core::ConnectionProfile], card_w: f32, card_h: f32, gap: f32) {
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
    theme::paint_card(ui, rect, hovered);
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
            crate::views::driver_glyph(ui, &p.driver, theme::icon::LG, env);
            // Name left-aligned and truncated; the badge keeps a fixed slot on the right.
            let name_w = (ui.available_width() - 84.0).max(40.0);
            ui.allocate_ui_with_layout(egui::vec2(name_w, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(&p.name).strong().size(14.5)).truncate());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::badge(ui, p.environment.short(), env);
                star_button(ui, cx, p.id);
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
        if cx.conns.session(p.id).is_some() {
            crate::views::open_connection(cx, p);
        } else {
            cx.cmd(Cmd::Connect(p.id));
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

/// Star toggle: pins the connection to the Favourites section of Home.
pub fn star_button(ui: &mut egui::Ui, cx: &mut AppCtx, id: uuid::Uuid) {
    let fav = cx.store.is_favorite(id);
    let t = theme::tokens(ui);
    let (glyph, color, tip) = if fav { (ICON_STAR, t.warning, "Remove from favourites") } else { (ICON_STAR, t.muted_fg.gamma_multiply(0.4), "Add to favourites") };
    let r = ui.add(egui::Label::new(RichText::new(String::from(glyph)).size(theme::icon::LG).color(color)).sense(egui::Sense::click()).selectable(false));
    if r.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked() {
        cx.store.toggle_favorite(id);
        cx.mark_dirty();
    }
}
