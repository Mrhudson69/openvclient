//! Network paths shared by every TCP connector.
//!
//! A [`Tunnel`] listens on `127.0.0.1:<random>` and forwards each accepted
//! connection to the real target through an SSH tunnel, a SOCKS5 proxy or an
//! HTTP CONNECT proxy. Drivers just connect to the local port, so any TCP
//! protocol works without driver changes.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use openvclient_core::{ConnectParams, Error, Result};
pub mod entra;

use russh::client;
use russh::keys::{HashAlg, PrivateKeyWithHashAlg};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

#[derive(Clone, PartialEq, Eq)]
pub enum SshAuth {
    Password(String),
    /// Keyboard-interactive (PAM, OTP/2FA): prompts that mention a password
    /// get `password`; any other prompt gets `otp` (a one-time code).
    KeyboardInteractive { password: String, otp: Option<String> },
    Key { path: PathBuf, passphrase: Option<String> },
    /// Private key pasted as text (OpenSSH / PEM / PKCS#8).
    KeyText { pem: String, passphrase: Option<String> },
    /// OpenSSH user certificate plus its private key.
    Certificate { key: PathBuf, cert: PathBuf, passphrase: Option<String> },
    /// ssh-agent (Unix) / Pageant or the OpenSSH agent pipe (Windows).
    Agent,
    /// "none" method (servers that allow login without credentials).
    None,
}

impl std::fmt::Debug for SshAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SshAuth::Password(_) => "Password",
            SshAuth::KeyboardInteractive { .. } => "KeyboardInteractive",
            SshAuth::Key { .. } => "Key",
            SshAuth::KeyText { .. } => "KeyText",
            SshAuth::Certificate { .. } => "Certificate",
            SshAuth::Agent => "Agent",
            SshAuth::None => "None",
        })
    }
}

impl std::fmt::Debug for NetSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NetSecrets { .. }")
    }
}

#[derive(Clone, Debug)]
pub enum ProxySpec {
    Ssh { host: String, port: u16, user: String, auth: SshAuth, known_fingerprint: Option<String>, timeout: Duration },
    Socks5 { host: String, port: u16, auth: Option<(String, String)> },
    Http { host: String, port: u16, auth: Option<(String, String)> },
}

/// Profile parameter keys. Secrets live in the keychain under the
/// connection id with these names as suffixes.
pub mod keys {
    pub const MODE: &str = "net.mode"; // direct | ssh | socks5 | http
    pub const SSH_HOST: &str = "ssh.host";
    pub const SSH_PORT: &str = "ssh.port";
    pub const SSH_USER: &str = "ssh.user";
    pub const SSH_AUTH: &str = "ssh.auth"; // password | key | agent
    pub const SSH_KEY: &str = "ssh.key_path";
    pub const SSH_FINGERPRINT: &str = "ssh.fingerprint";
    pub const SSH_TIMEOUT: &str = "ssh.timeout_ms";
    pub const PROXY_HOST: &str = "proxy.host";
    pub const PROXY_PORT: &str = "proxy.port";
    pub const PROXY_USER: &str = "proxy.user";
    /// Keychain suffixes.
    pub const SECRET_SSH_PASSWORD: &str = "ssh.password";
    pub const SECRET_SSH_PASSPHRASE: &str = "ssh.passphrase";
    pub const SECRET_PROXY_PASSWORD: &str = "proxy.password";
}

/// Secrets the network path needs, resolved by the caller (keychain).
#[derive(Clone, Default)]
pub struct NetSecrets {
    pub ssh_password: Option<String>,
    pub ssh_passphrase: Option<String>,
    pub proxy_password: Option<String>,
}

