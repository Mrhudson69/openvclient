//! Redis connector (also Valkey/KeyDB, anything speaking RESP). One managed,
//! auto-reconnecting connection per logical database.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use openvclient_core::kv::*;
use openvclient_core::{Category, ConnectParams, Driver, DriverInfo, Error, FieldKind, FieldSpec, Result, ServerInfo, ServiceSession, SessionHandle};
use redis::aio::ConnectionManager;
use redis::{Cmd, Value as R};
use tokio::sync::Mutex;

pub struct RedisDriver {
    info: DriverInfo,
}

impl Default for RedisDriver {
    fn default() -> Self {
        Self {
            info: DriverInfo {
                id: "redis",
                name: "Redis",
                category: Category::KeyValue,
                description: "Redis, Valkey, KeyDB and other RESP servers",
                badge: "RD",
                fields: vec![
                    FieldSpec::text("host", "Host").default("localhost").required(),
                    FieldSpec::text("port", "Port").kind(FieldKind::Number).default("6379"),
                    FieldSpec::text("user", "Username").placeholder("default").help("Redis 6+ ACL user; leave empty for legacy AUTH."),
                    FieldSpec::text("password", "Password").kind(FieldKind::Secret),
                    FieldSpec::text("db", "Database index").kind(FieldKind::Number).default("0"),
                    FieldSpec::text("tls", "TLS").kind(FieldKind::Bool).default("false").advanced(),
                ],
            },
        }
    }
}

fn r_err(e: redis::RedisError) -> Error {
    let msg = e.to_string();
    if e.kind() == redis::ErrorKind::AuthenticationFailed || msg.contains("NOAUTH") || msg.contains("WRONGPASS") {
        Error::auth(msg)
    } else if e.is_connection_dropped() || e.is_io_error() || e.is_timeout() || e.is_connection_refusal() {
        Error::connection(msg)
    } else {
        Error::server(msg)
    }
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

#[derive(Clone)]
struct Target {
    host: String,
    port: u16,
    user: Option<String>,
    password: Option<String>,
    tls: bool,
}

impl Target {
    fn url(&self, db: u32) -> String {
        let scheme = if self.tls { "rediss" } else { "redis" };
        let auth = match (&self.user, &self.password) {
            (Some(u), Some(p)) => format!("{}:{}@", enc(u), enc(p)),
            (None, Some(p)) => format!(":{}@", enc(p)),
            (Some(u), None) => format!("{}@", enc(u)),
            (None, None) => String::new(),
        };
        format!("{scheme}://{auth}{}:{}/{db}", self.host, self.port)
    }
}

#[async_trait]
impl Driver for RedisDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }

    async fn connect(&self, p: ConnectParams) -> Result<SessionHandle> {
        let target = Target {
            host: p.get_or("host", "localhost").to_string(),
            port: p.port(6379)?,
            user: p.get("user").map(str::to_owned),
            password: p.secret.clone().filter(|s| !s.is_empty()),
            tls: p.flag("tls"),
        };
        let default_db: u32 = p.get_or("db", "0").parse().map_err(|_| Error::invalid("Database index must be a number"))?;
        let s = RedisSession { target, conns: Mutex::new(HashMap::new()), info: parking(), default_db, read_only: p.read_only };
        let info = s.info_map().await?;
        let g = |k: &str| info.iter().find(|(_, key, _)| key == k).map(|x| x.2.clone()).unwrap_or_default();
        let product = if !g("valkey_version").is_empty() { format!("Valkey {}", g("valkey_version")) } else { format!("Redis {}", g("redis_version")) };
        *s.info.lock().await = ServerInfo {
            product,
            version: g("redis_version"),
            details: vec![("Mode".into(), g("redis_mode")), ("Role".into(), g("role")), ("OS".into(), g("os")), ("Memory used".into(), g("used_memory_human"))],
        };
        Ok(SessionHandle::KeyValue(Arc::new(s)))
    }
}

fn parking() -> Mutex<ServerInfo> {
    Mutex::new(ServerInfo::default())
}

struct RedisSession {
    target: Target,
    conns: Mutex<HashMap<u32, ConnectionManager>>,
    info: Mutex<ServerInfo>,
    default_db: u32,
    read_only: bool,
}

