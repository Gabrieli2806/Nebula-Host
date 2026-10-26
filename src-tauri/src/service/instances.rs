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
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tracing::{info, warn};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::registry::{InstallContext, InstallProgress, LaunchContext, ProgressSink};
use crate::game::{BuildSelector, GameRegistry};
use crate::model::{
    ConsoleLevel, ConsoleLine, Instance, InstanceState, InstanceStatus, InstanceView, Node,
    RuntimeKind,
};
use crate::net::firewall::{self, FirewallStatus};
use crate::net::{ports, upnp::PortForwarder, wake_proxy};
use crate::runtime::console::ConsoleParser;
use crate::runtime::process::{ProcessSupervisor, RuntimeEvent, StartRequest};
use crate::service::completion::CachedCommands;
use crate::service::files::DiskUsage;
use crate::service::import::ImportProgress;
use crate::store::{settings, InstanceStore};

/// How long a disk-usage answer is reused before the folder is walked again.
const DISK_USAGE_TTL: chrono::TimeDelta = chrono::TimeDelta::seconds(60);

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
    /// A drag-and-drop import moving bytes. Throttled; the last one has
    /// `done: true`.
    Import {
        #[serde(rename = "instanceId")]
        instance_id: String,
        progress: ImportProgress,
    },
    /// A Java runtime download moving along. Not tied to one server: two
    /// servers on the same version share the download.
    JavaDownload {
        major: u32,
        progress: crate::runtime::java::JavaDownloadProgress,
    },
    /// The queue of addon changes waiting for this server to stop changed
    /// (something was queued, cancelled, applied or failed).
    PendingAddons {
        #[serde(rename = "instanceId")]
        instance_id: String,
        pending: Vec<crate::service::pending::PendingAddonChange>,
    },
    /// A world import moving along: `backup`, `unpack` (bytes), `place`,
    /// `done`. Throttled.
    WorldImport {
        #[serde(rename = "instanceId")]
        instance_id: String,
        progress: crate::service::world::WorldImportProgress,
    },
    /// A server transfer between machines moving along (export, upload,
    /// verify, adopt). Throttled; the last one has `done: true`.
    Transfer {
        #[serde(rename = "instanceId")]
        instance_id: String,
        progress: crate::service::transfer::TransferProgress,
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
    /// The user ticked "I accept" for the game's agreement (the Minecraft
    /// EULA) in the create dialog. Only the UI ever sets this; MCP and other
    /// callers leave it false and the server waits for the user.
    #[serde(default)]
    pub accept_agreement: bool,
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
    /// Per instance: player name → account UUID, as announced on join.
    identities: RwLock<HashMap<String, HashMap<String, String>>>,
    /// Per instance: the server's own command list, until restart.
    completions: RwLock<HashMap<String, CachedCommands>>,
    /// Per instance: serialises its `help` captures (one console, one at a
    /// time) without holding up the other servers.
    completion_fetch: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    disk_usage: RwLock<HashMap<String, DiskUsage>>,
    /// Per instance: Modrinth project id → installed file name.
    pub(crate) addon_index: RwLock<HashMap<String, HashMap<String, String>>>,
    pub(crate) events: broadcast::Sender<AppEvent>,
    /// Serialises applying queued addon changes, so a stop and the start
    /// right after it (a restart) never move the same jars twice.
    pub(crate) pending_lock: Mutex<()>,
    /// Servers being exported for a transfer: they must not start until the
    /// transfer finishes or is abandoned, or the copy would miss that play.
    pub(crate) transfer_locks: Mutex<HashMap<String, crate::service::transfer::ExportLock>>,
    /// Copies adopted here whose source has not confirmed yet: what to put
    /// back if the transfer is called off.
    pub(crate) adoptions: Mutex<HashMap<String, crate::service::transfer::Adoption>>,
    /// Tests point the folders a transfer writes to somewhere temporary.
    data_root: std::sync::RwLock<Option<PathBuf>>,
}

impl InstanceService {
    pub async fn new(node: Node) -> AppResult<Arc<Self>> {
        Self::with_store(node, InstanceStore::load().await?).await
    }

