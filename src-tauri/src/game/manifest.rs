//! The manifest schema. Every field here is serialisable, so a game can ship
//! as a JSON file without a code change.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Everything the app knows about a game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameManifest {
    /// Stable slug. Written into instance configs; never rename.
    pub id: String,
    /// Untranslated product name ("Minecraft: Java Edition").
    pub name: String,
    /// Accent colour for per-game tinting in the UI, `#rrggbb`.
    pub accent: String,
    /// Icon asset id resolved by the frontend icon registry.
    pub icon: String,
    /// Installable flavours (Vanilla, Paper, Fabric…).
    pub distributions: Vec<Distribution>,
    pub runtime: RuntimeSpec,
    pub ports: Vec<PortSpec>,
    pub console: ConsoleSpec,
    pub players: PlayerSource,
    #[serde(default)]
    pub settings: Vec<SettingField>,
    pub backup: BackupSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake: Option<WakeSpec>,
    /// Where the game writes its own log files, for sharing them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logs: Option<LogSpec>,
    /// The picture the game shows for this server in its server list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_icon: Option<ServerIconSpec>,
}

impl GameManifest {
    pub fn distribution(&self, id: &str) -> Option<&Distribution> {
        self.distributions.iter().find(|d| d.id == id)
    }

    /// The port the game is reached on, used for wake-on-join and forwarding.
    pub fn primary_port(&self) -> Option<&PortSpec> {
        self.ports.iter().find(|p| p.primary).or(self.ports.first())
    }
}

/// One installable flavour of a game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    pub id: String,
    pub name: String,
    /// Translation key for the one-line "what is this" help text.
    pub summary_key: String,
    /// Whether this flavour takes a separate loader/build version.
    #[serde(default)]
    pub has_loader_version: bool,
    /// Addon ecosystem this flavour accepts, if any — drives the mods browser
    /// (`fabric`, `forge`, `neoforge`, `paper`, `spigot`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addon_loaders: Vec<String>,
    /// Where addons are installed relative to the instance root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addon_dir: Option<String>,
    /// What the addon catalogue calls this flavour's addons (`mod`,
    /// `plugin`). Drives the catalogue's project-type facet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addon_project_type: Option<String>,
    /// Console command that asks for the tick rate. Distribution-level because
    /// Forge answers `forge tps` and NeoForge only answers `neoforge tps`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_rate_command: Option<String>,
    /// Identity colour, `#rrggbb`. The UI stripes a server with it so a shelf
    /// of ten instances is scannable by flavour. Distinct from status colour,
    /// which means something entirely different and is never mixed with this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
}

/// What the host needs before this game can run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeSpec {
    /// A JVM of at least `min_major`, resolved per game version by the adapter.
    Java {
        #[serde(default)]
        min_major: u32,
    },
    /// A native executable shipped with the server files.
    Native { executable: String },
    /// Only runnable inside a container image.
    Container { image: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortSpec {
    /// Setting key this port is stored under (`server-port`).
    pub key: String,
    pub default: u16,
    #[serde(default)]
    pub protocol: PortProtocol,
    /// The port players connect to. Exactly one port should set this.
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortProtocol {
    #[default]
    Tcp,
    Udp,
    Both,
}

/// How to read meaning out of the server's stdout, and how to talk back to it.
///
/// Patterns are regular expressions with named captures. Keeping them here is
/// what makes "detect players" a data change: Valheim's join line is a
/// different regex, not different code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleSpec {
    /// Matches the line that means "accepting connections now".
    pub ready_pattern: String,
    /// Captures `name` when a player joins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_pattern: Option<String>,
    /// Captures `name` when a player leaves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leave_pattern: Option<String>,
    /// Patterns that capture `value` for a tick-rate readout. Several are
    /// allowed because one game can report it in more than one shape: Paper
    /// answers `tps` with a three-average line, NeoForge with a per-dimension
    /// table whose `Overall` row is the server-wide figure.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tick_rate_patterns: Vec<String>,
    /// Lines matching these are swallowed rather than shown. They are the
    /// noise a background poll produces — the user never asked for it, so it
    /// must not land in their console.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suppress_patterns: Vec<String>,
    /// Command that stops the server cleanly.
    pub stop_command: String,
    /// Strips in-band formatting before display (Minecraft's `§` codes).
    #[serde(default)]
    pub strip_colour_codes: bool,
    /// Captures `name` and `message` when a player says something in chat.
    /// Checked before the join/leave patterns, so a player typing "Steve
    /// joined the game" is chat, not a join.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_pattern: Option<String>,
    /// Captures `name` and `uuid` when the server announces who a connecting
    /// player is, so faces can be looked up by account rather than by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_pattern: Option<String>,
    /// How to ask the running server which commands it knows, for console
    /// autocomplete. Absent means "only the builtin list".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<CompletionSpec>,
    /// Matches the game's own log prefix (its timestamp and thread/level tag)
    /// at the start of a line. The console already shows Nebula's time in
    /// its gutter, so the line's display body drops this; the raw line is
    /// kept as `message`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_prefix_pattern: Option<String>,
}

/// Console autocomplete, as data.
///
/// The running server is asked for its command list with `help_command`
/// (hidden from the user's console), and the answer is read with
/// `entry_pattern`. A paginated answer is detected with `page_pattern` and the
/// remaining pages fetched with `page_command`. `builtin` is the fallback for
/// when the server is not running, and is merged with whatever it reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionSpec {
    pub help_command: String,
    /// `{page}` is substituted with the page number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_command: Option<String>,
    /// Captures `page` and `pages` from a page header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_pattern: Option<String>,
    /// Captures `command`, and optionally `description` or `usage`.
    pub entry_pattern: String,
    /// Other lines of the help answer that must stay out of the console.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub noise_patterns: Vec<String>,
    #[serde(default = "default_max_pages")]
    pub max_pages: u32,
    /// Usage tokens that stand for a player name (`<targets>`), so the UI can
    /// offer online players for that argument.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub player_placeholders: Vec<String>,
    /// Commands every server of this game understands.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub builtin: Vec<BuiltinCommand>,
}

