//! Monitoring, user management, database lifecycle and native backups.

use std::path::PathBuf;
use std::time::Instant;

use openvclient_core::sql::*;
use openvclient_core::{Error, Result, ResultSet};

use crate::{D, PgSession};

/// Version-dependent pieces of the monitoring queries. Wait events and
/// pg_blocking_pids arrived in PostgreSQL 9.6, backend_type in 10; openGauss and
/// Kingbase report older versions and use the 9.2 `waiting` flag instead.
struct Monitor {
    waiting: &'static str,
    lock_wait: &'static str,
    backend_type: &'static str,
    blocked_by: &'static str,
}

fn monitor(s: &PgSession) -> Monitor {
    let v = s.version_num();
    Monitor {
        waiting: if v >= 90600 { "CASE WHEN wait_event IS NOT NULL THEN wait_event_type || ': ' || wait_event END" } else { "CASE WHEN waiting THEN 'Lock' END" },
        lock_wait: if v >= 90600 { "wait_event_type = 'Lock'" } else { "waiting" },
        backend_type: if v >= 100000 { "backend_type" } else { "NULL::text AS backend_type" },
        blocked_by: if v >= 90600 { "array_to_string(pg_blocking_pids(pid), ', ')" } else { "NULL::text" },
    }
}

pub async fn activity(s: &PgSession) -> Result<ActivitySnapshot> {
    let m = monitor(s);
    let sql = format!(
        "SELECT pid::text AS pid, usename::text AS \"user\", datname::text AS database, application_name AS application, \
         host(client_addr) AS client, state, \
         {} AS waiting, \
         to_char(now() - query_start, 'HH24:MI:SS.MS') AS running_for, \
         to_char(now() - xact_start, 'HH24:MI:SS') AS in_transaction, \
         {}, {} AS blocked_by, \
         CASE WHEN pid = pg_backend_pid() THEN '(this connection) ' ELSE '' END || coalesce(query, '') AS query \
         FROM pg_stat_activity ORDER BY (state = 'active') DESC NULLS LAST, query_start NULLS LAST",
        m.waiting, m.backend_type, m.blocked_by
    );
    let rows = s.text_result(None, &sql, &[]).await?;
    Ok(ActivitySnapshot { rows, id_column: 0 })
}

pub async fn kill(s: &PgSession, id: &str, mode: KillMode) -> Result<()> {
    // i64: openGauss uses thread ids as pids, which exceed 32 bits. The parsed
    // number goes in as a literal so the server picks int or bigint.
    let pid: i64 = id.parse().map_err(|_| Error::invalid(format!("Invalid pid {id}")))?;
    let f = if mode == KillMode::CancelQuery { "pg_cancel_backend" } else { "pg_terminate_backend" };
    let rows = s.text_rows(None, &format!("SELECT {f}({pid})::text"), &[]).await?;
    match rows.first().and_then(|r| r[0].as_deref()) {
        Some("true") => Ok(()),
        _ => Err(Error::server(format!("Server did not signal pid {pid} (already gone, or insufficient privilege)"))),
    }
}

pub async fn locks(s: &PgSession) -> Result<ResultSet> {
    let blocked_by = monitor(s).blocked_by.replace("(pid)", "(l.pid)");
    let sql = format!(
        "SELECT l.pid::text AS pid, a.usename::text AS \"user\", l.locktype AS lock_type, \
         coalesce(c.relname::text, l.relation::text) AS relation, l.mode, l.granted::text AS granted, \
         {blocked_by} AS blocked_by, \
         to_char(now() - a.query_start, 'HH24:MI:SS') AS waiting_for, a.query \
         FROM pg_locks l LEFT JOIN pg_class c ON c.oid = l.relation \
         LEFT JOIN pg_stat_activity a ON a.pid = l.pid \
         WHERE l.pid <> pg_backend_pid() \
         ORDER BY l.granted, l.pid"
    );
    s.text_result(None, &sql, &[]).await
}

