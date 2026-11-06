//! ClickHouse over its HTTP interface. One request = one statement (the
//! server rejects stacked statements), consoles use server-side sessions.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use openvclient_core::explain::{ExplainResult, PlanNode};
use openvclient_core::sql::*;
use openvclient_core::sqltext::{Dialect, leading_keyword};
use openvclient_core::{
    Category, ColumnMeta, ConnectParams, Driver, DriverInfo, Error, FieldKind, FieldSpec, Result, ResultSet, ServerInfo,
    ServiceSession, SessionHandle, Value,
};
use parking_lot::Mutex;
use reqwest::Method;
use serde_json::Value as J;

use crate::Http;

// Backtick identifiers and backslash escapes match ClickHouse's lexer.
const D: Dialect = Dialect::MySql;

pub struct ClickHouseDriver {
    info: DriverInfo,
}

impl Default for ClickHouseDriver {
    fn default() -> Self {
        Self {
            info: DriverInfo {
                id: "clickhouse",
                name: "ClickHouse",
                category: Category::Analytical,
                description: "ClickHouse server and ClickHouse Cloud (HTTP interface)",
                badge: "CH",
                fields: vec![
                    FieldSpec::text("host", "Host").default("localhost").required(),
                    FieldSpec::text("port", "HTTP port").kind(FieldKind::Number).default("8123"),
                    FieldSpec::text("user", "User").default("default"),
                    FieldSpec::text("password", "Password").kind(FieldKind::Secret),
                    FieldSpec::text("database", "Default database").default("default"),
                    FieldSpec::text("tls", "HTTPS").kind(FieldKind::Bool).default("false").help("ClickHouse Cloud uses HTTPS on port 8443"),
                ],
            },
        }
    }
}

#[derive(Clone)]
struct Ch {
    http: Http,
    database: String,
    read_only: bool,
}

struct Reply {
    rs: Option<ResultSet>,
    written: u64,
}

impl Ch {
    fn base_req(&self, database: Option<&str>, session: Option<&str>, query_id: Option<&str>) -> reqwest::RequestBuilder {
        let mut q: Vec<(&str, String)> = vec![("database", database.unwrap_or(&self.database).to_string()), ("wait_end_of_query", "1".into())];
        if let Some(s) = session {
            q.push(("session_id", s.to_string()));
            q.push(("session_timeout", "3600".into()));
        }
        if let Some(id) = query_id {
            q.push(("query_id", id.to_string()));
        }
        if self.read_only {
            q.push(("readonly", "2".into()));
        }
        // ClickHouse reads credentials from these headers.
        let mut r = self.http.client.request(Method::POST, format!("{}/", self.http.base)).query(&q);
        if let Some(u) = &self.http.user {
            r = r.header("X-ClickHouse-User", u);
        }
        if let Some(p) = &self.http.password {
            r = r.header("X-ClickHouse-Key", p);
        }
        r
    }

