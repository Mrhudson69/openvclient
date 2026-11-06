//! Elasticsearch / OpenSearch over the REST API.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use openvclient_core::resource::*;
use openvclient_core::{Category, ConnectParams, Driver, DriverInfo, Error, FieldKind, FieldSpec, Result, ServerInfo, ServiceSession, SessionHandle};
use reqwest::Method;
use serde_json::{Value as J, json};

use crate::{Http, objects_table};

pub struct ElasticDriver {
    info: DriverInfo,
}

impl Default for ElasticDriver {
    fn default() -> Self {
        Self {
            info: DriverInfo {
                id: "elasticsearch",
                name: "Elasticsearch",
                category: Category::Search,
                description: "Elasticsearch 7/8 and OpenSearch over REST",
                badge: "ES",
                fields: vec![
                    FieldSpec::text("host", "Host").default("localhost").required(),
                    FieldSpec::text("port", "Port").kind(FieldKind::Number).default("9200"),
                    FieldSpec::text("tls", "HTTPS").kind(FieldKind::Bool).default("false"),
                    FieldSpec::text("auth", "Authentication").kind(FieldKind::Choice(&["basic", "api_key", "none"])).default("basic"),
                    FieldSpec::text("user", "User").default("elastic"),
                    FieldSpec::text("password", "Password / API key").kind(FieldKind::Secret),
                    FieldSpec::text("insecure_tls", "Skip certificate check").kind(FieldKind::Bool).default("false").advanced(),
                    FieldSpec::text("url", "Base URL").placeholder("https://… (overrides host/port)").advanced(),
                ],
            },
        }
    }
}

#[async_trait]
impl Driver for ElasticDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }

    async fn connect(&self, mut p: ConnectParams) -> Result<SessionHandle> {
        if p.get("auth") == Some("none") {
            p.values.remove("user");
            p.secret = None;
        }
        let http = Http::from_params(&p, 9200)?;
        let root = http.json(http.req(Method::GET, "/")).await?;
        let version = root.pointer("/version/number").and_then(|v| v.as_str()).unwrap_or("?").to_string();
        let distro = root.pointer("/version/distribution").and_then(|v| v.as_str()).unwrap_or("elasticsearch");
        let product = if distro == "opensearch" { format!("OpenSearch {version}") } else { format!("Elasticsearch {version}") };
        let cluster = root.get("cluster_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        Ok(SessionHandle::Resource(Arc::new(EsSession { http, info: ServerInfo { product, version, details: vec![("Cluster".into(), cluster)] }, read_only: p.read_only })))
    }
}

struct EsSession {
    http: Http,
    info: ServerInfo,
    read_only: bool,
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~*,".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

impl EsSession {
    async fn call(&self, m: Method, path: &str, body: Option<J>) -> Result<J> {
        let mut r = self.http.req(m, path);
        if let Some(b) = body {
            r = r.json(&b);
        }
        self.http.json(r).await
    }

