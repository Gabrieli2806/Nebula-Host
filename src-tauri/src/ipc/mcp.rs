//! Settings for the MCP endpoint (AI agents).

use std::sync::Arc;

use crate::error::AppResult;
use crate::ipc::State;
use crate::mcp::{McpServer, McpStatus};
use crate::store::peers::{self, Peer};
use crate::store::settings;

type Mcp<'a> = tauri::State<'a, Arc<McpServer>>;

#[tauri::command]
pub async fn mcp_status(mcp: Mcp<'_>) -> AppResult<McpStatus> {
    mcp.status().await
}

/// Turn the endpoint on or off, move it, or switch read-only, then apply.
#[tauri::command]
pub async fn mcp_configure(
    state: State<'_>,
    mcp: Mcp<'_>,
    enabled: bool,
    read_only: bool,
    port: Option<u16>,
    launcher_play: Option<bool>,
    remote: Option<bool>,
) -> AppResult<McpStatus> {
    let mut config = settings::load()?;
    config.mcp_enabled = enabled;
    config.mcp_read_only = read_only;
    if let Some(play) = launcher_play {
        config.mcp_launcher_play = play;
    }
    if let Some(remote) = remote {
        config.mcp_remote = remote;
    }
    if let Some(port) = port.filter(|p| *p >= 1024) {
        config.mcp_port = port;
    }
    settings::save(&config)?;
    mcp.reconcile(state.inner().clone()).await
}

/// The window's list of paired machines, handed over so an agent connected
/// here can reach them. Called at startup and whenever the list changes.
#[tauri::command]
pub async fn mcp_sync_machines(machines: Vec<Peer>) -> AppResult<()> {
    if peers::load()? != machines {
        peers::save(&machines)?;
    }
    Ok(())
}
