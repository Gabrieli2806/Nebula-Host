//! Importing a world into a server, from a `.zip` or (on this machine) a
//! folder: a singleplayer save or the worlds of a previous server.
//!
//! The current world is backed up first, the import is unpacked into a
//! staging folder inside the instance (same disk, so moves are instant and a
//! failed import leaves the server untouched), and the game adapter decides
//! where the world goes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::registry::WorldImportReport;
use crate::service::{backups, InstanceService};
use crate::store::paths;

/// Archives bigger than this are refused before anything is unpacked.
const MAX_UNPACKED_BYTES: u64 = 64 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldImportResult {
    #[serde(flatten)]
    pub report: WorldImportReport,
    /// The backup made of the world that was replaced.
    pub backup_id: Option<String>,
}

/// Where uploaded archives wait for `world_import` (remote imports).
pub fn uploads_dir() -> AppResult<PathBuf> {
    let dir = paths::config_dir()?.join("uploads");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The archive an upload id refers to. Ids are UUIDs we issued; anything else
/// is refused so an id can never point outside the uploads folder.
pub fn upload_path(upload_id: &str) -> AppResult<PathBuf> {
    if uuid::Uuid::parse_str(upload_id).is_err() {
        return Err(AppError::new(ErrorCode::InvalidInput).with("field", "uploadId"));
    }
    let path = uploads_dir()?.join(format!("{upload_id}.zip"));
    if !path.is_file() {
        return Err(AppError::new(ErrorCode::FileNotFound).with("upload", upload_id));
    }
    Ok(path)
}

fn extract_zip(archive: &Path, into: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|error| {
        AppError::new(ErrorCode::InvalidInput).with("reason", "not_a_zip").detail(error)
    })?;
    let total: u64 = (0..zip.len()).filter_map(|i| zip.by_index_raw(i).ok().map(|e| e.size())).sum();
    if total > MAX_UNPACKED_BYTES {
        return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "archive_too_large"));
    }
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|error| AppError::new(ErrorCode::InvalidInput).detail(error))?;
        // Never trust paths inside an archive.
        let Some(name) = entry.enclosed_name() else { continue };
        let target = paths::resolve_inside(into, &name.to_string_lossy())?;
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

fn copy_folder(from: &Path, to: &Path) -> AppResult<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_symlink() {
            continue;
        } else if kind.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

impl InstanceService {
    /// Import the world in `source` (a `.zip` or a folder on this machine).
    pub async fn import_world(self: &Arc<Self>, instance_id: &str, source: PathBuf) -> AppResult<WorldImportResult> {
        if self.processes.is_running(instance_id).await {
            return Err(AppError::new(ErrorCode::InstanceAlreadyRunning).with("instance", instance_id));
        }
        let instance = self.store.get(instance_id).await?;
        let adapter = self.registry.get(&instance.game)?;
        if !source.exists() {
            return Err(AppError::new(ErrorCode::FileNotFound).with("path", source.display()));
        }

        // Back up what is there now: replacing a world must be undoable.
        let backup_id = match backups::create(self, instance_id, Some("Before importing a world".into())).await {
            Ok(backup) => Some(backup.id),
            Err(error) => {
                tracing::warn!(%instance_id, %error, "no backup before world import");
                return Err(error);
            }
        };

        let root = instance.path.clone();
        let staged = root.join(format!(".nebula-import-{}", uuid::Uuid::new_v4().simple()));
        let build = instance.build.clone();
        let result = tokio::task::spawn_blocking(move || {
            let outcome = (|| {
                let is_zip = source.is_file()
                    && source.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
                if is_zip {
                    extract_zip(&source, &staged)?;
                } else if source.is_dir() {
                    copy_folder(&source, &staged)?;
                } else {
                    return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "not_zip_or_folder"));
                }
                adapter.import_world(&root, &staged, &build)
            })();
            let _ = std::fs::remove_dir_all(&staged);
            outcome
        })
        .await
        .map_err(AppError::unexpected)??;

        self.after_folder_change(instance_id).await;
        tracing::info!(%instance_id, level = %result.level_name, "world imported");
        Ok(WorldImportResult { report: result, backup_id })
    }
}
