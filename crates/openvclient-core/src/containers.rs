//! Contract for container engines (Docker Engine API and compatibles).

use async_trait::async_trait;

use crate::{Result, ServiceSession};

#[derive(Clone, Debug)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    /// `running`, `exited`, `paused`…
    pub state: String,
    /// Human status, e.g. `Up 3 hours`.
    pub status: String,
    pub ports: String,
    pub created: i64,
    pub compose_project: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Kill,
    Remove { force: bool },
}

impl ContainerAction {
    pub fn label(self) -> &'static str {
        match self {
            ContainerAction::Start => "Start",
            ContainerAction::Stop => "Stop",
            ContainerAction::Restart => "Restart",
            ContainerAction::Pause => "Pause",
            ContainerAction::Unpause => "Unpause",
            ContainerAction::Kill => "Kill",
            ContainerAction::Remove { .. } => "Remove",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImageInfo {
    pub id: String,
    pub tags: Vec<String>,
    pub size: i64,
    pub created: i64,
    pub containers: i64,
}

#[derive(Clone, Debug)]
pub struct VolumeInfo {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub created: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NetworkInfo {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
}

#[derive(Clone, Debug, Default)]
pub struct ContainerStats {
    pub cpu_percent: f64,
    pub memory_usage: u64,
    pub memory_limit: u64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub block_read: u64,
    pub block_write: u64,
    pub pids: u64,
}

#[async_trait]
pub trait ContainerSession: ServiceSession {
    async fn containers(&self, all: bool) -> Result<Vec<ContainerInfo>>;
    async fn container_action(&self, id: &str, action: ContainerAction) -> Result<()>;
    async fn logs(&self, id: &str, tail: usize, timestamps: bool) -> Result<String>;
    async fn inspect(&self, id: &str) -> Result<String>;
    async fn stats(&self, id: &str) -> Result<ContainerStats>;
    /// Run a command inside a container and capture its output.
    async fn exec(&self, id: &str, cmd: Vec<String>) -> Result<(Option<i64>, String)>;
    async fn images(&self) -> Result<Vec<ImageInfo>>;
    async fn remove_image(&self, id: &str, force: bool) -> Result<()>;
    async fn pull_image(&self, reference: &str) -> Result<String>;
    async fn volumes(&self) -> Result<Vec<VolumeInfo>>;
    async fn remove_volume(&self, name: &str) -> Result<()>;
    async fn networks(&self) -> Result<Vec<NetworkInfo>>;
    async fn system_info(&self) -> Result<Vec<(String, String)>>;
}

pub fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{b} B") } else { format!("{v:.1} {}", UNITS[u]) }
}
