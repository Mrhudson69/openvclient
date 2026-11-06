//! Live sessions per saved profile, with health pings and reconnects.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use openvclient_core::{ConnectParams, ConnectionProfile, DriverRegistry, Error, Result, SessionHandle};
use uuid::Uuid;

use crate::bg::{Bg, Job, poll};
use crate::store::secrets;

const PING_EVERY: Duration = Duration::from_secs(30);

pub enum ConnState {
    Connecting { job: Job<Result<(SessionHandle, Option<openvclient_net::Tunnel>)>>, started: Instant },
    Connected(Live),
    Failed(Error),
}

pub struct Live {
    pub handle: SessionHandle,
    /// Keeps the SSH/proxy forwarder alive for the session's lifetime.
    pub tunnel: Option<std::sync::Arc<openvclient_net::Tunnel>>,
    pub latency: Option<Duration>,
    pub last_ping: Instant,
    ping: Option<Job<Result<Duration>>>,
    /// Ping failed once; a second failure marks the session lost.
    pub degraded: bool,
}

pub enum ConnEvent {
    /// A driver needs the user to do something (e.g. enter a sign-in code).
    Notice(Uuid, String),
    /// Connected; carries the SSH host key fingerprint seen, for pinning.
    Connected(Uuid, Option<String>),
    Failed(Uuid, Error),
    Lost(Uuid, Error),
}

pub struct Connections {
    pub registry: DriverRegistry,
    states: HashMap<Uuid, ConnState>,
    /// Passwords typed for profiles that do not save secrets; memory only.
    session_secrets: HashMap<Uuid, String>,
    notices: std::sync::Arc<parking_lot::Mutex<Vec<(Uuid, String)>>>,
}

impl Connections {
    pub fn new(registry: DriverRegistry) -> Self {
        Self { registry, states: HashMap::new(), session_secrets: HashMap::new(), notices: Default::default() }
    }

    pub fn state(&self, id: Uuid) -> Option<&ConnState> {
        self.states.get(&id)
    }

    pub fn session(&self, id: Uuid) -> Option<&SessionHandle> {
        match self.states.get(&id) {
            Some(ConnState::Connected(l)) => Some(&l.handle),
            _ => None,
        }
    }

    pub fn is_connected(&self, id: Uuid) -> bool {
        self.session(id).is_some()
    }

    pub fn is_connecting(&self, id: Uuid) -> bool {
        matches!(self.states.get(&id), Some(ConnState::Connecting { .. }))
    }

    pub fn any_connecting(&self) -> bool {
        self.states.values().any(|s| matches!(s, ConnState::Connecting { .. }))
    }

    pub fn connected_ids(&self) -> Vec<Uuid> {
        self.states.iter().filter(|(_, s)| matches!(s, ConnState::Connected(_))).map(|(k, _)| *k).collect()
    }

    /// Does this profile need a password typed before connecting?
    pub fn needs_prompt(&self, p: &ConnectionProfile) -> bool {
        let has_secret_field = self
            .registry
            .get(&p.driver)
            .is_some_and(|d| d.info().fields.iter().any(|f| f.kind == openvclient_core::FieldKind::Secret));
        has_secret_field && !p.save_secret && !self.session_secrets.contains_key(&p.id)
    }

    pub fn remember_secret(&mut self, id: Uuid, secret: String) {
        self.session_secrets.insert(id, secret);
    }

    pub fn connect(&mut self, bg: &Bg, p: &ConnectionProfile) {
        if self.is_connecting(p.id) {
            return;
        }
        let Some(driver) = self.registry.get(&p.driver) else {
            self.states.insert(p.id, ConnState::Failed(Error::unsupported(format!("No connector for '{}' in this build", p.driver))));
            return;
        };
        let profile = p.clone();
        let typed = self.session_secrets.get(&p.id).cloned();
        let sink = self.notices.clone();
        let pid = p.id;
        let notifier = openvclient_core::Notifier(std::sync::Arc::new(move |m: String| sink.lock().push((pid, m))));
        let job = bg.spawn(format!("Connecting to {}", p.name), async move {
            let secret = if profile.save_secret {
                let id = profile.id;
                // Keychain access may block (e.g. a macOS permission prompt).
                tokio::task::spawn_blocking(move || secrets::get(id)).await.map_err(Error::from)?.map_err(Error::other)?
            } else {
                typed
            };
            let id = profile.id;
            let net = tokio::task::spawn_blocking(move || net_secrets(id)).await.map_err(Error::from)?;
            let (name, driver_id, endpoint, mode) = (profile.name.clone(), profile.driver.clone(), openvclient_core::redact(&profile.summary()), profile.param(openvclient_net::keys::MODE).to_string());
            let network = if mode.is_empty() { "direct".to_string() } else { mode };
            tracing::info!(target: "openvclient::conn", %name, driver = %driver_id, %endpoint, %network, "connecting");
            let t0 = Instant::now();
            let r = open_session(driver, profile, secret, net, Some(notifier)).await;
            match &r {
                Ok((h, t)) => tracing::info!(target: "openvclient::conn", %name, product = %h.base().server_info().product, via = %t.as_ref().map(|t| t.description.as_str()).unwrap_or("direct"), elapsed_ms = t0.elapsed().as_millis() as u64, "connected"),
                Err(e) => tracing::warn!(target: "openvclient::conn", %name, %endpoint, kind = ?e.kind, error = %openvclient_core::redact(&e.message), detail = %openvclient_core::redact(e.detail.as_deref().unwrap_or("")), hint = %e.hint().unwrap_or(""), "connect failed"),
            }
            r
        });
        self.states.insert(p.id, ConnState::Connecting { job, started: Instant::now() });
    }

