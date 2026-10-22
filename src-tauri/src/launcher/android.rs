//! Private Rust → Android game bridge. No account tokens cross the webview IPC.
use crate::error::{AppError, AppResult, ErrorCode};
use serde_json::Value;
use std::sync::OnceLock;
use tauri::plugin::{PluginHandle, TauriPlugin};

static ENGINE: OnceLock<PluginHandle<tauri::Wry>> = OnceLock::new();

pub fn init() -> TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("nebula-game")
        .setup(|_, api| {
            let handle = api.register_android_plugin("net.g2806.nebulahost", "GamePlugin")?;
            ENGINE.set(handle).map_err(|_| "Android game engine already initialized")?;
            Ok(())
        }).build()
}

fn call(command: &str, args: Value) -> AppResult<Value> {
    ENGINE.get().ok_or_else(|| AppError::new(ErrorCode::GameNotSupported))?
        .run_mobile_plugin(command, args).map_err(|e| AppError::new(ErrorCode::InstanceStartFailed).detail(e))
}

pub fn start(payload: Value) -> AppResult<()> { call("start", payload).map(|_| ()) }
pub fn stop() -> AppResult<()> { call("stop", serde_json::json!({})).map(|_| ()) }
pub fn focus_app() -> AppResult<()> { call("focusApp", serde_json::json!({})).map(|_| ()) }
pub fn resume() -> AppResult<()> { call("resume", serde_json::json!({})).map(|_| ()) }
pub fn running() -> AppResult<bool> { Ok(call("status", serde_json::json!({}))?["running"].as_bool().unwrap_or(false)) }
pub fn pin_shortcut(payload: Value) -> AppResult<()> { call("pinShortcut", payload).map(|_| ()) }
pub fn take_shortcut() -> AppResult<Value> { call("takeShortcut", serde_json::json!({})) }
