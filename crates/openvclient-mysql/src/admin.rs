//! Catalog browsing, DDL, monitoring, accounts and native dump/restore.

use std::path::PathBuf;
use std::time::Instant;

use openvclient_core::sql::*;
use openvclient_core::{Error, Result, ResultSet};

use crate::{D, MySession, is_system_db};

fn human_size(bytes: Option<&String>) -> String {
    let b: u64 = bytes.and_then(|b| b.parse().ok()).unwrap_or(0);
    if b == 0 { String::new() } else { openvclient_core::containers::human_bytes(b) }
}

pub async fn children(s: &MySession, node: Option<(ObjectKind, ObjPath)>) -> Result<Vec<TreeNode>> {
    let Some((kind, path)) = node else {
        let rows = s
            .text_rows(
                "SELECT s.SCHEMA_NAME, SUM(t.DATA_LENGTH + t.INDEX_LENGTH), COUNT(t.TABLE_NAME) \
                 FROM information_schema.SCHEMATA s LEFT JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = s.SCHEMA_NAME \
                 GROUP BY s.SCHEMA_NAME ORDER BY s.SCHEMA_NAME",
                vec![],
            )
            .await?;
        let mut v: Vec<TreeNode> = rows
            .into_iter()
            .map(|r| {
                let name = r[0].clone().unwrap_or_default();
                let n = r[2].clone().unwrap_or_default();
                let size = human_size(r[1].as_ref());
                let detail = [if n == "0" { String::new() } else { format!("{n} tables") }, size]
                    .into_iter()
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                TreeNode::new(ObjectKind::Database, name.clone(), ObjPath::db(name)).detail(detail)
            })
            .collect();
        v.sort_by_key(|n| (is_system_db(&n.label), n.label.clone()));
        return Ok(v);
    };
    let db = s.db_of(&path)?;
    let base = ObjPath::db(&db);
    let one = |sql: &'static str, okind: ObjectKind, leaf: bool| {
        let db = db.clone();
        let base = base.clone();
        async move {
            let rows = s.text_rows(sql, vec![db.into()]).await?;
            Ok::<_, Error>(
                rows.into_iter()
                    .map(|r| {
                        let name = r[0].clone().unwrap_or_default();
                        let detail = r.iter().skip(1).flatten().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" · ");
                        let n = TreeNode::new(okind, name.clone(), base.clone().with_name(name)).detail(detail);
                        if leaf { n.leaf() } else { n }
                    })
                    .collect(),
            )
        }
    };
    match kind {
        ObjectKind::Database => Ok(vec![
            TreeNode::folder(FolderKind::Tables, base.clone()),
            TreeNode::folder(FolderKind::Views, base.clone()),
            TreeNode::folder(FolderKind::Procedures, base.clone()),
            TreeNode::folder(FolderKind::Functions, base.clone()),
            TreeNode::folder(FolderKind::Triggers, base.clone()),
            TreeNode::folder(FolderKind::Events, base),
        ]),
        ObjectKind::Folder(FolderKind::Tables) => {
            let rows = s
                .text_rows(
                    "SELECT TABLE_NAME, TABLE_ROWS, DATA_LENGTH + INDEX_LENGTH, ENGINE FROM information_schema.TABLES \
                     WHERE TABLE_SCHEMA = ? AND TABLE_TYPE IN ('BASE TABLE', 'SYSTEM VERSIONED') ORDER BY TABLE_NAME",
                    vec![db.clone().into()],
                )
                .await?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    let name = r[0].clone().unwrap_or_default();
                    let mut parts = Vec::new();
                    if let Some(n) = r[1].as_ref().and_then(|n| n.parse::<u64>().ok()) {
                        parts.push(format!("~{n} rows"));
                    }
                    let sz = human_size(r[2].as_ref());
                    if !sz.is_empty() {
                        parts.push(sz);
                    }
                    if let Some(e) = r[3].clone() {
                        parts.push(e);
                    }
                    TreeNode::new(ObjectKind::Table, name.clone(), base.clone().with_name(name)).detail(parts.join(" · "))
                })
                .collect())
        }
        ObjectKind::Folder(FolderKind::Views) => {
            one("SELECT TABLE_NAME FROM information_schema.VIEWS WHERE TABLE_SCHEMA = ? ORDER BY 1", ObjectKind::View, true).await
        }
        ObjectKind::Folder(FolderKind::Procedures) => {
            one(
                "SELECT ROUTINE_NAME FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'PROCEDURE' ORDER BY 1",
                ObjectKind::Procedure,
                true,
            )
            .await
        }
        ObjectKind::Folder(FolderKind::Functions) => {
            one(
                "SELECT ROUTINE_NAME, CONCAT('→ ', DTD_IDENTIFIER) FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'FUNCTION' ORDER BY 1",
                ObjectKind::Function,
                true,
            )
            .await
        }
        ObjectKind::Folder(FolderKind::Triggers) if path.name.is_none() => {
            one(
                "SELECT TRIGGER_NAME, CONCAT(ACTION_TIMING, ' ', EVENT_MANIPULATION, ' ON ', EVENT_OBJECT_TABLE) FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA = ? ORDER BY 1",
                ObjectKind::Trigger,
                true,
            )
            .await
        }
        ObjectKind::Folder(FolderKind::Events) => {
            one(
                "SELECT EVENT_NAME, STATUS FROM information_schema.EVENTS WHERE EVENT_SCHEMA = ? ORDER BY 1",
                ObjectKind::Event,
                true,
            )
            .await
        }
        ObjectKind::Table => Ok(vec![
            TreeNode::folder(FolderKind::Columns, path.clone()),
            TreeNode::folder(FolderKind::Indexes, path.clone()),
            TreeNode::folder(FolderKind::Constraints, path.clone()),
        ]),
        ObjectKind::Folder(FolderKind::Columns) => Ok(columns(s, &path)
            .await?
            .into_iter()
            .map(|c| {
                let mut d = c.data_type.clone();
                if c.primary_key {
                    d.push_str(" · PK");
                }
                if c.auto_increment {
                    d.push_str(" · auto_increment");
                }
                if !c.nullable {
                    d.push_str(" · NOT NULL");
                }
                TreeNode::new(ObjectKind::Column, c.name.clone(), path.clone().with_sub(c.name)).detail(d)
            })
            .collect()),
        ObjectKind::Folder(FolderKind::Indexes) => {
            let rows = s
                .text_rows(
                    "SELECT INDEX_NAME, MIN(NON_UNIQUE), GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX SEPARATOR ', '), MIN(INDEX_TYPE) \
                     FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? GROUP BY INDEX_NAME ORDER BY INDEX_NAME = 'PRIMARY' DESC, INDEX_NAME",
                    vec![db.clone().into(), path.name()?.to_string().into()],
                )
                .await?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    let name = r[0].clone().unwrap_or_default();
                    let uniq = if name == "PRIMARY" { "primary key" } else if r[1].as_deref() == Some("0") { "unique" } else { "" };
                    let d = [uniq.to_string(), format!("({})", r[2].clone().unwrap_or_default()), r[3].clone().unwrap_or_default()]
                        .into_iter()
                        .filter(|x| !x.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    TreeNode::new(ObjectKind::Index, name.clone(), path.clone().with_sub(name)).detail(d)
                })
                .collect())
        }
        ObjectKind::Folder(FolderKind::Constraints) => {
            let rows = s
                .text_rows(
                    "SELECT CONSTRAINT_NAME, CONSTRAINT_TYPE FROM information_schema.TABLE_CONSTRAINTS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY CONSTRAINT_TYPE, 1",
                    vec![db.clone().into(), path.name()?.to_string().into()],
                )
                .await?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    let name = r[0].clone().unwrap_or_default();
                    TreeNode::new(ObjectKind::Constraint, name.clone(), path.clone().with_sub(name))
                        .detail(r[1].clone().unwrap_or_default().to_lowercase())
                })
                .collect())
        }
        _ => Ok(Vec::new()),
    }
}

