//! Nebula Host — a self-hosted game-server manager.
//!
//! The shape, top to bottom:
//!
//! - [`model`] — nodes and instances, with no game in them.
//! - [`game`] — manifests (data) and adapters (the little that cannot be).
//! - [`runtime`] — starting processes and reading their console.
//! - [`net`] — ports, LAN, router forwarding, wake-on-join.
//! - [`store`] — what is on disk.
//! - [`service`] — the rules that tie those together.
//! - [`ipc`] — a thin, typed door for the UI.
//!
//! There is exactly one build profile: local. No cloud branch, no
//! `if is_cloud` in a leaf component. If a hosted build ever exists, it
//! replaces the node's transport at the top and nothing below changes.

pub mod error;
pub mod game;
#[cfg(feature = "desktop")]
pub mod ipc;
pub mod mcp;
pub mod model;
pub mod net;
pub mod node;
pub mod remote;
pub mod runtime;
pub mod service;
pub mod store;
#[cfg(feature = "desktop")]
pub mod tray;

use std::sync::Arc;

#[cfg(feature = "desktop")]
use tauri::{Emitter, Manager};
#[cfg(feature = "desktop")]
use tracing_subscriber::EnvFilter;

#[cfg(feature = "desktop")]
use crate::model::Node;
use crate::service::InstanceService;

/// The single channel the UI listens on.
#[cfg(feature = "desktop")]
const EVENT_NAME: &str = "nebula:event";

#[cfg(feature = "desktop")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("NEBULA_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second launch focuses the window that is already hosting.
            tray::show_main(app);
        }))
        .setup(|app| {
            let handle = app.handle().clone();

            tauri::async_runtime::block_on(async move {
                let node = match store::nodes::load()? {
                    Some(node) => node,
                    None => {
                        // First run: name the machine after itself. The user
                        // renames it in the first-run flow — there is no
                        // account to name instead.
                        let node = Node::local(store::nodes::suggested_name());
                        store::nodes::save(&node)?;
                        node
                    }
                };

                let service = InstanceService::new(node).await?;
                forward_events(handle.clone(), service.clone());
                service::poller::spawn(service.clone());
                restore_sleepers(service.clone());
                handle.manage(service.clone());

                let mcp = Arc::new(mcp::McpServer::new());
                if let Err(error) = mcp.reconcile(service.clone()).await {
                    tracing::warn!(%error, "MCP endpoint not started");
                }
                handle.manage(mcp);

                let remote = Arc::new(remote::RemoteServer::new());
                if let Err(error) = remote.reconcile(service.clone()).await {
                    tracing::warn!(%error, "remote API not started");
                }
                handle.manage(remote);

                Ok::<(), error::AppError>(())
            })?;

            if let Some(service) = app.try_state::<Arc<InstanceService>>() {
                if let Err(error) = tray::install(app.handle(), service.inner().clone()) {
                    tracing::warn!(%error, "system tray unavailable");
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // With "keep running in the tray" (the default) closing the
                // window only hides it: servers keep hosting, and the tray is
                // the way back or out. Otherwise stop every server before the
                // process goes away — a server killed with the app loses
                // whatever it had not saved.
                let in_tray = store::settings::load().map(|s| s.run_in_tray).unwrap_or(true)
                    && window.app_handle().tray_by_id("nebula-tray").is_some();
                if in_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    tray::quit(window.app_handle());
                }
            }
        })
        .invoke_handler(ipc_handlers!())
        .run(tauri::generate_context!())
        .expect("error while running Nebula Host");
}

#[cfg(feature = "desktop")]
/// Pipe service events to the webview. One subscription, one event name — the
/// UI switches on the payload rather than registering a dozen listeners.
fn forward_events(handle: tauri::AppHandle, service: Arc<InstanceService>) {
    let mut events = service.subscribe();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let _ = handle.emit(EVENT_NAME, &event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "UI event listener fell behind");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Servers that wake on join were asleep when the app last closed: put them
/// back to sleep on launch, so a join wakes them instead of finding nothing.
pub fn restore_sleepers(service: Arc<InstanceService>) {
    tokio::spawn(async move {
        for view in service.list().await.unwrap_or_default() {
            let wakeable = view.instance.automation.wake_on_join
                && view.instance.install.is_some()
                && view.state.status == model::InstanceStatus::Offline
                && service.registry.manifest(&view.instance.game).is_ok_and(|m| m.wake.is_some());
            if wakeable {
                if let Err(error) = service.sleep(&view.instance.id).await {
                    tracing::warn!(instance_id = %view.instance.id, %error, "could not restore sleep on launch");
                }
            }
        }
    });
}

/// So `AppEvent` stays reachable for consumers of this crate as a library.
pub use service::AppEvent as Event;
