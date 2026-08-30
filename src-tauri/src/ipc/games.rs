//! What can be installed.

use crate::error::AppResult;
use crate::game::{GameManifest, VersionInfo};
use crate::ipc::State;

#[tauri::command]
pub async fn games_list(state: State<'_>) -> AppResult<Vec<GameManifest>> {
    Ok(state.registry.manifests())
}

#[tauri::command]
pub async fn game_versions(
    state: State<'_>,
    game: String,
    distribution: String,
) -> AppResult<Vec<VersionInfo>> {
    state.registry.get(&game)?.versions(&distribution).await
}

#[tauri::command]
pub async fn game_loader_versions(
    state: State<'_>,
    game: String,
    distribution: String,
    version: String,
) -> AppResult<Vec<VersionInfo>> {
    state.registry.get(&game)?.loader_versions(&distribution, &version).await
}
