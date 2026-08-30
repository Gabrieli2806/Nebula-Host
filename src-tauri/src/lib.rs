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
pub mod ipc;
pub mod model;
pub mod net;
pub mod node;
pub mod runtime;
pub mod service;
pub mod store;

use std::sync::Arc;

use tauri::{Emitter, Manager};
use tracing_subscriber::EnvFilter;

use crate::model::Node;
use crate::service::InstanceService;

/// The single channel the UI listens on.
const EVENT_NAME: &str = "nebula:event";

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
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
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
                handle.manage(service);

                Ok::<(), error::AppError>(())
            })?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Stop every server before the process goes away; a server
                // killed with the app loses whatever it had not saved.
                if let Some(service) = window.app_handle().try_state::<Arc<InstanceService>>() {
                    let service = service.inner().clone();
                    tauri::async_runtime::block_on(async move {
                        service.wake.stop_all().await;
                        service.forwarder.release_all().await;
                        service.processes.stop_all("stop").await;
                    });
                }
            }
        })
        .invoke_handler(ipc_handlers!())
        .run(tauri::generate_context!())
        .expect("error while running Nebula Host");
}

/// Pipe service events to the webview. One subscription, one event name — the
/// UI switches on the payload rather than registering a dozen listeners.
fn forward_events(handle: tauri::AppHandle, service: Arc<InstanceService>) {
    let mut events = service.subscribe();
    tauri::async_runtime::spawn(async move {
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

/// So `AppEvent` stays reachable for consumers of this crate as a library.
pub use service::AppEvent as Event;
