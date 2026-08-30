//! Server lifecycle commands.

use serde::Deserialize;

use crate::error::AppResult;
use crate::ipc::State;
use crate::model::{ConsoleLine, Instance, InstanceView, RuntimeKind};
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

#[tauri::command]
pub async fn instance_console(
    state: State<'_>,
    instance_id: String,
    limit: Option<usize>,
) -> AppResult<Vec<ConsoleLine>> {
    Ok(state.console_tail(&instance_id, limit.unwrap_or(500)).await)
}

/// Everything on the instance a user can change from the UI. Absent fields are
/// left alone, so the UI can send one edit at a time.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancePatch {
    pub name: Option<String>,
    pub memory_mb: Option<u32>,
    pub port: Option<u16>,
    pub runtime_kind: Option<RuntimeKind>,
    pub runtime_path: Option<String>,
    pub extra_args: Option<Vec<String>>,
    pub sleep_when_empty: Option<bool>,
    pub sleep_after_minutes: Option<u32>,
    pub wake_on_join: Option<bool>,
    pub restart_after_crash: Option<bool>,
    pub track_latest_version: Option<bool>,
    pub schedules: Option<Vec<crate::model::ScheduleRule>>,
}

#[tauri::command]
pub async fn instance_update(
    state: State<'_>,
    instance_id: String,
    patch: InstancePatch,
) -> AppResult<Instance> {
    // Validate before touching the stored record: a rejected port must not
    // leave a half-applied edit behind.
    if let Some(port) = patch.port {
        crate::net::ports::validate(port)?;
    }

    let manifest = {
        let instance = state.store.get(&instance_id).await?;
        state.registry.manifest(&instance.game)?
    };

    state
        .store
        .update(&instance_id, move |instance| {
            if let Some(name) = patch.name {
                instance.name = name;
            }
            if let Some(memory_mb) = patch.memory_mb {
                instance.resources.memory_mb = memory_mb;
            }
            if let Some(port) = patch.port {
                if let Some(spec) = manifest.primary_port() {
                    instance.ports.insert(spec.key.clone(), port);
                }
            }
            if let Some(runtime_kind) = patch.runtime_kind {
                instance.runtime_kind = runtime_kind;
            }
            if let Some(runtime_path) = patch.runtime_path {
                instance.resources.runtime_path =
                    Some(runtime_path).filter(|path| !path.trim().is_empty());
            }
            if let Some(extra_args) = patch.extra_args {
                instance.resources.extra_args = extra_args;
            }
            if let Some(value) = patch.sleep_when_empty {
                instance.automation.sleep_when_empty = value;
            }
            if let Some(value) = patch.sleep_after_minutes {
                instance.automation.sleep_after_minutes = value.max(1);
            }
            if let Some(value) = patch.wake_on_join {
                instance.automation.wake_on_join = value;
            }
            if let Some(value) = patch.restart_after_crash {
                instance.automation.restart_after_crash = value;
            }
            if let Some(value) = patch.track_latest_version {
                instance.track_latest_version = value;
            }
            if let Some(schedules) = patch.schedules {
                instance.automation.schedules = schedules;
            }
        })
        .await
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

#[tauri::command]
pub async fn instance_release_port(state: State<'_>, instance_id: String) -> AppResult<()> {
    state.forwarder.release(&instance_id).await;
    Ok(())
}