    fn guard(&self) -> Result<()> {
        if self.read_only { Err(Error::invalid("Connection is read-only")) } else { Ok(()) }
    }
}

/// Search hits → table with _id, _score and flattened _source fields.
fn hits_table(v: &J) -> (openvclient_core::ResultSet, String) {
    let hits = v.pointer("/hits/hits").and_then(|h| h.as_array()).cloned().unwrap_or_default();
    let total = v.pointer("/hits/total/value").and_then(|t| t.as_u64()).or_else(|| v.pointer("/hits/total").and_then(|t| t.as_u64()));
    let rel = v.pointer("/hits/total/relation").and_then(|t| t.as_str()).unwrap_or("eq");
    let rows: Vec<J> = hits
        .iter()
        .map(|h| {
            let mut o = serde_json::Map::new();
            o.insert("_id".into(), h.get("_id").cloned().unwrap_or(J::Null));
            o.insert("_score".into(), h.get("_score").cloned().unwrap_or(J::Null));
            if let Some(src) = h.get("_source").and_then(|s| s.as_object()) {
                for (k, v) in src {
                    o.insert(k.clone(), v.clone());
                }
            }
            J::Object(o)
        })
        .collect();
    let took = v.get("took").and_then(|t| t.as_u64()).unwrap_or(0);
    let note = match total {
        Some(t) => format!("{} hits shown of {}{t} · {took} ms", rows.len(), if rel == "gte" { "≥" } else { "" }),
        None => format!("{} hits · {took} ms", rows.len()),
    };
    (objects_table(&rows, &["_id", "_score"]), note)
}

#[async_trait]
impl ServiceSession for EsSession {
    fn server_info(&self) -> ServerInfo {
        self.info.clone()
    }
    async fn ping(&self) -> Result<Duration> {
        let t = Instant::now();
        self.call(Method::GET, "/_cluster/health", None).await?;
        Ok(t.elapsed())
    }
    async fn close(&self) {}
}

#[async_trait]
impl ResourceSession for EsSession {
    async fn children(&self, parent: Option<&str>) -> Result<Vec<Node>> {
        match parent {
            None => {
                let v = self.call(Method::GET, "/_cat/indices?format=json&h=index,health,status,docs.count,store.size&s=index", None).await?;
                let mut out: Vec<Node> = vec![Node::new("#cluster", "Cluster health", "cluster", NodeIcon::Server)];
                let mut user = Vec::new();
                let mut system = 0;
                for i in v.as_array().cloned().unwrap_or_default() {
                    let name = i.get("index").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    if name.starts_with('.') {
                        system += 1;
                        continue;
                    }
                    let detail = format!(
                        "{} · {} docs · {}",
                        i.get("health").and_then(|x| x.as_str()).unwrap_or(""),
                        i.get("docs.count").and_then(|x| x.as_str()).unwrap_or("0"),
                        i.get("store.size").and_then(|x| x.as_str()).unwrap_or("")
                    );
                    let closed = i.get("status").and_then(|x| x.as_str()) == Some("close");
                    user.push(Node::new(format!("i/{name}"), &name, if closed { "closed_index" } else { "index" }, NodeIcon::Index).folder().detail(if closed {
                        "closed".to_string()
                    } else {
                        detail
                    }));
                }
                out.extend(user);
                if system > 0 {
                    out.push(Node::new("#system", format!("{system} hidden system indices"), "info", NodeIcon::Info).container());
                }
                Ok(out)
            }
            Some(id) if id.starts_with("i/") => {
                let name = &id[2..];
                Ok(vec![Node::new(format!("m/{name}"), "Mapping", "mapping", NodeIcon::File), Node::new(format!("s/{name}"), "Settings", "settings", NodeIcon::File)])
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn open(&self, node: &str, opts: OpenOptions) -> Result<Content> {
        let pretty = |v: &J| serde_json::to_string_pretty(v).unwrap_or_default();
        if node == "#cluster" {
            let h = self.call(Method::GET, "/_cluster/health", None).await?;
            return Ok(Content::Props(h.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))).collect()).unwrap_or_default()));
        }
        if let Some(name) = node.strip_prefix("m/") {
            return Ok(Content::Json(pretty(&self.call(Method::GET, &format!("/{}/_mapping", enc(name)), None).await?)));
        }
        if let Some(name) = node.strip_prefix("s/") {
            return Ok(Content::Json(pretty(&self.call(Method::GET, &format!("/{}/_settings", enc(name)), None).await?)));
        }
        let name = node.strip_prefix("i/").ok_or_else(|| Error::invalid("Unknown node"))?;
        let f = opts.filter.trim();
        let body = if f.starts_with('{') {
            // Full Query DSL; paging fields are added unless given.
            let mut b: J = serde_json::from_str(f).map_err(|e| Error::invalid(format!("Not valid JSON: {e}")))?;
            if let Some(o) = b.as_object_mut()
                && !o.contains_key("query")
                && !o.contains_key("aggs")
                && !o.contains_key("size")
            {
                b = json!({ "query": b });
            }
            let o = b.as_object_mut().unwrap();
            o.entry("size").or_insert(json!(opts.limit));
            o.entry("from").or_insert(json!(opts.offset));
            o.entry("track_total_hits").or_insert(json!(true));
            b
        } else if f.is_empty() {
            json!({ "query": { "match_all": {} }, "size": opts.limit, "from": opts.offset, "track_total_hits": true })
        } else {
            json!({ "query": { "query_string": { "query": f } }, "size": opts.limit, "from": opts.offset, "track_total_hits": true })
        };
        let v = self.call(Method::POST, &format!("/{}/_search", enc(name)), Some(body)).await?;
        if let Some(aggs) = v.get("aggregations")
            && v.pointer("/hits/hits").and_then(|h| h.as_array()).is_none_or(|h| h.is_empty())
        {
            return Ok(Content::Json(pretty(aggs)));
        }
        let (rs, note) = hits_table(&v);
        Ok(Content::TableWithNote(rs, note))
    }

    fn filter_hint(&self, kind: &str) -> Option<&'static str> {
        (kind == "index").then_some(r#"Query string (status:active AND age:>30) or Query DSL JSON ({"match": {"title": "rust"}})"#)
    }

