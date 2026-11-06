//! Contract for relational engines.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::explain::ExplainResult;
use crate::sqltext::Dialect;
use crate::{Error, Result, ResultSet, ServiceSession, Value};

/// What a relational session can really do. The UI hides anything that is
/// `false`; connectors must never set a flag they cannot honour.
#[derive(Clone, Copy, Debug, Default)]
pub struct SqlCapabilities {
    pub multiple_databases: bool,
    pub schemas: bool,
    pub create_database: bool,
    pub drop_database: bool,
    /// Toggle whether a database accepts connections (PostgreSQL
    /// `ALLOW_CONNECTIONS`); not a full offline mode.
    pub database_connection_toggle: bool,
    /// Databases can be taken OFFLINE / brought ONLINE (SQL Server).
    pub database_offline: bool,
    /// Backup/restore files live on the database server, not this machine.
    pub server_side_backup: bool,
    pub create_schema: bool,
    pub explain: bool,
    pub explain_analyze: bool,
    pub activity: bool,
    pub cancel_query: bool,
    pub terminate_session: bool,
    pub locks: bool,
    pub metrics: bool,
    pub users: bool,
    /// Separate roles that are not login users (PostgreSQL).
    pub roles: bool,
    pub grants: bool,
    pub edit_data: bool,
    pub truncate: bool,
    pub backup: bool,
    pub restore: bool,
    pub materialized_views: bool,
    pub sequences: bool,
    pub procedures: bool,
    pub functions: bool,
    pub triggers: bool,
    pub events: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FolderKind {
    /// SSMS-style server folders.
    Databases,
    SystemDatabases,
    Security,
    ActivityMonitor,
    Programmability,
    Tables,
    Views,
    MaterializedViews,
    ForeignTables,
    Functions,
    Procedures,
    Sequences,
    Triggers,
    Events,
    Columns,
    Indexes,
    Constraints,
}

impl FolderKind {
    pub fn label(self) -> &'static str {
        match self {
            FolderKind::Databases => "Databases",
            FolderKind::SystemDatabases => "System Databases",
            FolderKind::Security => "Security",
            FolderKind::ActivityMonitor => "Activity Monitor",
            FolderKind::Programmability => "Programmability",
            FolderKind::Tables => "Tables",
            FolderKind::Views => "Views",
            FolderKind::MaterializedViews => "Materialized views",
            FolderKind::ForeignTables => "Foreign tables",
            FolderKind::Functions => "Functions",
            FolderKind::Procedures => "Procedures",
            FolderKind::Sequences => "Sequences",
            FolderKind::Triggers => "Triggers",
            FolderKind::Events => "Events",
            FolderKind::Columns => "Columns",
            FolderKind::Indexes => "Indexes",
            FolderKind::Constraints => "Keys & constraints",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectKind {
    Database,
    Schema,
    Folder(FolderKind),
    Table,
    View,
    MaterializedView,
    ForeignTable,
    Function,
    Procedure,
    Sequence,
    Trigger,
    Event,
    Column,
    Index,
    Constraint,
}

impl ObjectKind {
    pub fn label(self) -> &'static str {
        match self {
            ObjectKind::Database => "Database",
            ObjectKind::Schema => "Schema",
            ObjectKind::Folder(f) => f.label(),
            ObjectKind::Table => "Table",
            ObjectKind::View => "View",
            ObjectKind::MaterializedView => "Materialized view",
            ObjectKind::ForeignTable => "Foreign table",
            ObjectKind::Function => "Function",
            ObjectKind::Procedure => "Procedure",
            ObjectKind::Sequence => "Sequence",
            ObjectKind::Trigger => "Trigger",
            ObjectKind::Event => "Event",
            ObjectKind::Column => "Column",
            ObjectKind::Index => "Index",
            ObjectKind::Constraint => "Constraint",
        }
    }

    /// Objects whose rows can be browsed in a data grid.
    pub fn has_rows(self) -> bool {
        matches!(self, ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView | ObjectKind::ForeignTable)
    }

    /// SQL keyword used by DROP statements.
    pub fn sql_keyword(self) -> Option<&'static str> {
        Some(match self {
            ObjectKind::Database => "DATABASE",
            ObjectKind::Schema => "SCHEMA",
            ObjectKind::Table => "TABLE",
            ObjectKind::View => "VIEW",
            ObjectKind::MaterializedView => "MATERIALIZED VIEW",
            ObjectKind::ForeignTable => "FOREIGN TABLE",
            ObjectKind::Function => "FUNCTION",
            ObjectKind::Procedure => "PROCEDURE",
            ObjectKind::Sequence => "SEQUENCE",
            ObjectKind::Trigger => "TRIGGER",
            ObjectKind::Event => "EVENT",
            ObjectKind::Index => "INDEX",
            _ => return None,
        })
    }
}

/// Location of an object. Which parts are used depends on the engine:
/// SQLite uses `schema` for the attached database name, MySQL leaves
/// `schema` empty and uses `database`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjPath {
    pub database: Option<String>,
    pub schema: Option<String>,
    /// Table, view, routine… name.
    pub name: Option<String>,
    /// Child of `name` (column, index, trigger) or a routine signature.
    pub sub: Option<String>,
}

