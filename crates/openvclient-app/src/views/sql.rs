//! SQL editor tab with its own dedicated server session.

use std::path::PathBuf;
use std::sync::Arc;

use egui::{Key, KeyboardShortcut, Modifiers, RichText};
use egui_material_icons::icons::*;
use openvclient_core::explain::{ExplainResult, PlanNode};
use openvclient_core::sql::{ExecOptions, SqlConsole, StatementOutcome, StatementResult};
use openvclient_core::sqltext::{Dialect, leading_keyword, split_statements, statement_at};
use openvclient_core::{Error, Result};
use uuid::Uuid;

use crate::app::{AppCtx, Cmd};
use crate::bg::{Bg, Job, poll};
use crate::export::Format;
use crate::theme::{self, ico};
use crate::widgets::grid::{Grid, GridEvent, GridState, Overlay, detail_window};
use crate::widgets::sqledit::{EditorState, sql_editor};

enum Console {
    Opening(Job<Result<Arc<dyn SqlConsole>>>),
    Ready(Arc<dyn SqlConsole>),
    Failed(Error),
    Closed,
}

#[derive(PartialEq, Clone, Copy)]
enum Pane {
    Results,
    Plan,
    History,
}

pub struct SqlTab {
    pub conn: Uuid,
    pub dirty: bool,
    database: Option<String>,
    text: String,
    file: Option<PathBuf>,
    console: Console,
    dialect: Dialect,
    run: Option<Job<Vec<StatementResult>>>,
    running_sql: Vec<String>,
    results: Vec<StatementResult>,
    grids: Vec<GridState>,
    active: usize,
    generation: u64,
    explain: Option<Job<Result<ExplainResult>>>,
    plan: Option<Result<ExplainResult>>,
    plan_diagram: bool,
    plan_view: crate::widgets::plan_graph::PlanView,
    pane: Pane,
    editor: EditorState,
    n: u64,
    pending_confirm: bool,
    /// Last known cursor/selection; clicking a toolbar button unfocuses the
    /// editor, so the live values would be gone by then.
    last_cursor: Option<usize>,
    last_selection: Option<(usize, usize)>,
}

impl SqlTab {
    pub fn new(cx: &mut AppCtx, conn: Uuid, database: Option<String>, text: String) -> Self {
        let n = cx.tab_id();
        let s = cx.conns.session(conn).and_then(|h| h.sql()).cloned();
        let dialect = s.as_ref().map(|s| s.dialect()).unwrap_or(Dialect::Postgres);
        let database = database.or_else(|| s.as_ref().and_then(|s| s.default_database()));
        let console = match s {
            Some(s) => {
                let db = database.clone();
                Console::Opening(cx.bg.spawn("Opening SQL session", async move { s.open_console(db).await }))
            }
            None => Console::Failed(Error::connection("Not connected")),
        };
        let mut editor = EditorState::default();
        editor.focus = true;
        Self {
            conn,
            dirty: false,
            database,
            text,
            file: None,
            console,
            dialect,
            run: None,
            running_sql: Vec::new(),
            results: Vec::new(),
            grids: Vec::new(),
            active: 0,
            generation: 0,
            explain: None,
            plan: None,
            plan_diagram: true,
            plan_view: Default::default(),
            pane: Pane::Results,
            editor,
            n,
            pending_confirm: false,
            last_cursor: None,
            last_selection: None,
        }
    }

