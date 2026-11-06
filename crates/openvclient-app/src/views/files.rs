//! File-manager view for storage connections (Azure Blob, S3 and compatible
//! providers, FTP/FTPS, SFTP): breadcrumb navigation with back/forward/up,
//! a sortable file list with multi-select, upload (button or drag and drop),
//! download, new folder, rename, share link, delete, and a preview panel.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Key, RichText, Sense};
use egui_extras::{Column, TableBuilder};
use egui_material_icons::icons::*;
use openvclient_core::resource::{ActionOutcome, Args, Content, FileEntry, FileListing, ResourceSession};
use openvclient_core::{Error, Result};
use uuid::Uuid;

use crate::app::AppCtx;
use crate::bg::{Job, poll};
use crate::theme::{self, ico};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Name,
    Size,
    Modified,
    Kind,
}

/// A batch of actions run in the background (uploads, downloads, deletes).
struct Batch {
    label: String,
    job: Job<Vec<(String, Result<ActionOutcome>)>>,
}

enum Modal {
    NewFolder(String),
    Rename(FileEntry, String),
    Share(String),
}

pub struct FilesTab {
    pub conn: Uuid,
    /// Current folder; `None` = top level (buckets / containers).
    cwd: Option<String>,
    back: Vec<Option<String>>,
    forward: Vec<Option<String>>,
    listing: Option<Result<FileListing>>,
    job: Option<Job<Result<FileListing>>>,
    filter: String,
    applied_filter: String,
    sort: (SortBy, bool),
    selected: BTreeSet<String>,
    anchor: Option<usize>,
    preview: Option<(FileEntry, Option<Result<Content>>)>,
    preview_job: Option<Job<Result<Content>>>,
    batch: Option<Batch>,
    modal: Option<Modal>,
    delete_confirmed: Arc<AtomicBool>,
    pending_delete: Vec<FileEntry>,
    started: bool,
}

/// File type icon from the extension.
fn file_icon(name: &str) -> egui_material_icons::MaterialIcon {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "tif" | "tiff" | "heic" => ICON_IMAGE,
        "json" | "yaml" | "yml" | "toml" | "xml" => ICON_DATA_OBJECT,
        "csv" | "tsv" | "parquet" | "xlsx" | "xls" | "avro" | "orc" => ICON_TABLE_CHART,
        "zip" | "gz" | "tgz" | "tar" | "bz2" | "xz" | "7z" | "rar" | "zst" => ICON_FOLDER_ZIP,
        "pdf" => ICON_PICTURE_AS_PDF,
        "mp4" | "mov" | "mkv" | "avi" | "webm" => ICON_MOVIE,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => ICON_AUDIO_FILE,
        "rs" | "py" | "js" | "ts" | "go" | "java" | "c" | "cpp" | "h" | "sh" | "sql" | "html" | "css" => ICON_CODE,
        "txt" | "md" | "log" | "ini" | "conf" | "cfg" => ICON_DESCRIPTION,
        _ => ICON_DRAFT,
    }
}

fn human(n: u64) -> String {
    openvclient_core::containers::human_bytes(n)
}

fn when(t: &chrono::DateTime<chrono::Utc>) -> String {
    let local = t.with_timezone(&chrono::Local);
    let today = chrono::Local::now().date_naive();
    if local.date_naive() == today { format!("Today {}", local.format("%H:%M")) } else { local.format("%Y-%m-%d %H:%M").to_string() }
}

fn kind_label(e: &FileEntry) -> String {
    if e.is_dir {
        return match e.kind.as_str() {
            "container" => "Container".into(),
            "bucket" => "Bucket".into(),
            _ => "Folder".into(),
        };
    }
    let ext = e.name.rsplit_once('.').map(|(_, x)| x.to_ascii_uppercase()).unwrap_or_default();
    if ext.is_empty() { "File".into() } else { format!("{ext} file") }
}

impl FilesTab {
    pub fn new(conn: Uuid, start: Option<String>) -> Self {
        let started = start.is_some();
        Self {
            conn,
            cwd: start,
            back: Vec::new(),
            forward: Vec::new(),
            listing: None,
            job: None,
            filter: String::new(),
            applied_filter: String::new(),
            sort: (SortBy::Name, true),
            selected: BTreeSet::new(),
            anchor: None,
            preview: None,
            preview_job: None,
            batch: None,
            modal: None,
            delete_confirmed: Arc::new(AtomicBool::new(false)),
            pending_delete: Vec::new(),
            started,
        }
    }

    /// Open `node` (a folder id) from outside, e.g. the sidebar.
    pub fn navigate(&mut self, node: Option<String>) {
        if node != self.cwd {
            self.go(node);
        }
    }