/// Commands that never modify data, allowed on read-only connections.
fn is_read_command(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "GET"
            | "MGET"
            | "GETRANGE"
            | "STRLEN"
            | "EXISTS"
            | "TYPE"
            | "TTL"
            | "PTTL"
            | "KEYS"
            | "SCAN"
            | "HGET"
            | "HGETALL"
            | "HMGET"
            | "HKEYS"
            | "HVALS"
            | "HLEN"
            | "HSCAN"
            | "HEXISTS"
            | "LRANGE"
            | "LLEN"
            | "LINDEX"
            | "SMEMBERS"
            | "SCARD"
            | "SISMEMBER"
            | "SSCAN"
            | "ZRANGE"
            | "ZCARD"
            | "ZSCORE"
            | "ZRANK"
            | "ZSCAN"
            | "ZRANGEBYSCORE"
            | "XRANGE"
            | "XREVRANGE"
            | "XLEN"
            | "XINFO"
            | "INFO"
            | "PING"
            | "DBSIZE"
            | "MEMORY"
            | "OBJECT"
            | "CLIENT"
            | "SLOWLOG"
            | "CONFIG"
            | "TIME"
            | "ECHO"
            | "DUMP"
            | "RANDOMKEY"
            | "COMMAND"
            | "LATENCY"
            | "SCRIPT"
    )
}

impl RedisSession {
    async fn con(&self, db: u32) -> Result<ConnectionManager> {
        if let Some(c) = self.conns.lock().await.get(&db) {
            return Ok(c.clone());
        }
        let client = redis::Client::open(self.target.url(db)).map_err(r_err)?;
        let cfg = redis::aio::ConnectionManagerConfig::new().set_connection_timeout(Some(Duration::from_secs(10))).set_response_timeout(Some(Duration::from_secs(60)));
        let mgr = tokio::time::timeout(Duration::from_secs(12), client.get_connection_manager_with_config(cfg))
            .await
            .map_err(|_| Error::connection(format!("Timed out connecting to {}:{}", self.target.host, self.target.port)))?
            .map_err(r_err)?;
        self.conns.lock().await.insert(db, mgr.clone());
        Ok(mgr)
    }

    async fn q<T: redis::FromRedisValue>(&self, db: u32, cmd: &Cmd) -> Result<T> {
        let mut c = self.con(db).await?;
        cmd.query_async(&mut c).await.map_err(r_err)
    }

    fn guard(&self) -> Result<()> {
        if self.read_only { Err(Error::invalid("Connection is read-only")) } else { Ok(()) }
    }

    async fn info_map(&self) -> Result<Vec<(String, String, String)>> {
        let text: String = self.q(self.default_db, &redis::cmd("INFO")).await?;
        let mut section = String::new();
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(s) = line.strip_prefix("# ") {
                section = s.to_string();
            } else if let Some((k, v)) = line.split_once(':') {
                out.push((section.clone(), k.to_string(), v.to_string()));
            }
        }
        Ok(out)
    }
}

fn s(v: &R) -> String {
    match v {
        R::BulkString(b) => String::from_utf8_lossy(b).into_owned(),
        R::SimpleString(s) => s.clone(),
        R::Int(i) => i.to_string(),
        R::Double(d) => d.to_string(),
        R::VerbatimString { text, .. } => text.clone(),
        R::Okay => "OK".into(),
        R::Nil => String::new(),
        other => format!("{other:?}"),
    }
}

/// Render a reply like redis-cli does.
pub fn render(v: &R, indent: usize) -> String {
    let pad = " ".repeat(indent);
    match v {
        R::Nil => "(nil)".into(),
        R::Int(i) => format!("(integer) {i}"),
        R::Double(d) => format!("(double) {d}"),
        R::Boolean(b) => format!("(boolean) {b}"),
        R::Okay => "OK".into(),
        R::SimpleString(s) => s.clone(),
        R::BulkString(b) => match std::str::from_utf8(b) {
            Ok(t) => format!("{t:?}"),
            Err(_) => format!("(binary) {}", openvclient_core::hex_full(b)),
        },
        R::VerbatimString { text, .. } => text.clone(),
        R::BigNumber(n) => format!("(big number) {}", String::from_utf8_lossy(n)),
        R::Array(items) | R::Set(items) => {
            if items.is_empty() {
                return "(empty array)".into();
            }
            items.iter().enumerate().map(|(i, x)| format!("{}{}) {}", if i == 0 { String::new() } else { pad.clone() }, i + 1, render(x, indent + 3))).collect::<Vec<_>>().join("\n")
        }
        R::Map(pairs) => {
            if pairs.is_empty() {
                return "(empty map)".into();
            }
            pairs
                .iter()
                .enumerate()
                .map(|(i, (k, x))| format!("{}{}# {} => {}", if i == 0 { String::new() } else { pad.clone() }, i + 1, render(k, 0), render(x, indent + 3)))
                .collect::<Vec<_>>()
                .join("\n")
        }
        R::ServerError(e) => format!("(error) {e:?}"),
        other => format!("{other:?}"),
    }
}