impl ProxySpec {
    /// Build from profile parameters; `None` means a direct connection.
    pub fn from_params(p: &ConnectParams, s: &NetSecrets) -> Result<Option<Self>> {
        let port = |key: &str, default: u16| -> Result<u16> {
            match p.get(key) {
                None => Ok(default),
                Some(v) => v.trim().parse().map_err(|_| Error::invalid(format!("Invalid port: {v}"))),
            }
        };
        let req = |key: &str, what: &str| -> Result<String> {
            p.get(key).map(str::to_owned).ok_or_else(|| Error::invalid(format!("{what} is required")))
        };
        Ok(match p.get(keys::MODE).unwrap_or("direct") {
            "direct" => None,
            "ssh" => {
                let auth = match p.get(keys::SSH_AUTH).unwrap_or("password") {
                    "key" => SshAuth::Key { path: PathBuf::from(expand(&req(keys::SSH_KEY, "SSH private key")?)), passphrase: s.ssh_passphrase.clone() },
                    "agent" => SshAuth::Agent,
                    _ => SshAuth::Password(s.ssh_password.clone().unwrap_or_default()),
                };
                Some(ProxySpec::Ssh {
                    host: req(keys::SSH_HOST, "SSH host")?,
                    port: port(keys::SSH_PORT, 22)?,
                    user: req(keys::SSH_USER, "SSH user")?,
                    auth,
                    known_fingerprint: p.get(keys::SSH_FINGERPRINT).map(str::to_owned),
                    timeout: Duration::from_millis(p.get(keys::SSH_TIMEOUT).and_then(|v| v.parse().ok()).unwrap_or(10_000)),
                })
            }
            m @ ("socks5" | "http") => {
                let auth = p.get(keys::PROXY_USER).map(|u| (u.to_string(), s.proxy_password.clone().unwrap_or_default()));
                let host = req(keys::PROXY_HOST, "Proxy host")?;
                if m == "socks5" {
                    Some(ProxySpec::Socks5 { host, port: port(keys::PROXY_PORT, 1080)?, auth })
                } else {
                    Some(ProxySpec::Http { host, port: port(keys::PROXY_PORT, 3128)?, auth })
                }
            }
            other => return Err(Error::invalid(format!("Unknown network mode {other}"))),
        })
    }

    pub fn label(&self) -> String {
        match self {
            ProxySpec::Ssh { host, port, user, .. } => format!("SSH {user}@{host}:{port}"),
            ProxySpec::Socks5 { host, port, .. } => format!("SOCKS5 {host}:{port}"),
            ProxySpec::Http { host, port, .. } => format!("HTTP proxy {host}:{port}"),
        }
    }
}

fn expand(p: &str) -> String {
    match (p.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => p.to_string(),
    }
}

/// A running forwarder. Dropping it stops listening and closes the path.
pub struct Tunnel {
    pub local: SocketAddr,
    pub description: String,
    /// SSH host key fingerprint (SHA256:…), for pinning on first use.
    pub fingerprint: Option<String>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Tunnel {
    /// Open a forwarder to `target_host:target_port` through `spec`. The path
    /// is verified up front (SSH login, or a probe through the proxy), so
    /// configuration errors surface before the driver connects.
    pub async fn open(spec: ProxySpec, target_host: &str, target_port: u16) -> Result<Tunnel> {
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| Error::io(format!("Could not open local port: {e}")))?;
        let local = listener.local_addr()?;
        let target = (target_host.to_string(), target_port);
        let description = format!("{} → {}:{}", spec.label(), target_host, target_port);
        match spec {
            ProxySpec::Ssh { host, port, user, auth, known_fingerprint, timeout } => {
                let (session, fingerprint) = ssh_connect(&host, port, &user, &auth, known_fingerprint.as_deref(), timeout).await?;
                let session = Arc::new(Mutex::new(session));
                // Probe once so an unreachable target fails now, not mid-query.
                {
                    let s = session.lock().await;
                    let ch = s
                        .channel_open_direct_tcpip(target.0.clone(), target.1 as u32, "127.0.0.1", 0)
                        .await
                        .map_err(|e| Error::connection(format!("SSH server could not reach {}:{} ({e})", target.0, target.1)))?;
                    let _ = ch.close().await;
                }
                let task = tokio::spawn(async move {
                    loop {
                        let Ok((mut inbound, peer)) = listener.accept().await else { break };
                        let session = session.clone();
                        let target = target.clone();
                        tokio::spawn(async move {
                            let ch = {
                                let s = session.lock().await;
                                s.channel_open_direct_tcpip(target.0.clone(), target.1 as u32, peer.ip().to_string(), peer.port() as u32).await
                            };
                            match ch {
                                Ok(ch) => {
                                    let mut stream = ch.into_stream();
                                    let _ = tokio::io::copy_bidirectional(&mut inbound, &mut stream).await;
                                }
                                Err(e) => tracing::warn!("ssh direct-tcpip failed: {e}"),
                            }
                        });
                    }
                });
                Ok(Tunnel { local, description, fingerprint: Some(fingerprint), task })
            }
            spec @ (ProxySpec::Socks5 { .. } | ProxySpec::Http { .. }) => {
                // Verify the proxy and target up front.
                drop(dial_via_proxy(&spec, &target.0, target.1).await?);
                let spec = Arc::new(spec);
                let task = tokio::spawn(async move {
                    loop {
                        let Ok((mut inbound, _)) = listener.accept().await else { break };
                        let (spec, target) = (spec.clone(), target.clone());
                        tokio::spawn(async move {
                            match dial_via_proxy(&spec, &target.0, target.1).await {
                                Ok(mut out) => {
                                    let _ = tokio::io::copy_bidirectional(&mut inbound, &mut out).await;
                                }
                                Err(e) => tracing::warn!("proxy dial failed: {e}"),
                            }
                        });
                    }
                });
                Ok(Tunnel { local, description, fingerprint: None, task })
            }
        }
    }
}

