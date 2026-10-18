//! Mods and plugins for one instance.
//!
//! Which ecosystem an instance takes — project type, loader tags, folder —
//! comes from its distribution in the game manifest, so this layer never asks
//! "is it Fabric?". The catalogue itself is Modrinth.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::minecraft::addons as files;
use crate::game::minecraft::modrinth::{
    AddonBrowseResult, AddonDependency, AddonInstallResult, AddonProfile, AddonSort,
    AddonVersion, InstalledAddonsReport, Modrinth, ModrinthStatus,
};
use crate::model::Instance;
use crate::service::pending::{PendingAction, PendingAddonChange};
use crate::service::InstanceService;

/// What the addons panel needs to know before it asks for anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonContext {
    /// False for distributions that take no addons (Vanilla).
    pub supported: bool,
    /// `mod` or `plugin`.
    pub project_type: Option<String>,
    pub loaders: Vec<String>,
    pub game_version: String,
    /// Folder relative to the instance root (`mods`, `plugins`).
    pub addon_dir: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseRequest {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub sort: AddonSort,
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub limit: Option<u32>,
    /// Extra Modrinth category tags to require (`utility`, `worldgen`).
    #[serde(default)]
    pub categories: Vec<String>,
}

impl InstanceService {
    pub(crate) fn addon_profile(&self, instance: &Instance) -> AppResult<(PathBuf, AddonProfile)> {
        let manifest = self.registry.manifest(&instance.game)?;
        let unsupported =
            || AppError::new(ErrorCode::GameNotSupported).with("distribution", &instance.build.distribution);
        let dist = manifest.distribution(&instance.build.distribution).ok_or_else(unsupported)?;
        let dir = dist.addon_dir.as_ref().ok_or_else(unsupported)?;
        let project_type = dist.addon_project_type.clone().ok_or_else(unsupported)?;
        if dist.addon_loaders.is_empty() {
            return Err(unsupported());
        }
        Ok((
            instance.path.join(dir),
            AddonProfile {
                project_type,
                loaders: dist.addon_loaders.clone(),
                game_version: instance.build.version.clone(),
            },
        ))
    }

    pub async fn addon_context(&self, instance_id: &str) -> AppResult<AddonContext> {
        let instance = self.store.get(instance_id).await?;
        let manifest = self.registry.manifest(&instance.game)?;
        let dist = manifest.distribution(&instance.build.distribution);
        let supported = self.addon_profile(&instance).is_ok();
        Ok(AddonContext {
            supported,
            project_type: dist.and_then(|d| d.addon_project_type.clone()),
            loaders: dist.map(|d| d.addon_loaders.clone()).unwrap_or_default(),
            game_version: instance.build.version.clone(),
            addon_dir: dist.and_then(|d| d.addon_dir.clone()),
        })
    }

    /// Every jar in the addon folder, identified by hash where Modrinth
    /// knows it. Also refreshes the "installed" index search results use.
    pub async fn addons_list(&self, instance_id: &str) -> AppResult<InstalledAddonsReport> {
        let instance = self.store.get(instance_id).await?;
        let (dir, profile) = self.addon_profile(&instance)?;
        let report = Modrinth::new().identify_dir(&dir, &profile).await?;

        let index: HashMap<String, String> = report
            .addons
            .iter()
            .filter_map(|a| a.modrinth.as_ref().map(|m| (m.project.project_id.clone(), a.file_name.clone())))
            .collect();
        // Only a lookup that worked describes what is installed. When
        // Modrinth did not answer every jar reads as unknown, and caching
        // that empty map would make a later install sit next to the same
        // project's jar instead of replacing it.
        if report.modrinth_status == ModrinthStatus::Ok {
            self.addon_index.write().await.insert(instance_id.to_string(), index);
        } else {
            self.addon_index.write().await.remove(instance_id);
        }
        Ok(report)
    }

