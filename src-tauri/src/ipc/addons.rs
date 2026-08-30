//! Mods and plugins.
//!
//! Which ecosystem an instance takes comes from its distribution in the game
//! manifest, so this layer never asks "is it Fabric?".

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::minecraft::addons::{self, AddonClient};
use crate::game::minecraft::types::{AddonModrinthMeta, AddonSearchResult, InstalledAddon, ServerType};
use crate::ipc::State;

/// Resolve an instance to its addon directory and loader.
async fn addon_context(
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
    let (_, server_type, version) = addon_context(&state, &instance_id).await?;
    AddonClient::new()
        .search(&query, server_type, &version, offset.unwrap_or(0), limit.unwrap_or(20))
        .await
}

#[tauri::command]
pub async fn addons_installed(
    state: State<'_>,
    instance_id: String,
) -> AppResult<(Vec<InstalledAddon>, Vec<AddonModrinthMeta>)> {
    let (dir, server_type, version) = addon_context(&state, &instance_id).await?;
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
    let (dir, _, _) = addon_context(&state, &instance_id).await?;
    AddonClient::new().install(&url, &dir, &file_name).await
}

#[tauri::command]
pub async fn addon_set_enabled(
    state: State<'_>,
    instance_id: String,
    file_name: String,
    enabled: bool,
) -> AppResult<String> {
    let (dir, _, _) = addon_context(&state, &instance_id).await?;
    addons::set_enabled(&dir, &file_name, enabled)
}

#[tauri::command]
pub async fn addon_remove(
    state: State<'_>,
    instance_id: String,
    file_name: String,
) -> AppResult<()> {
    let (dir, _, _) = addon_context(&state, &instance_id).await?;
    let path = crate::store::paths::resolve_inside(&dir, &file_name)?;
    std::fs::remove_file(path)?;
    Ok(())
}
