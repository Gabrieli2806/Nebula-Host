//! File browsing inside one instance.

use crate::error::AppResult;
use crate::ipc::State;
use crate::service::files::{self, FileEntry};

#[tauri::command]
pub async fn files_list(
    state: State<'_>,
    instance_id: String,
    path: Option<String>,
) -> AppResult<Vec<FileEntry>> {
    let instance = state.store.get(&instance_id).await?;
    files::list(&instance.path, path.as_deref().unwrap_or(""))
}

#[tauri::command]
pub async fn file_read(state: State<'_>, instance_id: String, path: String) -> AppResult<String> {
    let instance = state.store.get(&instance_id).await?;
    files::read_text(&instance.path, &path)
}

#[tauri::command]
pub async fn file_write(
    state: State<'_>,
    instance_id: String,
    path: String,
    contents: String,
) -> AppResult<()> {
    let instance = state.store.get(&instance_id).await?;
    files::write_text(&instance.path, &path, &contents)
}

#[tauri::command]
pub async fn file_delete(state: State<'_>, instance_id: String, path: String) -> AppResult<()> {
    let instance = state.store.get(&instance_id).await?;
    files::delete(&instance.path, &path)
}

#[tauri::command]
pub async fn file_rename(
    state: State<'_>,
    instance_id: String,
    path: String,
    new_name: String,
) -> AppResult<()> {
    let instance = state.store.get(&instance_id).await?;
    files::rename(&instance.path, &path, &new_name)
}

#[tauri::command]
pub async fn folder_create(state: State<'_>, instance_id: String, path: String) -> AppResult<()> {
    let instance = state.store.get(&instance_id).await?;
    files::create_dir(&instance.path, &path)
}

/// Open the instance folder in the system file manager.
#[tauri::command]
pub async fn instance_reveal(state: State<'_>, instance_id: String) -> AppResult<String> {
    let instance = state.store.get(&instance_id).await?;
    Ok(instance.path.to_string_lossy().to_string())
}
