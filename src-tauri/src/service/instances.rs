//! Instance lifecycle: create, install, start, sleep, stop.
//!
//! This is the only place that decides what a server's status *is*. The
//! runtime reports events, the wake proxy reports joins, the poller reports
//! numbers — none of them set status on their own.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{info, warn};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::registry::{InstallContext, InstallProgress, LaunchContext, ProgressSink};
use crate::game::{BuildSelector, GameRegistry};
use crate::model::{
    ConsoleLevel, ConsoleLine, Instance, InstanceState, InstanceStatus, InstanceView, Node,
    RuntimeKind,
};
use crate::net::{ports, upnp::PortForwarder, wake_proxy};
use crate::runtime::console::ConsoleParser;
use crate::runtime::process::{ProcessSupervisor, RuntimeEvent, StartRequest};
use crate::store::{settings, InstanceStore};

/// How many console lines are kept per instance. Enough to scroll back through
/// a startup, small enough that ten idle servers do not cost anything.
const CONSOLE_BUFFER: usize = 2_000;

/// Anything the UI should react to. One channel, so a new surface (the mobile
/// app, later) subscribes rather than being wired in.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AppEvent {
    Status {
        #[serde(rename = "instanceId")]
        instance_id: String,
        status: InstanceStatus,
    },
    Console {
        #[serde(rename = "instanceId")]
        instance_id: String,
        line: ConsoleLine,
    },
    Install {
        #[serde(rename = "instanceId")]
        instance_id: String,
        progress: InstallProgress,
    },
    Stats {
        #[serde(rename = "instanceId")]
        instance_id: String,
        state: Box<InstanceState>,
    },
    Players {
        #[serde(rename = "instanceId")]
        instance_id: String,
        online: Vec<String>,
    },
}

/// What the UI sends when the user creates a server.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstanceRequest {
    pub game: String,
    pub name: String,
    pub build: BuildSelector,
    #[serde(default)]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub runtime_kind: Option<RuntimeKind>,
    #[serde(default)]
    pub directory: Option<String>,
}

pub struct InstanceService {
    pub registry: GameRegistry,
    pub store: InstanceStore,
    pub processes: ProcessSupervisor,
    pub wake: wake_proxy::WakeProxyManager,
    pub forwarder: PortForwarder,
    node: RwLock<Node>,
    states: RwLock<HashMap<String, InstanceState>>,
    consoles: RwLock<HashMap<String, VecDeque<ConsoleLine>>>,
    online_players: RwLock<HashMap<String, Vec<String>>>,
    events: broadcast::Sender<AppEvent>,
}

impl InstanceService {
    pub async fn new(node: Node) -> AppResult<Arc<Self>> {
        let (events, _) = broadcast::channel(512);
        let service = Arc::new(Self {
            registry: GameRegistry::with_builtin_games(),
            store: InstanceStore::load().await?,
            processes: ProcessSupervisor::new(),
            wake: wake_proxy::WakeProxyManager::new(),
            forwarder: PortForwarder::new(),
            node: RwLock::new(node),
            states: RwLock::new(HashMap::new()),
            consoles: RwLock::new(HashMap::new()),
            online_players: RwLock::new(HashMap::new()),
            events,
        });
        Ok(service)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.events.subscribe()
    }

    fn emit(&self, event: AppEvent) {
        // A send error only means nobody is listening yet.
        let _ = self.events.send(event);
    }

    pub async fn node(&self) -> Node {
        self.node.read().await.clone()
    }

    pub async fn rename_node(&self, name: String) -> AppResult<Node> {
        let mut node = self.node.write().await;
        node.name = name;
        crate::store::nodes::save(&node)?;
        Ok(node.clone())
    }

    // -- reading -----------------------------------------------------------

    pub async fn list(&self) -> AppResult<Vec<InstanceView>> {
        let instances = self.store.list().await;
        let states = self.states.read().await;
        let address = crate::net::lan::primary_address().map(|ip| ip.to_string());

        Ok(instances
            .into_iter()
            .map(|instance| {
                let state = states.get(&instance.id).cloned().unwrap_or_default();
                let address = address.as_ref().map(|ip| {
                    let port = self
                        .registry
                        .manifest(&instance.game)
                        .map(|manifest| instance.primary_port(&manifest))
                        .unwrap_or_default();
                    format!("{ip}:{port}")
                });
                InstanceView { instance, state, address }
            })
            .collect())
    }

