//! Instance persistence.
//!
//! Each instance owns a `nebula-instance.json` inside its own directory, and a
//! small index maps ids to those directories. That split is deliberate: the
//! index makes startup one read instead of a disk scan, while the per-instance
//! file means a server folder copied to another drive still describes itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::model::Instance;
use crate::store::paths;

/// The record each server folder carries about itself.
pub const INSTANCE_FILE: &str = "nebula-instance.json";
const INDEX_FILE: &str = "instances.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    /// instance id → directory
    #[serde(default)]
    instances: BTreeMap<String, PathBuf>,
}

pub struct InstanceStore {
    index_path: PathBuf,
    instances: RwLock<BTreeMap<String, Instance>>,
}

impl InstanceStore {
    /// Load every instance the index knows about. An instance whose directory
    /// has gone (unplugged drive, deleted folder) is skipped with a warning
    /// rather than failing startup — the user still has the other nine.
    pub async fn load() -> AppResult<Self> {
        Self::load_from(paths::config_dir()?.join(INDEX_FILE)).await
    }

    /// Load from a specific index file (tests, and anything that runs two
    /// stores in one process).
    pub async fn load_from(index_path: PathBuf) -> AppResult<Self> {
        let index: Index = match std::fs::read_to_string(&index_path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Index::default(),
        };

        let mut instances = BTreeMap::new();
        for (id, dir) in index.instances {
            match read_instance(&dir) {
                Ok(instance) => {
                    instances.insert(instance.id.clone(), instance);
                }
                Err(error) => {
                    tracing::warn!(%id, ?dir, %error, "skipping unreadable instance");
                }
            }
        }

        Ok(Self { index_path, instances: RwLock::new(instances) })
    }

    pub async fn list(&self) -> Vec<Instance> {
        self.instances.read().await.values().cloned().collect()
    }

    pub async fn get(&self, id: &str) -> AppResult<Instance> {
        self.instances
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorCode::InstanceNotFound).with("instance", id))
    }

    /// Insert or replace an instance and persist it.
    pub async fn put(&self, instance: Instance) -> AppResult<()> {
        write_instance(&instance)?;
        self.instances.write().await.insert(instance.id.clone(), instance);
        self.write_index().await
    }

    /// Apply a change to a stored instance and persist the result.
    pub async fn update<F>(&self, id: &str, edit: F) -> AppResult<Instance>
    where
        F: FnOnce(&mut Instance),
    {
        let mut guard = self.instances.write().await;
        let instance = guard
            .get_mut(id)
            .ok_or_else(|| AppError::new(ErrorCode::InstanceNotFound).with("instance", id))?;
        edit(instance);
        let updated = instance.clone();
        drop(guard);

        write_instance(&updated)?;
        Ok(updated)
    }

    /// Forget an instance. `delete_files` is the difference between "remove
    /// from the app" and "delete the world" — never infer it.
    ///
    /// The files go first: when deleting them fails part way (a file held
    /// open on Windows, a permission), the record stays so the server is
    /// still listed and can be deleted again.
    pub async fn remove(&self, id: &str, delete_files: bool) -> AppResult<()> {
        let instance = self.get(id).await?;

        if delete_files {
            match std::fs::remove_dir_all(&instance.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        } else {
            let _ = std::fs::remove_file(instance.path.join(INSTANCE_FILE));
        }

        self.instances.write().await.remove(id);
        self.write_index().await
    }

    /// Point every instance recorded under node `from` at node `to`. Used once,
    /// when a node's legacy shared id (`"local"`) is replaced by a unique one.
    pub async fn migrate_node_id(&self, from: &str, to: &str) -> AppResult<usize> {
        let ids: Vec<String> = self
            .instances
            .read()
            .await
            .values()
            .filter(|instance| instance.node_id == from)
            .map(|instance| instance.id.clone())
            .collect();
        for id in &ids {
            self.update(id, |instance| instance.node_id = to.to_string()).await?;
        }
        Ok(ids.len())
    }

    /// Adopt an existing server directory that already carries an instance file.
    pub async fn import(&self, dir: &Path) -> AppResult<Instance> {
        let instance = read_instance(dir)?;
        self.put(instance.clone()).await?;
        Ok(instance)
    }

    async fn write_index(&self) -> AppResult<()> {
        let index = Index {
            instances: self
                .instances
                .read()
                .await
                .values()
                .map(|instance| (instance.id.clone(), instance.path.clone()))
                .collect(),
        };
        let temp = self.index_path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(&index)?)?;
        std::fs::rename(&temp, &self.index_path)?;
        Ok(())
    }
}

