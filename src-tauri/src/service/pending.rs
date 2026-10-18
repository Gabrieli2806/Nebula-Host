//! Addon changes that wait for the server to stop.
//!
//! A running server has its jars loaded (and, on Windows, open): swapping
//! one under it does nothing until a restart at best, and fails halfway at
//! worst. So while it runs, installs, updates, removals and switches are
//! queued instead. Downloads still happen at once — into a staging folder —
//! so applying the queue is only moving files, done the moment the server
//! stops and again before it next starts.
//!
//! The queue is a file per instance in the app's config folder, so it
//! survives the app being closed with the server still up.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::minecraft::addons as files;
use crate::game::minecraft::modrinth;
use crate::model::{ConsoleLevel, ConsoleLine};
use crate::service::{AppEvent, InstanceService};

/// One queued change, as the UI lists it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAddonChange {
    pub id: String,
    pub queued_at: DateTime<Utc>,
    #[serde(flatten)]
    pub action: PendingAction,
    /// What to call it in the list: the project's title, else the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Why the last attempt to apply it failed. Failed changes stay queued
    /// until they work or the user cancels them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<StoredError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PendingAction {
    /// Put a downloaded jar in place, replacing `replace` if set (an update
    /// or a version switch).
    Install {
        file_name: String,
        /// The downloaded jar, inside this instance's staging folder.
        staged: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version_number: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replace: Option<String>,
        /// Install it switched off (it replaces a switched-off jar, or the
        /// user switched it off while it waited).
        #[serde(default)]
        disabled: bool,
    },
    Remove {
        file_name: String,
    },
    SetEnabled {
        file_name: String,
        enabled: bool,
    },
}

/// An [`AppError`] that can be written to disk and read back, in the same
/// shape the UI already renders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredError {
    pub code: ErrorCode,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl From<&AppError> for StoredError {
    fn from(error: &AppError) -> Self {
        Self { code: error.code, params: error.params.clone(), detail: error.detail.clone() }
    }
}

impl PendingAddonChange {
    pub fn new(action: PendingAction, title: Option<String>) -> Self {
        Self { id: uuid::Uuid::new_v4().to_string(), queued_at: Utc::now(), action, title, error: None }
    }
}

impl InstanceService {
    /// The queued changes for one server, oldest first.
    pub async fn pending_addons(&self, instance_id: &str) -> AppResult<Vec<PendingAddonChange>> {
        let _ = self.store.get(instance_id).await?;
        load(instance_id)
    }

    /// Whether addon changes for this server must wait: its process is up.
    pub(crate) async fn addons_locked(&self, instance_id: &str) -> bool {
        self.processes.is_running(instance_id).await
    }

    /// This instance's staging folder for queued downloads.
    pub(crate) fn pending_staging(&self, instance_id: &str) -> AppResult<PathBuf> {
        staging_dir(instance_id)
    }