    /// Run one statement. Row-returning statements come back as JSONCompact.
    async fn query(&self, sql: &str, database: Option<&str>, session: Option<&str>, query_id: Option<&str>, max: usize) -> Result<Reply> {
        let sql = sql.trim().trim_end_matches(';').trim();
        let kw = leading_keyword(sql);
        let rows = matches!(kw.as_str(), "SELECT" | "WITH" | "SHOW" | "DESCRIBE" | "DESC" | "EXISTS" | "EXPLAIN" | "VALUES");
        let has_format = sql.to_ascii_uppercase().split_whitespace().rev().nth(1) == Some("FORMAT");
        let body = if rows && !has_format { format!("{sql}\nFORMAT JSONCompact") } else { sql.to_string() };
        let mut r = self.base_req(database, session, query_id);
        if rows && max < usize::MAX {
            r = r.query(&[("max_result_rows", (max + 1).to_string()), ("result_overflow_mode", "break".to_string())]);
        }
        let (status, text) = self.http.send(r.body(body)).await?;
        if status >= 400 {
            let first = text.lines().next().unwrap_or("").to_string();
            let msg = first.split(". (").next().unwrap_or(&first).trim_start_matches("Code: ").to_string();
            if text.contains("QUERY_WAS_CANCELLED") {
                return Err(Error::cancelled());
            }
            return Err(Error::server(msg).with_detail(text.chars().take(1500).collect::<String>()));
        }
        if !rows || has_format {
            return Ok(Reply { rs: if has_format { Some(ResultSet::from_text_rows(&["result"], vec![vec![Some(text)]])) } else { None }, written: 0 });
        }
        let v: J = serde_json::from_str(&text).map_err(|e| Error::other(format!("Unexpected response: {e}")))?;
        let meta = v.get("meta").and_then(|m| m.as_array()).cloned().unwrap_or_default();
        let mut rs = ResultSet::new(
            meta.iter().map(|c| ColumnMeta::new(c.get("name").and_then(|n| n.as_str()).unwrap_or(""), c.get("type").and_then(|n| n.as_str()).unwrap_or(""))).collect(),
        );
        for row in v.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default() {
            if rs.rows.len() >= max {
                rs.truncated = true;
                break;
            }
            rs.rows.push(
                row.as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| match c {
                        J::Null => Value::Null,
                        J::Bool(b) => Value::Bool(b),
                        J::Number(n) => n.as_i64().map(Value::Int).unwrap_or_else(|| Value::Float(n.as_f64().unwrap_or(f64::NAN))),
                        J::String(s) => Value::Text(s),
                        other => Value::Text(other.to_string()),
                    })
                    .collect(),
            );
        }
        if v.get("rows_before_limit_at_least").is_some() || v.get("rows").and_then(|r| r.as_u64()).is_some_and(|r| r as usize > max) {
            rs.truncated = rs.truncated || rs.rows.len() >= max;
        }
        Ok(Reply { rs: Some(rs), written: 0 })
    }

    async fn rows(&self, sql: &str, database: Option<&str>) -> Result<Vec<Vec<Option<String>>>> {
        let r = self.query(sql, database, None, None, usize::MAX).await?;
        Ok(r.rs.map(|rs| rs.rows.into_iter().map(|r| r.into_iter().map(|v| v.to_text()).collect()).collect()).unwrap_or_default())
    }
}

#[async_trait]
impl Driver for ClickHouseDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }

    async fn connect(&self, p: ConnectParams) -> Result<SessionHandle> {
        let mut http = Http::from_params(&p, 8123)?;
        if http.user.is_none() {
            http.user = Some("default".into());
        }
        let ch = Ch { http, database: p.get_or("database", "default").to_string(), read_only: p.read_only };
        let r = ch.rows("SELECT version(), currentUser(), timezone(), uptime()", None).await?;
        let r = r.into_iter().next().unwrap_or_default();
        let g = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
        let info = ServerInfo { product: format!("ClickHouse {}", g(0)), version: g(0), details: vec![("User".into(), g(1)), ("Time zone".into(), g(2))] };
        Ok(SessionHandle::Sql(Arc::new(ChSession { ch, info })))
    }
}

struct ChSession {
    ch: Ch,
    info: ServerInfo,
}

fn lit(s: &str) -> String {
    D.quote_literal(s)
}

#[async_trait]
impl ServiceSession for ChSession {
    fn server_info(&self) -> ServerInfo {
        self.info.clone()
    }
    async fn ping(&self) -> Result<Duration> {
        let t = Instant::now();
        self.ch.rows("SELECT 1", None).await?;
        Ok(t.elapsed())
    }
    async fn close(&self) {}
}

#[async_trait]
impl SqlSession for ChSession {
    fn dialect(&self) -> Dialect {
        D
    }

