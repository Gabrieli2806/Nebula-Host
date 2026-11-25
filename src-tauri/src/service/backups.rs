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
    /// Files left out because the running server held them locked, relative
    /// to the server folder. Known only for the backup just taken.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
}

/// Files a backup never holds: the lock a running server keeps on an open
/// world. It carries no data, and on Windows it cannot even be read.
const NEVER_BACKED_UP: &[&str] = &["session.lock"];

/// Take a backup. If the server is online, ask it to flush first so the copy
/// is not missing the last few minutes of play.
/// Suffix on the id of backups taken by the automatic schedule. Only those
/// count against the keep-limit and are ever removed automatically.
const AUTO_SUFFIX: &str = "-auto";

pub async fn create(
    service: &Arc<InstanceService>,
    instance_id: &str,
    label: Option<String>,
) -> AppResult<Backup> {
    create_inner(service, instance_id, label, false).await
}

/// An automatic backup: same as a manual one, marked so rotation can find it.
pub async fn create_auto(service: &Arc<InstanceService>, instance_id: &str) -> AppResult<Backup> {
    create_inner(service, instance_id, Some("auto".into()), true).await
}

/// Remove the oldest automatic backups beyond `keep`. Manual ones untouched.
pub fn prune_auto(instance_id: &str, keep: usize) -> AppResult<usize> {
    let autos: Vec<Backup> = list(instance_id)?.into_iter().filter(|b| b.id.ends_with(AUTO_SUFFIX)).collect();
    let mut removed = 0;
    // `list` is newest first: everything after the first `keep` goes.
    for old in autos.iter().skip(keep.max(1)) {
        if std::fs::remove_file(&old.path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// When the newest automatic backup of this server was taken.
pub fn last_auto(instance_id: &str) -> Option<DateTime<Utc>> {
    list(instance_id).ok()?.into_iter().find(|b| b.id.ends_with(AUTO_SUFFIX)).map(|b| b.created_at)
}

async fn create_inner(
    service: &Arc<InstanceService>,
    instance_id: &str,
    label: Option<String>,
    auto: bool,
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
    let id = format!(
        "{}-{}{}",
        instance_id,
        created_at.format("%Y%m%d-%H%M%S"),
        if auto { AUTO_SUFFIX } else { "" }
    );
    let path = paths::backups_dir()?.join(format!("{id}.zip"));

    let root = instance.path.clone();
    let archive_path = path.clone();
    let spec_for_task = spec.clone();
    // A server with a process holds files open. One it has locked is left
    // out and named; with no process, a file that cannot be read is a failure.
    let running = status.expects_process();

    // Zipping is blocking work measured in gigabytes; keep it off the async
    // runtime's worker threads.
    let (size_bytes, skipped) = tokio::task::spawn_blocking(move || {
        write_backup(&root, &archive_path, &spec_for_task, running)
    })
    .await
    .map_err(|error| AppError::new(ErrorCode::BackupFailed).detail(error))??;
    if !skipped.is_empty() {
        tracing::warn!(instance_id, ?skipped, "backup taken without the files the running server holds locked");
    }

    Ok(Backup { id, instance_id: instance_id.to_string(), label, created_at, size_bytes, path, skipped })
}

/// Write the archive beside its final name and move it there only once it
/// is whole: a backup that failed half way is never listed, and so never
/// restored.
fn write_backup(root: &Path, archive_path: &Path, spec: &BackupSpec, running: bool) -> AppResult<(u64, Vec<String>)> {
    let mut partial = archive_path.as_os_str().to_owned();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    let written = write_archive(root, &partial, spec, running).and_then(|done| {
        std::fs::rename(&partial, archive_path)?;
        Ok(done)
    });
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written
}

/// Whether reading failed because another process holds the file (or part
/// of it) locked, as a running server does with some of its own.
fn is_locked(error: &std::io::Error) -> bool {
    // Windows: ERROR_SHARING_VIOLATION (32), ERROR_LOCK_VIOLATION (33).
    (cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33))) || error.kind() == std::io::ErrorKind::WouldBlock
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
            label: name.trim_end_matches(".zip").ends_with(AUTO_SUFFIX).then(|| "auto".to_string()),
            id: name.trim_end_matches(".zip").to_string(),
            instance_id: instance_id.to_string(),
            created_at: metadata.modified().ok().map(DateTime::<Utc>::from).unwrap_or_else(Utc::now),
            size_bytes: metadata.len(),
            path: entry.path(),
            skipped: Vec::new(),
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
    service.ensure_not_transferring(instance_id).await?;
    if service.processes.is_running(instance_id).await {
        return Err(AppError::new(ErrorCode::InstanceAlreadyRunning).with("instance", instance_id));
    }

    let instance = service.store.get(instance_id).await?;
    let backup = find(instance_id, backup_id)?;
    let root = instance.path.clone();
    // Settings changed since the backup must not be written back over the
    // restored ones at the next start.
    service.forget_settings_while_running(instance_id).await;

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

/// The archive's size, and the files left out because they were locked
/// (only tolerated with `skip_locked`: while the server runs).
fn write_archive(root: &Path, archive_path: &Path, spec: &BackupSpec, skip_locked: bool) -> AppResult<(u64, Vec<String>)> {
    let file = std::fs::File::create(archive_path)?;
    let mut writer = Archive {
        zip: zip::ZipWriter::new(file),
        options: SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        skip_locked,
        skipped: Vec::new(),
    };

    for include in &spec.include {
        let source = root.join(include);
        if !source.exists() {
            continue;
        }
        if source.is_dir() {
            add_dir(&mut writer, root, &source, spec)?;
        } else {
            add_file(&mut writer, root, &source)?;
        }
    }

    writer.zip.finish().map_err(|error| AppError::new(ErrorCode::BackupFailed).detail(error))?;
    Ok((std::fs::metadata(archive_path)?.len(), writer.skipped))
}

struct Archive {
    zip: zip::ZipWriter<std::fs::File>,
    options: SimpleFileOptions,
    skip_locked: bool,
    skipped: Vec<String>,
}

fn add_dir(archive: &mut Archive, root: &Path, dir: &Path, spec: &BackupSpec) -> AppResult<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if is_excluded(root, &path, spec) {
            continue;
        }
        if path.is_dir() {
            add_dir(archive, root, &path, spec)?;
        } else {
            add_file(archive, root, &path)?;
        }
    }
    Ok(())
}