    /// Project ids with an install waiting, so search results and
    /// dependency lists can treat them as already there.
    pub(crate) async fn pending_projects(&self, instance_id: &str) -> HashSet<String> {
        load(instance_id)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|change| match change.action {
                PendingAction::Install { project_id, .. } => project_id,
                _ => None,
            })
            .collect()
    }

    /// Add a change to the queue, folding it into what is already there
    /// (an install then a removal of the same jar cancel out).
    pub(crate) async fn queue_addon_change(
        &self,
        instance_id: &str,
        change: PendingAddonChange,
    ) -> AppResult<Vec<PendingAddonChange>> {
        let _guard = self.pending_lock.lock().await;
        let mut queue = load(instance_id)?;
        let discarded = merge(&mut queue, change);
        save(instance_id, &queue)?;
        remove_staged(instance_id, &discarded);
        self.emit_pending(instance_id, &queue);
        Ok(queue)
    }

    /// Drop one queued change; a waiting download is deleted with it.
    pub async fn cancel_pending_addon(&self, instance_id: &str, change_id: &str) -> AppResult<Vec<PendingAddonChange>> {
        let _ = self.store.get(instance_id).await?;
        let _guard = self.pending_lock.lock().await;
        let mut queue = load(instance_id)?;
        let Some(index) = queue.iter().position(|change| change.id == change_id) else {
            return Err(AppError::new(ErrorCode::NotFound).with("change", change_id));
        };
        let removed = queue.remove(index);
        save(instance_id, &queue)?;
        remove_staged(instance_id, &[removed]);
        self.emit_pending(instance_id, &queue);
        Ok(queue)
    }

    /// Apply the queue if the server is not running. Called when it stops,
    /// before it starts, and when the user retries. Each change that works
    /// leaves the queue; one that fails stays, with its error, and the rest
    /// still go ahead.
    pub async fn apply_pending_addons(&self, instance_id: &str) -> AppResult<Vec<PendingAddonChange>> {
        let _guard = self.pending_lock.lock().await;
        let queue = load(instance_id)?;
        if queue.is_empty() || self.processes.is_running(instance_id).await {
            return Ok(queue);
        }
        let instance = self.store.get(instance_id).await?;
        let dir = match self.addon_profile(&instance) {
            Ok((dir, _)) => dir,
            // The server changed to a flavour without addons: nothing can
            // ever apply, and a queue that never empties helps nobody.
            Err(error) => {
                let failed: Vec<PendingAddonChange> = queue
                    .into_iter()
                    .map(|mut change| {
                        change.error = Some(StoredError::from(&error));
                        change
                    })
                    .collect();
                save(instance_id, &failed)?;
                self.emit_pending(instance_id, &failed);
                return Ok(failed);
            }
        };

        let staging = staging_dir(instance_id)?;
        let mut remaining = Vec::new();
        let mut applied = Vec::new();
        for mut change in queue {
            match apply_one(&dir, &staging, &change.action) {
                Ok(()) => applied.push(change),
                Err(error) => {
                    tracing::warn!(instance_id, %error, "a queued addon change could not be applied");
                    change.error = Some(StoredError::from(&error));
                    remaining.push(change);
                }
            }
        }
        save(instance_id, &remaining)?;
        remove_staged(instance_id, &applied);

        if !applied.is_empty() {
            self.after_addon_change(instance_id).await;
        }
        let message = match (applied.len(), remaining.len()) {
            (0, 0) => None,
            (done, 0) => Some((ConsoleLevel::System, format!("Applied {done} pending addon change(s)."))),
            (done, failed) => Some((
                ConsoleLevel::Warn,
                format!("Applied {done} pending addon change(s); {failed} could not be applied and are still pending."),
            )),
        };
        if let Some((level, message)) = message {
            self.push_console(instance_id, ConsoleLine::new(level, message)).await;
        }
        self.emit_pending(instance_id, &remaining);
        Ok(remaining)
    }

    /// Forget the queue and its downloads (the instance is being deleted).
    pub(crate) async fn drop_pending_addons(&self, instance_id: &str) {
        let _guard = self.pending_lock.lock().await;
        if let Ok(path) = queue_file(instance_id) {
            let _ = std::fs::remove_file(path);
        }
        if let Ok(dir) = staging_dir(instance_id) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn emit_pending(&self, instance_id: &str, queue: &[PendingAddonChange]) {
        let _ = self.events.send(AppEvent::PendingAddons {
            instance_id: instance_id.to_string(),
            pending: queue.to_vec(),
        });
    }
}

