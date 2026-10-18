//! The system tray: the app keeps hosting when its window is closed, and the
//! tray is how you get back to it, start or stop a server, or really quit.
//!
//! The menu is rebuilt whenever a server's status changes, so each server's
//! entry always shows its current state and offers the action that fits it.

use std::sync::Arc;

use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::model::InstanceStatus;
use crate::service::{AppEvent, InstanceService};
use crate::store::settings;

const TRAY_ID: &str = "nebula-tray";

struct Copy {
    open: &'static str,
    servers: &'static str,
    none: &'static str,
    start: &'static str,
    stop: &'static str,
    restart: &'static str,
    stop_all: &'static str,
    quit: &'static str,
}

const EN: Copy = Copy {
    open: "Open Nebula Host",
    servers: "Servers",
    none: "No servers yet",
    start: "Start",
    stop: "Stop",
    restart: "Restart",
    stop_all: "Stop all servers",
    quit: "Quit Nebula Host",
};

const ES: Copy = Copy {
    open: "Abrir Nebula Host",
    servers: "Servidores",
    none: "Todavía no hay servidores",
    start: "Arrancar",
    stop: "Detener",
    restart: "Reiniciar",
    stop_all: "Detener todos los servidores",
    quit: "Salir de Nebula Host",
};

fn copy() -> &'static Copy {
    let language = settings::load().map(|s| s.language).unwrap_or_default();
    let spanish = match language.as_str() {
        "es" => true,
        "en" => false,
        _ => sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("es")),
    };
    if spanish {
        &ES
    } else {
        &EN
    }
}

fn status_label(status: InstanceStatus, spanish: bool) -> &'static str {
    match (status, spanish) {
        (InstanceStatus::Online, false) => "online",
        (InstanceStatus::Online, true) => "en línea",
        (InstanceStatus::Starting, false) => "starting",
        (InstanceStatus::Starting, true) => "arrancando",
        (InstanceStatus::Stopping, false) => "stopping",
        (InstanceStatus::Stopping, true) => "deteniéndose",
        (InstanceStatus::Sleeping, false) => "sleeping",
        (InstanceStatus::Sleeping, true) => "durmiendo",
        (InstanceStatus::Crashed, false) => "error",
        (InstanceStatus::Crashed, true) => "error",
        (InstanceStatus::Offline, false) => "offline",
        (InstanceStatus::Offline, true) => "apagado",
    }
}

async fn build_menu(app: &AppHandle, service: &InstanceService) -> tauri::Result<Menu<Wry>> {
    let copy = copy();
    let spanish = std::ptr::eq(copy, &ES);
    let mut servers = SubmenuBuilder::new(app, copy.servers);
    let list = service.list().await.unwrap_or_default();
    if list.is_empty() {
        servers = servers.item(&MenuItemBuilder::with_id("noop", copy.none).enabled(false).build(app)?);
    }
    for view in &list {
        let id = &view.instance.id;
        let status = view.state.status;
        let running = matches!(status, InstanceStatus::Online | InstanceStatus::Starting);
        let label = format!("{} — {}", view.instance.name, status_label(status, spanish));
        let mut sub = SubmenuBuilder::new(app, label);
        if running {
            sub = sub
                .item(&MenuItemBuilder::with_id(format!("stop:{id}"), copy.stop).build(app)?)
                .item(&MenuItemBuilder::with_id(format!("restart:{id}"), copy.restart).build(app)?);
        } else {
            sub = sub.item(
                &MenuItemBuilder::with_id(format!("start:{id}"), copy.start)
                    .enabled(status != InstanceStatus::Stopping)
                    .build(app)?,
            );
        }
        servers = servers.item(&sub.build()?);
    }

    MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id("open", copy.open).build(app)?)
        .separator()
        .item(&servers.build()?)
        .item(&MenuItemBuilder::with_id("stop_all", copy.stop_all).build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("quit", copy.quit).build(app)?)
        .build()
}

/// A start the tray cannot finish on its own: the EULA needs the user's
/// tick, or the server is an older copy left behind by a transfer. The
/// window comes up and asks (or shows why), instead of nothing happening.
fn refused_in_window(app: &AppHandle, instance_id: &str, error: &crate::error::AppError) {
    use tauri::Emitter;
    let reason = error.params.get("reason").map(String::as_str);
    if !matches!(reason, Some("agreement_required" | "transferred_out")) {
        return;
    }
    show_main(app);
    let _ = app.emit(
        "nebula:tray-refused",
        serde_json::json!({
            "instanceId": instance_id,
            "reason": reason,
            "url": error.params.get("url"),
        }),
    );
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Stop everything that holds a port or a process, then exit.
pub fn quit(app: &AppHandle) {
    if let Some(service) = app.try_state::<Arc<InstanceService>>() {
        let service = service.inner().clone();
        tauri::async_runtime::block_on(async move {
            service.wake.stop_all().await;
            service.forwarder.release_all().await;
            service.processes.stop_all("stop").await;
        });
    }
    app.exit(0);
}

pub fn install(app: &AppHandle, service: Arc<InstanceService>) -> tauri::Result<()> {
    let menu = tauri::async_runtime::block_on(build_menu(app, &service))?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Nebula Host")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref().to_string();
            let app = app.clone();
            match id.as_str() {
                "open" => show_main(&app),
                "quit" => quit(&app),
                "stop_all" => {
                    if let Some(service) = app.try_state::<Arc<InstanceService>>() {
                        let service = service.inner().clone();
                        tauri::async_runtime::spawn(async move {
                            for view in service.list().await.unwrap_or_default() {
                                if matches!(view.state.status, InstanceStatus::Online | InstanceStatus::Starting) {
                                    let _ = service.stop(&view.instance.id).await;
                                }
                            }
                        });
                    }
                }
                other => {
                    let Some((action, instance_id)) = other.split_once(':') else { return };
                    let (action, instance_id) = (action.to_string(), instance_id.to_string());
                    if let Some(service) = app.try_state::<Arc<InstanceService>>() {
                        let service = service.inner().clone();
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let result = match action.as_str() {
                                "start" => service.start(&instance_id).await,
                                "stop" => service.stop(&instance_id).await,
                                "restart" => service.restart(&instance_id).await,
                                _ => Ok(()),
                            };
                            if let Err(error) = result {
                                tracing::warn!(%instance_id, %error, "tray action failed");
                                refused_in_window(&app, &instance_id, &error);
                            }
                        });
                    }
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;

    // Keep the menu in step with server status.
    let app = app.clone();
    let mut events = service.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(AppEvent::Status { .. }) => {
                    if let (Ok(menu), Some(tray)) = (build_menu(&app, &service).await, app.tray_by_id(TRAY_ID)) {
                        let _ = tray.set_menu(Some(menu));
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}