    pub async fn view(&self, instance_id: &str) -> AppResult<InstanceView> {
        let instance = self.store.get(instance_id).await?;
        let state = self.state_of(instance_id).await;
        Ok(InstanceView { instance, state, address: None })
    }

    pub async fn state_of(&self, instance_id: &str) -> InstanceState {
        self.states.read().await.get(instance_id).cloned().unwrap_or_default()
    }

    pub async fn console_tail(&self, instance_id: &str, limit: usize) -> Vec<ConsoleLine> {
        let consoles = self.consoles.read().await;
        let Some(buffer) = consoles.get(instance_id) else { return Vec::new() };
        buffer.iter().rev().take(limit).rev().cloned().collect()
    }

    pub async fn online_players(&self, instance_id: &str) -> Vec<String> {
        self.online_players.read().await.get(instance_id).cloned().unwrap_or_default()
    }

    // -- creating ----------------------------------------------------------

    /// Create the instance record and its directory. Installing the files is a
    /// separate step so the UI can show real progress for it.
    pub async fn create(&self, request: CreateInstanceRequest) -> AppResult<Instance> {
        let manifest = self.registry.manifest(&request.game)?;
        let name = validate_name(&request.name)?;

        let root = match request.directory {
            Some(dir) => PathBuf::from(dir),
            None => settings::load()?.instances_dir()?.join(slug(&name)),
        };
        if root.exists() && root.read_dir().map(|mut d| d.next().is_some()).unwrap_or(false) {
            return Err(AppError::new(ErrorCode::InvalidInput)
                .with("reason", "directory_not_empty")
                .with("path", root.display()));
        }
        std::fs::create_dir_all(&root)?;

        let mut instance =
            Instance::new(self.node.read().await.id.clone(), &request.game, name, request.build, root);

        if let Some(spec) = manifest.primary_port() {
            let port = match request.port {
                Some(port) => {
                    ports::validate(port)?;
                    port
                }
                None => ports::first_free_from(spec.default).unwrap_or(spec.default),
            };
            instance.ports.insert(spec.key.clone(), port);
        }
        if let Some(memory_mb) = request.memory_mb {
            instance.resources.memory_mb = memory_mb;
        }
        if let Some(runtime_kind) = request.runtime_kind {
            instance.runtime_kind = runtime_kind;
        }

        self.store.put(instance.clone()).await?;
        Ok(instance)
    }

    /// Download and lay out the server files, reporting each phase.
    pub async fn install(&self, instance_id: &str) -> AppResult<()> {
        let instance = self.store.get(instance_id).await?;
        let adapter = self.registry.get(&instance.game)?;

        let id = instance_id.to_string();
        let events = self.events.clone();
        let progress = ProgressSink::new(move |progress| {
            let _ = events.send(AppEvent::Install { instance_id: id.clone(), progress });
        });

        let report = adapter
            .install(InstallContext {
                root: &instance.path,
                build: &instance.build,
                java: None,
                progress: progress.clone(),
            })
            .await
            .inspect_err(|error| {
                progress.report(InstallProgress::Failed { code: error.code });
            })?;

        // Seed the game's own config with the port we allocated, before the
        // first launch writes its defaults.
        let manifest = self.registry.manifest(&instance.game)?;
        if instance.game == crate::game::MINECRAFT {
            crate::game::minecraft::launch::write_initial_properties(
                &instance.path,
                instance.primary_port(&manifest),
            )?;
        }

        self.store
            .update(instance_id, |instance| {
                if let Some(build) = report.resolved_build.clone() {
                    instance.build = build;
                }
                instance.install = Some(report.clone());
            })
            .await?;

        progress.report(InstallProgress::Ready);
        Ok(())
    }

    // -- lifecycle ---------------------------------------------------------

