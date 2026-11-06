//! Connectors that speak HTTP: Elasticsearch/OpenSearch (REST), ClickHouse
//! (HTTP interface) and RabbitMQ (management API).

pub mod clickhouse;
pub mod elastic;
pub mod rabbitmq;


use std::time::Duration;

use openvclient_core::{ConnectParams, Error, Result};

/// A base URL plus credentials, shared by the HTTP connectors.
#[derive(Clone)]
pub(crate) struct Http {
    pub client: reqwest::Client,
    pub base: String,
    pub user: Option<String>,
    pub password: Option<String>,
    pub api_key: Option<String>,
}

impl Http {
    /// `scheme://host:port` from the standard fields; TLS when `tls` is on.
    pub fn from_params(p: &ConnectParams, default_port: u16) -> Result<Self> {
        let tls = p.flag("tls");
        let host = p.get_or("host", "localhost");
        let base = match p.get("url") {
            Some(u) => u.trim_end_matches('/').to_string(),
            None => format!("{}://{host}:{}", if tls { "https" } else { "http" }, p.port(default_port)?),
        };
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .danger_accept_invalid_certs(p.flag("insecure_tls"))
            .user_agent("OpenVClient")
            .build()
            .map_err(|e| Error::other(e.to_string()))?;
        let secret = p.secret.clone().filter(|s| !s.is_empty());
        let api_key = if p.get("auth") == Some("api_key") { secret.clone() } else { None };
        Ok(Self { client, base, user: p.get("user").map(str::to_owned), password: if api_key.is_some() { None } else { secret }, api_key })
    }

    pub fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = if path.starts_with("http") { path.to_string() } else { format!("{}{}{}", self.base, if path.starts_with('/') { "" } else { "/" }, path) };
        let mut r = self.client.request(method, url);
        if let Some(k) = &self.api_key {
            r = r.header("Authorization", format!("ApiKey {k}"));
        } else if let Some(u) = &self.user {
            r = r.basic_auth(u, self.password.clone());
        }
        r
    }

    /// Send and return (status, body); transport problems become connection errors.
    pub async fn send(&self, r: reqwest::RequestBuilder) -> Result<(u16, String)> {
        let resp = r.send().await.map_err(|e| {
            let m = e.to_string();
            if e.is_connect() || e.is_timeout() { Error::connection(format!("{m}: {}", self.base)) } else { Error::other(m) }
        })?;
        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(|e| Error::other(e.to_string()))?;
        if status == 401 || status == 403 {
            return Err(Error::auth(format!("HTTP {status}: {}", first_line(&body))));
        }
        Ok((status, body))
    }

    pub async fn json(&self, r: reqwest::RequestBuilder) -> Result<serde_json::Value> {
        let (status, body) = self.send(r).await?;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::String(body.clone()));
        if status >= 400 {
            let reason = v.pointer("/error/reason").and_then(|x| x.as_str()).map(str::to_owned).or_else(|| v.get("reason").and_then(|x| x.as_str()).map(str::to_owned)).unwrap_or_else(|| first_line(&body));
            return Err(Error::server(format!("HTTP {status}: {reason}")).with_detail(body.chars().take(2000).collect::<String>()));
        }
        Ok(v)
    }
}

pub(crate) fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(300).collect()
}

/// JSON objects → table (union of keys, nested values as JSON text).
pub(crate) fn objects_table(rows: &[serde_json::Value], lead: &[&str]) -> openvclient_core::ResultSet {
    use openvclient_core::{ColumnMeta, ResultSet, Value};
    let mut keys: Vec<String> = lead.iter().map(|s| s.to_string()).collect();
    for r in rows {
        if let Some(o) = r.as_object() {
            for k in o.keys() {
                if !keys.contains(k) {
                    keys.push(k.clone());
                }
            }
        }
    }
    let mut rs = ResultSet::new(keys.iter().map(|k| ColumnMeta::new(k.clone(), "")).collect());
    rs.rows = rows
        .iter()
        .map(|r| {
            keys.iter()
                .map(|k| match r.get(k) {
                    None | Some(serde_json::Value::Null) => Value::Null,
                    Some(serde_json::Value::Bool(b)) => Value::Bool(*b),
                    Some(serde_json::Value::Number(n)) => n.as_i64().map(Value::Int).unwrap_or_else(|| Value::Float(n.as_f64().unwrap_or(f64::NAN))),
                    Some(serde_json::Value::String(s)) => Value::Text(s.clone()),
                    Some(other) => Value::Text(other.to_string()),
                })
                .collect()
        })
        .collect();
    for (i, c) in rs.columns.iter_mut().enumerate() {
        c.numeric = rs.rows.iter().any(|r| r[i].is_numeric()) && rs.rows.iter().all(|r| r[i].is_numeric() || r[i].is_null());
    }
    rs
}
