//! The system tray: the app keeps hosting when its window is closed, and the
//! tray is how you get back to it, start or stop a server, or really quit.
//!
//! On Windows the icon opens a small panel, a webview by the notification area
//! (src/features/tray): the classic menu can only list strings and looks
//! nothing like the app. Elsewhere it stays a native menu, because many Linux
//! desktops never tell an app its tray icon was clicked. The menu is rebuilt
//! whenever a server's status changes, so each server's entry always shows its
//! current state and offers the action that fits it.

use std::sync::Arc;

#[cfg(not(windows))]
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
#[cfg(not(windows))]
use tauri::Wry;
use tauri::{AppHandle, Manager};

use crate::model::InstanceStatus;
use crate::service::InstanceService;
#[cfg(not(windows))]
use crate::service::AppEvent;
#[cfg(not(windows))]
use crate::store::settings;

const TRAY_ID: &str = "nebula-tray";

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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

/// Start, stop or restart a server, or stop them all, from the tray.
pub fn server_action(app: &AppHandle, action: &str, instance_id: &str) {
    let Some(service) = app.try_state::<Arc<InstanceService>>() else { return };
    let service = service.inner().clone();
    let app = app.clone();
    let (action, instance_id) = (action.to_string(), instance_id.to_string());
    tauri::async_runtime::spawn(async move {
        if action == "stop_all" {
            for view in service.list().await.unwrap_or_default() {
                if matches!(view.state.status, InstanceStatus::Online | InstanceStatus::Starting) {
                    let _ = service.stop(&view.instance.id).await;
                }
            }
            return;
        }
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

#[cfg(windows)]
pub fn install(app: &AppHandle, _service: Arc<InstanceService>) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Nebula Host")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left | MouseButton::Right, button_state: MouseButtonState::Up, position, .. } = event {
                panel::toggle(tray.app_handle(), position.x, position.y);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg(not(windows))]
pub fn install(app: &AppHandle, service: Arc<InstanceService>) -> tauri::Result<()> {
    let menu = tauri::async_runtime::block_on(build_menu(app, &service))?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Nebula Host")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref().to_string();
            match id.as_str() {
                "open" => show_main(app),
                "quit" => quit(app),
                "stop_all" => server_action(app, "stop_all", ""),
                other => {
                    if let Some((action, instance_id)) = other.split_once(':') {
                        server_action(app, action, instance_id);
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

/// The panel by the notification area: a borderless window kept hidden between
/// uses, so it opens at once. It closes when it loses focus, as a menu does.
#[cfg(windows)]
pub mod panel {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

    pub const LABEL: &str = "tray";
    const WIDTH: f64 = 340.0;
    /// Gap between the panel and the screen edge it opens against.
    const EDGE_GAP: i32 = 8;

    static ANCHOR: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
    static LAST_HIDDEN: Mutex<Option<Instant>> = Mutex::new(None);
    /// The content's height when last measured, in CSS pixels.
    static LAST_HEIGHT: Mutex<f64> = Mutex::new(200.0);

    /// The click that dismisses an open panel arrives after it lost focus; it
    /// must not open it again.
    const REOPEN_AFTER: Duration = Duration::from_millis(300);

    pub fn toggle(app: &AppHandle, x: f64, y: f64) {
        if let Ok(mut anchor) = ANCHOR.lock() {
            *anchor = (x, y);
        }
        if let Some(window) = app.get_webview_window(LABEL) {
            if window.is_visible().unwrap_or(false) {
                hide(&window);
            } else if !LAST_HIDDEN.lock().ok().and_then(|hidden| *hidden).is_some_and(|at| at.elapsed() < REOPEN_AFTER) {
                // Open at once with what it last showed; the page then
                // brings it up to date and refits.
                let height = LAST_HEIGHT.lock().map(|height| *height).unwrap_or(200.0);
                fit(app, height, true);
                let _ = app.emit_to(LABEL, "nebula:tray-open", ());
            }
            return;
        }
        // The page shows itself once it has measured its content.
        let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#/tray-panel".into()))
            .title("Nebula Host")
            .inner_size(WIDTH, 200.0)
            .decorations(false)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .shadow(true)
            .visible(false)
            .build();
        if let Err(error) = built {
            tracing::warn!(%error, "tray panel unavailable");
            super::show_main(app);
        }
    }

    pub fn hide(window: &WebviewWindow) {
        if let Ok(mut hidden) = LAST_HIDDEN.lock() {
            *hidden = Some(Instant::now());
        }
        let _ = window.hide();
    }

    /// Size the panel to its content against the corner the icon is in: up and
    /// to the left of it, inside the work area wherever the taskbar is.
    pub fn fit(app: &AppHandle, height: f64, show: bool) {
        let Some(window) = app.get_webview_window(LABEL) else { return };
        if let Ok(mut last) = LAST_HEIGHT.lock() {
            *last = height;
        }
        let (x, y) = ANCHOR.lock().map(|anchor| *anchor).unwrap_or_default();
        let Ok(Some(monitor)) = app.monitor_from_point(x, y) else { return };
        let scale = monitor.scale_factor();
        let (width, height) = ((WIDTH * scale).ceil() as i32, (height * scale).ceil() as i32);
        let work = monitor.work_area();
        let (left, top) = (work.position.x, work.position.y);
        let (right, bottom) = (left + work.size.width as i32, top + work.size.height as i32);
        let px = (x as i32 - width / 2).clamp(left + EDGE_GAP, (right - width - EDGE_GAP).max(left + EDGE_GAP));
        let mut py = y as i32 - height - EDGE_GAP;
        if py < top + EDGE_GAP {
            py = (y as i32 + EDGE_GAP).min(bottom - height - EDGE_GAP).max(top + EDGE_GAP);
        }
        let _ = window.set_size(PhysicalSize::new(width as u32, height as u32));
        let _ = window.set_position(PhysicalPosition::new(px, py));
        if show {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}