    async fn installed_projects(&self, instance_id: &str) -> HashMap<String, String> {
        if let Some(index) = self.addon_index.read().await.get(instance_id) {
            return index.clone();
        }
        // First visit straight to the catalogue: identify once, quietly.
        match self.addons_list(instance_id).await {
            Ok(_) => self.addon_index.read().await.get(instance_id).cloned().unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    pub async fn addons_browse(&self, instance_id: &str, request: BrowseRequest) -> AppResult<AddonBrowseResult> {
        let instance = self.store.get(instance_id).await?;
        let (_, profile) = self.addon_profile(&instance)?;
        let installed = self.installed_or_pending(instance_id).await;
        Modrinth::new()
            .search(
                &profile,
                &request.query,
                request.sort,
                request.offset,
                request.limit.unwrap_or(20),
                &request.categories,
                &installed,
            )
            .await
    }

    pub async fn addon_versions(&self, instance_id: &str, project_id: &str, all: bool) -> AppResult<Vec<AddonVersion>> {
        let instance = self.store.get(instance_id).await?;
        let (_, profile) = self.addon_profile(&instance)?;
        Modrinth::new().project_versions(&profile, project_id, !all).await
    }

    pub async fn addon_dependencies(&self, instance_id: &str, version_id: &str) -> AppResult<Vec<AddonDependency>> {
        let instance = self.store.get(instance_id).await?;
        let (_, profile) = self.addon_profile(&instance)?;
        let client = Modrinth::new();
        let version = client.version(&profile, version_id).await?;
        let installed = self.installed_or_pending(instance_id).await;
        client.dependencies(&profile, &version, &installed).await
    }

    /// Projects already in the folder, plus those waiting to go in: neither
    /// should be offered for install again.
    async fn installed_or_pending(&self, instance_id: &str) -> HashSet<String> {
        let mut installed: HashSet<String> = self.installed_projects(instance_id).await.into_keys().collect();
        installed.extend(self.pending_projects(instance_id).await);
        installed
    }

    /// Install a project: the given version, or the best compatible one.
    /// If a jar of the same project is already there it is replaced (that is
    /// what makes this an update); `replace` names one explicitly.
    pub async fn addon_install_version(
        &self,
        instance_id: &str,
        project_id: &str,
        version_id: Option<&str>,
        replace: Option<&str>,
    ) -> AppResult<AddonInstallResult> {
        let instance = self.store.get(instance_id).await?;
        let (dir, profile) = self.addon_profile(&instance)?;
        let client = Modrinth::new();

        let version = match version_id {
            Some(id) => client.version(&profile, id).await?,
            None => client.best_version(&profile, project_id).await?,
        };
        if version.project_id != project_id {
            return Err(AppError::new(ErrorCode::InvalidInput).with("version", &version.id));
        }

        // Which jar (if any) this replaces, read from the folder as it is now
        // rather than from the cached index: a file deleted or renamed from
        // the Files panel would otherwise leave the index stale. Hashes are
        // cached by size and mtime, so this costs little.
        let existing = match replace {
            Some(name) => Some(name.to_string()),
            None => {
                let report = self.addons_list(instance_id).await?;
                if report.modrinth_status != ModrinthStatus::Ok {
                    // Without a lookup there is no telling whether this
                    // project is already there; installing anyway risks two
                    // copies of it side by side.
                    return Err(AppError::new(ErrorCode::UpstreamUnavailable).with("service", "modrinth"));
                }
                report
                    .addons
                    .iter()
                    .find(|a| a.modrinth.as_ref().is_some_and(|m| m.project.project_id == project_id))
                    .map(|a| a.file_name.clone())
            }
        };
        // Choosing a version for a switched-off addon keeps it switched off,
        // as an update does.
        let keep_disabled = existing.as_deref().is_some_and(files::is_disabled_name);
        if self.addons_locked(instance_id).await {
            return self.queue_install(instance_id, &client, &version, existing, keep_disabled, None).await;
        }
        let mut result = client.install(&dir, &version, existing.as_deref()).await?;
        if keep_disabled {
            result.file_name = files::set_enabled(&dir, &result.file_name, false)?;
        }
        self.after_addon_change(instance_id).await;
        Ok(result)
    }

    /// Update one installed jar to its latest compatible version.
    pub async fn addon_update(&self, instance_id: &str, file_name: &str) -> AppResult<AddonInstallResult> {
        let report = self.addons_list(instance_id).await?;
        let entry = report
            .addons
            .iter()
            .find(|a| a.file_name == file_name)
            .ok_or_else(|| AppError::new(ErrorCode::FileNotFound).with("file", file_name))?;
        let found = entry
            .modrinth
            .as_ref()
            .ok_or_else(|| AppError::new(ErrorCode::NotFound).with("file", file_name))?;
        let latest = found
            .latest_version
            .as_ref()
            .filter(|_| found.update_available)
            .ok_or_else(|| AppError::new(ErrorCode::VersionNotFound).with("file", file_name))?;

        let instance = self.store.get(instance_id).await?;
        let (dir, _) = self.addon_profile(&instance)?;
        if self.addons_locked(instance_id).await {
            let title = Some(found.project.title.clone());
            return self
                .queue_install(instance_id, &Modrinth::new(), latest, Some(file_name.to_string()), !entry.enabled, title)
                .await;
        }
        let mut result = Modrinth::new().install(&dir, latest, Some(file_name)).await?;
        // An update keeps a disabled addon disabled.
        if !entry.enabled {
            result.file_name = files::set_enabled(&dir, &result.file_name, false)?;
        }
        self.after_addon_change(instance_id).await;
        Ok(result)
    }

    pub async fn addon_toggle(&self, instance_id: &str, file_name: &str, enabled: bool) -> AppResult<String> {
        let instance = self.store.get(instance_id).await?;
        let (dir, _) = self.addon_profile(&instance)?;
        if self.addons_locked(instance_id).await {
            let path = crate::store::paths::resolve_inside(&dir, file_name)?;
            if !path.is_file() {
                return Err(AppError::new(ErrorCode::FileNotFound).with("file", file_name));
            }
            let action = PendingAction::SetEnabled { file_name: file_name.to_string(), enabled };
            self.queue_addon_change(instance_id, PendingAddonChange::new(action, None)).await?;
            // The file keeps its name until the change applies.
            return Ok(file_name.to_string());
        }
        let name = files::set_enabled(&dir, file_name, enabled)?;
        self.after_addon_change(instance_id).await;
        Ok(name)
    }

    pub async fn addon_delete(&self, instance_id: &str, file_name: &str) -> AppResult<()> {
        let instance = self.store.get(instance_id).await?;
        let (dir, _) = self.addon_profile(&instance)?;
        let path = crate::store::paths::resolve_inside(&dir, file_name)?;
        if !path.is_file() {
            return Err(AppError::new(ErrorCode::FileNotFound).with("file", file_name));
        }
        if self.addons_locked(instance_id).await {
            let action = PendingAction::Remove { file_name: file_name.to_string() };
            self.queue_addon_change(instance_id, PendingAddonChange::new(action, None)).await?;
            return Ok(());
        }
        std::fs::remove_file(path)?;
        self.after_addon_change(instance_id).await;
        Ok(())
    }

    /// The server is running: download the jar now, into staging, and queue
    /// the move into the addon folder for when it stops.
    async fn queue_install(
        &self,
        instance_id: &str,
        client: &Modrinth,
        version: &AddonVersion,
        replace: Option<String>,
        disabled: bool,
        title: Option<String>,
    ) -> AppResult<AddonInstallResult> {
        let staging = self.pending_staging(instance_id)?;
        let (staged, file_name) = client.stage(&staging, version).await?;
        let staged_name = staged.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let title = match title {
            Some(title) => Some(title),
            None => client.projects(std::slice::from_ref(&version.project_id)).await.ok().and_then(|p| p.into_iter().next()).map(|p| p.title),
        };
        let action = PendingAction::Install {
            file_name: file_name.clone(),
            staged: staged_name,
            project_id: Some(version.project_id.clone()),
            version_number: Some(version.version_number.clone()),
            replace: replace.clone(),
            disabled,
        };
        self.queue_addon_change(instance_id, PendingAddonChange::new(action, title)).await?;
        Ok(AddonInstallResult {
            file_name,
            project_id: version.project_id.clone(),
            version_id: version.id.clone(),
            version_number: version.version_number.clone(),
            replaced: replace,
            pending: true,
        })
    }

    /// Anything that changes the addon folder makes the installed index and
    /// the command list stale.
    pub async fn after_addon_change(&self, instance_id: &str) {
        self.addon_index.write().await.remove(instance_id);
        self.invalidate_completions(instance_id).await;
    }
}
