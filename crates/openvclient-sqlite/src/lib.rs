//! SQLite connector built on rusqlite (bundled SQLite). All calls run on the
//! blocking pool; each editor console gets its own connection.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use openvclient_core::explain::{ExplainResult, sqlite_plan_tree};
use openvclient_core::sql::*;
use openvclient_core::sqltext::Dialect;
use openvclient_core::{Category, ColumnMeta, ConnectParams, Driver, DriverInfo, Error, FieldKind, FieldSpec, Result, ResultSet, ServerInfo, ServiceSession, SessionHandle, Value};
use parking_lot::Mutex;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, params_from_iter};

const D: Dialect = Dialect::Sqlite;

pub struct SqliteDriver {
    info: DriverInfo,
}

impl Default for SqliteDriver {
    fn default() -> Self {
        Self {
            info: DriverInfo {
                id: "sqlite",
                name: "SQLite",
                category: Category::Relational,
                description: "Local database file (bundled SQLite engine)",
                badge: "SL",
                fields: vec![
                    FieldSpec::text("path", "Database file")
                        .kind(FieldKind::File { save: true, filters: &["db", "sqlite", "sqlite3", "db3"] })
                        .required()
                        .placeholder("/path/to/database.db")
                        .help("The file is created if it does not exist (unless read-only)."),
                    FieldSpec::text("busy_timeout", "Busy timeout (ms)").kind(FieldKind::Number).default("5000").advanced(),
                ],
            },
        }
    }
}

#[async_trait]
impl Driver for SqliteDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }

    async fn connect(&self, params: ConnectParams) -> Result<SessionHandle> {
        let path = params.get("path").ok_or_else(|| Error::invalid("Choose a database file"))?.to_string();
        let busy: u64 = params.get_or("busy_timeout", "5000").parse().unwrap_or(5000);
        let read_only = params.read_only;
        let opener = Opener { path: path.clone(), read_only, busy: Duration::from_millis(busy) };
        let conn = {
            let o = opener.clone();
            tokio::task::spawn_blocking(move || o.open()).await??
        };
        let version = rusqlite::version().to_string();
        Ok(SessionHandle::Sql(Arc::new(SqliteSession { opener, conn: Arc::new(Mutex::new(conn)), version })))
    }
}

#[derive(Clone)]
struct Opener {
    path: String,
    read_only: bool,
    busy: Duration,
}

impl Opener {
    fn open(&self) -> Result<Connection> {
        let flags = if self.read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX
        };
        if self.read_only && !Path::new(&self.path).exists() {
            return Err(Error::invalid(format!("File not found: {}", self.path)));
        }
        let conn = Connection::open_with_flags(&self.path, flags).map_err(err)?;
        conn.busy_timeout(self.busy).map_err(err)?;
        // Fail fast on files that are not SQLite databases.
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(())).map_err(err)?;
        Ok(conn)
    }
}

fn err(e: rusqlite::Error) -> Error {
    use rusqlite::ffi::ErrorCode;
    match &e {
        rusqlite::Error::SqliteFailure(f, msg) => {
            let text = msg.clone().unwrap_or_else(|| e.to_string());
            match f.code {
                ErrorCode::OperationInterrupted => Error::cancelled(),
                ErrorCode::CannotOpen | ErrorCode::NotADatabase => Error::connection(text),
                ErrorCode::ReadOnly => Error::server(format!("{text} (connection is read-only)")),
                _ => Error::server(text).with_detail(format!("{:?}", f.code)),
            }
        }
        _ => Error::server(e.to_string()),
    }
}

fn to_value(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Int(i),
        ValueRef::Real(f) => Value::Float(f),
        ValueRef::Text(t) => Value::Text(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::Bytes(b.to_vec()),
    }
}

fn bind(v: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as V;
    match v {
        Value::Null => V::Null,
        Value::Bool(b) => V::Integer(*b as i64),
        Value::Int(i) => V::Integer(*i),
        Value::UInt(u) => V::Integer(*u as i64),
        Value::Float(f) => V::Real(*f),
        Value::Text(s) => V::Text(s.clone()),
        Value::Bytes(b) => V::Blob(b.clone()),
    }
}

