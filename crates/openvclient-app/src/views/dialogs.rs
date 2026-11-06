//! Small forms (new folder, database, schema) and tracked admin actions.

use egui::RichText;
use openvclient_core::Result;
use openvclient_core::sql::{BackupRequest, CreateDatabaseSpec};
use uuid::Uuid;

use crate::app::{AppCtx, Cmd};
use crate::bg::{Job, poll};
use crate::theme;

pub enum Dialog {
    Folder {
        parent: String,
        name: String,
    },
    RenameFolder {
        from: String,
        name: String,
    },
    MoveToFolder {
        conn: Uuid,
        folder: String,
    },
    CreateDatabase {
        conn: Uuid,
        spec: CreateDatabaseSpec,
        job: Option<Job<Result<()>>>,
    },
    CreateSchema {
        conn: Uuid,
        database: Option<String>,
        name: String,
        job: Option<Job<Result<()>>>,
    },
    /// BACKUP/RESTORE to a path on the database server (SQL Server).
    ServerBackup {
        conn: Uuid,
        database: String,
        restore: bool,
        path: String,
    },
    /// A running admin job whose outcome becomes a toast.
    Tracked {
        job: Job<Result<String>>,
        done: String,
        conn: Option<Uuid>,
    },
}

impl Dialog {
    pub fn new_folder() -> Self {
        Dialog::Folder { parent: String::new(), name: String::new() }
    }
    pub fn new_folder_in(parent: &str) -> Self {
        Dialog::Folder { parent: parent.to_string(), name: String::new() }
    }
    pub fn create_database(conn: Uuid) -> Self {
        Dialog::CreateDatabase { conn, spec: CreateDatabaseSpec::default(), job: None }
    }
    pub fn server_backup(conn: Uuid, database: String, restore: bool) -> Self {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M");
        let path = if restore { String::new() } else { format!("/var/opt/mssql/backup/{database}-{stamp}.bak") };
        Dialog::ServerBackup { conn, database, restore, path }
    }
    pub fn create_schema(conn: Uuid, database: Option<String>) -> Self {
        Dialog::CreateSchema { conn, database, name: String::new(), job: None }
    }