    pub async fn start(self: &Arc<Self>, instance_id: &str) -> AppResult<()> {
        let instance = self.store.get(instance_id).await?;

        if self.processes.is_running(instance_id).await {
            return Err(AppError::new(ErrorCode::InstanceAlreadyRunning).with("instance", instance_id));
        }

        // Hand the port back before trying to bind it as the server.
        if self.wake.stop(instance_id).await {
            wake_proxy::wait_for_port_release(instance_id, instance.primary_port(&self.registry.manifest(&instance.game)?)).await;
        }

        let manifest = self.registry.manifest(&instance.game)?;
        let adapter = self.registry.get(&instance.game)?;
        let install = instance.install.clone().unwrap_or_default();

        let plan = adapter.launch_plan(LaunchContext {
            root: &instance.path,
            build: &instance.build,
            install: &install,
            java: instance.resources.runtime_path.as_deref().map(PathBuf::from),
            memory_mb: instance.resources.memory_mb,
            extra_args: &instance.resources.extra_args,
        })?;

        let parser = Arc::new(ConsoleParser::new(&manifest.console)?);
        let (console_tx, console_rx) = mpsc::channel(256);
        let (events_tx, events_rx) = mpsc::channel(256);

        self.set_status(instance_id, InstanceStatus::Starting).await;
        self.push_console(
            instance_id,
            ConsoleLine {
                timestamp: Utc::now(),
                level: ConsoleLevel::System,
                message: format!("Starting {}…", instance.name),
            },
        )
        .await;

        self.clone().consume_console(instance.id.clone(), console_rx);
        self.clone().consume_events(events_rx);

        self.processes
            .start(
                StartRequest {
                    instance_id: instance.id.clone(),
                    instance_name: instance.name.clone(),
                    plan,
                    runtime_kind: instance.runtime_kind,
                    parser,
                    stop_command: manifest.console.stop_command.clone(),
                    container_image: None,
                    port: instance.primary_port(&manifest),
                },
                console_tx,
                events_tx,
            )
            .await
            .inspect_err(|_| {
                let service = self.clone();
                let id = instance_id.to_string();
                tokio::spawn(async move {
                    service.set_status(&id, InstanceStatus::Offline).await;
                });
            })?;

        self.store
            .update(instance_id, |instance| {
                instance.last_started_at = Some(Utc::now());
                instance.automation.restart_attempts_remaining = 3;
            })
            .await?;

        Ok(())
    }

    pub async fn stop(&self, instance_id: &str) -> AppResult<()> {
        let instance = self.store.get(instance_id).await?;
        let manifest = self.registry.manifest(&instance.game)?;

        self.set_status(instance_id, InstanceStatus::Stopping).await;
        self.processes.stop(instance_id, &manifest.console.stop_command).await
    }

