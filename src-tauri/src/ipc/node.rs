//! The machine itself: its name, its health, and pairing.

use crate::error::AppResult;
use crate::ipc::State;
use crate::model::{Node, NodeHealth};
use crate::node::health::{self, Requirement};
use crate::node::pairing::{self, PairingCode};

#[tauri::command]
pub async fn node_get(state: State<'_>) -> AppResult<Node> {
    Ok(state.node().await)
}

#[tauri::command]
pub async fn node_rename(state: State<'_>, name: String) -> AppResult<Node> {
    state.rename_node(name).await
}

#[tauri::command]
pub async fn node_health(state: State<'_>) -> AppResult<NodeHealth> {
    let running = state.processes.running_pids().await.len() as u32;
    Ok(health::sample(running))
}

#[tauri::command]
pub async fn node_requirements() -> AppResult<Vec<Requirement>> {
    health::check_requirements()
}

/// Issue a short-lived pairing code. Nothing consumes it yet — remote access
/// is not shipped — but the code path is the real one, expiry included.
#[tauri::command]
pub async fn node_pairing_code(state: State<'_>) -> AppResult<PairingCode> {
    pairing::issue(&state.node().await.id)
}
