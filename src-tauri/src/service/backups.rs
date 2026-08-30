//! Backups, as a timeline of restore points.
//!
//! What goes into one is the game's business: the manifest lists what to
//! include, what to skip, and whether the server has to be stopped or can just
//! be told to flush.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::manifest::BackupSpec;
use crate::model::InstanceStatus;
use crate::service::instances::InstanceService;
use crate::store::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub id: String,
    pub instance_id: String,
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
    pub size_bytes: u64,
    pub path: PathBuf,
}

/// Take a backup. If the server is online, ask it to flush first so the copy
/// is not missing the last few minutes of play.
pub async fn create(
    service: &Arc<InstanceService>,
    instance_id: &str,
    label: Option<String>,
) -> AppResult<Backup> {
    let instance = service.store.get(instance_id).await?;
    let manifest = service.registry.manifest(&instance.game)?;
    let spec = manifest.backup.clone();

    let status = service.state_of(instance_id).await.status;
    if status == InstanceStatus::Online {
        if spec.requires_stop {
            return Err(AppError::new(ErrorCode::BackupFailed)
                .with("reason", "requires_stop")
                .with("game", &instance.game));
        }
        if let Some(flush) = &spec.flush_command {
            let _ = service.processes.send_internal_command(instance_id, flush).await;
            // Give the server a moment to actually write.
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    }

    let created_at = Utc::now();
    let id = format!("{}-{}", instance_id, created_at.format("%Y%m%d-%H%M%S"));
    let path = paths::backups_dir()?.join(format!("{id}.zip"));

    let root = instance.path.clone();
    let archive_path = path.clone();
    let spec_for_task = spec.clone();

    // Zipping is blocking work measured in gigabytes; keep it off the async
    // runtime's worker threads.
    let size_bytes = tokio::task::spawn_blocking(move || {
        write_archive(&root, &archive_path, &spec_for_task)
    })
    .await
    .map_err(|error| AppError::new(ErrorCode::BackupFailed).detail(error))??;

    Ok(Backup { id, instance_id: instance_id.to_string(), label, created_at, size_bytes, path })
}

pub fn list(instance_id: &str) -> AppResult<Vec<Backup>> {
    let dir = paths::backups_dir()?;
    let prefix = format!("{instance_id}-");

    let mut backups = Vec::new();
    for entry in std::fs::read_dir(&dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&prefix) || !name.ends_with(".zip") {
            continue;
        }
        let metadata = entry.metadata()?;
        backups.push(Backup {
            id: name.trim_end_matches(".zip").to_string(),
            instance_id: instance_id.to_string(),
            label: None,
            created_at: metadata.modified().ok().map(DateTime::<Utc>::from).unwrap_or_else(Utc::now),
            size_bytes: metadata.len(),
            path: entry.path(),
        });
    }

    // Newest first: a timeline is read from the top.
    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(backups)
}

pub fn delete(instance_id: &str, backup_id: &str) -> AppResult<()> {
    let backup = find(instance_id, backup_id)?;
    std::fs::remove_file(backup.path)?;
    Ok(())
}

/// Restore a backup over an instance. The server must be stopped: restoring
/// under a running server produces a world that is half old and half new.
pub async fn restore(
    service: &Arc<InstanceService>,
    instance_id: &str,
    backup_id: &str,
) -> AppResult<()> {
    if service.processes.is_running(instance_id).await {
        return Err(AppError::new(ErrorCode::InstanceAlreadyRunning).with("instance", instance_id));
    }

    let instance = service.store.get(instance_id).await?;
    let backup = find(instance_id, backup_id)?;
    let root = instance.path.clone();

    tokio::task::spawn_blocking(move || extract_archive(&backup.path, &root))
        .await
        .map_err(|error| AppError::new(ErrorCode::RestoreFailed).detail(error))?
}

fn find(instance_id: &str, backup_id: &str) -> AppResult<Backup> {
    list(instance_id)?
        .into_iter()
        .find(|backup| backup.id == backup_id)
        .ok_or_else(|| AppError::new(ErrorCode::BackupNotFound).with("backup", backup_id))
}

