//! Letting other devices control this machine, and the UPnP self-test.

use std::sync::Arc;

use crate::error::AppResult;
use crate::ipc::State;
use crate::model::Node;
use crate::net::upnp::{self, UpnpCheck};
use crate::remote::{RemoteServer, RemoteStatus};
use crate::store::settings;

type Remote<'a> = tauri::State<'a, Arc<RemoteServer>>;

#[tauri::command]
pub async fn remote_status(remote: Remote<'_>) -> AppResult<RemoteStatus> {
    remote.status().await
}

#[tauri::command]
pub async fn remote_configure(
    state: State<'_>,
    remote: Remote<'_>,
    enabled: bool,
    web_panel: Option<bool>,
) -> AppResult<RemoteStatus> {
    let mut config = settings::load()?;
    config.remote_enabled = enabled;
    if let Some(web_panel) = web_panel {
        config.web_panel = web_panel;
    }
    settings::save(&config)?;
    remote.reconcile(state.inner().clone()).await
}

#[tauri::command]
pub async fn node_forget_device(state: State<'_>, device_id: String) -> AppResult<Node> {
    state.remove_paired_device(&device_id).await
}

#[tauri::command]
pub async fn upnp_check() -> AppResult<UpnpCheck> {
    Ok(upnp::check().await)
}
