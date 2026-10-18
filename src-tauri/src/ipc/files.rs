//! File browsing inside one instance.

use crate::error::AppResult;
use crate::ipc::State;
use crate::service::files::{self, DiskUsage, FileEntry};
use crate::service::import::{self, ConflictPolicy, ImportCheck, ImportSummary};

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
    files::write_text(&instance.path, &path, &contents)?;
    state.after_folder_change(&instance_id).await;
    Ok(())
}

#[tauri::command]
pub async fn file_delete(state: State<'_>, instance_id: String, path: String) -> AppResult<()> {
    let instance = state.store.get(&instance_id).await?;
    files::delete(&instance.path, &path)?;
    state.after_folder_change(&instance_id).await;
    Ok(())
}

#[tauri::command]
pub async fn file_rename(
    state: State<'_>,
    instance_id: String,
    path: String,
    new_name: String,
) -> AppResult<()> {
    let instance = state.store.get(&instance_id).await?;
    files::rename(&instance.path, &path, &new_name)?;
    state.after_folder_change(&instance_id).await;
    Ok(())
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

/// Dry run of a drop: what would land where, and what already exists.
#[tauri::command]
pub async fn files_import_check(
    state: State<'_>,
    instance_id: String,
    target: String,
    sources: Vec<String>,
) -> AppResult<ImportCheck> {
    let instance = state.store.get(&instance_id).await?;
    tokio::task::spawn_blocking(move || import::check(&instance.path, &target, &sources))
        .await
        .map_err(crate::error::AppError::unexpected)?
}

/// Copy dropped files and folders (absolute OS paths) into `target`, an
/// instance-relative folder. Progress arrives as `import` events tagged with
/// `import_id`; the summary is the return value.
#[tauri::command]
pub async fn files_import(
    state: State<'_>,
    instance_id: String,
    target: String,
    sources: Vec<String>,
    conflict: Option<ConflictPolicy>,
    import_id: Option<String>,
) -> AppResult<ImportSummary> {
    let import_id = import_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    state
        .import_files(&instance_id, target, sources, conflict.unwrap_or_default(), import_id)
        .await
}

/// Disk used by the instance folder. Cached for a minute.
#[tauri::command]
pub async fn instance_disk_usage(
    state: State<'_>,
    instance_id: String,
    refresh: Option<bool>,
) -> AppResult<DiskUsage> {
    state.disk_usage(&instance_id, refresh.unwrap_or(false)).await
}
