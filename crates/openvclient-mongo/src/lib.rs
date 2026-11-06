//! MongoDB connector. Documents are shown as a table of top-level fields
//! (nested values as Extended JSON); filters and updates use MongoDB's own
//! JSON syntax, and the console runs database commands.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::TryStreamExt;
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::IndexOptions;
use mongodb::{Client, IndexModel};
use openvclient_core::resource::*;
use openvclient_core::{Category, ColumnMeta, ConnectParams, Driver, DriverInfo, Error, ErrorKind, FieldKind, FieldSpec, Result, ResultSet, ServerInfo, ServiceSession, SessionHandle, Value};

pub struct MongoDriver {
    info: DriverInfo,
}

impl Default for MongoDriver {
    fn default() -> Self {
        Self {
            info: DriverInfo {
                id: "mongodb",
                name: "MongoDB",
                category: Category::Document,
                description: "MongoDB 4.2+, Atlas and wire-compatible servers",
                badge: "MG",
                fields: vec![
                    FieldSpec::text("host", "Host").default("localhost").required(),
                    FieldSpec::text("port", "Port").kind(FieldKind::Number).default("27017"),
                    FieldSpec::text("user", "User"),
                    FieldSpec::text("password", "Password").kind(FieldKind::Secret),
                    FieldSpec::text("auth_db", "Auth database").default("admin").advanced(),
                    FieldSpec::text("tls", "TLS").kind(FieldKind::Bool).default("false").advanced(),
                    FieldSpec::text("uri", "Connection string").placeholder("mongodb+srv://… (overrides the fields above)").advanced(),
                ],
            },
        }
    }
}

fn m_err(e: mongodb::error::Error) -> Error {
    use mongodb::error::ErrorKind as K;
    let msg = e.to_string();
    match *e.kind {
        K::Authentication { .. } => Error::auth(msg),
        K::ServerSelection { .. } | K::Io(_) | K::ConnectionPoolCleared { .. } => Error::connection(msg),
        K::InvalidArgument { .. } => Error::invalid(msg),
        _ => Error::server(msg),
    }
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Parse user JSON (Extended JSON allowed: {"$oid": …}, {"$date": …}).
pub fn parse_doc(text: &str) -> Result<Document> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(Document::new());
    }
    let v: serde_json::Value = serde_json::from_str(t).map_err(|e| Error::invalid(format!("Not valid JSON: {e}")))?;
    match Bson::try_from(v).map_err(|e| Error::invalid(format!("Not valid Extended JSON: {e}")))? {
        Bson::Document(d) => Ok(d),
        _ => Err(Error::invalid("Expected a JSON object")),
    }
}

fn to_json(d: Document) -> serde_json::Value {
    Bson::Document(d).into_relaxed_extjson()
}

fn cell(b: &Bson) -> Value {
    match b {
        Bson::Null | Bson::Undefined => Value::Null,
        Bson::Boolean(x) => Value::Bool(*x),
        Bson::Int32(x) => Value::Int(*x as i64),
        Bson::Int64(x) => Value::Int(*x),
        Bson::Double(x) => Value::Float(*x),
        Bson::String(s) => Value::Text(s.clone()),
        Bson::ObjectId(o) => Value::Text(format!("ObjectId(\"{o}\")")),
        Bson::DateTime(d) => Value::Text(d.try_to_rfc3339_string().unwrap_or_else(|_| d.to_string())),
        Bson::Decimal128(d) => Value::Text(d.to_string()),
        other => Value::Text(other.clone().into_relaxed_extjson().to_string()),
    }
}

/// Documents → table: columns are the union of top-level keys, `_id` first.
fn docs_table(docs: &[Document]) -> ResultSet {
    let mut keys: Vec<String> = Vec::new();
    let mut seen = BTreeSet::new();
    if docs.iter().any(|d| d.contains_key("_id")) {
        keys.push("_id".into());
        seen.insert("_id".to_string());
    }
    for d in docs {
        for k in d.keys() {
            if seen.insert(k.clone()) {
                keys.push(k.clone());
            }
        }
    }
    let mut rs = ResultSet::new(keys.iter().map(|k| ColumnMeta::new(k.clone(), "")).collect());
    rs.rows = docs.iter().map(|d| keys.iter().map(|k| d.get(k).map(cell).unwrap_or(Value::Null)).collect()).collect();
    for (i, c) in rs.columns.iter_mut().enumerate() {
        c.numeric = rs.rows.iter().any(|r| r[i].is_numeric()) && rs.rows.iter().all(|r| r[i].is_numeric() || r[i].is_null());
    }
    rs
}

