//! Who is on, who is allowed, who is banned.

use crate::error::AppResult;
use crate::ipc::State;
use crate::service::players::{self, PlayerEntry, PlayerLists};

#[tauri::command]
pub async fn players_online(state: State<'_>, instance_id: String) -> AppResult<Vec<String>> {
    Ok(state.online_players(&instance_id).await)
}

#[tauri::command]
pub async fn players_lists(state: State<'_>, instance_id: String) -> AppResult<PlayerLists> {
    let instance = state.store.get(&instance_id).await?;
    let manifest = state.registry.manifest(&instance.game)?;
    players::read_lists(&instance.path, &manifest.players)
}

#[tauri::command]
pub async fn players_write_list(
    state: State<'_>,
    instance_id: String,
    list: String,
    entries: Vec<PlayerEntry>,
) -> AppResult<()> {
    let instance = state.store.get(&instance_id).await?;
    let manifest = state.registry.manifest(&instance.game)?;
    players::write_list(&instance.path, &manifest.players, &list, &entries)
}