#[async_trait]
impl ServiceSession for RedisSession {
    fn server_info(&self) -> ServerInfo {
        self.info.try_lock().map(|i| i.clone()).unwrap_or_default()
    }

    async fn ping(&self) -> Result<Duration> {
        let t = Instant::now();
        let _: String = self.q(self.default_db, &redis::cmd("PING")).await?;
        Ok(t.elapsed())
    }

    async fn close(&self) {
        self.conns.lock().await.clear();
    }
}

#[async_trait]
impl KeyValueSession for RedisSession {
    async fn databases(&self) -> Result<Vec<KvDatabase>> {
        // CONFIG may be disabled (managed services); fall back to the keyspace.
        let count: u32 = match self.q::<Vec<String>>(self.default_db, redis::cmd("CONFIG").arg("GET").arg("databases")).await {
            Ok(v) => v.get(1).and_then(|n| n.parse().ok()).unwrap_or(16),
            Err(_) => 16,
        };
        let info = self.info_map().await?;
        let mut dbs: Vec<KvDatabase> = (0..count).map(|index| KvDatabase { index, keys: 0, expires: 0 }).collect();
        for (sec, k, v) in info {
            if sec != "Keyspace" {
                continue;
            }
            let Some(idx) = k.strip_prefix("db").and_then(|n| n.parse::<u32>().ok()) else { continue };
            let field = |name: &str| v.split(',').find_map(|p| p.strip_prefix(&format!("{name}="))).and_then(|x| x.parse().ok()).unwrap_or(0);
            if let Some(d) = dbs.iter_mut().find(|d| d.index == idx) {
                d.keys = field("keys");
                d.expires = field("expires");
            } else {
                dbs.push(KvDatabase { index: idx, keys: field("keys"), expires: field("expires") });
            }
        }
        Ok(dbs)
    }