/// Node ids: "db", "db/coll", "db/coll/#indexes", "db/coll/#indexes/name".
fn split(id: &str) -> (String, Option<String>, Option<String>) {
    let mut it = id.splitn(4, '/');
    let db = it.next().unwrap_or_default().to_string();
    let coll = it.next().map(str::to_owned);
    let _marker = it.next();
    let index = it.next().map(str::to_owned);
    (db, coll, index)
}

#[async_trait]
impl Driver for MongoDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }

    async fn connect(&self, p: ConnectParams) -> Result<SessionHandle> {
        let uri = match p.get("uri") {
            Some(u) => u.to_string(),
            None => {
                let auth = match (p.get("user"), &p.secret) {
                    (Some(u), Some(pw)) => format!("{}:{}@", enc(u), enc(pw)),
                    (Some(u), None) => format!("{}@", enc(u)),
                    _ => String::new(),
                };
                let mut q = vec![format!("authSource={}", p.get_or("auth_db", "admin")), "appName=OpenVClient".into(), "serverSelectionTimeoutMS=8000".into(), "directConnection=true".into()];
                if p.flag("tls") {
                    q.push("tls=true".into());
                }
                format!("mongodb://{auth}{}:{}/?{}", p.get_or("host", "localhost"), p.port(27017)?, q.join("&"))
            }
        };
        let client = Client::with_uri_str(&uri).await.map_err(m_err)?;
        let info = client.database("admin").run_command(doc! { "buildInfo": 1 }).await.map_err(m_err)?;
        let version = info.get_str("version").unwrap_or("?").to_string();
        let hello = client.database("admin").run_command(doc! { "hello": 1 }).await.map_err(m_err)?;
        let role = if hello.get_str("setName").is_ok() {
            format!("replica set {}", hello.get_str("setName").unwrap_or(""))
        } else if hello.get_str("msg").ok() == Some("isdbgrid") {
            "mongos (sharded)".into()
        } else {
            "standalone".into()
        };
        Ok(SessionHandle::Resource(Arc::new(MongoSession {
            client,
            info: ServerInfo { product: format!("MongoDB {version}"), version, details: vec![("Topology".into(), role)] },
            read_only: p.read_only,
        })))
    }
}

struct MongoSession {
    client: Client,
    info: ServerInfo,
    read_only: bool,
}

impl MongoSession {
    fn guard(&self) -> Result<()> {
        if self.read_only { Err(Error::invalid("Connection is read-only")) } else { Ok(()) }
    }
}

#[async_trait]
impl ServiceSession for MongoSession {
    fn server_info(&self) -> ServerInfo {
        self.info.clone()
    }
    async fn ping(&self) -> Result<Duration> {
        let t = Instant::now();
        self.client.database("admin").run_command(doc! { "ping": 1 }).await.map_err(m_err)?;
        Ok(t.elapsed())
    }
    async fn close(&self) {}
}

