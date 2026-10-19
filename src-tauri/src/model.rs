//! The game-agnostic domain model.
//!
//! A **node** is a machine that can host servers. Today there is exactly one
//! (this machine), but nothing here assumes that: the mobile app will pair with
//! a node, and a rented box would be a node whose hardware the user does not
//! own — same shape, same API, not a second product.
//!
//! An **instance** is one server on a node. It belongs to a game, and every
//! game-specific decision it needs is read from that game's manifest.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::game::BuildSelector;
use crate::game::registry::InstallReport;

// ---------------------------------------------------------------------------
// Node
// ---------------------------------------------------------------------------

/// How the app reaches a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeTransport {
    /// This machine. No network hop, no auth — the process *is* the node.
    Local,
    /// Another machine on the LAN, reached over the node protocol after
    /// pairing. Not implemented yet; modelled so the UI never has to change.
    Paired { address: String, fingerprint: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    /// User-chosen machine name, set during first run. Not an account name.
    pub name: String,
    pub transport: NodeTransport,
    /// Devices allowed to control this node. Empty until pairing ships.
    #[serde(default)]
    pub paired_devices: Vec<PairedDevice>,
    pub created_at: DateTime<Utc>,
}

impl Node {
    /// The id every node used before ids were unique. Records that still carry
    /// it are migrated once on load (see `store::nodes::load`).
    pub const LEGACY_LOCAL_ID: &'static str = "local";

    /// This machine, with a fresh globally unique id. Transfers move servers
    /// between nodes, so "which machine" has to mean one machine.
    pub fn local(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            transport: NodeTransport::Local,
            paired_devices: Vec::new(),
            created_at: Utc::now(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self.transport, NodeTransport::Local)
    }
}

/// A device that has been paired with this node.
///
/// Pairing, not login: there is no account server. A pairing code is a
/// short-lived secret exchanged on the LAN, and it must expire — a permanent
/// code is a permanent key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub paired_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
}

/// Live health of a node, shown in the header instead of a user avatar.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeHealth {
    pub cpu_percent: f32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub disk_used_gb: u64,
    pub disk_total_gb: u64,
    /// LAN address other machines can reach this node on.
    pub lan_address: Option<String>,
    pub instances_running: u32,
    /// Seconds since the machine itself booted.
    #[serde(default)]
    pub uptime_seconds: u64,
    /// Operating system, e.g. "Windows 11 Pro" or "Ubuntu 26.04".
    #[serde(default)]
    pub os: String,
    /// Logical CPUs.
    #[serde(default)]
    pub cpu_count: usize,
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

/// The six states a server can be in. There is exactly one status vocabulary
/// in this codebase, and exactly one component that renders it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    Offline,
    /// Stopped, but a wake proxy holds the port: the next join starts it.
    Sleeping,
    Starting,
    Online,
    Stopping,
    Crashed,
}

impl InstanceStatus {
    /// Whether a host process should exist for this status. The stats poller
    /// uses this instead of inferring status from process liveness, which
    /// would clobber `Sleeping` (no process, but very much not offline) back
    /// to `Offline` on every tick.
    pub fn expects_process(self) -> bool {
        matches!(self, Self::Starting | Self::Online | Self::Stopping)
    }
}

/// How an instance is executed on its node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    #[default]
    HostProcess,
    Docker,
}

/// Automatic behaviour the user asked for, in player-facing terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationConfig {
    /// Stop the server once it has been empty for `sleep_after_minutes`.
    #[serde(default)]
    pub sleep_when_empty: bool,
    #[serde(default = "default_sleep_minutes")]
    pub sleep_after_minutes: u32,
    /// Hold the port while stopped and start again when someone joins.
    #[serde(default)]
    pub wake_on_join: bool,
    /// Bring the server back up if it exits unexpectedly.
    #[serde(default)]
    pub restart_after_crash: bool,
    #[serde(default)]
    pub restart_attempts_remaining: u8,
    #[serde(default)]
    pub schedules: Vec<ScheduleRule>,
    /// Take an automatic backup every this many hours (0 = off).
    #[serde(default)]
    pub auto_backup_hours: u32,
    /// How many automatic backups to keep; the oldest goes when a new one is
    /// made. Manual backups are never counted or removed.
    #[serde(default = "default_auto_backup_keep")]
    pub auto_backup_keep: u32,
}