pub async fn metrics(s: &PgSession) -> Result<Vec<Metric>> {
    let lock_wait = monitor(s).lock_wait;
    let sql = format!(
        "SELECT \
              current_setting('server_version'), \
              to_char(now() - pg_postmaster_start_time(), 'DD \"days\" HH24:MI:SS'), \
              (SELECT count(*) FROM pg_stat_activity)::text, \
              current_setting('max_connections'), \
              (SELECT count(*) FROM pg_stat_activity WHERE state = 'active')::text, \
              (SELECT count(*) FROM pg_stat_activity WHERE state = 'idle in transaction')::text, \
              (SELECT count(*) FROM pg_stat_activity WHERE {lock_wait})::text, \
              pg_size_pretty(pg_database_size(current_database())), \
              pg_database_size(current_database())::text, \
              (SELECT round(100.0 * sum(blks_hit) / nullif(sum(blks_hit) + sum(blks_read), 0), 2) FROM pg_stat_database)::text, \
              (SELECT sum(xact_commit) FROM pg_stat_database)::text, \
              (SELECT sum(xact_rollback) FROM pg_stat_database)::text, \
              (SELECT sum(deadlocks) FROM pg_stat_database)::text, \
              (SELECT sum(temp_files) FROM pg_stat_database)::text, \
              (SELECT pg_size_pretty(sum(temp_bytes)::bigint) FROM pg_stat_database), \
              (SELECT sum(tup_returned) FROM pg_stat_database)::text, \
              (SELECT sum(tup_inserted) FROM pg_stat_database)::text, \
              (SELECT sum(tup_updated) FROM pg_stat_database)::text, \
              (SELECT sum(tup_deleted) FROM pg_stat_database)::text, \
              (SELECT to_char(max(now() - query_start), 'HH24:MI:SS') FROM pg_stat_activity WHERE state = 'active' AND pid <> pg_backend_pid()), \
              (SELECT to_char(max(now() - xact_start), 'HH24:MI:SS') FROM pg_stat_activity WHERE xact_start IS NOT NULL AND pid <> pg_backend_pid()), \
              (SELECT count(*) FROM pg_stat_replication)::text, \
              pg_is_in_recovery()::text, \
              current_setting('shared_buffers'), current_setting('work_mem'), current_setting('effective_cache_size'), \
              (SELECT count(*) FROM pg_database WHERE NOT datistemplate)::text, \
              (SELECT sum(n_dead_tup) FROM pg_stat_user_tables)::text"
    );
    let r = s.text_rows(None, &sql, &[]).await?;
    let r = r.into_iter().next().unwrap_or_default();
    let g = |i: usize| r.get(i).cloned().flatten().unwrap_or_else(|| "–".into());
    Ok(vec![
        Metric::new("Server", "Version", g(0)),
        Metric::new("Server", "Uptime", g(1)),
        Metric::new("Server", "In recovery (standby)", g(22)),
        Metric::new("Server", "Replicas connected", g(21)),
        Metric::new("Server", "Databases", g(26)),
        Metric::new("Connections", "Total", g(2)),
        Metric::new("Connections", "Max", g(3)),
        Metric::new("Connections", "Active", g(4)),
        Metric::new("Connections", "Idle in transaction", g(5)).hint("Holds locks and snapshots; investigate if high"),
        Metric::new("Connections", "Waiting on locks", g(6)),
        Metric::new("Connections", "Longest running query", g(19)),
        Metric::new("Connections", "Longest open transaction", g(20)),
        Metric::new("Storage", "Current database size", g(7)),
        Metric::new("Storage", "Current database bytes", g(8)),
        Metric::new("Storage", "Dead tuples (user tables)", g(27)).hint("Reclaimed by (auto)vacuum"),
        Metric::new("Throughput", "Cache hit ratio (%)", g(9)).hint("Below ~99% on OLTP suggests too little memory"),
        Metric::new("Throughput", "Commits", g(10)),
        Metric::new("Throughput", "Rollbacks", g(11)),
        Metric::new("Throughput", "Rows returned", g(15)),
        Metric::new("Throughput", "Rows inserted", g(16)),
        Metric::new("Throughput", "Rows updated", g(17)),
        Metric::new("Throughput", "Rows deleted", g(18)),
        Metric::new("Problems", "Deadlocks", g(12)),
        Metric::new("Problems", "Temp files", g(13)).hint("Queries spilling to disk; consider work_mem"),
        Metric::new("Problems", "Temp bytes", g(14)),
        Metric::new("Configuration", "shared_buffers", g(23)),
        Metric::new("Configuration", "work_mem", g(24)),
        Metric::new("Configuration", "effective_cache_size", g(25)),
    ])
}

