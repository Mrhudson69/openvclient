//! New/edit connection: pick a connector, fill its own fields, test, save.

use std::time::Instant;

use egui::RichText;
use egui_material_icons::icons::*;
use openvclient_core::{ConnectionProfile, Environment, Error, FieldKind, Result};

use crate::app::{AppCtx, Cmd};
use crate::bg::{Job, poll};
use crate::store::secrets;
use crate::theme::{self, ico};

pub struct ConnDialog {
    p: ConnectionProfile,
    is_new: bool,
    picking: bool,
    secret: String,
    /// Existing profile already has a keychain secret; empty field keeps it.
    has_saved_secret: bool,
    show_advanced: bool,
    test: Option<Job<Result<String>>>,
    test_result: Option<std::result::Result<String, Error>>,
    error: Option<String>,
    tab: FormTab,
    picker_filter: String,
    picker_focus: bool,
    /// Network-path secrets typed in this dialog; empty keeps the saved one.
    ssh_password: String,
    ssh_passphrase: String,
    proxy_password: String,
}

#[derive(Clone, Copy, PartialEq)]
enum FormTab {
    Main,
    Network,
}

impl ConnDialog {
    pub fn new(cx: &AppCtx, p: Option<ConnectionProfile>, folder: Option<String>) -> Self {
        match p {
            Some(p) => Self::edit(p),
            None => {
                let mut p = ConnectionProfile::new("", "");
                p.folder = folder.unwrap_or_default();
                let _ = cx;
                Self {
                    p,
                    is_new: true,
                    picking: true,
                    secret: String::new(),
                    has_saved_secret: false,
                    show_advanced: false,
                    test: None,
                    test_result: None,
                    error: None,
                    tab: FormTab::Main,
                    picker_filter: String::new(),
                    picker_focus: true,
                    ssh_password: String::new(),
                    ssh_passphrase: String::new(),
                    proxy_password: String::new(),
                }
            }
        }
    }

    pub fn new_from(_cx: &AppCtx, p: ConnectionProfile) -> Self {
        let mut d = Self::edit(p);
        d.is_new = true;
        d.has_saved_secret = false;
        d
    }

    fn edit(p: ConnectionProfile) -> Self {
        let has = p.save_secret;
        Self {
            p,
            is_new: false,
            picking: false,
            secret: String::new(),
            has_saved_secret: has,
            show_advanced: false,
            test: None,
            test_result: None,
            error: None,
            tab: FormTab::Main,
            picker_filter: String::new(),
            picker_focus: true,
            ssh_password: String::new(),
            ssh_passphrase: String::new(),
            proxy_password: String::new(),
        }
    }

    /// Skip the picker and open the form for `driver`.
    pub fn preselect(&mut self, cx: &AppCtx, driver: &str) {
        self.pick(cx, driver);
    }

    fn pick(&mut self, cx: &AppCtx, driver: &str) {
        self.p.driver = driver.to_string();
        if let Some(d) = cx.conns.registry.get(driver) {
            for f in &d.info().fields {
                if !f.default.is_empty() && f.kind != FieldKind::Secret {
                    self.p.params.entry(f.key.to_string()).or_insert_with(|| f.default.to_string());
                }
            }
            if self.p.name.is_empty() {
                self.p.name = d.info().name.split(' ').next().unwrap_or("Connection").to_string();
            }
        }
        self.picking = false;
    }

    fn validate(&self, cx: &AppCtx) -> std::result::Result<(), String> {
        if self.p.name.trim().is_empty() {
            return Err("Give the connection a name".into());
        }
        let d = cx.conns.registry.get(&self.p.driver).ok_or("Unknown connector")?;
        for f in &d.info().fields {
            if f.required && f.kind != FieldKind::Secret && self.p.param(f.key).trim().is_empty() {
                return Err(format!("{} is required", f.label));
            }
            if f.kind == FieldKind::Number && !self.p.param(f.key).is_empty() && self.p.param(f.key).trim().parse::<f64>().is_err() {
                return Err(format!("{} must be a number", f.label));
            }
        }
        Ok(())
    }

