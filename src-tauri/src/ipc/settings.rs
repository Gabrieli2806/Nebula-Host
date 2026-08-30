//! App settings and per-instance game settings.

use std::collections::BTreeMap;

use crate::error::AppResult;
use crate::ipc::State;
use crate::runtime::java::{self, JavaInstallation};
use crate::service::properties::{self, SettingValue};
use crate::store::settings::{self as store, AppSettings};

#[tauri::command]
pub async fn settings_get() -> AppResult<AppSettings> {
    store::load()
}

#[tauri::command]
pub async fn settings_set(settings: AppSettings) -> AppResult<AppSettings> {
    store::save(&settings)?;
    store::load()
}

/// The game's own settings, described by its manifest and filled in from its
/// config file.
#[tauri::command]
pub async fn instance_settings_get(
    state: State<'_>,
    instance_id: String,
) -> AppResult<Vec<SettingValue>> {
    let instance = state.store.get(&instance_id).await?;
    let manifest = state.registry.manifest(&instance.game)?;
    properties::read(&instance.path, &manifest)
}

#[tauri::command]
pub async fn instance_settings_set(
    state: State<'_>,
    instance_id: String,
    values: BTreeMap<String, String>,
) -> AppResult<()> {
    let instance = state.store.get(&instance_id).await?;
    let manifest = state.registry.manifest(&instance.game)?;
    properties::write(&instance.path, &manifest, &values)
}

/// Check a Java path the user typed, and report what it actually is.
#[tauri::command]
pub async fn java_probe(path: String) -> AppResult<JavaInstallation> {
    java::probe(std::path::Path::new(&path))
}