pub async fn principals(s: &PgSession) -> Result<ResultSet> {
    s.text_result(
        None,
        "SELECT r.rolname::text AS name, r.rolcanlogin::text AS can_login, r.rolsuper::text AS superuser, \
         r.rolcreatedb::text AS create_db, r.rolcreaterole::text AS create_role, r.rolreplication::text AS replication, \
         CASE WHEN r.rolconnlimit < 0 THEN 'unlimited' ELSE r.rolconnlimit::text END AS connection_limit, \
         r.rolvaliduntil::text AS valid_until, \
         (SELECT string_agg(g.rolname, ', ' ORDER BY g.rolname) FROM pg_auth_members m JOIN pg_roles g ON g.oid = m.roleid WHERE m.member = r.oid) AS member_of \
         FROM pg_roles r WHERE r.rolname NOT LIKE 'pg\\_%' ORDER BY r.rolcanlogin DESC, r.rolname",
        &[],
    )
    .await
}

pub async fn privileges(s: &PgSession, principal: &str) -> Result<ResultSet> {
    let p = principal.to_string();
    s.text_result(
        None,
        "SELECT 'database' AS object_type, d.datname::text AS object, \
           concat_ws(', ', CASE WHEN has_database_privilege($1, d.datname, 'CONNECT') THEN 'CONNECT' END, \
                           CASE WHEN has_database_privilege($1, d.datname, 'CREATE') THEN 'CREATE' END, \
                           CASE WHEN has_database_privilege($1, d.datname, 'TEMP') THEN 'TEMPORARY' END) AS privileges \
         FROM pg_database d WHERE NOT d.datistemplate \
         UNION ALL \
         SELECT 'schema', n.nspname::text, concat_ws(', ', CASE WHEN has_schema_privilege($1, n.oid, 'USAGE') THEN 'USAGE' END, \
                                                         CASE WHEN has_schema_privilege($1, n.oid, 'CREATE') THEN 'CREATE' END) \
         FROM pg_namespace n WHERE n.nspname NOT LIKE 'pg\\_%' AND n.nspname <> 'information_schema' \
         UNION ALL \
         SELECT 'table', g.table_schema || '.' || g.table_name, string_agg(g.privilege_type, ', ' ORDER BY g.privilege_type) \
         FROM information_schema.role_table_grants g WHERE g.grantee = $1 \
           AND g.table_schema NOT IN ('pg_catalog', 'information_schema') GROUP BY g.table_schema, g.table_name \
         ORDER BY 1, 2",
        &[&p],
    )
    .await
}

pub async fn create_principal(s: &PgSession, spec: PrincipalSpec) -> Result<()> {
    if spec.name.trim().is_empty() {
        return Err(Error::invalid("Role name is required"));
    }
    let mut sql = format!("CREATE ROLE {} WITH", D.quote_ident(&spec.name));
    sql.push_str(if spec.can_login { " LOGIN" } else { " NOLOGIN" });
    // Only the non-default options: the NO… forms are the defaults everywhere, and
    // openGauss rejects NOSUPERUSER (its superuser option is SYSADMIN).
    if spec.superuser {
        sql.push_str(if s.is_gauss() { " SYSADMIN" } else { " SUPERUSER" });
    }
    if spec.create_db {
        sql.push_str(" CREATEDB");
    }
    if spec.create_role {
        sql.push_str(" CREATEROLE");
    }
    if let Some(pw) = spec.password.filter(|p| !p.is_empty()) {
        sql.push_str(&format!(" PASSWORD {}", D.quote_literal(&pw)));
    }
    s.run(None, &sql).await.map(|_| ())
}

pub async fn grant(s: &PgSession, spec: GrantSpec, revoke: bool) -> Result<()> {
    if spec.privileges.is_empty() {
        return Err(Error::invalid("Select at least one privilege"));
    }
    let privs = spec.privileges.join(", ");
    let (on, db) = match &spec.target {
        GrantTarget::Database(d) => (format!("DATABASE {}", D.quote_ident(d)), None),
        GrantTarget::Schema(sc) => (format!("SCHEMA {}", D.quote_ident(sc)), None),
        GrantTarget::AllTablesInSchema(sc) => (format!("ALL TABLES IN SCHEMA {}", D.quote_ident(sc)), None),
        GrantTarget::Table(p) => (format!("TABLE {}", D.qualified(p.schema.as_deref(), p.name()?)), p.database.clone()),
    };
    let who = D.quote_ident(&spec.principal);
    let sql = if revoke { format!("REVOKE {privs} ON {on} FROM {who}") } else { format!("GRANT {privs} ON {on} TO {who}{}", if spec.with_grant_option { " WITH GRANT OPTION" } else { "" }) };
    s.run(db.as_deref(), &sql).await.map(|_| ())
}

