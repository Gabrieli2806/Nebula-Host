//! The adapter side of "games are data": the small amount of behaviour that a
//! JSON manifest cannot express — talking to upstream version APIs and running
//! a vendor installer — plus the registry that resolves a game id to it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::{BuildSelector, GameManifest, VersionInfo};

/// A phase of a long install, reported to the UI as it happens.
///
/// Starting a modded server takes 30–90 seconds. The user gets to see which
/// of those seconds is doing what, so these variants are deliberately specific
/// — never collapse them into "working".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum InstallProgress {
    /// Asking upstream which build to fetch.
    Resolving,
    /// Bytes are moving. `total` is absent when the server sends no length.
    Downloading {
        file: String,
        received: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
    },
    /// A vendor installer is unpacking (Forge/NeoForge).
    InstallingLoader { distribution: String },
    /// Writing eula/properties/launch files.
    Configuring,
    /// The first launch that generates the world, if the game needs one.
    GeneratingWorld,
    Ready,
    Failed { code: ErrorCode },
}

/// Callback the adapter uses to report progress. Cloneable and thread-safe so
/// it can be handed to download loops on other tasks.
#[derive(Clone)]
pub struct ProgressSink(Arc<dyn Fn(InstallProgress) + Send + Sync>);

impl ProgressSink {
    pub fn new(f: impl Fn(InstallProgress) + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    /// A sink that drops everything, for tests and headless calls.
    pub fn noop() -> Self {
        Self::new(|_| {})
    }

    pub fn report(&self, progress: InstallProgress) {
        (self.0)(progress);
    }
}

impl std::fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProgressSink")
    }
}

/// Everything an adapter needs in order to install into a directory.
pub struct InstallContext<'a> {
    /// The instance directory. Already created and empty on a fresh install.
    pub root: &'a Path,
    pub build: &'a BuildSelector,
    /// Resolved Java executable, when the manifest asks for a JVM.
    pub java: Option<PathBuf>,
    pub progress: ProgressSink,
}

/// What an install produced, recorded on the instance so a later launch does
/// not have to re-derive it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallReport {
    /// Resolved build, with the loader version filled in if it was "latest".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_build: Option<BuildSelector>,
    /// Entry point relative to the instance root: a jar, an arg file, an exe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_point: Option<String>,
    /// Java major version this build needs, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_major: Option<u32>,
}

/// Everything an adapter needs in order to produce a launch command.
pub struct LaunchContext<'a> {
    pub root: &'a Path,
    pub build: &'a BuildSelector,
    pub install: &'a InstallReport,
    pub java: Option<PathBuf>,
    pub memory_mb: u32,
    pub extra_args: &'a [String],
}

/// A fully resolved "how to run this" — the only thing the runtime layer sees.
/// It has no idea which game it is starting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchPlan {
    pub program: String,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[async_trait]
pub trait GameAdapter: Send + Sync {
    fn manifest(&self) -> &GameManifest;

    fn id(&self) -> &str {
        &self.manifest().id
    }

    /// Selectable game versions for a distribution, newest first.
    async fn versions(&self, distribution: &str) -> AppResult<Vec<VersionInfo>>;

    /// Loader/build versions for a distribution that has them. Distributions
    /// without a loader version return an empty list.
    async fn loader_versions(
        &self,
        _distribution: &str,
        _version: &str,
    ) -> AppResult<Vec<VersionInfo>> {
        Ok(Vec::new())
    }

    /// Download and lay out the server files. Must be safe to re-run over an
    /// existing instance (upgrade in place) without destroying world data.
    async fn install(&self, ctx: InstallContext<'_>) -> AppResult<InstallReport>;

    /// Build the launch command. Pure: no network, no installs.
    fn launch_plan(&self, ctx: LaunchContext<'_>) -> AppResult<LaunchPlan>;
}

/// Resolves game ids to adapters. Built once at startup and shared.
#[derive(Clone, Default)]
pub struct GameRegistry {
    adapters: BTreeMap<String, Arc<dyn GameAdapter>>,
}

impl GameRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The set of games this build ships with.
    pub fn with_builtin_games() -> Self {
        let mut registry = Self::new();
        registry.register(Arc::new(crate::game::minecraft::MinecraftAdapter::new()));
        registry
    }

    pub fn register(&mut self, adapter: Arc<dyn GameAdapter>) {
        self.adapters.insert(adapter.id().to_string(), adapter);
    }

    pub fn get(&self, id: &str) -> AppResult<Arc<dyn GameAdapter>> {
        self.adapters
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorCode::GameNotSupported).with("game", id))
    }

    pub fn manifest(&self, id: &str) -> AppResult<GameManifest> {
        Ok(self.get(id)?.manifest().clone())
    }

    /// Every manifest, for the "add a server" game picker.
    pub fn manifests(&self) -> Vec<GameManifest> {
        self.adapters.values().map(|a| a.manifest().clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_exposes_minecraft() {
        let registry = GameRegistry::with_builtin_games();
        let manifest = registry.manifest(crate::game::MINECRAFT).expect("minecraft is builtin");
        assert_eq!(manifest.id, "minecraft");
        assert!(!manifest.distributions.is_empty());
    }

    #[test]
    fn unknown_game_is_a_typed_error_not_a_panic() {
        let registry = GameRegistry::with_builtin_games();
        let Err(err) = registry.get("valheim") else { panic!("valheim is not registered yet") };
        assert_eq!(err.code, ErrorCode::GameNotSupported);
        assert_eq!(err.params.get("game").map(String::as_str), Some("valheim"));
    }
}
