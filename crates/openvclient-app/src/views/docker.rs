//! Docker: containers (actions, logs, stats, exec), images, volumes, networks.

use std::time::{Duration, Instant};

use egui::{Key, RichText};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::*;
use openvclient_core::Result;
use openvclient_core::containers::*;
use uuid::Uuid;

use crate::app::AppCtx;
use crate::bg::{Job, poll};
use crate::theme::{self, ico};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Containers,
    Images,
    Volumes,
    Networks,
    System,
}

impl Section {
    pub const ALL: [Section; 5] = [Section::Containers, Section::Images, Section::Volumes, Section::Networks, Section::System];
    pub fn label(self) -> &'static str {
        match self {
            Section::Containers => "Containers",
            Section::Images => "Images",
            Section::Volumes => "Volumes",
            Section::Networks => "Networks",
            Section::System => "System",
        }
    }
    pub fn icon(self) -> MaterialIcon {
        match self {
            Section::Containers => ICON_DEPLOYED_CODE,
            Section::Images => ICON_LAYERS,
            Section::Volumes => ICON_STORAGE,
            Section::Networks => ICON_LAN,
            Section::System => ICON_INFO,
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Detail {
    Logs,
    Stats,
    Inspect,
    Exec,
}

pub struct DockerTab {
    pub conn: Uuid,
    pub section: Section,
    show_all: bool,
    filter: String,
    containers: Option<Result<Vec<ContainerInfo>>>,
    containers_job: Option<Job<Result<Vec<ContainerInfo>>>>,
    images: Option<Result<Vec<ImageInfo>>>,
    images_job: Option<Job<Result<Vec<ImageInfo>>>>,
    volumes: Option<Result<Vec<VolumeInfo>>>,
    volumes_job: Option<Job<Result<Vec<VolumeInfo>>>>,
    networks: Option<Result<Vec<NetworkInfo>>>,
    networks_job: Option<Job<Result<Vec<NetworkInfo>>>>,
    system: Option<Result<Vec<(String, String)>>>,
    system_job: Option<Job<Result<Vec<(String, String)>>>>,
    selected: Option<String>,
    detail: Detail,
    logs: Option<Result<String>>,
    logs_job: Option<Job<Result<String>>>,
    tail: usize,
    follow: bool,
    timestamps: bool,
    stats: Option<Result<ContainerStats>>,
    stats_job: Option<Job<Result<ContainerStats>>>,
    inspect: Option<Result<String>>,
    inspect_job: Option<Job<Result<String>>>,
    exec_input: String,
    exec_out: Vec<(String, std::result::Result<(Option<i64>, String), String>)>,
    exec_job: Option<Job<Result<(Option<i64>, String)>>>,
    action: Option<Job<Result<String>>>,
    pull: String,
    last_poll: Instant,
}

impl DockerTab {
    pub fn new(conn: Uuid, section: Section) -> Self {
        Self {
            conn,
            section,
            show_all: true,
            filter: String::new(),
            containers: None,
            containers_job: None,
            images: None,
            images_job: None,
            volumes: None,
            volumes_job: None,
            networks: None,
            networks_job: None,
            system: None,
            system_job: None,
            selected: None,
            detail: Detail::Logs,
            logs: None,
            logs_job: None,
            tail: 500,
            follow: true,
            timestamps: false,
            stats: None,
            stats_job: None,
            inspect: None,
            inspect_job: None,
            exec_input: String::new(),
            exec_out: Vec::new(),
            exec_job: None,
            action: None,
            pull: String::new(),
            last_poll: Instant::now(),
        }
    }

    fn session(&self, cx: &AppCtx) -> Option<std::sync::Arc<dyn ContainerSession>> {
        cx.conns.session(self.conn).and_then(|h| h.containers()).cloned()
    }

    fn reload(&mut self) {
        self.containers = None;
        self.images = None;
        self.volumes = None;
        self.networks = None;
        self.system = None;
    }

    fn act<F>(&mut self, cx: &AppCtx, label: String, f: impl FnOnce(std::sync::Arc<dyn ContainerSession>) -> F)
    where
        F: std::future::Future<Output = Result<String>> + Send + 'static,
    {
        if let Some(s) = self.session(cx) {
            self.action = Some(cx.bg.spawn(label, f(s)));
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut AppCtx, tab_id: u64) {
        let t = theme::tokens(ui);
        let Some(s) = self.session(cx) else {
            crate::views::unsupported(ui, "Not connected");
            return;
        };
        macro_rules! drive {
            ($slot:ident, $job:ident, $label:expr, $fut:expr) => {
                if let Some(r) = poll(&mut self.$job) {
                    self.$slot = Some(r);
                }
                if self.$slot.is_none() && self.$job.is_none() {
                    let s = s.clone();
                    self.$job = Some(cx.bg.spawn($label, async move { $fut(s).await }));
                }
            };
        }
        if let Some(r) = poll(&mut self.action) {
            match r {
                Ok(m) => cx.toasts.ok(m),
                Err(e) => cx.toasts.error("Docker action failed", &e),
            }
            self.reload();
        }
        // Light polling keeps container state fresh while visible.
        if self.section == Section::Containers && self.last_poll.elapsed() > Duration::from_secs(5) && self.containers_job.is_none() {
            self.containers = None;
            self.last_poll = Instant::now();
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));

        ui.horizontal(|ui| {
            crate::views::header(ui, ICON_DEPLOYED_CODE, &cx.profile_name(self.conn), &s.server_info().product);
            ui.separator();
            for sec in Section::ALL {
                ui.selectable_value(&mut self.section, sec, ico(sec.icon(), sec.label()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(String::from(ICON_REFRESH)).on_hover_text("Refresh").clicked() {
                    self.reload();
                }
                if self.action.is_some() {
                    ui.add(egui::Spinner::new());
                }
            });
        });
        ui.separator();
        match self.section {
            Section::Containers => {
                let all = self.show_all;
                drive!(containers, containers_job, "", move |s: std::sync::Arc<dyn ContainerSession>| async move { s.containers(all).await });
                self.containers_ui(ui, cx, &t, tab_id);
            }
            Section::Images => {
                drive!(images, images_job, "Listing images", |s: std::sync::Arc<dyn ContainerSession>| async move { s.images().await });
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.pull).hint_text("image:tag, e.g. postgres:17").desired_width(260.0));
                    if ui.add_enabled(!self.pull.trim().is_empty(), egui::Button::new(ico(ICON_DOWNLOAD, "Pull"))).clicked() {
                        let r = self.pull.trim().to_string();
                        self.act(cx, format!("Pulling {r}"), move |s| async move { s.pull_image(&r).await });
                    }
                });
                match &self.images {
                    None => crate::views::busy(ui, "Loading images…"),
                    Some(Err(e)) => crate::views::error_box(ui, &e.clone()),
                    Some(Ok(list)) => {
                        let list = list.clone();
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            egui::Grid::new("images").striped(true).num_columns(5).spacing([16.0, 6.0]).show(ui, |ui| {
                                for h in ["Tags", "ID", "Size", "Created", ""] {
                                    ui.label(RichText::new(h).strong());
                                }
                                ui.end_row();
                                for i in list {
                                    let tags = if i.tags.is_empty() { "<none>".to_string() } else { i.tags.join(", ") };
                                    ui.label(RichText::new(&tags).monospace());
                                    ui.label(RichText::new(i.id.trim_start_matches("sha256:").chars().take(12).collect::<String>()).monospace().color(t.muted_fg));
                                    ui.label(human_bytes(i.size.max(0) as u64));
                                    ui.label(ago(i.created));
                                    if ui.small_button(RichText::new("Remove…").color(t.destructive)).clicked() {
                                        let (sess, id) = (s.clone(), i.id.clone());
                                        cx.confirm(Some(self.conn), format!("Remove image {tags}?"), "Fails if a container uses it.", "Remove", None, move |cx| {
                                            crate::views::dialogs::track(cx, "Removing image".into(), "Image removed", None, async move { sess.remove_image(&id, false).await });
                                        });
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    }
                }
            }
            Section::Volumes => {
                drive!(volumes, volumes_job, "Listing volumes", |s: std::sync::Arc<dyn ContainerSession>| async move { s.volumes().await });
                match &self.volumes {
                    None => crate::views::busy(ui, "Loading volumes…"),
                    Some(Err(e)) => crate::views::error_box(ui, &e.clone()),
                    Some(Ok(v)) if v.is_empty() => crate::views::unsupported(ui, "No volumes"),
                    Some(Ok(list)) => {
                        let list = list.clone();
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            egui::Grid::new("vols").striped(true).num_columns(4).spacing([16.0, 6.0]).show(ui, |ui| {
                                for h in ["Name", "Driver", "Mountpoint", ""] {
                                    ui.label(RichText::new(h).strong());
                                }
                                ui.end_row();
                                for v in list {
                                    ui.label(RichText::new(&v.name).monospace());
                                    ui.label(&v.driver);
                                    ui.label(RichText::new(&v.mountpoint).small().color(t.muted_fg));
                                    if ui.small_button(RichText::new("Remove…").color(t.destructive)).clicked() {
                                        let (sess, n) = (s.clone(), v.name.clone());
                                        cx.confirm(
                                            Some(self.conn),
                                            format!("Remove volume {n}?"),
                                            "Its data is deleted permanently. Fails while a container uses it.",
                                            "Remove",
                                            Some(n.clone()),
                                            move |cx| {
                                                crate::views::dialogs::track(cx, format!("Removing volume {n}"), "Volume removed", None, async move { sess.remove_volume(&n).await });
                                            },
                                        );
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    }
                }
            }
            Section::Networks => {
                drive!(networks, networks_job, "Listing networks", |s: std::sync::Arc<dyn ContainerSession>| async move { s.networks().await });
                match &self.networks {
                    None => crate::views::busy(ui, "Loading networks…"),
                    Some(Err(e)) => crate::views::error_box(ui, &e.clone()),
                    Some(Ok(list)) => {
                        egui::Grid::new("nets").striped(true).num_columns(4).spacing([16.0, 6.0]).show(ui, |ui| {
                            for h in ["Name", "Driver", "Scope", "ID"] {
                                ui.label(RichText::new(h).strong());
                            }
                            ui.end_row();
                            for n in list {
                                ui.label(RichText::new(&n.name).monospace());
                                ui.label(&n.driver);
                                ui.label(&n.scope);
                                ui.label(RichText::new(n.id.chars().take(12).collect::<String>()).monospace().color(t.muted_fg));
                                ui.end_row();
                            }
                        });
                    }
                }
            }
            Section::System => {
                drive!(system, system_job, "docker info", |s: std::sync::Arc<dyn ContainerSession>| async move { s.system_info().await });
                match &self.system {
                    None => crate::views::busy(ui, "Loading…"),
                    Some(Err(e)) => crate::views::error_box(ui, &e.clone()),
                    Some(Ok(kv)) => {
                        egui::Grid::new("sys").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                            for (k, v) in kv {
                                ui.label(RichText::new(k).color(t.muted_fg));
                                ui.label(RichText::new(v).monospace());
                                ui.end_row();
                            }
                        });
                    }
                }
            }
        }
    }

    fn containers_ui(&mut self, ui: &mut egui::Ui, cx: &mut AppCtx, t: &theme::Tokens, tab_id: u64) {
        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.show_all, "Include stopped").changed() {
                self.containers = None;
            }
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(ico(ICON_SEARCH, "Filter by name, image, project")).desired_width(240.0));
        });
        let list = match &self.containers {
            None if self.containers_job.is_some() && self.selected.is_none() => {
                crate::views::busy(ui, "Loading containers…");
                return;
            }
            Some(Err(e)) => {
                crate::views::error_box(ui, &e.clone());
                return;
            }
            Some(Ok(l)) => l.clone(),
            None => Vec::new(),
        };
        let f = self.filter.to_lowercase();
        let list: Vec<ContainerInfo> =
            list.into_iter().filter(|c| f.is_empty() || c.name.to_lowercase().contains(&f) || c.image.to_lowercase().contains(&f) || c.compose_project.as_deref().unwrap_or("").contains(&f)).collect();
        let avail = ui.available_height();
        egui::Panel::top(egui::Id::unique(("ctr_list", tab_id))).resizable(true).default_size((avail * 0.45).max(140.0)).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("ctrs").striped(true).num_columns(6).spacing([14.0, 6.0]).show(ui, |ui| {
                    for h in ["", "Name", "Image", "Status", "Ports", "Actions"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    let mut project: Option<String> = None;
                    for c in &list {
                        if c.compose_project != project {
                            project = c.compose_project.clone();
                            if let Some(p) = &project {
                                ui.label("");
                                ui.label(RichText::new(ico(ICON_HUB, p)).small().strong().color(t.muted_fg));
                                ui.end_row();
                            }
                        }
                        let color = match c.state.as_str() {
                            "running" => t.accent,
                            "paused" => t.warning,
                            "restarting" => t.info,
                            "dead" => t.destructive,
                            _ => t.muted_fg,
                        };
                        theme::dot(ui, color).on_hover_text(&c.state);
                        let sel = self.selected.as_deref() == Some(c.id.as_str());
                        if ui.selectable_label(sel, RichText::new(&c.name).monospace().strong()).clicked() {
                            self.selected = Some(c.id.clone());
                            self.logs = None;
                            self.stats = None;
                            self.inspect = None;
                            self.exec_out.clear();
                        }
                        ui.add(egui::Label::new(RichText::new(&c.image).small().color(t.muted_fg)).truncate());
                        ui.label(RichText::new(&c.status).small());
                        ui.label(RichText::new(&c.ports).small().monospace());
                        ui.horizontal(|ui| {
                            let running = c.state == "running";
                            let paused = c.state == "paused";
                            let act = |ui: &mut egui::Ui, icon: MaterialIcon, tip: &str, a: ContainerAction, me: &mut Self| {
                                if ui.small_button(String::from(icon)).on_hover_text(tip).clicked() {
                                    let (id, name) = (c.id.clone(), c.name.clone());
                                    me.act(cx, format!("{} {name}", a.label()), move |s| async move { s.container_action(&id, a).await.map(|_| format!("{} {name}", a.label())) });
                                }
                            };
                            if running {
                                act(ui, ICON_STOP, "Stop", ContainerAction::Stop, self);
                                act(ui, ICON_RESTART_ALT, "Restart", ContainerAction::Restart, self);
                                act(ui, ICON_PAUSE, "Pause", ContainerAction::Pause, self);
                            } else if paused {
                                act(ui, ICON_PLAY_ARROW, "Unpause", ContainerAction::Unpause, self);
                            } else {
                                act(ui, ICON_PLAY_ARROW, "Start", ContainerAction::Start, self);
                            }
                            if ui.small_button(RichText::new(String::from(ICON_DELETE)).color(t.destructive)).on_hover_text("Remove…").clicked()
                                && let Some(s) = self.session(cx)
                            {
                                let (id, name) = (c.id.clone(), c.name.clone());
                                let force = running;
                                cx.confirm(
                                    Some(self.conn),
                                    format!("Remove container {name}?"),
                                    if force { "It is running and will be killed first. Its writable layer is deleted." } else { "Its writable layer is deleted. Named volumes are kept." },
                                    "Remove",
                                    Some(name.clone()),
                                    move |cx| {
                                        crate::views::dialogs::track(cx, format!("Removing {name}"), "Container removed", None, async move {
                                            s.container_action(&id, ContainerAction::Remove { force }).await
                                        });
                                    },
                                );
                            }
                        });
                        ui.end_row();
                    }
                });
                if list.is_empty() {
                    crate::views::unsupported(ui, "No containers");
                }
            });
        });
        let Some(id) = self.selected.clone() else {
            ui.label(RichText::new("Select a container for logs, live stats, inspect and exec.").color(t.muted_fg));
            return;
        };
        let Some(s) = self.session(cx) else { return };
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.detail, Detail::Logs, ico(ICON_LIST, "Logs"));
            ui.selectable_value(&mut self.detail, Detail::Stats, ico(ICON_MONITORING, "Stats"));
            ui.selectable_value(&mut self.detail, Detail::Inspect, ico(ICON_DATA_OBJECT, "Inspect"));
            ui.selectable_value(&mut self.detail, Detail::Exec, ico(ICON_TERMINAL, "Exec"));
        });
        match self.detail {
            Detail::Logs => {
                if let Some(r) = poll(&mut self.logs_job) {
                    self.logs = Some(r);
                    self.last_poll = Instant::now();
                }
                let refresh_due = self.follow && self.logs_job.is_none() && self.last_poll.elapsed() > Duration::from_secs(2);
                if (self.logs.is_none() || refresh_due) && self.logs_job.is_none() {
                    let (s2, id2, tail, ts) = (s.clone(), id.clone(), self.tail, self.timestamps);
                    self.logs_job = Some(cx.bg.spawn("", async move { s2.logs(&id2, tail, ts).await }));
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.follow, "Follow");
                    if ui.checkbox(&mut self.timestamps, "Timestamps").changed() {
                        self.logs = None;
                    }
                    ui.label("Tail");
                    if ui.add(egui::DragValue::new(&mut self.tail).range(10..=100_000)).changed() {
                        self.logs = None;
                    }
                    if let Some(Ok(l)) = &self.logs
                        && ui.button(ico(ICON_CONTENT_COPY, "Copy")).clicked()
                    {
                        ui.ctx().copy_text(l.clone());
                    }
                });
                match &self.logs {
                    None => crate::views::busy(ui, "Loading logs…"),
                    Some(Err(e)) => crate::views::error_box(ui, &e.clone()),
                    Some(Ok(l)) => {
                        egui::ScrollArea::both().stick_to_bottom(self.follow).auto_shrink([false, false]).show(ui, |ui| {
                            ui.add(egui::Label::new(RichText::new(l).monospace().size(12.0)).extend());
                        });
                    }
                }
            }
            Detail::Stats => {
                if let Some(r) = poll(&mut self.stats_job) {
                    self.stats = Some(r);
                }
                if self.stats_job.is_none() && self.last_poll.elapsed() > Duration::from_secs(2) || self.stats.is_none() && self.stats_job.is_none() {
                    let (s2, id2) = (s.clone(), id.clone());
                    self.stats_job = Some(cx.bg.spawn("", async move { s2.stats(&id2).await }));
                    self.last_poll = Instant::now();
                }
                match &self.stats {
                    None => crate::views::busy(ui, "Sampling…"),
                    Some(Err(e)) => crate::views::error_box(ui, &e.clone()),
                    Some(Ok(st)) => {
                        ui.horizontal_wrapped(|ui| {
                            stat(ui, t, "CPU", &format!("{:.1} %", st.cpu_percent));
                            let mem = if st.memory_limit > 0 { format!("{} / {}", human_bytes(st.memory_usage), human_bytes(st.memory_limit)) } else { human_bytes(st.memory_usage) };
                            stat(ui, t, "Memory", &mem);
                            stat(ui, t, "Network ↓ / ↑", &format!("{} / {}", human_bytes(st.net_rx), human_bytes(st.net_tx)));
                            stat(ui, t, "Block read / write", &format!("{} / {}", human_bytes(st.block_read), human_bytes(st.block_write)));
                            stat(ui, t, "Processes", &st.pids.to_string());
                        });
                    }
                }
            }
            Detail::Inspect => {
                if let Some(r) = poll(&mut self.inspect_job) {
                    self.inspect = Some(r);
                }
                if self.inspect.is_none() && self.inspect_job.is_none() {
                    let (s2, id2) = (s.clone(), id.clone());
                    self.inspect_job = Some(cx.bg.spawn("", async move { s2.inspect(&id2).await }));
                }
                match &self.inspect {
                    None => crate::views::busy(ui, "Inspecting…"),
                    Some(Err(e)) => crate::views::error_box(ui, &e.clone()),
                    Some(Ok(j)) => {
                        let mut j = j.clone();
                        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                            ui.add(egui::TextEdit::multiline(&mut j).code_editor().interactive(false).desired_width(f32::INFINITY));
                        });
                    }
                }
            }
            Detail::Exec => {
                if let Some(r) = poll(&mut self.exec_job)
                    && let Some(last) = self.exec_out.last_mut()
                {
                    last.1 = r.map_err(|e| e.to_string());
                }
                ui.label(RichText::new("Runs a command inside the container (no shell unless you call one, e.g. sh -c '…').").small().color(t.muted_fg));
                let h = ui.available_height() - 40.0;
                egui::ScrollArea::vertical().max_height(h).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                    for (cmd, out) in &self.exec_out {
                        ui.label(RichText::new(format!("$ {cmd}")).monospace().color(t.info));
                        match out {
                            Ok((code, o)) => {
                                ui.label(RichText::new(o).monospace());
                                if let Some(c) = code {
                                    ui.label(RichText::new(format!("exit {c}")).small().color(if *c == 0 { t.muted_fg } else { t.destructive }));
                                }
                            }
                            Err(e) => {
                                ui.label(RichText::new(e).monospace().color(t.destructive));
                            }
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut self.exec_input).font(egui::TextStyle::Monospace).hint_text("e.g. ls -la /data").desired_width(ui.available_width() - 80.0));
                    let go = (r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) | ui.button("Run").clicked();
                    if go && !self.exec_input.trim().is_empty() && self.exec_job.is_none() {
                        let line = std::mem::take(&mut self.exec_input);
                        match openvclient_core::kv::split_command_line(&line) {
                            Ok(args) => {
                                let (s2, id2) = (s.clone(), id.clone());
                                self.exec_job = Some(cx.bg.spawn(format!("exec {line}"), async move { s2.exec(&id2, args).await }));
                                self.exec_out.push((line, Ok((None, "…".into()))));
                            }
                            Err(e) => self.exec_out.push((line, Err(e.to_string()))),
                        }
                        r.request_focus();
                    }
                });
            }
        }
    }
}

fn stat(ui: &mut egui::Ui, t: &theme::Tokens, label: &str, value: &str) {
    egui::Frame::new().fill(t.card).stroke(egui::Stroke::new(1.0, t.border)).corner_radius(8).inner_margin(10).show(ui, |ui| {
        ui.set_width(190.0);
        ui.label(RichText::new(label).small().color(t.muted_fg));
        ui.label(RichText::new(value).size(16.0).strong());
    });
}

fn ago(ts: i64) -> String {
    let secs = chrono::Utc::now().timestamp() - ts;
    match secs {
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} days ago", s / 86_400),
    }
}