    pub async fn restart(self: &Arc<Self>, instance_id: &str) -> AppResult<()> {
        if self.processes.is_running(instance_id).await {
            self.stop(instance_id).await?;
            for _ in 0..60 {
                if !self.processes.is_running(instance_id).await {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        self.start(instance_id).await
    }

    pub async fn send_command(&self, instance_id: &str, command: &str) -> AppResult<()> {
        self.push_console(
            instance_id,
            ConsoleLine {
                timestamp: Utc::now(),
                level: ConsoleLevel::System,
                message: format!("> {command}"),
            },
        )
        .await;
        self.processes.send_command(instance_id, command).await
    }

    /// Put a stopped instance to sleep: hold its port and wake it on a join.
    pub async fn sleep(self: &Arc<Self>, instance_id: &str) -> AppResult<()> {
        let instance = self.store.get(instance_id).await?;
        let manifest = self.registry.manifest(&instance.game)?;

        let Some(spec) = manifest.wake.clone() else {
            return Err(AppError::new(ErrorCode::GameNotSupported).with("game", &instance.game));
        };

        let service = self.clone();
        let wake_id = instance_id.to_string();
        let log_service = self.clone();

        let hooks = wake_proxy::WakeHooks {
            on_wake: Arc::new(move |id| {
                let service = service.clone();
                tokio::spawn(async move {
                    if let Err(error) = service.start(&id).await {
                        warn!(instance_id = %id, %error, "wake-on-join could not start the server");
                    }
                });
            }),
            on_log: Arc::new(move |id, level, message| {
                let service = log_service.clone();
                let line = ConsoleLine {
                    timestamp: Utc::now(),
                    level: if level == "WARN" { ConsoleLevel::Warn } else { ConsoleLevel::System },
                    message,
                };
                tokio::spawn(async move {
                    service.push_console(&id, line).await;
                });
            }),
        };

        // Loopback unless the user opened this node to the LAN: the wake
        // listener stands in for the server, and it must not be reachable more
        // widely than the server itself was.
        let bind = if settings::load()?.listen_on_lan {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        };

        self.wake
            .spawn(
                hooks,
                wake_id,
                bind,
                instance.primary_port(&manifest),
                spec,
                instance.name.clone(),
            )
            .await;

        self.set_status(instance_id, InstanceStatus::Sleeping).await;
        Ok(())
    }

    pub async fn wake(self: &Arc<Self>, instance_id: &str) -> AppResult<()> {
        self.wake.stop(instance_id).await;
        self.start(instance_id).await
    }

    pub async fn delete(&self, instance_id: &str, delete_files: bool) -> AppResult<()> {
        if self.processes.is_running(instance_id).await {
            return Err(AppError::new(ErrorCode::InstanceAlreadyRunning).with("instance", instance_id));
        }
        self.wake.stop(instance_id).await;
        self.forwarder.release(instance_id).await;
        self.states.write().await.remove(instance_id);
        self.consoles.write().await.remove(instance_id);
        self.store.remove(instance_id, delete_files).await
    }

    // -- event plumbing ----------------------------------------------------

    /// Each running server gets its own console channel, so every line that
    /// arrives here belongs to `instance_id` and needs no tagging on the wire.
    fn consume_console(self: Arc<Self>, instance_id: String, mut rx: mpsc::Receiver<ConsoleLine>) {
        tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                self.push_console(&instance_id, line).await;
            }
        });
    }

