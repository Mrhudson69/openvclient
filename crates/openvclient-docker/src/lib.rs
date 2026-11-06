//! Docker Engine connector (bollard). Works with Docker Desktop, Colima,
//! OrbStack, Podman's Docker-compatible socket and remote TCP daemons.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bollard::Docker;
use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::query_parameters::*;
use futures::StreamExt;
use openvclient_core::containers::*;
use openvclient_core::{Category, ConnectParams, Driver, DriverInfo, Error, FieldKind, FieldSpec, Result, ServerInfo, ServiceSession, SessionHandle};

pub struct DockerDriver {
    info: DriverInfo,
}

impl Default for DockerDriver {
    fn default() -> Self {
        Self {
            info: DriverInfo {
                id: "docker",
                name: "Docker",
                category: Category::Containers,
                description: "Docker Engine API: local socket or remote daemon",
                badge: "DK",
                fields: vec![
                    FieldSpec::text("endpoint", "Endpoint")
                        .placeholder("auto (DOCKER_HOST or local socket)")
                        .help("unix:///var/run/docker.sock, npipe:////./pipe/docker_engine or tcp://host:2375. Empty = auto-detect."),
                    FieldSpec::text("timeout", "Request timeout (s)").kind(FieldKind::Number).default("120").advanced(),
                ],
            },
        }
    }
}

fn d_err(e: bollard::errors::Error) -> Error {
    use bollard::errors::Error as E;
    match e {
        E::DockerResponseServerError { status_code, message } => {
            if status_code == 404 {
                Error::invalid(message)
            } else {
                Error::server(format!("{message} (HTTP {status_code})"))
            }
        }
        other => {
            let m = other.to_string();
            let lower = m.to_ascii_lowercase();
            // Windows reports a missing named pipe as "client error (Connect)" or
            // "The system cannot find the file specified" (os error 2).
            if ["connect", "no such file", "cannot find the file", "refused", "socket", "pipe"].iter().any(|k| lower.contains(k)) {
                let hint = if cfg!(windows) { "Is Docker Desktop running? It serves the Docker API on \\\\.\\pipe\\docker_engine." } else { "Is the Docker daemon running?" };
                Error::connection(format!("{m}. {hint}"))
            } else {
                Error::other(m)
            }
        }
    }
}

/// Docker Desktop on macOS moved its socket under the user's home.
fn local_socket_candidates() -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(h) = std::env::var("HOME") {
        v.push(format!("{h}/.docker/run/docker.sock"));
        v.push(format!("{h}/.colima/default/docker.sock"));
        v.push(format!("{h}/.orbstack/run/docker.sock"));
    }
    v.push("/var/run/docker.sock".into());
    v
}

#[async_trait]
impl Driver for DockerDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }

    async fn connect(&self, p: ConnectParams) -> Result<SessionHandle> {
        let timeout: u64 = p.get_or("timeout", "120").parse().unwrap_or(120);
        let docker = match p.get("endpoint") {
            Some(ep) if ep.starts_with("tcp://") || ep.starts_with("http://") => Docker::connect_with_http(ep, timeout, bollard::API_DEFAULT_VERSION).map_err(d_err)?,
            Some(ep) => Docker::connect_with_socket(ep, timeout, bollard::API_DEFAULT_VERSION).map_err(d_err)?,
            None if std::env::var("DOCKER_HOST").is_ok() => Docker::connect_with_defaults().map_err(d_err)?,
            None => {
                let sock = local_socket_candidates().into_iter().find(|s| std::path::Path::new(s).exists());
                match sock {
                    Some(s) => Docker::connect_with_socket(&s, timeout, bollard::API_DEFAULT_VERSION).map_err(d_err)?,
                    None => Docker::connect_with_local_defaults().map_err(d_err)?,
                }
            }
        };
        let v = docker.version().await.map_err(d_err)?;
        let info = ServerInfo {
            product: format!("Docker Engine {}", v.version.clone().unwrap_or_default()),
            version: v.version.clone().unwrap_or_default(),
            details: vec![("API".into(), v.api_version.unwrap_or_default()), ("OS/Arch".into(), format!("{}/{}", v.os.unwrap_or_default(), v.arch.unwrap_or_default()))],
        };
        Ok(SessionHandle::Containers(Arc::new(DockerSession { docker, info, read_only: p.read_only })))
    }
}