    /// A service over a given store (tests run two "machines" in one process).
    pub async fn with_store(node: Node, store: InstanceStore) -> AppResult<Arc<Self>> {
        // Records written when every node was called "local" follow the node
        // to its unique id, so a transfer can tell machines apart.
        if node.id != Node::LEGACY_LOCAL_ID {
            let moved = store.migrate_node_id(Node::LEGACY_LOCAL_ID, &node.id).await?;
            if moved > 0 {
                info!(moved, node_id = %node.id, "instance records moved to the unique node id");
            }
        }
        let (events, _) = broadcast::channel(512);
        let service = Arc::new(Self {
            registry: GameRegistry::with_builtin_games(),
            store,
            processes: ProcessSupervisor::new(),
            wake: wake_proxy::WakeProxyManager::new(),
            forwarder: PortForwarder::new(),
            node: RwLock::new(node),
            states: RwLock::new(HashMap::new()),
            consoles: RwLock::new(HashMap::new()),
            online_players: RwLock::new(HashMap::new()),
            identities: RwLock::new(HashMap::new()),
            completions: RwLock::new(HashMap::new()),
            completion_fetch: Mutex::new(HashMap::new()),
            disk_usage: RwLock::new(HashMap::new()),
            addon_index: RwLock::new(HashMap::new()),
            events,
            pending_lock: Mutex::new(()),
            transfer_locks: Mutex::new(HashMap::new()),
            adoptions: Mutex::new(HashMap::new()),
            data_root: std::sync::RwLock::new(None),
        });
        Ok(service)
    }

    /// Keep exports and adopted servers under `root` instead of the app's
    /// folders. For tests that run two machines in one process.
    pub fn set_data_root(&self, root: PathBuf) {
        if let Ok(mut slot) = self.data_root.write() {
            *slot = Some(root);
        }
    }

    pub(crate) fn data_root_override(&self) -> Option<PathBuf> {
        self.data_root.read().ok().and_then(|slot| slot.clone())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.events.subscribe()
    }