pub async fn create_database(s: &PgSession, spec: CreateDatabaseSpec) -> Result<()> {
    if spec.name.trim().is_empty() {
        return Err(Error::invalid("Database name is required"));
    }
    let mut sql = format!("CREATE DATABASE {}", D.quote_ident(&spec.name));
    if let Some(o) = spec.owner.filter(|x| !x.is_empty()) {
        sql.push_str(&format!(" OWNER {}", D.quote_ident(&o)));
    }
    if let Some(t) = spec.template.filter(|x| !x.is_empty()) {
        sql.push_str(&format!(" TEMPLATE {}", D.quote_ident(&t)));
    }
    if let Some(e) = spec.encoding.filter(|x| !x.is_empty()) {
        sql.push_str(&format!(" ENCODING {}", D.quote_literal(&e)));
    }
    if let Some(c) = spec.collation.filter(|x| !x.is_empty()) {
        sql.push_str(&format!(" LC_COLLATE {} LC_CTYPE {}", D.quote_literal(&c), D.quote_literal(&c)));
    }
    s.run(None, &sql).await.map(|_| ())
}

pub async fn drop_object(s: &PgSession, kind: ObjectKind, path: &ObjPath, cascade: bool) -> Result<()> {
    let tail = if cascade { " CASCADE" } else { "" };
    let schema = path.schema.as_deref();
    let db = path.database.clone();
    let sql = match kind {
        ObjectKind::Database => {
            let name = path.database.clone().ok_or_else(|| Error::invalid("Database name missing"))?;
            if name == s.target.database {
                return Err(Error::invalid("This is the connection's default database. Connect to another database (edit the connection) to drop it."));
            }
            s.drop_pool(&name);
            let force = if cascade && s.version_num() >= 130000 { " WITH (FORCE)" } else { "" };
            let sql = format!("DROP DATABASE {}{force}", D.quote_ident(&name));
            return s.run(None, &sql).await.map(|_| ());
        }
        ObjectKind::Schema => format!("DROP SCHEMA {}{tail}", D.quote_ident(schema.unwrap_or_default())),
        ObjectKind::Function | ObjectKind::Procedure => {
            format!("DROP {} {}({}){tail}", kind.sql_keyword().unwrap_or_default(), D.qualified(schema, path.name()?), path.sub.clone().unwrap_or_default())
        }
        ObjectKind::Index => format!("DROP INDEX {}{tail}", D.qualified(schema, path.sub.as_deref().unwrap_or_default())),
        ObjectKind::Trigger => format!("DROP TRIGGER {} ON {}{tail}", D.quote_ident(path.sub.as_deref().unwrap_or_default()), D.qualified(schema, path.name()?)),
        ObjectKind::Constraint => format!("ALTER TABLE {} DROP CONSTRAINT {}{tail}", D.qualified(schema, path.name()?), D.quote_ident(path.sub.as_deref().unwrap_or_default())),
        ObjectKind::Column => format!("ALTER TABLE {} DROP COLUMN {}{tail}", D.qualified(schema, path.name()?), D.quote_ident(path.sub.as_deref().unwrap_or_default())),
        k => match k.sql_keyword() {
            Some(kw) => format!("DROP {kw} {}{tail}", D.qualified(schema, path.name()?)),
            None => return Err(Error::unsupported(format!("Cannot drop a {}", k.label()))),
        },
    };
    s.run(db.as_deref(), &sql).await.map(|_| ())
}

/// Locate a PostgreSQL client tool on PATH or in common install locations.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for extra in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/Applications/Postgres.app/Contents/Versions/latest/bin", "/usr/lib/postgresql/17/bin", "/usr/lib/postgresql/16/bin"] {
        dirs.push(PathBuf::from(extra));
    }
    // Windows: the EDB installer uses C:\Program Files\PostgreSQL\<version>\bin
    // and does not add it to PATH. Newest version first. Scanned once, because
    // capabilities() (and so this function) runs every frame.
    static WINDOWS_DIRS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    dirs.extend(
        WINDOWS_DIRS
            .get_or_init(|| openvclient_core::process::program_files().iter().flat_map(|pf| openvclient_core::process::versioned_bin_dirs(&pf.join("PostgreSQL"), "")).collect())
            .iter()
            .cloned(),
    );
    dirs.into_iter().map(|d| d.join(&exe)).find(|p| p.is_file())
}