fn bind_text(v: &Option<String>) -> rusqlite::types::Value {
    match v {
        None => rusqlite::types::Value::Null,
        Some(s) => rusqlite::types::Value::Text(s.clone()),
    }
}

/// Run a prepared statement, collecting at most `max` rows.
fn query_rows(stmt: &mut rusqlite::Statement<'_>, params: &[rusqlite::types::Value], max: usize) -> Result<ResultSet> {
    let columns: Vec<ColumnMeta> = stmt.columns().map(|c| ColumnMeta::new(c.name(), c.decl_type().unwrap_or(""))).collect();
    let n = columns.len();
    let mut rs = ResultSet::new(columns);
    let mut rows = stmt.query(params_from_iter(params.iter())).map_err(err)?;
    while let Some(row) = rows.next().map_err(err)? {
        if rs.rows.len() >= max {
            rs.truncated = true;
            break;
        }
        rs.rows.push((0..n).map(|i| row.get_ref(i).map(to_value).unwrap_or(Value::Null)).collect());
    }
    // Typed columns without a declared type (expressions) still sort right.
    for (i, c) in rs.columns.iter_mut().enumerate() {
        if c.type_name.is_empty() {
            c.numeric = rs.rows.iter().any(|r| r[i].is_numeric()) && rs.rows.iter().all(|r| r[i].is_numeric() || r[i].is_null());
        }
    }
    Ok(rs)
}

struct SqliteSession {
    opener: Opener,
    conn: Arc<Mutex<Connection>>,
    version: String,
}

impl SqliteSession {
    async fn with<T: Send + 'static>(&self, f: impl FnOnce(&Connection) -> Result<T> + Send + 'static) -> Result<T> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || f(&conn.lock())).await?
    }
}

fn schema_of(p: &ObjPath) -> String {
    p.schema.clone().unwrap_or_else(|| "main".into())
}

fn master(schema: &str) -> String {
    if schema == "temp" { "sqlite_temp_master".into() } else { format!("{}.sqlite_master", D.quote_ident(schema)) }
}

fn names(conn: &Connection, sql: &str) -> Result<Vec<String>> {
    let mut st = conn.prepare(sql).map_err(err)?;
    let rows = st.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(err)
}

fn table_columns(conn: &Connection, schema: &str, table: &str) -> Result<Vec<ColumnInfo>> {
    let sql = format!("PRAGMA {}.table_xinfo({})", D.quote_ident(schema), D.quote_literal(table));
    let mut st = conn.prepare(&sql).map_err(err)?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                r.get::<_, i64>(3)? != 0,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6).unwrap_or(0),
            ))
        })
        .map_err(err)?;
    let mut out = Vec::new();
    let mut pk_count = 0;
    for r in rows {
        let (name, ty, notnull, default, pk, hidden) = r.map_err(err)?;
        if pk > 0 {
            pk_count += 1;
        }
        let comment = match hidden {
            2 => Some("generated (virtual)".to_string()),
            3 => Some("generated (stored)".to_string()),
            _ => None,
        };
        out.push(ColumnInfo { name, data_type: ty, nullable: !notnull, default, primary_key: pk > 0, auto_increment: false, comment });
    }
    // A single INTEGER PRIMARY KEY aliases rowid and auto-assigns values.
    if pk_count == 1 {
        for c in &mut out {
            if c.primary_key && c.data_type.eq_ignore_ascii_case("integer") {
                c.auto_increment = true;
            }
        }
    }
    Ok(out)
}

fn object_type(conn: &Connection, schema: &str, name: &str) -> Result<Option<(String, String)>> {
    let sql = format!("SELECT type, coalesce(sql,'') FROM {} WHERE name = ?1", master(schema));
    let mut st = conn.prepare(&sql).map_err(err)?;
    let mut rows = st.query([name]).map_err(err)?;
    Ok(match rows.next().map_err(err)? {
        Some(r) => Some((r.get(0).map_err(err)?, r.get(1).map_err(err)?)),
        None => None,
    })
}