    /// Returns false when the dialog should close.
    pub fn show(&mut self, ctx: &egui::Context, cx: &mut AppCtx) -> bool {
        let mut open = true;
        let mut close = false;
        if let Some(r) = poll(&mut self.test) {
            self.test_result = Some(r);
        }
        let title = if self.is_new { "New connection" } else { "Edit connection" };
        egui::Modal::new(egui::Id::unique("conn_dialog")).show(ctx, |ui| {
            // Large enough that most forms fit without scrolling, never wider than the window.
            let screen = ctx.content_rect();
            ui.set_width((if self.picking { 900.0_f32 } else { 820.0 }).min(screen.width() - 48.0));
            ui.horizontal(|ui| {
                ui.heading(title);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(String::from(ICON_CLOSE)).on_hover_text("Close (Esc)").clicked() {
                        open = false;
                    }
                });
            });
            ui.separator();
            // Header and close button stay put; the body scrolls when the window is short.
            let max_h = (screen.height() * 0.88 - 110.0).max(240.0);
            egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                if self.picking {
                    self.picker(ui, cx);
                } else {
                    close = self.form(ui, cx);
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                open = false;
            }
        });
        open && !close
    }

    fn picker(&mut self, ui: &mut egui::Ui, cx: &AppCtx) {
        let t = theme::tokens(ui);
        let r = ui.add(egui::TextEdit::singleline(&mut self.picker_filter).hint_text(ico(ICON_SEARCH, "Find a connector")).desired_width(f32::INFINITY));
        if self.picker_focus {
            r.request_focus();
            self.picker_focus = false;
        }
        ui.add_space(theme::space::SM);
        let f = self.picker_filter.to_lowercase();
        let infos: Vec<openvclient_core::DriverInfo> = cx
            .conns
            .registry
            .all()
            .iter()
            .map(|d| d.info().clone())
            .filter(|i| f.is_empty() || i.name.to_lowercase().contains(&f) || i.id.contains(&f) || i.description.to_lowercase().contains(&f))
            .collect();
        let mut chosen = None;
        // One flat list of compact tiles: icon + name; details in the tooltip.
        crate::widgets::cards::grid(ui, &infos, 176.0, 40.0, |ui, info, resp| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new(String::from(crate::views::driver_icon(info.id))).size(theme::icon::MD).color(t.accent));
                ui.add(egui::Label::new(RichText::new(info.name).strong()).truncate());
            });
            if resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(info.description).clicked() {
                chosen = Some(info.id);
            }
        });
        if infos.is_empty() {
            ui.label(RichText::new("No connector matches").color(t.muted_fg));
        }
        if let Some(id) = chosen {
            self.pick(cx, id);
        }
    }

    /// Returns true when saved.
    fn form(&mut self, ui: &mut egui::Ui, cx: &mut AppCtx) -> bool {
        let t = theme::tokens(ui);
        let Some(driver) = cx.conns.registry.get(&self.p.driver) else {
            ui.label("Unknown connector");
            return false;
        };
        let info = driver.info().clone();
        ui.horizontal(|ui| {
            ui.label(RichText::new(String::from(crate::views::driver_icon(info.id))).color(t.accent).size(18.0));
            ui.label(RichText::new(info.name).strong());
            if self.is_new && ui.small_button("Change").clicked() {
                self.picking = true;
                self.p.params.clear();
            }
        });
        let networked = info.fields.iter().any(|f| f.key == "host");
        if networked {
            ui.add_space(theme::space::XS);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, FormTab::Main, ico(ICON_SETTINGS, "Main"));
                let mode = self.p.param(openvclient_net::keys::MODE).to_string();
                let label = match mode.as_str() {
                    "ssh" => "Network · SSH tunnel",
                    "socks5" => "Network · SOCKS5",
                    "http" => "Network · HTTP proxy",
                    _ => "Network",
                };
                ui.selectable_value(&mut self.tab, FormTab::Network, ico(ICON_LAN, label));
            });
            ui.separator();
        } else {
            self.tab = FormTab::Main;
        }
        ui.add_space(theme::space::SM);
        if self.tab == FormTab::Network {
            self.network_ui(ui);
            return self.footer(ui, cx, &driver);
        }
        let folders = cx.store.all_folders();
        egui::Grid::new("conn_form").num_columns(2).spacing([14.0, 10.0]).min_col_width(130.0).show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut self.p.name).desired_width(f32::INFINITY));
            ui.end_row();
            ui.label("Environment");
            ui.horizontal(|ui| {
                for e in Environment::ALL {
                    let c = theme::env_color(ui, e);
                    let sel = self.p.environment == e;
                    let txt = RichText::new(e.label()).color(if sel { c } else { t.fg });
                    if ui.selectable_label(sel, txt).clicked() {
                        self.p.environment = e;
                    }
                }
            });
            ui.end_row();
            ui.label("Folder");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.p.folder).hint_text("e.g. Customers/ACME").desired_width(240.0));
                egui::ComboBox::from_id_salt("folder_pick").selected_text("Pick").width(80.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.p.folder, String::new(), "(root)");
                    for f in &folders {
                        ui.selectable_value(&mut self.p.folder, f.clone(), f);
                    }
                });
            });
            ui.end_row();
            for f in info.fields.iter().filter(|f| !f.advanced) {
                self.field(ui, f);
            }
            if info.fields.iter().any(|f| f.kind == FieldKind::Secret) {
                ui.label("");
                ui.checkbox(&mut self.p.save_secret, "Save password in the system keychain");
                ui.end_row();
            }
            ui.label("");
            ui.checkbox(&mut self.p.read_only, "Read-only (enforced by the server where supported)");
            ui.end_row();
        });
        let adv: Vec<_> = info.fields.iter().filter(|f| f.advanced).cloned().collect();
        if !adv.is_empty() {
            ui.add_space(theme::space::XS);
            let label = if self.show_advanced { ico(ICON_EXPAND_MORE, "Advanced") } else { ico(ICON_CHEVRON_RIGHT, "Advanced") };
            if ui.selectable_label(false, label).clicked() {
                self.show_advanced = !self.show_advanced;
            }
            if self.show_advanced {
                egui::Grid::new("conn_adv").num_columns(2).spacing([14.0, 10.0]).min_col_width(130.0).show(ui, |ui| {
                    for f in &adv {
                        self.field(ui, f);
                    }
                });
            }
        }
        if self.p.environment.is_protected() {
            ui.add_space(theme::space::SM);
            ui.label(RichText::new(ico(ICON_WARNING, "Production: destructive actions ask you to type the object's name.")).color(t.destructive).small());
        }
        self.footer(ui, cx, &driver)
    }

    /// Errors, test result and the Test / Save row. Returns true when saved.
    fn footer(&mut self, ui: &mut egui::Ui, cx: &mut AppCtx, driver: &std::sync::Arc<dyn openvclient_core::Driver>) -> bool {
        let t = theme::tokens(ui);
        ui.add_space(theme::space::SM);
        if let Some(e) = &self.error {
            ui.label(RichText::new(e).color(t.destructive));
        }
        match &self.test_result {
            Some(Ok(m)) => {
                ui.label(RichText::new(ico(ICON_CHECK, m)).color(t.accent));
            }
            Some(Err(e)) => {
                crate::views::error_box(ui, e);
                if let Some(fp) = openvclient_net::unknown_host_key(e) {
                    let param = if e.message.contains("(jump host)") { "jump_fingerprint" } else { openvclient_net::keys::SSH_FINGERPRINT };
                    let t = theme::tokens(ui);
                    egui::Frame::new().fill(t.muted).corner_radius(6.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                        ui.label(RichText::new("Host key fingerprint (SHA-256) — compare it with the one from the server's administrator").small().color(t.muted_fg));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&fp).monospace().strong());
                            if ui.button(ico(ICON_VERIFIED_USER, "Trust this key")).clicked() {
                                self.p.params.insert(param.to_string(), fp.clone());
                                self.test_result = None;
                            }
                        });
                    });
                }
            }
            None => {}
        }
        ui.add_space(theme::space::SM);
        let mut saved = false;
        ui.horizontal(|ui| {
            let testing = self.test.is_some();
            if ui.add_enabled(!testing, egui::Button::new(ico(ICON_BOLT, "Test connection"))).clicked() {
                match self.validate(cx) {
                    Ok(()) => {
                        self.error = None;
                        self.test_result = None;
                        let p = self.p.clone();
                        let typed = (!self.secret.is_empty()).then(|| self.secret.clone());
                        let use_saved = self.has_saved_secret && typed.is_none();
                        let d = driver.clone();
                        let typed_net = (self.ssh_password.clone(), self.ssh_passphrase.clone(), self.proxy_password.clone());
                        self.test = Some(cx.bg.spawn(format!("Testing {}", p.name), async move {
                            let id = p.id;
                            let secret = if use_saved { tokio::task::spawn_blocking(move || secrets::get(id)).await.map_err(Error::from)?.map_err(Error::other)? } else { typed };
                            let mut net = tokio::task::spawn_blocking(move || crate::conns::net_secrets(id)).await.map_err(Error::from)?;
                            let pick = |typed: String, saved: Option<String>| if typed.is_empty() { saved } else { Some(typed) };
                            net.ssh_password = pick(typed_net.0, net.ssh_password);
                            net.ssh_passphrase = pick(typed_net.1, net.ssh_passphrase);
                            net.proxy_password = pick(typed_net.2, net.proxy_password);
                            let t0 = Instant::now();
                            let (h, tunnel) = crate::conns::open_session(d, p, secret, net, None).await?;
                            let via = tunnel.as_ref().map(|t| format!(" via {}", t.description)).unwrap_or_default();
                            let lat = h.base().ping().await?;
                            let info = h.base().server_info();
                            h.base().close().await;
                            Ok(format!("Connected to {}{via} in {} (ping {})", info.product, openvclient_core::elapsed_ms(t0.elapsed()), openvclient_core::elapsed_ms(lat)))
                        }));
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            if testing {
                ui.add(egui::Spinner::new());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.is_new { "Save & connect" } else { "Save" };
                if theme::primary_button(ui, label).clicked() {
                    match self.validate(cx) {
                        Ok(()) => {
                            let id = self.p.id;
                            let mut ok = true;
                            for (name, v) in [
                                (openvclient_net::keys::SECRET_SSH_PASSWORD, &self.ssh_password),
                                (openvclient_net::keys::SECRET_SSH_PASSPHRASE, &self.ssh_passphrase),
                                (openvclient_net::keys::SECRET_PROXY_PASSWORD, &self.proxy_password),
                            ] {
                                if !v.is_empty()
                                    && let Err(e) = secrets::set_named(id, name, v)
                                {
                                    self.error = Some(e);
                                    ok = false;
                                }
                            }
                            if self.p.save_secret && !self.secret.is_empty() {
                                if let Err(e) = secrets::set(id, &self.secret) {
                                    self.error = Some(format!("{e}. Turn off “Save password” to type it at connect time instead."));
                                    ok = false;
                                }
                            } else if !self.p.save_secret {
                                let _ = secrets::delete_main(id);
                                if !self.secret.is_empty() {
                                    cx.conns.remember_secret(id, self.secret.clone());
                                }
                            }
                            if ok {
                                self.p.folder = self.p.folder.trim_matches('/').to_string();
                                let was_connected = cx.conns.is_connected(id);
                                cx.store.upsert(self.p.clone());
                                cx.mark_dirty();
                                cx.toasts.ok(format!("Saved {}", self.p.name));
                                if self.is_new || was_connected {
                                    if was_connected {
                                        cx.cmd(Cmd::Disconnect(id));
                                    }
                                    cx.cmd(Cmd::Connect(id));
                                }
                                cx.selected_conn = Some(id);
                                saved = true;
                            }
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            });
        });
        saved
    }

    fn network_ui(&mut self, ui: &mut egui::Ui) {
        use openvclient_net::keys::*;
        let t = theme::tokens(ui);
        let mode = self.p.params.entry(MODE.into()).or_insert_with(|| "direct".into()).clone();
        ui.horizontal(|ui| {
            ui.label("Connect through");
            for (m, l) in [("direct", "Direct"), ("ssh", "SSH tunnel"), ("socks5", "SOCKS5 proxy"), ("http", "HTTP proxy")] {
                if ui.selectable_label(mode == m, l).clicked() {
                    self.p.params.insert(MODE.into(), m.into());
                }
            }
        });
        ui.add_space(theme::space::SM);
        let text = |ui: &mut egui::Ui, p: &mut openvclient_core::ConnectionProfile, key: &str, hint: &str, w: f32| {
            let v = p.params.entry(key.into()).or_default();
            ui.add(egui::TextEdit::singleline(v).hint_text(hint).desired_width(w));
        };
        let saved = |has: bool| if has { "•••••• saved (leave empty to keep)" } else { "" };
        match mode.as_str() {
            "ssh" => {
                egui::Grid::new("ssh_form").num_columns(2).spacing([14.0, 10.0]).min_col_width(130.0).show(ui, |ui| {
                    ui.label("SSH host *");
                    ui.horizontal(|ui| {
                        text(ui, &mut self.p, SSH_HOST, "bastion.example.com", 260.0);
                        ui.label("Port");
                        text(ui, &mut self.p, SSH_PORT, "22", 60.0);
                    });
                    ui.end_row();
                    ui.label("User *");
                    text(ui, &mut self.p, SSH_USER, "ubuntu", 260.0);
                    ui.end_row();
                    ui.label("Authentication");
                    let auth = self.p.params.entry(SSH_AUTH.into()).or_insert_with(|| "password".into()).clone();
                    ui.horizontal(|ui| {
                        for (a, l) in [("password", "Password"), ("key", "Private key"), ("agent", "SSH agent")] {
                            if ui.selectable_label(auth == a, l).clicked() {
                                self.p.params.insert(SSH_AUTH.into(), a.into());
                            }
                        }
                    });
                    ui.end_row();
                    match auth.as_str() {
                        "key" => {
                            ui.label("Private key *");
                            ui.horizontal(|ui| {
                                text(ui, &mut self.p, SSH_KEY, "~/.ssh/id_ed25519", 300.0);
                                if ui.button("Browse…").clicked()
                                    && let Some(f) = rfd::FileDialog::new().pick_file()
                                {
                                    self.p.params.insert(SSH_KEY.into(), f.display().to_string());
                                }
                            });
                            ui.end_row();
                            ui.label("Passphrase");
                            ui.add(egui::TextEdit::singleline(&mut self.ssh_passphrase).password(true).hint_text(saved(!self.is_new)).desired_width(260.0));
                            ui.end_row();
                        }
                        "agent" => {
                            ui.label("");
                            let agent_hint = if cfg!(windows) {
                                "Uses the keys loaded in the Windows OpenSSH agent (ssh-agent service), or in Pageant."
                            } else {
                                "Uses the keys loaded in your SSH agent (SSH_AUTH_SOCK)."
                            };
                            ui.label(RichText::new(agent_hint).small().color(t.muted_fg));
                            ui.end_row();
                        }
                        _ => {
                            ui.label("Password");
                            ui.add(egui::TextEdit::singleline(&mut self.ssh_password).password(true).hint_text(saved(!self.is_new)).desired_width(260.0));
                            ui.end_row();
                        }
                    }
                    ui.label("Timeout (ms)");
                    text(ui, &mut self.p, SSH_TIMEOUT, "10000", 100.0);
                    ui.end_row();
                    ui.label("Host key");
                    let fp = self.p.param(SSH_FINGERPRINT).to_string();
                    ui.horizontal(|ui| {
                        if fp.is_empty() {
                            ui.label(RichText::new("Pinned on first successful connect").small().color(t.muted_fg));
                        } else {
                            ui.label(RichText::new(&fp).monospace().small());
                            if ui.small_button("Forget").on_hover_text("Trust the next key the server presents").clicked() {
                                self.p.params.remove(SSH_FINGERPRINT);
                            }
                        }
                    });
                    ui.end_row();
                });
                ui.add_space(theme::space::XS);
                ui.label(
                    RichText::new("The database host and port on the Main tab are resolved from the SSH server, so use addresses it can reach (e.g. localhost for a DB on the same machine).")
                        .small()
                        .color(t.muted_fg),
                );
            }
            "socks5" | "http" => {
                egui::Grid::new("proxy_form").num_columns(2).spacing([14.0, 10.0]).min_col_width(130.0).show(ui, |ui| {
                    ui.label("Proxy host *");
                    ui.horizontal(|ui| {
                        text(ui, &mut self.p, PROXY_HOST, "proxy.example.com", 260.0);
                        ui.label("Port");
                        text(ui, &mut self.p, PROXY_PORT, if mode == "socks5" { "1080" } else { "3128" }, 60.0);
                    });
                    ui.end_row();
                    ui.label("Username");
                    text(ui, &mut self.p, PROXY_USER, "optional", 260.0);
                    ui.end_row();
                    ui.label("Password");
                    ui.add(egui::TextEdit::singleline(&mut self.proxy_password).password(true).hint_text(saved(!self.is_new)).desired_width(260.0));
                    ui.end_row();
                });
                ui.add_space(theme::space::XS);
                let note = if mode == "http" { "Uses HTTP CONNECT; the proxy must allow CONNECT to the database port." } else { "SOCKS5 with optional username/password (RFC 1929)." };
                ui.label(RichText::new(note).small().color(t.muted_fg));
            }
            _ => {
                ui.label(RichText::new("Connects straight to the host and port on the Main tab.").color(t.muted_fg));
            }
        }
    }

    fn field(&mut self, ui: &mut egui::Ui, f: &openvclient_core::FieldSpec) {
        if let Some((key, values)) = f.show_if {
            let current = self.p.params.get(key).map(String::as_str).unwrap_or("");
            if !values.contains(&current) {
                return;
            }
        }
        let label = if f.required { format!("{} *", f.label) } else { f.label.to_string() };
        let resp = ui.label(label);
        if !f.help.is_empty() {
            resp.on_hover_text(f.help);
        }
        let hint = if f.placeholder.is_empty() { f.default } else { f.placeholder };
        match &f.kind {
            FieldKind::Secret => {
                let hint = if self.has_saved_secret { "•••••• saved in keychain (leave empty to keep)" } else { "" };
                ui.add(egui::TextEdit::singleline(&mut self.secret).password(true).hint_text(hint).desired_width(f32::INFINITY));
            }
            FieldKind::Bool => {
                let v = self.p.params.entry(f.key.into()).or_insert_with(|| f.default.into());
                let mut b = v == "true";
                if ui.checkbox(&mut b, "").changed() {
                    *v = b.to_string();
                }
            }
            FieldKind::Choice(opts) => {
                let v = self.p.params.entry(f.key.into()).or_insert_with(|| f.default.into());
                egui::ComboBox::from_id_salt(f.key).selected_text(openvclient_core::choice_label(f.key, v)).show_ui(ui, |ui| {
                    for o in *opts {
                        ui.selectable_value(v, o.to_string(), openvclient_core::choice_label(f.key, o));
                    }
                });
            }
            FieldKind::File { save, filters } => {
                ui.horizontal(|ui| {
                    let v = self.p.params.entry(f.key.into()).or_default();
                    ui.add(egui::TextEdit::singleline(v).hint_text(hint).desired_width(330.0));
                    if ui.button("Browse…").clicked() {
                        let dlg = rfd::FileDialog::new().add_filter("Database", filters).add_filter("All files", &["*"]);
                        let picked = if *save { dlg.set_file_name("database.db").save_file() } else { dlg.pick_file() };
                        if let Some(path) = picked {
                            *v = path.display().to_string();
                        }
                    }
                });
            }
            FieldKind::Text | FieldKind::Number => {
                let v = self.p.params.entry(f.key.into()).or_default();
                ui.add(egui::TextEdit::singleline(v).hint_text(hint).desired_width(f32::INFINITY));
            }
        }
        ui.end_row();
    }
}