/// Connect to `host:port` through a SOCKS5 or HTTP proxy.
pub async fn dial_via_proxy(spec: &ProxySpec, host: &str, port: u16) -> Result<TcpStream> {
    let timeout = Duration::from_secs(15);
    let fut = async {
        match spec {
            ProxySpec::Socks5 { host: ph, port: pp, auth } => {
                let proxy = (ph.as_str(), *pp);
                let s = match auth {
                    Some((u, p)) => tokio_socks::tcp::Socks5Stream::connect_with_password(proxy, (host, port), u, p).await,
                    None => tokio_socks::tcp::Socks5Stream::connect(proxy, (host, port)).await,
                };
                s.map(|s| s.into_inner()).map_err(|e| Error::connection(format!("SOCKS5 proxy {ph}:{pp}: {e}")))
            }
            ProxySpec::Http { host: ph, port: pp, auth } => {
                let mut s = TcpStream::connect((ph.as_str(), *pp)).await.map_err(|e| Error::connection(format!("HTTP proxy {ph}:{pp}: {e}")))?;
                let mut req = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\nUser-Agent: OpenVClient\r\n");
                if let Some((u, p)) = auth {
                    let token = base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}"));
                    req.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
                }
                req.push_str("\r\n");
                s.write_all(req.as_bytes()).await?;
                // Read the response head byte by byte so no tunneled bytes are consumed.
                let mut head = Vec::new();
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).await? == 0 || head.len() > 16 * 1024 {
                        return Err(Error::connection("HTTP proxy closed the connection during CONNECT"));
                    }
                    head.push(b[0]);
                }
                let status = String::from_utf8_lossy(&head);
                let line = status.lines().next().unwrap_or("");
                let code = line.split_whitespace().nth(1).unwrap_or("");
                match code {
                    "200" => Ok(s),
                    "407" => Err(Error::auth(format!("HTTP proxy requires authentication ({line})"))),
                    _ => Err(Error::connection(format!("HTTP proxy refused CONNECT to {host}:{port}: {line}"))),
                }
            }
            ProxySpec::Ssh { .. } => Err(Error::invalid("SSH is not a dial-through proxy")),
        }
    };
    tokio::time::timeout(timeout, fut).await.map_err(|_| Error::connection(format!("Timed out connecting through {}", spec.label())))?
}

pub struct HostKeyCheck {
    host: String,
    port: u16,
    expected: Option<String>,
    seen: Arc<std::sync::Mutex<Option<String>>>,
    /// Set when ~/.ssh/known_hosts lists this host with a different key.
    known_hosts_mismatch: Arc<std::sync::atomic::AtomicBool>,
}