pub async fn columns(s: &MySession, path: &ObjPath) -> Result<Vec<ColumnInfo>> {
    let rows = s
        .text_rows(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, COLUMN_KEY, EXTRA, COLUMN_COMMENT \
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            vec![s.db_of(path)?.into(), path.name()?.to_string().into()],
        )
        .await?;
    if rows.is_empty() {
        return Err(Error::invalid(format!("{} not found", path.display())));
    }
    Ok(rows
        .into_iter()
        .map(|r| ColumnInfo {
            name: r[0].clone().unwrap_or_default(),
            data_type: r[1].clone().unwrap_or_default(),
            nullable: r[2].as_deref() == Some("YES"),
            default: r[3].clone(),
            primary_key: r[4].as_deref() == Some("PRI"),
            auto_increment: r[5].as_deref().is_some_and(|e| e.contains("auto_increment")),
            comment: r[6].clone().filter(|c| !c.is_empty()).or_else(|| r[5].clone().filter(|e| e.contains("GENERATED"))),
        })
        .collect())
}

pub async fn ddl(s: &MySession, kind: ObjectKind, path: &ObjPath) -> Result<String> {
    let db = s.db_of(path)?;
    let (stmt, col) = match kind {
        ObjectKind::Database => (format!("SHOW CREATE DATABASE {}", D.quote_ident(&db)), "Create Database"),
        ObjectKind::Table => (format!("SHOW CREATE TABLE {}", D.qualified(Some(&db), path.name()?)), "Create Table"),
        ObjectKind::View => (format!("SHOW CREATE VIEW {}", D.qualified(Some(&db), path.name()?)), "Create View"),
        ObjectKind::Procedure => (format!("SHOW CREATE PROCEDURE {}", D.qualified(Some(&db), path.name()?)), "Create Procedure"),
        ObjectKind::Function => (format!("SHOW CREATE FUNCTION {}", D.qualified(Some(&db), path.name()?)), "Create Function"),
        ObjectKind::Trigger => (format!("SHOW CREATE TRIGGER {}", D.qualified(Some(&db), path.name()?)), "SQL Original Statement"),
        ObjectKind::Event => (format!("SHOW CREATE EVENT {}", D.qualified(Some(&db), path.name()?)), "Create Event"),
        ObjectKind::Index | ObjectKind::Constraint | ObjectKind::Column => {
            return Box::pin(ddl(s, ObjectKind::Table, &ObjPath { sub: None, ..path.clone() })).await;
        }
        k => return Err(Error::unsupported(format!("No DDL for {}", k.label()))),
    };
    let rs = s.result(&stmt, vec![]).await?;
    let i = rs.column_index(col).ok_or_else(|| Error::other(format!("{stmt} returned no `{col}` column (insufficient privilege?)")))?;
    let text = rs.rows.first().and_then(|r| r[i].to_text()).ok_or_else(|| Error::invalid("Object not found or definition hidden"))?;
    Ok(format!("{text};\n"))
}