fn default_auto_backup_keep() -> u32 {
    3
}

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            sleep_when_empty: false,
            sleep_after_minutes: default_sleep_minutes(),
            wake_on_join: false,
            restart_after_crash: false,
            restart_attempts_remaining: 0,
            schedules: Vec::new(),
            auto_backup_hours: 0,
            auto_backup_keep: default_auto_backup_keep(),
        }
    }
}

fn default_sleep_minutes() -> u32 {
    30
}

/// At `time` on the given weekdays, run `action`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleRule {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub action: ScheduleAction,
    /// Local time, `HH:MM`, 24-hour.
    pub time: String,
    /// 0 = Sunday … 6 = Saturday.
    pub days: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleAction {
    Start,
    Stop,
    Restart,
    Backup,
}

fn default_true() -> bool {
    true
}

/// Resources granted to an instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    pub memory_mb: u32,
    /// Path to a specific runtime (a JVM, say). `None` means "resolve one".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_path: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self { memory_mb: 4096, runtime_path: None, extra_args: Vec::new() }
    }
}

/// The persisted description of a server. Anything derived at runtime lives in
/// [`InstanceState`] instead, so this file stays small and stable on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub node_id: String,
    /// Game manifest id — `minecraft`, later `valheim`, `palworld`…
    pub game: String,
    pub name: String,
    pub build: BuildSelector,
    /// Whether to follow the newest game version on each start.
    #[serde(default)]
    pub track_latest_version: bool,
    pub path: PathBuf,
    #[serde(default)]
    pub runtime_kind: RuntimeKind,
    #[serde(default)]
    pub resources: ResourceConfig,
    #[serde(default)]
    pub automation: AutomationConfig,
    /// Ports keyed by the manifest's `PortSpec::key`.
    #[serde(default)]
    pub ports: BTreeMap<String, u16>,
    /// What the last successful install produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_started_at: Option<DateTime<Utc>>,
    /// Lineage counter: bumped each time the authoritative copy changes
    /// machines, and when an older copy is forced back into play. Records
    /// from before transfers read as 0.
    #[serde(default)]
    pub generation: u64,
    /// Whether this copy is the one to play, or an older copy left behind by
    /// a transfer.
    #[serde(default)]
    pub transfer: TransferState,
    /// Set when this copy was forced into play while another machine had a
    /// newer one: from then on the two copies have diverged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<ForkInfo>,
}

/// Where the authoritative copy of a server is, from this copy's point of view.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TransferState {
    /// This copy is the server. It starts, sleeps and wakes normally.
    #[default]
    Active,
    /// The server moved to another machine; this is the copy it left behind.
    /// It does not start unless the user forces it (which forks it).
    TransferredOut {
        to_node_id: String,
        to_node_name: String,
        at: DateTime<Utc>,
        /// The generation the destination took on — newer than ours.
        generation: u64,
    },
}

/// The copy this one diverged from when it was forced into play.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkInfo {
    /// The machine holding the other (possibly newer) copy.
    pub node_id: String,
    pub node_name: String,
    /// That copy's generation at the time of the fork.
    pub generation: u64,
    pub at: DateTime<Utc>,
}

impl Instance {
    /// Whether this copy is an older one left behind by a transfer.
    pub fn is_transferred_out(&self) -> bool {
        matches!(self.transfer, TransferState::TransferredOut { .. })
    }
    pub fn new(
        node_id: impl Into<String>,
        game: impl Into<String>,
        name: impl Into<String>,
        build: BuildSelector,
        path: PathBuf,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            node_id: node_id.into(),
            game: game.into(),
            name: name.into(),
            build,
            track_latest_version: false,
            path,
            runtime_kind: RuntimeKind::default(),
            resources: ResourceConfig::default(),
            automation: AutomationConfig::default(),
            ports: BTreeMap::new(),
            install: None,
            icon: None,
            created_at: Utc::now(),
            last_started_at: None,
            generation: 0,
            transfer: TransferState::Active,
            forked_from: None,
        }
    }

    /// The port players connect to, per the game's manifest.
    pub fn primary_port(&self, manifest: &crate::game::GameManifest) -> u16 {
        manifest
            .primary_port()
            .map(|spec| self.ports.get(&spec.key).copied().unwrap_or(spec.default))
            .unwrap_or(0)
    }
}

