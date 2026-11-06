use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::containers::ContainerSession;
use crate::kv::KeyValueSession;
use crate::resource::ResourceSession;
use crate::sql::SqlSession;
use crate::{ConnectParams, Result};

/// Top-level grouping in the sidebar and the "new connection" picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Relational,
    Analytical,
    Document,
    KeyValue,
    Search,
    Messaging,
    Storage,
    Remote,
    Containers,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Relational => "Relational databases",
            Category::Analytical => "Analytical & wide-column",
            Category::Document => "Document databases",
            Category::KeyValue => "Key-value stores",
            Category::Search => "Search engines",
            Category::Messaging => "Message brokers",
            Category::Storage => "Object & file storage",
            Category::Remote => "Remote servers",
            Category::Containers => "Containers",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    Text,
    Number,
    /// Stored in the OS keychain, never in the profile file.
    Secret,
    /// A local file path; `save` picks a save dialog instead of open.
    File {
        save: bool,
        filters: &'static [&'static str],
    },
    Bool,
    Choice(&'static [&'static str]),
}

/// One input in the connection form. Drivers describe their own fields, so
/// adding a connector never touches the form code.
#[derive(Clone, Debug)]
pub struct FieldSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub default: &'static str,
    pub placeholder: &'static str,
    pub required: bool,
    pub help: &'static str,
    /// Shown under "Advanced" rather than in the main form.
    pub advanced: bool,
    /// Only shown when another field has one of these values.
    pub show_if: Option<(&'static str, &'static [&'static str])>,
}

impl FieldSpec {
    pub const fn text(key: &'static str, label: &'static str) -> Self {
        Self { key, label, kind: FieldKind::Text, default: "", placeholder: "", required: false, help: "", advanced: false, show_if: None }
    }
    pub const fn show_if(mut self, key: &'static str, values: &'static [&'static str]) -> Self {
        self.show_if = Some((key, values));
        self
    }
    pub const fn kind(mut self, kind: FieldKind) -> Self {
        self.kind = kind;
        self
    }
    pub const fn default(mut self, v: &'static str) -> Self {
        self.default = v;
        self
    }
    pub const fn placeholder(mut self, v: &'static str) -> Self {
        self.placeholder = v;
        self
    }
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }
    pub const fn help(mut self, v: &'static str) -> Self {
        self.help = v;
        self
    }
    pub const fn advanced(mut self) -> Self {
        self.advanced = true;
        self
    }
}

/// Standard host/port/user/password fields for network databases.
pub fn network_fields(default_port: &'static str, default_user: &'static str) -> Vec<FieldSpec> {
    vec![
        FieldSpec::text("host", "Host").default("localhost").required(),
        FieldSpec::text("port", "Port").kind(FieldKind::Number).default(default_port),
        FieldSpec::text("user", "User").default(default_user),
        FieldSpec::text("password", "Password").kind(FieldKind::Secret),
    ]
}

#[derive(Clone, Debug)]
pub struct DriverInfo {
    /// Stable identifier persisted in profiles, e.g. `postgres`.
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub description: &'static str,
    /// Short glyph shown in the sidebar.
    pub badge: &'static str,
    pub fields: Vec<FieldSpec>,
}

/// A connector. Implementations live in their own crates and are registered
/// at startup; the UI only talks to these traits.
#[async_trait]
pub trait Driver: Send + Sync + 'static {
    fn info(&self) -> &DriverInfo;
    async fn connect(&self, params: ConnectParams) -> Result<SessionHandle>;
}

#[derive(Clone, Debug, Default)]
pub struct ServerInfo {
    /// Product as reported by the server, e.g. `PostgreSQL 16.4`.
    pub product: String,
    pub version: String,
    /// Extra facts shown in the connection header (encoding, user…).
    pub details: Vec<(String, String)>,
}

/// Operations every live session supports.
#[async_trait]
pub trait ServiceSession: Send + Sync {
    fn server_info(&self) -> ServerInfo;
    /// Round-trip check used by health indicators.
    async fn ping(&self) -> Result<Duration>;
    async fn close(&self);
}

/// A connected session, tagged with the family of operations it supports.
#[derive(Clone)]
pub enum SessionHandle {
    Sql(Arc<dyn SqlSession>),
    KeyValue(Arc<dyn KeyValueSession>),
    Containers(Arc<dyn ContainerSession>),
    Resource(Arc<dyn ResourceSession>),
}