pub async fn drop_object(s: &MySession, kind: ObjectKind, path: &ObjPath) -> Result<()> {
    let db = s.db_of(path)?;
    let sql = match kind {
        ObjectKind::Database => format!("DROP DATABASE {}", D.quote_ident(&db)),
        ObjectKind::Index => format!(
            "DROP INDEX {} ON {}",
            D.quote_ident(path.sub.as_deref().unwrap_or_default()),
            D.qualified(Some(&db), path.name()?)
        ),
        ObjectKind::Column => format!(
            "ALTER TABLE {} DROP COLUMN {}",
            D.qualified(Some(&db), path.name()?),
            D.quote_ident(path.sub.as_deref().unwrap_or_default())
        ),
        ObjectKind::Constraint => {
            let c = path.sub.clone().unwrap_or_default();
            if c == "PRIMARY" {
                format!("ALTER TABLE {} DROP PRIMARY KEY", D.qualified(Some(&db), path.name()?))
            } else {
                format!("ALTER TABLE {} DROP CONSTRAINT {}", D.qualified(Some(&db), path.name()?), D.quote_ident(&c))
            }
        }
        k => match k.sql_keyword() {
            Some(kw) if k != ObjectKind::Schema => format!("DROP {kw} {}", D.qualified(Some(&db), path.name()?)),
            _ => return Err(Error::unsupported(format!("Cannot drop a {} in MySQL", k.label()))),
        },
    };
    s.run(None, &sql).await.map(|_| ())
}