fn tool_command(s: &PgSession, tool: &str) -> Result<tokio::process::Command> {
    let path = find_tool(tool).ok_or_else(|| Error::unsupported(format!("{tool} was not found on PATH. Install the PostgreSQL client tools.")))?;
    let mut cmd = openvclient_core::process::command(path);
    cmd.arg("-h").arg(&s.target.host).arg("-p").arg(s.target.port.to_string()).arg("-U").arg(&s.target.user);
    cmd.env("PGSSLMODE", &s.target.sslmode).env("PGAPPNAME", "OpenVClient");
    if let Some(pw) = &s.target.password {
        cmd.env("PGPASSWORD", pw);
    }
    cmd.stdin(std::process::Stdio::null()).kill_on_drop(true);
    Ok(cmd)
}

async fn run_tool(mut cmd: tokio::process::Command, what: &str) -> Result<String> {
    let out = cmd.output().await.map_err(|e| Error::io(format!("Could not start {what}: {e}")))?;
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        return Err(Error::server(format!("{what} failed ({})", out.status)).with_detail(stderr));
    }
    Ok(stderr)
}

/// `-d` takes a connection string, so pass the name as one (a database
/// called "host=evil" must not redirect the dump).
fn dbname_arg(db: &str) -> String {
    format!("dbname='{}'", db.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Refuse plain dumps that would run programs through psql meta-commands.
fn check_plain_dump(path: &std::path::Path) -> Result<()> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(format!("{}: {e}", path.display())))?;
    if let Some((line, cmd)) = openvclient_core::sqltext::dangerous_dump_line(&text, openvclient_core::sqltext::DumpClient::Psql) {
        return Err(Error::invalid(format!("Refusing to restore: line {line} runs a program on this computer ({cmd}). Remove psql shell commands from the file if you trust it.")));
    }
    Ok(())
}

pub async fn backup(s: &PgSession, req: BackupRequest) -> Result<String> {
    let db = req.database.clone().unwrap_or_else(|| s.target.database.clone());
    let plain = req.path.extension().is_some_and(|e| e == "sql");
    let mut cmd = tool_command(s, "pg_dump")?;
    cmd.arg("-d").arg(dbname_arg(&db)).arg("-f").arg(&req.path);
    cmd.arg(if plain { "--format=plain" } else { "--format=custom" });
    let t = Instant::now();
    let warnings = run_tool(cmd, "pg_dump").await?;
    let size = std::fs::metadata(&req.path).map(|m| m.len()).unwrap_or(0);
    let mut msg = format!("pg_dump wrote {} ({} format, {} bytes) in {}", req.path.display(), if plain { "plain SQL" } else { "custom" }, size, openvclient_core::elapsed_ms(t.elapsed()));
    if !warnings.is_empty() {
        msg.push_str(&format!("\n{warnings}"));
    }
    Ok(msg)
}

pub async fn restore(s: &PgSession, req: BackupRequest) -> Result<String> {
    let db = req.database.clone().unwrap_or_else(|| s.target.database.clone());
    let mut head = [0u8; 5];
    let is_custom = std::fs::File::open(&req.path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).map(|_| &head == b"PGDMP").unwrap_or(false);
    let t = Instant::now();
    let (warnings, tool) = if is_custom {
        let mut cmd = tool_command(s, "pg_restore")?;
        cmd.arg("-d").arg(dbname_arg(&db)).arg("--no-owner").arg(&req.path);
        (run_tool(cmd, "pg_restore").await?, "pg_restore")
    } else {
        check_plain_dump(&req.path)?;
        let mut cmd = tool_command(s, "psql")?;
        cmd.arg("-d").arg(dbname_arg(&db)).arg("-v").arg("ON_ERROR_STOP=1").arg("-q").arg("-f").arg(&req.path);
        (run_tool(cmd, "psql").await?, "psql")
    };
    let mut msg = format!("{tool} restored {} into {db} in {}", req.path.display(), openvclient_core::elapsed_ms(t.elapsed()));
    if !warnings.is_empty() {
        msg.push_str(&format!("\n{warnings}"));
    }
    Ok(msg)
}