    fn consume_events(self: Arc<Self>, mut rx: mpsc::Receiver<RuntimeEvent>) {
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    RuntimeEvent::Status(id, status) => self.set_status(&id, status).await,
                    RuntimeEvent::PlayerJoined(id, name) => {
                        let mut players = self.online_players.write().await;
                        let list = players.entry(id.clone()).or_default();
                        if !list.contains(&name) {
                            list.push(name);
                        }
                        let online = list.clone();
                        drop(players);
                        self.mark_player_activity(&id).await;
                        self.emit(AppEvent::Players { instance_id: id, online });
                    }
                    RuntimeEvent::PlayerLeft(id, name) => {
                        let mut players = self.online_players.write().await;
                        let list = players.entry(id.clone()).or_default();
                        list.retain(|player| player != &name);
                        let online = list.clone();
                        drop(players);
                        self.mark_player_activity(&id).await;
                        self.emit(AppEvent::Players { instance_id: id, online });
                    }
                    RuntimeEvent::TickRate(id, rate) => {
                        let mut states = self.states.write().await;
                        states.entry(id).or_default().tick_rate = Some(rate);
                    }
                    RuntimeEvent::Exited { instance_id, expected, code } => {
                        self.on_exit(&instance_id, expected, code).await;
                    }
                }
            }
        });
    }

    async fn on_exit(self: &Arc<Self>, instance_id: &str, expected: bool, code: Option<i32>) {
        self.online_players.write().await.remove(instance_id);

        let instance = match self.store.get(instance_id).await {
            Ok(instance) => instance,
            Err(_) => return,
        };

        if !expected {
            info!(instance_id, ?code, "server exited unexpectedly");
            self.set_status(instance_id, InstanceStatus::Crashed).await;
            self.push_console(
                instance_id,
                ConsoleLine {
                    timestamp: Utc::now(),
                    level: ConsoleLevel::Error,
                    message: match code {
                        Some(code) => format!("Server stopped unexpectedly (exit code {code})."),
                        None => "Server stopped unexpectedly.".to_string(),
                    },
                },
            )
            .await;

            if instance.automation.restart_after_crash
                && instance.automation.restart_attempts_remaining > 0
            {
                let service = self.clone();
                let id = instance_id.to_string();
                let _ = self
                    .store
                    .update(instance_id, |instance| {
                        instance.automation.restart_attempts_remaining -= 1;
                    })
                    .await;
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    let _ = service.start(&id).await;
                });
            }
            return;
        }

        self.set_status(instance_id, InstanceStatus::Offline).await;
        self.push_console(
            instance_id,
            ConsoleLine {
                timestamp: Utc::now(),
                level: ConsoleLevel::System,
                message: "Server stopped.".to_string(),
            },
        )
        .await;

        // A clean stop is exactly when wake-on-join takes over.
        if instance.automation.wake_on_join {
            let service = self.clone();
            let id = instance_id.to_string();
            let port = self
                .registry
                .manifest(&instance.game)
                .map(|manifest| instance.primary_port(&manifest))
                .unwrap_or_default();
            tokio::spawn(async move {
                wake_proxy::wait_for_port_release(&id, port).await;
                if let Err(error) = service.sleep(&id).await {
                    warn!(instance_id = %id, %error, "could not put the server to sleep");
                }
            });
        }
    }

    /// The one place status is written.
    pub async fn set_status(&self, instance_id: &str, status: InstanceStatus) {
        {
            let mut states = self.states.write().await;
            let state = states.entry(instance_id.to_string()).or_default();
            state.status = status;
            if status == InstanceStatus::Online && state.last_player_seen_at.is_none() {
                state.last_player_seen_at = Some(Utc::now());
            }
            if !status.expects_process() {
                state.uptime_seconds = None;
                state.cpu_percent = 0.0;
                state.memory_used_mb = 0;
                state.tick_rate = None;
            }
        }
        self.emit(AppEvent::Status { instance_id: instance_id.to_string(), status });
    }

    /// Store freshly measured numbers and tell the UI. Status is not part of
    /// this: only `set_status` writes it.
    pub async fn publish_state(&self, instance_id: &str, state: InstanceState) {
        {
            let mut states = self.states.write().await;
            let stored = states.entry(instance_id.to_string()).or_default();
            let status = stored.status;
            *stored = state.clone();
            stored.status = status;
        }
        self.emit(AppEvent::Stats {
            instance_id: instance_id.to_string(),
            state: Box::new(state),
        });
    }

    async fn mark_player_activity(&self, instance_id: &str) {
        let mut states = self.states.write().await;
        states.entry(instance_id.to_string()).or_default().last_player_seen_at = Some(Utc::now());
    }

    async fn push_console(&self, instance_id: &str, line: ConsoleLine) {
        {
            let mut consoles = self.consoles.write().await;
            let buffer = consoles.entry(instance_id.to_string()).or_default();
            if buffer.len() == CONSOLE_BUFFER {
                buffer.pop_front();
            }
            buffer.push_back(line.clone());
        }
        self.emit(AppEvent::Console { instance_id: instance_id.to_string(), line });
    }
}

/// A server name becomes a directory name, so it has to be boring.
fn validate_name(name: &str) -> AppResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(AppError::new(ErrorCode::InvalidInput).with("field", "name"));
    }
    if trimmed.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) || trimmed.contains("..") {
        return Err(AppError::new(ErrorCode::InvalidInput).with("field", "name"));
    }
    Ok(trimmed.to_string())
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in name.trim().to_lowercase().chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("server");
    }
    out.truncate(48);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_would_escape_the_instances_directory_are_refused() {
        for name in ["../evil", "a/b", "", "  ", "C:name", &"x".repeat(65)] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
        assert_eq!(validate_name("  Survival  ").expect("ok"), "Survival");
    }

    #[test]
    fn slugs_are_directory_safe() {
        assert_eq!(slug("My SMP Server"), "my-smp-server");
        assert_eq!(slug("¡¡¡!!!"), "server");
        assert_eq!(slug("Sky   Block 2"), "sky-block-2");
    }
}
