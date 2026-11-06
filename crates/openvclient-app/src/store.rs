//! Saved workspace (profiles, folders, preferences) and keychain secrets.

use std::path::PathBuf;

use openvclient_core::ConnectionProfile;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const KEYCHAIN_SERVICE: &str = "dev.openvclient.client";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub theme: ThemeChoice,
    #[serde(default = "default_rows")]
    pub max_rows: usize,
    #[serde(default = "default_page")]
    pub page_size: u64,
    #[serde(default = "default_font")]
    pub editor_font_size: f32,
    #[serde(default = "default_true")]
    pub confirm_production: bool,
    #[serde(default = "default_scroll")]
    pub scroll_speed: f32,
    /// Look for a newer release on GitHub at startup.
    #[serde(default = "default_true")]
    pub check_updates: bool,
}

fn default_scroll() -> f32 {
    0.6
}

fn default_rows() -> usize {
    10_000
}
fn default_page() -> u64 {
    200
}
fn default_font() -> f32 {
    13.5
}
fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::System,
            max_rows: default_rows(),
            page_size: default_page(),
            editor_font_size: default_font(),
            confirm_production: true,
            scroll_speed: default_scroll(),
            check_updates: true,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Workspace {
    #[serde(default)]
    pub profiles: Vec<ConnectionProfile>,
    /// Folders that exist even when empty.
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub settings: Settings,
    /// Recently run SQL per connection, newest first.
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub profile: Uuid,
    pub sql: String,
    pub at: String,
    pub ok: bool,
}

pub struct Store {
    path: PathBuf,
    pub ws: Workspace,
    pub load_error: Option<String>,
}

/// Owner-only permissions: the workspace holds SQL history and hosts.
pub fn restrict(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

pub fn config_dir() -> PathBuf {
    if let Ok(p) = std::env::var("OPENVCLIENT_CONFIG_DIR") {
        return PathBuf::from(p);
    }
    directories::ProjectDirs::from("dev", "OpenVClient", "OpenVClient")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".openvclient"))
}

impl Store {
    pub fn load() -> Self {
        let path = config_dir().join("workspace.json");
        // One-time move from the pre-rename location ("OpenV").
        if !path.exists() && std::env::var("OPENVCLIENT_CONFIG_DIR").is_err() {
            if let Some(old) = directories::ProjectDirs::from("dev", "OpenV", "OpenV").map(|d| d.config_dir().join("workspace.json")) {
                if old.exists() {
                    let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
                    let _ = std::fs::copy(&old, &path);
                }
            }
        }
        let (ws, load_error) = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(ws) => (ws, None),
                Err(e) => {
                    // Keep the unreadable file so nothing is lost.
                    let backup = path.with_extension(format!("broken-{}.json", chrono::Utc::now().timestamp()));
                    let _ = std::fs::copy(&path, &backup);
                    (Workspace::default(), Some(format!("Could not read {}: {e}. A copy was kept at {}", path.display(), backup.display())))
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Workspace::default(), None),
            Err(e) => (Workspace::default(), Some(format!("Could not read {}: {e}", path.display()))),
        };
        Self { path, ws, load_error }
    }

    /// Atomic write: temp file then rename.
    pub fn save(&self) -> Result<(), String> {
        let dir = self.path.parent().ok_or("bad config path")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        let data = serde_json::to_vec_pretty(&self.ws).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
        restrict(&tmp);
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    pub fn profile(&self, id: Uuid) -> Option<&ConnectionProfile> {
        self.ws.profiles.iter().find(|p| p.id == id)
    }

    pub fn profile_mut(&mut self, id: Uuid) -> Option<&mut ConnectionProfile> {
        self.ws.profiles.iter_mut().find(|p| p.id == id)
    }

    pub fn upsert(&mut self, p: ConnectionProfile) {
        match self.ws.profiles.iter_mut().find(|x| x.id == p.id) {
            Some(slot) => *slot = p,
            None => self.ws.profiles.push(p),
        }
    }

    pub fn remove(&mut self, id: Uuid) {
        self.ws.profiles.retain(|p| p.id != id);
        self.ws.history.retain(|h| h.profile != id);
    }

    /// Every folder path, including parents of nested folders.
    pub fn all_folders(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut add = |f: &str| {
            let mut acc = String::new();
            for part in f.split('/').filter(|p| !p.is_empty()) {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(part);
                if !v.contains(&acc) {
                    v.push(acc.clone());
                }
            }
        };
        for f in &self.ws.folders {
            add(f);
        }
        for p in &self.ws.profiles {
            add(&p.folder);
        }
        v.sort_by_key(|s| s.to_lowercase());
        v
    }

    pub fn push_history(&mut self, profile: Uuid, sql: &str, ok: bool) {
        let sql = sql.trim();
        if sql.is_empty() {
            return;
        }
        self.ws.history.retain(|h| !(h.profile == profile && h.sql == sql));
        self.ws.history.insert(0, HistoryEntry { profile, sql: sql.to_string(), at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(), ok });
        self.ws.history.truncate(500);
    }
}

/// OS keychain (macOS Keychain, Windows Credential Manager, Secret Service).
pub mod secrets {
    use super::*;

    fn entry(id: Uuid) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, &id.to_string()).map_err(|e| format!("Keychain unavailable: {e}"))
    }

    pub fn get(id: Uuid) -> Result<Option<String>, String> {
        match entry(id)?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("Keychain read failed: {e}")),
        }
    }

    pub fn set(id: Uuid, secret: &str) -> Result<(), String> {
        entry(id)?.set_password(secret).map_err(|e| format!("Keychain write failed: {e}"))
    }

    /// Additional secrets (SSH password, proxy password…) for a profile.
    pub fn get_named(id: Uuid, name: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new(KEYCHAIN_SERVICE, &format!("{id}:{name}")).map_err(|e| e.to_string())?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("Keychain read failed: {e}")),
        }
    }

    pub fn set_named(id: Uuid, name: &str, secret: &str) -> Result<(), String> {
        let e = keyring::Entry::new(KEYCHAIN_SERVICE, &format!("{id}:{name}")).map_err(|e| e.to_string())?;
        if secret.is_empty() {
            match e.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(err) => Err(err.to_string()),
            }
        } else {
            e.set_password(secret).map_err(|err| format!("Keychain write failed: {err}"))
        }
    }

    /// Remove only the main password (network-path secrets stay).
    pub fn delete_main(id: Uuid) -> Result<(), String> {
        match entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Keychain delete failed: {e}")),
        }
    }

    /// Remove every secret of a connection (when it is deleted).
    pub fn delete(id: Uuid) -> Result<(), String> {
        for n in [openvclient_net::keys::SECRET_SSH_PASSWORD, openvclient_net::keys::SECRET_SSH_PASSPHRASE, openvclient_net::keys::SECRET_PROXY_PASSWORD] {
            let _ = set_named(id, n, "");
        }
        match entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Keychain delete failed: {e}")),
        }
    }
}