#[async_trait]
impl ResourceSession for MongoSession {
    async fn children(&self, parent: Option<&str>) -> Result<Vec<Node>> {
        match parent {
            None => {
                let dbs = self.client.list_databases().await.map_err(m_err)?;
                Ok(dbs
                    .into_iter()
                    .map(|d| {
                        let size = openvclient_core::containers::human_bytes(d.size_on_disk);
                        Node::new(&d.name, &d.name, "database", NodeIcon::Database).folder().detail(size)
                    })
                    .collect())
            }
            Some(id) => {
                let (db, coll, _) = split(id);
                match coll {
                    None => {
                        let mut out = Vec::new();
                        let mut cur = self.client.database(&db).list_collections().await.map_err(m_err)?;
                        while let Some(c) = cur.try_next().await.map_err(m_err)? {
                            let view = matches!(c.collection_type, mongodb::results::CollectionType::View);
                            let kind = if view { "view" } else { "collection" };
                            let mut n = Node::new(format!("{db}/{}", c.name), &c.name, kind, if view { NodeIcon::Table } else { NodeIcon::Collection });
                            if !view {
                                n = n.folder();
                                if let Ok(cnt) = self.client.database(&db).collection::<Document>(&c.name).estimated_document_count().await {
                                    n = n.detail(format!("~{cnt} docs"));
                                }
                            } else {
                                n = n.detail("view");
                            }
                            out.push(n);
                        }
                        out.sort_by(|a, b| a.label.cmp(&b.label));
                        Ok(out)
                    }
                    Some(c) if !id.contains("/#indexes") => Ok(vec![Node::new(format!("{db}/{c}/#indexes"), "Indexes", "indexes", NodeIcon::Index).container()]),
                    Some(c) => {
                        let mut cur = self.client.database(&db).collection::<Document>(&c).list_indexes().await.map_err(m_err)?;
                        let mut out = Vec::new();
                        while let Some(ix) = cur.try_next().await.map_err(m_err)? {
                            let name = ix.options.as_ref().and_then(|o| o.name.clone()).unwrap_or_default();
                            let unique = ix.options.as_ref().and_then(|o| o.unique).unwrap_or(false);
                            let keys = to_json(ix.keys.clone()).to_string();
                            out.push(Node::new(format!("{db}/{c}/#indexes/{name}"), &name, "index", NodeIcon::Index).detail(format!("{keys}{}", if unique { " · unique" } else { "" })));
                        }
                        Ok(out)
                    }
                }
            }
        }
    }

    async fn open(&self, node: &str, opts: OpenOptions) -> Result<Content> {
        let (db, coll, index) = split(node);
        let d = self.client.database(&db);
        match (coll, index) {
            (None, _) => {
                let s = d.run_command(doc! { "dbStats": 1 }).await.map_err(m_err)?;
                Ok(Content::Props(
                    s.into_iter()
                        .map(|(k, v)| {
                            (
                                k,
                                match v {
                                    Bson::String(s) => s,
                                    other => other.into_relaxed_extjson().to_string(),
                                },
                            )
                        })
                        .collect(),
                ))
            }
            (Some(c), Some(name)) => {
                let mut cur = d.collection::<Document>(&c).list_indexes().await.map_err(m_err)?;
                while let Some(ix) = cur.try_next().await.map_err(m_err)? {
                    if ix.options.as_ref().and_then(|o| o.name.as_deref()) == Some(name.as_str()) {
                        let json = serde_json::json!({ "key": to_json(ix.keys), "options": serde_json::to_value(&ix.options).unwrap_or_default() });
                        return Ok(Content::Json(serde_json::to_string_pretty(&json).unwrap_or_default()));
                    }
                }
                Err(Error::invalid(format!("Index {name} not found")))
            }
            (Some(c), None) => {
                let filter = parse_doc(&opts.filter)?;
                let col = d.collection::<Document>(&c);
                let limit = opts.limit.clamp(1, 5000) as i64;
                let docs: Vec<Document> = col.find(filter.clone()).skip(opts.offset).limit(limit).await.map_err(m_err)?.try_collect().await.map_err(m_err)?;
                let total = if filter.is_empty() { col.estimated_document_count().await.ok() } else { col.count_documents(filter).await.ok() };
                let note = match total {
                    Some(t) => format!("{} documents shown (from {}) of {t}{}", docs.len(), opts.offset + 1, if opts.filter.trim().is_empty() { " (estimated)" } else { " matching" }),
                    None => format!("{} documents", docs.len()),
                };
                Ok(Content::TableWithNote(docs_table(&docs), note))
            }
        }
    }

    fn filter_hint(&self, kind: &str) -> Option<&'static str> {
        matches!(kind, "collection" | "view").then_some(r#"MongoDB filter, e.g. {"status": "active", "age": {"$gt": 30}}"#)
    }

