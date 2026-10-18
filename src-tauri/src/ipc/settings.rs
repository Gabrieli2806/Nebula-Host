//! App settings and per-instance game settings.

use std::collections::BTreeMap;

use crate::error::AppResult;
use crate::ipc::State;
use crate::runtime::java::{self, JavaInstallation};
use crate::service::java::JavaRuntimes;
use crate::service::properties::{self, SettingValue};
use crate::store::settings::{self as store, AppSettings};

#[tauri::command]
pub async fn settings_get() -> AppResult<AppSettings> {
    store::load()
}

#[tauri::command]
pub async fn settings_set(mut settings: AppSettings) -> AppResult<AppSettings> {
    // The MCP endpoint is configured through its own commands (it has a live
    // server and a secret behind it); a general settings save must never
    // switch it off or drop its token just because the UI did not send them.
    let current = store::load()?;
    settings.mcp_enabled = current.mcp_enabled;
    settings.mcp_port = current.mcp_port;
    settings.mcp_token = current.mcp_token;
    settings.mcp_read_only = current.mcp_read_only;
    settings.remote_enabled = current.remote_enabled;
    settings.remote_port = current.remote_port;
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
    state.ensure_not_transferring(&instance_id).await?;
    let instance = state.store.get(&instance_id).await?;
    let manifest = state.registry.manifest(&instance.game)?;
    properties::write(&instance.path, &manifest, &values)
}

/// Check a Java path the user typed, and report what it actually is.
#[tauri::command]
pub async fn java_probe(path: String) -> AppResult<JavaInstallation> {
    java::probe(std::path::Path::new(&path))
}

/// Every JVM on the machine, which one this server's game version wants, and
/// which one it is set to use.
#[tauri::command]
pub async fn java_runtimes(
    state: State<'_>,
    instance_id: String,
    refresh: Option<bool>,
) -> AppResult<JavaRuntimes> {
    state.java_runtimes(&instance_id, refresh.unwrap_or(false)).await
}

/// Download Eclipse Temurin of a major version. Progress arrives as
/// `javaDownload` events.
#[tauri::command]
pub async fn java_download(state: State<'_>, major: u32) -> AppResult<JavaInstallation> {
    state.java_download(major).await
}

/// Settings › Java: the four Java slots and every JVM found.
#[tauri::command]
pub async fn java_overview(refresh: Option<bool>) -> AppResult<crate::service::java::JavaOverview> {
    crate::service::java::java_overview(refresh.unwrap_or(false)).await
}

/// Settings › Diagnostics: the tail of the app's own log.
#[tauri::command]
pub async fn app_log(lines: Option<usize>) -> AppResult<crate::logging::AppLog> {
    Ok(crate::logging::tail(lines.unwrap_or(400).min(5000)))
}

/// Open the logs folder on this PC in the system file manager.
#[tauri::command]
pub async fn app_open_log_folder(app: tauri::AppHandle) -> AppResult<()> {
    let folder = crate::logging::folder()?;
    crate::ipc::open_folder(&app, std::path::Path::new(&folder))
}
