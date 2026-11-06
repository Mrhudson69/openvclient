#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod bg;
mod conns;
mod export;
mod store;
mod theme;
mod update;
mod views;
mod widgets;

fn registry() -> openvclient_core::DriverRegistry {
    let mut r = openvclient_core::DriverRegistry::default();
    #[cfg(feature = "mysql")]
    r.register(openvclient_mysql::MySqlDriver::default());
    #[cfg(feature = "mysql")]
    r.register(openvclient_mysql::MySqlDriver::mariadb());
    #[cfg(feature = "postgres")]
    r.register(openvclient_postgres::PostgresDriver::default());
    #[cfg(feature = "sqlserver")]
    r.register(openvclient_sqlserver::SqlServerDriver::default());
    #[cfg(feature = "postgres")]
    r.register(openvclient_postgres::PostgresDriver::flavor("kingbase", "Kingbase", "KingbaseES over the PostgreSQL protocol (PostgreSQL-compatible features)", "54321", "system", "test"));
    #[cfg(feature = "postgres")]
    r.register(openvclient_postgres::PostgresDriver::flavor("gaussdb", "GaussDB / openGauss", "GaussDB and openGauss over the PostgreSQL protocol (MD5 or SCRAM auth)", "5432", "gaussdb", "postgres"));
    #[cfg(feature = "sqlite")]
    r.register(openvclient_sqlite::SqliteDriver::default());
    #[cfg(feature = "http")]
    r.register(openvclient_http::clickhouse::ClickHouseDriver::default());
    #[cfg(feature = "http")]
    r.register(openvclient_http::elastic::ElasticDriver::default());
    #[cfg(feature = "http")]
    r.register(openvclient_http::rabbitmq::RabbitDriver::default());
    #[cfg(feature = "storage")]
    {
        r.register(openvclient_storage::ssh::SshDriver::default());
        r.register(openvclient_storage::ftp::FtpDriver::default());
        r.register(openvclient_storage::s3::S3Driver::default());
        r.register(openvclient_storage::azure::AzureBlobDriver::default());
        for d in openvclient_storage::s3::S3Driver::providers() {
            r.register(d);
        }
    }
    #[cfg(feature = "mongodb")]
    r.register(openvclient_mongo::MongoDriver::default());
    #[cfg(feature = "redis")]
    r.register(openvclient_redis::RedisDriver::default());
    #[cfg(feature = "docker")]
    r.register(openvclient_docker::DockerDriver::default());
    r
}

/// Logs go to stderr and to `<config>/logs/openvclient.log` (rotated at 5 MB).
/// Secrets are never logged; connection logs carry host, port and outcome.
fn init_logging() {
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let filter = || tracing_subscriber::EnvFilter::try_from_env("OPENVCLIENT_LOG").unwrap_or_else(|_| "warn,openvclient=info".into());
    let dir = store::config_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("openvclient.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 5 * 1024 * 1024) {
        let _ = std::fs::rename(&path, dir.join("openvclient.1.log"));
    }
    let file = std::fs::OpenOptions::new().create(true).append(true).open(&path).ok().map(std::sync::Arc::new);
    store::restrict(&path);
    let stderr = tracing_subscriber::fmt::layer().with_writer(std::io::stderr).with_filter(filter());
    let file_layer = file.map(|f| tracing_subscriber::fmt::layer().with_ansi(false).with_writer(f).with_filter(filter()));
    tracing_subscriber::registry().with(stderr).with(file_layer).init();
    tracing::info!(target: "openvclient", "OpenVClient {} starting on {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
}

fn main() -> eframe::Result<()> {
    init_logging();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(4).thread_name("openvclient-io").enable_all().build().expect("tokio runtime");
    let handle = rt.handle().clone();
    let options =
        eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_title("OpenVClient").with_inner_size([1440.0, 900.0]).with_min_inner_size([900.0, 560.0]), ..Default::default() };
    let result = eframe::run_native(
        "OpenVClient",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::OpenvApp::new(&cc.egui_ctx, handle, registry())))
        }),
    );
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    result
}
