//! A copy of a server: same build, world, settings and mods or plugins, under
//! a new name and on its own port. What it is for is trying something (a
//! plugin under development, an update, a risky command) on a copy instead of
//! on the server people play on.
//!
//! The copy is made of the folder, not of a backup: a backup leaves out the
//! server jar and its libraries, and a copy has to start without installing.
//! A running source is asked to flush first, as a backup does.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::registry::InstallProgress;
use crate::model::{Instance, InstanceStatus};
use crate::net::ports;
use crate::service::instances::{CreateInstanceRequest, InstanceService};

/// Never worth copying: what the server wrote about past runs, and the lock
/// that says a world is open.
const NEVER_COPIED: &[&str] = &["logs", "crash-reports", "cache"];
const SKIPPED_FILES: &[&str] = &["session.lock"];

#[derive(Debug, Clone, Copy)]
pub struct CloneOptions {
    pub include_world: bool,
    pub include_addons: bool,
}

/// What a clone that has just begun looks like.
#[derive(Debug, Clone, Serialize)]
pub struct CloneStarted {
    pub instance: Instance,
    /// What is being copied.
    pub bytes: u64,
    pub files: u64,
    /// The source was running and was asked to write its world to disk first.
    pub flushed: bool,
}

/// The folders at the top of a server's folder that a copy leaves out.
fn skipped_folders(level_name: &str, addon_dir: Option<&str>, options: CloneOptions) -> Vec<String> {
    let mut skipped: Vec<String> = NEVER_COPIED.iter().map(|name| name.to_string()).collect();
    if !options.include_world {
        // Vanilla keeps the dimensions inside the world; Bukkit beside it.
        skipped.extend([level_name.to_string(), format!("{level_name}_nether"), format!("{level_name}_the_end")]);
    }
    if !options.include_addons {
        if let Some(dir) = addon_dir {
            skipped.push(dir.to_string());
        }
    }
    skipped
}

fn is_skipped_file(name: &str) -> bool {
    SKIPPED_FILES.contains(&name) || name.ends_with(".nebula-tmp")
}

/// Walk what a copy of `root` holds: every file but the skipped ones, never
/// through a link. `visit` gets each file's path relative to `root`.
fn walk(root: &Path, skipped: &[String], visit: &mut dyn FnMut(&Path, u64) -> AppResult<()>) -> AppResult<()> {
    fn inner(root: &Path, dir: &Path, skipped: &[String], visit: &mut dyn FnMut(&Path, u64) -> AppResult<()>) -> AppResult<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let name = entry.file_name().to_string_lossy().to_string();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                // Only the top level is compared: a plugin may well have its own `cache`.
                if dir == root && skipped.iter().any(|skip| skip.eq_ignore_ascii_case(&name)) {
                    continue;
                }
                inner(root, &entry.path(), skipped, visit)?;
            } else if !is_skipped_file(&name) {
                let path = entry.path();
                let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                visit(&relative, entry.metadata()?.len())?;
            }
        }
        Ok(())
    }
    inner(root, root, skipped, visit)
}

/// How much a copy of `root` holds: bytes and files.
fn measure(root: &Path, skipped: &[String]) -> AppResult<(u64, u64)> {
    let (mut bytes, mut files) = (0, 0);
    walk(root, skipped, &mut |_, size| {
        bytes += size;
        files += 1;
        Ok(())
    })?;
    Ok((bytes, files))
}

fn copy_tree(from: &Path, to: &Path, skipped: &[String]) -> AppResult<()> {
    walk(from, skipped, &mut |relative, _| {
        let target = to.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from.join(relative), target)?;
        Ok(())
    })
}

/// The first port at or after `start` that nothing is listening on and no
/// server here is set to use (a stopped server's port looks free to the OS).
fn free_port(start: u16, taken: &[u16]) -> Option<u16> {
    let mut from = start;
    loop {
        let port = ports::first_free_from(from)?;
        if !taken.contains(&port) {
            return Some(port);
        }
        from = port.checked_add(1)?;
    }
}

