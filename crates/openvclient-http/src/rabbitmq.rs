//! RabbitMQ through the management plugin's HTTP API.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use openvclient_core::resource::*;
use openvclient_core::{Category, ConnectParams, Driver, DriverInfo, Error, FieldKind, FieldSpec, Result, ServerInfo, ServiceSession, SessionHandle};
use reqwest::Method;
use serde_json::{Value as J, json};

use crate::{Http, objects_table};

pub struct RabbitDriver {
    info: DriverInfo,
}

impl Default for RabbitDriver {
    fn default() -> Self {
        Self {
            info: DriverInfo {
                id: "rabbitmq",
                name: "RabbitMQ",
                category: Category::Messaging,
                description: "RabbitMQ via the management plugin (HTTP API)",
                badge: "MQ",
                fields: vec![
                    FieldSpec::text("host", "Host").default("localhost").required(),
                    FieldSpec::text("port", "Management port").kind(FieldKind::Number).default("15672").help("The management plugin's HTTP port, not AMQP 5672"),
                    FieldSpec::text("tls", "HTTPS").kind(FieldKind::Bool).default("false"),
                    FieldSpec::text("user", "User").default("guest"),
                    FieldSpec::text("password", "Password").kind(FieldKind::Secret),
                    FieldSpec::text("insecure_tls", "Skip certificate check").kind(FieldKind::Bool).default("false").advanced(),
                    FieldSpec::text("url", "Base URL").placeholder("https://… (overrides host/port)").advanced(),
                ],
            },
        }
    }
}

/// Percent-encode one path segment (vhost "/" becomes %2F).
fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

#[async_trait]
impl Driver for RabbitDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }

    async fn connect(&self, p: ConnectParams) -> Result<SessionHandle> {
        let mut p = p;
        if p.get("user").is_none() {
            p.values.insert("user".into(), "guest".into());
        }
        let http = Http::from_params(&p, 15672)?;
        let s = RabbitSession { http, info: ServerInfo::default(), read_only: p.read_only };
        let ov = s.call(Method::GET, "/api/overview", None).await?;
        let version = ov.get("rabbitmq_version").and_then(|v| v.as_str()).unwrap_or("?").to_string();
        let mut details = Vec::new();
        for (k, label) in [("cluster_name", "Cluster"), ("node", "Node"), ("erlang_version", "Erlang"), ("management_version", "Management")] {
            if let Some(v) = ov.get(k).and_then(|v| v.as_str()) {
                details.push((label.to_string(), v.to_string()));
            }
        }
        let info = ServerInfo { product: format!("RabbitMQ {version}"), version, details };
        Ok(SessionHandle::Resource(Arc::new(RabbitSession { info, ..s })))
    }
}

struct RabbitSession {
    http: Http,
    info: ServerInfo,
    read_only: bool,
}

impl RabbitSession {
    async fn call(&self, m: Method, path: &str, body: Option<J>) -> Result<J> {
        let mut r = self.http.req(m, path);
        if let Some(b) = body {
            r = r.json(&b);
        }
        let (status, text) = self.http.send(r).await?;
        if status == 404 && path == "/api/overview" {
            return Err(Error::connection("No RabbitMQ management API here. Enable it with `rabbitmq-plugins enable rabbitmq_management` and use its port (default 15672)."));
        }
        let v: J = if text.trim().is_empty() { J::Null } else { serde_json::from_str(&text).unwrap_or(J::String(text.clone())) };
        if status >= 400 {
            let reason = v.get("reason").and_then(|r| r.as_str()).map(str::to_owned).unwrap_or_else(|| crate::first_line(&text));
            return Err(Error::server(format!("HTTP {status}: {reason}")));
        }
        Ok(v)
    }

    fn guard(&self) -> Result<()> {
        if self.read_only { Err(Error::invalid("Connection is read-only")) } else { Ok(()) }
    }
}