struct DockerSession {
    docker: Docker,
    info: ServerInfo,
    read_only: bool,
}

impl DockerSession {
    fn guard(&self) -> Result<()> {
        if self.read_only { Err(Error::invalid("Connection is read-only")) } else { Ok(()) }
    }
}

fn log_text(chunk: LogOutput) -> String {
    match chunk {
        LogOutput::StdErr { message } | LogOutput::StdOut { message } | LogOutput::Console { message } | LogOutput::StdIn { message } => String::from_utf8_lossy(&message).into_owned(),
    }
}

#[async_trait]
impl ServiceSession for DockerSession {
    fn server_info(&self) -> ServerInfo {
        self.info.clone()
    }

    async fn ping(&self) -> Result<Duration> {
        let t = Instant::now();
        self.docker.ping().await.map_err(d_err)?;
        Ok(t.elapsed())
    }

    async fn close(&self) {}
}

#[async_trait]
impl ContainerSession for DockerSession {
    async fn containers(&self, all: bool) -> Result<Vec<ContainerInfo>> {
        let list = self.docker.list_containers(Some(ListContainersOptionsBuilder::default().all(all).build())).await.map_err(d_err)?;
        let mut out: Vec<ContainerInfo> = list
            .into_iter()
            .map(|c| {
                let mut ports: Vec<String> = c
                    .ports
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| match p.public_port {
                        Some(pp) => format!("{}:{}→{}", p.ip.unwrap_or_default(), pp, p.private_port),
                        None => p.private_port.to_string(),
                    })
                    .collect();
                ports.sort();
                ports.dedup();
                ContainerInfo {
                    id: c.id.unwrap_or_default(),
                    name: c.names.unwrap_or_default().first().map(|n| n.trim_start_matches('/').to_string()).unwrap_or_default(),
                    image: c.image.unwrap_or_default(),
                    state: c.state.map(|s| s.to_string()).unwrap_or_default(),
                    status: c.status.unwrap_or_default(),
                    ports: ports.join(", "),
                    created: c.created.unwrap_or(0),
                    compose_project: c.labels.and_then(|l| l.get("com.docker.compose.project").cloned()),
                }
            })
            .collect();
        out.sort_by_key(|a| (a.compose_project.clone(), a.name.clone()));
        Ok(out)
    }

    async fn container_action(&self, id: &str, action: ContainerAction) -> Result<()> {
        self.guard()?;
        let d = &self.docker;
        match action {
            ContainerAction::Start => d.start_container(id, None::<StartContainerOptions>).await,
            ContainerAction::Stop => d.stop_container(id, None::<StopContainerOptions>).await,
            ContainerAction::Restart => d.restart_container(id, None::<RestartContainerOptions>).await,
            ContainerAction::Pause => d.pause_container(id).await,
            ContainerAction::Unpause => d.unpause_container(id).await,
            ContainerAction::Kill => d.kill_container(id, None::<KillContainerOptions>).await,
            ContainerAction::Remove { force } => d.remove_container(id, Some(RemoveContainerOptionsBuilder::default().force(force).build())).await,
        }
        .map_err(d_err)
    }

    async fn logs(&self, id: &str, tail: usize, timestamps: bool) -> Result<String> {
        let opts = LogsOptionsBuilder::default().stdout(true).stderr(true).timestamps(timestamps).tail(&tail.to_string()).build();
        let mut stream = self.docker.logs(id, Some(opts));
        let mut out = String::new();
        while let Some(chunk) = stream.next().await {
            out.push_str(&log_text(chunk.map_err(d_err)?));
        }
        Ok(out)
    }

    async fn inspect(&self, id: &str) -> Result<String> {
        let c = self.docker.inspect_container(id, None::<InspectContainerOptions>).await.map_err(d_err)?;
        serde_json_pretty(&c)
    }

    async fn stats(&self, id: &str) -> Result<ContainerStats> {
        let mut s = self.docker.stats(id, Some(StatsOptionsBuilder::default().stream(false).one_shot(false).build()));
        let st = s.next().await.ok_or_else(|| Error::other("No stats returned"))?.map_err(d_err)?;
        let cpu = st.cpu_stats.as_ref();
        let pre = st.precpu_stats.as_ref();
        let total = |c: Option<&bollard::models::ContainerCpuStats>| c.and_then(|c| c.cpu_usage.as_ref()).and_then(|u| u.total_usage).unwrap_or(0);
        let sys = |c: Option<&bollard::models::ContainerCpuStats>| c.and_then(|c| c.system_cpu_usage).unwrap_or(0);
        let cpu_delta = total(cpu).saturating_sub(total(pre)) as f64;
        let sys_delta = sys(cpu).saturating_sub(sys(pre)) as f64;
        let cpus = cpu.and_then(|c| c.online_cpus).unwrap_or(1) as f64;
        let cpu_percent = if sys_delta > 0.0 { cpu_delta / sys_delta * cpus * 100.0 } else { 0.0 };
        let mem = st.memory_stats.as_ref();
        // Match `docker stats`: exclude reclaimable page cache.
        let cache = mem.and_then(|m| m.stats.as_ref()).and_then(|s| s.get("inactive_file").or(s.get("cache")).copied()).unwrap_or(0);
        let (rx, tx) = st.networks.as_ref().map(|n| n.values().fold((0, 0), |(r, t), v| (r + v.rx_bytes.unwrap_or(0), t + v.tx_bytes.unwrap_or(0)))).unwrap_or((0, 0));
        let (br, bw) = st
            .blkio_stats
            .as_ref()
            .and_then(|b| b.io_service_bytes_recursive.as_ref())
            .map(|v| {
                v.iter().fold((0u64, 0u64), |(r, w), e| match e.op.as_deref().map(str::to_ascii_lowercase).as_deref() {
                    Some("read") => (r + e.value.unwrap_or(0), w),
                    Some("write") => (r, w + e.value.unwrap_or(0)),
                    _ => (r, w),
                })
            })
            .unwrap_or((0, 0));
        Ok(ContainerStats {
            cpu_percent,
            memory_usage: mem.and_then(|m| m.usage).unwrap_or(0).saturating_sub(cache),
            memory_limit: mem.and_then(|m| m.limit).unwrap_or(0),
            net_rx: rx,
            net_tx: tx,
            block_read: br,
            block_write: bw,
            pids: st.pids_stats.as_ref().and_then(|p| p.current).unwrap_or(0),
        })
    }

    async fn exec(&self, id: &str, cmd: Vec<String>) -> Result<(Option<i64>, String)> {
        self.guard()?;
        if cmd.is_empty() {
            return Err(Error::invalid("Enter a command"));
        }
        let exec = self.docker.create_exec(id, CreateExecOptions { attach_stdout: Some(true), attach_stderr: Some(true), cmd: Some(cmd), ..Default::default() }).await.map_err(d_err)?;
        let mut out = String::new();
        if let StartExecResults::Attached { mut output, .. } = self.docker.start_exec(&exec.id, None).await.map_err(d_err)? {
            while let Some(chunk) = output.next().await {
                out.push_str(&log_text(chunk.map_err(d_err)?));
                if out.len() > 4 << 20 {
                    out.push_str("\n… output truncated at 4 MiB");
                    break;
                }
            }
        }
        let code = self.docker.inspect_exec(&exec.id).await.map_err(d_err)?.exit_code;
        Ok((code, out))
    }

    async fn images(&self) -> Result<Vec<ImageInfo>> {
        let list = self.docker.list_images(Some(ListImagesOptionsBuilder::default().all(false).build())).await.map_err(d_err)?;
        let mut v: Vec<ImageInfo> = list.into_iter().map(|i| ImageInfo { id: i.id, tags: i.repo_tags, size: i.size, created: i.created, containers: i.containers }).collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.created));
        Ok(v)
    }

    async fn remove_image(&self, id: &str, force: bool) -> Result<()> {
        self.guard()?;
        self.docker.remove_image(id, Some(RemoveImageOptionsBuilder::default().force(force).build()), None).await.map_err(d_err)?;
        Ok(())
    }

    async fn pull_image(&self, reference: &str) -> Result<String> {
        self.guard()?;
        let (image, tag) = match reference.rsplit_once(':').filter(|(_, t)| !t.contains('/')) {
            Some((i, t)) => (i.to_string(), t.to_string()),
            None => (reference.to_string(), "latest".to_string()),
        };
        let opts = CreateImageOptionsBuilder::default().from_image(&image).tag(&tag).build();
        let mut stream = self.docker.create_image(Some(opts), None, None);
        let mut last = String::new();
        while let Some(item) = stream.next().await {
            let info = item.map_err(d_err)?;
            if let Some(s) = info.status {
                last = s;
            }
        }
        Ok(format!("{image}:{tag}: {last}"))
    }

    async fn volumes(&self) -> Result<Vec<VolumeInfo>> {
        let r = self.docker.list_volumes(None::<ListVolumesOptions>).await.map_err(d_err)?;
        let mut v: Vec<VolumeInfo> =
            r.volumes.unwrap_or_default().into_iter().map(|v| VolumeInfo { name: v.name, driver: v.driver, mountpoint: v.mountpoint, created: v.created_at.map(|d| d.to_string()) }).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(v)
    }

    async fn remove_volume(&self, name: &str) -> Result<()> {
        self.guard()?;
        self.docker.remove_volume(name, None::<RemoveVolumeOptions>).await.map_err(d_err)
    }

    async fn networks(&self) -> Result<Vec<NetworkInfo>> {
        let list = self.docker.list_networks(None::<ListNetworksOptions>).await.map_err(d_err)?;
        let mut v: Vec<NetworkInfo> = list
            .into_iter()
            .map(|n| NetworkInfo { id: n.id.unwrap_or_default(), name: n.name.unwrap_or_default(), driver: n.driver.unwrap_or_default(), scope: n.scope.unwrap_or_default() })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(v)
    }

    async fn system_info(&self) -> Result<Vec<(String, String)>> {
        let i = self.docker.info().await.map_err(d_err)?;
        let o = |x: Option<String>| x.unwrap_or_default();
        let n = |x: Option<i64>| x.map(|v| v.to_string()).unwrap_or_default();
        Ok(vec![
            ("Name".into(), o(i.name)),
            ("Server version".into(), o(i.server_version)),
            ("Operating system".into(), o(i.operating_system)),
            ("Kernel".into(), o(i.kernel_version)),
            ("Architecture".into(), o(i.architecture)),
            ("CPUs".into(), n(i.ncpu)),
            ("Memory".into(), i.mem_total.map(|m| human_bytes(m as u64)).unwrap_or_default()),
            ("Storage driver".into(), o(i.driver)),
            ("Containers".into(), n(i.containers)),
            ("Running".into(), n(i.containers_running)),
            ("Paused".into(), n(i.containers_paused)),
            ("Stopped".into(), n(i.containers_stopped)),
            ("Images".into(), n(i.images)),
            ("Docker root".into(), o(i.docker_root_dir)),
        ])
    }
}