    fn capabilities(&self) -> SqlCapabilities {
        let rw = !self.ch.read_only;
        SqlCapabilities {
            multiple_databases: true,
            create_database: rw,
            drop_database: rw,
            explain: true,
            activity: true,
            cancel_query: true,
            terminate_session: false,
            metrics: true,
            users: true,
            truncate: rw,
            ..Default::default()
        }
    }

    fn default_database(&self) -> Option<String> {
        Some(self.ch.database.clone())
    }

    async fn children(&self, node: Option<(ObjectKind, ObjPath)>) -> Result<Vec<TreeNode>> {
        let Some((kind, path)) = node else {
            let rows = self.ch.rows("SELECT d.name, d.engine, (SELECT count() FROM system.tables t WHERE t.database = d.name) FROM system.databases d ORDER BY d.name IN ('system','INFORMATION_SCHEMA','information_schema'), d.name", None).await?;
            return Ok(rows
                .into_iter()
                .map(|r| {
                    let name = r[0].clone().unwrap_or_default();
                    TreeNode::new(ObjectKind::Database, name.clone(), ObjPath::db(name)).detail(format!("{} · {} tables", r[1].clone().unwrap_or_default(), r[2].clone().unwrap_or_default()))
                })
                .collect());
        };
        let db = path.database.clone().unwrap_or_else(|| self.ch.database.clone());
        match kind {
            ObjectKind::Database => Ok(vec![TreeNode::folder(FolderKind::Tables, ObjPath::db(&db)), TreeNode::folder(FolderKind::Views, ObjPath::db(&db))]),
            ObjectKind::Folder(f @ (FolderKind::Tables | FolderKind::Views)) => {
                let cond = if f == FolderKind::Views { "engine IN ('View','MaterializedView','LiveView','WindowView')" } else { "engine NOT IN ('View','MaterializedView','LiveView','WindowView')" };
                let rows = self.ch.rows(&format!("SELECT name, engine, total_rows, formatReadableSize(total_bytes) FROM system.tables WHERE database = {} AND {cond} AND NOT is_temporary ORDER BY name", lit(&db)), None).await?;
                Ok(rows
                    .into_iter()
                    .map(|r| {
                        let name = r[0].clone().unwrap_or_default();
                        let mut d = vec![r[1].clone().unwrap_or_default()];
                        if let Some(n) = r[2].clone() {
                            d.push(format!("{n} rows"));
                        }
                        if let Some(sz) = r[3].clone() {
                            d.push(sz);
                        }
                        let k = if f == FolderKind::Views { ObjectKind::View } else { ObjectKind::Table };
                        TreeNode::new(k, name.clone(), ObjPath::db(&db).with_name(name)).detail(d.join(" · "))
                    })
                    .collect())
            }
            ObjectKind::Table | ObjectKind::View => Ok(vec![TreeNode::folder(FolderKind::Columns, path)]),
            ObjectKind::Folder(FolderKind::Columns) => Ok(self
                .columns(&path)
                .await?
                .into_iter()
                .map(|c| {
                    let d = format!("{}{}", c.data_type, if c.primary_key { " · key" } else { "" });
                    TreeNode::new(ObjectKind::Column, c.name.clone(), path.clone().with_sub(c.name)).detail(d)
                })
                .collect()),
            _ => Ok(Vec::new()),
        }
    }

    async fn ddl(&self, kind: ObjectKind, path: &ObjPath) -> Result<String> {
        let db = path.database.clone().unwrap_or_else(|| self.ch.database.clone());
        let sql = match kind {
            ObjectKind::Database => format!("SHOW CREATE DATABASE {}", D.quote_ident(&db)),
            _ => format!("SHOW CREATE TABLE {}", D.qualified(Some(&db), path.name()?)),
        };
        let r = self.ch.rows(&sql, None).await?;
        r.into_iter().next().and_then(|r| r.into_iter().next().flatten()).map(|s| format!("{s};\n")).ok_or_else(|| Error::invalid("Object not found"))
    }