impl ObjPath {
    pub fn db(database: impl Into<String>) -> Self {
        Self { database: Some(database.into()), ..Default::default() }
    }
    pub fn with_schema(mut self, s: impl Into<String>) -> Self {
        self.schema = Some(s.into());
        self
    }
    pub fn with_name(mut self, n: impl Into<String>) -> Self {
        self.name = Some(n.into());
        self
    }
    pub fn with_sub(mut self, n: impl Into<String>) -> Self {
        self.sub = Some(n.into());
        self
    }
    pub fn name(&self) -> Result<&str> {
        self.name.as_deref().ok_or_else(|| Error::invalid("Object name missing"))
    }
    /// `schema.name` or `name`, for titles.
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if let Some(s) = &self.schema {
            parts.push(s.as_str());
        } else if let Some(d) = &self.database {
            parts.push(d.as_str());
        }
        if let Some(n) = &self.name {
            parts.push(n.as_str());
        }
        if let Some(s) = &self.sub {
            parts.push(s.as_str());
        }
        parts.join(".")
    }
}

#[derive(Clone, Debug)]
pub struct TreeNode {
    pub kind: ObjectKind,
    pub label: String,
    pub path: ObjPath,
    /// Secondary text: column type, row estimate, size…
    pub detail: Option<String>,
    pub expandable: bool,
}

