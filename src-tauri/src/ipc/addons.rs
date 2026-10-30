//! Mods and plugins.
//!
//! Which ecosystem an instance takes comes from its distribution in the game
//! manifest, so this layer never asks "is it Fabric?".

use crate::error::AppResult;
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
