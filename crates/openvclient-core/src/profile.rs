use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Deployment stage a connection belongs to. Production connections get a
/// red accent and confirmation on destructive actions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Environment {
    #[default]
    Development,
    Testing,
    Staging,
    Production,
}

impl Environment {
    pub const ALL: [Environment; 4] = [Environment::Development, Environment::Testing, Environment::Staging, Environment::Production];

    pub fn label(self) -> &'static str {
        match self {
            Environment::Development => "Development",
            Environment::Testing => "Testing",
            Environment::Staging => "Staging",
            Environment::Production => "Production",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Environment::Development => "DEV",
            Environment::Testing => "TEST",
            Environment::Staging => "STAGE",
            Environment::Production => "PROD",
        }
    }

    pub fn is_protected(self) -> bool {
        self == Environment::Production
    }
}

/// A saved connection. Secrets never live here: they are kept in the OS
/// keychain under [`ConnectionProfile::id`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: Uuid,
    pub name: String,
    pub driver: String,
    /// Slash-separated folder path, e.g. `Customers/ACME`. Empty = root.
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub environment: Environment,
    /// Driver parameters keyed by [`crate::FieldSpec::key`].
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// Whether a secret was saved to the keychain for this profile.
    #[serde(default)]
    pub save_secret: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub last_connected_at: Option<String>,
}

impl ConnectionProfile {
    pub fn new(driver: &str, name: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_owned(),
            driver: driver.to_owned(),
            folder: String::new(),
            environment: Environment::Development,
            params: BTreeMap::new(),
            save_secret: true,
            read_only: false,
            created_at: Some(chrono::Utc::now().to_rfc3339()),
            last_connected_at: None,
        }
    }

    pub fn param(&self, key: &str) -> &str {
        self.params.get(key).map(String::as_str).unwrap_or("")
    }

    /// One-line summary for lists: `user@host:port/db` or a file path.
    pub fn summary(&self) -> String {
        let p = |k: &str| self.params.get(k).filter(|v| !v.is_empty());
        if let Some(path) = p("path") {
            return path.clone();
        }
        let mut s = String::new();
        if let Some(u) = p("user") {
            s.push_str(u);
            s.push('@');
        }
        s.push_str(p("host").map(String::as_str).unwrap_or("localhost"));
        if let Some(port) = p("port") {
            s.push(':');
            s.push_str(port);
        }
        if let Some(db) = p("database") {
            s.push('/');
            s.push_str(db);
        }
        s
    }
}

/// Lets a driver tell the user something mid-connect (e.g. a sign-in code).
#[derive(Clone)]
pub struct Notifier(pub std::sync::Arc<dyn Fn(String) + Send + Sync>);

impl std::fmt::Debug for Notifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Notifier")
    }
}

/// Everything a driver needs to open a session.
#[derive(Clone, Debug)]
pub struct ConnectParams {
    pub values: BTreeMap<String, String>,
    pub secret: Option<String>,
    pub read_only: bool,
    pub notifier: Option<Notifier>,
}

impl ConnectParams {
    pub fn from_profile(p: &ConnectionProfile, secret: Option<String>) -> Self {
        Self { values: p.params.clone(), secret, read_only: p.read_only, notifier: None }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str).filter(|v| !v.trim().is_empty())
    }

    pub fn get_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.get(key).unwrap_or(default)
    }

    pub fn notify(&self, msg: impl Into<String>) {
        if let Some(n) = &self.notifier {
            (n.0)(msg.into());
        }
    }

    pub fn flag(&self, key: &str) -> bool {
        matches!(self.get(key), Some("true" | "1" | "yes" | "on"))
    }

    pub fn port(&self, default: u16) -> crate::Result<u16> {
        match self.get("port") {
            None => Ok(default),
            Some(p) => p.trim().parse().map_err(|_| crate::Error::invalid(format!("Invalid port: {p}"))),
        }
    }
}