fn default_max_pages() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinCommand {
    pub name: String,
    /// Argument syntax, without the command name (`<targets> [<reason>]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<String>,
}

/// Where the online-player list comes from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlayerSource {
    /// Derived from the console join/leave patterns above.
    Console,
    /// Queried over the network with a game protocol.
    Query { protocol: String },
    /// Read from files in the instance directory (whitelist.json, ops.json…).
    Files {
        #[serde(default)]
        lists: BTreeMap<String, String>,
        /// A name → account-id cache the game keeps (`usercache.json`), used
        /// to put faces on players who are not on any list.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identity_file: Option<String>,
    },
}

/// One user-editable setting, rendered generically by the settings panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingField {
    /// Key in the game's own config file (`server.properties` etc.).
    pub key: String,
    /// Translation key for the label.
    pub label_key: String,
    /// Translation key for the help text under the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help_key: Option<String>,
    #[serde(flatten)]
    pub kind: SettingKind,
    /// Grouping in the settings UI (`world`, `players`, `performance`).
    #[serde(default)]
    pub group: String,
    /// Changing this only takes effect after a restart.
    #[serde(default)]
    pub restart_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SettingKind {
    Bool {
        default: bool,
    },
    Int {
        default: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
    Text {
        #[serde(default)]
        default: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_len: Option<usize>,
    },
    Choice {
        #[serde(default)]
        default: String,
        /// Value plus its translation key.
        options: Vec<ChoiceOption>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoiceOption {
    pub value: String,
    pub label_key: String,
}

/// What a backup of this game contains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSpec {
    /// Globs relative to the instance root, in priority order.
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    /// The server must be stopped (or flushed) before a consistent copy.
    #[serde(default)]
    pub requires_stop: bool,
    /// Command that flushes state to disk instead of stopping, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flush_command: Option<String>,
}

/// The game's log files, relative to the instance root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogSpec {
    /// The log of the current (or last) run, e.g. `logs/latest.log`.
    pub latest: String,
    /// A folder of crash reports, newest by modification time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crash_dir: Option<String>,
}

/// A server-list icon the game reads from the instance folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerIconSpec {
    /// File relative to the instance root, always written as PNG.
    pub file: String,
    /// Square side in pixels; the game ignores any other size.
    pub size: u32,
}

/// Wake-on-join support: answer the server-list ping while asleep, start the
/// real server when someone actually tries to connect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeSpec {
    pub protocol: WakeProtocol,
    /// Shown in the server browser while asleep; `{name}` is substituted.
    #[serde(default = "default_sleeping_motd")]
    pub sleeping_motd: String,
    /// The game setting that pins the server to one interface (`server-ip`
    /// for Minecraft). The sleeping stand-in listens exactly where the server
    /// itself would: that interface when set, every interface when empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_setting: Option<String>,
}

fn default_sleeping_motd() -> String {
    "{name} is asleep — join to wake it".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeProtocol {
    /// Minecraft Java Edition handshake + status response.
    MinecraftJava,
    /// Any TCP connection attempt wakes the server. No pretty MOTD.
    TcpConnect,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with_ports(ports: Vec<PortSpec>) -> GameManifest {
        GameManifest {
            id: "test".into(),
            name: "Test".into(),
            accent: "#ffffff".into(),
            icon: "test".into(),
            distributions: vec![],
            runtime: RuntimeSpec::Java { min_major: 21 },
            ports,
            console: ConsoleSpec {
                ready_pattern: "ready".into(),
                join_pattern: None,
                leave_pattern: None,
                tick_rate_patterns: vec![],
                suppress_patterns: vec![],
                stop_command: "stop".into(),
                strip_colour_codes: false,
                chat_pattern: None,
                identity_pattern: None,
                completion: None,
                log_prefix_pattern: None,
            },
            players: PlayerSource::Console,
            settings: vec![],
            backup: BackupSpec {
                include: vec!["**".into()],
                exclude: vec![],
                requires_stop: false,
                flush_command: None,
            },
            wake: None,
            logs: None,
            server_icon: None,
        }
    }

    fn port(key: &str, primary: bool) -> PortSpec {
        PortSpec { key: key.into(), default: 1234, protocol: PortProtocol::Tcp, primary }
    }

    #[test]
    fn primary_port_prefers_the_flagged_one() {
        let manifest = manifest_with_ports(vec![port("query", false), port("game", true)]);
        assert_eq!(manifest.primary_port().expect("a port").key, "game");
    }

    #[test]
    fn primary_port_falls_back_to_the_first() {
        let manifest = manifest_with_ports(vec![port("game", false), port("query", false)]);
        assert_eq!(manifest.primary_port().expect("a port").key, "game");
    }

    #[test]
    fn every_distribution_has_an_identity_colour() {
        for dist in crate::game::minecraft::manifest().distributions {
            let accent = dist.accent.unwrap_or_default();
            assert!(
                accent.starts_with('#') && accent.len() == 7,
                "{} needs a #rrggbb accent, got {accent:?}",
                dist.id
            );
        }
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let manifest = manifest_with_ports(vec![port("game", true)]);
        let json = serde_json::to_string(&manifest).expect("serialize");
        let back: GameManifest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, manifest.id);
        assert!(matches!(back.runtime, RuntimeSpec::Java { min_major: 21 }));
    }
}