impl InstanceService {
    /// Begin a copy of a server under `name`. The new server is recorded at
    /// once and shows as being set up while its files are copied in the
    /// background; it is installed, and stopped, when that ends.
    pub async fn clone_instance(self: &Arc<Self>, source_id: &str, name: &str, options: CloneOptions) -> AppResult<CloneStarted> {
        self.ensure_not_transferring(source_id).await?;
        let source = self.store.get(source_id).await?;
        let manifest = self.registry.manifest(&source.game)?;
        if source.install.is_none() || self.setup_of(source_id).is_some() {
            return Err(AppError::new(ErrorCode::InvalidInput)
                .with("reason", "not_installed")
                .detail("the server to copy is not installed yet"));
        }

        // A running server keeps part of its world in memory: have it written
        // out, as a backup does. One that is starting or stopping is not in a
        // state to copy.
        let status = self.state_of(source_id).await.status;
        let mut flushed = false;
        match status {
            InstanceStatus::Online => {
                let flush = manifest.backup.flush_command.as_ref().filter(|_| !manifest.backup.requires_stop);
                let Some(flush) = flush else {
                    return Err(AppError::new(ErrorCode::InstanceAlreadyRunning).with("instance", source_id));
                };
                self.processes.send_internal_command(source_id, flush).await?;
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                flushed = true;
            }
            InstanceStatus::Starting | InstanceStatus::Stopping => {
                return Err(AppError::new(ErrorCode::NodeBusy)
                    .with("instance", source_id)
                    .detail("the server to copy is starting or stopping"));
            }
            InstanceStatus::Offline | InstanceStatus::Sleeping | InstanceStatus::Crashed => {}
        }

        let addon_dir = manifest
            .distribution(&source.build.distribution)
            .and_then(|distribution| distribution.addon_dir.clone());
        let level_name = crate::game::minecraft::world::level_name(&source.path);
        let skipped = skipped_folders(&level_name, addon_dir.as_deref(), options);

        let from = source.path.clone();
        let (bytes, files) = {
            let (from, skipped) = (from.clone(), skipped.clone());
            tokio::task::spawn_blocking(move || measure(&from, &skipped)).await.map_err(AppError::unexpected)??
        };

        let taken: Vec<u16> =
            self.store.list().await.iter().map(|instance| instance.primary_port(&manifest)).collect();
        let port = manifest.primary_port().and_then(|spec| free_port(spec.default, &taken));

        let created = self
            .create(CreateInstanceRequest {
                game: source.game.clone(),
                name: name.to_string(),
                build: source.build.clone(),
                memory_mb: Some(source.resources.memory_mb),
                port,
                runtime_kind: Some(source.runtime_kind),
                directory: None,
                accept_agreement: false,
            })
            .await?;
        self.report_setup(&created.id, InstallProgress::Configuring);

        let service = self.clone();
        let (id, to) = (created.id.clone(), created.path.clone());
        tokio::spawn(async move {
            let copied = {
                let to = to.clone();
                tokio::task::spawn_blocking(move || copy_tree(&from, &to, &skipped)).await.map_err(AppError::unexpected).and_then(|done| done)
            };
            let finished = match copied {
                Ok(()) => service.finish_clone(&id, &source, &to).await,
                Err(error) => Err(error),
            };
            match finished {
                Ok(()) => service.report_setup(&id, InstallProgress::Ready),
                Err(error) => {
                    tracing::warn!(instance_id = %id, %error, "copying a server failed");
                    service.report_setup(&id, InstallProgress::Failed { code: error.code });
                }
            }
            service.emit(crate::service::AppEvent::Instances);
        });

        Ok(CloneStarted { instance: created, bytes, files, flushed })
    }