/// `kind/vhost/name` ids; the vhost is percent-encoded so it may contain '/'.
fn split(id: &str) -> Option<(String, String)> {
    let (vh, name) = id.split_once('/')?;
    Some((percent_decode(vh), name.to_string()))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn num(v: &J, k: &str) -> u64 {
    v.get(k).and_then(|x| x.as_u64()).unwrap_or(0)
}

fn props(v: &J, keys: &[&str]) -> Vec<(String, String)> {
    keys.iter()
        .filter_map(|k| {
            let x = v.pointer(&format!("/{}", k.replace('.', "/")))?;
            Some((k.to_string(), x.as_str().map(str::to_owned).unwrap_or_else(|| x.to_string())))
        })
        .collect()
}

#[async_trait]
impl ServiceSession for RabbitSession {
    fn server_info(&self) -> ServerInfo {
        self.info.clone()
    }
    async fn ping(&self) -> Result<Duration> {
        let t = Instant::now();
        self.call(Method::GET, "/api/whoami", None).await?;
        Ok(t.elapsed())
    }
    async fn close(&self) {}
}

#[async_trait]
impl ResourceSession for RabbitSession {
    async fn children(&self, parent: Option<&str>) -> Result<Vec<Node>> {
        match parent {
            None => {
                let mut out = vec![
                    Node::new("#overview", "Overview", "overview", NodeIcon::Server),
                    Node::new("#connections", "Connections", "connections", NodeIcon::Info),
                    Node::new("#users", "Users", "users", NodeIcon::Info),
                ];
                let vhosts = self.call(Method::GET, "/api/vhosts", None).await?;
                for v in vhosts.as_array().cloned().unwrap_or_default() {
                    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                    let d = format!("{} msgs", num(&v, "messages"));
                    out.push(Node::new(format!("v/{}", enc(&name)), format!("vhost {name}"), "vhost", NodeIcon::Folder).container().detail(d));
                }
                Ok(out)
            }
            Some(id) if id.starts_with("v/") => {
                let vh = &id[2..];
                Ok(vec![Node::new(format!("qs/{vh}"), "Queues", "queues", NodeIcon::Folder).folder(), Node::new(format!("xs/{vh}"), "Exchanges", "exchanges", NodeIcon::Folder).folder()])
            }
            Some(id) if id.starts_with("qs/") => {
                let vh = &id[3..];
                let v = self.call(Method::GET, &format!("/api/queues/{vh}?columns=name,messages,messages_ready,consumers,state,type"), None).await?;
                Ok(v.as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|q| {
                        let name = q.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        let d = format!("{} msgs · {} consumers", num(q, "messages"), num(q, "consumers"));
                        Node::new(format!("q/{vh}/{name}"), name, "queue", NodeIcon::Queue).detail(d)
                    })
                    .collect())
            }
            Some(id) if id.starts_with("xs/") => {
                let vh = &id[3..];
                let v = self.call(Method::GET, &format!("/api/exchanges/{vh}"), None).await?;
                Ok(v.as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|x| {
                        let name = x.get("name").and_then(|n| n.as_str())?;
                        let label = if name.is_empty() { "(default)" } else { name };
                        let kind = if name.is_empty() || name.starts_with("amq.") { "builtin_exchange" } else { "exchange" };
                        Some(Node::new(format!("x/{vh}/{name}"), label, kind, NodeIcon::Exchange).detail(x.get("type").and_then(|t| t.as_str()).unwrap_or("")))
                    })
                    .collect())
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn open(&self, node: &str, _opts: OpenOptions) -> Result<Content> {
        match node {
            "#overview" => {
                let v = self.call(Method::GET, "/api/overview", None).await?;
                Ok(Content::Props(props(
                    &v,
                    &[
                        "rabbitmq_version",
                        "erlang_version",
                        "cluster_name",
                        "node",
                        "queue_totals.messages",
                        "queue_totals.messages_ready",
                        "queue_totals.messages_unacknowledged",
                        "object_totals.connections",
                        "object_totals.channels",
                        "object_totals.queues",
                        "object_totals.exchanges",
                        "object_totals.consumers",
                        "message_stats.publish",
                        "message_stats.deliver_get",
                    ],
                )))
            }
            "#connections" => {
                let v = self.call(Method::GET, "/api/connections?columns=name,user,vhost,state,channels,protocol,client_properties.product", None).await?;
                Ok(Content::Table(objects_table(v.as_array().map(|a| a.as_slice()).unwrap_or(&[]), &["name", "user", "vhost", "state", "channels", "protocol"])))
            }
            "#users" => {
                let v = self.call(Method::GET, "/api/users", None).await?;
                let rows: Vec<J> = v.as_array().cloned().unwrap_or_default().into_iter().map(|u| json!({"name": u["name"], "tags": u["tags"]})).collect();
                Ok(Content::Table(objects_table(&rows, &["name", "tags"])))
            }
            _ => {
                if let Some(rest) = node.strip_prefix("q/") {
                    let (vh, name) = split(rest).ok_or_else(|| Error::invalid("Bad node"))?;
                    let base = format!("/api/queues/{}/{}", enc(&vh), enc(&name));
                    let q = self.call(Method::GET, &base, None).await?;
                    let mut p = props(
                        &q,
                        &["name", "vhost", "type", "state", "durable", "auto_delete", "exclusive", "messages", "messages_ready", "messages_unacknowledged", "consumers", "memory", "node", "arguments"],
                    );
                    let b = self.call(Method::GET, &format!("{base}/bindings"), None).await?;
                    for x in b.as_array().cloned().unwrap_or_default() {
                        let src = x.get("source").and_then(|s| s.as_str()).unwrap_or("");
                        p.push(("binding".into(), format!("{} → key '{}'", if src.is_empty() { "(default)" } else { src }, x.get("routing_key").and_then(|s| s.as_str()).unwrap_or(""))));
                    }
                    return Ok(Content::Props(p));
                }
                if let Some(rest) = node.strip_prefix("x/") {
                    let (vh, name) = split(rest).ok_or_else(|| Error::invalid("Bad node"))?;
                    let ex = if name.is_empty() { "amq.default".to_string() } else { name.clone() };
                    let x = self.call(Method::GET, &format!("/api/exchanges/{}/{}", enc(&vh), enc(&ex)), None).await?;
                    let mut p = props(&x, &["name", "vhost", "type", "durable", "auto_delete", "internal", "arguments"]);
                    let b = self.call(Method::GET, &format!("/api/exchanges/{}/{}/bindings/source", enc(&vh), enc(&ex)), None).await?;
                    for x in b.as_array().cloned().unwrap_or_default() {
                        p.push((
                            "binding".into(),
                            format!(
                                "→ {} {} key '{}'",
                                x.get("destination_type").and_then(|s| s.as_str()).unwrap_or(""),
                                x.get("destination").and_then(|s| s.as_str()).unwrap_or(""),
                                x.get("routing_key").and_then(|s| s.as_str()).unwrap_or("")
                            ),
                        ));
                    }
                    return Ok(Content::Props(p));
                }
                Err(Error::invalid("Unknown node"))
            }
        }
    }

    fn actions(&self, kind: &str) -> Vec<ActionSpec> {
        let publish = || {
            ActionSpec::new("publish", "Publish message…")
                .primary()
                .field(FieldSpec::text("payload", "Payload").required())
                .field(FieldSpec::text("routing_key", "Routing key"))
                .field(FieldSpec::text("headers_json", "Headers (JSON)").placeholder(r#"{"x-source": "openvclient"}"#))
                .field(FieldSpec::text("persistent", "Persistent").kind(FieldKind::Bool).default("true"))
        };
        match kind {
            "vhost" => vec![
                ActionSpec::new("create_queue", "New queue…")
                    .field(FieldSpec::text("name", "Name").required())
                    .field(FieldSpec::text("type", "Type").kind(FieldKind::Choice(&["classic", "quorum", "stream"])).default("classic"))
                    .field(FieldSpec::text("durable", "Durable").kind(FieldKind::Bool).default("true")),
                ActionSpec::new("create_exchange", "New exchange…")
                    .field(FieldSpec::text("name", "Name").required())
                    .field(FieldSpec::text("type", "Type").kind(FieldKind::Choice(&["direct", "fanout", "topic", "headers"])).default("direct"))
                    .field(FieldSpec::text("durable", "Durable").kind(FieldKind::Bool).default("true")),
            ],
            "queue" => vec![
                ActionSpec::new("get", "Get messages…").primary().field(FieldSpec::text("count", "Count").kind(FieldKind::Number).default("10")).field(
                    FieldSpec::text("ackmode", "Mode")
                        .kind(FieldKind::Choice(&["requeue", "ack"]))
                        .default("requeue")
                        .help("requeue: peek and put back (marks them redelivered) · ack: remove them from the queue"),
                ),
                ActionSpec::new("publish_queue", "Publish to queue…").field(FieldSpec::text("payload", "Payload").required()),
                ActionSpec::new("purge", "Purge messages").destructive(),
                ActionSpec::new("delete_queue", "Delete queue").destructive(),
            ],
            "exchange" => vec![
                publish(),
                ActionSpec::new("bind", "Bind queue…").field(FieldSpec::text("queue", "Queue").required()).field(FieldSpec::text("routing_key", "Routing key")),
                ActionSpec::new("delete_exchange", "Delete exchange").destructive(),
            ],
            "builtin_exchange" => vec![publish()],
            _ => vec![],
        }
    }

    async fn run_action(&self, node: &str, action: &str, a: Args) -> Result<ActionOutcome> {
        let flag = |k: &str| a.get(k).is_some_and(|v| v == "true");
        match action {
            "get" => {} // reading; "ack" mode is checked below
            _ => self.guard()?,
        }
        match action {
            "create_queue" | "create_exchange" => {
                let vh = node.strip_prefix("v/").ok_or_else(|| Error::invalid("Pick a vhost"))?;
                let name = arg(&a, "name")?;
                let ty = a.get("type").cloned().unwrap_or_default();
                if action == "create_queue" {
                    let body = json!({ "durable": flag("durable") || ty == "quorum" || ty == "stream", "auto_delete": false, "arguments": { "x-queue-type": ty } });
                    self.call(Method::PUT, &format!("/api/queues/{vh}/{}", enc(name)), Some(body)).await?;
                    Ok(ActionOutcome::msg(format!("Created queue {name}")).refresh(format!("qs/{vh}")))
                } else {
                    self.call(Method::PUT, &format!("/api/exchanges/{vh}/{}", enc(name)), Some(json!({ "type": ty, "durable": flag("durable") }))).await?;
                    Ok(ActionOutcome::msg(format!("Created exchange {name}")).refresh(format!("xs/{vh}")))
                }
            }
            "get" | "publish_queue" | "purge" | "delete_queue" => {
                let (vh, name) = split(node.strip_prefix("q/").ok_or_else(|| Error::invalid("Not a queue"))?).ok_or_else(|| Error::invalid("Bad node"))?;
                let base = format!("/api/queues/{}/{}", enc(&vh), enc(&name));
                match action {
                    "get" => {
                        let ack = a.get("ackmode").map(String::as_str) == Some("ack");
                        if ack {
                            self.guard()?;
                        }
                        let count: u64 = a.get("count").and_then(|c| c.parse().ok()).unwrap_or(10).clamp(1, 1000);
                        let v = self
                            .call(
                                Method::POST,
                                &format!("{base}/get"),
                                Some(json!({ "count": count, "ackmode": if ack { "ack_requeue_false" } else { "ack_requeue_true" }, "encoding": "auto", "truncate": 50000 })),
                            )
                            .await?;
                        let rows: Vec<J> = v
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .map(|m| json!({ "payload": m["payload"], "routing_key": m["routing_key"], "exchange": m["exchange"], "redelivered": m["redelivered"], "properties": m["properties"], "encoding": m["payload_encoding"] }))
                            .collect();
                        let n = rows.len();
                        Ok(ActionOutcome::msg(format!("{n} message(s) {}", if ack { "taken" } else { "peeked and requeued" }))
                            .show(Content::Table(objects_table(&rows, &["payload", "routing_key", "exchange", "redelivered"]))))
                    }
                    "publish_queue" => {
                        let body = json!({ "properties": {}, "routing_key": name, "payload": arg(&a, "payload")?, "payload_encoding": "string" });
                        let r = self.call(Method::POST, &format!("/api/exchanges/{}/amq.default/publish", enc(&vh)), Some(body)).await?;
                        let routed = r.get("routed").and_then(|x| x.as_bool()).unwrap_or(false);
                        Ok(ActionOutcome::msg(if routed { "Message published" } else { "Message was not routed" }).refresh(format!("qs/{}", enc(&vh))))
                    }
                    "purge" => {
                        self.call(Method::DELETE, &format!("{base}/contents"), None).await?;
                        Ok(ActionOutcome::msg(format!("Purged {name}")).refresh(format!("qs/{}", enc(&vh))))
                    }
                    _ => {
                        self.call(Method::DELETE, &base, None).await?;
                        Ok(ActionOutcome::msg(format!("Deleted queue {name}")).refresh(format!("qs/{}", enc(&vh))))
                    }
                }
            }
            "publish" | "bind" | "delete_exchange" => {
                let (vh, name) = split(node.strip_prefix("x/").ok_or_else(|| Error::invalid("Not an exchange"))?).ok_or_else(|| Error::invalid("Bad node"))?;
                let ex = if name.is_empty() { "amq.default".to_string() } else { name.clone() };
                match action {
                    "publish" => {
                        let mut props = json!({ "delivery_mode": if flag("persistent") { 2 } else { 1 } });
                        if let Some(h) = a.get("headers_json").filter(|h| !h.trim().is_empty()) {
                            props["headers"] = serde_json::from_str(h).map_err(|e| Error::invalid(format!("Headers are not valid JSON: {e}")))?;
                        }
                        let body = json!({ "properties": props, "routing_key": a.get("routing_key").cloned().unwrap_or_default(), "payload": arg(&a, "payload")?, "payload_encoding": "string" });
                        let r = self.call(Method::POST, &format!("/api/exchanges/{}/{}/publish", enc(&vh), enc(&ex)), Some(body)).await?;
                        let routed = r.get("routed").and_then(|x| x.as_bool()).unwrap_or(false);
                        Ok(ActionOutcome::msg(if routed { "Message published and routed" } else { "Published, but no queue matched the routing key" }))
                    }
                    "bind" => {
                        let q = arg(&a, "queue")?;
                        self.call(Method::POST, &format!("/api/bindings/{}/e/{}/q/{}", enc(&vh), enc(&ex), enc(q)), Some(json!({ "routing_key": a.get("routing_key").cloned().unwrap_or_default() })))
                            .await?;
                        Ok(ActionOutcome::msg(format!("Bound {q} to {ex}")))
                    }
                    _ => {
                        self.call(Method::DELETE, &format!("/api/exchanges/{}/{}", enc(&vh), enc(&ex)), None).await?;
                        Ok(ActionOutcome::msg(format!("Deleted exchange {ex}")).refresh(format!("xs/{}", enc(&vh))))
                    }
                }
            }
            other => Err(Error::unsupported(format!("Unknown action {other}"))),
        }
    }

    fn console_language(&self) -> Option<&'static str> {
        Some("Management API: METHOD /api/path {json body}, e.g. GET /api/nodes")
    }

    async fn console(&self, _node: Option<&str>, input: &str) -> Result<Content> {
        let input = input.trim();
        let (head, body) = match input.find(['{', '[']) {
            Some(i) => (input[..i].trim(), Some(input[i..].trim())),
            None => (input, None),
        };
        let mut it = head.split_whitespace();
        let method = it.next().unwrap_or("GET").to_ascii_uppercase();
        let path = it.next().ok_or_else(|| Error::invalid("Write METHOD /api/path, e.g. GET /api/overview"))?;
        if !path.starts_with("/api/") {
            return Err(Error::invalid("Paths start with /api/"));
        }
        let m = Method::from_bytes(method.as_bytes()).map_err(|_| Error::invalid(format!("Unknown method {method}")))?;
        if self.read_only && m != Method::GET {
            return Err(Error::invalid(format!("{method} is blocked on a read-only connection")));
        }
        let b = match body {
            Some(b) => Some(serde_json::from_str::<J>(b).map_err(|e| Error::invalid(format!("Body is not valid JSON: {e}")))?),
            None => None,
        };
        let v = self.call(m, path, b).await?;
        Ok(match &v {
            J::Array(rows) if rows.iter().all(|r| r.is_object()) && !rows.is_empty() => Content::Table(objects_table(rows, &["name"])),
            J::Null => Content::Empty("OK".into()),
            _ => Content::Json(serde_json::to_string_pretty(&v).unwrap_or_default()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_roundtrip_vhost_slash() {
        assert_eq!(enc("/"), "%2F");
        assert_eq!(split("%2F/orders.q"), Some(("/".into(), "orders.q".into())));
        assert_eq!(percent_decode("a%2Fb%20c"), "a/b c");
    }

    #[tokio::test]
    async fn live_rabbitmq() {
        let mut v = std::collections::BTreeMap::new();
        v.insert("host".into(), "127.0.0.1".into());
        v.insert("port".into(), std::env::var("OPENVCLIENT_RABBIT_PORT").unwrap_or_else(|_| "55673".into()));
        v.insert("user".into(), std::env::var("OPENVCLIENT_RABBIT_USER").unwrap_or_else(|_| "ovc".into()));
        let secret = Some(std::env::var("OPENVCLIENT_RABBIT_PASSWORD").unwrap_or_else(|_| "ovcpass".into()));
        let h = match RabbitDriver::default().connect(ConnectParams { values: v, secret, read_only: false, notifier: None }).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("skipping: no RabbitMQ ({e})");
                return;
            }
        };
        let s = h.resource().unwrap().clone();
        assert!(s.server_info().product.starts_with("RabbitMQ"));
        let top = s.children(None).await.unwrap();
        let vh = top.iter().find(|n| n.kind == "vhost").expect("vhost").id.clone();
        let vhe = vh.strip_prefix("v/").unwrap().to_string();
        let mut a = Args::new();
        a.insert("name".into(), "ovc_test_q".into());
        a.insert("type".into(), "classic".into());
        a.insert("durable".into(), "false".into());
        let _ = s.run_action(&format!("q/{vhe}/ovc_test_q"), "delete_queue", Args::new()).await;
        s.run_action(&vh, "create_queue", a).await.unwrap();
        let qid = format!("q/{vhe}/ovc_test_q");
        let mut p = Args::new();
        p.insert("payload".into(), "hello".into());
        s.run_action(&qid, "publish_queue", p).await.unwrap();
        // The management API counts messages with a short delay.
        let mut got = None;
        for _ in 0..10 {
            let mut g = Args::new();
            g.insert("count".into(), "5".into());
            g.insert("ackmode".into(), "requeue".into());
            let out = s.run_action(&qid, "get", g).await.unwrap();
            if let Some(Content::Table(rs)) = &out.content
                && !rs.rows.is_empty()
            {
                got = Some(rs.rows[0][0].clone());
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        assert_eq!(got, Some(openvclient_core::Value::Text("hello".into())));
        assert!(matches!(s.open(&qid, OpenOptions::default()).await.unwrap(), Content::Props(_)));
        assert!(matches!(s.open("#overview", OpenOptions::default()).await.unwrap(), Content::Props(_)));
        assert!(matches!(s.console(None, "GET /api/nodes").await.unwrap(), Content::Table(_)));
        s.run_action(&qid, "purge", Args::new()).await.unwrap();
        s.run_action(&qid, "delete_queue", Args::new()).await.unwrap();
    }
}
