//! Games are data, not code.
//!
//! A [`GameManifest`] declares everything the rest of the app needs to know
//! about a game: which distributions exist, what runtime it needs, how to read
//! its console, which settings it exposes, what to back up, and whether it can
//! be woken by a join attempt. Nothing outside this module may branch on a
//! game id.
//!
//! The parts that genuinely cannot be data — talking to PaperMC's API,
//! unpacking a Forge installer — live behind [`GameAdapter`], one
//! implementation per game, resolved through the [`GameRegistry`].

pub mod manifest;
pub mod minecraft;
pub mod registry;

pub use manifest::{
    BackupSpec, BuiltinCommand, CompletionSpec, ConsoleSpec, Distribution, GameManifest, PlayerSource, PortSpec, RuntimeSpec,
    SettingField, SettingKind, WakeProtocol, WakeSpec,
};
pub use registry::{GameAdapter, GameRegistry};

use serde::{Deserialize, Serialize};

/// What every upstream call identifies itself as. Modrinth asks for a
/// descriptive agent with a contact, and rate-limits generic ones harder.
pub const USER_AGENT: &str = concat!("NebulaHost/", env!("CARGO_PKG_VERSION"), " (G2806)");

/// One HTTP client for every upstream call: connection reuse, a real user
/// agent (several of these APIs rate-limit anonymous traffic harder), and
/// timeouts so a hung CDN cannot wedge an install forever.
///
/// Built once: a fresh client per call throws away the connection pool.
pub fn http_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(build_http_client).clone()
}

fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("the default TLS backend always builds")
}

/// Canonical game ids. A game id is a stable slug; it appears in saved
/// instance configs on disk, so it must never be renamed.
pub const MINECRAFT: &str = "minecraft";

/// A specific installable build of a game: distribution + game version +
/// optional loader version (Fabric/Forge/NeoForge build number).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildSelector {
    pub distribution: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
}

impl BuildSelector {
    pub fn new(distribution: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            distribution: distribution.into(),
            version: version.into(),
            loader_version: None,
        }
    }
}

/// One selectable version of a game, as offered by an upstream API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub id: String,
    /// `release`, `snapshot`, `beta`… Used to group and to hide unstable
    /// builds behind a toggle rather than a separate list.
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub stable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released_at: Option<String>,
}
