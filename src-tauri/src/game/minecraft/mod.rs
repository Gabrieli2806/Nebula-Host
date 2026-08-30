//! Minecraft: Java Edition.
//!
//! The manifest is a JSON file — the same file a second game would ship. Only
//! the parts a manifest cannot express live here as code: the version APIs
//! (Mojang, PaperMC Fill v3, Fabric meta, Forge and NeoForge Maven, Purpur)
//! and running the Forge-family installer jar.

pub mod addons;
pub mod downloader;
pub mod launch;
pub mod types;

use std::path::PathBuf;
use std::sync::OnceLock;

use async_trait::async_trait;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::manifest::GameManifest;
use crate::game::registry::{
    GameAdapter, InstallContext, InstallReport, LaunchContext, LaunchPlan,
};
use crate::game::VersionInfo;
use crate::runtime::java;

use downloader::Downloader;
use types::ServerType;

const MANIFEST_JSON: &str = include_str!("../../../games/minecraft.json");

/// The parsed manifest, shared by everything that needs it.
pub fn manifest() -> GameManifest {
    static PARSED: OnceLock<GameManifest> = OnceLock::new();
    PARSED
        .get_or_init(|| {
            serde_json::from_str(MANIFEST_JSON).expect("bundled minecraft.json is valid")
        })
        .clone()
}

pub struct MinecraftAdapter {
    manifest: GameManifest,
    downloader: Downloader,
}

impl MinecraftAdapter {
    pub fn new() -> Self {
        Self { manifest: manifest(), downloader: Downloader::new() }
    }

    /// The JVM this build needs, resolved from the user's settings.
    fn java_for(&self, version: &str, override_path: Option<&str>) -> AppResult<PathBuf> {
        java::resolve_java(override_path, launch::min_java_version(version))
    }
}

impl Default for MinecraftAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GameAdapter for MinecraftAdapter {
    fn manifest(&self) -> &GameManifest {
        &self.manifest
    }

    async fn versions(&self, distribution: &str) -> AppResult<Vec<VersionInfo>> {
        let server_type = ServerType::parse(distribution)?;
        let versions = self.downloader.get_versions(server_type).await?;
        Ok(versions
            .into_iter()
            .map(|id| {
                let stable = is_release(&id);
                VersionInfo {
                    channel: if stable { "release".into() } else { "snapshot".into() },
                    stable,
                    id,
                    released_at: None,
                }
            })
            .collect())
    }

    async fn loader_versions(
        &self,
        distribution: &str,
        version: &str,
    ) -> AppResult<Vec<VersionInfo>> {
        let server_type = ServerType::parse(distribution)?;
        let ids = match server_type {
            ServerType::Fabric => self.downloader.get_fabric_loader_versions().await?,
            ServerType::Forge => self.downloader.get_forge_loader_versions(version).await?,
            ServerType::NeoForge => self.downloader.get_neoforge_loader_versions(version).await?,
            _ => Vec::new(),
        };
        Ok(ids
            .into_iter()
            .map(|id| VersionInfo { id, channel: "release".into(), stable: true, released_at: None })
            .collect())
    }

    async fn install(&self, ctx: InstallContext<'_>) -> AppResult<InstallReport> {
        let server_type = ServerType::parse(&ctx.build.distribution)?;
        let java_path = match &ctx.java {
            Some(path) => Some(path.clone()),
            None => Some(self.java_for(&ctx.build.version, None)?),
        };

        std::fs::create_dir_all(ctx.root)?;

        let resolved_loader = self
            .downloader
            .install(
                server_type,
                &ctx.build.version,
                ctx.build.loader_version.as_deref(),
                ctx.root,
                java_path.as_deref().and_then(|p| p.to_str()),
                ctx.progress.clone(),
            )
            .await?;

        // Minecraft refuses to start until the EULA is accepted; the UI asks
        // before we ever get here.
        launch::write_eula(ctx.root)?;

        let entry_point = launch::installer_arg_file(ctx.root, true)
            .or_else(|| ctx.root.join("server.jar").exists().then(|| "server.jar".to_string()));

        let mut resolved_build = ctx.build.clone();
        resolved_build.loader_version = resolved_loader.or(ctx.build.loader_version.clone());

        Ok(InstallReport {
            resolved_build: Some(resolved_build),
            entry_point,
            java_major: Some(launch::min_java_version(&ctx.build.version)),
        })
    }

