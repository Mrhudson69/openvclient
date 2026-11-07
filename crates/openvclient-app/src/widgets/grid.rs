//! Virtualized result grid: only visible rows are laid out, so 100k-row
//! results scroll smoothly.

use std::collections::{HashMap, HashSet};

use egui::{Align, Layout, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use openvclient_core::{ResultSet, Value};

use crate::theme;

#[derive(Default)]
pub struct GridState {
    pub selected: Option<(usize, usize)>,
    /// Extra selected rows (shift/cmd-click), by data index.
    pub selected_rows: HashSet<usize>,
    /// Client-side sort (column, ascending). Ignored when `server_sort`.
    pub sort: Option<(usize, bool)>,
    pub filter: String,
    view: Vec<usize>,
    view_key: Option<(usize, Option<(usize, bool)>, String, u64)>,
    pub editing: Option<(usize, usize, String)>,
    pub detail_open: bool,
}

impl GridState {
    /// Data row indexes in display order (after filter and sort).
    pub fn view(&mut self, rs: &ResultSet, generation: u64, server_sort: bool) -> &[usize] {
        let key = (rs.rows.len(), if server_sort { None } else { self.sort }, self.filter.clone(), generation);
        if self.view_key.as_ref() != Some(&key) {
            let needle = self.filter.to_lowercase();
            let mut v: Vec<usize> = (0..rs.rows.len()).filter(|&i| needle.is_empty() || rs.rows[i].iter().any(|c| c.display().to_lowercase().contains(&needle))).collect();
            if let (Some((c, asc)), false) = (self.sort, server_sort) {
                v.sort_by(|&a, &b| {
                    let o = rs.rows[a][c].cmp_for_sort(&rs.rows[b][c]);
                    if asc { o } else { o.reverse() }
                });
            }
            self.view = v;
            self.view_key = Some(key);
        }
        &self.view
    }

    pub fn selected_rows_sorted(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.selected_rows.iter().copied().collect();
        if let Some((r, _)) = self.selected
            && !v.contains(&r)
        {
            v.push(r);
        }
        v.sort();
        v
    }
}

pub enum GridEvent {
    HeaderClicked(usize),
    CommitEdit { row: usize, col: usize, text: String },
    SetNull { row: usize, col: usize },
    DeleteRows(Vec<usize>),
    RevertRow(usize),
    CopyRows(Vec<usize>, crate::export::Format),
}

/// Per-row and per-cell decoration supplied by the owner (pending edits).
#[derive(Default)]
pub struct Overlay<'a> {
    pub edited: Option<&'a HashMap<(usize, usize), Option<String>>>,
    pub deleted: Option<&'a HashSet<usize>>,
    pub inserted_from: Option<usize>,
}

pub struct Grid<'a> {
    pub id: egui::Id,
    pub rs: &'a ResultSet,
    pub editable: bool,
    pub server_sort: Option<&'a [(String, openvclient_core::sql::SortDir)]>,
    pub overlay: Overlay<'a>,
    pub generation: u64,
    /// Row numbers start here (table paging offset).
    pub row_offset: u64,
}

fn cell_text(v: &Value) -> (String, bool) {
    match v {
        Value::Null => ("NULL".into(), true),
        other => (other.display().into_owned(), false),
    }
}

fn initial_width(rs: &ResultSet, c: usize) -> f32 {
    let mut chars = rs.columns[c].name.len().max(rs.columns[c].type_name.len() / 2 + 2);
    for r in rs.rows.iter().take(50) {
        chars = chars.max(r[c].display().chars().count().min(60));
    }
    (chars as f32 * 7.4 + 24.0).clamp(56.0, 360.0)
}

