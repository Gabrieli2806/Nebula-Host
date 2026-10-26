//! Server lifecycle commands.



use crate::error::AppResult;
use crate::ipc::State;
use crate::model::{ConsoleLine, Instance, InstanceView};
use crate::service::completion::CompletionSet;
use crate::service::icon::IconFit;
use crate::service::logs::{LogFiles, LogSource, SharedLog};
use crate::service::CreateInstanceRequest;

#[tauri::command]
pub async fn instances_list(state: State<'_>) -> AppResult<Vec<InstanceView>> {
    state.list().await
}

#[tauri::command]
pub async fn instance_get(state: State<'_>, instance_id: String) -> AppResult<InstanceView> {
    state.view(&instance_id).await
}

#[tauri::command]
pub async fn instance_create(
    state: State<'_>,
    request: CreateInstanceRequest,
) -> AppResult<Instance> {
    state.create(request).await
}

/// Download and lay out the files. Progress arrives as `install` events, not
/// as a return value — this can take 90 seconds.
#[tauri::command]
pub async fn instance_install(state: State<'_>, instance_id: String) -> AppResult<()> {
    state.install(&instance_id).await
}

/// The user accepted the game's agreement (the Minecraft EULA) in the UI.
#[tauri::command]
pub async fn instance_accept_agreement(state: State<'_>, instance_id: String) -> AppResult<()> {
    state.accept_agreement(&instance_id).await
}

#[tauri::command]
pub async fn instance_start(state: State<'_>, instance_id: String) -> AppResult<()> {
    state.inner().start(&instance_id).await
}

#[tauri::command]
pub async fn instance_stop(state: State<'_>, instance_id: String) -> AppResult<()> {
    state.stop(&instance_id).await
}

#[tauri::command]
pub async fn instance_restart(state: State<'_>, instance_id: String) -> AppResult<()> {
    state.inner().restart(&instance_id).await
}

/// Put a stopped server to sleep so a join wakes it.
#[tauri::command]
pub async fn instance_sleep(state: State<'_>, instance_id: String) -> AppResult<()> {
    state.inner().sleep(&instance_id).await
}

#[tauri::command]
pub async fn instance_delete(
    state: State<'_>,
    instance_id: String,
    delete_files: bool,
) -> AppResult<()> {
    state.delete(&instance_id, delete_files).await
}

#[tauri::command]
pub async fn instance_command(
    state: State<'_>,
    instance_id: String,
    command: String,
) -> AppResult<()> {
    state.send_command(&instance_id, &command).await
}

/// Autocomplete for the console input: commands (builtin + what the running
/// server reports) and online players.
#[tauri::command]
pub async fn console_completions(
    state: State<'_>,
    instance_id: String,
    refresh: Option<bool>,
) -> AppResult<CompletionSet> {
    state.completions(&instance_id, refresh.unwrap_or(false)).await
}

#[tauri::command]
pub async fn instance_console(
    state: State<'_>,
    instance_id: String,
    limit: Option<usize>,
) -> AppResult<Vec<ConsoleLine>> {
    Ok(state.console_tail(&instance_id, limit.unwrap_or(500)).await)
}

pub use crate::service::InstancePatch;

#[tauri::command]
pub async fn instance_update(
    state: State<'_>,
    instance_id: String,
    patch: InstancePatch,
) -> AppResult<Instance> {
    state.apply_patch(&instance_id, patch).await
}

/// Ask the router to forward this server's port. Opt-in, and never implied by
/// starting a server.
#[tauri::command]
pub async fn instance_forward_port(state: State<'_>, instance_id: String) -> AppResult<()> {
    let instance = state.store.get(&instance_id).await?;
    let manifest = state.registry.manifest(&instance.game)?;
    let port = instance.primary_port(&manifest);
    state.forwarder.forward(&instance_id, port, &format!("Nebula — {}", instance.name)).await
}

/// Whether Windows Firewall blocks this server's program.
#[tauri::command]
pub async fn instance_firewall(state: State<'_>, instance_id: String) -> AppResult<crate::net::firewall::FirewallStatus> {
    state.firewall_status(&instance_id).await
}

/// Let this server's program through Windows Firewall (UAC prompt).
#[tauri::command]
pub async fn instance_firewall_fix(state: State<'_>, instance_id: String) -> AppResult<crate::net::firewall::FirewallStatus> {
    state.firewall_fix(&instance_id).await
}

#[tauri::command]
pub async fn instance_release_port(state: State<'_>, instance_id: String) -> AppResult<()> {
    state.forwarder.release(&instance_id).await;
    Ok(())
}

/// Make any picture the server's icon (cropped or fitted to the game's
/// square). `data` is the image's bytes in base64, or a `data:` URL.
#[tauri::command]
pub async fn instance_icon_set(
    state: State<'_>,
    instance_id: String,
    data: String,
    fit: Option<IconFit>,
) -> AppResult<()> {
    state.set_icon(&instance_id, &data, fit.unwrap_or_default()).await
}

/// Remove the server's icon; the game and the app fall back to their defaults.
#[tauri::command]
pub async fn instance_icon_clear(state: State<'_>, instance_id: String) -> AppResult<()> {
    state.clear_icon(&instance_id).await
}

/// The game's own log files that can be shared (latest log, crash reports).
#[tauri::command]
pub async fn instance_log_files(state: State<'_>, instance_id: String) -> AppResult<LogFiles> {
    state.log_files(&instance_id).await
}

/// Upload a log to mclo.gs. The UI has asked the user first.
#[tauri::command]
pub async fn instance_log_share(
    state: State<'_>,
    instance_id: String,
    source: LogSource,
) -> AppResult<SharedLog> {
    state.share_log(&instance_id, source).await
}