#[async_trait]
impl ServiceSession for SqliteSession {
    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            product: format!("SQLite {}", self.version),
            version: self.version.clone(),
            details: vec![("File".into(), self.opener.path.clone()), ("Mode".into(), if self.opener.read_only { "read-only" } else { "read-write" }.into())],
        }
    }

    async fn ping(&self) -> Result<Duration> {
        let t = Instant::now();
        self.with(|c| c.query_row("SELECT 1", [], |_| Ok(())).map_err(err)).await?;
        Ok(t.elapsed())
    }

    async fn close(&self) {}
}

#[async_trait]
impl SqlSession for SqliteSession {
    fn dialect(&self) -> Dialect {
        D
    }

    fn capabilities(&self) -> SqlCapabilities {
        SqlCapabilities { schemas: true, explain: true, metrics: true, edit_data: !self.opener.read_only, backup: true, restore: !self.opener.read_only, triggers: true, ..Default::default() }
    }

    fn default_database(&self) -> Option<String> {
        Some("main".into())
    }

    async fn children(&self, node: Option<(ObjectKind, ObjPath)>) -> Result<Vec<TreeNode>> {
        self.with(move |c| {
            let Some((kind, path)) = node else {
                let mut st = c.prepare("PRAGMA database_list").map_err(err)?;
                let rows = st.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?))).map_err(err)?;
                let mut out = Vec::new();
                for r in rows {
                    let (name, file) = r.map_err(err)?;
                    out.push(
                        TreeNode::new(ObjectKind::Schema, name.clone(), ObjPath::default().with_schema(name))
                            .detail(Path::new(&file).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
                    );
                }
                return Ok(out);
            };
            let schema = schema_of(&path);
            let base = ObjPath::default().with_schema(&schema);
            let list = |ty: &str| names(c, &format!("SELECT name FROM {} WHERE type = '{ty}' AND name NOT LIKE 'sqlite_%' ORDER BY name", master(&schema)));
            Ok(match kind {
                ObjectKind::Schema => vec![TreeNode::folder(FolderKind::Tables, base.clone()), TreeNode::folder(FolderKind::Views, base.clone()), TreeNode::folder(FolderKind::Triggers, base)],
                ObjectKind::Folder(FolderKind::Tables) => list("table")?.into_iter().map(|n| TreeNode::new(ObjectKind::Table, n.clone(), base.clone().with_name(n))).collect(),
                ObjectKind::Folder(FolderKind::Views) => list("view")?.into_iter().map(|n| TreeNode::new(ObjectKind::View, n.clone(), base.clone().with_name(n)).leaf()).collect(),
                ObjectKind::Folder(FolderKind::Triggers) => list("trigger")?.into_iter().map(|n| TreeNode::new(ObjectKind::Trigger, n.clone(), base.clone().with_name(n)).leaf()).collect(),
                ObjectKind::Table => vec![TreeNode::folder(FolderKind::Columns, path.clone()), TreeNode::folder(FolderKind::Indexes, path.clone())],
                ObjectKind::Folder(FolderKind::Columns) => {
                    let table = path.name()?;
                    table_columns(c, &schema, table)?
                        .into_iter()
                        .map(|col| {
                            let mut d = col.data_type.clone();
                            if col.primary_key {
                                d.push_str(" · PK");
                            }
                            if !col.nullable {
                                d.push_str(" · NOT NULL");
                            }
                            TreeNode::new(ObjectKind::Column, col.name.clone(), path.clone().with_sub(col.name)).detail(d)
                        })
                        .collect()
                }
                ObjectKind::Folder(FolderKind::Indexes) => {
                    let table = path.name()?;
                    let sql = format!("PRAGMA {}.index_list({})", D.quote_ident(&schema), D.quote_literal(table));
                    let mut st = c.prepare(&sql).map_err(err)?;
                    let rows = st.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?))).map_err(err)?;
                    let mut out = Vec::new();
                    for r in rows {
                        let (name, unique, origin) = r.map_err(err)?;
                        let detail = match (unique != 0, origin.as_str()) {
                            (_, "pk") => "primary key",
                            (true, _) => "unique",
                            _ => "",
                        };
                        out.push(TreeNode::new(ObjectKind::Index, name.clone(), path.clone().with_sub(name)).detail(detail));
                    }
                    out
                }
                _ => Vec::new(),
            })
        })
        .await
    }

    async fn ddl(&self, kind: ObjectKind, path: &ObjPath) -> Result<String> {
        let path = path.clone();
        self.with(move |c| {
            let schema = schema_of(&path);
            let name = match kind {
                ObjectKind::Index | ObjectKind::Column => path.sub.clone().unwrap_or_default(),
                _ => path.name()?.to_string(),
            };
            let Some((_, sql)) = object_type(c, &schema, &name)? else {
                return Err(Error::invalid(format!("{name} not found")));
            };
            let mut out = format!("{sql};\n");
            if kind == ObjectKind::Table {
                let extra = names(
                    c,
                    &format!("SELECT sql || ';' FROM {} WHERE tbl_name = {} AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY type, name", master(&schema), D.quote_literal(&name)),
                )?;
                for e in extra {
                    out.push('\n');
                    out.push_str(&e);
                    out.push('\n');
                }
            }
            Ok(out)
        })
        .await
    }

    async fn columns(&self, path: &ObjPath) -> Result<Vec<ColumnInfo>> {
        let path = path.clone();
        self.with(move |c| table_columns(c, &schema_of(&path), path.name()?)).await
    }

    async fn table_page(&self, q: TableQuery) -> Result<TablePage> {
        let read_only = self.opener.read_only;
        self.with(move |c| {
            let schema = schema_of(&q.path);
            let name = q.path.name()?.to_string();
            let target = D.qualified(Some(&schema), &name);
            let (otype, osql) = object_type(c, &schema, &name)?.ok_or_else(|| Error::invalid(format!("{name} not found")))?;
            let cols = table_columns(c, &schema, &name)?;
            let pk: Vec<String> = cols.iter().filter(|c| c.primary_key).map(|c| c.name.clone()).collect();
            let without_rowid = osql.to_ascii_uppercase().contains("WITHOUT ROWID");
            let use_rowid = otype == "table" && pk.is_empty() && !without_rowid;
            let mut sql = if use_rowid { format!("SELECT rowid AS \"__openvclient_rowid\", * FROM {target}") } else { format!("SELECT * FROM {target}") };
            if let Some(f) = q.filter.as_deref().filter(|f| !f.trim().is_empty()) {
                sql.push_str(&format!(" WHERE {f}"));
            }
            if !q.order_by.is_empty() {
                let parts: Vec<String> = q.order_by.iter().map(|(c, d)| format!("{} {}", D.quote_ident(c), if *d == SortDir::Asc { "ASC" } else { "DESC" })).collect();
                sql.push_str(&format!(" ORDER BY {}", parts.join(", ")));
            }
            sql.push_str(&format!(" LIMIT {} OFFSET {}", q.limit, q.offset));
            let mut st = c.prepare(&sql).map_err(err)?;
            let mut rows = query_rows(&mut st, &[], usize::MAX)?;
            // Use declared types from the schema for header and alignment.
            let mut row_ids = None;
            if use_rowid {
                rows.columns.remove(0);
                row_ids = Some(rows.rows.iter_mut().map(|r| r.remove(0).to_text().unwrap_or_default()).collect());
            }
            for col in &mut rows.columns {
                if let Some(info) = cols.iter().find(|c| c.name == col.name) {
                    *col = ColumnMeta::new(col.name.clone(), info.data_type.clone());
                }
            }
            let read_only_reason = if read_only {
                Some("Connection is read-only".to_string())
            } else if otype != "table" {
                Some("Views are read-only in SQLite".to_string())
            } else if pk.is_empty() && !use_rowid {
                Some("Table has no primary key".to_string())
            } else {
                None
            };
            Ok(TablePage { rows, key_columns: pk, row_ids, read_only_reason })
        })
        .await
    }

    async fn count_rows(&self, path: &ObjPath, filter: Option<&str>) -> Result<u64> {
        let path = path.clone();
        let filter = filter.map(str::to_owned);
        self.with(move |c| {
            let mut sql = format!("SELECT count(*) FROM {}", D.qualified(Some(&schema_of(&path)), path.name()?));
            if let Some(f) = filter.filter(|f| !f.trim().is_empty()) {
                sql.push_str(&format!(" WHERE {f}"));
            }
            c.query_row(&sql, [], |r| r.get::<_, i64>(0)).map(|n| n as u64).map_err(err)
        })
        .await
    }

    async fn apply_changes(&self, path: &ObjPath, changes: Vec<RowChange>) -> Result<u64> {
        let path = path.clone();
        self.with(move |c| {
            let target = D.qualified(Some(&schema_of(&path)), path.name()?);
            let tx = c.unchecked_transaction().map_err(err)?;
            let mut total = 0u64;
            for ch in changes {
                let (sql, params) = match ch {
                    RowChange::Update { key, values } => {
                        let mut params: Vec<_> = values.iter().map(|(_, v)| bind_text(v)).collect();
                        let sets: Vec<String> = values.iter().enumerate().map(|(i, (c, _))| format!("{} = ?{}", D.quote_ident(c), i + 1)).collect();
                        let (w, kp) = where_key(&key, params.len());
                        params.extend(kp);
                        (format!("UPDATE {target} SET {} WHERE {w}", sets.join(", ")), params)
                    }
                    RowChange::Insert { values } => {
                        let cols: Vec<String> = values.iter().map(|(c, _)| D.quote_ident(c)).collect();
                        let ph: Vec<String> = (1..=values.len()).map(|i| format!("?{i}")).collect();
                        let params = values.iter().map(|(_, v)| bind_text(v)).collect();
                        if cols.is_empty() {
                            (format!("INSERT INTO {target} DEFAULT VALUES"), params)
                        } else {
                            (format!("INSERT INTO {target} ({}) VALUES ({})", cols.join(", "), ph.join(", ")), params)
                        }
                    }
                    RowChange::Delete { key } => {
                        let (w, kp) = where_key(&key, 0);
                        (format!("DELETE FROM {target} WHERE {w}"), kp)
                    }
                };
                let n = tx.execute(&sql, params_from_iter(params.iter())).map_err(err)?;
                total += n as u64;
            }
            tx.commit().map_err(err)?;
            Ok(total)
        })
        .await
    }

    async fn catalog(&self, _database: Option<&str>) -> Result<Catalog> {
        self.with(|c| {
            let mut cat = Catalog { schemas: names(c, "SELECT name FROM pragma_database_list")?, ..Default::default() };
            let mut st = c
                .prepare(
                    "SELECT m.name, m.type, p.name FROM sqlite_master m LEFT JOIN pragma_table_info(m.name) p \
                     WHERE m.type IN ('table','view') AND m.name NOT LIKE 'sqlite_%' ORDER BY m.name, p.cid",
                )
                .map_err(err)?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))).map_err(err)?;
            for r in rows {
                let (t, ty, col) = r.map_err(err)?;
                let entry = cat.columns.entry(t.clone()).or_default();
                if entry.is_empty() {
                    let kind = if ty == "view" { ObjectKind::View } else { ObjectKind::Table };
                    cat.objects.push((String::new(), t, kind));
                }
                if let Some(col) = col {
                    entry.push(col);
                }
            }
            Ok(cat)
        })
        .await
    }

    async fn open_console(&self, _database: Option<String>) -> Result<Arc<dyn SqlConsole>> {
        let o = self.opener.clone();
        let conn = tokio::task::spawn_blocking(move || o.open()).await??;
        let interrupt = conn.get_interrupt_handle();
        Ok(Arc::new(SqliteConsole { conn: Arc::new(Mutex::new(conn)), interrupt }))
    }

    async fn run(&self, _database: Option<&str>, sql: &str) -> Result<u64> {
        let sql = sql.to_string();
        self.with(move |c| {
            c.execute_batch(&sql).map_err(err)?;
            Ok(c.changes())
        })
        .await
    }

    async fn drop_object(&self, kind: ObjectKind, path: &ObjPath, _cascade: bool) -> Result<()> {
        let kw = match kind {
            ObjectKind::Table | ObjectKind::View | ObjectKind::Trigger => kind.sql_keyword().unwrap_or_default(),
            ObjectKind::Index => "INDEX",
            _ => return Err(Error::unsupported(format!("SQLite cannot drop a {}", kind.label()))),
        };
        let name = if kind == ObjectKind::Index { path.sub.clone().unwrap_or_default() } else { path.name()?.to_string() };
        let sql = format!("DROP {kw} {}", D.qualified(Some(&schema_of(path)), &name));
        self.run(None, &sql).await.map(|_| ())
    }

    async fn metrics(&self) -> Result<Vec<Metric>> {
        let path = self.opener.path.clone();
        self.with(move |c| {
            let pragma =
                |p: &str| -> String { c.query_row(&format!("PRAGMA {p}"), [], |r| r.get_ref(0).map(to_value)).map(|v| v.to_text().unwrap_or_default()).unwrap_or_else(|e| format!("error: {e}")) };
            let page_size: f64 = pragma("page_size").parse().unwrap_or(0.0);
            let pages: f64 = pragma("page_count").parse().unwrap_or(0.0);
            let free: f64 = pragma("freelist_count").parse().unwrap_or(0.0);
            let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let wal_size = std::fs::metadata(format!("{path}-wal")).map(|m| m.len()).ok();
            let mut m = vec![
                Metric::new("Storage", "File size (bytes)", file_size),
                Metric::new("Storage", "Page size (bytes)", page_size),
                Metric::new("Storage", "Pages", pages),
                Metric::new("Storage", "Free pages", free).hint("Reclaimable with VACUUM"),
                Metric::new("Storage", "Fragmentation (%)", format!("{:.1}", if pages > 0.0 { free / pages * 100.0 } else { 0.0 })),
                Metric::new("Configuration", "Journal mode", pragma("journal_mode")),
                Metric::new("Configuration", "Synchronous", pragma("synchronous")),
                Metric::new("Configuration", "Auto-vacuum", pragma("auto_vacuum")),
                Metric::new("Configuration", "Encoding", pragma("encoding")),
                Metric::new("Configuration", "Foreign keys enforced", pragma("foreign_keys")),
                Metric::new("Configuration", "Cache size", pragma("cache_size")),
                Metric::new("Schema", "User version", pragma("user_version")),
                Metric::new("Schema", "Schema version", pragma("schema_version")),
                Metric::new("Engine", "SQLite version", rusqlite::version()),
            ];
            if let Some(w) = wal_size {
                m.insert(1, Metric::new("Storage", "WAL file size (bytes)", w));
            }
            Ok(m)
        })
        .await
    }

    async fn backup(&self, req: BackupRequest) -> Result<String> {
        let dest = req.path.clone();
        self.with(move |c| {
            let t = Instant::now();
            c.backup(rusqlite::MAIN_DB, &dest, None::<fn(rusqlite::backup::Progress)>).map_err(err)?;
            let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
            Ok(format!("Online backup written to {} ({} bytes) in {}", dest.display(), size, openvclient_core::elapsed_ms(t.elapsed())))
        })
        .await
    }

    async fn restore(&self, req: BackupRequest) -> Result<String> {
        let src = req.path.clone();
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut c = conn.lock();
            let t = Instant::now();
            c.restore(rusqlite::MAIN_DB, &src, None::<fn(rusqlite::backup::Progress)>).map_err(err)?;
            Ok(format!("Restored main database from {} in {}", src.display(), openvclient_core::elapsed_ms(t.elapsed())))
        })
        .await?
    }
}