    fn actions(&self, kind: &str) -> Vec<ActionSpec> {
        let json = |k: &'static str, l: &'static str, ph: &'static str| FieldSpec::text(k, l).placeholder(ph).required();
        match kind {
            "" => vec![
                ActionSpec::new("create_db", "New database…")
                    .field(FieldSpec::text("name", "Database").required())
                    .field(FieldSpec::text("collection", "First collection").required().help("MongoDB creates a database when its first collection is created.")),
            ],
            "database" => {
                vec![ActionSpec::new("create_coll", "New collection…").primary().field(FieldSpec::text("name", "Collection").required()), ActionSpec::new("drop_db", "Drop database").destructive()]
            }
            "collection" => vec![
                ActionSpec::new("insert", "Insert document…").primary().field(json("document", "Document (JSON)", r#"{"name": "Ann", "age": 31}"#)),
                ActionSpec::new("update", "Update many…").field(json("filter_json", "Filter", r#"{"status": "pending"}"#)).field(json("update_json", "Update", r#"{"$set": {"status": "done"}}"#)),
                ActionSpec::new("delete", "Delete many…").destructive().field(json("filter_json", "Filter", r#"{"status": "cancelled"}"#)),
                ActionSpec::new("create_index", "Create index…")
                    .field(json("keys_json", "Keys", r#"{"email": 1}"#))
                    .field(FieldSpec::text("unique", "Unique").kind(FieldKind::Bool).default("false"))
                    .field(FieldSpec::text("name", "Name").placeholder("auto")),
                ActionSpec::new("export", "Export to JSON file…")
                    .field(FieldSpec::text("path", "File").kind(FieldKind::File { save: true, filters: &["json"] }).default("export.json").required())
                    .field(FieldSpec::text("filter_json", "Filter").placeholder("{} = all")),
                ActionSpec::new("import", "Import JSON file…")
                    .field(FieldSpec::text("path", "File").kind(FieldKind::File { save: false, filters: &["json"] }).required().help("A JSON array of documents, or one document per line")),
                ActionSpec::new("rename", "Rename…").field(FieldSpec::text("to", "New name").required()),
                ActionSpec::new("drop_coll", "Drop collection").destructive(),
            ],
            "view" => vec![ActionSpec::new("drop_coll", "Drop view").destructive()],
            "index" => vec![ActionSpec::new("drop_index", "Drop index").destructive()],
            _ => vec![],
        }
    }

    async fn run_action(&self, node: &str, action: &str, a: Args) -> Result<ActionOutcome> {
        let (db, coll, index) = split(node);
        let d = self.client.database(&db);
        let coll_name = || coll.clone().ok_or_else(|| Error::invalid("Select a collection"));
        if action != "export" {
            self.guard()?;
        }
        match action {
            "create_db" => {
                let (name, c) = (arg(&a, "name")?, arg(&a, "collection")?);
                self.client.database(name).create_collection(c).await.map_err(m_err)?;
                Ok(ActionOutcome::msg(format!("Created {name}.{c}")).refresh(""))
            }
            "create_coll" => {
                let name = arg(&a, "name")?;
                d.create_collection(name).await.map_err(m_err)?;
                Ok(ActionOutcome::msg(format!("Created collection {name}")).refresh(db))
            }
            "drop_db" => {
                d.drop().await.map_err(m_err)?;
                Ok(ActionOutcome::msg(format!("Dropped {db}")).refresh(""))
            }
            "insert" => {
                let doc = parse_doc(arg(&a, "document")?)?;
                let r = d.collection::<Document>(&coll_name()?).insert_one(doc).await.map_err(m_err)?;
                Ok(ActionOutcome::msg(format!("Inserted {}", r.inserted_id)))
            }
            "update" => {
                let (f, u) = (parse_doc(arg(&a, "filter_json")?)?, parse_doc(arg(&a, "update_json")?)?);
                if !u.keys().all(|k| k.starts_with('$')) {
                    return Err(Error::invalid("The update must use operators such as $set, $inc or $unset"));
                }
                let r = d.collection::<Document>(&coll_name()?).update_many(f, u).await.map_err(m_err)?;
                Ok(ActionOutcome::msg(format!("Matched {}, modified {}", r.matched_count, r.modified_count)))
            }
            "delete" => {
                let f = parse_doc(arg(&a, "filter_json")?)?;
                let r = d.collection::<Document>(&coll_name()?).delete_many(f).await.map_err(m_err)?;
                Ok(ActionOutcome::msg(format!("Deleted {} documents", r.deleted_count)))
            }
            "create_index" => {
                let keys = parse_doc(arg(&a, "keys_json")?)?;
                let mut o = IndexOptions::default();
                o.unique = Some(a.get("unique").is_some_and(|v| v == "true"));
                o.name = a.get("name").filter(|n| !n.trim().is_empty()).cloned();
                let r = d.collection::<Document>(&coll_name()?).create_index(IndexModel::builder().keys(keys).options(o).build()).await.map_err(m_err)?;
                Ok(ActionOutcome::msg(format!("Created index {}", r.index_name)).refresh(format!("{db}/{}/#indexes", coll_name()?)))
            }
            "drop_index" => {
                let name = index.ok_or_else(|| Error::invalid("Select an index"))?;
                if name == "_id_" {
                    return Err(Error::invalid("The _id index cannot be dropped"));
                }
                d.collection::<Document>(&coll_name()?).drop_index(&name).await.map_err(m_err)?;
                Ok(ActionOutcome::msg(format!("Dropped index {name}")).refresh(format!("{db}/{}/#indexes", coll_name()?)))
            }
            "rename" => {
                let to = arg(&a, "to")?;
                let from = coll_name()?;
                self.client.database("admin").run_command(doc! { "renameCollection": format!("{db}.{from}"), "to": format!("{db}.{to}") }).await.map_err(m_err)?;
                Ok(ActionOutcome::msg(format!("Renamed to {to}")).refresh(db))
            }
            "drop_coll" => {
                d.collection::<Document>(&coll_name()?).drop().await.map_err(m_err)?;
                Ok(ActionOutcome::msg(format!("Dropped {}", coll_name()?)).refresh(db))
            }
            "export" => {
                let path = arg(&a, "path")?.to_string();
                let f = parse_doc(a.get("filter_json").map(String::as_str).unwrap_or(""))?;
                let docs: Vec<Document> = d.collection::<Document>(&coll_name()?).find(f).await.map_err(m_err)?.try_collect().await.map_err(m_err)?;
                let json: Vec<serde_json::Value> = docs.into_iter().map(to_json).collect();
                let n = json.len();
                tokio::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap_or_default()).await?;
                Ok(ActionOutcome::msg(format!("Exported {n} documents to {path}")))
            }
            "import" => {
                let text = tokio::fs::read_to_string(arg(&a, "path")?).await?;
                let docs: Vec<Document> = if text.trim_start().starts_with('[') {
                    let arr: Vec<serde_json::Value> = serde_json::from_str(&text).map_err(|e| Error::invalid(format!("Not a JSON array: {e}")))?;
                    arr.into_iter().map(|v| parse_doc(&v.to_string())).collect::<Result<_>>()?
                } else {
                    text.lines().filter(|l| !l.trim().is_empty()).map(parse_doc).collect::<Result<_>>()?
                };
                if docs.is_empty() {
                    return Err(Error::invalid("No documents in file"));
                }
                let r = d.collection::<Document>(&coll_name()?).insert_many(docs).await.map_err(m_err)?;
                Ok(ActionOutcome::msg(format!("Imported {} documents", r.inserted_ids.len())))
            }
            other => Err(Error::unsupported(format!("Unknown action {other}"))),
        }
    }

    fn console_language(&self) -> Option<&'static str> {
        Some(r#"Database command (JSON), e.g. {"count": "users"}"#)
    }

    async fn console(&self, node: Option<&str>, input: &str) -> Result<Content> {
        let db = node.map(|n| split(n).0).filter(|d| !d.is_empty()).unwrap_or_else(|| "admin".into());
        let cmd = parse_doc(input)?;
        if self.read_only {
            let name = cmd.keys().next().cloned().unwrap_or_default().to_lowercase();
            const READ: &[&str] = &["find", "aggregate", "count", "distinct", "dbstats", "collstats", "listcollections", "listindexes", "ping", "buildinfo", "serverstatus", "hello", "explain"];
            if !READ.contains(&name.as_str()) {
                return Err(Error::new(ErrorKind::Invalid, format!("{name} is blocked on a read-only connection")));
            }
        }
        let out = self.client.database(&db).run_command(cmd).await.map_err(m_err)?;
        // Show cursor batches as a table when the command returned documents.
        if let Ok(c) = out.get_document("cursor")
            && let Ok(batch) = c.get_array("firstBatch")
        {
            let docs: Vec<Document> = batch.iter().filter_map(|b| b.as_document().cloned()).collect();
            return Ok(Content::TableWithNote(docs_table(&docs), format!("{} documents in first batch", docs.len())));
        }
        Ok(Content::Json(serde_json::to_string_pretty(&to_json(out)).unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn live_mongo() {
        let mut v = std::collections::BTreeMap::new();
        v.insert("host".into(), "127.0.0.1".into());
        v.insert("port".into(), "57017".into());
        v.insert("user".into(), "root".into());
        let Ok(h) = MongoDriver::default().connect(ConnectParams { values: v, secret: Some("ovcpass".into()), read_only: false, notifier: None }).await else {
            eprintln!("skipping: no MongoDB");
            return;
        };
        let s = h.resource().unwrap().clone();
        assert!(s.server_info().product.starts_with("MongoDB"));
        let _ = s.run_action("ovc_test", "drop_db", Args::new()).await;
        let mut a = Args::new();
        a.insert("name".into(), "ovc_test".into());
        a.insert("collection".into(), "users".into());
        s.run_action("", "create_db", a).await.unwrap();
        let mut a = Args::new();
        a.insert("document".into(), r#"{"name": "ann", "age": 31, "tags": ["a"], "joined": {"$date": "2024-01-02T03:04:05Z"}}"#.into());
        s.run_action("ovc_test/users", "insert", a.clone()).await.unwrap();
        a.insert("document".into(), r#"{"name": "bob", "age": 25}"#.into());
        s.run_action("ovc_test/users", "insert", a).await.unwrap();
        assert!(s.children(None).await.unwrap().iter().any(|n| n.label == "ovc_test"));
        let colls = s.children(Some("ovc_test")).await.unwrap();
        assert_eq!(colls[0].label, "users");
        let Content::TableWithNote(rs, note) = s.open("ovc_test/users", OpenOptions { limit: 10, offset: 0, filter: r#"{"age": {"$gt": 30}}"#.into() }).await.unwrap() else { panic!() };
        assert_eq!(rs.rows.len(), 1, "{note}");
        assert_eq!(rs.columns[0].name, "_id");
        let name_col = rs.column_index("name").unwrap();
        assert_eq!(rs.rows[0][name_col], Value::Text("ann".into()));
        let joined = rs.column_index("joined").unwrap();
        assert!(rs.rows[0][joined].to_text().unwrap().starts_with("2024-01-02T03:04:05"), "{:?}", rs.rows[0][joined]);
        let mut a = Args::new();
        a.insert("filter_json".into(), r#"{"name": "bob"}"#.into());
        a.insert("update_json".into(), r#"{"$inc": {"age": 1}}"#.into());
        assert!(s.run_action("ovc_test/users", "update", a).await.unwrap().message.contains("modified 1"));
        let mut a = Args::new();
        a.insert("keys_json".into(), r#"{"name": 1}"#.into());
        a.insert("unique".into(), "true".into());
        s.run_action("ovc_test/users", "create_index", a).await.unwrap();
        let idx = s.children(Some("ovc_test/users/#indexes")).await.unwrap();
        assert!(idx.iter().any(|i| i.label == "name_1"), "{:?}", idx.iter().map(|i| &i.label).collect::<Vec<_>>());
        let Content::TableWithNote(rs, _) = s.console(Some("ovc_test"), r#"{"find": "users", "filter": {}}"#).await.unwrap() else { panic!() };
        assert_eq!(rs.rows.len(), 2);
        let file = std::env::temp_dir().join("ovc-mongo-export.json");
        let mut a = Args::new();
        a.insert("path".into(), file.display().to_string());
        s.run_action("ovc_test/users", "export", a.clone()).await.unwrap();
        let mut d = Args::new();
        d.insert("filter_json".into(), "{}".into());
        s.run_action("ovc_test/users", "delete", d).await.unwrap();
        let mut a2 = Args::new();
        a2.insert("path".into(), file.display().to_string());
        assert!(s.run_action("ovc_test/users", "import", a2).await.unwrap().message.contains("Imported 2"));
        assert!(parse_doc("[1]").is_err());
        s.run_action("ovc_test", "drop_db", Args::new()).await.unwrap();
    }
}