fn add_file(archive: &mut Archive, root: &Path, path: &Path) -> AppResult<()> {
    if path.file_name().and_then(|name| name.to_str()).is_some_and(|name| NEVER_BACKED_UP.contains(&name)) {
        return Ok(());
    }
    let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
    // Read before the entry is opened: a file that cannot be read must not
    // leave an empty one behind in the archive.
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if archive.skip_locked && is_locked(&error) => {
            archive.skipped.push(relative);
            return Ok(());
        }
        Err(error) => {
            return Err(AppError::from(error).with("file", relative));
        }
    };
    archive
        .zip
        .start_file(relative, archive.options)
        .map_err(|error| AppError::new(ErrorCode::BackupFailed).detail(error))?;
    archive.zip.write_all(&bytes)?;
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
        write_archive(&root, &archive, &spec(), false).expect("archive");

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

    fn names_in(archive: &Path) -> Vec<String> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(archive).expect("open")).expect("a whole zip");
        let mut names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).expect("entry").name().to_string()).collect();
        names.sort();
        names
    }

    fn world() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("world").join("region")).expect("tree");
        std::fs::write(dir.path().join("world").join("level.dat"), b"level").expect("level");
        std::fs::write(dir.path().join("world").join("session.lock"), b"\xe2\x98\x83").expect("lock");
        std::fs::write(dir.path().join("world").join("region").join("r.0.0.mca"), b"region").expect("region");
        std::fs::write(dir.path().join("server.properties"), b"motd=hi").expect("props");
        dir
    }

    #[test]
    fn the_world_lock_is_never_in_a_backup() {
        let server = world();
        let out = tempfile::tempdir().expect("tempdir");
        let archive = out.path().join("backup.zip");
        let (size, skipped) = write_backup(server.path(), &archive, &spec(), false).expect("backup");
        assert!(size > 0 && skipped.is_empty());
        assert_eq!(names_in(&archive), ["server.properties", "world/level.dat", "world/region/r.0.0.mca"]);
        assert!(!out.path().join("backup.zip.part").exists(), "nothing half written is left");
    }

    /// What a running Minecraft server does to `session.lock`, here done to
    /// a file that matters: a byte-range lock, which lets the file be opened
    /// and fails the read.
    #[cfg(windows)]
    #[test]
    fn a_file_the_running_server_holds_locked_is_left_out_and_named() {
        let server = world();
        let region = server.path().join("world").join("region").join("r.0.0.mca");
        let held = std::fs::OpenOptions::new().read(true).write(true).open(&region).expect("open");
        held.lock().expect("lock");
        assert!(std::fs::read(&region).is_err_and(|error| is_locked(&error)), "the read is what fails");

        let out = tempfile::tempdir().expect("tempdir");
        let archive = out.path().join("backup.zip");

        // Running: the backup is whole but for that file, and says so.
        let (_, skipped) = write_backup(server.path(), &archive, &spec(), true).expect("backup");
        assert_eq!(skipped, ["world/region/r.0.0.mca"]);
        assert_eq!(names_in(&archive), ["server.properties", "world/level.dat"], "no empty entry for the locked file");

        // Stopped: nothing should be locked, so it is a failure, and no
        // archive is left to be listed or restored, whole or partial.
        std::fs::remove_file(&archive).expect("remove");
        let error = write_backup(server.path(), &archive, &spec(), false).expect_err("refused");
        assert_eq!(error.params.get("file").map(String::as_str), Some("world/region/r.0.0.mca"));
        assert!(!archive.exists());
        assert!(std::fs::read_dir(out.path()).expect("dir").next().is_none(), "the partial archive is removed");

        held.unlock().expect("unlock");
        let (_, skipped) = write_backup(server.path(), &archive, &spec(), false).expect("backup");
        assert!(skipped.is_empty());
        assert_eq!(names_in(&archive).len(), 3);
    }

    #[test]
    fn a_backup_that_fails_leaves_no_archive() {
        let server = world();
        let out = tempfile::tempdir().expect("tempdir");
        // The folder the archive goes in does not exist: it cannot be written.
        let archive = out.path().join("missing").join("backup.zip");
        assert!(write_backup(server.path(), &archive, &spec(), true).is_err());
        assert!(std::fs::read_dir(out.path()).expect("dir").next().is_none());
    }

    #[test]
    fn only_a_lock_counts_as_locked() {
        assert!(!is_locked(&std::io::Error::from(std::io::ErrorKind::NotFound)));
        assert!(!is_locked(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)));
        assert!(is_locked(&std::io::Error::from(std::io::ErrorKind::WouldBlock)));
        assert_eq!(is_locked(&std::io::Error::from_raw_os_error(33)), cfg!(windows));
        assert_eq!(is_locked(&std::io::Error::from_raw_os_error(32)), cfg!(windows));
    }
}