fn where_key(key: &RowKey, offset: usize) -> (String, Vec<rusqlite::types::Value>) {
    match key {
        RowKey::RowId(id) => (format!("rowid = ?{}", offset + 1), vec![rusqlite::types::Value::Text(id.clone())]),
        RowKey::PrimaryKey(cols) => {
            let w: Vec<String> = cols
                .iter()
                .enumerate()
                .map(|(i, (c, v))| if v.is_null() { format!("{} IS ?{}", D.quote_ident(c), offset + i + 1) } else { format!("{} = ?{}", D.quote_ident(c), offset + i + 1) })
                .collect();
            (w.join(" AND "), cols.iter().map(|(_, v)| bind(v)).collect())
        }
    }
}

struct SqliteConsole {
    conn: Arc<Mutex<Connection>>,
    interrupt: rusqlite::InterruptHandle,
}

#[async_trait]
impl SqlConsole for SqliteConsole {
    async fn execute(&self, statements: Vec<String>, opts: ExecOptions) -> Vec<StatementResult> {
        let conn = self.conn.clone();
        let res = tokio::task::spawn_blocking(move || {
            let c = conn.lock();
            let mut out = Vec::new();
            for sql in statements {
                let t = Instant::now();
                let outcome = (|| -> Result<StatementOutcome> {
                    let mut st = c.prepare(&sql).map_err(err)?;
                    if st.column_count() > 0 {
                        Ok(StatementOutcome::Rows(query_rows(&mut st, &[], opts.max_rows)?))
                    } else {
                        let n = st.raw_execute().map_err(err)?;
                        let kw = openvclient_core::sqltext::leading_keyword(&sql);
                        Ok(if matches!(kw.as_str(), "INSERT" | "UPDATE" | "DELETE" | "REPLACE") { StatementOutcome::Affected(n as u64) } else { StatementOutcome::Done(format!("{kw} completed")) })
                    }
                })();
                let failed = outcome.is_err();
                out.push(StatementResult { sql, elapsed: t.elapsed(), outcome });
                if failed {
                    break;
                }
            }
            out
        })
        .await;
        res.unwrap_or_else(|e| vec![StatementResult { sql: String::new(), elapsed: Duration::ZERO, outcome: Err(e.into()) }])
    }