impl TreeNode {
    pub fn new(kind: ObjectKind, label: impl Into<String>, path: ObjPath) -> Self {
        let expandable = !matches!(kind, ObjectKind::Column | ObjectKind::Index | ObjectKind::Constraint);
        Self { kind, label: label.into(), path, detail: None, expandable }
    }
    pub fn folder(f: FolderKind, path: ObjPath) -> Self {
        Self::new(ObjectKind::Folder(f), f.label(), path)
    }
    pub fn detail(mut self, d: impl Into<String>) -> Self {
        let d = d.into();
        if !d.is_empty() {
            self.detail = Some(d);
        }
        self
    }
    pub fn leaf(mut self) -> Self {
        self.expandable = false;
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    /// Full type as the engine prints it, e.g. `character varying(64)`.
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub comment: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDir {
    Asc,
    Desc,
}

/// One page of a table, computed by the engine (filters and sorts run
/// server-side so large tables stay fast).
#[derive(Clone, Debug)]
pub struct TableQuery {
    pub path: ObjPath,
    /// Raw SQL boolean expression for WHERE, in the engine's dialect.
    pub filter: Option<String>,
    pub order_by: Vec<(String, SortDir)>,
    pub limit: u64,
    pub offset: u64,
}

/// How rows are addressed when editing. Engines without a primary key use
/// their physical row id where one exists (`ctid`, `rowid`).
#[derive(Clone, Debug, PartialEq)]
pub enum RowKey {
    PrimaryKey(Vec<(String, Value)>),
    RowId(String),
}

#[derive(Clone, Debug)]
pub enum RowChange {
    Update { key: RowKey, values: Vec<(String, Option<String>)> },
    Insert { values: Vec<(String, Option<String>)> },
    Delete { key: RowKey },
}

/// A table page plus what is needed to edit it.
#[derive(Clone, Debug, Default)]
pub struct TablePage {
    pub rows: ResultSet,
    /// Columns forming the row key; empty when rows cannot be addressed.
    pub key_columns: Vec<String>,
    /// Physical row ids parallel to `rows`, when the engine uses them.
    pub row_ids: Option<Vec<String>>,
    /// Why editing is disabled, if it is.
    pub read_only_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub enum StatementOutcome {
    Rows(ResultSet),
    Affected(u64),
    Done(String),
}

#[derive(Clone, Debug)]
pub struct StatementResult {
    pub sql: String,
    pub elapsed: Duration,
    pub outcome: Result<StatementOutcome>,
}

#[derive(Clone, Copy, Debug)]
pub struct ExecOptions {
    /// Maximum rows kept per result set; the rest is discarded.
    pub max_rows: usize,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self { max_rows: 10_000 }
    }
}

/// A dedicated connection for one editor tab. Session state (transactions,
/// `SET` variables, temp tables) persists between executions.
#[async_trait]
pub trait SqlConsole: Send + Sync {
    /// Run statements one after another, stopping at the first error.
    async fn execute(&self, statements: Vec<String>, opts: ExecOptions) -> Vec<StatementResult>;
    async fn explain(&self, sql: &str, analyze: bool) -> Result<ExplainResult>;
    /// Ask the server to cancel what this console is running. Safe to call
    /// from any thread while `execute` is in flight.
    fn cancel(&self);
    /// Current database/schema for autocomplete and titles.
    fn current_database(&self) -> Option<String>;
    async fn close(&self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KillMode {
    CancelQuery,
    Terminate,
}

/// Running sessions, with the column holding the id `kill` accepts.
#[derive(Clone, Debug, Default)]
pub struct ActivitySnapshot {
    pub rows: ResultSet,
    pub id_column: usize,
}

#[derive(Clone, Debug)]
pub struct Metric {
    pub group: String,
    pub name: String,
    pub value: String,
    pub numeric: Option<f64>,
    pub hint: Option<String>,
}

impl Metric {
    pub fn new(group: &str, name: &str, value: impl ToString) -> Self {
        let value = value.to_string();
        let numeric = value.parse().ok();
        Self { group: group.into(), name: name.into(), value, numeric, hint: None }
    }
    pub fn hint(mut self, h: &str) -> Self {
        self.hint = Some(h.into());
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct PrincipalSpec {
    pub name: String,
    /// MySQL account host part, e.g. `%` or `localhost`.
    pub host: Option<String>,
    pub password: Option<String>,
    pub can_login: bool,
    pub superuser: bool,
    pub create_db: bool,
    pub create_role: bool,
}

#[derive(Clone, Debug)]
pub enum GrantTarget {
    Database(String),
    Schema(String),
    AllTablesInSchema(String),
    Table(ObjPath),
}

#[derive(Clone, Debug)]
pub struct GrantSpec {
    pub privileges: Vec<String>,
    pub target: GrantTarget,
    pub principal: String,
    pub host: Option<String>,
    pub with_grant_option: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CreateDatabaseSpec {
    pub name: String,
    pub owner: Option<String>,
    pub encoding: Option<String>,
    pub collation: Option<String>,
    pub template: Option<String>,
}

/// Columns and objects for autocomplete.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub schemas: Vec<String>,
    /// (schema or empty, name, kind)
    pub objects: Vec<(String, String, ObjectKind)>,
    /// table name → columns
    pub columns: std::collections::HashMap<String, Vec<String>>,
    pub functions: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct BackupRequest {
    pub database: Option<String>,
    pub path: PathBuf,
}

#[async_trait]
pub trait SqlSession: ServiceSession {
    fn dialect(&self) -> Dialect;
    fn capabilities(&self) -> SqlCapabilities;
    /// Database the profile connects to by default.
    fn default_database(&self) -> Option<String>;

    /// Children of a tree node; `None` lists the top level.
    async fn children(&self, node: Option<(ObjectKind, ObjPath)>) -> Result<Vec<TreeNode>>;
    async fn ddl(&self, kind: ObjectKind, path: &ObjPath) -> Result<String>;
    async fn columns(&self, path: &ObjPath) -> Result<Vec<ColumnInfo>>;
    async fn table_page(&self, q: TableQuery) -> Result<TablePage>;
    async fn count_rows(&self, path: &ObjPath, filter: Option<&str>) -> Result<u64>;
    /// Apply edits atomically; returns affected rows.
    async fn apply_changes(&self, path: &ObjPath, changes: Vec<RowChange>) -> Result<u64>;
    async fn catalog(&self, database: Option<&str>) -> Result<Catalog>;

    async fn open_console(&self, database: Option<String>) -> Result<Arc<dyn SqlConsole>>;

    /// Run statements on a pooled connection (admin actions, DDL).
    async fn run(&self, database: Option<&str>, sql: &str) -> Result<u64>;

    async fn drop_object(&self, kind: ObjectKind, path: &ObjPath, cascade: bool) -> Result<()> {
        let _ = (kind, path, cascade);
        Err(Error::unsupported("Dropping objects is not supported by this connector"))
    }
    async fn truncate(&self, path: &ObjPath) -> Result<()> {
        let _ = path;
        Err(Error::unsupported("TRUNCATE is not supported by this engine"))
    }
    async fn create_database(&self, spec: CreateDatabaseSpec) -> Result<()> {
        let _ = spec;
        Err(Error::unsupported("Creating databases is not supported by this engine"))
    }
    async fn set_database_allow_connections(&self, database: &str, allow: bool) -> Result<()> {
        let _ = (database, allow);
        Err(Error::unsupported("This engine has no per-database connection switch"))
    }
    async fn set_database_online(&self, database: &str, online: bool) -> Result<()> {
        let _ = (database, online);
        Err(Error::unsupported("This engine has no offline/online database state"))
    }
    async fn create_schema(&self, database: Option<&str>, name: &str) -> Result<()> {
        let _ = (database, name);
        Err(Error::unsupported("This engine has no schemas"))
    }
    async fn activity(&self) -> Result<ActivitySnapshot> {
        Err(Error::unsupported("Session monitoring is not available for this engine"))
    }
    async fn kill(&self, id: &str, mode: KillMode) -> Result<()> {
        let _ = (id, mode);
        Err(Error::unsupported("Killing sessions is not available for this engine"))
    }
    async fn locks(&self) -> Result<ResultSet> {
        Err(Error::unsupported("Lock monitoring is not available for this engine"))
    }
    async fn metrics(&self) -> Result<Vec<Metric>> {
        Err(Error::unsupported("Metrics are not available for this engine"))
    }
    async fn principals(&self) -> Result<ResultSet> {
        Err(Error::unsupported("User management is not available for this engine"))
    }
    async fn privileges(&self, principal: &str, host: Option<&str>) -> Result<ResultSet> {
        let _ = (principal, host);
        Err(Error::unsupported("Privilege listing is not available for this engine"))
    }
    async fn create_principal(&self, spec: PrincipalSpec) -> Result<()> {
        let _ = spec;
        Err(Error::unsupported("User management is not available for this engine"))
    }
    async fn drop_principal(&self, name: &str, host: Option<&str>) -> Result<()> {
        let _ = (name, host);
        Err(Error::unsupported("User management is not available for this engine"))
    }
    async fn set_password(&self, name: &str, host: Option<&str>, password: &str) -> Result<()> {
        let _ = (name, host, password);
        Err(Error::unsupported("User management is not available for this engine"))
    }
    async fn grant(&self, spec: GrantSpec, revoke: bool) -> Result<()> {
        let _ = (spec, revoke);
        Err(Error::unsupported("Grants are not available for this engine"))
    }
    /// Privilege names the engine accepts for a target, for the grant form.
    fn privilege_names(&self, target: &GrantTarget) -> Vec<&'static str> {
        let _ = target;
        Vec::new()
    }
    async fn backup(&self, req: BackupRequest) -> Result<String> {
        let _ = req;
        Err(Error::unsupported("Backup is not available for this engine"))
    }
    async fn restore(&self, req: BackupRequest) -> Result<String> {
        let _ = req;
        Err(Error::unsupported("Restore is not available for this engine"))
    }
}