/// Volatile per-instance state. Never written to disk: an idle clock that
/// survives a relaunch would be lying about how long the server sat empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceState {
    pub status: InstanceStatus,
    pub players_online: u32,
    pub players_max: u32,
    pub cpu_percent: f32,
    pub memory_used_mb: u64,
    /// Seconds since the process started.
    pub uptime_seconds: Option<u64>,
    /// Ticks per second, or whatever the game's health metric is.
    pub tick_rate: Option<f32>,
    /// Last moment a player was seen, or when the server came online if none
    /// ever joined. Drives sleep-when-empty.
    pub last_player_seen_at: Option<DateTime<Utc>>,
}

impl Default for InstanceState {
    fn default() -> Self {
        Self {
            status: InstanceStatus::Offline,
            players_online: 0,
            players_max: 0,
            cpu_percent: 0.0,
            memory_used_mb: 0,
            uptime_seconds: None,
            tick_rate: None,
            last_player_seen_at: None,
        }
    }
}

/// Config plus live state, as the UI consumes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceView {
    #[serde(flatten)]
    pub instance: Instance,
    #[serde(flatten)]
    pub state: InstanceState,
    /// Address to hand to players, when the node knows one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// Internet address (router's public IP + port) while UPnP forwards it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_address: Option<String>,
    /// The server's `server-icon.png` as a `data:` URL, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// One line of server output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleLine {
    pub timestamp: DateTime<Utc>,
    pub level: ConsoleLevel,
    pub message: String,
    /// `message` without the game's own time/level prefix, for display (the
    /// console gutter already has the time). Absent when there was none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Set when the line is something a player did: said, joined, left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<ConsoleSpeaker>,
}

impl ConsoleLine {
    /// A line with no player attached — app messages, plain server output.
    pub fn new(level: ConsoleLevel, message: impl Into<String>) -> Self {
        Self { timestamp: Utc::now(), level, message: message.into(), body: None, speaker: None }
    }
}

/// The player a console line is about, read with the manifest's patterns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleSpeaker {
    pub name: String,
    pub kind: SpeakerKind,
    /// What they said, for chat lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerKind {
    Chat,
    Join,
    Leave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleLevel {
    Info,
    Warn,
    Error,
    /// Emitted by the app itself, not the server ("Starting…", "Stopped").
    System,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeping_expects_no_process() {
        assert!(!InstanceStatus::Sleeping.expects_process());
        assert!(!InstanceStatus::Offline.expects_process());
        assert!(InstanceStatus::Online.expects_process());
        assert!(InstanceStatus::Starting.expects_process());
    }

    #[test]
    fn instance_falls_back_to_the_manifest_port_default() {
        let manifest = crate::game::minecraft::manifest();
        let instance = Instance::new(
            "local",
            crate::game::MINECRAFT,
            "Survival",
            BuildSelector::new("vanilla", "1.21.4"),
            PathBuf::from("/tmp/survival"),
        );
        assert_eq!(instance.primary_port(&manifest), 25565);
    }

    #[test]
    fn explicit_port_wins_over_the_default() {
        let manifest = crate::game::minecraft::manifest();
        let mut instance = Instance::new(
            "local",
            crate::game::MINECRAFT,
            "Survival",
            BuildSelector::new("vanilla", "1.21.4"),
            PathBuf::from("/tmp/survival"),
        );
        let key = manifest.primary_port().expect("minecraft has a port").key.clone();
        instance.ports.insert(key, 25570);
        assert_eq!(instance.primary_port(&manifest), 25570);
    }
}