    async fn columns(&self, path: &ObjPath) -> Result<Vec<ColumnInfo>> {
        let db = path.database.clone().unwrap_or_else(|| self.ch.database.clone());
        let rows = self
            .ch
            .rows(
                &format!("SELECT name, type, default_kind, default_expression, is_in_primary_key, comment FROM system.columns WHERE database = {} AND table = {} ORDER BY position", lit(&db), lit(path.name()?)),
                None,
            )
            .await?;
        if rows.is_empty() {
            return Err(Error::invalid(format!("{} not found", path.display())));
        }
        Ok(rows
            .into_iter()
            .map(|r| ColumnInfo {
                name: r[0].clone().unwrap_or_default(),
                nullable: r[1].as_deref().is_some_and(|t| t.starts_with("Nullable")),
                data_type: r[1].clone().unwrap_or_default(),
                default: r[3].clone().filter(|d| !d.is_empty()).map(|d| format!("{} {d}", r[2].clone().unwrap_or_default())),
                primary_key: r[4].as_deref() == Some("1"),
                auto_increment: false,
                comment: r[5].clone().filter(|c| !c.is_empty()),
            })
            .collect())
    }

    async fn table_page(&self, q: TableQuery) -> Result<TablePage> {
        let db = q.path.database.clone().unwrap_or_else(|| self.ch.database.clone());
        let mut sql = format!("SELECT * FROM {}", D.qualified(Some(&db), q.path.name()?));
        if let Some(f) = q.filter.as_deref().filter(|f| !f.trim().is_empty()) {
            sql.push_str(&format!(" WHERE {f}"));
        }
        if !q.order_by.is_empty() {
            let o: Vec<String> = q.order_by.iter().map(|(c, d)| format!("{} {}", D.quote_ident(c), if *d == SortDir::Asc { "ASC" } else { "DESC" })).collect();
            sql.push_str(&format!(" ORDER BY {}", o.join(", ")));
        }
        sql.push_str(&format!(" LIMIT {} OFFSET {}", q.limit, q.offset));
        let rows = self.ch.query(&sql, None, None, None, usize::MAX).await?.rs.unwrap_or_default();
        Ok(TablePage {
            rows,
            key_columns: Vec::new(),
            row_ids: None,
            read_only_reason: Some("ClickHouse changes rows through asynchronous mutations; use ALTER TABLE … UPDATE / DELETE in a SQL editor".into()),
        })
    }

