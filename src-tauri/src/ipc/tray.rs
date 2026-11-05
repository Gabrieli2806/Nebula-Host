//! The Windows tray panel's door to the app. It always speaks about this PC,
//! even while the main window manages another machine.

use tauri::AppHandle;

use crate::error::AppResult;
use crate::ipc::State;

/// The panel measured its content. `show` when it was just asked to open.
#[tauri::command]
pub fn tray_panel_fit(app: AppHandle, height: f64, show: bool) {
    #[cfg(windows)]
    crate::tray::panel::fit(&app, height, show);
    #[cfg(not(windows))]
    let _ = (app, height, show);
}

#[tauri::command]
pub fn tray_panel_hide(app: AppHandle) {
    #[cfg(windows)]
    {
        use tauri::Manager;
        if let Some(window) = app.get_webview_window(crate::tray::panel::LABEL) {
            crate::tray::panel::hide(&window);
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

/// Bring up the main window: on a server, on Settings, or as it was.
/// `reload`: the panel switched the machine the window manages.
#[tauri::command]
pub async fn tray_open(
    app: AppHandle,
    state: State<'_>,
    settings: bool,
    instance_id: Option<String>,
    remote: Option<bool>,
    reload: Option<bool>,
) -> AppResult<()> {
    #[cfg(desktop)]
    {
        use tauri::Manager;
        // The id goes into the page's address: one of this PC's servers, or
        // for another machine, an id's characters and nothing else.
        let route = match instance_id {
            Some(id) if remote.unwrap_or(false) => {
                if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    return Err(crate::error::AppError::new(crate::error::ErrorCode::InvalidInput).with("instance", id));
                }
                Some(format!("#/servers/{id}/overview"))
            }
            Some(id) => Some(format!("#/servers/{}/overview", state.store.get(&id).await?.id)),
            None if settings => Some("#/settings".to_string()),
            None => None,
        };
        crate::tray::show_main(&app);
        if let Some(main) = app.get_webview_window("main") {
            let mut script = route.map(|route| format!("window.location.hash = '{route}';")).unwrap_or_default();
            if reload.unwrap_or(false) {
                script.push_str("window.location.reload();");
            }
            if !script.is_empty() {
                let _ = main.eval(script);
            }
        }
    }
    #[cfg(not(desktop))]
    let _ = (app, state, settings, instance_id, remote, reload);
    Ok(())
}

#[tauri::command]
pub fn tray_server_action(app: AppHandle, action: String, instance_id: String) {
    #[cfg(desktop)]
    crate::tray::server_action(&app, &action, &instance_id);
    #[cfg(not(desktop))]
    let _ = (app, action, instance_id);
}

/// Not async: quitting blocks on the servers stopping, which needs to run
/// outside the async runtime.
#[tauri::command]
pub fn tray_quit(app: AppHandle) {
    #[cfg(desktop)]
    crate::tray::quit(&app);
    #[cfg(not(desktop))]
    let _ = app;
}