pub async fn locks(s: &MySession) -> Result<ResultSet> {
    let modern = s
        .result(
            "SELECT t.PROCESSLIST_ID AS connection, l.OBJECT_SCHEMA AS db, l.OBJECT_NAME AS `table`, l.INDEX_NAME AS `index`, \
             l.LOCK_TYPE AS lock_type, l.LOCK_MODE AS mode, l.LOCK_STATUS AS status, l.LOCK_DATA AS data, \
             (SELECT GROUP_CONCAT(bt.PROCESSLIST_ID) FROM performance_schema.data_lock_waits w \
               JOIN performance_schema.threads bt ON bt.THREAD_ID = w.BLOCKING_THREAD_ID \
               WHERE w.REQUESTING_ENGINE_LOCK_ID = l.ENGINE_LOCK_ID) AS blocked_by \
             FROM performance_schema.data_locks l LEFT JOIN performance_schema.threads t ON t.THREAD_ID = l.THREAD_ID \
             ORDER BY l.LOCK_STATUS DESC, t.PROCESSLIST_ID",
            vec![],
        )
        .await;
    match modern {
        Ok(rs) => Ok(rs),
        // MariaDB and MySQL 5.7 expose lock waits through InnoDB tables.
        Err(_) => {
            s.result(
                "SELECT trx_mysql_thread_id AS connection, trx_state AS state, trx_started AS started, \
                 trx_wait_started AS waiting_since, trx_rows_locked AS rows_locked, trx_tables_locked AS tables_locked, trx_query AS query \
                 FROM information_schema.INNODB_TRX ORDER BY trx_started",
                vec![],
            )
            .await
        }
    }
}

pub async fn metrics(s: &MySession) -> Result<Vec<Metric>> {
    let status: std::collections::HashMap<String, String> = s
        .text_rows("SHOW GLOBAL STATUS", vec![])
        .await?
        .into_iter()
        .filter_map(|r| Some((r[0].clone()?.to_ascii_lowercase(), r[1].clone().unwrap_or_default())))
        .collect();
    let vars: std::collections::HashMap<String, String> = s
        .text_rows(
            "SHOW GLOBAL VARIABLES WHERE Variable_name IN ('max_connections','innodb_buffer_pool_size','version','long_query_time','slow_query_log','read_only','innodb_flush_log_at_trx_commit','sync_binlog','log_bin')",
            vec![],
        )
        .await?
        .into_iter()
        .filter_map(|r| Some((r[0].clone()?.to_ascii_lowercase(), r[1].clone().unwrap_or_default())))
        .collect();
    let st = |k: &str| status.get(k).cloned().unwrap_or_else(|| "–".into());
    let var = |k: &str| vars.get(k).cloned().unwrap_or_else(|| "–".into());
    let num = |k: &str| status.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let uptime = num("uptime");
    let reads = num("innodb_buffer_pool_reads");
    let requests = num("innodb_buffer_pool_read_requests");
    let hit = if requests > 0.0 { format!("{:.2}", (1.0 - reads / requests) * 100.0) } else { "–".into() };
    let qps = if uptime > 0.0 { format!("{:.1}", num("questions") / uptime) } else { "–".into() };
    let bp = vars.get("innodb_buffer_pool_size").and_then(|v| v.parse().ok()).map(openvclient_core::containers::human_bytes).unwrap_or_default();
    let d = uptime as u64;
    Ok(vec![
        Metric::new("Server", "Version", var("version")),
        Metric::new("Server", "Uptime", format!("{}d {:02}:{:02}:{:02}", d / 86400, d / 3600 % 24, d / 60 % 60, d % 60)),
        Metric::new("Server", "Read only", var("read_only")),
        Metric::new("Server", "Binary log", var("log_bin")),
        Metric::new("Connections", "Connected", st("threads_connected")),
        Metric::new("Connections", "Running", st("threads_running")),
        Metric::new("Connections", "Max", var("max_connections")),
        Metric::new("Connections", "Max used", st("max_used_connections")),
        Metric::new("Connections", "Aborted connects", st("aborted_connects")),
        Metric::new("Throughput", "Queries per second (avg)", qps),
        Metric::new("Throughput", "SELECT", st("com_select")),
        Metric::new("Throughput", "INSERT", st("com_insert")),
        Metric::new("Throughput", "UPDATE", st("com_update")),
        Metric::new("Throughput", "DELETE", st("com_delete")),
        Metric::new("Throughput", "Bytes received", st("bytes_received")),
        Metric::new("Throughput", "Bytes sent", st("bytes_sent")),
        Metric::new("InnoDB", "Buffer pool size", bp),
        Metric::new("InnoDB", "Buffer pool hit ratio (%)", hit).hint("Below ~99% suggests the working set does not fit in memory"),
        Metric::new("InnoDB", "Row lock waits", st("innodb_row_lock_waits")),
        Metric::new("InnoDB", "Current row lock waits", st("innodb_row_lock_current_waits")),
        Metric::new("Problems", "Slow queries", st("slow_queries")).hint("Queries slower than long_query_time"),
        Metric::new("Problems", "Temp tables on disk", st("created_tmp_disk_tables")),
        Metric::new("Problems", "Full table scans (select_scan)", st("select_scan")),
        Metric::new("Configuration", "long_query_time", var("long_query_time")),
        Metric::new("Configuration", "slow_query_log", var("slow_query_log")),
        Metric::new("Configuration", "innodb_flush_log_at_trx_commit", var("innodb_flush_log_at_trx_commit")),
        Metric::new("Configuration", "sync_binlog", var("sync_binlog")),
    ])
}