    async fn count_rows(&self, path: &ObjPath, filter: Option<&str>) -> Result<u64> {
        let db = path.database.clone().unwrap_or_else(|| self.ch.database.clone());
        let mut sql = format!("SELECT count() FROM {}", D.qualified(Some(&db), path.name()?));
        if let Some(f) = filter.filter(|f| !f.trim().is_empty()) {
            sql.push_str(&format!(" WHERE {f}"));
        }
        let r = self.ch.rows(&sql, None).await?;
        Ok(r.first().and_then(|r| r[0].as_ref()).and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    async fn apply_changes(&self, _path: &ObjPath, _changes: Vec<RowChange>) -> Result<u64> {
        Err(Error::unsupported("ClickHouse rows are changed with ALTER TABLE … UPDATE / DELETE mutations"))
    }

    async fn catalog(&self, database: Option<&str>) -> Result<Catalog> {
        let db = database.map(str::to_owned).unwrap_or_else(|| self.ch.database.clone());
        let rows = self.ch.rows(&format!("SELECT table, name FROM system.columns WHERE database = {} ORDER BY table, position LIMIT 50000", lit(&db)), None).await?;
        let mut cat = Catalog::default();
        for r in rows {
            let t = r[0].clone().unwrap_or_default();
            let e = cat.columns.entry(t.clone()).or_default();
            if e.is_empty() {
                cat.objects.push((String::new(), t, ObjectKind::Table));
            }
            if let Some(c) = r[1].clone() {
                e.push(c);
            }
        }
        cat.schemas = self.ch.rows("SELECT name FROM system.databases", None).await?.into_iter().filter_map(|r| r.into_iter().next().flatten()).collect();
        cat.functions = self.ch.rows("SELECT name FROM system.functions WHERE NOT is_aggregate LIMIT 2000", None).await?.into_iter().filter_map(|r| r.into_iter().next().flatten()).collect();
        Ok(cat)
    }

    async fn open_console(&self, database: Option<String>) -> Result<Arc<dyn SqlConsole>> {
        let session = format!("openvclient-{}", random_id());
        Ok(Arc::new(ChConsole { ch: self.ch.clone(), session, database: Mutex::new(database.unwrap_or_else(|| self.ch.database.clone())), running: Mutex::new(None), rt: tokio::runtime::Handle::current() }))
    }

    async fn run(&self, database: Option<&str>, sql: &str) -> Result<u64> {
        for span in openvclient_core::sqltext::split_statements(D, sql) {
            self.ch.query(&sql[span.start..span.end], database, None, None, 0).await?;
        }
        Ok(0)
    }

    async fn drop_object(&self, kind: ObjectKind, path: &ObjPath, _cascade: bool) -> Result<()> {
        let db = path.database.clone().unwrap_or_else(|| self.ch.database.clone());
        let sql = match kind {
            ObjectKind::Database => format!("DROP DATABASE {}", D.quote_ident(&db)),
            ObjectKind::Table => format!("DROP TABLE {}", D.qualified(Some(&db), path.name()?)),
            ObjectKind::View => format!("DROP VIEW {}", D.qualified(Some(&db), path.name()?)),
            ObjectKind::Column => format!("ALTER TABLE {} DROP COLUMN {}", D.qualified(Some(&db), path.name()?), D.quote_ident(path.sub.as_deref().unwrap_or_default())),
            k => return Err(Error::unsupported(format!("Cannot drop a {} in ClickHouse", k.label()))),
        };
        self.run(None, &sql).await.map(|_| ())
    }

    async fn truncate(&self, path: &ObjPath) -> Result<()> {
        let db = path.database.clone().unwrap_or_else(|| self.ch.database.clone());
        self.run(None, &format!("TRUNCATE TABLE {}", D.qualified(Some(&db), path.name()?))).await.map(|_| ())
    }

    async fn create_database(&self, spec: CreateDatabaseSpec) -> Result<()> {
        if spec.name.trim().is_empty() {
            return Err(Error::invalid("Database name is required"));
        }
        self.run(None, &format!("CREATE DATABASE {}", D.quote_ident(&spec.name))).await.map(|_| ())
    }

    async fn activity(&self) -> Result<ActivitySnapshot> {
        let rows = self
            .ch
            .query(
                "SELECT query_id, user, round(elapsed, 2) AS seconds, read_rows, formatReadableSize(memory_usage) AS memory, address, substring(query, 1, 500) AS query FROM system.processes ORDER BY elapsed DESC",
                None,
                None,
                None,
                usize::MAX,
            )
            .await?
            .rs
            .unwrap_or_default();
        Ok(ActivitySnapshot { rows, id_column: 0 })
    }

    async fn kill(&self, id: &str, _mode: KillMode) -> Result<()> {
        self.run(None, &format!("KILL QUERY WHERE query_id = {} ASYNC", lit(id))).await.map(|_| ())
    }

    async fn metrics(&self) -> Result<Vec<Metric>> {
        let r = self
            .ch
            .rows(
                "SELECT version(), formatReadableTimeDelta(uptime()), \
                 (SELECT value FROM system.metrics WHERE metric = 'Query'), \
                 (SELECT value FROM system.metrics WHERE metric = 'TCPConnection') + (SELECT value FROM system.metrics WHERE metric = 'HTTPConnection'), \
                 (SELECT formatReadableSize(value) FROM system.metrics WHERE metric = 'MemoryTracking'), \
                 (SELECT count() FROM system.parts WHERE active), \
                 (SELECT formatReadableSize(sum(bytes_on_disk)) FROM system.parts WHERE active), \
                 (SELECT sum(rows) FROM system.parts WHERE active), \
                 (SELECT value FROM system.events WHERE event = 'Query'), \
                 (SELECT value FROM system.events WHERE event = 'SelectQuery'), \
                 (SELECT value FROM system.events WHERE event = 'InsertQuery'), \
                 (SELECT value FROM system.events WHERE event = 'FailedQuery'), \
                 (SELECT count() FROM system.mutations WHERE NOT is_done), \
                 (SELECT count() FROM system.replicas WHERE is_readonly)",
                None,
            )
            .await?;
        let r = r.into_iter().next().unwrap_or_default();
        let g = |i: usize| r.get(i).cloned().flatten().unwrap_or_else(|| "0".into());
        Ok(vec![
            Metric::new("Server", "Version", g(0)),
            Metric::new("Server", "Uptime", g(1)),
            Metric::new("Load", "Running queries", g(2)),
            Metric::new("Load", "Open connections", g(3)),
            Metric::new("Load", "Memory tracked", g(4)),
            Metric::new("Storage", "Active parts", g(5)).hint("Many small parts slow merges and queries"),
            Metric::new("Storage", "Data on disk", g(6)),
            Metric::new("Storage", "Rows stored", g(7)),
            Metric::new("Throughput", "Queries (since start)", g(8)),
            Metric::new("Throughput", "SELECT queries", g(9)),
            Metric::new("Throughput", "INSERT queries", g(10)),
            Metric::new("Problems", "Failed queries", g(11)),
            Metric::new("Problems", "Pending mutations", g(12)),
            Metric::new("Problems", "Read-only replicas", g(13)),
        ])
    }

    async fn principals(&self) -> Result<ResultSet> {
        Ok(self.ch.query("SELECT name, storage, auth_type, arrayStringConcat(host_ip, ', ') AS hosts, arrayStringConcat(default_roles_list, ', ') AS default_roles FROM system.users ORDER BY name", None, None, None, usize::MAX).await?.rs.unwrap_or_default())
    }

    async fn privileges(&self, principal: &str, _host: Option<&str>) -> Result<ResultSet> {
        Ok(self.ch.query(&format!("SELECT access_type, database, table, column, is_partial_revoke, grant_option FROM system.grants WHERE user_name = {}", lit(principal)), None, None, None, usize::MAX).await?.rs.unwrap_or_default())
    }

    async fn create_principal(&self, spec: PrincipalSpec) -> Result<()> {
        let pw = spec.password.clone().unwrap_or_default();
        self.run(None, &format!("CREATE USER {} IDENTIFIED WITH sha256_password BY {}", D.quote_ident(&spec.name), lit(&pw))).await.map(|_| ())
    }

    async fn drop_principal(&self, name: &str, _host: Option<&str>) -> Result<()> {
        self.run(None, &format!("DROP USER {}", D.quote_ident(name))).await.map(|_| ())
    }
}

fn random_id() -> String {
    format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0))
}

