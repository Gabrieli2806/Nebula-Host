//! Backups and restore points.

use crate::error::AppResult;
use crate::ipc::State;
use crate::service::backups::{self, Backup};

#[tauri::command]
pub async fn backups_list(state: State<'_>, instance_id: String) -> AppResult<Vec<Backup>> {
    let _ = state.store.get(&instance_id).await?;
    backups::list(&instance_id)
}

#[tauri::command]
pub async fn backup_create(
    state: State<'_>,
    instance_id: String,
    label: Option<String>,
) -> AppResult<Backup> {
    backups::create(state.inner(), &instance_id, label).await
}

#[tauri::command]
pub async fn backup_restore(
    state: State<'_>,
    instance_id: String,
    backup_id: String,
) -> AppResult<()> {
    backups::restore(state.inner(), &instance_id, &backup_id).await
}

#[tauri::command]
pub async fn backup_delete(
    state: State<'_>,
    instance_id: String,
    backup_id: String,
) -> AppResult<()> {
    let _ = state.store.get(&instance_id).await?;
    backups::delete(&instance_id, &backup_id)
}