    async fn explain(&self, sql: &str, analyze: bool) -> Result<ExplainResult> {
        if analyze {
            return Err(Error::unsupported("SQLite has no EXPLAIN ANALYZE; showing the query plan requires EXPLAIN QUERY PLAN"));
        }
        let statement = format!("EXPLAIN QUERY PLAN {sql}");
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let c = conn.lock();
            let mut st = c.prepare(&statement).map_err(err)?;
            let rows: Vec<(i64, i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(3)?))).map_err(err)?.collect::<std::result::Result<_, _>>().map_err(err)?;
            let raw = rows.iter().map(|r| format!("{} {} {}", r.0, r.1, r.2)).collect::<Vec<_>>().join("\n");
            Ok(ExplainResult { statement, raw, tree: Some(sqlite_plan_tree(&rows)), table: None, summary: Vec::new() })
        })
        .await?
    }

    fn cancel(&self) {
        self.interrupt.interrupt();
    }

    fn current_database(&self) -> Option<String> {
        Some("main".into())
    }

    async fn close(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn session(path: &str) -> Arc<dyn SqlSession> {
        let mut values = std::collections::BTreeMap::new();
        values.insert("path".to_string(), path.to_string());
        let h = SqliteDriver::default().connect(ConnectParams { values, secret: None, read_only: false, notifier: None }).await.unwrap();
        h.sql().unwrap().clone()
    }

    #[tokio::test]
    async fn end_to_end() {
        let dir = std::env::temp_dir().join(format!("openvclient-sqlite-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let _ = std::fs::remove_file(&path);
        let s = session(path.to_str().unwrap()).await;
        s.run(None, "CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT NOT NULL, score REAL); CREATE TABLE nokey(a TEXT); CREATE VIEW v AS SELECT name FROM users;").await.unwrap();

        let console = s.open_console(None).await.unwrap();
        let stmts = openvclient_core::sqltext::split_statements(Dialect::Sqlite, "INSERT INTO users(name, score) VALUES ('ann', 1.5), ('bob', 2); SELECT * FROM users ORDER BY id;");
        let sql = "INSERT INTO users(name, score) VALUES ('ann', 1.5), ('bob', 2); SELECT * FROM users ORDER BY id;";
        let res = console.execute(stmts.iter().map(|s| sql[s.start..s.end].to_string()).collect(), ExecOptions::default()).await;
        assert!(matches!(res[0].outcome, Ok(StatementOutcome::Affected(2))));
        let Ok(StatementOutcome::Rows(rs)) = &res[1].outcome else { panic!("{:?}", res[1].outcome) };
        assert_eq!(rs.rows.len(), 2);
        assert_eq!(rs.rows[1][1], Value::Text("bob".into()));

        let tree = s.children(None).await.unwrap();
        assert_eq!(tree[0].label, "main");
        let tables = s.children(Some((ObjectKind::Folder(FolderKind::Tables), ObjPath::default().with_schema("main")))).await.unwrap();
        assert_eq!(tables.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), vec!["nokey", "users"]);

        let p = ObjPath::default().with_schema("main").with_name("users");
        let page = s.table_page(TableQuery { path: p.clone(), filter: Some("score > 1".into()), order_by: vec![("name".into(), SortDir::Desc)], limit: 10, offset: 0 }).await.unwrap();
        assert_eq!(page.key_columns, vec!["id"]);
        assert_eq!(page.rows.rows[0][1], Value::Text("bob".into()));
        let id = page.rows.rows[0][0].clone();
        let n = s
            .apply_changes(
                &p,
                vec![
                    RowChange::Update { key: RowKey::PrimaryKey(vec![("id".into(), id)]), values: vec![("score".into(), Some("9".into()))] },
                    RowChange::Insert { values: vec![("name".into(), Some("cy".into()))] },
                ],
            )
            .await
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(s.count_rows(&p, None).await.unwrap(), 3);

        let nk = ObjPath::default().with_schema("main").with_name("nokey");
        s.apply_changes(&nk, vec![RowChange::Insert { values: vec![("a".into(), Some("x".into()))] }]).await.unwrap();
        let page = s.table_page(TableQuery { path: nk.clone(), filter: None, order_by: vec![], limit: 10, offset: 0 }).await.unwrap();
        let rid = page.row_ids.unwrap()[0].clone();
        s.apply_changes(&nk, vec![RowChange::Delete { key: RowKey::RowId(rid) }]).await.unwrap();
        assert_eq!(s.count_rows(&nk, None).await.unwrap(), 0);

        let v = s.table_page(TableQuery { path: ObjPath::default().with_schema("main").with_name("v"), filter: None, order_by: vec![], limit: 10, offset: 0 }).await.unwrap();
        assert!(v.read_only_reason.is_some());

        let plan = console.explain("SELECT * FROM users WHERE id = 1", false).await.unwrap();
        assert!(plan.raw.to_uppercase().contains("SEARCH"), "{}", plan.raw);

        assert!(s.ddl(ObjectKind::Table, &p).await.unwrap().contains("CREATE TABLE users"));
        let backup = dir.join("b.db");
        let _ = std::fs::remove_file(&backup);
        s.backup(BackupRequest { database: None, path: backup.clone() }).await.unwrap();
        assert!(backup.exists());
        assert!(!s.metrics().await.unwrap().is_empty());
        let cat = s.catalog(None).await.unwrap();
        assert!(cat.columns["users"].contains(&"score".to_string()));
    }
}
