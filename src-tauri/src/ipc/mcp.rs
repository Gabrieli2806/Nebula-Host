//! Settings for the MCP endpoint (AI assistants).

use std::sync::Arc;

use crate::error::AppResult;
use crate::ipc::State;
use crate::mcp::{self, McpServer, McpStatus};
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
) -> AppResult<McpStatus> {
    let mut config = settings::load()?;
    config.mcp_enabled = enabled;
    config.mcp_read_only = read_only;
    if let Some(play) = launcher_play {
        config.mcp_launcher_play = play;
    }
    if let Some(port) = port.filter(|p| *p >= 1024) {
        config.mcp_port = port;
    }
    settings::save(&config)?;
    mcp.reconcile(state.inner().clone()).await
}

/// Issue a new token; every connected assistant must be given the new one.
#[tauri::command]
pub async fn mcp_regenerate_token(state: State<'_>, mcp: Mcp<'_>) -> AppResult<McpStatus> {
    let mut config = settings::load()?;
    config.mcp_token = Some(mcp::new_token());
    settings::save(&config)?;
    mcp.reconcile(state.inner().clone()).await
}
