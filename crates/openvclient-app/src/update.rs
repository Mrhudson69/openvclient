//! Update check against the project's GitHub releases. The app never
//! replaces itself silently: it tells the user and opens the download page
//! (or downloads the matching installer when one is attached).

use std::time::Duration;

use openvclient_core::{Error, Result};

pub const REPO: &str = "Mrhudson69/openvclient";
/// Project website.
pub const WEBSITE: &str = "https://openvclient.xyz/";
/// Recent releases, newest first (includes pre-releases, excludes drafts).
const API: &str = "https://api.github.com/repos/Mrhudson69/openvclient/releases?per_page=30";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub page: String,
    pub notes: String,
    /// Installer/archive for this OS and CPU, when the release has one.
    pub asset: Option<(String, String)>,
}

/// A SemVer version: numeric core plus pre-release identifiers
/// (`1.0.0-beta.2` → `[1,0,0]`, `["beta", "2"]`). Build metadata is ignored.
#[derive(Debug, PartialEq, Eq)]
struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

fn parse(v: &str) -> Version {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let v = v.split('+').next().unwrap_or(v);
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, p.split('.').map(str::to_owned).collect()),
        None => (v, Vec::new()),
    };
    let mut it = core.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    Version { core: [it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0)], pre }
}

impl Ord for Version {
    /// SemVer precedence: 1.0.0-beta.1 < 1.0.0-beta.2 < 1.0.0-rc.1 < 1.0.0.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Equal,
            (true, false) => Greater,
            (false, true) => Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let o = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Less,
                        (Err(_), Ok(_)) => Greater,
                        (Err(_), Err(_)) => a.cmp(b),
                    };
                    if o != Equal {
                        return o;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
fn is_newer(latest: &str, current: &str) -> bool {
    parse(latest) > parse(current)
}

/// Is `v` a pre-release (beta, rc…)?
pub fn is_prerelease(v: &str) -> bool {
    !parse(v).pre.is_empty()
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
    let list: serde_json::Value = resp.json().await.map_err(|e| Error::other(e.to_string()))?;
    let current = env!("CARGO_PKG_VERSION");
    // Stable builds are offered stable releases only; a beta also sees newer betas.
    let Some(v) = pick(list.as_array().map(Vec::as_slice).unwrap_or(&[]), current) else { return Ok(None) };
    let tag = v.get("tag_name").and_then(|t| t.as_str()).unwrap_or("").to_string();
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

/// The newest published release worth offering to `current`, if any.
fn pick<'a>(releases: &'a [serde_json::Value], current: &str) -> Option<&'a serde_json::Value> {
    let include_pre = is_prerelease(current);
    releases
        .iter()
        .filter(|r| !r.get("draft").and_then(|d| d.as_bool()).unwrap_or(false))
        .filter(|r| include_pre || !r.get("prerelease").and_then(|p| p.as_bool()).unwrap_or(false))
        .filter_map(|r| Some((parse(r.get("tag_name")?.as_str()?), r)))
        .filter(|(v, _)| *v > parse(current))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, r)| r)
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
        assert!(is_newer("v1.0.0-beta.2", "1.0.0-beta.1"));
        assert!(is_newer("1.0.0-beta.10", "1.0.0-beta.9"));
        assert!(is_newer("1.0.0-rc.1", "1.0.0-beta.3"));
        assert!(!is_newer("1.0.0-beta.1", "1.0.0-beta.1"));
        assert!(is_prerelease("1.0.0-beta.1") && !is_prerelease("v1.0.0"));
        // Which release is offered.
        let rel = |tag: &str, pre: bool, draft: bool| serde_json::json!({ "tag_name": tag, "prerelease": pre, "draft": draft });
        let list = vec![rel("v1.1.0-beta.1", true, false), rel("v1.0.0", false, false), rel("v1.0.0-beta.2", true, false), rel("v2.0.0", false, true)];
        let tag = |r: Option<&serde_json::Value>| r.and_then(|r| r["tag_name"].as_str().map(str::to_owned));
        assert_eq!(tag(pick(&list, "1.0.0-beta.1")).as_deref(), Some("v1.1.0-beta.1"), "a beta sees newer betas and stables");
        assert_eq!(tag(pick(&list, "1.0.0")), None, "a stable build is not offered betas or drafts");
        assert_eq!(tag(pick(&list, "0.9.0")).as_deref(), Some("v1.0.0"));
        assert_eq!(tag(pick(&[rel("v1.0.0-beta.1", true, false)], "1.0.0-beta.1")), None);
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