    /// Test hook: select an entry by name as a single click would.
    #[cfg(test)]
    pub fn click_name(&mut self, cx: &AppCtx, name: &str) {
        let entries = self.entries();
        if let Some((i, e)) = entries.iter().enumerate().find(|(_, e)| e.name == name) {
            self.selected = BTreeSet::from([e.id.clone()]);
            self.anchor = Some(i);
            if !e.is_dir {
                self.show_preview(cx, e.clone());
            }
        }
    }

    pub fn title(&self) -> String {
        self.cwd.as_deref().map(|c| c.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(c).to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Files".into())
    }

    fn session(cx: &AppCtx, conn: Uuid) -> Option<Arc<dyn ResourceSession>> {
        cx.conns.session(conn).and_then(|h| h.resource()).cloned()
    }

    fn go(&mut self, node: Option<String>) {
        self.back.push(self.cwd.take());
        self.forward.clear();
        self.cwd = node;
        self.reset_view();
    }

    fn reset_view(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.preview = None;
        self.preview_job = None;
        self.filter.clear();
        self.applied_filter.clear();
        self.listing = None;
        self.job = None;
    }

    fn load(&mut self, cx: &AppCtx) {
        let Some(s) = Self::session(cx, self.conn) else { return };
        let (node, filter) = (self.cwd.clone(), self.applied_filter.clone());
        self.job = Some(cx.bg.spawn("Loading folder", async move { s.list_dir(node.as_deref(), &filter).await }));
    }

    fn entries(&self) -> Vec<FileEntry> {
        let mut v = match &self.listing {
            Some(Ok(l)) => l.entries.clone(),
            _ => Vec::new(),
        };
        let (by, asc) = self.sort;
        v.sort_by(|a, b| {
            // Folders first, then the chosen column.
            let o = b.is_dir.cmp(&a.is_dir).then_with(|| match by {
                SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Modified => a.modified.cmp(&b.modified),
                SortBy::Kind => kind_label(a).cmp(&kind_label(b)),
            });
            if asc || a.is_dir != b.is_dir { o } else { o.reverse() }
        });
        v
    }

    fn selection(&self) -> Vec<FileEntry> {
        self.entries().into_iter().filter(|e| self.selected.contains(&e.id)).collect()
    }

    fn open_entry(&mut self, cx: &AppCtx, e: &FileEntry) {
        if e.is_dir {
            self.go(Some(e.id.clone()));
        } else {
            self.show_preview(cx, e.clone());
        }
    }

    fn show_preview(&mut self, cx: &AppCtx, e: FileEntry) {
        let Some(s) = Self::session(cx, self.conn) else { return };
        let id = e.id.clone();
        self.preview = Some((e, None));
        self.preview_job = Some(cx.bg.spawn("Loading preview", async move { s.open(&id, Default::default()).await }));
    }

    /// Run `(node, action, args)` items one after another in the background.
    fn run_batch(&mut self, cx: &AppCtx, label: String, items: Vec<(String, &'static str, Args)>) {
        let Some(s) = Self::session(cx, self.conn) else { return };
        let total = items.len();
        let job = cx.bg.spawn(label.clone(), async move {
            let mut out = Vec::with_capacity(total);
            for (node, action, args) in items {
                let name = args.get("name").or(args.get("file")).cloned().unwrap_or_else(|| node.clone());
                out.push((name, s.run_action(&node, action, args).await));
            }
            out
        });
        self.batch = Some(Batch { label, job });
    }

    fn upload_paths(&mut self, cx: &mut AppCtx, paths: Vec<PathBuf>) {
        let Some(s) = Self::session(cx, self.conn) else { return };
        let (Some(dir), Some(action)) = (self.cwd.clone(), s.file_actions().upload) else {
            cx.toasts.info("Open a folder first", "Files are uploaded into the folder that is open.");
            return;
        };
        let files: Vec<PathBuf> = paths.into_iter().filter(|p| p.is_file()).collect();
        if files.is_empty() {
            cx.toasts.info("Nothing to upload", "Folders can't be uploaded yet; drop or pick files.");
            return;
        }
        let n = files.len();
        let items = files
            .into_iter()
            .map(|p| {
                let mut a = Args::new();
                a.insert("file".into(), p.display().to_string());
                (dir.clone(), action, a)
            })
            .collect();
        self.run_batch(cx, format!("Uploading {n} file{}", if n == 1 { "" } else { "s" }), items);
    }

    fn download(&mut self, cx: &AppCtx, files: Vec<FileEntry>) {
        let Some(s) = Self::session(cx, self.conn) else { return };
        let Some(action) = s.file_actions().download else { return };
        let files: Vec<FileEntry> = files.into_iter().filter(|f| !f.is_dir).collect();
        let targets: Vec<(FileEntry, PathBuf)> = match files.len() {
            0 => return,
            1 => match rfd::FileDialog::new().set_file_name(&files[0].name).save_file() {
                Some(p) => vec![(files[0].clone(), p)],
                None => return,
            },
            _ => match rfd::FileDialog::new().set_title("Download to folder").pick_folder() {
                Some(dir) => files
                    .into_iter()
                    .map(|f| {
                        let p = dir.join(&f.name);
                        (f, p)
                    })
                    .collect(),
                None => return,
            },
        };
        let n = targets.len();
        let items = targets
            .into_iter()
            .map(|(f, p)| {
                let mut a = Args::new();
                a.insert("file".into(), p.display().to_string());
                a.insert("name".into(), f.name.clone());
                (f.id, action, a)
            })
            .collect();
        self.run_batch(cx, format!("Downloading {n} file{}", if n == 1 { "" } else { "s" }), items);
    }

    fn ask_delete(&mut self, cx: &mut AppCtx, items: Vec<FileEntry>) {
        if items.is_empty() {
            return;
        }
        let names: Vec<String> = items.iter().take(8).map(|e| format!("{}{}", e.name, if e.is_dir { "/" } else { "" })).collect();
        let more = items.len().saturating_sub(8);
        let dirs = items.iter().filter(|e| e.is_dir).count();
        let mut body = format!("This permanently deletes:\n{}", names.join("\n"));
        if more > 0 {
            body.push_str(&format!("\n… and {more} more"));
        }
        if dirs > 0 {
            body.push_str("\n\nFolders are deleted with everything inside them.");
        }
        let flag = self.delete_confirmed.clone();
        self.pending_delete = items;
        let n = self.pending_delete.len();
        cx.confirm(Some(self.conn), format!("Delete {n} item{}?", if n == 1 { "" } else { "s" }), body, "Delete", None, move |_| flag.store(true, Ordering::SeqCst));
    }

    fn do_delete(&mut self, cx: &AppCtx) {
        let Some(s) = Self::session(cx, self.conn) else { return };
        let fa = s.file_actions();
        let items: Vec<(String, &'static str, Args)> = std::mem::take(&mut self.pending_delete)
            .into_iter()
            .filter_map(|e| {
                let action = if e.is_dir { fa.delete_dir } else { fa.delete_file }?;
                let mut a = Args::new();
                a.insert("name".into(), e.name.clone());
                Some((e.id, action, a))
            })
            .collect();
        let n = items.len();
        if n > 0 {
            self.run_batch(cx, format!("Deleting {n} item{}", if n == 1 { "" } else { "s" }), items);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut AppCtx) {
        let t = theme::tokens(ui);
        let Some(s) = Self::session(cx, self.conn) else {
            crate::views::unsupported(ui, "Not connected. Connect from the sidebar to browse files.");
            return;
        };
        if !self.started {
            self.started = true;
            self.cwd = s.root_dir();
        }
        let fa = s.file_actions();
        let read_only = cx.store.profile(self.conn).is_some_and(|p| p.read_only);

        // ---- background results -------------------------------------------------
        if self.listing.is_none() && self.job.is_none() {
            self.load(cx);
        }
        if let Some(r) = poll(&mut self.job) {
            self.listing = Some(r);
        }
        if let Some(r) = poll(&mut self.preview_job)
            && let Some((_, c)) = &mut self.preview
        {
            *c = Some(r);
        }
        if let Some(b) = &mut self.batch
            && let Some(results) = b.job.take()
        {
            let label = b.label.clone();
            self.batch = None;
            let failed: Vec<&(String, Result<ActionOutcome>)> = results.iter().filter(|(_, r)| r.is_err()).collect();
            let ok = results.len() - failed.len();
            if let Some((_, Ok(o))) = results.first()
                && let Some(Content::Text(url)) = &o.content
                && results.len() == 1
                && url.starts_with("http")
            {
                self.modal = Some(Modal::Share(url.clone()));
            } else if failed.is_empty() {
                let msg = results.first().and_then(|(_, r)| r.as_ref().ok()).map(|o| o.message.clone()).unwrap_or_default();
                cx.toasts.ok(if results.len() == 1 { msg } else { format!("{label}: done ({ok})") });
            } else {
                let (name, err) = failed[0];
                let e = err.as_ref().err().cloned().unwrap_or_else(|| Error::other("failed"));
                cx.toasts.error(format!("{label}: {} of {} failed (first: {name})", failed.len(), results.len()), &e);
            }
            self.listing = None; // refresh
        }
        if self.delete_confirmed.swap(false, Ordering::SeqCst) {
            self.do_delete(cx);
        }

        // ---- drag and drop --------------------------------------------------------
        let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let dropped: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if !dropped.is_empty() && ui.ui_contains_pointer() {
            if read_only {
                cx.toasts.info("Read-only connection", "Uploads are turned off for this connection.");
            } else {
                self.upload_paths(cx, dropped);
            }
        }

        // ---- navigation bar ---------------------------------------------------------
        let conn_name = cx.profile_name(self.conn);
        let crumbs = self.cwd.as_deref().map(|c| s.breadcrumbs(c)).unwrap_or_default();
        let root = s.root_dir();
        let mut nav: Option<Option<String>> = None;
        ui.horizontal(|ui| {
            let b = |ui: &mut egui::Ui, icon, tip: &str, on: bool| {
                ui.add_enabled(on, egui::Button::new(String::from(icon)).min_size(egui::vec2(30.0, 28.0))).on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
            };
            if b(ui, ICON_ARROW_BACK, "Back", !self.back.is_empty()).clicked() {
                let prev = self.back.pop().unwrap_or(None);
                self.forward.push(self.cwd.take());
                self.cwd = prev;
                self.reset_view();
            }
            if b(ui, ICON_ARROW_FORWARD, "Forward", !self.forward.is_empty()).clicked() {
                let next = self.forward.pop().unwrap_or(None);
                self.back.push(self.cwd.take());
                self.cwd = next;
                self.reset_view();
            }
            let up = if crumbs.len() >= 2 {
                Some(Some(crumbs[crumbs.len() - 2].0.clone()))
            } else if self.cwd.is_some() && root.is_none() {
                Some(None)
            } else {
                None
            };
            if b(ui, ICON_ARROW_UPWARD, "Up one level", up.is_some()).clicked() {
                nav = up.clone();
            }
            ui.add_space(theme::space::XS);
            // Breadcrumb: connection › container › folder › …
            egui::Frame::new().fill(t.muted).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.set_max_width((ui.available_width() - 280.0).max(200.0));
                egui::ScrollArea::horizontal().id_salt("crumbs").stick_to_right(true).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let link = |ui: &mut egui::Ui, text: String, current: bool| {
                            let rt = if current { RichText::new(text).strong().color(t.fg) } else { RichText::new(text).color(t.muted_fg) };
                            ui.add(egui::Label::new(rt).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand)
                        };
                        let at_root = self.cwd.is_none() || (root.is_some() && self.cwd == root && crumbs.len() <= 1);
                        let first = if theme::is_cartoon() {
                            // Cartoon: the connection's sticker, then its name.
                            let driver = cx.store.profile(self.conn).map(|p| p.driver.clone()).unwrap_or_default();
                            crate::views::driver_glyph(ui, &driver, 14.0, t.muted_fg);
                            conn_name.clone()
                        } else {
                            ico(ICON_CLOUD, &conn_name)
                        };
                        if link(ui, first, at_root && crumbs.is_empty()).clicked() && root.is_none() {
                            nav = Some(None);
                        }
                        for (i, (id, label)) in crumbs.iter().enumerate() {
                            ui.label(RichText::new(String::from(ICON_CHEVRON_RIGHT)).color(t.muted_fg));
                            let current = i == crumbs.len() - 1;
                            if link(ui, label.clone(), current).clicked() && !current {
                                nav = Some(Some(id.clone()));
                            }
                        }
                    });
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if b(ui, ICON_REFRESH, "Refresh (F5)", true).clicked() || ui.input(|i| i.key_pressed(Key::F5)) {
                    self.listing = None;
                    self.job = None;
                }
                let r = ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(ico(ICON_SEARCH, "Filter this folder")).desired_width(220.0));
                if (r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) || (r.changed() && self.filter.is_empty()) {
                    self.applied_filter = self.filter.clone();
                    self.listing = None;
                    self.job = None;
                }
            });
        });
        if let Some(n) = nav {
            self.go(n);
            return;
        }

