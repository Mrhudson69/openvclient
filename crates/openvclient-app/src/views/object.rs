//! Structure and DDL for any catalog object.

use egui::RichText;
use egui_material_icons::icons::*;
use openvclient_core::Result;
use openvclient_core::sql::{ColumnInfo, ObjPath, ObjectKind};
use uuid::Uuid;

use crate::app::{AppCtx, Cmd};
use crate::bg::{Job, poll};
use crate::theme::{self, ico};

pub struct ObjectTab {
    pub conn: Uuid,
    pub kind: ObjectKind,
    pub path: ObjPath,
    ddl: Option<Result<String>>,
    ddl_job: Option<Job<Result<String>>>,
    cols: Option<Result<Vec<ColumnInfo>>>,
    cols_job: Option<Job<Result<Vec<ColumnInfo>>>>,
}

impl ObjectTab {
    pub fn new(conn: Uuid, kind: ObjectKind, path: ObjPath) -> Self {
        Self { conn, kind, path, ddl: None, ddl_job: None, cols: None, cols_job: None }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut AppCtx, tab_id: u64) {
        if let Some(r) = poll(&mut self.ddl_job) {
            self.ddl = Some(r);
        }
        if let Some(r) = poll(&mut self.cols_job) {
            self.cols = Some(r);
        }
        let Some(s) = cx.conns.session(self.conn).and_then(|h| h.sql()).cloned() else {
            crate::views::unsupported(ui, "Not connected");
            return;
        };
        if self.ddl.is_none() && self.ddl_job.is_none() {
            let (k, p, s2) = (self.kind, self.path.clone(), s.clone());
            self.ddl_job = Some(cx.bg.spawn("Loading DDL", async move { s2.ddl(k, &p).await }));
        }
        let tableish = self.kind.has_rows();
        if tableish && self.cols.is_none() && self.cols_job.is_none() {
            let p = self.path.clone();
            self.cols_job = Some(cx.bg.spawn("", async move { s.columns(&p).await }));
        }
        ui.horizontal(|ui| {
            crate::views::header(ui, crate::views::kind_icon(self.kind), &self.path.display(), self.kind.label());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(String::from(ICON_REFRESH)).on_hover_text("Reload").clicked() {
                    self.ddl = None;
                    self.cols = None;
                }
                if tableish && ui.button(ico(ICON_TABLE_ROWS, "Open data")).clicked() {
                    cx.cmd(Cmd::OpenTable { conn: self.conn, kind: self.kind, path: self.path.clone() });
                }
            });
        });
        ui.add_space(theme::space::SM);
        if tableish {
            crate::views::table::structure_ui(ui, &self.cols, tab_id);
            ui.add_space(theme::space::MD);
        }
        ddl_ui(ui, cx, self.conn, &self.ddl, self.ddl_job.is_some(), self.path.database.clone());
    }
}

pub fn ddl_ui(ui: &mut egui::Ui, cx: &mut AppCtx, conn: Uuid, ddl: &Option<Result<String>>, loading: bool, database: Option<String>) {
    if loading {
        crate::views::busy(ui, "Loading definition…");
        return;
    }
    match ddl {
        None => {}
        Some(Err(e)) => crate::views::error_box(ui, e),
        Some(Ok(text)) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Definition").strong());
                ui.label(RichText::new("as reported by the engine").small().color(theme::muted(ui)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(ico(ICON_CODE, "Open in editor")).clicked() {
                        cx.cmd(Cmd::NewSql { conn, database: database.clone(), text: text.clone() });
                    }
                    if ui.button(ico(ICON_CONTENT_COPY, "Copy")).clicked() {
                        ui.ctx().copy_text(text.clone());
                        cx.toasts.ok("DDL copied");
                    }
                });
            });
            let mut t = text.clone();
            let dialect = cx.conns.session(conn).and_then(|h| h.sql()).map(|s| s.dialect()).unwrap_or(openvclient_core::sqltext::Dialect::Postgres);
            let dark = ui.visuals().dark_mode;
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                let c = theme::syntax(dark);
                let mut job = egui::text::LayoutJob::default();
                let font = egui::FontId::monospace(13.0);
                for (k, sp) in openvclient_core::sqltext::tokenize(dialect, buf.as_str()) {
                    use openvclient_core::sqltext::TokenKind::*;
                    let col = match k {
                        Keyword => c.keyword,
                        String => c.string,
                        Number => c.number,
                        Comment => c.comment,
                        QuotedIdent => c.quoted,
                        Operator => c.operator,
                        Param => c.param,
                        _ => c.ident,
                    };
                    job.append(&buf.as_str()[sp.start..sp.end], 0.0, egui::TextFormat::simple(font.clone(), col));
                }
                job.wrap.max_width = wrap;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut t).code_editor().desired_width(f32::INFINITY).interactive(false).layouter(&mut layouter));
            });
        }
    }
}