impl client::Handler for HostKeyCheck {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &russh::keys::PublicKey) -> Result<bool, Self::Error> {
        let fp = key.fingerprint(HashAlg::Sha256).to_string();
        *self.seen.lock().unwrap() = Some(fp.clone());
        // 1. ~/.ssh/known_hosts, when it knows the host, decides.
        match russh::keys::check_known_hosts(&self.host, self.port, key) {
            Ok(true) => return Ok(true),
            Err(russh::keys::Error::KeyChanged { .. }) => {
                self.known_hosts_mismatch.store(true, std::sync::atomic::Ordering::SeqCst);
                return Ok(false);
            }
            _ => {}
        }
        // 2. Otherwise trust on first use; afterwards the pinned key must match.
        Ok(self.expected.as_ref().is_none_or(|e| e == &fp))
    }
}

/// An SSH server to log in to.
#[derive(Clone, Debug)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: SshAuth,
    /// Pinned SHA256 fingerprint from an earlier connection.
    pub expected: Option<String>,
}

/// Log in to an SSH server; returns the session and the host key fingerprint.
pub async fn ssh_connect(host: &str, port: u16, user: &str, auth: &SshAuth, expected: Option<&str>, timeout: Duration) -> Result<(client::Handle<HostKeyCheck>, String)> {
    let t = SshTarget { host: host.into(), port, user: user.into(), auth: auth.clone(), expected: expected.map(str::to_owned) };
    ssh_connect_via(None, &t, timeout).await
}

/// Log in, optionally through a jump host (like OpenSSH ProxyJump).
pub async fn ssh_connect_via(jump: Option<&client::Handle<HostKeyCheck>>, t: &SshTarget, timeout: Duration) -> Result<(client::Handle<HostKeyCheck>, String)> {
    let config = Arc::new(client::Config { inactivity_timeout: None, keepalive_interval: Some(Duration::from_secs(30)), ..Default::default() });
    let seen = Arc::new(std::sync::Mutex::new(None));
    let mismatch = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handler = HostKeyCheck { host: t.host.clone(), port: t.port, expected: t.expected.clone(), seen: seen.clone(), known_hosts_mismatch: mismatch.clone() };
    let (host, port) = (t.host.as_str(), t.port);
    let connect = async {
        match jump {
            None => client::connect(config, (host, port), handler).await,
            Some(j) => {
                let ch = j.channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 0).await?;
                client::connect_stream(config, ch.into_stream(), handler).await
            }
        }
    };
    let mut session = match tokio::time::timeout(timeout, connect).await {
        Err(_) => return Err(Error::connection(format!("Timed out connecting to SSH {host}:{port}"))),
        Ok(Err(e)) => {
            let fp = seen.lock().unwrap().clone();
            if mismatch.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(Error::auth(format!(
                    "SSH host key for {host} does not match ~/.ssh/known_hosts (got {}). This can mean a man-in-the-middle attack; if the server was rebuilt, update known_hosts (ssh-keygen -R {host}).",
                    fp.unwrap_or_default()
                )));
            }
            if let (Some(exp), Some(got)) = (t.expected.as_deref(), fp) {
                if exp != got {
                    return Err(Error::auth(format!(
                        "SSH host key for {host} changed (expected {exp}, got {got}). This can mean a man-in-the-middle attack; if the server was rebuilt, clear the pinned key in the connection's SSH settings."
                    )));
                }
            }
            return Err(Error::connection(format!("SSH {host}:{port}: {e}")));
        }
        Ok(Ok(s)) => s,
    };
    let fingerprint = seen.lock().unwrap().clone().unwrap_or_default();
    let user = t.user.as_str();
    let ce = |e: russh::Error| Error::connection(e.to_string());
    let load = |path: &std::path::Path, pass: Option<&str>| russh::keys::load_secret_key(path, pass).map_err(|e| Error::invalid(format!("Could not load private key {}: {e}", path.display())));
    let ok = match &t.auth {
        SshAuth::None => session.authenticate_none(user).await.map_err(ce)?.success(),
        SshAuth::Password(p) => session.authenticate_password(user, p).await.map_err(ce)?.success(),
        SshAuth::KeyboardInteractive { password, otp } => {
            use client::KeyboardInteractiveAuthResponse as K;
            let mut r = session.authenticate_keyboard_interactive_start(user, None).await.map_err(ce)?;
            let mut rounds = 0;
            loop {
                match r {
                    K::Success => break true,
                    K::Failure { .. } => break false,
                    K::InfoRequest { prompts, .. } => {
                        rounds += 1;
                        if rounds > 8 {
                            break false;
                        }
                        let answers = prompts
                            .iter()
                            .map(|p| if p.prompt.to_lowercase().contains("pass") { password.clone() } else { otp.clone().unwrap_or_else(|| password.clone()) })
                            .collect();
                        r = session.authenticate_keyboard_interactive_respond(answers).await.map_err(ce)?;
                    }
                }
            }
        }
        SshAuth::Key { path, passphrase } => {
            let key = load(path, passphrase.as_deref())?;
            let hash = session.best_supported_rsa_hash().await.map_err(ce)?.flatten();
            session.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await.map_err(ce)?.success()
        }
        SshAuth::KeyText { pem, passphrase } => {
            let key = russh::keys::decode_secret_key(pem.trim(), passphrase.as_deref()).map_err(|e| Error::invalid(format!("Could not read the pasted private key: {e}")))?;
            let hash = session.best_supported_rsa_hash().await.map_err(ce)?.flatten();
            session.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await.map_err(ce)?.success()
        }
        SshAuth::Certificate { key, cert, passphrase } => {
            let k = load(key, passphrase.as_deref())?;
            let c = russh::keys::load_openssh_certificate(cert).map_err(|e| Error::invalid(format!("Could not load certificate {}: {e}", cert.display())))?;
            session.authenticate_openssh_cert(user, Arc::new(k), c).await.map_err(ce)?.success()
        }
        SshAuth::Agent => agent_auth(&mut session, user).await?,
    };
    if !ok {
        return Err(Error::auth(format!("SSH authentication failed for {user}@{host} ({:?})", t.auth)));
    }
    Ok((session, fingerprint))
}

