//! Mods and plugins.
//!
//! Which ecosystem an instance takes comes from its distribution in the game
//! manifest, so this layer never asks "is it Fabric?".
//!
//! The `addons_context` / `addons_list` / `addons_browse` / `addon_versions`
//! / `addon_dependencies` / `addon_install_version` / `addon_update` set is
//! the current API. `addons_search`, `addons_installed` and `addon_install`
//! are the round-2 API, kept until the panel has moved over.

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::minecraft::addons::{self, AddonClient};
use crate::game::minecraft::types::{AddonModrinthMeta, AddonSearchResult, InstalledAddon, ServerType};
use crate::game::minecraft::modrinth::{
    AddonBrowseResult, AddonDependency, AddonInstallResult, AddonVersion, InstalledAddonsReport,
};
use crate::ipc::State;
use crate::service::addons::{AddonContext, BrowseRequest};
use crate::service::pending::PendingAddonChange;

#[tauri::command]
pub async fn addons_context(state: State<'_>, instance_id: String) -> AppResult<AddonContext> {
    state.addon_context(&instance_id).await
}

#[tauri::command]
pub async fn addons_list(state: State<'_>, instance_id: String) -> AppResult<InstalledAddonsReport> {
    state.addons_list(&instance_id).await
}

#[tauri::command]
pub async fn addons_browse(
    state: State<'_>,
    instance_id: String,
    request: BrowseRequest,
) -> AppResult<AddonBrowseResult> {
    state.addons_browse(&instance_id, request).await
}

#[tauri::command]
pub async fn addon_versions(
    state: State<'_>,
    instance_id: String,
    project_id: String,
    all: Option<bool>,
) -> AppResult<Vec<AddonVersion>> {
    state.addon_versions(&instance_id, &project_id, all.unwrap_or(false)).await
}

#[tauri::command]
pub async fn addon_dependencies(
    state: State<'_>,
    instance_id: String,
    version_id: String,
) -> AppResult<Vec<AddonDependency>> {
    state.addon_dependencies(&instance_id, &version_id).await
}

#[tauri::command]
pub async fn addon_install_version(
    state: State<'_>,
    instance_id: String,
    project_id: String,
    version_id: Option<String>,
    replace: Option<String>,
) -> AppResult<AddonInstallResult> {
    state
        .addon_install_version(&instance_id, &project_id, version_id.as_deref(), replace.as_deref())
        .await
}

#[tauri::command]
pub async fn addon_update(
    state: State<'_>,
    instance_id: String,
    file_name: String,
) -> AppResult<AddonInstallResult> {
    state.addon_update(&instance_id, &file_name).await
}

/// Resolve an instance to its addon directory and loader.
async fn legacy_context(
    state: &State<'_>,
    instance_id: &str,
) -> AppResult<(std::path::PathBuf, ServerType, String)> {
    let instance = state.store.get(instance_id).await?;
    let manifest = state.registry.manifest(&instance.game)?;

    let dist = manifest
        .distribution(&instance.build.distribution)
        .ok_or_else(|| AppError::new(ErrorCode::GameNotSupported).with("game", &instance.game))?;
    let dir = dist
        .addon_dir
        .as_ref()
        .ok_or_else(|| {
            AppError::new(ErrorCode::GameNotSupported).with("distribution", &dist.id)
        })?;

    let server_type = ServerType::parse(&instance.build.distribution)?;
    Ok((instance.path.join(dir), server_type, instance.build.version.clone()))
}

#[tauri::command]
pub async fn addons_search(
    state: State<'_>,
    instance_id: String,
    query: String,
    offset: Option<u32>,
    limit: Option<u32>,
) -> AppResult<AddonSearchResult> {
    let (_, server_type, version) = legacy_context(&state, &instance_id).await?;
    AddonClient::new()
        .search(&query, server_type, &version, offset.unwrap_or(0), limit.unwrap_or(20))
        .await
}

#[tauri::command]
pub async fn addons_installed(
    state: State<'_>,
    instance_id: String,
) -> AppResult<(Vec<InstalledAddon>, Vec<AddonModrinthMeta>)> {
    let (dir, server_type, version) = legacy_context(&state, &instance_id).await?;
    let installed = addons::list_installed(&dir)?;
    // Upstream metadata is a nicety: a failed lookup must not hide the jars
    // the user can see in their own folder.
    let known = AddonClient::new()
        .identify_installed(&dir, server_type, &version)
        .await
        .unwrap_or_default();
    Ok((installed, known))
}

#[tauri::command]
pub async fn addon_install(
    state: State<'_>,
    instance_id: String,
    url: String,
    file_name: String,
) -> AppResult<()> {
    let (dir, _, _) = legacy_context(&state, &instance_id).await?;
    AddonClient::new().install(&url, &dir, &file_name).await?;
    state.after_addon_change(&instance_id).await;
    Ok(())
}

#[tauri::command]
pub async fn addon_set_enabled(
    state: State<'_>,
    instance_id: String,
    file_name: String,
    enabled: bool,
) -> AppResult<String> {
    state.addon_toggle(&instance_id, &file_name, enabled).await
}

#[tauri::command]
pub async fn addon_remove(
    state: State<'_>,
    instance_id: String,
    file_name: String,
) -> AppResult<()> {
    state.addon_delete(&instance_id, &file_name).await
}

/// Addon changes waiting for the server to stop.
#[tauri::command]
pub async fn addons_pending(state: State<'_>, instance_id: String) -> AppResult<Vec<PendingAddonChange>> {
    state.pending_addons(&instance_id).await
}

#[tauri::command]
pub async fn addon_pending_cancel(
    state: State<'_>,
    instance_id: String,
    change_id: String,
) -> AppResult<Vec<PendingAddonChange>> {
    state.cancel_pending_addon(&instance_id, &change_id).await
}

/// Try the queue again now (it applies on its own when the server stops;
/// this is for changes that failed then). Does nothing while it runs.
#[tauri::command]
pub async fn addons_pending_apply(state: State<'_>, instance_id: String) -> AppResult<Vec<PendingAddonChange>> {
    state.apply_pending_addons(&instance_id).await
}
