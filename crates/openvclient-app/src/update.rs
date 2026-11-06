//! Update check against the project's GitHub releases. The app never
//! replaces itself silently: it tells the user and opens the download page
//! (or downloads the matching installer when one is attached).

use std::time::Duration;

use openvclient_core::{Error, Result};

pub const REPO: &str = "Mrhudson69/openvclient";
const API: &str = "https://api.github.com/repos/Mrhudson69/openvclient/releases/latest";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub page: String,
    pub notes: String,
    /// Installer/archive for this OS and CPU, when the release has one.
    pub asset: Option<(String, String)>,
}

/// Parse "v1.2.3" / "1.2.3-beta.1" into comparable parts (pre-releases sort lower).
fn parse(v: &str) -> (u64, u64, u64, bool) {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match v.split_once('-') {
        Some((c, _)) => (c, true),
        None => (v, false),
    };
    let mut it = core.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0), !pre)
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    parse(latest) > parse(current)
}

fn asset_matches(name: &str) -> bool {
    let n = name.to_lowercase();
    let os = match std::env::consts::OS {
        "macos" => ["macos", "darwin", ".dmg"].iter().any(|k| n.contains(k)),
        "windows" => ["windows", "win64", ".msi", ".exe"].iter().any(|k| n.contains(k)),
        _ => ["linux", ".appimage", ".deb", ".rpm"].iter().any(|k| n.contains(k)),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => n.contains("aarch64") || n.contains("arm64") || n.contains("universal"),
        _ => n.contains("x86_64") || n.contains("amd64") || n.contains("x64") || n.contains("universal"),
    };
    os && arch && !n.ends_with(".sha256") && !n.contains("sha256sums")
}

/// Ask GitHub for the latest release; `None` when already up to date.
pub async fn check() -> Result<Option<Release>> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).user_agent(concat!("OpenVClient/", env!("CARGO_PKG_VERSION"))).build().map_err(|e| Error::other(e.to_string()))?;
    let resp = client.get(API).header("Accept", "application/vnd.github+json").send().await.map_err(|e| Error::connection(format!("Could not reach GitHub: {e}")))?;
    if resp.status().as_u16() == 404 {
        return Ok(None); // no published release yet
    }
    if !resp.status().is_success() {
        return Err(Error::server(format!("GitHub answered HTTP {}", resp.status().as_u16())));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| Error::other(e.to_string()))?;
    let tag = v.get("tag_name").and_then(|t| t.as_str()).unwrap_or("").to_string();
    if tag.is_empty() || !is_newer(&tag, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let asset = v.get("assets").and_then(|a| a.as_array()).and_then(|a| {
        a.iter().find_map(|x| {
            let name = x.get("name")?.as_str()?;
            let url = x.get("browser_download_url")?.as_str()?;
            asset_matches(name).then(|| (name.to_string(), url.to_string()))
        })
    });
    Ok(Some(Release {
        version: tag.trim_start_matches('v').to_string(),
        page: v.get("html_url").and_then(|u| u.as_str()).map(str::to_owned).unwrap_or_else(|| format!("https://github.com/{REPO}/releases/latest")),
        notes: v.get("body").and_then(|b| b.as_str()).unwrap_or("").chars().take(2000).collect(),
        asset,
    }))
}

/// Download the release asset into the user's Downloads folder.
pub async fn download(name: String, url: String) -> Result<std::path::PathBuf> {
    let dir = directories::UserDirs::new().and_then(|d| d.download_dir().map(|p| p.to_path_buf())).unwrap_or_else(std::env::temp_dir);
    // Only keep a plain file name from the server.
    let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect();
    let path = dir.join(if safe.is_empty() { "openvclient-update".into() } else { safe });
    let client = reqwest::Client::builder().timeout(Duration::from_secs(1800)).user_agent("OpenVClient").build().map_err(|e| Error::other(e.to_string()))?;
    let mut resp = client.get(&url).send().await.map_err(|e| Error::connection(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(Error::server(format!("Download failed: HTTP {}", resp.status().as_u16())));
    }
    let mut f = tokio::fs::File::create(&path).await.map_err(|e| Error::io(format!("{}: {e}", path.display())))?;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = resp.chunk().await.map_err(|e| Error::connection(e.to_string()))? {
        f.write_all(&chunk).await.map_err(|e| Error::io(e.to_string()))?;
    }
    f.flush().await.map_err(|e| Error::io(e.to_string()))?;
    Ok(path)
}

/// Open a web page in the browser, or a local folder in the file manager.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        // explorer.exe falls back to "Documents" when a local path uses forward
        // slashes (e.g. OPENVCLIENT_CONFIG_DIR=C:/ovc), so normalise them.
        let target = if url.contains("://") { url.to_string() } else { url.replace('/', "\\") };
        let _ = std::process::Command::new("explorer").arg(target).spawn();
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = std::process::Command::new(opener).arg(url).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "1.0.0-beta.2"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0-rc.1", "0.1.0"));
        assert!(asset_matches(&format!(
            "openvclient-1.0.0-{}-{}.zip",
            if cfg!(target_os = "macos") {
                "macos"
            } else if cfg!(windows) {
                "windows"
            } else {
                "linux"
            },
            std::env::consts::ARCH
        )));
    }
}