#[cfg(unix)]
async fn agent_auth(session: &mut client::Handle<HostKeyCheck>, user: &str) -> Result<bool> {
    let agent = russh::keys::agent::client::AgentClient::connect_env().await.map_err(|e| Error::connection(format!("No SSH agent available (SSH_AUTH_SOCK): {e}")))?;
    agent_try(session, user, agent).await
}

#[cfg(windows)]
async fn agent_auth(session: &mut client::Handle<HostKeyCheck>, user: &str) -> Result<bool> {
    // Windows OpenSSH agent pipe first, then Pageant.
    if let Ok(a) = russh::keys::agent::client::AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
        return agent_try(session, user, a).await;
    }
    let a = russh::keys::agent::client::AgentClient::connect_pageant().await.map_err(|e| Error::connection(format!("No SSH agent or Pageant running: {e}")))?;
    agent_try(session, user, a).await
}

async fn agent_try<S: russh::keys::agent::client::AgentStream + Send + Unpin + 'static>(session: &mut client::Handle<HostKeyCheck>, user: &str, mut agent: russh::keys::agent::client::AgentClient<S>) -> Result<bool> {
    let ids = agent.request_identities().await.map_err(|e| Error::connection(format!("SSH agent: {e}")))?;
    if ids.is_empty() {
        return Err(Error::auth("The SSH agent has no keys loaded (ssh-add)"));
    }
    for id in ids {
        let hash = session.best_supported_rsa_hash().await.map_err(|e| Error::connection(e.to_string()))?.flatten();
        let r = match id {
            russh::keys::agent::AgentIdentity::PublicKey { key, .. } => session.authenticate_publickey_with(user, key, hash, &mut agent).await,
            russh::keys::agent::AgentIdentity::Certificate { certificate, .. } => session.authenticate_certificate_with(user, certificate, hash, &mut agent).await,
        };
        if r.is_ok_and(|r| r.success()) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Replace host/port in driver params with the tunnel's local endpoint.
pub fn rewrite_params(p: &mut ConnectParams, local: SocketAddr) {
    p.values.insert("host".into(), local.ip().to_string());
    p.values.insert("port".into(), local.port().to_string());
}

#[cfg(test)]
mod tests;
