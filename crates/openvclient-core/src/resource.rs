//! Contract for non-relational services (documents, search, object storage,
//! files over FTP/SFTP, SSH hosts, message brokers…). Each connector exposes
//! a tree of nodes, what opening a node shows, the engine-native actions a
//! node supports and an optional console, so one generic UI serves them all
//! without pretending they are SQL.

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::{FieldSpec, Result, ResultSet, ServiceSession};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeIcon {
    Server,
    Database,
    Collection,
    Index,
    Folder,
    File,
    Bucket,
    Topic,
    Queue,
    Exchange,
    Key,
    Table,
    Terminal,
    Info,
    User,
}

#[derive(Clone, Debug)]
pub struct Node {
    /// Opaque, connector-defined path (stable across refreshes).
    pub id: String,
    pub label: String,
    /// Connector-defined kind that selects actions, e.g. `collection`.
    pub kind: String,
    pub icon: NodeIcon,
    pub detail: Option<String>,
    pub expandable: bool,
    /// Opening the node shows content (double-click / select).
    pub openable: bool,
}

impl Node {
    pub fn new(id: impl Into<String>, label: impl Into<String>, kind: &str, icon: NodeIcon) -> Self {
        Self { id: id.into(), label: label.into(), kind: kind.to_string(), icon, detail: None, expandable: false, openable: true }
    }
    pub fn folder(mut self) -> Self {
        self.expandable = true;
        self
    }
    pub fn container(mut self) -> Self {
        self.expandable = true;
        self.openable = false;
        self
    }
    pub fn detail(mut self, d: impl Into<String>) -> Self {
        let d = d.into();
        if !d.is_empty() {
            self.detail = Some(d);
        }
        self
    }
}

#[derive(Clone, Debug)]
pub enum Content {
    Table(ResultSet),
    /// Pretty-printed JSON documents.
    Json(String),
    Text(String),
    Props(Vec<(String, String)>),
    /// Rows plus a note (e.g. "first 100 of 9,201").
    TableWithNote(ResultSet, String),
    Empty(String),
}

/// Options for opening a node: paging and an engine-native filter.
#[derive(Clone, Debug, Default)]
pub struct OpenOptions {
    pub limit: u64,
    pub offset: u64,
    /// Engine-native filter: a Mongo filter document, an ES query string,
    /// a key prefix…  Empty = none.
    pub filter: String,
}

#[derive(Clone, Debug)]
pub struct ActionSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Irreversible; the UI asks for confirmation.
    pub destructive: bool,
    /// Inputs collected before running. File fields open a local picker.
    pub fields: Vec<FieldSpec>,
    /// Shown on the node's toolbar rather than only in its menu.
    pub primary: bool,
}

impl ActionSpec {
    pub fn new(id: &'static str, label: &'static str) -> Self {
        Self { id, label, destructive: false, fields: Vec::new(), primary: false }
    }
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }
    pub fn field(mut self, f: FieldSpec) -> Self {
        self.fields.push(f);
        self
    }
}

#[derive(Clone, Debug)]
pub struct ActionOutcome {
    pub message: String,
    /// Content to show after the action (e.g. a command's output).
    pub content: Option<Content>,
    /// The tree under this node id should be reloaded.
    pub refresh: Option<String>,
}

impl ActionOutcome {
    pub fn msg(m: impl Into<String>) -> Self {
        Self { message: m.into(), content: None, refresh: None }
    }
    pub fn refresh(mut self, node: impl Into<String>) -> Self {
        self.refresh = Some(node.into());
        self
    }
    pub fn show(mut self, c: Content) -> Self {
        self.content = Some(c);
        self
    }
}

pub type Args = BTreeMap<String, String>;

/// Keystrokes and size changes sent to an interactive terminal.
#[derive(Clone, Debug)]
pub enum TermInput {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

/// Output from an interactive terminal.
#[derive(Clone, Debug)]
pub enum TermEvent {
    Data(Vec<u8>),
    /// The remote side ended; the text explains why (exit status, error).
    Closed(String),
}

/// A live interactive terminal (e.g. an SSH shell with a PTY).
pub struct Terminal {
    pub input: tokio::sync::mpsc::UnboundedSender<TermInput>,
    pub output: std::sync::mpsc::Receiver<TermEvent>,
}


#[async_trait]
pub trait ResourceSession: ServiceSession {
    /// Children of a node; `None` lists the top level.
    async fn children(&self, parent: Option<&str>) -> Result<Vec<Node>>;
    /// Content shown when a node is opened.
    async fn open(&self, node: &str, opts: OpenOptions) -> Result<Content>;
    /// Hint for the filter box when opening nodes of this kind (None = no filter).
    fn filter_hint(&self, kind: &str) -> Option<&'static str> {
        let _ = kind;
        None
    }
    /// Actions for a node kind; `""` is the connection (root) itself.
    fn actions(&self, kind: &str) -> Vec<ActionSpec>;
    async fn run_action(&self, node: &str, action: &str, args: Args) -> Result<ActionOutcome>;
    /// Whether `open_terminal` is available.
    fn supports_terminal(&self) -> bool {
        false
    }
    /// Start an interactive terminal of the given size.
    async fn open_terminal(&self, cols: u16, rows: u16) -> Result<Terminal> {
        let _ = (cols, rows);
        Err(crate::Error::unsupported("This connection has no interactive terminal"))
    }
    /// Console language label, e.g. "Mongo command (JSON)". None = no console.
    fn console_language(&self) -> Option<&'static str> {
        None
    }
    /// Run console input in the context of `node` (database, host…).
    async fn console(&self, node: Option<&str>, input: &str) -> Result<Content> {
        let _ = (node, input);
        Err(crate::Error::unsupported("This service has no console"))
    }
}

/// Small helper for connectors: required argument or a clear error.
pub fn arg<'a>(a: &'a Args, key: &str) -> Result<&'a str> {
    a.get(key).map(String::as_str).filter(|v| !v.trim().is_empty()).ok_or_else(|| crate::Error::invalid(format!("{key} is required")))
}