    /// Returns false when finished.
    pub fn show(&mut self, ctx: &egui::Context, cx: &mut AppCtx) -> bool {
        match self {
            Dialog::Tracked { job, done, conn } => {
                if let Some(r) = job.take() {
                    match r {
                        Ok(msg) => cx.toasts.info(done.clone(), msg),
                        Err(e) => cx.toasts.error("Action failed", &e),
                    }
                    if let Some(c) = conn {
                        cx.cmd(Cmd::RefreshTree(*c));
                        cx.cmd(Cmd::InvalidateCatalog(*c));
                    }
                    return false;
                }
                !job.is_dead()
            }
            Dialog::ServerBackup { conn, database, restore, path } => {
                let (conn, restore) = (*conn, *restore);
                let Some(s) = cx.conns.session(conn).and_then(|h| h.sql()).cloned() else { return false };
                let mut keep = true;
                let mut go = false;
                modal(ctx, if restore { "Restore database" } else { "Back up database" }, |ui| {
                    ui.label(RichText::new(format!("{} {database}", if restore { "Restore into" } else { "Back up" })).strong());
                    ui.label(RichText::new("The path is on the database server, not this computer. The server's service account must be able to write it (default backup folder on Linux: /var/opt/mssql/backup or /var/opt/mssql/data).").small().color(theme::muted(ui)));
                    ui.add_space(theme::space::SM);
                    ui.add(egui::TextEdit::singleline(path).hint_text("/var/opt/mssql/backup/db.bak  or  D:\\Backups\\db.bak").desired_width(f32::INFINITY));
                    buttons(ui, if restore { "Restore…" } else { "Back up" }, &mut keep, |ok| {
                        go = ok && !path.trim().is_empty();
                        go
                    });
                });
                if go {
                    let (db, p) = (database.clone(), std::path::PathBuf::from(path.trim()));
                    if restore {
                        cx.confirm(
                            Some(conn),
                            format!("Restore {db}?"),
                            format!("Every session in {db} is disconnected and the database is replaced with the contents of {}.", p.display()),
                            "Restore",
                            Some(db.clone()),
                            move |cx| {
                                let job = cx.bg.spawn(format!("RESTORE {db}"), async move { s.restore(BackupRequest { database: Some(db), path: p }).await });
                                cx.dialog = Some(Dialog::Tracked { job, done: "Restore finished".into(), conn: Some(conn) });
                            },
                        );
                    } else {
                        let job = cx.bg.spawn(format!("BACKUP {db}"), async move { s.backup(BackupRequest { database: Some(db), path: p }).await });
                        cx.dialog = Some(Dialog::Tracked { job, done: "Backup finished".into(), conn: None });
                    }
                    // Closing; the Tracked dialog set above (if any) stays.
                    return false;
                }
                keep
            }
            Dialog::RenameFolder { from, name } => {
                let mut keep = true;
                let from_c = from.clone();
                modal(ctx, "Rename folder", |ui| {
                    ui.label(RichText::new(format!("Renaming {from_c}")).color(theme::muted(ui)));
                    let r = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                    r.request_focus();
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    buttons(ui, "Rename", &mut keep, |ok| {
                        let new_leaf = name.trim().trim_matches('/');
                        if (ok || enter) && !new_leaf.is_empty() && !new_leaf.contains('/') {
                            let parent = from_c.rsplit_once('/').map(|(p, _)| format!("{p}/")).unwrap_or_default();
                            let to = format!("{parent}{new_leaf}");
                            let remap = |f: &str| -> Option<String> { if f == from_c { Some(to.clone()) } else { f.strip_prefix(&format!("{from_c}/")).map(|rest| format!("{to}/{rest}")) } };
                            for f in cx.store.ws.folders.iter_mut() {
                                if let Some(n) = remap(f) {
                                    *f = n;
                                }
                            }
                            for p in cx.store.ws.profiles.iter_mut() {
                                if let Some(n) = remap(p.folder.trim_matches('/')) {
                                    p.folder = n;
                                }
                            }
                            cx.store.ws.folders.dedup();
                            cx.mark_dirty();
                            cx.toasts.ok(format!("Renamed to {to}"));
                            return true;
                        }
                        false
                    })
                });
                keep
            }
            Dialog::MoveToFolder { conn, folder } => {
                let conn = *conn;
                let mut keep = true;
                let folders = cx.store.all_folders();
                let name = cx.profile_name(conn);
                modal(ctx, "Move to folder", |ui| {
                    ui.label(RichText::new(format!("Move {name} to")).color(theme::muted(ui)));
                    egui::ComboBox::from_id_salt("move_folder").width(300.0).selected_text(if folder.is_empty() { "(top level)".to_string() } else { folder.clone() }).show_ui(ui, |ui| {
                        ui.selectable_value(folder, String::new(), "(top level)");
                        for f in &folders {
                            ui.selectable_value(folder, f.clone(), f);
                        }
                    });
                    ui.add(egui::TextEdit::singleline(folder).hint_text("or type a new folder path, e.g. Clients/ACME").desired_width(f32::INFINITY));
                    buttons(ui, "Move", &mut keep, |ok| {
                        if ok {
                            if let Some(p) = cx.store.profile_mut(conn) {
                                p.folder = folder.trim().trim_matches('/').to_string();
                            }
                            cx.mark_dirty();
                            return true;
                        }
                        false
                    })
                });
                keep
            }
            Dialog::Folder { parent, name } => {
                let mut keep = true;
                modal(ctx, "New folder", |ui| {
                    if !parent.is_empty() {
                        ui.label(RichText::new(format!("Inside {parent}")).color(theme::muted(ui)));
                    }
                    let r = ui.add(egui::TextEdit::singleline(name).hint_text("Folder name").desired_width(f32::INFINITY));
                    r.request_focus();
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    buttons(ui, "Create", &mut keep, |ok| {
                        if (ok || enter) && !name.trim().is_empty() {
                            let full = if parent.is_empty() { name.trim().to_string() } else { format!("{parent}/{}", name.trim()) };
                            if !cx.store.ws.folders.contains(&full) {
                                cx.store.ws.folders.push(full);
                                cx.mark_dirty();
                            }
                            return true;
                        }
                        false
                    })
                });
                keep
            }
            Dialog::CreateDatabase { conn, spec, job } => {
                let conn = *conn;
                if let Some(r) = poll(job) {
                    match r {
                        Ok(()) => {
                            cx.toasts.ok(format!("Database {} created", spec.name));
                            cx.cmd(Cmd::RefreshTree(conn));
                            return false;
                        }
                        Err(e) => cx.toasts.error("Could not create database", &e),
                    }
                }
                let Some(s) = cx.conns.session(conn).and_then(|h| h.sql()).cloned() else { return false };
                let pg = s.dialect() == openvclient_core::sqltext::Dialect::Postgres;
                let mut keep = true;
                let busy = job.is_some();
                modal(ctx, "New database", |ui| {
                    egui::Grid::new("newdb").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label("Name *");
                        ui.add(egui::TextEdit::singleline(&mut spec.name).desired_width(260.0)).request_focus();
                        ui.end_row();
                        let opt = |ui: &mut egui::Ui, label: &str, v: &mut Option<String>, hint: &str| {
                            ui.label(label);
                            let mut s = v.clone().unwrap_or_default();
                            ui.add(egui::TextEdit::singleline(&mut s).hint_text(hint).desired_width(260.0));
                            *v = (!s.is_empty()).then_some(s);
                            ui.end_row();
                        };
                        if pg {
                            opt(ui, "Owner", &mut spec.owner, "current user");
                            opt(ui, "Template", &mut spec.template, "template1");
                            opt(ui, "Encoding", &mut spec.encoding, "UTF8");
                            opt(ui, "Locale", &mut spec.collation, "server default");
                        } else {
                            opt(ui, "Character set", &mut spec.encoding, "utf8mb4");
                            opt(ui, "Collation", &mut spec.collation, "server default");
                        }
                    });
                    if busy {
                        crate::views::busy(ui, "Creating…");
                    }
                    buttons(ui, "Create", &mut keep, |ok| {
                        if ok && !busy && !spec.name.trim().is_empty() {
                            let sp = spec.clone();
                            let s = s.clone();
                            *job = Some(cx.bg.spawn(format!("CREATE DATABASE {}", sp.name), async move { s.create_database(sp).await }));
                        }
                        false
                    })
                });
                keep
            }
            Dialog::CreateSchema { conn, database, name, job } => {
                let conn = *conn;
                if let Some(r) = poll(job) {
                    match r {
                        Ok(()) => {
                            cx.toasts.ok(format!("Schema {name} created"));
                            cx.cmd(Cmd::RefreshTree(conn));
                            return false;
                        }
                        Err(e) => cx.toasts.error("Could not create schema", &e),
                    }
                }
                let Some(s) = cx.conns.session(conn).and_then(|h| h.sql()).cloned() else { return false };
                let mut keep = true;
                let busy = job.is_some();
                modal(ctx, "New schema", |ui| {
                    if let Some(d) = database {
                        ui.label(RichText::new(format!("In database {d}")).color(theme::muted(ui)));
                    }
                    ui.add(egui::TextEdit::singleline(name).hint_text("Schema name").desired_width(f32::INFINITY)).request_focus();
                    buttons(ui, "Create", &mut keep, |ok| {
                        if ok && !busy && !name.trim().is_empty() {
                            let (s, db, n) = (s.clone(), database.clone(), name.trim().to_string());
                            *job = Some(cx.bg.spawn(format!("CREATE SCHEMA {n}"), async move { s.create_schema(db.as_deref(), &n).await }));
                        }
                        false
                    })
                });
                keep
            }
        }
    }
}