    pub fn title(&self, conn_name: &str) -> String {
        if let Some(f) = &self.file {
            return f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
        match &self.database {
            Some(d) => format!("{conn_name} · {d}"),
            None => format!("{conn_name} · console {}", self.n),
        }
    }

    pub fn close(&mut self, bg: &Bg) {
        if let Some(j) = &self.run {
            if let Console::Ready(c) = &self.console {
                c.cancel();
            }
            j.abort();
        }
        if let Console::Ready(c) = std::mem::replace(&mut self.console, Console::Closed) {
            let _ = bg.spawn("", async move { c.close().await });
        }
    }

    fn console(&self) -> Option<Arc<dyn SqlConsole>> {
        match &self.console {
            Console::Ready(c) => Some(c.clone()),
            _ => None,
        }
    }

    fn reopen(&mut self, cx: &mut AppCtx) {
        self.close(&cx.bg);
        if let Some(s) = cx.conns.session(self.conn).and_then(|h| h.sql()).cloned() {
            let db = self.database.clone();
            self.console = Console::Opening(cx.bg.spawn("Opening SQL session", async move { s.open_console(db).await }));
        } else {
            self.console = Console::Failed(Error::connection("Not connected"));
        }
    }

    /// Statements to run: the selection, the statement at the cursor, or all.
    fn pick(&self, all: bool, cursor: Option<usize>, selection: Option<(usize, usize)>) -> Vec<String> {
        let (src, base) = match selection {
            Some((a, b)) => (&self.text[a..b], 0),
            None => (self.text.as_str(), 0),
        };
        let _ = base;
        let spans = split_statements(self.dialect, src);
        if all || selection.is_some() {
            return spans.iter().map(|s| src[s.start..s.end].to_string()).collect();
        }
        match statement_at(&spans, cursor.unwrap_or(0)) {
            Some(s) => vec![src[s.start..s.end].to_string()],
            None => Vec::new(),
        }
    }

    fn start(&mut self, cx: &mut AppCtx, stmts: Vec<String>, confirmed: bool) {
        if stmts.is_empty() {
            cx.toasts.info("Nothing to run", "Write a statement first");
            return;
        }
        let Some(c) = self.console() else {
            cx.toasts.warn("Session not ready", "The SQL session is still opening or failed; see the banner above the editor.");
            return;
        };
        let writes = stmts.iter().any(|s| !matches!(leading_keyword(s).as_str(), "SELECT" | "WITH" | "SHOW" | "EXPLAIN" | "VALUES" | "TABLE" | "DESCRIBE" | "DESC" | "PRAGMA"));
        if writes && !confirmed && cx.is_production(self.conn) && cx.store.ws.settings.confirm_production {
            self.pending_confirm = true;
            self.running_sql = stmts;
            return;
        }
        let max = cx.store.ws.settings.max_rows;
        self.running_sql = stmts.clone();
        let n = stmts.len();
        self.run = Some(cx.bg.spawn(format!("Running {n} statement{}", if n == 1 { "" } else { "s" }), async move { c.execute(stmts, ExecOptions { max_rows: max }).await }));
        self.pane = Pane::Results;
    }

    fn explain(&mut self, cx: &mut AppCtx, analyze: bool, cursor: Option<usize>, selection: Option<(usize, usize)>) {
        let Some(stmt) = self.pick(false, cursor, selection).into_iter().next() else { return };
        let Some(c) = self.console() else { return };
        self.explain = Some(cx.bg.spawn(if analyze { "EXPLAIN ANALYZE" } else { "EXPLAIN" }, async move { c.explain(&stmt, analyze).await }));
        self.pane = Pane::Plan;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut AppCtx, tab_id: u64) {
        let t = theme::tokens(ui);
        // Background progress.
        if let Console::Opening(job) = &mut self.console {
            if let Some(r) = job.take() {
                self.console = match r {
                    Ok(c) => Console::Ready(c),
                    Err(e) => Console::Failed(e),
                };
            }
        }
        if let Some(res) = poll(&mut self.run) {
            for r in &res {
                cx.store.push_history(self.conn, &r.sql, r.outcome.is_ok());
            }
            cx.mark_dirty();
            let ddl = res.iter().any(|r| r.outcome.is_ok() && matches!(leading_keyword(&r.sql).as_str(), "CREATE" | "DROP" | "ALTER" | "RENAME"));
            if ddl {
                cx.cmd(Cmd::RefreshTree(self.conn));
                cx.cmd(Cmd::InvalidateCatalog(self.conn));
            }
            if let Some(i) = res.iter().position(|r| r.outcome.is_err()) {
                self.active = i;
            } else {
                self.active = res.iter().rposition(|r| matches!(r.outcome, Ok(StatementOutcome::Rows(_)))).unwrap_or(res.len().saturating_sub(1));
            }
            self.grids = res.iter().map(|_| GridState::default()).collect();
            self.results = res;
            self.generation += 1;
        }
        if let Some(r) = poll(&mut self.explain) {
            self.plan = Some(r);
            self.plan_view = Default::default();
        }
        let running = self.run.is_some();
        let caps = cx.conns.session(self.conn).and_then(|h| h.sql()).map(|s| s.capabilities()).unwrap_or_default();

        // Shortcuts apply only while this tab is focused.
        let focused = ui.ui_contains_pointer() || ui.memory(|m| m.has_focus(egui::Id::unique(("sqleditor", tab_id))));
        let (run_one, run_all, explain, cancel, save, refresh) = if focused {
            ui.input_mut(|i| {
                (
                    i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter)),
                    i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter)),
                    i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::E)),
                    i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Period)),
                    i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S)),
                    i.consume_key(Modifiers::NONE, Key::F5),
                )
            })
        } else {
            (false, false, false, false, false, false)
        };

        let mut want: Option<(bool, bool)> = None; // (all, explain)
        let mut want_analyze = false;
        // Toolbar.
        ui.horizontal(|ui| {
            if running {
                if theme::danger_button(ui, ico(ICON_STOP, &theme::kbd("Cancel  ⌘."))).clicked() {
                    if let Some(c) = self.console() {
                        c.cancel();
                    }
                }
                ui.add(egui::Spinner::new());
                if let Some(j) = &self.run {
                    ui.label(RichText::new(format!("{:.1}s", j.started.elapsed().as_secs_f32())).color(t.muted_fg));
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                }
            } else {
                let label = if self.last_selection.is_some() { "Run selection" } else { "Run" };
                let tip = if self.last_selection.is_some() { "Runs only the highlighted SQL (⌘⏎)" } else { "Runs the statement under the cursor (⌘⏎). Highlight text to run just that part." };
                if theme::primary_button(ui, ico(ICON_PLAY_ARROW, label)).on_hover_text(theme::kbd(tip)).clicked() {
                    want = Some((false, false));
                }
                if ui.button(ico(ICON_PLAYLIST_PLAY, "Run all")).on_hover_text(theme::kbd("Whole script (⌘⇧⏎)")).clicked() {
                    want = Some((true, false));
                }
            }
            if caps.explain && ui.add_enabled(!running, egui::Button::new(ico(ICON_ACCOUNT_TREE, "Explain"))).on_hover_text(theme::kbd("Query plan for the statement at cursor (⌘E)")).clicked() {
                want = Some((false, true));
            }
            if caps.explain_analyze
                && ui
                    .add_enabled(!running, egui::Button::new(ico(ICON_SPEED, "Analyze")))
                    .on_hover_text("EXPLAIN ANALYZE: executes the statement; data changes are rolled back")
                    .clicked()
            {
                want_analyze = true;
            }
            ui.separator();
            if ui.button(String::from(ICON_FILE_OPEN)).on_hover_text("Open .sql file").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("SQL", &["sql", "txt"]).pick_file() {
                    match std::fs::read_to_string(&p) {
                        Ok(s) => {
                            self.text = s;
                            self.file = Some(p);
                            self.dirty = false;
                        }
                        Err(e) => cx.toasts.error_text("Could not open file", e.to_string()),
                    }
                }
            }
            if ui.button(String::from(ICON_SAVE)).on_hover_text(theme::kbd("Save (⌘S)")).clicked() {
                self.save(cx, false);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match &self.console {
                    Console::Ready(c) => {
                        let db = c.current_database().unwrap_or_default();
                        ui.label(RichText::new(ico(ICON_DATABASE, &db)).color(t.muted_fg)).on_hover_text("Dedicated session for this tab. Transactions and SET persist.");
                        theme::dot(ui, t.accent);
                    }
                    Console::Opening(_) => crate::views::busy(ui, "opening session"),
                    Console::Failed(_) | Console::Closed => {
                        if ui.button(ico(ICON_REFRESH, "Reconnect session")).clicked() {
                            self.reopen(cx);
                        }
                    }
                }
                if let Some(p) = cx.store.profile(self.conn) {
                    theme::badge(ui, p.environment.short(), theme::env_color(ui, p.environment));
                }
            });
        });
        if let Console::Failed(e) = &self.console {
            crate::views::error_box(ui, e);
        }
        ui.add_space(theme::space::XS);

        // Editor (top, resizable) and results.
        let catalog = cx.catalog(self.conn, self.database.clone()).cloned();
        let font = cx.store.ws.settings.editor_font_size;
        let mut out = None;
        let avail = ui.available_height();
        egui::Panel::top(egui::Id::unique(("sql_editor_panel", tab_id)))
            .resizable(true)
            .default_size((avail * 0.42).max(160.0))
            .size_range(90.0..=(avail - 120.0).max(120.0))
            .frame(egui::Frame::new().fill(ui.visuals().extreme_bg_color).stroke(egui::Stroke::new(1.0, t.border)).corner_radius(6).inner_margin(6))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt(("sql_scroll", tab_id)).auto_shrink([false, false]).show(ui, |ui| {
                    let before = self.text.len();
                    let o = sql_editor(ui, egui::Id::unique(("sqleditor", tab_id)), &mut self.text, self.dialect, catalog.as_ref(), &mut self.editor, font);
                    if self.text.len() != before {
                        self.dirty = true;
                    }
                    out = Some(o);
                });
            });
        if let Some(o) = &out {
            if o.cursor.is_some() {
                self.last_cursor = o.cursor;
                self.last_selection = o.selection;
            }
        }
        // Ranges must still fit the text (it may have been edited since).
        let n = self.text.len();
        let cursor = self.last_cursor.filter(|c| *c <= n);
        let selection = self.last_selection.filter(|(a, b)| a < b && *b <= n && self.text.is_char_boundary(*a) && self.text.is_char_boundary(*b));
        if save {
            self.save(cx, false);
        }
        if cancel && running {
            if let Some(c) = self.console() {
                c.cancel();
            }
        }
        if !running {
            if run_one || refresh {
                want = Some((false, false));
            }
            if run_all {
                want = Some((true, false));
            }
            if explain && caps.explain {
                want = Some((false, true));
            }
        }
        match want {
            Some((_, true)) => self.explain(cx, false, cursor, selection),
            Some((all, false)) => {
                let s = self.pick(all, cursor, selection);
                self.start(cx, s, false);
            }
            None => {}
        }
        if want_analyze {
            self.explain(cx, true, cursor, selection);
        }
        if self.pending_confirm {
            self.pending_confirm = false;
            let stmts = std::mem::take(&mut self.running_sql);
            let preview: String = stmts.iter().map(|s| s.lines().next().unwrap_or("")).collect::<Vec<_>>().join("\n");
            let name = cx.profile_name(self.conn);
            let tid = tab_id;
            cx.confirm(Some(self.conn), "Run on production?", format!("These statements change data or schema on {name}:\n\n{preview}"), "Run", None, move |cx| {
                cx.pending_sql = Some((tid, stmts));
            });
        }
        if let Some((tid, stmts)) = cx.pending_sql.take() {
            if tid == tab_id {
                self.start(cx, stmts, true);
            } else {
                cx.pending_sql = Some((tid, stmts));
            }
        }

        ui.add_space(theme::space::XS);
        egui::CentralPanel::no_frame().show(ui, |ui| self.results_ui(ui, cx, tab_id));
    }

    fn save(&mut self, cx: &mut AppCtx, ask: bool) {
        let path = match (&self.file, ask) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new().add_filter("SQL", &["sql"]).set_file_name("query.sql").save_file(),
        };
        if let Some(p) = path {
            match std::fs::write(&p, &self.text) {
                Ok(()) => {
                    cx.toasts.ok(format!("Saved {}", p.display()));
                    self.file = Some(p);
                    self.dirty = false;
                }
                Err(e) => cx.toasts.error_text("Could not save", e.to_string()),
            }
        }
    }

    fn results_ui(&mut self, ui: &mut egui::Ui, cx: &mut AppCtx, tab_id: u64) {
        let t = theme::tokens(ui);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.pane, Pane::Results, ico(ICON_TABLE_ROWS, "Results"));
            ui.selectable_value(&mut self.pane, Pane::Plan, ico(ICON_ACCOUNT_TREE, "Plan"));
            ui.selectable_value(&mut self.pane, Pane::History, ico(ICON_HISTORY, "History"));
            if self.pane == Pane::Results && self.results.len() > 1 {
                ui.separator();
                for (i, r) in self.results.iter().enumerate() {
                    let (icon, color) = match &r.outcome {
                        Ok(StatementOutcome::Rows(_)) => (ICON_TABLE, t.fg),
                        Ok(_) => (ICON_CHECK, t.accent),
                        Err(_) => (ICON_ERROR, t.destructive),
                    };
                    if ui.selectable_label(self.active == i, RichText::new(format!("{} {}", String::from(icon), i + 1)).color(color)).on_hover_text(&r.sql).clicked() {
                        self.active = i;
                    }
                }
            }
        });
        ui.separator();
        match self.pane {
            Pane::Plan => self.plan_ui(ui),
            Pane::History => {
                let items: Vec<_> = cx.store.ws.history.iter().filter(|h| h.profile == self.conn).take(200).cloned().collect();
                if items.is_empty() {
                    crate::views::unsupported(ui, "Statements you run on this connection appear here.");
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for h in items {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(String::from(if h.ok { ICON_CHECK } else { ICON_ERROR })).color(if h.ok { t.accent } else { t.destructive }));
                            ui.label(RichText::new(&h.at).small().color(t.muted_fg));
                            let one: String = h.sql.split_whitespace().collect::<Vec<_>>().join(" ");
                            if ui.add(egui::Label::new(RichText::new(one).monospace()).truncate().sense(egui::Sense::click())).on_hover_text("Click to insert into the editor").clicked() {
                                if !self.text.ends_with('\n') && !self.text.is_empty() {
                                    self.text.push('\n');
                                }
                                self.text.push_str(&h.sql);
                                self.text.push_str(";\n");
                                self.dirty = true;
                            }
                        });
                    }
                });
            }
            Pane::Results => {
                if self.run.is_some() && self.results.is_empty() {
                    crate::views::busy(ui, "Running…");
                    return;
                }
                let Some(r) = self.results.get(self.active) else {
                    crate::views::unsupported(ui, &theme::kbd("Run a statement with ⌘⏎. Results, row counts and errors appear here."));
                    return;
                };
                let elapsed = openvclient_core::elapsed_ms(r.elapsed);
                match &r.outcome {
                    Err(e) => {
                        ui.label(RichText::new(r.sql.lines().next().unwrap_or("")).monospace().color(t.muted_fg));
                        crate::views::error_box(ui, e);
                        if self.results.len() > 1 {
                            ui.label(RichText::new(format!("Stopped at statement {} of {}; later statements did not run.", self.active + 1, self.running_sql.len().max(self.results.len()))).color(t.muted_fg));
                        }
                    }
                    Ok(StatementOutcome::Affected(n)) => {
                        ui.label(RichText::new(ico(ICON_CHECK, &format!("{n} row{} affected · {elapsed}", if *n == 1 { "" } else { "s" }))).color(t.accent));
                    }
                    Ok(StatementOutcome::Done(m)) => {
                        ui.label(RichText::new(ico(ICON_CHECK, &format!("{m} · {elapsed}"))).color(t.accent));
                    }
                    Ok(StatementOutcome::Rows(rs)) => {
                        let rs = rs.clone();
                        let st = &mut self.grids[self.active];
                        let mut events = Vec::new();
                        ui.horizontal(|ui| {
                            let shown = st.view(&rs, self.generation, false).len();
                            let mut txt = format!("{} row{} · {elapsed}", rs.rows.len(), if rs.rows.len() == 1 { "" } else { "s" });
                            if shown != rs.rows.len() {
                                txt = format!("{shown} of {txt}");
                            }
                            ui.label(RichText::new(txt).color(t.muted_fg));
                            if rs.truncated {
                                theme::badge(ui, "LIMITED", t.warning).on_hover_text(format!("Only the first {} rows were kept (Settings → Max rows).", rs.rows.len()));
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.menu_button(ico(ICON_DOWNLOAD, "Export"), |ui| {
                                    for f in Format::ALL {
                                        if ui.button(format!("{}…", f.label())).clicked() {
                                            let rows = st.view(&rs, self.generation, false).to_vec();
                                            export_to_file(cx, &rs, &rows, f, self.dialect);
                                            ui.close();
                                        }
                                    }
                                });
                                ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text(ico(ICON_SEARCH, "Filter rows")).desired_width(200.0));
                            });
                        });
                        events.extend(Grid { id: egui::Id::unique(("grid", tab_id, self.active, self.generation)), rs: &rs, editable: false, server_sort: None, overlay: Overlay::default(), generation: self.generation, row_offset: 0 }.show(ui, st));
                        detail_window(ui.ctx(), egui::Id::unique(("detail", tab_id)), &rs, st);
                        for e in events {
                            if let GridEvent::CopyRows(rows, f) = e {
                                ui.ctx().copy_text(crate::export::render(&rs, &rows, None, f, Some(self.dialect), ""));
                                cx.toasts.ok(format!("Copied {} row(s) as {}", rows.len(), f.label()));
                            }
                        }
                    }
                }
            }
        }
    }

    fn plan_ui(&mut self, ui: &mut egui::Ui) {
        let t = theme::tokens(ui);
        if self.explain.is_some() {
            crate::views::busy(ui, "Explaining…");
            return;
        }
        match &self.plan {
            None => crate::views::unsupported(ui, &theme::kbd("Press Explain (⌘E) to see how the engine runs the statement at the cursor.")),
            Some(Err(e)) => crate::views::error_box(ui, e),
            Some(Ok(p)) => {
                ui.horizontal_wrapped(|ui| {
                    for (k, v) in &p.summary {
                        theme::badge(ui, &format!("{k}: {v}"), t.info);
                    }
                    if ui.small_button(ico(ICON_CONTENT_COPY, "Copy raw")).clicked() {
                        ui.ctx().copy_text(p.raw.clone());
                    }
                    if p.tree.is_some() {
                        ui.separator();
                        ui.selectable_value(&mut self.plan_diagram, true, ico(ICON_ACCOUNT_TREE, "Diagram"));
                        ui.selectable_value(&mut self.plan_diagram, false, ico(ICON_LIST, "Tree"));
                    }
                });
                let one_line = p.statement.split_whitespace().collect::<Vec<_>>().join(" ");
                ui.add(egui::Label::new(RichText::new(one_line).monospace().small().color(t.muted_fg)).truncate()).on_hover_text(&p.statement);
                ui.add_space(theme::space::SM);
                if self.plan_diagram
                    && let Some(tree) = &p.tree
                {
                    self.plan_view.show(ui, tree);
                    return;
                }
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    if let Some(tree) = &p.tree {
                        let max_cost = tree.max_total_cost().max(1e-9);
                        let max_ms = tree.max_actual_ms();
                        plan_node(ui, tree, 0, max_cost, max_ms);
                    } else if let Some(rs) = &p.table {
                        let mut st = GridState::default();
                        Grid { id: egui::Id::unique("plan_table"), rs, editable: false, server_sort: None, overlay: Overlay::default(), generation: 0, row_offset: 0 }.show(ui, &mut st);
                    } else {
                        ui.label(RichText::new(&p.raw).monospace());
                    }
                });
            }
        }
    }
}