    /// The files are in place: make the copy a server of its own.
    async fn finish_clone(&self, id: &str, source: &Instance, root: &PathBuf) -> AppResult<()> {
        let copy = self.store.get(id).await?;
        let manifest = self.registry.manifest(&copy.game)?;
        // The copied config still names the source's port.
        if let Some(spec) = manifest.primary_port() {
            if root.join("server.properties").is_file() {
                crate::service::properties::set_internal(root, &spec.key, &copy.primary_port(&manifest).to_string())?;
            }
        }
        let (install, track_latest, icon) = (source.install.clone(), source.track_latest_version, source.icon.clone());
        self.store
            .update(id, |instance| {
                instance.install = install.clone();
                instance.track_latest_version = track_latest;
                instance.icon = icon.clone();
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: CloneOptions = CloneOptions { include_world: true, include_addons: true };

    fn server() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        for (path, text) in [
            ("server.jar", "jar"),
            ("server.properties", "server-port=25565\nlevel-name=world\n"),
            ("eula.txt", "eula=true"),
            ("world/level.dat", "level"),
            ("world/session.lock", "lock"),
            ("world/region/r.0.0.mca", "region!"),
            ("world_nether/level.dat", "nether"),
            ("plugins/MyPlugin.jar", "plugin"),
            ("plugins/MyPlugin/config.yml", "a: 1"),
            ("plugins/MyPlugin/cache/kept.bin", "kept"),
            ("logs/latest.log", "log"),
            ("crash-reports/crash.txt", "crash"),
            ("config/paper.yml.nebula-tmp", "half"),
        ] {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        dir
    }

    fn files_of(root: &Path, skipped: &[String]) -> Vec<String> {
        let mut found = Vec::new();
        walk(root, skipped, &mut |relative, _| {
            found.push(relative.to_string_lossy().replace('\\', "/"));
            Ok(())
        })
        .unwrap();
        found.sort();
        found
    }

    #[test]
    fn a_copy_leaves_out_logs_locks_and_what_was_not_asked_for() {
        let source = server();
        let everything = skipped_folders("world", Some("plugins"), ALL);
        assert_eq!(
            files_of(source.path(), &everything),
            [
                "eula.txt",
                "plugins/MyPlugin.jar",
                "plugins/MyPlugin/cache/kept.bin",
                "plugins/MyPlugin/config.yml",
                "server.jar",
                "server.properties",
                "world/level.dat",
                "world/region/r.0.0.mca",
                "world_nether/level.dat",
            ]
        );

        let no_world = skipped_folders("world", Some("plugins"), CloneOptions { include_world: false, include_addons: true });
        assert!(files_of(source.path(), &no_world).iter().all(|file| !file.starts_with("world")));
        let bare = skipped_folders("world", Some("plugins"), CloneOptions { include_world: false, include_addons: false });
        assert_eq!(files_of(source.path(), &bare), ["eula.txt", "server.jar", "server.properties"]);
        // A game with no mods or plugins folder has nothing to leave out.
        assert_eq!(skipped_folders("w", None, CloneOptions { include_world: true, include_addons: false }).len(), NEVER_COPIED.len());
    }

    #[test]
    fn a_copy_holds_what_was_measured() {
        let source = server();
        let target = tempfile::tempdir().expect("tempdir");
        let skipped = skipped_folders("world", Some("plugins"), ALL);

        let (bytes, files) = measure(source.path(), &skipped).unwrap();
        assert_eq!(files, 9);
        copy_tree(source.path(), target.path(), &skipped).unwrap();

        assert_eq!(files_of(target.path(), &[]), files_of(source.path(), &skipped));
        assert_eq!(measure(target.path(), &[]).unwrap(), (bytes, files));
        assert_eq!(std::fs::read_to_string(target.path().join("world/region/r.0.0.mca")).unwrap(), "region!");
        assert!(!target.path().join("logs").exists());
    }

    #[test]
    fn a_copy_gets_a_port_no_server_here_uses() {
        let first = ports::first_free_from(40_000).expect("a free port");
        assert_eq!(free_port(40_000, &[]), Some(first));
        let next = free_port(40_000, &[first]).expect("another free port");
        assert!(next > first);
        assert!(!vec![first].contains(&next));
    }
}