impl<'a> Grid<'a> {
    pub fn show(self, ui: &mut Ui, st: &mut GridState) -> Vec<GridEvent> {
        let mut events = Vec::new();
        let rs = self.rs;
        let server_sort = self.server_sort.is_some();
        let view: Vec<usize> = st.view(rs, self.generation, server_sort).to_vec();
        let dark = ui.visuals().dark_mode;
        let muted = theme::muted(ui);
        // Pending-change tints derive from semantic tokens; the row label also says "new".
        let t = theme::tokens(ui);
        let tint = if dark { 0.22 } else { 0.14 };
        let edit_bg = t.warning.gamma_multiply(tint);
        let del_bg = t.destructive.gamma_multiply(tint);
        let new_bg = t.accent.gamma_multiply(tint);
        let mono = egui::TextStyle::Monospace.resolve(ui.style());
        let row_h = mono.size + 9.0;

        let mut tb = TableBuilder::new(ui)
            .id_salt(self.id)
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::exact(52.0));
        for c in 0..rs.columns.len() {
            tb = tb.column(Column::initial(initial_width(rs, c)).at_least(36.0).clip(true).resizable(true));
        }
        if rs.columns.is_empty() {
            ui.weak("No columns");
            return events;
        }
        tb.header(row_h + 12.0, |mut h| {
            h.col(|ui| {
                ui.weak("#");
            });
            for (c, col) in rs.columns.iter().enumerate() {
                h.col(|ui| {
                    let arrow = match (self.server_sort, st.sort) {
                        (Some(order), _) => order.iter().find(|(n, _)| n == &col.name).map(|(_, d)| if *d == openvclient_core::sql::SortDir::Asc { " ▲" } else { " ▼" }),
                        (None, Some((sc, asc))) if sc == c => Some(if asc { " ▲" } else { " ▼" }),
                        _ => None,
                    };
                    let resp = ui
                        .vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(RichText::new(format!("{}{}", col.name, arrow.unwrap_or(""))).strong());
                            if !col.type_name.is_empty() {
                                ui.label(RichText::new(&col.type_name).small().color(muted));
                            }
                        })
                        .response
                        .interact(Sense::click());
                    if resp.clicked() {
                        if server_sort {
                            events.push(GridEvent::HeaderClicked(c));
                        } else {
                            st.sort = match st.sort {
                                Some((sc, true)) if sc == c => Some((c, false)),
                                Some((sc, false)) if sc == c => None,
                                _ => Some((c, true)),
                            };
                        }
                    }
                    resp.on_hover_text("Click to sort");
                });
            }
        })
        .body(|body| {
            body.rows(row_h, view.len(), |mut row| {
                let vi = row.index();
                let r = view[vi];
                let deleted = self.overlay.deleted.is_some_and(|d| d.contains(&r));
                let inserted = self.overlay.inserted_from.is_some_and(|f| r >= f);
                let row_selected = st.selected.map(|s| s.0) == Some(r) || st.selected_rows.contains(&r);
                row.set_selected(row_selected);
                let (_, rn) = row.col(|ui| {
                    let label = if inserted { "new".to_string() } else { (self.row_offset + vi as u64 + 1).to_string() };
                    ui.label(RichText::new(label).color(muted).monospace());
                });
                if rn.clicked() {
                    let m = ui_mods(&rn);
                    if m.command || m.shift {
                        if !st.selected_rows.insert(r) {
                            st.selected_rows.remove(&r);
                        }
                    } else {
                        st.selected_rows.clear();
                        st.selected = Some((r, 0));
                    }
                }
                for c in 0..rs.columns.len() {
                    let editing_here = st.editing.as_ref().is_some_and(|(er, ec, _)| *er == r && *ec == c);
                    let pending = self.overlay.edited.and_then(|e| e.get(&(r, c)));
                    let (_, resp) = row.col(|ui| {
                        let rect = ui.max_rect();
                        if deleted {
                            ui.painter().rect_filled(rect, 0.0, del_bg);
                        } else if inserted {
                            ui.painter().rect_filled(rect, 0.0, new_bg);
                        } else if pending.is_some() {
                            ui.painter().rect_filled(rect, 0.0, edit_bg);
                        }
                        if st.selected == Some((r, c)) {
                            ui.painter().rect_stroke(rect.shrink(1.0), 2.0, egui::Stroke::new(1.5, theme::tokens(ui).ring), egui::StrokeKind::Inside);
                        }
                        if editing_here {
                            let (_, _, buf) = st.editing.as_mut().unwrap();
                            let te = ui.add(egui::TextEdit::singleline(buf).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                            te.request_focus();
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                st.editing = None;
                            } else if te.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                let (er, ec, text) = st.editing.take().unwrap();
                                events.push(GridEvent::CommitEdit { row: er, col: ec, text });
                            }
                            return;
                        }
                        let (text, is_null) = match pending {
                            Some(None) => ("NULL".to_string(), true),
                            Some(Some(t)) => (t.clone(), false),
                            None => cell_text(&rs.rows[r][c]),
                        };
                        let mut rt = RichText::new(text).monospace();
                        if is_null {
                            rt = rt.italics().color(muted);
                        }
                        if deleted {
                            rt = rt.strikethrough();
                        }
                        // Untyped columns (SQLite views, expressions) still right-align numbers.
                        let numeric = !is_null && (rs.columns[c].numeric || rs.rows[r][c].is_numeric());
                        if numeric {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(rt));
                        } else {
                            ui.add(egui::Label::new(rt).truncate());
                        }
                    });
                    if resp.clicked() {
                        st.selected = Some((r, c));
                        if !ui_mods(&resp).command {
                            st.selected_rows.clear();
                        }
                    }
                    if resp.double_clicked() {
                        if self.editable && !deleted {
                            let cur = match pending {
                                Some(v) => v.clone().unwrap_or_default(),
                                None => rs.rows[r][c].to_text().unwrap_or_default(),
                            };
                            st.editing = Some((r, c, cur));
                        } else {
                            st.detail_open = true;
                        }
                    }
                    resp.context_menu(|ui| {
                        st.selected = Some((r, c));
                        if ui.button("Copy value").clicked() {
                            let v = pending.cloned().unwrap_or_else(|| rs.rows[r][c].to_text()).unwrap_or_else(|| "NULL".into());
                            ui.ctx().copy_text(v);
                            ui.close();
                        }
                        let rows = {
                            let s = st.selected_rows_sorted();
                            if s.is_empty() { vec![r] } else { s }
                        };
                        ui.menu_button("Copy rows as", |ui| {
                            for f in crate::export::Format::ALL {
                                if ui.button(f.label()).clicked() {
                                    events.push(GridEvent::CopyRows(rows.clone(), f));
                                    ui.close();
                                }
                            }
                        });
                        if ui.button("View value…").clicked() {
                            st.detail_open = true;
                            ui.close();
                        }
                        if self.editable {
                            ui.separator();
                            if ui.button("Edit cell").clicked() {
                                st.editing = Some((r, c, rs.rows[r][c].to_text().unwrap_or_default()));
                                ui.close();
                            }
                            if ui.button("Set NULL").clicked() {
                                events.push(GridEvent::SetNull { row: r, col: c });
                                ui.close();
                            }
                            if ui.button(format!("Delete {} row(s)", rows.len())).clicked() {
                                events.push(GridEvent::DeleteRows(rows.clone()));
                                ui.close();
                            }
                            if (pending.is_some() || deleted || inserted) && ui.button("Revert row").clicked() {
                                events.push(GridEvent::RevertRow(r));
                                ui.close();
                            }
                        }
                    });
                }
            });
        });
        events
    }
}

