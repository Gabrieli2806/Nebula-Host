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
    state.ensure_not_transferring(&instance_id).await?;
    let instance = state.store.get(&instance_id).await?;
    files::write_text(&instance.path, &path, &contents)?;
    state.after_folder_change(&instance_id).await;
    Ok(())
}

#[tauri::command]
pub async fn file_delete(state: State<'_>, instance_id: String, path: String) -> AppResult<()> {
    state.ensure_not_transferring(&instance_id).await?;
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
    state.ensure_not_transferring(&instance_id).await?;
    let instance = state.store.get(&instance_id).await?;
    files::rename(&instance.path, &path, &new_name)?;
    state.after_folder_change(&instance_id).await;
    Ok(())
}

#[tauri::command]
pub async fn folder_create(state: State<'_>, instance_id: String, path: String) -> AppResult<()> {
    state.ensure_not_transferring(&instance_id).await?;
    let instance = state.store.get(&instance_id).await?;
    files::create_dir(&instance.path, &path)
}

/// Open the instance folder in the system file manager.
///
/// Opened from here rather than from the webview: the opener plugin's JS
/// `openPath` is limited to a path scope, and instance folders live wherever
/// the user put them (a custom instances folder included), so the call from
/// the UI was refused and the button did nothing.
#[tauri::command]
pub async fn instance_open_folder(
    app: tauri::AppHandle,
    state: State<'_>,
    instance_id: String,
) -> AppResult<()> {
    let instance = state.store.get(&instance_id).await?;
    std::fs::create_dir_all(&instance.path)?;
    crate::ipc::open_folder(&app, &instance.path)
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

/// Import a world from a `.zip` or a folder on this PC.
#[tauri::command]
pub async fn world_import(
    state: State<'_>,
    instance_id: String,
    path: String,
    skip_backup: Option<bool>,
) -> AppResult<crate::service::world::WorldImportResult> {
    state
        .inner()
        .import_world(&instance_id, std::path::PathBuf::from(path), skip_backup.unwrap_or(false))
        .await
}

/// Stream a file on this PC to a paired node (its `/upload`), returning the
/// upload id `world_import` takes over there. Runs here, not in the webview,
/// so a multi-gigabyte world never passes through the UI.
#[tauri::command]
pub async fn upload_to_node(app: tauri::AppHandle, address: String, token: String, path: String) -> AppResult<String> {
    use tauri::Emitter;
    use crate::error::{AppError, ErrorCode};
    let file = tokio::fs::File::open(&path).await?;
    let length = file.metadata().await?.len();
    // Report bytes sent as a local event (`nebula:upload`): while managing a
    // remote machine the UI listens to that machine's events, not ours.
    let sent = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let last = std::sync::Arc::new(std::sync::Mutex::new(std::time::Instant::now()));
    let stream = {
        use futures_util::StreamExt;
        let (sent, last, app) = (sent.clone(), last.clone(), app.clone());
        tokio_util_stream(file).map(move |chunk| {
            if let Ok(bytes) = &chunk {
                let done = sent.fetch_add(bytes.len() as u64, std::sync::atomic::Ordering::Relaxed) + bytes.len() as u64;
                if let Ok(mut at) = last.lock() {
                    if at.elapsed().as_millis() >= 100 || done >= length {
                        *at = std::time::Instant::now();
                        let _ = app.emit("nebula:upload", serde_json::json!({ "sent": done, "total": length }));
                    }
                }
            }
            chunk
        })
    };
    let response = reqwest::Client::new()
        .post(format!("http://{address}/upload"))
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_LENGTH, length)
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await
        .map_err(|e| AppError::new(ErrorCode::NodeUnreachable).detail(e.to_string()))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::new(ErrorCode::Unexpected).detail(e.to_string()))?;
    if !status.is_success() {
        // The node answered with its own AppError JSON; surface it as detail.
        return Err(AppError::new(ErrorCode::Unexpected).with("status", status.as_u16()).detail(body.to_string()));
    }
    body.get("uploadId")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| AppError::new(ErrorCode::Unexpected).detail("no upload id"))
}

/// A tokio file as a stream of byte chunks (without pulling in tokio-util).
fn tokio_util_stream(
    file: tokio::fs::File,
) -> impl futures_util::Stream<Item = std::io::Result<Vec<u8>>> + Send + 'static {
    use tokio::io::AsyncReadExt;
    futures_util::stream::unfold(file, |mut file| async move {
        let mut buffer = vec![0u8; 256 * 1024];
        match file.read(&mut buffer).await {
            Ok(0) => None,
            Ok(n) => {
                buffer.truncate(n);
                Some((Ok(buffer), file))
            }
            Err(error) => Some((Err(error), file)),
        }
    })
}