        // ---- action bar ---------------------------------------------------------------
        let sel = self.selection();
        let files_sel: Vec<FileEntry> = sel.iter().filter(|e| !e.is_dir).cloned().collect();
        let in_folder = self.cwd.is_some();
        let busy = self.batch.is_some();
        ui.add_space(theme::space::XS);
        ui.horizontal(|ui| {
            let can_write = !read_only && in_folder && !busy;
            if fa.upload.is_some()
                && ui
                    .add_enabled(can_write, egui::Button::new(RichText::new(ico(ICON_UPLOAD, "Upload")).color(t.on_accent)).fill(t.accent))
                    .on_hover_text("Upload files into this folder (or drop files here)")
                    .on_disabled_hover_text(if read_only { "Read-only connection" } else { "Open a folder to upload into" })
                    .clicked()
                && let Some(paths) = rfd::FileDialog::new().set_title("Upload files").pick_files()
            {
                self.upload_paths(cx, paths);
            }
            if fa.mkdir.is_some() && ui.add_enabled(can_write, egui::Button::new(ico(ICON_CREATE_NEW_FOLDER, "New folder"))).clicked() {
                self.modal = Some(Modal::NewFolder(String::new()));
            }
            ui.separator();
            let dl_label = if files_sel.len() > 1 { format!("Download ({})", files_sel.len()) } else { "Download".into() };
            if fa.download.is_some() && ui.add_enabled(!files_sel.is_empty() && !busy, egui::Button::new(ico(ICON_DOWNLOAD, &dl_label))).on_disabled_hover_text("Select one or more files").clicked() {
                self.download(cx, files_sel.clone());
            }
            if let Some(share) = fa.share
                && ui
                    .add_enabled(files_sel.len() == 1 && sel.len() == 1 && !busy, egui::Button::new(ico(ICON_LINK, "Share link")))
                    .on_disabled_hover_text("Select one file")
                    .on_hover_text("Create a temporary download link")
                    .clicked()
            {
                let mut a = Args::new();
                a.insert("hours".into(), "24".into());
                self.run_batch(cx, "Creating link".into(), vec![(files_sel[0].id.clone(), share, a)]);
            }
            if fa.rename.is_some() && ui.add_enabled(sel.len() == 1 && can_write, egui::Button::new(ico(ICON_DRIVE_FILE_RENAME_OUTLINE, "Rename"))).clicked() {
                self.modal = Some(Modal::Rename(sel[0].clone(), sel[0].name.clone()));
            }
            let can_delete =
                !sel.is_empty() && !read_only && !busy && sel.iter().all(|e| if e.is_dir { fa.delete_dir.is_some() && e.kind != "container" && e.kind != "bucket" } else { fa.delete_file.is_some() });
            if ui
                .add_enabled(can_delete, egui::Button::new(RichText::new(ico(ICON_DELETE, "Delete")).color(if can_delete { t.destructive } else { t.muted_fg })))
                .on_disabled_hover_text("Select files or folders")
                .clicked()
            {
                self.ask_delete(cx, sel.clone());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let total = match &self.listing {
                    Some(Ok(l)) => l.entries.len(),
                    _ => 0,
                };
                let bytes: u64 = sel.iter().filter_map(|e| e.size).sum();
                let text = if sel.is_empty() {
                    format!("{total} item{}", if total == 1 { "" } else { "s" })
                } else {
                    format!("{} of {total} selected{}", sel.len(), if bytes > 0 { format!(" · {}", human(bytes)) } else { String::new() })
                };
                ui.label(RichText::new(text).color(t.muted_fg));
            });
        });
        if let Some(b) = &self.batch {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(format!("{}…", b.label)).color(t.info));
            });
        }
        ui.add_space(theme::space::XS);

        // ---- body: list + preview -------------------------------------------------------
        let preview_w = if self.preview.is_some() { 340.0_f32.min(ui.available_width() * 0.4) } else { 0.0 };
        let area = ui.available_rect_before_wrap();
        let gap = if preview_w > 0.0 { 12.0 } else { 0.0 };
        let list_rect = egui::Rect::from_min_max(area.min, egui::pos2(area.max.x - preview_w - gap, area.max.y));
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list_rect).layout(egui::Layout::top_down(egui::Align::Min)));
        self.list_ui(&mut list_ui, cx, &s, hovering, read_only);
        if self.preview.is_some() {
            let prev_rect = egui::Rect::from_min_max(egui::pos2(area.max.x - preview_w, area.min.y), area.max);
            ui.painter().vline(prev_rect.left() - gap / 2.0, area.y_range(), egui::Stroke::new(1.0, t.border));
            let mut pui = ui.new_child(egui::UiBuilder::new().max_rect(prev_rect).layout(egui::Layout::top_down(egui::Align::Min)));
            self.preview_ui(&mut pui, cx);
        }
        ui.allocate_rect(area, Sense::hover());

        self.modal_ui(ui.ctx(), cx, &s);
    }

    fn list_ui(&mut self, ui: &mut egui::Ui, cx: &mut AppCtx, s: &Arc<dyn ResourceSession>, hovering: bool, read_only: bool) {
        let t = theme::tokens(ui);
        let rect = ui.available_rect_before_wrap();
        match &self.listing {
            None => {
                ui.add_space(theme::space::XL);
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Loading…").color(t.muted_fg));
                });
                return;
            }
            Some(Err(e)) => {
                crate::views::error_box(ui, e);
                if ui.button(ico(ICON_REFRESH, "Try again")).clicked() {
                    self.listing = None;
                }
                return;
            }
            Some(Ok(l)) if l.entries.is_empty() => {
                ui.add_space(theme::space::XL * 2.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(String::from(ICON_FOLDER_OPEN)).size(44.0).color(t.muted_fg));
                    ui.label(RichText::new(if self.applied_filter.is_empty() { "This folder is empty" } else { "No items match the filter" }).size(16.0).color(t.fg));
                    if self.cwd.is_some() && !read_only && s.file_actions().upload.is_some() {
                        ui.label(RichText::new("Drop files here or use Upload").color(t.muted_fg));
                    }
                });
                self.drop_overlay(ui, rect, hovering);
                return;
            }
            _ => {}
        }
        let entries = self.entries();
        let truncated = matches!(&self.listing, Some(Ok(l)) if l.truncated);
        if truncated {
            ui.label(RichText::new(ico(ICON_INFO, "Only the first items are shown. Narrow with the filter.")).small().color(t.warning));
        }
        // Keyboard: Cmd/Ctrl+A, Enter, Delete, Backspace (up), arrows.
        let mut open: Option<FileEntry> = None;
        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing && ui.ui_contains_pointer() || !typing && !self.selected.is_empty() {
            let (sel_all, enter, del, up_dir, down, up) = ui.input(|i| {
                (
                    i.modifiers.command && i.key_pressed(Key::A),
                    i.key_pressed(Key::Enter),
                    i.key_pressed(Key::Delete),
                    i.key_pressed(Key::Backspace),
                    i.key_pressed(Key::ArrowDown),
                    i.key_pressed(Key::ArrowUp),
                )
            });
            if sel_all {
                self.selected = entries.iter().map(|e| e.id.clone()).collect();
            }
            if enter && self.selected.len() == 1 {
                open = entries.iter().find(|e| self.selected.contains(&e.id)).cloned();
            }
            if del && !read_only {
                let sel = self.selection();
                self.ask_delete(cx, sel);
            }
            if up_dir {
                let crumbs = self.cwd.as_deref().map(|c| s.breadcrumbs(c)).unwrap_or_default();
                if crumbs.len() >= 2 {
                    self.go(Some(crumbs[crumbs.len() - 2].0.clone()));
                    return;
                } else if self.cwd.is_some() && s.root_dir().is_none() {
                    self.go(None);
                    return;
                }
            }
            if (down || up) && !entries.is_empty() {
                let cur = self.anchor.unwrap_or(0) as i64 + if down { 1 } else { -1 };
                let i = cur.clamp(0, entries.len() as i64 - 1) as usize;
                self.anchor = Some(i);
                self.selected = BTreeSet::from([entries[i].id.clone()]);
                if !entries[i].is_dir {
                    self.show_preview(cx, entries[i].clone());
                }
            }
        }

        let header = |ui: &mut egui::Ui, label: &str, by: SortBy, sort: &mut (SortBy, bool)| {
            let arrow = if sort.0 == by { if sort.1 { String::from(ICON_ARROW_DROP_UP) } else { String::from(ICON_ARROW_DROP_DOWN) } } else { String::new() };
            let r = ui.add(egui::Label::new(RichText::new(format!("{label} {arrow}")).strong().color(t.muted_fg)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
            if r.clicked() {
                *sort = if sort.0 == by { (by, !sort.1) } else { (by, true) };
            }
        };
        let mut clicked: Option<(usize, egui::Modifiers)> = None;
        let mut double: Option<usize> = None;
        let mut context: Option<(usize, FileEntry)> = None;
        let mut menu_action: Option<(&'static str, FileEntry)> = None;
        let row_h = 30.0;
        let sel_ids = self.selected.clone();
        let mut sort = self.sort;
        TableBuilder::new(ui)
            .id_salt(("files", self.conn, self.cwd.clone()))
            .striped(false)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .max_scroll_height(f32::INFINITY)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::remainder().at_least(220.0).clip(true))
            .column(Column::exact(96.0))
            .column(Column::exact(150.0))
            .column(Column::initial(150.0).at_least(80.0).clip(true))
            .header(28.0, |mut h| {
                h.col(|ui| header(ui, "Name", SortBy::Name, &mut sort));
                h.col(|ui| header(ui, "Size", SortBy::Size, &mut sort));
                h.col(|ui| header(ui, "Modified", SortBy::Modified, &mut sort));
                h.col(|ui| header(ui, "Type", SortBy::Kind, &mut sort));
            })
            .body(|body| {
                body.rows(row_h, entries.len(), |mut row| {
                    let i = row.index();
                    let e = &entries[i];
                    row.set_selected(sel_ids.contains(&e.id));
                    row.col(|ui| {
                        let icon = if e.is_dir { if e.kind == "container" || e.kind == "bucket" { ICON_INVENTORY_2 } else { ICON_FOLDER } } else { file_icon(&e.name) };
                        let color = if e.is_dir { t.warning } else { t.info };
                        crate::views::item_glyph(ui, icon, 18.0, color);
                        ui.add(egui::Label::new(RichText::new(&e.name).color(t.fg)).truncate().selectable(false)).on_hover_text(&e.name);
                    });
                    row.col(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(e.size.map(human).unwrap_or_else(|| "—".into())).color(t.muted_fg));
                        });
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(e.modified.as_ref().map(when).unwrap_or_else(|| "—".into())).color(t.muted_fg));
                    });
                    row.col(|ui| {
                        let k = match &e.detail {
                            Some(d) if !e.is_dir => format!("{} · {d}", kind_label(e)),
                            _ => kind_label(e),
                        };
                        ui.add(egui::Label::new(RichText::new(k).color(t.muted_fg)).truncate());
                    });
                    let resp = row.response().on_hover_cursor(egui::CursorIcon::PointingHand);
                    if resp.double_clicked() {
                        double = Some(i);
                    } else if resp.clicked() {
                        clicked = Some((i, resp.ctx.input(|inp| inp.modifiers)));
                    }
                    if resp.secondary_clicked() {
                        context = Some((i, e.clone()));
                    }
                    resp.context_menu(|ui| {
                        if ui.button(ico(if e.is_dir { ICON_FOLDER_OPEN } else { ICON_VISIBILITY }, if e.is_dir { "Open" } else { "Preview" })).clicked() {
                            menu_action = Some(("open", e.clone()));
                            ui.close();
                        }
                        if !e.is_dir && ui.button(ico(ICON_DOWNLOAD, "Download…")).clicked() {
                            menu_action = Some(("download", e.clone()));
                            ui.close();
                        }
                        if !e.is_dir && s.file_actions().share.is_some() && ui.button(ico(ICON_LINK, "Share link")).clicked() {
                            menu_action = Some(("share", e.clone()));
                            ui.close();
                        }
                        if ui.button(ico(ICON_CONTENT_COPY, "Copy name")).clicked() {
                            ui.ctx().copy_text(e.name.clone());
                            ui.close();
                        }
                        if !read_only && s.file_actions().rename.is_some() && ui.button(ico(ICON_DRIVE_FILE_RENAME_OUTLINE, "Rename…")).clicked() {
                            menu_action = Some(("rename", e.clone()));
                            ui.close();
                        }
                        if !read_only && e.kind != "container" && e.kind != "bucket" {
                            ui.separator();
                            if ui.button(RichText::new(ico(ICON_DELETE, "Delete…")).color(t.destructive)).clicked() {
                                menu_action = Some(("delete", e.clone()));
                                ui.close();
                            }
                        }
                    });
                });
            });
        self.sort = sort;
        self.drop_overlay(ui, rect, hovering);

        // ---- apply interactions -------------------------------------------------------
        if let Some((i, e)) = context
            && !self.selected.contains(&e.id)
        {
            self.selected = BTreeSet::from([e.id.clone()]);
            self.anchor = Some(i);
        }
        if let Some((i, m)) = clicked {
            let id = entries[i].id.clone();
            if m.shift
                && let Some(a) = self.anchor
            {
                let (lo, hi) = (a.min(i), a.max(i));
                self.selected = entries[lo..=hi].iter().map(|e| e.id.clone()).collect();
            } else if m.command {
                if !self.selected.remove(&id) {
                    self.selected.insert(id);
                }
                self.anchor = Some(i);
            } else {
                self.selected = BTreeSet::from([id]);
                self.anchor = Some(i);
            }
            if self.selected.len() == 1 && !entries[i].is_dir && !m.shift && !m.command {
                self.show_preview(cx, entries[i].clone());
            } else if self.selected.len() != 1 {
                self.preview = None;
            }
        }
        if let Some(i) = double {
            open = Some(entries[i].clone());
        }
        if let Some((what, e)) = menu_action {
            match what {
                "open" => open = Some(e),
                "download" => self.download(cx, vec![e]),
                "share" => {
                    if let Some(share) = s.file_actions().share {
                        let mut a = Args::new();
                        a.insert("hours".into(), "24".into());
                        self.run_batch(cx, "Creating link".into(), vec![(e.id.clone(), share, a)]);
                    }
                }
                "rename" => self.modal = Some(Modal::Rename(e.clone(), e.name.clone())),
                "delete" => self.ask_delete(cx, vec![e]),
                _ => {}
            }
        }
        if let Some(e) = open {
            self.open_entry(cx, &e);
        }
    }

    fn drop_overlay(&self, ui: &egui::Ui, rect: egui::Rect, hovering: bool) {
        if !hovering || self.cwd.is_none() {
            return;
        }
        let t = theme::tokens(ui);
        let p = ui.painter();
        p.rect(rect.shrink(4.0), 10.0, t.accent.gamma_multiply(0.12), egui::Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
        p.text(rect.center(), egui::Align2::CENTER_CENTER, format!("{}  Drop files to upload here", String::from(ICON_CLOUD_UPLOAD)), egui::FontId::proportional(18.0), t.fg);
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui, cx: &mut AppCtx) {
        let t = theme::tokens(ui);
        let Some((e, content)) = self.preview.clone() else { return };
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                crate::views::item_glyph(ui, file_icon(&e.name), 22.0, t.info);
                ui.add(egui::Label::new(RichText::new(&e.name).strong()).wrap());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(String::from(ICON_CLOSE)).on_hover_text("Close preview").clicked() {
                        self.preview = None;
                    }
                });
            });
            ui.label(RichText::new(format!("{} · {}", e.size.map(human).unwrap_or_default(), e.modified.as_ref().map(when).unwrap_or_default())).color(t.muted_fg));
            ui.add_space(theme::space::SM);
            if theme::primary_button(ui, ico(ICON_DOWNLOAD, "Download")).clicked() {
                self.download(cx, vec![e.clone()]);
            }
            ui.add_space(theme::space::SM);
            ui.separator();
            match &content {
                None => {
                    ui.spinner();
                }
                Some(Err(err)) => crate::views::error_box(ui, err),
                Some(Ok(c)) => {
                    egui::ScrollArea::both().id_salt("file_preview").auto_shrink([false, false]).show(ui, |ui| match c {
                        Content::Text(s) | Content::Json(s) => {
                            let shown: String = s.chars().take(200_000).collect();
                            ui.add(egui::Label::new(RichText::new(shown).monospace().size(12.0)).wrap_mode(egui::TextWrapMode::Extend));
                        }
                        Content::Props(p) => {
                            egui::Grid::new("file_props").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                                for (k, v) in p {
                                    ui.label(RichText::new(k).color(t.muted_fg));
                                    ui.add(egui::Label::new(v).wrap());
                                    ui.end_row();
                                }
                            });
                        }
                        Content::Empty(m) => {
                            ui.label(RichText::new(m).color(t.muted_fg));
                        }
                        _ => {
                            ui.label(RichText::new("No preview for this file type. Use Download.").color(t.muted_fg));
                        }
                    });
                }
            }
        });
    }

    fn modal_ui(&mut self, ctx: &egui::Context, cx: &mut AppCtx, s: &Arc<dyn ResourceSession>) {
        let Some(m) = self.modal.take() else { return };
        let mut keep = true;
        let mut submit: Option<(String, &'static str, Args, String)> = None;
        let m = match m {
            Modal::NewFolder(mut name) => {
                egui::Modal::new(egui::Id::unique(("files_mkdir", self.conn))).show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.heading("New folder");
                    let r = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Folder name").desired_width(f32::INFINITY));
                    r.request_focus();
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    ui.add_space(theme::space::SM);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            keep = false;
                        }
                        let ok = !name.trim().is_empty() && !name.contains('/');
                        if (ui.add_enabled(ok, egui::Button::new("Create")).clicked() || (enter && ok))
                            && let (Some(dir), Some(a)) = (self.cwd.clone(), s.file_actions().mkdir)
                        {
                            let mut args = Args::new();
                            args.insert("name".into(), name.trim().to_string());
                            submit = Some((dir, a, args, "Creating folder".into()));
                        }
                    });
                });
                Modal::NewFolder(name)
            }
            Modal::Rename(e, mut name) => {
                egui::Modal::new(egui::Id::unique(("files_rename", self.conn))).show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.heading(format!("Rename {}", e.name));
                    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
                    r.request_focus();
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    ui.add_space(theme::space::SM);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            keep = false;
                        }
                        let ok = !name.trim().is_empty() && name.trim() != e.name && !name.contains('/');
                        if (ui.add_enabled(ok, egui::Button::new("Rename")).clicked() || (enter && ok))
                            && let Some(a) = s.file_actions().rename
                        {
                            let mut args = Args::new();
                            args.insert("name".into(), name.trim().to_string());
                            submit = Some((e.id.clone(), a, args, "Renaming".into()));
                        }
                    });
                });
                Modal::Rename(e, name)
            }
            Modal::Share(url) => {
                egui::Modal::new(egui::Id::unique(("files_share", self.conn))).show(ctx, |ui| {
                    ui.set_width(520.0_f32.min(ctx.content_rect().width() - 48.0));
                    ui.heading("Share link");
                    ui.label(RichText::new("Anyone with this link can download the file until it expires (24 hours).").color(theme::muted(ui)));
                    ui.add_space(theme::space::SM);
                    let mut u = url.clone();
                    ui.add(egui::TextEdit::multiline(&mut u).desired_rows(3).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
                    ui.horizontal(|ui| {
                        if theme::primary_button(ui, ico(ICON_CONTENT_COPY, "Copy link")).clicked() {
                            ui.ctx().copy_text(url.clone());
                            cx.toasts.ok("Link copied");
                        }
                        if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            keep = false;
                        }
                    });
                });
                Modal::Share(url)
            }
        };
        if let Some((node, action, args, label)) = submit {
            self.run_batch(cx, label, vec![(node, action, args)]);
            keep = false;
        }
        if keep {
            self.modal = Some(m);
        }
    }
}