    fn actions(&self, kind: &str) -> Vec<ActionSpec> {
        match kind {
            "" => vec![
                ActionSpec::new("create_index", "New index…")
                    .field(FieldSpec::text("name", "Index").required())
                    .field(FieldSpec::text("shards", "Shards").kind(FieldKind::Number).default("1"))
                    .field(FieldSpec::text("replicas", "Replicas").kind(FieldKind::Number).default("0"))
                    .field(FieldSpec::text("mappings_json", "Mappings (JSON)").placeholder(r#"{"properties": {"title": {"type": "text"}}}"#)),
            ],
            "index" => vec![
                ActionSpec::new("index_doc", "Index document…").primary().field(FieldSpec::text("document_json", "Document (JSON)").required()).field(FieldSpec::text("id", "ID").placeholder("auto")),
                ActionSpec::new("delete_doc", "Delete document by ID…").destructive().field(FieldSpec::text("id", "ID").required()),
                ActionSpec::new("delete_by_query", "Delete by query…").destructive().field(FieldSpec::text("query_json", "Query (JSON)").required().placeholder(r#"{"term": {"status": "old"}}"#)),
                ActionSpec::new("refresh", "Refresh"),
                ActionSpec::new("close", "Close index").destructive(),
                ActionSpec::new("delete_index", "Delete index").destructive(),
            ],
            "closed_index" => vec![ActionSpec::new("open", "Open index").primary(), ActionSpec::new("delete_index", "Delete index").destructive()],
            _ => vec![],
        }
    }

    async fn run_action(&self, node: &str, action: &str, a: Args) -> Result<ActionOutcome> {
        self.guard()?;
        let index = node.strip_prefix("i/").unwrap_or(node).to_string();
        let path = |suffix: &str| format!("/{}{suffix}", enc(&index));
        match action {
            "create_index" => {
                let name = arg(&a, "name")?;
                let mut body = json!({ "settings": { "number_of_shards": a.get("shards").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1), "number_of_replicas": a.get("replicas").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0) } });
                if let Some(m) = a.get("mappings_json").filter(|m| !m.trim().is_empty()) {
                    body["mappings"] = serde_json::from_str(m).map_err(|e| Error::invalid(format!("Mappings are not valid JSON: {e}")))?;
                }
                self.call(Method::PUT, &format!("/{}", enc(name)), Some(body)).await?;
                Ok(ActionOutcome::msg(format!("Created index {name}")).refresh(""))
            }
            "index_doc" => {
                let doc: J = serde_json::from_str(arg(&a, "document_json")?).map_err(|e| Error::invalid(format!("Not valid JSON: {e}")))?;
                let v = match a.get("id").filter(|i| !i.trim().is_empty()) {
                    Some(id) => self.call(Method::PUT, &path(&format!("/_doc/{}?refresh=wait_for", enc(id))), Some(doc)).await?,
                    None => self.call(Method::POST, &path("/_doc?refresh=wait_for"), Some(doc)).await?,
                };
                Ok(ActionOutcome::msg(format!("{} document {}", v.get("result").and_then(|r| r.as_str()).unwrap_or("indexed"), v.get("_id").and_then(|r| r.as_str()).unwrap_or(""))))
            }
            "delete_doc" => {
                let v = self.call(Method::DELETE, &path(&format!("/_doc/{}?refresh=wait_for", enc(arg(&a, "id")?))), None).await?;
                Ok(ActionOutcome::msg(format!("Document {}", v.get("result").and_then(|r| r.as_str()).unwrap_or("deleted"))))
            }
            "delete_by_query" => {
                let q: J = serde_json::from_str(arg(&a, "query_json")?).map_err(|e| Error::invalid(format!("Not valid JSON: {e}")))?;
                let v = self.call(Method::POST, &path("/_delete_by_query?refresh=true"), Some(json!({ "query": q }))).await?;
                Ok(ActionOutcome::msg(format!("Deleted {} documents", v.get("deleted").and_then(|d| d.as_u64()).unwrap_or(0))))
            }
            "refresh" => {
                self.call(Method::POST, &path("/_refresh"), None).await?;
                Ok(ActionOutcome::msg("Index refreshed"))
            }
            "close" => {
                self.call(Method::POST, &path("/_close"), None).await?;
                Ok(ActionOutcome::msg(format!("Closed {index}")).refresh(""))
            }
            "open" => {
                self.call(Method::POST, &path("/_open"), None).await?;
                Ok(ActionOutcome::msg(format!("Opened {index}")).refresh(""))
            }
            "delete_index" => {
                self.call(Method::DELETE, &path(""), None).await?;
                Ok(ActionOutcome::msg(format!("Deleted index {index}")).refresh(""))
            }
            other => Err(Error::unsupported(format!("Unknown action {other}"))),
        }
    }

    fn console_language(&self) -> Option<&'static str> {
        Some("REST: METHOD /path {json body}, e.g. GET /_cat/nodes?v")
    }

    async fn console(&self, _node: Option<&str>, input: &str) -> Result<Content> {
        let input = input.trim();
        let (head, body) = match input.find('{') {
            Some(i) => (input[..i].trim(), Some(input[i..].trim())),
            None => (input, None),
        };
        let mut it = head.split_whitespace();
        let method = it.next().unwrap_or("GET").to_ascii_uppercase();
        let path = it.next().ok_or_else(|| Error::invalid("Write METHOD /path, e.g. GET /_cluster/health"))?;
        let m = Method::from_bytes(method.as_bytes()).map_err(|_| Error::invalid(format!("Unknown method {method}")))?;
        if self.read_only && m != Method::GET && !(m == Method::POST && (path.contains("_search") || path.contains("_count"))) {
            return Err(Error::invalid(format!("{method} is blocked on a read-only connection")));
        }
        let mut r = self.http.req(m, path);
        if let Some(b) = body {
            let v: J = serde_json::from_str(b).map_err(|e| Error::invalid(format!("Body is not valid JSON: {e}")))?;
            r = r.json(&v);
        }
        let (status, text) = self.http.send(r).await?;
        if path.contains("_search")
            && let Ok(v) = serde_json::from_str::<J>(&text)
            && v.pointer("/hits/hits").is_some()
            && status < 400
        {
            let (rs, note) = hits_table(&v);
            return Ok(Content::TableWithNote(rs, note));
        }
        if status >= 400 {
            return Err(Error::server(format!("HTTP {status}")).with_detail(text));
        }
        Ok(match serde_json::from_str::<J>(&text) {
            Ok(v) => Content::Json(serde_json::to_string_pretty(&v).unwrap_or(text)),
            Err(_) => Content::Text(text),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn live_elasticsearch() {
        let mut v = std::collections::BTreeMap::new();
        v.insert("host".into(), "127.0.0.1".into());
        v.insert("port".into(), "59200".into());
        v.insert("auth".into(), "none".into());
        let Ok(h) = ElasticDriver::default().connect(ConnectParams { values: v, secret: None, read_only: false, notifier: None }).await else {
            eprintln!("skipping: no Elasticsearch");
            return;
        };
        let s = h.resource().unwrap().clone();
        assert!(s.server_info().product.starts_with("Elasticsearch"));
        let _ = s.run_action("i/ovc-test", "delete_index", Args::new()).await;
        let mut a = Args::new();
        a.insert("name".into(), "ovc-test".into());
        a.insert("mappings_json".into(), r#"{"properties": {"title": {"type": "text"}, "views": {"type": "integer"}}}"#.into());
        s.run_action("", "create_index", a).await.unwrap();
        for (id, title, views) in [("1", "rust databases", 10), ("2", "egui desktop apps", 5), ("3", "rust tooling", 1)] {
            let mut a = Args::new();
            a.insert("id".into(), id.into());
            a.insert("document_json".into(), format!(r#"{{"title": "{title}", "views": {views}}}"#));
            s.run_action("i/ovc-test", "index_doc", a).await.unwrap();
        }
        assert!(s.children(None).await.unwrap().iter().any(|n| n.label == "ovc-test"));
        let Content::TableWithNote(rs, note) = s.open("i/ovc-test", OpenOptions { limit: 10, offset: 0, filter: "title:rust".into() }).await.unwrap() else { panic!() };
        assert_eq!(rs.rows.len(), 2, "{note}");
        let Content::TableWithNote(rs, _) = s.open("i/ovc-test", OpenOptions { limit: 10, offset: 0, filter: r#"{"range": {"views": {"gte": 5}}}"#.into() }).await.unwrap() else { panic!() };
        assert_eq!(rs.rows.len(), 2);
        assert!(matches!(s.open("m/ovc-test", OpenOptions::default()).await.unwrap(), Content::Json(j) if j.contains("\"views\"")));
        let mut d = Args::new();
        d.insert("query_json".into(), r#"{"term": {"views": 1}}"#.into());
        assert!(s.run_action("i/ovc-test", "delete_by_query", d).await.unwrap().message.contains("Deleted 1"));
        assert!(matches!(s.console(None, "GET /_cat/health?format=json").await.unwrap(), Content::Json(_)));
        let Content::TableWithNote(rs, _) = s.console(None, r#"POST /ovc-test/_search {"query": {"match_all": {}}}"#).await.unwrap() else { panic!() };
        assert_eq!(rs.rows.len(), 2);
        s.run_action("i/ovc-test", "close", Args::new()).await.unwrap();
        s.run_action("i/ovc-test", "open", Args::new()).await.unwrap();
        s.run_action("i/ovc-test", "delete_index", Args::new()).await.unwrap();
    }
}