    pub fn cancel_connect(&mut self, id: Uuid) {
        if let Some(ConnState::Connecting { job, .. }) = self.states.get(&id) {
            job.abort();
            self.states.remove(&id);
        }
    }

    pub fn disconnect(&mut self, bg: &Bg, id: Uuid) {
        if let Some(ConnState::Connected(l)) = self.states.remove(&id) {
            let h = l.handle.clone();
            let _ = bg.spawn("", async move { h.base().close().await });
        } else {
            self.states.remove(&id);
        }
    }

    pub fn forget(&mut self, id: Uuid) {
        self.session_secrets.remove(&id);
    }

    /// Drive connection jobs and health pings; call once per frame.
    pub fn poll(&mut self, bg: &Bg) -> Vec<ConnEvent> {
        let mut events: Vec<ConnEvent> = self.notices.lock().drain(..).map(|(id, m)| ConnEvent::Notice(id, m)).collect();
        let now = Instant::now();
        for (id, st) in self.states.iter_mut() {
            match st {
                ConnState::Connecting { job, .. } => match job.take() {
                    Some(Ok((handle, tunnel))) => {
                        // Tunnel host key, or the SSH connector's own server key.
                        let fp = tunnel.as_ref().and_then(|t| t.fingerprint.clone()).or_else(|| {
                            handle.base().server_info().details.iter().find(|(k, _)| k == "Host key (SHA256)").map(|(_, v)| v.clone()).filter(|v| !v.is_empty())
                        });
                        *st = ConnState::Connected(Live { handle, tunnel: tunnel.map(std::sync::Arc::new), latency: None, last_ping: now - PING_EVERY, ping: None, degraded: false });
                        events.push(ConnEvent::Connected(*id, fp));
                    }
                    Some(Err(e)) => {
                        events.push(ConnEvent::Failed(*id, e.clone()));
                        *st = ConnState::Failed(e);
                    }
                    None => {
                        if job.is_dead() {
                            *st = ConnState::Failed(Error::cancelled());
                        }
                    }
                },
                ConnState::Connected(l) => {
                    match poll(&mut l.ping) {
                        Some(Ok(d)) => {
                            l.latency = Some(d);
                            l.degraded = false;
                        }
                        Some(Err(e)) => {
                            if l.degraded || e.is_connection() {
                                tracing::warn!(target: "openvclient::conn", id = %id, error = %e.message, "connection lost");
                                events.push(ConnEvent::Lost(*id, e.clone()));
                                *st = ConnState::Failed(Error::connection(format!("Connection lost: {}", e.message)));
                                continue;
                            }
                            l.degraded = true;
                        }
                        None => {}
                    }
                    if l.ping.is_none() && now.duration_since(l.last_ping) >= PING_EVERY {
                        l.last_ping = now;
                        let h = l.handle.clone();
                        l.ping = Some(bg.spawn("", async move {
                            match tokio::time::timeout(Duration::from_secs(15), h.base().ping()).await {
                                Ok(r) => r,
                                Err(_) => Err(Error::connection("Health check timed out")),
                            }
                        }));
                    }
                }
                ConnState::Failed(_) => {}
            }
        }
        if self.states.values().any(|s| matches!(s, ConnState::Connected(_))) {
            bg.ctx.request_repaint_after(PING_EVERY);
        }
        events
    }
}

/// Driver's default port, used when the profile leaves it empty.
pub fn default_port(driver: &str) -> u16 {
    match driver {
        "postgres" | "kingbase" | "gaussdb" => 5432,
        "mysql" | "mariadb" => 3306,
        "sqlserver" => 1433,
        "redis" => 6379,
        "mongodb" => 27017,
        "clickhouse" => 8123,
        "elasticsearch" => 9200,
        "cassandra" => 9042,
        "oracle" => 1521,
        "db2" => 50000,
        "kafka" => 9092,
        "rabbitmq" => 15672,
        "ssh" => 22,
        "ftp" => 21,
        _ => 0,
    }
}

/// Keychain secrets for the profile's network path.
pub fn net_secrets(id: Uuid) -> openvclient_net::NetSecrets {
    use openvclient_net::keys::*;
    let g = |n| secrets::get_named(id, n).ok().flatten();
    openvclient_net::NetSecrets { ssh_password: g(SECRET_SSH_PASSWORD), ssh_passphrase: g(SECRET_SSH_PASSPHRASE), proxy_password: g(SECRET_PROXY_PASSWORD) }
}

/// Open the network path (if any) and the driver session through it.
pub async fn open_session(
    driver: std::sync::Arc<dyn openvclient_core::Driver>,
    profile: ConnectionProfile,
    secret: Option<String>,
    net: openvclient_net::NetSecrets,
    notifier: Option<openvclient_core::Notifier>,
) -> Result<(SessionHandle, Option<openvclient_net::Tunnel>)> {
    let mut params = ConnectParams::from_profile(&profile, secret);
    params.notifier = notifier;
    let tunnel = match openvclient_net::ProxySpec::from_params(&params, &net)? {
        None => None,
        Some(spec) => {
            let host = params.get_or("host", "localhost").to_string();
            let port = params.port(default_port(&profile.driver))?;
            let t = openvclient_net::Tunnel::open(spec, &host, port).await?;
            openvclient_net::rewrite_params(&mut params, t.local);
            Some(t)
        }
    };
    match tokio::time::timeout(Duration::from_secs(45), driver.connect(params)).await {
        Ok(r) => r.map(|h| (h, tunnel)),
        Err(_) => Err(Error::connection("Connection attempt timed out after 45 s")),
    }
}