struct ChConsole {
    ch: Ch,
    session: String,
    database: Mutex<String>,
    running: Mutex<Option<String>>,
    rt: tokio::runtime::Handle,
}

#[async_trait]
impl SqlConsole for ChConsole {
    async fn execute(&self, statements: Vec<String>, opts: ExecOptions) -> Vec<StatementResult> {
        let mut out = Vec::new();
        for sql in statements {
            let t = Instant::now();
            let qid = format!("openvclient-{}", random_id());
            *self.running.lock() = Some(qid.clone());
            let db = self.database.lock().clone();
            let kw = leading_keyword(&sql);
            let r = self.ch.query(&sql, Some(&db), Some(&self.session), Some(&qid), opts.max_rows).await;
            *self.running.lock() = None;
            let outcome = r.map(|rep| match rep.rs {
                Some(rs) => StatementOutcome::Rows(rs),
                None if kw == "INSERT" => StatementOutcome::Done(format!("INSERT completed{}", if rep.written > 0 { format!(" ({} rows)", rep.written) } else { String::new() })),
                None => StatementOutcome::Done(format!("{kw} completed")),
            });
            if outcome.is_ok() && kw == "USE" {
                if let Some(name) = sql.split_whitespace().nth(1) {
                    *self.database.lock() = name.trim_matches(['`', '"', ';']).to_string();
                }
            }
            let failed = outcome.is_err();
            out.push(StatementResult { sql, elapsed: t.elapsed(), outcome });
            if failed {
                break;
            }
        }
        out
    }