fn plan_node(ui: &mut egui::Ui, n: &PlanNode, depth: usize, max_cost: f64, max_ms: f64) {
    let t = theme::tokens(ui);
    let id = ui.make_persistent_id(("plan", depth, &n.title, n.total_cost.map(|c| c.to_bits())));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, depth < 6)
        .show_header(ui, |ui| {
            // Heat bar: share of the most expensive node, so hot spots stand out.
            let frac = match (n.actual_ms, max_ms > 0.0) {
                (Some(ms), true) => (ms / max_ms) as f32,
                _ => (n.total_cost.unwrap_or(0.0) / max_cost) as f32,
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(60.0, 10.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 3.0, t.muted);
            let color = if frac > 0.6 { t.destructive } else if frac > 0.25 { t.warning } else { t.accent };
            ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * frac.clamp(0.02, 1.0), rect.height())), 3.0, color);
            ui.label(RichText::new(&n.title).strong());
            let mut parts = Vec::new();
            if let Some(c) = n.total_cost {
                parts.push(format!("cost {c:.2}"));
            }
            if let Some(r) = n.plan_rows {
                parts.push(format!("est. rows {r:.0}"));
            }
            if let Some(r) = n.actual_rows {
                parts.push(format!("rows {r:.0}"));
            }
            if let Some(ms) = n.actual_ms {
                parts.push(format!("{ms:.3} ms"));
            }
            if let Some(l) = n.loops.filter(|l| *l > 1.0) {
                parts.push(format!("×{l:.0} loops"));
            }
            ui.label(RichText::new(parts.join(" · ")).small().color(t.muted_fg));
        })
        .body(|ui| {
            for (k, v) in &n.props {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(k).small().color(t.muted_fg));
                    ui.label(RichText::new(v).small().monospace());
                });
            }
            for c in &n.children {
                plan_node(ui, c, depth + 1, max_cost, max_ms);
            }
        });
}

pub fn export_to_file(cx: &mut AppCtx, rs: &openvclient_core::ResultSet, rows: &[usize], f: Format, d: Dialect) {
    let Some(path) = rfd::FileDialog::new().set_file_name(format!("export.{}", f.ext())).save_file() else { return };
    let text = crate::export::render(rs, rows, None, f, Some(d), "");
    match std::fs::write(&path, text) {
        Ok(()) => cx.toasts.ok(format!("Exported {} rows to {}", rows.len(), path.display())),
        Err(e) => cx.toasts.error_text("Export failed", e.to_string()),
    }
}