pub fn account(name: &str, host: Option<&str>) -> String {
    format!("{}@{}", D.quote_literal(name), D.quote_literal(host.unwrap_or("%")))
}

pub async fn principals(s: &MySession) -> Result<ResultSet> {
    let full = s
        .result(
            "SELECT User AS name, Host AS host, plugin AS auth_plugin, account_locked, password_expired, \
             Super_priv AS superuser, Create_priv AS create_db, Grant_priv AS can_grant FROM mysql.user ORDER BY User, Host",
            vec![],
        )
        .await;
    match full {
        Ok(rs) => Ok(rs),
        Err(_) => s.result("SELECT User AS name, Host AS host FROM mysql.user ORDER BY User, Host", vec![]).await,
    }
}

pub async fn create_principal(s: &MySession, spec: PrincipalSpec) -> Result<()> {
    if spec.name.trim().is_empty() {
        return Err(Error::invalid("User name is required"));
    }
    let who = account(&spec.name, spec.host.as_deref());
    let mut sql = format!("CREATE USER {who}");
    if let Some(pw) = spec.password.filter(|p| !p.is_empty()) {
        sql.push_str(&format!(" IDENTIFIED BY {}", D.quote_literal(&pw)));
    }
    if !spec.can_login {
        sql.push_str(" ACCOUNT LOCK");
    }
    s.run(None, &sql).await?;
    if spec.superuser {
        s.run(None, &format!("GRANT ALL PRIVILEGES ON *.* TO {who} WITH GRANT OPTION")).await?;
    } else {
        let mut privs = Vec::new();
        if spec.create_db {
            privs.push("CREATE");
        }
        if spec.create_role {
            privs.push("CREATE USER");
        }
        if !privs.is_empty() {
            s.run(None, &format!("GRANT {} ON *.* TO {who}", privs.join(", "))).await?;
        }
    }
    Ok(())
}

pub async fn grant(s: &MySession, spec: GrantSpec, revoke: bool) -> Result<()> {
    if spec.privileges.is_empty() {
        return Err(Error::invalid("Select at least one privilege"));
    }
    let on = match &spec.target {
        GrantTarget::Database(d) | GrantTarget::Schema(d) | GrantTarget::AllTablesInSchema(d) => format!("{}.*", D.quote_ident(d)),
        GrantTarget::Table(p) => D.qualified(Some(&s.db_of(p)?), p.name()?),
    };
    let who = account(&spec.principal, spec.host.as_deref());
    let privs = spec.privileges.join(", ");
    let sql = if revoke {
        format!("REVOKE {privs} ON {on} FROM {who}")
    } else {
        format!("GRANT {privs} ON {on} TO {who}{}", if spec.with_grant_option { " WITH GRANT OPTION" } else { "" })
    };
    s.run(None, &sql).await.map(|_| ())
}

/// MySQL 9+ clients cannot authenticate to MariaDB, so each server uses its own tools.
pub fn dump_tools(mariadb: bool) -> &'static [&'static str] {
    if mariadb { &["mariadb-dump"] } else { &["mysqldump"] }
}

pub fn client_tools(mariadb: bool) -> &'static [&'static str] {
    if mariadb { &["mariadb"] } else { &["mysql"] }
}