    async fn explain(&self, sql: &str, analyze: bool) -> Result<ExplainResult> {
        if analyze {
            return Err(Error::unsupported("ClickHouse has no EXPLAIN ANALYZE; use EXPLAIN PIPELINE or system.query_log"));
        }
        let db = self.database.lock().clone();
        let statement = format!("EXPLAIN indexes = 1 {}", sql.trim().trim_end_matches(';'));
        let rep = self.ch.query(&statement, Some(&db), Some(&self.session), None, 10_000).await?;
        let lines: Vec<String> = rep.rs.map(|rs| rs.rows.into_iter().filter_map(|r| r.into_iter().next().and_then(|v| v.to_text())).collect()).unwrap_or_default();
        // Indentation (2 spaces per level) expresses nesting.
        let mut root = PlanNode { title: "Query plan".into(), ..Default::default() };
        let mut stack: Vec<(usize, Vec<usize>)> = Vec::new();
        for l in &lines {
            let indent = l.len() - l.trim_start().len();
            let text = l.trim().to_string();
            if text.is_empty() {
                continue;
            }
            let is_prop = !text.ends_with(')') && text.contains(':') && !stack.is_empty() && indent > stack.last().map(|s| s.0).unwrap_or(0);
            if is_prop {
                let path = stack.last().map(|s| s.1.clone()).unwrap_or_default();
                let mut n = &mut root;
                for &i in &path {
                    n = &mut n.children[i];
                }
                let (k, v) = text.split_once(':').unwrap_or((&text, ""));
                n.props.push((k.trim().to_string(), v.trim().to_string()));
                continue;
            }
            while stack.last().is_some_and(|(i, _)| *i >= indent) {
                stack.pop();
            }
            let parent = stack.last().map(|s| s.1.clone()).unwrap_or_default();
            let mut n = &mut root;
            for &i in &parent {
                n = &mut n.children[i];
            }
            n.children.push(PlanNode { title: text, ..Default::default() });
            let mut path = parent;
            path.push(n.children.len() - 1);
            stack.push((indent, path));
        }
        Ok(ExplainResult { statement, raw: lines.join("\n"), tree: Some(root), table: None, summary: Vec::new() })
    }

    fn cancel(&self) {
        let Some(qid) = self.running.lock().clone() else { return };
        let ch = self.ch.clone();
        self.rt.spawn(async move {
            let _ = ch.query(&format!("KILL QUERY WHERE query_id = {} ASYNC", lit(&qid)), None, None, None, 0).await;
        });
    }

    fn current_database(&self) -> Option<String> {
        Some(self.database.lock().clone())
    }