fn modal(ctx: &egui::Context, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Modal::new(egui::Id::unique(("dialog", title))).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading(title);
        ui.add_space(theme::space::SM);
        body(ui);
    });
}

/// Primary + Cancel row. `on` gets `true` when primary was clicked and
/// returns whether the dialog is done.
fn buttons(ui: &mut egui::Ui, primary: &str, keep: &mut bool, on: impl FnOnce(bool) -> bool) {
    ui.add_space(theme::space::MD);
    let mut clicked = false;
    ui.horizontal(|ui| {
        clicked = theme::primary_button(ui, primary).clicked();
        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            *keep = false;
        }
    });
    if on(clicked) {
        *keep = false;
    }
}

/// Run an admin action in the background and report its outcome as a toast.
pub fn track<F>(cx: &mut AppCtx, label: String, done: &str, conn: Option<Uuid>, fut: F)
where
    F: std::future::Future<Output = Result<()>> + Send + 'static,
{
    let job = cx.bg.spawn(label, async move { fut.await.map(|_| String::new()) });
    cx.dialog = Some(Dialog::Tracked { job, done: done.to_string(), conn });
}

pub fn start_backup(cx: &mut AppCtx, conn: Uuid, database: Option<String>, label: &str) {
    let Some(s) = cx.conns.session(conn).and_then(|h| h.sql()).cloned() else { return };
    let ext = match s.dialect() {
        openvclient_core::sqltext::Dialect::Postgres => "dump",
        openvclient_core::sqltext::Dialect::MySql => "sql",
        openvclient_core::sqltext::Dialect::Sqlite => "db",
        openvclient_core::sqltext::Dialect::SqlServer => "bak",
    };
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M");
    let Some(path) = rfd::FileDialog::new().set_file_name(format!("{label}-{stamp}.{ext}")).save_file() else { return };
    let job = cx.bg.spawn(format!("Backing up {label}"), async move { s.backup(BackupRequest { database, path }).await });
    cx.dialog = Some(Dialog::Tracked { job, done: "Backup finished".into(), conn: None });
}

pub fn start_restore(cx: &mut AppCtx, conn: Uuid, database: Option<String>, label: &str) {
    let Some(s) = cx.conns.session(conn).and_then(|h| h.sql()).cloned() else { return };
    let Some(path) = rfd::FileDialog::new().pick_file() else { return };
    let file = path.display().to_string();
    let label = label.to_string();
    cx.confirm(
        Some(conn),
        format!("Restore into {label}?"),
        format!("Objects and rows from {file} are written into {label}. Existing data may be overwritten."),
        "Restore",
        Some(label.clone()),
        move |cx| {
            let job = cx.bg.spawn(format!("Restoring {label}"), async move { s.restore(BackupRequest { database, path }).await });
            cx.dialog = Some(Dialog::Tracked { job, done: "Restore finished".into(), conn: Some(conn) });
        },
    );
}