    pub(crate) fn emit(&self, event: AppEvent) {
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

    /// Record a newly paired device on the node (public info only).
    pub async fn add_paired_device(&self, device: crate::model::PairedDevice) -> AppResult<Node> {
        let mut node = self.node.write().await;
        node.paired_devices.retain(|d| d.id != device.id);
        node.paired_devices.push(device);
        crate::store::nodes::save(&node)?;
        Ok(node.clone())
    }

    /// Forget a paired device: its token stops working at once.
    pub async fn remove_paired_device(&self, device_id: &str) -> AppResult<Node> {
        let mut node = self.node.write().await;
        node.paired_devices.retain(|d| d.id != device_id);
        crate::store::nodes::save(&node)?;
        let mut creds = crate::store::devices::load()?;
        creds.retain(|c| c.device_id != device_id);
        crate::store::devices::save(&creds)?;
        Ok(node.clone())
    }

    // -- reading -----------------------------------------------------------

    pub async fn list(&self) -> AppResult<Vec<InstanceView>> {
        let instances = self.store.list().await;
        let states = self.states.read().await;
        let address = crate::net::lan::primary_address().map(|ip| ip.to_string());
        let external = crate::net::upnp::cached_external_address();
        let mut forwarded = HashMap::new();
        for instance in &instances {
            if let Some(port) = self.forwarder.forwarded_port(&instance.id).await {
                forwarded.insert(instance.id.clone(), port);
            }
        }

        Ok(instances
            .into_iter()
            .map(|instance| {
                let public_address = external
                    .zip(forwarded.get(&instance.id))
                    .map(|(ip, port)| format!("{ip}:{port}"));
                let state = states.get(&instance.id).cloned().unwrap_or_default();
                let address = address.as_ref().map(|ip| {
                    let port = self
                        .registry
                        .manifest(&instance.game)
                        .map(|manifest| instance.primary_port(&manifest))
                        .unwrap_or_default();
                    format!("{ip}:{port}")
                });
                let icon_spec = self.registry.manifest(&instance.game).ok().and_then(|m| m.server_icon);
                let icon_url = crate::service::icon::icon_data_url(&instance.path, icon_spec.as_ref());
                InstanceView { instance, state, address, public_address, icon_url }
            })
            .collect())
    }

    pub async fn view(&self, instance_id: &str) -> AppResult<InstanceView> {
        let instance = self.store.get(instance_id).await?;
        let state = self.state_of(instance_id).await;
        let icon_spec = self.registry.manifest(&instance.game).ok().and_then(|m| m.server_icon);
        let icon_url = crate::service::icon::icon_data_url(&instance.path, icon_spec.as_ref());
        Ok(InstanceView { instance, state, address: None, public_address: None, icon_url })
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

    /// Player name → UUID for this instance: what the game's own files say
    /// (lists, name cache), overlaid with what the server announced this
    /// session. Names are as the game spells them; compare case-insensitively.
    pub async fn player_identities(&self, instance_id: &str) -> AppResult<HashMap<String, String>> {
        let instance = self.store.get(instance_id).await?;
        let manifest = self.registry.manifest(&instance.game)?;
        let root = instance.path.clone();
        let source = manifest.players.clone();
        let mut out = tokio::task::spawn_blocking(move || crate::service::players::read_identities(&root, &source))
            .await
            .map_err(AppError::unexpected)?;
        if let Some(seen) = self.identities.read().await.get(instance_id) {
            out.extend(seen.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        Ok(out)
    }

    /// Bytes the instance folder takes. Walked off the async runtime and
    /// reused for a minute unless `refresh` is set.
    pub async fn disk_usage(&self, instance_id: &str, refresh: bool) -> AppResult<DiskUsage> {
        let instance = self.store.get(instance_id).await?;
        if !refresh {
            if let Some(cached) = self.disk_usage.read().await.get(instance_id) {
                if Utc::now().signed_duration_since(cached.computed_at) < DISK_USAGE_TTL {
                    return Ok(cached.clone());
                }
            }
        }
        let root = instance.path.clone();
        let usage = tokio::task::spawn_blocking(move || crate::service::files::usage(&root))
            .await
            .map_err(AppError::unexpected)??;
        self.disk_usage.write().await.insert(instance_id.to_string(), usage.clone());
        Ok(usage)
    }

    /// Copy dropped paths into the instance, reporting progress as events.
    pub async fn import_files(
        &self,
        instance_id: &str,
        target: String,
        sources: Vec<String>,
        policy: crate::service::import::ConflictPolicy,
        import_id: String,
    ) -> AppResult<crate::service::import::ImportSummary> {
        self.ensure_not_transferring(instance_id).await?;
        let instance = self.store.get(instance_id).await?;
        let events = self.events.clone();
        let id = instance_id.to_string();
        let root = instance.path.clone();
        let summary = tokio::task::spawn_blocking(move || {
            crate::service::import::import(&root, &target, &sources, policy, &import_id, &|progress| {
                let _ = events.send(AppEvent::Import { instance_id: id.clone(), progress });
            })
        })
        .await
        .map_err(AppError::unexpected)??;
        self.disk_usage.write().await.remove(instance_id);
        // Dropping jars into mods/ or plugins/ changes what is installed.
        self.addon_index.write().await.remove(instance_id);
        Ok(summary)
    }

    /// The program players connect to, when the server runs as a process on
    /// this machine (a container has its own networking).
    async fn listening_program(&self, instance_id: &str) -> AppResult<Option<PathBuf>> {
        let instance = self.store.get(instance_id).await?;
        if !cfg!(windows) || instance.runtime_kind != RuntimeKind::HostProcess {
            return Ok(None);
        }
        let install = instance.install.clone().unwrap_or_default();
        let plan = self.registry.get(&instance.game)?.launch_plan(LaunchContext {
            root: &instance.path,
            build: &instance.build,
            install: &install,
            java: instance.resources.runtime_path.as_deref().map(PathBuf::from),
            memory_mb: instance.resources.memory_mb,
            extra_args: &instance.resources.extra_args,
        })?;
        Ok(Some(PathBuf::from(plan.program)))
    }

    /// Whether Windows Firewall lets players reach this server's program.
    pub async fn firewall_status(&self, instance_id: &str) -> AppResult<FirewallStatus> {
        let Some(program) = self.listening_program(instance_id).await? else { return Ok(FirewallStatus::default()) };
        tokio::task::spawn_blocking(move || firewall::check(&program)).await.map_err(AppError::unexpected)
    }

    /// Replace the block rules with allow rules (Windows asks for an
    /// administrator on this machine's screen).
    pub async fn firewall_fix(&self, instance_id: &str) -> AppResult<FirewallStatus> {
        let Some(program) = self.listening_program(instance_id).await? else { return Ok(FirewallStatus::default()) };
        tokio::task::spawn_blocking(move || {
            // Only a JVM this machine found itself (installed, or downloaded
            // by Nebula): a pinned runtime path can come from a paired device.
            let known = crate::runtime::java::detect_installations(false).iter().any(|java| {
                match (std::fs::canonicalize(&java.path), std::fs::canonicalize(&program)) {
                    (Ok(a), Ok(b)) => a == b,
                    _ => false,
                }
            });
            if !known {
                return Err(AppError::new(ErrorCode::InvalidInput).with("program", program.display()));
            }
            firewall::fix(&program)
        })
        .await
        .map_err(AppError::unexpected)?
    }

    /// Something outside the addon calls changed the instance folder (the
    /// Files panel, a restore): forget what was measured or identified there.
    pub async fn after_folder_change(&self, instance_id: &str) {
        self.disk_usage.write().await.remove(instance_id);
        self.after_addon_change(instance_id).await;
    }

    /// The lock that serialises one server's `help` captures.
    pub(crate) async fn completion_lock(&self, instance_id: &str) -> Arc<Mutex<()>> {
        self.completion_fetch.lock().await.entry(instance_id.to_string()).or_default().clone()
    }

    pub(crate) async fn cached_completions(&self, instance_id: &str) -> Option<CachedCommands> {
        self.completions.read().await.get(instance_id).cloned()
    }

    pub(crate) async fn store_completions(&self, instance_id: &str, cached: CachedCommands) {
        self.completions.write().await.insert(instance_id.to_string(), cached);
    }

    pub async fn invalidate_completions(&self, instance_id: &str) {
        self.completions.write().await.remove(instance_id);
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
        if request.accept_agreement && manifest.agreement.is_some() {
            self.registry.get(&request.game)?.accept_agreement(&root)?;
        }

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
        // New servers back themselves up once a day, keeping the last 3.
        instance.automation.auto_backup_hours = 24;
        instance.automation.auto_backup_keep = 3;
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

        if let Err(error) = self.ensure_default_icon(instance_id).await {
            warn!(instance_id, %error, "default server icon not applied");
        }

        progress.report(InstallProgress::Ready);
        Ok(())
    }

    // -- the game's agreement (the Minecraft EULA) ---------------------------

    /// Refuse to run a server whose operator has not accepted the game's
    /// agreement. The UI answers this error by asking the user; nothing in
    /// the backend ever accepts on their behalf.
    fn ensure_agreement(&self, instance: &Instance) -> AppResult<()> {
        let manifest = self.registry.manifest(&instance.game)?;
        let Some(agreement) = manifest.agreement else {
            return Ok(());
        };
        if self.registry.get(&instance.game)?.agreement_accepted(&instance.path) {
            return Ok(());
        }
        Err(AppError::new(ErrorCode::InvalidInput)
            .with("reason", "agreement_required")
            .with("instance", &instance.id)
            .with("url", agreement.url))
    }

    /// The user accepted the game's agreement for this server, in the UI.
    pub async fn accept_agreement(&self, instance_id: &str) -> AppResult<()> {
        self.ensure_not_transferring(instance_id).await?;
        let instance = self.store.get(instance_id).await?;
        let manifest = self.registry.manifest(&instance.game)?;
        if manifest.agreement.is_none() {
            return Ok(());
        }
        self.registry.get(&instance.game)?.accept_agreement(&instance.path)
    }

    // -- lifecycle ---------------------------------------------------------

    pub async fn start(self: &Arc<Self>, instance_id: &str) -> AppResult<()> {
        let instance = self.store.get(instance_id).await?;
        self.ensure_playable(&instance).await?;
        self.ensure_agreement(&instance)?;

        if self.processes.is_running(instance_id).await {
            return Err(AppError::new(ErrorCode::InstanceAlreadyRunning).with("instance", instance_id));
        }

        // Still running from before the app restarted? Take it back instead
        // of starting a second process on the same world.
        if instance.runtime_kind == RuntimeKind::HostProcess {
            let dir = instance.path.clone();
            let found = tokio::task::spawn_blocking(move || {
                crate::runtime::adopt::find_in(&crate::runtime::adopt::java_processes(), &dir)
            })
            .await
            .ok()
            .flatten();
            if let Some(found) = found {
                return self.adopt_one(&instance, found).await;
            }
        }

        // Addon changes queued while it last ran go in before it loads its
        // jars. Normally the stop already did this; this catches a queue left
        // by an app that closed with the server still up. A change that fails
        // stays queued and visible, and does not keep the server down.
        if let Err(error) = self.apply_pending_addons(instance_id).await {
            warn!(instance_id, %error, "pending addon changes were not applied before start");
        }

        // Servers from before default icons get theirs on their next start.
        if let Err(error) = self.ensure_default_icon(instance_id).await {
            warn!(instance_id, %error, "default server icon not applied");
        }

        // Hand the port back before trying to bind it as the server.
        if self.wake.stop(instance_id).await {
            wake_proxy::wait_for_port_release(instance_id, instance.primary_port(&self.registry.manifest(&instance.game)?)).await;
        }

        let manifest = self.registry.manifest(&instance.game)?;
        let adapter = self.registry.get(&instance.game)?;
        let install = instance.install.clone().unwrap_or_default();

        // The record is the source of truth for the port (the sleep proxy and
        // the UI use it); make sure the game's own config says the same before
        // it starts, so a port changed elsewhere can never silently diverge.
        if let Some(spec) = manifest.primary_port() {
            let port = instance.primary_port(&manifest).to_string();
            let current = crate::service::properties::get(&instance.path, &spec.key).ok().flatten();
            if current.as_deref().map(str::trim) != Some(port.as_str()) {
                if let Err(error) = crate::service::properties::set_internal(&instance.path, &spec.key, &port) {
                    tracing::warn!(%instance_id, %error, "could not sync the port into the game config");
                }
            }
        }

        let plan = adapter.launch_plan(LaunchContext {
            root: &instance.path,
            build: &instance.build,
            install: &install,
            java: instance.resources.runtime_path.as_deref().map(PathBuf::from),
            memory_mb: instance.resources.memory_mb,
            extra_args: &instance.resources.extra_args,
        })?;

        // A firewall that blocks the JVM makes a running server unreachable
        // with nothing in its own log; say so in the console (Overview offers
        // the fix).
        if cfg!(windows) && instance.runtime_kind == RuntimeKind::HostProcess {
            let service = self.clone();
            let id = instance_id.to_string();
            let program = PathBuf::from(&plan.program);
            tokio::spawn(async move {
                let status = tokio::task::spawn_blocking(move || firewall::check(&program)).await.unwrap_or_default();
                if status.blocked {
                    let program = status.program.unwrap_or_default();
                    warn!(instance_id = %id, %program, "Windows Firewall blocks the server's Java");
                    service
                        .push_console(
                            &id,
                            ConsoleLine::new(
                                ConsoleLevel::Warn,
                                format!("Windows Firewall is blocking {program}: players cannot connect. Open Overview to fix it."),
                            ),
                        )
                        .await;
                }
            });
        }

        let parser = Arc::new(ConsoleParser::new(&manifest.console)?);
        let (console_tx, console_rx) = mpsc::channel(256);
        let (events_tx, events_rx) = mpsc::channel(256);

        self.set_status(instance_id, InstanceStatus::Starting).await;
        self.push_console(
            instance_id,
            ConsoleLine::new(ConsoleLevel::System, format!("Starting {}…", instance.name)),
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

        self.auto_forward(&instance, instance.primary_port(&manifest));
        Ok(())
    }

    /// Take back every server still running from before the app started
    /// (see [`crate::runtime::adopt`]). Call once at launch, before sleeping
    /// servers get their wake proxy.
    pub async fn adopt_running(self: &Arc<Self>) {
        let processes = tokio::task::spawn_blocking(crate::runtime::adopt::java_processes)
            .await
            .unwrap_or_default();
        if processes.is_empty() {
            return;
        }
        for instance in self.store.list().await {
            if instance.runtime_kind != RuntimeKind::HostProcess || self.processes.is_running(&instance.id).await {
                continue;
            }
            if let Some(found) = crate::runtime::adopt::find_in(&processes, &instance.path) {
                if let Err(error) = self.adopt_one(&instance, found).await {
                    warn!(instance_id = %instance.id, %error, "could not reconnect to a running server");
                }
            }
        }
    }

    async fn adopt_one(self: &Arc<Self>, instance: &Instance, found: crate::runtime::adopt::Found) -> AppResult<()> {
        let manifest = self.registry.manifest(&instance.game)?;
        let parser = Arc::new(ConsoleParser::new(&manifest.console)?);
        let (console_tx, console_rx) = mpsc::channel(256);
        let (events_tx, events_rx) = mpsc::channel(256);
        self.clone().consume_console(instance.id.clone(), console_rx);
        self.clone().consume_events(events_rx);
        // A wake proxy cannot hold a port the server already has, but make sure.
        self.wake.stop(&instance.id).await;
        self.processes
            .adopt(
                &instance.id,
                found,
                parser,
                instance.path.join("logs").join("latest.log"),
                console_tx,
                events_tx,
            )
            .await?;
        self.set_status(&instance.id, InstanceStatus::Online).await;
        self.push_console(
            &instance.id,
            ConsoleLine::new(
                ConsoleLevel::System,
                "Reconnected: this server kept running while Nebula Host was closed. The console shows its log; \
                 typing commands works again after its next restart."
                    .to_string(),
            ),
        )
        .await;
        self.auto_forward(instance, instance.primary_port(&manifest));
        info!(instance_id = %instance.id, pid = found.pid, "reconnected to a server left running");
        Ok(())
    }

    /// Open the game port on the router (UPnP) when the user allowed it, so
    /// players outside the LAN can join. Runs in the background: a router
    /// that does not answer must not hold up the start.
    fn auto_forward(self: &Arc<Self>, instance: &Instance, port: u16) {
        if !settings::load().map(|settings| settings.allow_upnp).unwrap_or(false) {
            return;
        }
        let service = self.clone();
        let id = instance.id.clone();
        let description = format!("Nebula Host - {}", instance.name);
        tokio::spawn(async move {
            if service.forwarder.forwarded_port(&id).await == Some(port) {
                return;
            }
            match service.forwarder.forward(&id, port, &description).await {
                Ok(()) => {
                    let external = crate::net::upnp::external_address().await.ok();
                    info!(instance_id = %id, port, ?external, "port forwarded with UPnP");
                }
                Err(error) => warn!(instance_id = %id, port, %error, "UPnP port forwarding failed"),
            }
        });
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
            ConsoleLine::new(ConsoleLevel::System, format!("> {command}")),
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
        // A sleeping server is one join away from starting: same rule.
        self.ensure_playable(&instance).await?;
        self.ensure_agreement(&instance)?;

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
                let line = ConsoleLine::new(
                    if level == "WARN" { ConsoleLevel::Warn } else { ConsoleLevel::System },
                    message,
                );
                tokio::spawn(async move {
                    service.push_console(&id, line).await;
                });
            }),
        };

        // The wake listener stands in for the server, so it must be exactly as
        // reachable as the server itself — no more, no less. Players connect
        // to the address the UI shows (the LAN IP); a loopback-only stand-in
        // made a sleeping server look offline ("Connection refused") to all of
        // them. So: the interface the server is pinned to by its own config,
        // or every interface when unpinned, which is what the server does.
        let bind = spec
            .bind_setting
            .as_deref()
            .and_then(|key| crate::service::properties::get(&instance.path, key).ok().flatten())
            .and_then(|value| value.trim().parse::<IpAddr>().ok())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

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
        self.auto_forward(&instance, instance.primary_port(&manifest));
        Ok(())
    }

    pub async fn wake(self: &Arc<Self>, instance_id: &str) -> AppResult<()> {
        self.wake.stop(instance_id).await;
        self.start(instance_id).await
    }

    pub async fn delete(&self, instance_id: &str, delete_files: bool) -> AppResult<()> {
        self.ensure_not_transferring(instance_id).await?;
        self.delete_unchecked(instance_id, delete_files).await
    }

    /// `delete` without the transfer guard, for the transfer steps themselves.
    pub(crate) async fn delete_unchecked(&self, instance_id: &str, delete_files: bool) -> AppResult<()> {
        if self.processes.is_running(instance_id).await {
            return Err(AppError::new(ErrorCode::InstanceAlreadyRunning).with("instance", instance_id));
        }
        self.wake.stop(instance_id).await;
        self.forwarder.release(instance_id).await;
        self.drop_pending_addons(instance_id).await;
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
                    RuntimeEvent::PlayerIdentified(id, name, uuid) => {
                        self.identities.write().await.entry(id).or_default().insert(name, uuid);
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

        // The jars are free now: put queued addon changes in place, before a
        // crash restart or a wake-on-join could load the old set again.
        if let Err(error) = self.apply_pending_addons(instance_id).await {
            warn!(instance_id, %error, "pending addon changes were not applied after stop");
        }

        if !expected {
            info!(instance_id, ?code, "server exited unexpectedly");
            self.set_status(instance_id, InstanceStatus::Crashed).await;
            self.push_console(
                instance_id,
                ConsoleLine::new(
                    ConsoleLevel::Error,
                    match code {
                        Some(code) => format!("Server stopped unexpectedly (exit code {code})."),
                        None => "Server stopped unexpectedly.".to_string(),
                    },
                ),
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
            ConsoleLine::new(ConsoleLevel::System, "Server stopped.".to_string()),
        )
        .await;

        if !instance.automation.wake_on_join {
            self.forwarder.release(instance_id).await;
        }

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
            let previous = state.status;
            state.status = status;
            // Every fresh start gets a full idle grace period. Keeping the
            // timestamp from the previous run made sleep-when-empty stop a
            // just-woken server one tick after it came online — before the
            // player who woke it could reconnect.
            if status == InstanceStatus::Online && previous != InstanceStatus::Online {
                state.last_player_seen_at = Some(Utc::now());
            }
            if !matches!(status, InstanceStatus::Online | InstanceStatus::Stopping) {
                // A restarted server may have different plugins loaded.
                self.completions.write().await.remove(instance_id);
            }
            if !status.expects_process() {
                state.uptime_seconds = None;
                state.cpu_percent = 0.0;
                state.memory_used_mb = 0;
                state.tick_rate = None;
            }
            // Emitted while the state is locked, so events leave in the order
            // the states changed: a measurement taken before this change can
            // never reach a listener after it.
            self.emit(AppEvent::Status { instance_id: instance_id.to_string(), status });
        }
    }

    /// Store freshly measured numbers and tell the UI. Status is not part of
    /// this: only `set_status` writes it.
    pub async fn publish_state(&self, instance_id: &str, state: InstanceState) {
        let mut states = self.states.write().await;
        let stored = states.entry(instance_id.to_string()).or_default();
        let status = stored.status;
        *stored = state;
        stored.status = status;
        // Carries the authoritative status, so a listener that missed a
        // status event (a dropped connection, a busy tab) is corrected by
        // the next measurement instead of staying wrong.
        self.emit(AppEvent::Stats {
            instance_id: instance_id.to_string(),
            state: Box::new(stored.clone()),
        });
    }

    async fn mark_player_activity(&self, instance_id: &str) {
        let mut states = self.states.write().await;
        states.entry(instance_id.to_string()).or_default().last_player_seen_at = Some(Utc::now());
    }

    pub(crate) async fn push_console(&self, instance_id: &str, line: ConsoleLine) {
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
pub(crate) fn validate_name(name: &str) -> AppResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(AppError::new(ErrorCode::InvalidInput).with("field", "name"));
    }
    if trimmed.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) || trimmed.contains("..") {
        return Err(AppError::new(ErrorCode::InvalidInput).with("field", "name"));
    }
    Ok(trimmed.to_string())
}

pub(crate) fn slug(name: &str) -> String {
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

// -- editing ------------------------------------------------------------------

/// Everything on the instance a user can change from the UI. Absent fields are
/// left alone, so the UI can send one edit at a time.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancePatch {
    pub name: Option<String>,
    pub memory_mb: Option<u32>,
    pub port: Option<u16>,
    pub runtime_kind: Option<RuntimeKind>,
    pub runtime_path: Option<String>,
    pub extra_args: Option<Vec<String>>,
    pub sleep_when_empty: Option<bool>,
    pub sleep_after_minutes: Option<u32>,
    pub wake_on_join: Option<bool>,
    pub restart_after_crash: Option<bool>,
    pub auto_backup_hours: Option<u32>,
    pub auto_backup_keep: Option<u32>,
    pub track_latest_version: Option<bool>,
    pub schedules: Option<Vec<crate::model::ScheduleRule>>,
}

impl InstanceService {
    pub async fn apply_patch(&self, instance_id: &str, patch: InstancePatch) -> AppResult<Instance> {
    self.ensure_not_transferring(instance_id).await?;
    // Validate before touching the stored record: a rejected port must not
    // leave a half-applied edit behind.
    if let Some(port) = patch.port {
        crate::net::ports::validate(port)?;
    }

    let manifest = {
        let instance = self.store.get(instance_id).await?;
        self.registry.manifest(&instance.game)?
    };

    self
        .store
        .update(instance_id, move |instance| {
            if let Some(name) = patch.name {
                instance.name = name;
            }
            if let Some(memory_mb) = patch.memory_mb {
                instance.resources.memory_mb = memory_mb;
            }
            if let Some(port) = patch.port {
                if let Some(spec) = manifest.primary_port() {
                    instance.ports.insert(spec.key.clone(), port);
                }
            }
            if let Some(runtime_kind) = patch.runtime_kind {
                instance.runtime_kind = runtime_kind;
            }
            if let Some(runtime_path) = patch.runtime_path {
                instance.resources.runtime_path =
                    Some(runtime_path).filter(|path| !path.trim().is_empty());
            }
            if let Some(extra_args) = patch.extra_args {
                instance.resources.extra_args = extra_args;
            }
            if let Some(value) = patch.sleep_when_empty {
                instance.automation.sleep_when_empty = value;
            }
            if let Some(value) = patch.sleep_after_minutes {
                instance.automation.sleep_after_minutes = value.max(1);
            }
            if let Some(value) = patch.wake_on_join {
                instance.automation.wake_on_join = value;
            }
            if let Some(value) = patch.restart_after_crash {
                instance.automation.restart_after_crash = value;
            }
            if let Some(value) = patch.auto_backup_hours {
                instance.automation.auto_backup_hours = value.min(24 * 30);
            }
            if let Some(value) = patch.auto_backup_keep {
                instance.automation.auto_backup_keep = value.clamp(1, 50);
            }
            if let Some(value) = patch.track_latest_version {
                instance.track_latest_version = value;
            }
            if let Some(schedules) = patch.schedules {
                instance.automation.schedules = schedules;
            }
        })
        .await
    }
}