    async fn close(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn live_clickhouse() {
        let mut v = std::collections::BTreeMap::new();
        v.insert("host".into(), "127.0.0.1".into());
        v.insert("port".into(), "58123".into());
        v.insert("user".into(), "ovc".into());
        let Ok(h) = ClickHouseDriver::default().connect(ConnectParams { values: v, secret: Some("ovcpass".into()), read_only: false, notifier: None }).await else {
            eprintln!("skipping: no ClickHouse");
            return;
        };
        let s = h.sql().unwrap().clone();
        assert!(s.server_info().product.starts_with("ClickHouse"));
        let _ = s.drop_object(ObjectKind::Database, &ObjPath::db("ovc_test"), false).await;
        s.create_database(CreateDatabaseSpec { name: "ovc_test".into(), ..Default::default() }).await.unwrap();
        s.run(Some("ovc_test"), "CREATE TABLE events (id UInt64, kind LowCardinality(String), amount Decimal(10,2), at DateTime) ENGINE = MergeTree ORDER BY id; INSERT INTO events SELECT number, ['a','b','c'][number % 3 + 1], number / 4, toDateTime('2026-01-01 00:00:00') + number FROM numbers(1000)").await.unwrap();
        let tables = s.children(Some((ObjectKind::Folder(FolderKind::Tables), ObjPath::db("ovc_test")))).await.unwrap();
        assert_eq!(tables[0].label, "events");
        let p = ObjPath::db("ovc_test").with_name("events");
        assert!(s.ddl(ObjectKind::Table, &p).await.unwrap().contains("MergeTree"));
        let cols = s.columns(&p).await.unwrap();
        assert!(cols[0].primary_key);
        let page = s.table_page(TableQuery { path: p.clone(), filter: Some("kind = 'b'".into()), order_by: vec![("id".into(), SortDir::Desc)], limit: 5, offset: 0 }).await.unwrap();
        assert_eq!(page.rows.rows.len(), 5);
        assert!(page.read_only_reason.is_some());
        assert_eq!(s.count_rows(&p, Some("kind = 'a'")).await.unwrap(), 334);
        // Stacked statements are refused by the server.
        assert!(s.table_page(TableQuery { path: p.clone(), filter: Some("1 = 1; DROP TABLE ovc_test.events".into()), order_by: vec![], limit: 5, offset: 0 }).await.is_err());
        assert_eq!(s.count_rows(&p, None).await.unwrap(), 1000);

        let c = s.open_console(Some("ovc_test".into())).await.unwrap();
        let r = c.execute(vec!["SET max_threads = 2".into(), "SELECT kind, count() AS n, sum(amount) AS total FROM events GROUP BY kind ORDER BY kind".into(), "SELECT getSetting('max_threads')".into(), "SELECT number FROM numbers(100)".into(), "SELEC broken".into()], ExecOptions { max_rows: 10 }).await;
        let Ok(StatementOutcome::Rows(rs)) = &r[1].outcome else { panic!("{:?}", r[1].outcome) };
        assert_eq!(rs.rows.len(), 3);
        assert!(rs.columns[1].numeric);
        let Ok(StatementOutcome::Rows(rs)) = &r[2].outcome else { panic!() };
        assert_eq!(rs.rows[0][0].to_text().as_deref(), Some("2"), "session settings must persist");
        let Ok(StatementOutcome::Rows(rs)) = &r[3].outcome else { panic!() };
        assert!(rs.truncated && rs.rows.len() == 10);
        assert!(r[4].outcome.is_err());
        let plan = c.explain("SELECT * FROM events WHERE id = 5", false).await.unwrap();
        assert!(!plan.raw.is_empty());

        let c2 = c.clone();
        let h = tokio::spawn(async move { c2.execute(vec!["SELECT sleepEachRow(1) FROM numbers(20) SETTINGS max_block_size = 1, function_sleep_max_microseconds_per_block = 30000000".into()], ExecOptions::default()).await });
        tokio::time::sleep(Duration::from_millis(1200)).await;
        c.cancel();
        let r = tokio::time::timeout(Duration::from_secs(10), h).await.expect("cancel too slow").unwrap();
        assert!(r[0].outcome.is_err(), "{:?}", r[0].outcome);
        assert!(s.metrics().await.unwrap().iter().any(|m| m.name == "Active parts"));
        s.activity().await.unwrap();
        s.drop_object(ObjectKind::Database, &ObjPath::db("ovc_test"), false).await.unwrap();
    }
}