impl SessionHandle {
    pub fn base(&self) -> &dyn ServiceSession {
        // Trait upcasting: every family trait has ServiceSession as supertrait.
        match self {
            SessionHandle::Sql(s) => &**s,
            SessionHandle::KeyValue(s) => &**s,
            SessionHandle::Containers(s) => &**s,
            SessionHandle::Resource(s) => &**s,
        }
    }
    pub fn sql(&self) -> Option<&Arc<dyn SqlSession>> {
        match self {
            SessionHandle::Sql(s) => Some(s),
            _ => None,
        }
    }
    pub fn kv(&self) -> Option<&Arc<dyn KeyValueSession>> {
        match self {
            SessionHandle::KeyValue(s) => Some(s),
            _ => None,
        }
    }
    pub fn resource(&self) -> Option<&Arc<dyn ResourceSession>> {
        match self {
            SessionHandle::Resource(s) => Some(s),
            _ => None,
        }
    }
    pub fn containers(&self) -> Option<&Arc<dyn ContainerSession>> {
        match self {
            SessionHandle::Containers(s) => Some(s),
            _ => None,
        }
    }
}

pub fn elapsed_ms(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 1.0 {
        format!("{:.2} ms", ms)
    } else if ms < 1000.0 {
        format!("{:.0} ms", ms)
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// Display text for a `FieldKind::Choice` value (the stored value stays
/// the same): "entra_interactive" -> "Microsoft Entra ID: interactive sign-in".
pub fn choice_label(field: &str, value: &str) -> String {
    let known = match (field, value) {
        // Sign-in methods
        (_, "sql") => "SQL Server login",
        (_, "windows") => "Windows (NTLM user and password)",
        (_, "integrated") => "Integrated (current Windows login)",
        (_, "entra_password") => "Microsoft Entra ID: password",
        (_, "entra_service_principal") => "Microsoft Entra ID: service principal",
        (_, "entra_managed_identity") => "Microsoft Entra ID: managed identity",
        (_, "entra_interactive") => "Microsoft Entra ID: interactive sign-in",
        (_, "entra_default") => "Microsoft Entra ID: default credentials",
        (_, "entra_token") => "Microsoft Entra ID: access token",
        (_, "account_key") => "Account name and key",
        (_, "connection_string") => "Connection string",
        (_, "sas") => "SAS (shared access signature)",
        (_, "anonymous") => "Anonymous (public access)",
        (_, "azurite") => "Azurite (local emulator)",
        (_, "api_key") => "API key",
        (_, "basic") => "User and password",
        (_, "keyboard_interactive") => "Keyboard-interactive (OTP / 2FA)",
        (_, "key") => "Private key file",
        (_, "key_text") => "Pasted private key",
        (_, "certificate") => "OpenSSH certificate",
        (_, "agent") => "SSH agent",
        (_, "password") => "Password",
        // Transport security
        (_, "ftps") => "FTPS (explicit TLS)",
        (_, "ftps_implicit") => "FTPS (implicit TLS)",
        (_, "plain") => "Plain FTP (not encrypted)",
        ("encrypt", "required") => "Required (verify certificate)",
        ("encrypt", "on") => "On (trust any certificate)",
        ("encrypt", "off") => "Off (login only)",
        (_, "verify-ca") => "Verify CA",
        (_, "verify-full") => "Verify full",
        (_, "verify_identity") => "Verify identity",
        // Clouds and misc
        ("cloud", "public") => "Azure public cloud",
        ("cloud", "china") => "Azure China",
        ("cloud", "usgov") => "Azure US Government",
        ("ackmode", "requeue") => "Peek (put messages back)",
        ("ackmode", "ack") => "Take (remove from queue)",
        (_, "none") => "None",
        (_, "default") => "Default",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    // Fallback: snake/kebab case to words, keeping common acronyms.
    const ACRONYMS: &[&str] =
        &["sql", "ssh", "sftp", "ftp", "ftps", "tls", "ssl", "id", "api", "url", "uri", "sas", "http", "https", "aws", "ca", "otp", "json", "csv", "rest", "s3", "ip", "dns", "ad"];
    let mut out = Vec::new();
    for (i, w) in value.split(['_', '-']).filter(|w| !w.is_empty()).enumerate() {
        let lw = w.to_ascii_lowercase();
        if ACRONYMS.contains(&lw.as_str()) {
            out.push(lw.to_ascii_uppercase());
        } else if w.chars().any(|c| c.is_ascii_uppercase()) {
            out.push(w.to_string()); // keep e.g. "Hot", "Archive"
        } else if i == 0 {
            let mut c = lw.chars();
            out.push(c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default());
        } else {
            out.push(lw);
        }
    }
    if out.is_empty() { value.to_string() } else { out.join(" ") }
}

#[cfg(test)]
mod choice_tests {
    use super::choice_label;

    #[test]
    fn labels() {
        assert_eq!(choice_label("auth", "sas"), "SAS (shared access signature)");
        assert_eq!(choice_label("auth", "entra_interactive"), "Microsoft Entra ID: interactive sign-in");
        assert_eq!(choice_label("auth", "connection_string"), "Connection string");
        assert_eq!(choice_label("encrypt", "on"), "On (trust any certificate)");
        assert_eq!(choice_label("x", "some_new_value"), "Some new value");
        assert_eq!(choice_label("x", "rest_api"), "REST API");
        assert_eq!(choice_label("tier", "Hot"), "Hot");
        assert_eq!(choice_label("type", "fanout"), "Fanout");
    }
}