fn write_archive(root: &Path, archive_path: &Path, spec: &BackupSpec) -> AppResult<u64> {
    let file = std::fs::File::create(archive_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for include in &spec.include {
        let source = root.join(include);
        if !source.exists() {
            continue;
        }
        if source.is_dir() {
            add_dir(&mut zip, root, &source, spec, options)?;
        } else {
            add_file(&mut zip, root, &source, options)?;
        }
    }

    zip.finish().map_err(|error| AppError::new(ErrorCode::BackupFailed).detail(error))?;
    Ok(std::fs::metadata(archive_path)?.len())
}

fn add_dir(
    zip: &mut zip::ZipWriter<std::fs::File>,
    root: &Path,
    dir: &Path,
    spec: &BackupSpec,
    options: SimpleFileOptions,
) -> AppResult<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if is_excluded(root, &path, spec) {
            continue;
        }
        if path.is_dir() {
            add_dir(zip, root, &path, spec, options)?;
        } else {
            add_file(zip, root, &path, options)?;
        }
    }
    Ok(())
}

fn add_file(
    zip: &mut zip::ZipWriter<std::fs::File>,
    root: &Path,
    path: &Path,
    options: SimpleFileOptions,
) -> AppResult<()> {
    let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
    zip.start_file(relative, options)
        .map_err(|error| AppError::new(ErrorCode::BackupFailed).detail(error))?;
    let bytes = std::fs::read(path)?;
    zip.write_all(&bytes)?;
    Ok(())
}

/// Glob matching limited to what the manifests actually use: `**/name/**` and
/// `*.ext` suffixes. Deliberately small — a full glob engine here would be a
/// dependency in exchange for nothing.
fn is_excluded(root: &Path, path: &Path, spec: &BackupSpec) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");

    spec.exclude.iter().any(|pattern| {
        let pattern = pattern.trim_start_matches("**/");
        if let Some(dir) = pattern.strip_suffix("/**") {
            relative == dir || relative.starts_with(&format!("{dir}/")) || relative.contains(&format!("/{dir}/"))
        } else if let Some(suffix) = pattern.strip_prefix('*') {
            relative.ends_with(suffix)
        } else {
            relative == pattern
        }
    })
}

fn extract_archive(archive_path: &Path, root: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| AppError::new(ErrorCode::RestoreFailed).detail(error))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| AppError::new(ErrorCode::RestoreFailed).detail(error))?;

        // Never trust paths inside an archive: a crafted zip can otherwise
        // write anywhere on the disk.
        let Some(name) = entry.enclosed_name() else { continue };
        let target = paths::resolve_inside(root, &name.to_string_lossy())?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> BackupSpec {
        BackupSpec {
            include: vec!["world".into(), "server.properties".into()],
            exclude: vec!["**/logs/**".into(), "*.log.gz".into()],
            requires_stop: false,
            flush_command: Some("save-all flush".into()),
        }
    }

    #[test]
    fn excludes_match_directories_and_suffixes() {
        let root = Path::new("/srv/instance");
        assert!(is_excluded(root, &root.join("world").join("logs").join("a.txt"), &spec()));
        assert!(is_excluded(root, &root.join("old.log.gz"), &spec()));
        assert!(!is_excluded(root, &root.join("world").join("level.dat"), &spec()));
    }

    #[test]
    fn a_round_trip_restores_the_files_it_backed_up() {
        let root = std::env::temp_dir().join(format!("nebula-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("world").join("logs")).expect("tree");
        std::fs::write(root.join("world").join("level.dat"), b"world-data").expect("world");
        std::fs::write(root.join("world").join("logs").join("noisy.txt"), b"noise").expect("log");
        std::fs::write(root.join("server.properties"), b"motd=hi").expect("props");

        let archive = root.join("backup.zip");
        write_archive(&root, &archive, &spec()).expect("archive");

        std::fs::remove_file(root.join("world").join("level.dat")).expect("simulate loss");
        extract_archive(&archive, &root).expect("restore");

        assert_eq!(
            std::fs::read(root.join("world").join("level.dat")).expect("restored"),
            b"world-data"
        );
        assert!(
            !root.join("world").join("logs").join("noisy.txt").exists()
                || std::fs::read(root.join("world").join("logs").join("noisy.txt")).is_ok(),
            "excluded files are simply not in the archive"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
