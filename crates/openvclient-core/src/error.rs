use std::fmt;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Error surfaced to the user. Connectors map driver errors into one of these
/// kinds so the UI can decide between "reconnect", "fix your input" and "the
/// engine does not do that".
#[derive(Debug, Clone)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    /// Engine-specific detail such as SQLSTATE, hint or position.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Could not reach or lost the server; reconnecting may help.
    Connection,
    /// Credentials rejected.
    Auth,
    /// The server rejected the request (syntax error, constraint, permission…).
    Server,
    /// The engine or this connector does not support the operation.
    Unsupported,
    /// Bad input from the user or configuration.
    Invalid,
    /// The operation was cancelled by the user.
    Cancelled,
    /// Local I/O (files, external tools).
    Io,
    Other,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), detail: None }
    }
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let d = detail.into();
        if !d.is_empty() {
            self.detail = Some(d);
        }
        self
    }
    pub fn connection(m: impl Into<String>) -> Self {
        Self::new(ErrorKind::Connection, m)
    }
    pub fn auth(m: impl Into<String>) -> Self {
        Self::new(ErrorKind::Auth, m)
    }
    pub fn server(m: impl Into<String>) -> Self {
        Self::new(ErrorKind::Server, m)
    }
    pub fn unsupported(m: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, m)
    }
    pub fn invalid(m: impl Into<String>) -> Self {
        Self::new(ErrorKind::Invalid, m)
    }
    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "Cancelled")
    }
    pub fn io(m: impl Into<String>) -> Self {
        Self::new(ErrorKind::Io, m)
    }
    pub fn other(m: impl Into<String>) -> Self {
        Self::new(ErrorKind::Other, m)
    }
    pub fn is_connection(&self) -> bool {
        self.kind == ErrorKind::Connection
    }
}

/// Hide passwords in `scheme://user:password@host` URLs inside any text.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        let end = tail.find(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'').unwrap_or(tail.len());
        let authority = &tail[..end];
        match authority.rfind('@') {
            Some(at) if authority[..at].contains(':') => {
                let user = &authority[..authority[..at].find(':').unwrap()];
                out.push_str(&format!("{user}:***{}", &authority[at..]));
            }
            _ => out.push_str(authority),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

impl Error {
    /// Plain-language explanation and next step for common failures.
    pub fn hint(&self) -> Option<&'static str> {
        let m = format!("{} {}", self.message, self.detail.as_deref().unwrap_or("")).to_lowercase();
        let has = |s: &str| m.contains(s);
        Some(if has("connection refused") || has("os error 61") || has("os error 111") || has("actively refused") || has("10061") {
            "Nothing is listening on that host and port. Check that the server is running, the port number is right, and no firewall blocks it."
        } else if has("timed out") || has("timeout") || has("os error 60") {
            "The server did not answer in time. Check the host name, VPN or network route, firewall rules, and that the server accepts remote connections."
        } else if has("failed to lookup address") || has("nodename nor servname") || has("name or service not known") || has("no such host") || has("dns") {
            "The host name could not be resolved. Check its spelling, your DNS/VPN, or use an IP address."
        } else if has("certificate") || has("tls") || has("ssl") || has("handshake") {
            "The secure (TLS) connection failed. Try the connection's TLS/SSL option (e.g. require without verification for self-signed certificates), or check the server's certificate."
        } else if has("too many connections") || has("remaining connection slots") || has("max_connections") {
            "The server reached its connection limit. Close idle sessions or raise the limit on the server."
        } else if has("password authentication failed") || has("access denied") || has("login failed") || has("authentication failed") || has("wrongpass") || has("noauth") || self.kind == ErrorKind::Auth {
            "The server rejected the credentials. Check the user and password (and for MySQL the 'user'@'host' the account allows); for SSH tunnels check the SSH login too."
        } else if (has("database") || has("schema")) && (has("does not exist") || has("unknown database") || has("cannot open database")) {
            "The database name is wrong or you lack access to it. Check the Database field in the connection."
        } else if has("permission denied") || has("insufficient privilege") || has("not authorized") {
            "Your account lacks the privilege for this action. Ask an administrator to grant it, or use a different account."
        } else if has("read-only") || has("read only") {
            "The connection or server is read-only, so changes are refused."
        } else if has("host key") {
            "The SSH server's identity changed since the last connection. Verify it with the server's administrator before trusting the new key."
        } else if self.kind == ErrorKind::Connection {
            "The connection failed or dropped. Check the network and that the server is up, then reconnect."
        } else {
            return None;
        })
    }

    /// Everything useful for a bug report, without secrets.
    pub fn diagnostics(&self) -> String {
        let mut s = format!("{:?}: {}", self.kind, redact(&self.message));
        if let Some(d) = &self.detail {
            s.push_str(&format!("\nDetail: {}", redact(d)));
        }
        if let Some(h) = self.hint() {
            s.push_str(&format!("\nHint: {h}"));
        }
        s.push_str(&format!("\nOS: {} {}", std::env::consts::OS, std::env::consts::ARCH));
        s
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(d) = &self.detail {
            write!(f, "\n{d}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::io(e.to_string())
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(e: tokio::task::JoinError) -> Self {
        if e.is_cancelled() { Error::cancelled() } else { Error::other(format!("Background task failed: {e}")) }
    }
}

#[cfg(test)]
mod redact_tests {
    #[test]
    fn hides_url_passwords() {
        assert_eq!(super::redact("mongodb://bob:s3cr3t@db:27017/x failed"), "mongodb://bob:***@db:27017/x failed");
        assert_eq!(super::redact("http://host:9200/_search"), "http://host:9200/_search");
        assert_eq!(super::redact("no url"), "no url");
    }
}