fn ui_mods(r: &egui::Response) -> egui::Modifiers {
    r.ctx.input(|i| i.modifiers)
}

/// Side window showing the full selected value (long text, JSON, binary).
pub fn detail_window(ctx: &egui::Context, id: egui::Id, rs: &ResultSet, st: &mut GridState) {
    if !st.detail_open {
        return;
    }
    let Some((r, c)) = st.selected else {
        st.detail_open = false;
        return;
    };
    let Some(v) = rs.rows.get(r).and_then(|row| row.get(c)) else {
        st.detail_open = false;
        return;
    };
    let mut open = true;
    egui::Window::new(format!("{} · row {}", rs.columns[c].name, r + 1)).id(id.with("detail")).open(&mut open).default_size([520.0, 360.0]).show(ctx, |ui| {
        let mut text = match v {
            Value::Text(s) => match serde_json::from_str::<serde_json::Value>(s) {
                Ok(j) if s.trim_start().starts_with(['{', '[']) => serde_json::to_string_pretty(&j).unwrap_or_else(|_| s.clone()),
                _ => s.clone(),
            },
            other => other.to_text().unwrap_or_else(|| "NULL".into()),
        };
        ui.horizontal(|ui| {
            ui.weak(format!("{} · {} chars", rs.columns[c].type_name, text.chars().count()));
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(text.clone());
            }
        });
        egui::ScrollArea::both().show(ui, |ui| {
            ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_width(f32::INFINITY).interactive(true));
        });
    });
    if !open {
        st.detail_open = false;
    }
}