/// Fold `change` into `queue`. Returns the changes dropped on the way, whose
/// staged downloads the caller deletes.
pub fn merge(queue: &mut Vec<PendingAddonChange>, change: PendingAddonChange) -> Vec<PendingAddonChange> {
    let mut discarded = Vec::new();
    match &change.action {
        PendingAction::Install { file_name, project_id, replace, disabled, .. } => {
            let mut change = change.clone();
            let mut inherited_replace = None;
            let mut disabled = *disabled;
            // A newer pick of the same project replaces the waiting one; the
            // jar on disk it was going to replace is still the one to replace.
            queue.retain(|queued| {
                let same = match &queued.action {
                    PendingAction::Install { file_name: f, project_id: p, replace: r, .. } => {
                        let same = (project_id.is_some() && p == project_id) || f == file_name;
                        if same {
                            inherited_replace = r.clone();
                        }
                        same
                    }
                    _ => false,
                };
                if same {
                    discarded.push(queued.clone());
                }
                !same
            });
            let replace = inherited_replace.or_else(|| replace.clone());
            // Installing over a jar makes waiting removals and switches of
            // that jar moot; a waiting switch decides how the new one starts.
            if let Some(old) = &replace {
                queue.retain(|queued| match &queued.action {
                    PendingAction::Remove { file_name } => file_name != old,
                    PendingAction::SetEnabled { file_name, enabled } if file_name == old => {
                        disabled = !enabled;
                        false
                    }
                    _ => true,
                });
            }
            if let PendingAction::Install { replace: r, disabled: d, .. } = &mut change.action {
                *r = replace;
                *d = disabled;
            }
            queue.push(change);
        }
        PendingAction::Remove { file_name } => {
            // Removing a jar that is only waiting to be installed undoes the
            // install; removing the jar a waiting install would replace means
            // the user wants the project gone, so the install goes too.
            let mut also_remove: Option<String> = None;
            let mut cancelled_install = false;
            queue.retain(|queued| {
                let drop = match &queued.action {
                    PendingAction::Install { file_name: f, replace, .. } if f == file_name => {
                        cancelled_install = true;
                        also_remove = replace.clone();
                        true
                    }
                    PendingAction::Install { replace: Some(r), .. } if r == file_name => true,
                    PendingAction::Remove { file_name: f } | PendingAction::SetEnabled { file_name: f, .. } => {
                        f == file_name
                    }
                    _ => false,
                };
                if drop {
                    discarded.push(queued.clone());
                }
                !drop
            });
            if cancelled_install {
                // What the cancelled install was replacing is on disk; remove
                // that instead (or nothing, for a fresh install).
                if let Some(old) = also_remove {
                    let mut removal = change.clone();
                    removal.action = PendingAction::Remove { file_name: old };
                    discarded.extend(merge(queue, removal));
                }
            } else {
                queue.push(change);
            }
        }
        PendingAction::SetEnabled { file_name, enabled } => {
            // Switching a jar that is waiting to go in (or to be replaced)
            // just decides how the new one starts.
            for queued in queue.iter_mut() {
                if let PendingAction::Install { file_name: f, replace, disabled, .. } = &mut queued.action {
                    if f == file_name || replace.as_deref() == Some(file_name.as_str()) {
                        *disabled = !enabled;
                        return discarded;
                    }
                }
            }
            if queue
                .iter()
                .any(|queued| matches!(&queued.action, PendingAction::Remove { file_name: f } if f == file_name))
            {
                return discarded;
            }
            queue.retain(|queued| {
                let same = matches!(&queued.action, PendingAction::SetEnabled { file_name: f, .. } if f == file_name);
                if same {
                    discarded.push(queued.clone());
                }
                !same
            });
            // Back to how the file already is: nothing left to do.
            let already = files::is_disabled_name(file_name) != *enabled;
            if !already {
                queue.push(change);
            }
        }
    }
    discarded
}

fn apply_one(dir: &Path, staging: &Path, action: &PendingAction) -> AppResult<()> {
    match action {
        PendingAction::Install { file_name, staged, replace, disabled, .. } => {
            let staged_path = crate::store::paths::resolve_inside(staging, staged)?;
            if !staged_path.is_file() {
                return Err(AppError::new(ErrorCode::FileNotFound).with("file", file_name));
            }
            // A replaced jar may have been removed by hand meanwhile; then
            // this is a plain install.
            let replace = replace.as_deref().filter(|old| dir.join(old).exists());
            modrinth::commit_staged(dir, &staged_path, file_name, replace)?;
            if *disabled {
                files::set_enabled(dir, file_name, false)?;
            }
            Ok(())
        }
        PendingAction::Remove { file_name } => {
            let path = crate::store::paths::resolve_inside(dir, file_name)?;
            match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                // Already gone is what was asked for.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(AppError::from(error).with("file", file_name)),
            }
        }
        PendingAction::SetEnabled { file_name, enabled } => {
            let target = if *enabled {
                file_name.trim_end_matches(modrinth::DISABLED_SUFFIX).to_string()
            } else if files::is_disabled_name(file_name) {
                file_name.clone()
            } else {
                format!("{file_name}{}", modrinth::DISABLED_SUFFIX)
            };
            // Already switched (by hand, or by an earlier attempt).
            if !dir.join(file_name).exists() && dir.join(&target).exists() {
                return Ok(());
            }
            files::set_enabled(dir, file_name, *enabled).map(|_| ())
        }
    }
}

