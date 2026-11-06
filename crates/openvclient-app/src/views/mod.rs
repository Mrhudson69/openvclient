pub mod conn_dialog;
pub mod dialogs;
pub mod docker;
pub mod files;
pub mod home;
pub mod monitor;
pub mod object;
pub mod palette;
pub mod redis;
pub mod resource;
pub mod sidebar;
pub mod sql;
pub mod table;
pub mod terminal;
pub mod users;

use egui::RichText;
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::*;
use openvclient_core::sql::{FolderKind, ObjectKind};

use crate::theme;

pub fn kind_icon(k: ObjectKind) -> MaterialIcon {
    match k {
        ObjectKind::Database => ICON_DATABASE,
        ObjectKind::Schema => ICON_SCHEMA,
        ObjectKind::Folder(FolderKind::Columns) => ICON_VIEW_COLUMN,
        ObjectKind::Folder(FolderKind::Indexes) => ICON_BOLT,
        ObjectKind::Folder(FolderKind::Constraints) => ICON_KEY,
        ObjectKind::Folder(_) => ICON_FOLDER,
        ObjectKind::Table => ICON_TABLE,
        ObjectKind::View | ObjectKind::MaterializedView => ICON_TABLE_VIEW,
        ObjectKind::ForeignTable => ICON_LINK,
        ObjectKind::Function | ObjectKind::Procedure => ICON_FUNCTIONS,
        ObjectKind::Sequence => ICON_FORMAT_LIST_NUMBERED,
        ObjectKind::Trigger | ObjectKind::Event => ICON_SCHEDULE,
        ObjectKind::Column => ICON_VIEW_COLUMN,
        ObjectKind::Index => ICON_BOLT,
        ObjectKind::Constraint => ICON_KEY,
    }
}

pub fn driver_icon(driver: &str) -> MaterialIcon {
    match driver {
        "redis" => ICON_KEY,
        "docker" => ICON_DEPLOYED_CODE,
        "sqlite" => ICON_STORAGE,
        "mongodb" => ICON_DATA_OBJECT,
        "elasticsearch" => ICON_MANAGE_SEARCH,
        "s3" | "b2" | "idrive_e2" | "wasabi" | "r2" | "do_spaces" | "gcs" | "oci" | "linode" | "scaleway" | "hetzner" | "storj" | "minio" | "alibaba_oss" => ICON_CLOUD,
        "azure_blob" => ICON_CLOUD_UPLOAD,
        "ftp" => ICON_FOLDER_SHARED,
        "ssh" => ICON_TERMINAL,
        "kafka" => ICON_STREAM,
        "rabbitmq" => ICON_QUEUE,
        "cassandra" => ICON_VIEW_COLUMN,
        "clickhouse" => ICON_BAR_CHART,
        "sqlserver" => ICON_DNS,
        "oracle" | "db2" | "jdbc" => ICON_DATABASE,
        _ => ICON_DATABASE,
    }
}

/// Connector logo: the material icon in the original themes, a cartoon
/// sticker in the cartoon theme. Decorative: the connector name is always next to it.
pub fn driver_glyph(ui: &mut egui::Ui, driver: &str, size: f32, color: egui::Color32) -> egui::Response {
    if theme::is_cartoon() {
        let s = size + 6.0;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
        theme::paint_connector_sticker(ui.painter(), rect.center(), s, driver, ui.visuals().dark_mode);
        resp
    } else {
        ui.label(RichText::new(String::from(driver_icon(driver))).size(size).color(color))
    }
}

/// Human name of a connector, e.g. "PostgreSQL".
pub fn driver_label(cx: &crate::app::AppCtx, driver: &str) -> String {
    cx.conns.registry.get(driver).map(|d| d.info().name.to_string()).unwrap_or_else(|| driver.to_string())
}

/// Heading with icon used at the top of tab bodies.
pub fn header(ui: &mut egui::Ui, icon: MaterialIcon, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(String::from(icon)).size(theme::icon::LG).color(theme::tokens(ui).accent));
        ui.label(RichText::new(title).size(16.0).strong());
        if !subtitle.is_empty() {
            ui.label(RichText::new(subtitle).color(theme::muted(ui)));
        }
    });
}

/// Error block shown inline, next to what failed.
pub fn error_box(ui: &mut egui::Ui, e: &openvclient_core::Error) {
    let t = theme::tokens(ui);
    egui::Frame::new().fill(t.destructive.gamma_multiply(0.12)).stroke(egui::Stroke::new(1.0, t.destructive.gamma_multiply(0.5))).corner_radius(6).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(String::from(ICON_ERROR)).color(t.destructive));
            ui.label(RichText::new(&e.message).color(t.destructive).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(String::from(ICON_CONTENT_COPY)).on_hover_text("Copy diagnostics").clicked() {
                    ui.ctx().copy_text(format!("OpenVClient {}\n{}", env!("CARGO_PKG_VERSION"), e.diagnostics()));
                }
            });
        });
        if let Some(d) = &e.detail {
            ui.label(RichText::new(d).monospace().small());
        }
        if let Some(h) = e.hint() {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("{}  {h}", String::from(ICON_LIGHTBULB))).color(t.fg));
        }
    });
}

/// "Not available for this engine" note (never a fake control).
pub fn unsupported(ui: &mut egui::Ui, what: &str) {
    ui.add_space(theme::space::XL);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(String::from(ICON_INFO)).size(28.0).color(theme::muted(ui)));
        ui.label(RichText::new(what).color(theme::muted(ui)));
    });
}

pub fn busy(ui: &mut egui::Ui, label: &str) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(14.0));
        ui.label(RichText::new(label).color(theme::muted(ui)));
    });
}