    async fn scan(&self, db: u32, cursor: u64, pattern: &str, count: u32, type_filter: Option<&str>) -> Result<ScanPage> {
        let mut cmd = redis::cmd("SCAN");
        cmd.arg(cursor).arg("MATCH").arg(if pattern.is_empty() { "*" } else { pattern }).arg("COUNT").arg(count);
        if let Some(t) = type_filter {
            cmd.arg("TYPE").arg(t);
        }
        let (next, keys): (u64, Vec<Vec<u8>>) = self.q(db, &cmd).await?;
        // Fetch type and TTL for the page in one round trip.
        let mut pipe = redis::pipe();
        for k in &keys {
            pipe.cmd("TYPE").arg(k).cmd("TTL").arg(k);
        }
        let mut c = self.con(db).await?;
        let meta: Vec<R> = if keys.is_empty() { Vec::new() } else { pipe.query_async(&mut c).await.map_err(r_err)? };
        let entries = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let ttl = match meta.get(i * 2 + 1) {
                    Some(R::Int(t)) if *t >= 0 => Some(*t),
                    _ => None,
                };
                KeyEntry { key: String::from_utf8_lossy(k).into_owned(), key_type: meta.get(i * 2).map(s).unwrap_or_default(), ttl }
            })
            .collect();
        Ok(ScanPage { cursor: next, keys: entries })
    }

    async fn key_detail(&self, db: u32, key: &str, limit: usize) -> Result<KeyDetail> {
        let key_type: String = self.q(db, redis::cmd("TYPE").arg(key)).await?;
        if key_type == "none" {
            return Err(Error::invalid(format!("Key {key:?} does not exist (expired or deleted)")));
        }
        let ttl: i64 = self.q(db, redis::cmd("TTL").arg(key)).await?;
        let memory: Option<u64> = self.q(db, redis::cmd("MEMORY").arg("USAGE").arg(key)).await.ok();
        let encoding: Option<String> = self.q(db, redis::cmd("OBJECT").arg("ENCODING").arg(key)).await.ok();
        let lim = limit.max(1) as i64;
        let (value, length, truncated) = match key_type.as_str() {
            "string" => {
                let v: Vec<u8> = self.q(db, redis::cmd("GET").arg(key)).await?;
                let n = v.len() as u64;
                (KvValue::String(v), n, false)
            }
            "hash" => {
                let n: u64 = self.q(db, redis::cmd("HLEN").arg(key)).await?;
                let mut items = Vec::new();
                let mut cursor = 0u64;
                loop {
                    let (next, kv): (u64, Vec<String>) = self.q(db, redis::cmd("HSCAN").arg(key).arg(cursor).arg("COUNT").arg(500)).await?;
                    items.extend(kv.chunks(2).map(|c| (c[0].clone(), c.get(1).cloned().unwrap_or_default())));
                    cursor = next;
                    if cursor == 0 || items.len() >= limit {
                        break;
                    }
                }
                items.truncate(limit);
                (KvValue::Hash(items), n, n as usize > limit)
            }
            "list" => {
                let n: u64 = self.q(db, redis::cmd("LLEN").arg(key)).await?;
                let v: Vec<String> = self.q(db, redis::cmd("LRANGE").arg(key).arg(0).arg(lim - 1)).await?;
                (KvValue::List(v), n, n as usize > limit)
            }
            "set" => {
                let n: u64 = self.q(db, redis::cmd("SCARD").arg(key)).await?;
                let mut items = Vec::new();
                let mut cursor = 0u64;
                loop {
                    let (next, m): (u64, Vec<String>) = self.q(db, redis::cmd("SSCAN").arg(key).arg(cursor).arg("COUNT").arg(500)).await?;
                    items.extend(m);
                    cursor = next;
                    if cursor == 0 || items.len() >= limit {
                        break;
                    }
                }
                items.truncate(limit);
                items.sort();
                (KvValue::Set(items), n, n as usize > limit)
            }
            "zset" => {
                let n: u64 = self.q(db, redis::cmd("ZCARD").arg(key)).await?;
                let v: Vec<(String, f64)> = self.q(db, redis::cmd("ZRANGE").arg(key).arg(0).arg(lim - 1).arg("WITHSCORES")).await?;
                (KvValue::ZSet(v), n, n as usize > limit)
            }
            "stream" => {
                let n: u64 = self.q(db, redis::cmd("XLEN").arg(key)).await?;
                let raw: Vec<(String, Vec<String>)> = self.q(db, redis::cmd("XREVRANGE").arg(key).arg("+").arg("-").arg("COUNT").arg(lim)).await?;
                let entries = raw.into_iter().map(|(id, f)| (id, f.chunks(2).map(|c| (c[0].clone(), c.get(1).cloned().unwrap_or_default())).collect())).collect();
                (KvValue::Stream(entries), n, n as usize > limit)
            }
            other => {
                // Module types (ReJSON, …): show what the server renders, read-only.
                let rendered = if other.starts_with("ReJSON") {
                    self.q::<String>(db, redis::cmd("JSON.GET").arg(key)).await.unwrap_or_else(|e| e.to_string())
                } else {
                    format!("Values of type {other} are not viewable here")
                };
                (KvValue::Other { type_name: other.into(), rendered }, 0, false)
            }
        };
        Ok(KeyDetail { key: key.into(), key_type, ttl: if ttl >= 0 { Some(ttl) } else { None }, length, memory, encoding, value, truncated })
    }

    async fn edit(&self, db: u32, key: &str, edit: KvEdit) -> Result<()> {
        self.guard()?;
        let mut cmd = Cmd::new();
        match edit {
            KvEdit::SetString { value } => cmd.arg("SET").arg(key).arg(value).arg("KEEPTTL"),
            KvEdit::HashSet { field, value } => cmd.arg("HSET").arg(key).arg(field).arg(value),
            KvEdit::HashDelete { field } => cmd.arg("HDEL").arg(key).arg(field),
            KvEdit::ListPush { value, head } => cmd.arg(if head { "LPUSH" } else { "RPUSH" }).arg(key).arg(value),
            KvEdit::ListSet { index, value } => cmd.arg("LSET").arg(key).arg(index).arg(value),
            KvEdit::ListRemove { value } => cmd.arg("LREM").arg(key).arg(1).arg(value),
            KvEdit::SetAdd { member } => cmd.arg("SADD").arg(key).arg(member),
            KvEdit::SetRemove { member } => cmd.arg("SREM").arg(key).arg(member),
            KvEdit::ZAdd { member, score } => cmd.arg("ZADD").arg(key).arg(score).arg(member),
            KvEdit::ZRemove { member } => cmd.arg("ZREM").arg(key).arg(member),
            KvEdit::StreamAdd { fields } => {
                cmd.arg("XADD").arg(key).arg("*");
                for (f, v) in fields {
                    cmd.arg(f).arg(v);
                }
                &mut cmd
            }
            KvEdit::StreamDelete { id } => cmd.arg("XDEL").arg(key).arg(id),
        };
        let _: R = self.q(db, &cmd).await?;
        Ok(())
    }

    async fn create_key(&self, db: u32, key: &str, key_type: &str, first: Vec<String>, ttl: Option<i64>) -> Result<()> {
        self.guard()?;
        let exists: i64 = self.q(db, redis::cmd("EXISTS").arg(key)).await?;
        if exists > 0 {
            return Err(Error::invalid(format!("Key {key:?} already exists")));
        }
        let mut cmd = Cmd::new();
        let need = |n: usize| if first.len() < n { Err(Error::invalid("Fill in the first value")) } else { Ok(()) };
        match key_type {
            "string" => {
                need(1)?;
                cmd.arg("SET").arg(key).arg(&first[0]);
            }
            "hash" => {
                need(2)?;
                cmd.arg("HSET").arg(key).arg(&first[0]).arg(&first[1]);
            }
            "list" => {
                need(1)?;
                cmd.arg("RPUSH").arg(key).arg(&first[0]);
            }
            "set" => {
                need(1)?;
                cmd.arg("SADD").arg(key).arg(&first[0]);
            }
            "zset" => {
                need(2)?;
                let score: f64 = first[1].parse().map_err(|_| Error::invalid("Score must be a number"))?;
                cmd.arg("ZADD").arg(key).arg(score).arg(&first[0]);
            }
            "stream" => {
                need(2)?;
                cmd.arg("XADD").arg(key).arg("*").arg(&first[0]).arg(&first[1]);
            }
            t => return Err(Error::unsupported(format!("Cannot create keys of type {t}"))),
        }
        let _: R = self.q(db, &cmd).await?;
        if let Some(t) = ttl.filter(|t| *t > 0) {
            let _: R = self.q(db, redis::cmd("EXPIRE").arg(key).arg(t)).await?;
        }
        Ok(())
    }

    async fn delete_keys(&self, db: u32, keys: Vec<String>) -> Result<u64> {
        self.guard()?;
        if keys.is_empty() {
            return Ok(0);
        }
        // UNLINK frees memory asynchronously on the server.
        self.q(db, redis::cmd("UNLINK").arg(keys)).await
    }

    async fn rename(&self, db: u32, from: &str, to: &str) -> Result<()> {
        self.guard()?;
        let ok: i64 = self.q(db, redis::cmd("RENAMENX").arg(from).arg(to)).await?;
        if ok == 0 { Err(Error::invalid(format!("Key {to:?} already exists"))) } else { Ok(()) }
    }

    async fn set_ttl(&self, db: u32, key: &str, ttl: Option<i64>) -> Result<()> {
        self.guard()?;
        let r: i64 = match ttl {
            Some(t) => self.q(db, redis::cmd("EXPIRE").arg(key).arg(t)).await?,
            None => {
                let _: i64 = self.q(db, redis::cmd("PERSIST").arg(key)).await?;
                1
            }
        };
        if r == 0 { Err(Error::invalid("Key does not exist")) } else { Ok(()) }
    }

    async fn command(&self, db: u32, line: &str) -> Result<String> {
        let args = split_command_line(line)?;
        let Some(name) = args.first() else { return Ok(String::new()) };
        let up = name.to_ascii_uppercase();
        if matches!(up.as_str(), "SELECT" | "SUBSCRIBE" | "PSUBSCRIBE" | "MONITOR" | "SYNC" | "PSYNC" | "QUIT") {
            return Err(Error::unsupported(format!("{up} would change the shared connection; use the database selector instead")));
        }
        if self.read_only && !is_read_command(name) {
            return Err(Error::invalid(format!("{up} is blocked on a read-only connection")));
        }
        let mut cmd = Cmd::new();
        for a in &args {
            cmd.arg(a);
        }
        let v: R = self.q(db, &cmd).await?;
        Ok(render(&v, 0))
    }

    async fn info(&self) -> Result<Vec<(String, String, String)>> {
        self.info_map().await
    }

    async fn clients(&self) -> Result<Vec<Vec<(String, String)>>> {
        let text: String = self.q(self.default_db, redis::cmd("CLIENT").arg("LIST")).await?;
        Ok(text.lines().map(|l| l.split(' ').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect()).collect())
    }

    async fn kill_client(&self, id: &str) -> Result<()> {
        self.guard()?;
        let _: R = self.q(self.default_db, redis::cmd("CLIENT").arg("KILL").arg("ID").arg(id)).await?;
        Ok(())
    }

    async fn slowlog(&self, count: u32) -> Result<Vec<(i64, i64, u64, String)>> {
        let v: R = self.q(self.default_db, redis::cmd("SLOWLOG").arg("GET").arg(count)).await?;
        let R::Array(entries) = v else { return Ok(Vec::new()) };
        Ok(entries
            .into_iter()
            .filter_map(|e| {
                let R::Array(f) = e else { return None };
                let int = |i: usize| if let Some(R::Int(n)) = f.get(i) { *n } else { 0 };
                let args = match f.get(3) {
                    Some(R::Array(a)) => a.iter().map(s).collect::<Vec<_>>().join(" "),
                    _ => String::new(),
                };
                Some((int(0), int(1), int(2) as u64, args))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn live_redis() {
        let mut v = std::collections::BTreeMap::new();
        v.insert("host".to_string(), "127.0.0.1".to_string());
        v.insert("port".to_string(), std::env::var("OPENVCLIENT_REDIS_PORT").unwrap_or_else(|_| "56379".into()));
        let Ok(h) = RedisDriver::default().connect(ConnectParams { values: v, secret: None, read_only: false, notifier: None }).await else {
            eprintln!("skipping: no Redis reachable");
            return;
        };
        let r = h.kv().unwrap().clone();
        assert!(r.server_info().product.starts_with("Redis") || r.server_info().product.starts_with("Valkey"));
        let db = 7;
        let _ = r.command(db, "FLUSHDB").await;
        r.create_key(db, "s", "string", vec!["hello world".into()], Some(100)).await.unwrap();
        r.create_key(db, "h", "hash", vec!["f".into(), "v".into()], None).await.unwrap();
        r.create_key(db, "z", "zset", vec!["m".into(), "2.5".into()], None).await.unwrap();
        r.create_key(db, "x", "stream", vec!["a".into(), "1".into()], None).await.unwrap();
        r.edit(db, "h", KvEdit::HashSet { field: "g".into(), value: "w".into() }).await.unwrap();
        r.command(db, "RPUSH l a b \"c d\"").await.unwrap();
        assert!(r.create_key(db, "s", "string", vec!["x".into()], None).await.is_err());

        let page = r.scan(db, 0, "*", 100, None).await.unwrap();
        let mut keys: Vec<_> = page.keys.iter().map(|k| (k.key.clone(), k.key_type.clone())).collect();
        keys.sort();
        assert_eq!(keys.len(), 5, "{keys:?}");
        assert!(page.keys.iter().find(|k| k.key == "s").unwrap().ttl.is_some());
        assert_eq!(r.scan(db, 0, "*", 100, Some("hash")).await.unwrap().keys.len(), 1);

        let d = r.key_detail(db, "h", 100).await.unwrap();
        assert!(matches!(&d.value, KvValue::Hash(v) if v.len() == 2));
        let d = r.key_detail(db, "l", 2).await.unwrap();
        assert!(matches!(&d.value, KvValue::List(v) if v == &vec!["a".to_string(), "b".to_string()]) && d.truncated && d.length == 3);
        let d = r.key_detail(db, "x", 10).await.unwrap();
        assert!(matches!(&d.value, KvValue::Stream(v) if v.len() == 1));
        r.edit(db, "s", KvEdit::SetString { value: "new".into() }).await.unwrap();
        let d = r.key_detail(db, "s", 10).await.unwrap();
        assert!(d.ttl.is_some(), "SET KEEPTTL must keep the expiry");
        r.set_ttl(db, "s", None).await.unwrap();
        assert!(r.key_detail(db, "s", 10).await.unwrap().ttl.is_none());
        r.rename(db, "s", "s2").await.unwrap();
        assert_eq!(r.delete_keys(db, vec!["s2".into(), "h".into()]).await.unwrap(), 2);
        assert!(r.command(db, "SELECT 1").await.is_err());
        assert_eq!(r.command(db, "DBSIZE").await.unwrap(), "(integer) 3");
        assert!(r.databases().await.unwrap().iter().any(|d| d.index == 7 && d.keys == 3));
        assert!(!r.info().await.unwrap().is_empty());
        assert!(!r.clients().await.unwrap().is_empty());
        r.slowlog(10).await.unwrap();
        r.command(db, "FLUSHDB").await.unwrap();
    }
}
