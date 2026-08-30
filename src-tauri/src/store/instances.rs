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

const INSTANCE_FILE: &str = "nebula-instance.json";
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
        let index_path = paths::config_dir()?.join(INDEX_FILE);
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
    pub async fn remove(&self, id: &str, delete_files: bool) -> AppResult<()> {
        let instance = self
            .instances
            .write()
            .await
            .remove(id)
            .ok_or_else(|| AppError::new(ErrorCode::InstanceNotFound).with("instance", id))?;

        if delete_files {
            std::fs::remove_dir_all(&instance.path)?;
        } else {
            let _ = std::fs::remove_file(instance.path.join(INSTANCE_FILE));
        }

        self.write_index().await
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

fn read_instance(dir: &Path) -> AppResult<Instance> {
    let text = std::fs::read_to_string(dir.join(INSTANCE_FILE))?;
    let mut instance: Instance = serde_json::from_str(&text)?;
    // The directory on disk is the truth: the folder may have been moved.
    instance.path = dir.to_path_buf();
    Ok(instance)
}

fn write_instance(instance: &Instance) -> AppResult<()> {
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

    #[test]
    fn a_missing_instance_file_is_a_typed_error() {
        let dir = temp_dir("missing");
        let err = read_instance(&dir).expect_err("nothing there");
        assert_eq!(err.code, ErrorCode::FileNotFound);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