fn serde_json_pretty<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string_pretty(v).map_err(|e| Error::other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn live_docker() {
        let p = ConnectParams { values: Default::default(), secret: None, read_only: false, notifier: None };
        let Ok(h) = DockerDriver::default().connect(p).await else {
            eprintln!("skipping: no Docker daemon");
            return;
        };
        let d = h.containers().unwrap().clone();
        assert!(d.server_info().product.starts_with("Docker Engine"));
        let list = d.containers(true).await.unwrap();
        let Some(c) = list.iter().find(|c| c.name == "openvclient-test-redis") else {
            eprintln!("skipping container checks: start `openvclient-test-redis` first");
            return;
        };
        assert_eq!(c.state, "running");
        assert!(c.ports.contains("56379"), "{}", c.ports);
        let logs = d.logs(&c.id, 50, false).await.unwrap();
        assert!(logs.contains("Ready to accept connections"), "{logs}");
        let st = d.stats(&c.id).await.unwrap();
        assert!(st.memory_usage > 0 && st.pids > 0, "{st:?}");
        let (code, out) = d.exec(&c.id, vec!["redis-cli".into(), "PING".into()]).await.unwrap();
        assert_eq!((code, out.trim()), (Some(0), "PONG"));
        d.container_action(&c.id, ContainerAction::Pause).await.unwrap();
        assert_eq!(d.containers(true).await.unwrap().iter().find(|x| x.id == c.id).unwrap().state, "paused");
        d.container_action(&c.id, ContainerAction::Unpause).await.unwrap();
        assert!(d.inspect(&c.id).await.unwrap().contains("\"Name\": \"/openvclient-test-redis\""));
        assert!(!d.images().await.unwrap().is_empty());
        assert!(!d.networks().await.unwrap().is_empty());
        d.volumes().await.unwrap();
        assert!(d.system_info().await.unwrap().iter().any(|(k, v)| k == "CPUs" && !v.is_empty()));
    }
}