fn queue_file(instance_id: &str) -> AppResult<PathBuf> {
    crate::store::paths::resolve_inside(&crate::store::paths::pending_dir()?, &format!("{}.json", safe_id(instance_id)?))
}

fn staging_dir(instance_id: &str) -> AppResult<PathBuf> {
    crate::store::paths::resolve_inside(&crate::store::paths::pending_dir()?, safe_id(instance_id)?)
}

/// Instance ids are UUIDs; anything else must not become a path.
fn safe_id(instance_id: &str) -> AppResult<&str> {
    if !instance_id.is_empty() && instance_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        Ok(instance_id)
    } else {
        Err(AppError::new(ErrorCode::InvalidInput).with("field", "instance"))
    }
}

fn load(instance_id: &str) -> AppResult<Vec<PendingAddonChange>> {
    let path = queue_file(instance_id)?;
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_else(|error| {
            tracing::warn!(?path, %error, "pending addon queue is unreadable; starting empty");
            Vec::new()
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

/// Written atomically: a torn queue file would lose changes the user asked
/// for and believes are coming.
fn save(instance_id: &str, queue: &[PendingAddonChange]) -> AppResult<()> {
    let path = queue_file(instance_id)?;
    if queue.is_empty() {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if let Ok(dir) = staging_dir(instance_id) {
            // Only removes an empty folder; a stray download stays put.
            let _ = std::fs::remove_dir(dir);
        }
        return Ok(());
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(queue)?)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

fn remove_staged(instance_id: &str, changes: &[PendingAddonChange]) {
    let Ok(staging) = staging_dir(instance_id) else { return };
    for change in changes {
        if let PendingAction::Install { staged, .. } = &change.action {
            if let Ok(path) = crate::store::paths::resolve_inside(&staging, staged) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let _ = std::fs::remove_dir(staging);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(file: &str, project: &str, replace: Option<&str>, disabled: bool) -> PendingAddonChange {
        PendingAddonChange::new(
            PendingAction::Install {
                file_name: file.into(),
                staged: format!("{file}.staged"),
                project_id: Some(project.into()),
                version_number: None,
                replace: replace.map(str::to_string),
                disabled,
            },
            None,
        )
    }

    fn remove(file: &str) -> PendingAddonChange {
        PendingAddonChange::new(PendingAction::Remove { file_name: file.into() }, None)
    }

    fn toggle(file: &str, enabled: bool) -> PendingAddonChange {
        PendingAddonChange::new(PendingAction::SetEnabled { file_name: file.into(), enabled }, None)
    }

    fn actions(queue: &[PendingAddonChange]) -> Vec<PendingAction> {
        queue.iter().map(|c| c.action.clone()).collect()
    }

    #[test]
    fn installing_then_removing_the_same_jar_cancels_out() {
        let mut queue = Vec::new();
        merge(&mut queue, install("sodium-1.jar", "sodium", None, false));
        let discarded = merge(&mut queue, remove("sodium-1.jar"));
        assert!(queue.is_empty(), "nothing left to do");
        assert_eq!(discarded.len(), 1, "the staged download is dropped");
    }

    #[test]
    fn removing_an_update_target_removes_the_project() {
        let mut queue = Vec::new();
        merge(&mut queue, install("lithium-2.jar", "lithium", Some("lithium-1.jar"), false));
        // The user removes the waiting new jar: the old one should go too.
        merge(&mut queue, remove("lithium-2.jar"));
        assert_eq!(actions(&queue), vec![PendingAction::Remove { file_name: "lithium-1.jar".into() }]);

        // Removing the old jar directly also drops the waiting update.
        let mut queue = Vec::new();
        merge(&mut queue, install("lithium-2.jar", "lithium", Some("lithium-1.jar"), false));
        merge(&mut queue, remove("lithium-1.jar"));
        assert_eq!(actions(&queue), vec![PendingAction::Remove { file_name: "lithium-1.jar".into() }]);
    }

    #[test]
    fn a_newer_pick_of_a_project_replaces_the_waiting_one() {
        let mut queue = Vec::new();
        merge(&mut queue, install("mod-2.jar", "mod", Some("mod-1.jar"), false));
        let discarded = merge(&mut queue, install("mod-3.jar", "mod", None, false));
        assert_eq!(discarded.len(), 1);
        match &queue[..] {
            [PendingAddonChange { action: PendingAction::Install { file_name, replace, .. }, .. }] => {
                assert_eq!(file_name, "mod-3.jar");
                assert_eq!(replace.as_deref(), Some("mod-1.jar"), "still replaces the jar on disk");
            }
            other => panic!("unexpected queue {other:?}"),
        }
    }

    #[test]
    fn switching_twice_is_no_change_and_switches_follow_waiting_installs() {
        let mut queue = Vec::new();
        merge(&mut queue, toggle("a.jar", false));
        assert_eq!(queue.len(), 1);
        merge(&mut queue, toggle("a.jar", true));
        assert!(queue.is_empty(), "back to how the file is");

        merge(&mut queue, install("b-2.jar", "b", Some("b-1.jar"), false));
        merge(&mut queue, toggle("b-1.jar", false));
        assert!(matches!(&queue[0].action, PendingAction::Install { disabled: true, .. }));
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn a_waiting_switch_carries_into_an_install_over_that_jar() {
        let mut queue = Vec::new();
        merge(&mut queue, toggle("c-1.jar", false));
        merge(&mut queue, install("c-2.jar", "c", Some("c-1.jar"), false));
        assert_eq!(queue.len(), 1);
        assert!(matches!(&queue[0].action, PendingAction::Install { disabled: true, .. }));
    }

    #[test]
    fn duplicate_removals_collapse() {
        let mut queue = Vec::new();
        merge(&mut queue, remove("x.jar"));
        merge(&mut queue, remove("x.jar"));
        assert_eq!(queue.len(), 1);
        merge(&mut queue, toggle("x.jar", false));
        assert_eq!(queue.len(), 1, "switching a jar about to be removed is moot");
    }

    #[test]
    fn applying_moves_staged_jars_and_removes_files() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = root.path().join("mods");
        let staging = root.path().join("staging");
        std::fs::create_dir_all(&dir).expect("mods");
        std::fs::create_dir_all(&staging).expect("staging");
        std::fs::write(dir.join("old.jar"), b"old").expect("old");
        std::fs::write(dir.join("gone.jar"), b"gone").expect("gone");
        std::fs::write(dir.join("off.jar"), b"off").expect("off");
        std::fs::write(staging.join("new.staged"), b"new").expect("staged");

        let install = PendingAction::Install {
            file_name: "new.jar".into(),
            staged: "new.staged".into(),
            project_id: None,
            version_number: None,
            replace: Some("old.jar".into()),
            disabled: false,
        };
        apply_one(&dir, &staging, &install).expect("install");
        apply_one(&dir, &staging, &PendingAction::Remove { file_name: "gone.jar".into() }).expect("remove");
        apply_one(&dir, &staging, &PendingAction::SetEnabled { file_name: "off.jar".into(), enabled: false })
            .expect("switch");
        // Applying a second time is harmless.
        apply_one(&dir, &staging, &PendingAction::Remove { file_name: "gone.jar".into() }).expect("idempotent");
        apply_one(&dir, &staging, &PendingAction::SetEnabled { file_name: "off.jar".into(), enabled: false })
            .expect("idempotent");

        assert_eq!(std::fs::read(dir.join("new.jar")).expect("new"), b"new");
        assert!(!dir.join("old.jar").exists());
        assert!(!dir.join("gone.jar").exists());
        assert!(dir.join("off.jar.disabled").exists());
    }

    #[test]
    fn queued_changes_round_trip_through_json() {
        let mut change = install("a.jar", "a", Some("b.jar"), true);
        change.error = Some(StoredError::from(&AppError::new(ErrorCode::IoFailed).with("file", "a.jar")));
        let json = serde_json::to_string(&change).expect("serialize");
        assert!(json.contains("\"action\":\"install\""));
        let back: PendingAddonChange = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.action, change.action);
        assert_eq!(back.error.map(|e| e.code), Some(ErrorCode::IoFailed));
    }

    #[test]
    fn instance_ids_cannot_walk_out_of_the_queue_folder() {
        assert!(safe_id("../evil").is_err());
        assert!(safe_id("").is_err());
        assert!(safe_id("4f1c2a8e-1d2b-4c3d-9e8f-001122334455").is_ok());
    }
}