pub fn find_tool(names: &[&str]) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for extra in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/usr/local/mysql/bin", "C:\\Program Files\\MySQL\\MySQL Server 8.0\\bin"] {
        dirs.push(PathBuf::from(extra));
    }
    for n in names {
        let exe = if cfg!(windows) { format!("{n}.exe") } else { n.to_string() };
        if let Some(p) = dirs.iter().map(|d| d.join(&exe)).find(|p| p.is_file()) {
            return Some(p);
        }
    }
    None
}

/// Credentials go through a private option file, never the command line.
struct OptionFile(PathBuf);

impl OptionFile {
    fn new(s: &MySession) -> Result<Self> {
        let path = std::env::temp_dir().join(format!("openvclient-my-{}-{}.cnf", std::process::id(), uuid_like()));
        let mut body = format!("[client]\nhost={}\nport={}\nuser={}\n", s.target.host, s.target.port, s.target.user);
        if let Some(pw) = &s.target.password {
            body.push_str(&format!("password=\"{}\"\n", pw.replace('\\', "\\\\").replace('"', "\\\"")));
        }
        if s.target.ssl == "disabled" {
            body.push_str("ssl-mode=DISABLED\n");
        }
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)?;
            f.write_all(body.as_bytes())?;
        }
        #[cfg(not(unix))]
        std::fs::write(&path, body)?;
        Ok(Self(path))
    }
}

impl Drop for OptionFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn uuid_like() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

pub async fn backup(s: &MySession, req: BackupRequest) -> Result<String> {
    let db = req.database.clone().or_else(|| s.target.database.clone()).ok_or_else(|| Error::invalid("Choose a database to back up"))?;
    let names = dump_tools(s.mariadb);
    let tool = find_tool(names).ok_or_else(|| Error::unsupported(format!("{} was not found on PATH", names[0])))?;
    let opt = OptionFile::new(s)?;
    let t = Instant::now();
    let out = tokio::process::Command::new(&tool)
        .arg(format!("--defaults-extra-file={}", opt.0.display()))
        .args(["--single-transaction", "--routines", "--triggers", "--events", "--hex-blob"])
        .arg(format!("--result-file={}", req.path.display()))
        .arg("--")
        .arg(&db)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| Error::io(format!("Could not start mysqldump: {e}")))?;
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        return Err(Error::server(format!("mysqldump failed ({})", out.status)).with_detail(stderr));
    }
    let size = std::fs::metadata(&req.path).map(|m| m.len()).unwrap_or(0);
    let mut msg = format!("mysqldump wrote {} ({size} bytes) in {}", req.path.display(), openvclient_core::elapsed_ms(t.elapsed()));
    if !stderr.is_empty() {
        msg.push_str(&format!("\n{stderr}"));
    }
    Ok(msg)
}

pub async fn restore(s: &MySession, req: BackupRequest) -> Result<String> {
    let db = req.database.clone().or_else(|| s.target.database.clone()).ok_or_else(|| Error::invalid("Choose a database to restore into"))?;
    let names = client_tools(s.mariadb);
    let tool = find_tool(names).ok_or_else(|| Error::unsupported(format!("The {} client was not found on PATH", names[0])))?;
    if db.starts_with('-') {
        return Err(Error::invalid("Database names starting with '-' cannot be restored with the command-line client"));
    }
    let text = std::fs::read_to_string(&req.path).map_err(|e| Error::io(format!("{}: {e}", req.path.display())))?;
    if let Some((line, cmd)) = openvclient_core::sqltext::dangerous_dump_line(&text, openvclient_core::sqltext::DumpClient::Mysql) {
        return Err(Error::invalid(format!("Refusing to restore: line {line} is a client command that runs programs or reads other files ({cmd}).")));
    }
    let opt = OptionFile::new(s)?;
    let input = std::fs::File::open(&req.path)?;
    let t = Instant::now();
    let out = tokio::process::Command::new(&tool)
        .arg(format!("--defaults-extra-file={}", opt.0.display()))
        .arg(&db)
        .stdin(input)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| Error::io(format!("Could not start mysql: {e}")))?;
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        return Err(Error::server(format!("mysql restore failed ({})", out.status)).with_detail(stderr));
    }
    Ok(format!("Restored {} into {db} in {}", req.path.display(), openvclient_core::elapsed_ms(t.elapsed())))
}