    fn launch_plan(&self, ctx: LaunchContext<'_>) -> AppResult<LaunchPlan> {
        let server_type = ServerType::parse(&ctx.build.distribution)?;
        let java_path = match &ctx.java {
            Some(path) => path.clone(),
            None => self.java_for(&ctx.build.version, None)?,
        };

        let mut args = launch::jvm_args(ctx.memory_mb, ctx.extra_args);
        args.extend(launch::entry_args(server_type, ctx.root, true)?);

        Ok(LaunchPlan {
            program: java_path.to_string_lossy().to_string(),
            args,
            working_dir: ctx.root.to_path_buf(),
            env: Default::default(),
        })
    }
}

/// `1.21.4` is a release; `24w45a` and `1.21.4-pre1` are not.
fn is_release(id: &str) -> bool {
    id.split('.').next().is_some_and(|head| head.chars().all(|c| c.is_ascii_digit()))
        && !id.contains("pre")
        && !id.contains("rc")
}

/// The console command that asks this distribution for its tick rate.
pub fn tick_rate_command(manifest: &GameManifest, distribution: &str) -> Option<String> {
    manifest.distribution(distribution)?.tick_rate_command.clone()
}

/// Where addons for this distribution are installed, if it takes any.
pub fn addon_dir(manifest: &GameManifest, distribution: &str) -> AppResult<PathBuf> {
    let dist = manifest
        .distribution(distribution)
        .ok_or_else(|| AppError::new(ErrorCode::GameNotSupported).with("distribution", distribution))?;
    dist.addon_dir
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| AppError::new(ErrorCode::GameNotSupported).with("distribution", distribution))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_manifest_parses() {
        let manifest = manifest();
        assert_eq!(manifest.id, "minecraft");
        assert_eq!(manifest.distributions.len(), 7);
        assert_eq!(manifest.primary_port().expect("a port").default, 25565);
    }

    #[test]
    fn tick_rate_command_is_distribution_specific() {
        // NeoForge rejects "forge tps"; it needs its own command.
        let manifest = manifest();
        assert_eq!(tick_rate_command(&manifest, "forge").as_deref(), Some("forge tps"));
        assert_eq!(tick_rate_command(&manifest, "neoforge").as_deref(), Some("neoforge tps"));
        assert_eq!(tick_rate_command(&manifest, "paper").as_deref(), Some("tps"));
        assert_eq!(tick_rate_command(&manifest, "vanilla"), None);
    }

    #[test]
    fn addon_dir_differs_between_mods_and_plugins() {
        let manifest = manifest();
        assert_eq!(addon_dir(&manifest, "fabric").expect("mods"), PathBuf::from("mods"));
        assert_eq!(addon_dir(&manifest, "paper").expect("plugins"), PathBuf::from("plugins"));
        assert!(addon_dir(&manifest, "vanilla").is_err(), "vanilla takes no addons");
    }

    #[test]
    fn release_detection_ignores_snapshots_and_prereleases() {
        assert!(is_release("1.21.4"));
        assert!(!is_release("24w45a"));
        assert!(!is_release("1.21.4-pre1"));
        assert!(!is_release("1.21-rc1"));
    }

    #[test]
    fn every_distribution_that_takes_addons_names_a_directory() {
        for dist in manifest().distributions {
            assert_eq!(
                dist.addon_loaders.is_empty(),
                dist.addon_dir.is_none(),
                "{} must declare both an addon ecosystem and a directory, or neither",
                dist.id
            );
        }
    }
}