pub(crate) fn read_instance(dir: &Path) -> AppResult<Instance> {
    let text = std::fs::read_to_string(dir.join(INSTANCE_FILE))?;
    let mut instance: Instance = serde_json::from_str(&text)?;
    // The directory on disk is the truth: the folder may have been moved.
    instance.path = dir.to_path_buf();
    Ok(instance)
}

pub(crate) fn write_instance(instance: &Instance) -> AppResult<()> {
    std::fs::create_dir_all(&instance.path)?;
    let path = instance.path.join(INSTANCE_FILE);
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(instance)?)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::BuildSelector;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("nebula-store-{label}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn sample(dir: &Path) -> Instance {
        Instance::new(
            "local",
            crate::game::MINECRAFT,
            "Survival",
            BuildSelector::new("paper", "1.21.4"),
            dir.to_path_buf(),
        )
    }

    #[test]
    fn instance_file_round_trips() {
        let dir = temp_dir("roundtrip");
        let instance = sample(&dir);
        write_instance(&instance).expect("write");

        let read = read_instance(&dir).expect("read");
        assert_eq!(read.id, instance.id);
        assert_eq!(read.build.distribution, "paper");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_moved_directory_wins_over_the_recorded_path() {
        let dir = temp_dir("moved");
        let mut instance = sample(&dir);
        instance.path = PathBuf::from("D:/somewhere/that/no/longer/exists");
        std::fs::write(
            dir.join(INSTANCE_FILE),
            serde_json::to_vec_pretty(&instance).expect("serialize"),
        )
        .expect("write");

        let read = read_instance(&dir).expect("read");
        assert_eq!(read.path, dir, "the folder we found it in is where it lives");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn legacy_node_ids_are_migrated_on_every_record() {
        let root = temp_dir("migrate");
        let store = InstanceStore::load_from(root.join("index.json")).await.expect("store");
        let mine = sample(&root.join("a"));
        let mut other = sample(&root.join("b"));
        other.node_id = "someone-else".into();
        store.put(mine.clone()).await.expect("put");
        store.put(other.clone()).await.expect("put");

        let moved = store.migrate_node_id("local", "node-123").await.expect("migrate");
        assert_eq!(moved, 1);
        assert_eq!(store.get(&mine.id).await.expect("get").node_id, "node-123");
        assert_eq!(store.get(&other.id).await.expect("get").node_id, "someone-else");
        // Persisted, not just in memory.
        assert_eq!(read_instance(&root.join("a")).expect("read").node_id, "node-123");
        assert_eq!(store.migrate_node_id("local", "node-123").await.expect("again"), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn records_from_before_transfers_read_as_active_generation_zero() {
        let dir = temp_dir("legacy-record");
        let mut value = serde_json::to_value(sample(&dir)).expect("json");
        let object = value.as_object_mut().expect("object");
        object.remove("generation");
        object.remove("transfer");
        object.remove("forked_from");
        std::fs::write(dir.join(INSTANCE_FILE), serde_json::to_vec(&value).expect("bytes")).expect("write");
        let read = read_instance(&dir).expect("read");
        assert_eq!(read.generation, 0);
        assert_eq!(read.transfer, crate::model::TransferState::Active);
        assert!(read.forked_from.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_instance_file_is_a_typed_error() {
        let dir = temp_dir("missing");
        let err = read_instance(&dir).expect_err("nothing there");
        assert_eq!(err.code, ErrorCode::FileNotFound);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
