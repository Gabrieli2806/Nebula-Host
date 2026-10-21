use crate::launcher::client::{ClientEngine, ClientStatus, Credentials};
use crate::{
    error::AppResult,
    launcher::{
        self,
        auth::{Account, AuthState, DeviceLogin, LoginResult},
        Capabilities, ContentKind, CreateProfile, GameVersion, Profile, SearchResult,
    },
};
use std::sync::Arc;

#[tauri::command]
pub async fn launcher_pin_shortcut(id: uuid::Uuid, mode: String, account_id: Option<String>) -> AppResult<()> {
    let profile = profile_by_id(id).await?;
    if !matches!(mode.as_str(), "demo" | "account") || (mode == "account" && account_id.as_deref().and_then(|value| uuid::Uuid::parse_str(value).ok()).is_none()) {
        return Err(crate::error::AppError::new(crate::error::ErrorCode::InvalidInput));
    }
    #[cfg(target_os = "android")]
    return launcher::android::pin_shortcut(serde_json::json!({"id": profile.id, "name": profile.name, "mode": mode, "accountId": account_id}));
    #[cfg(not(target_os = "android"))]
    { let _ = profile; Err(crate::error::AppError::new(crate::error::ErrorCode::GameNotSupported)) }
}

#[tauri::command]
pub async fn launcher_take_shortcut() -> AppResult<serde_json::Value> {
    #[cfg(target_os = "android")]
    return launcher::android::take_shortcut();
    #[cfg(not(target_os = "android"))]
    Ok(serde_json::Value::Null)
}

#[tauri::command]
pub async fn launcher_status(
    state: tauri::State<'_, Arc<ClientEngine>>,
) -> AppResult<std::collections::HashMap<uuid::Uuid, ClientStatus>> {
    Ok(state.statuses().await)
}
#[tauri::command]
pub async fn launcher_client_logs(
    state: tauri::State<'_, Arc<ClientEngine>>,
    id: uuid::Uuid,
) -> AppResult<Vec<String>> {
    Ok(state.logs(id))
}
#[tauri::command]
pub async fn launcher_client_stop(
    state: tauri::State<'_, Arc<ClientEngine>>,
    id: uuid::Uuid,
) -> AppResult<()> {
    state.stop(id).await
}
#[tauri::command]
pub async fn launcher_client_resume(
    state: tauri::State<'_, Arc<ClientEngine>>,
    id: uuid::Uuid,
) -> AppResult<()> {
    state.resume(id).await
}
#[tauri::command]
pub async fn launcher_client_start(
    state: tauri::State<'_, Arc<ClientEngine>>,
    auth: tauri::State<'_, AuthState>,
    id: uuid::Uuid,
    mode: String,
    account_id: Option<String>,
) -> AppResult<()> {
    use crate::error::{AppError, ErrorCode};
    let profile = launcher::profiles()
        .await?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound))?;
    #[cfg(target_os = "android")]
    if state.statuses().await.get(&id).is_some_and(|s| s.phase == "running") {
        return state.resume(id).await;
    }
    let credentials = match mode.as_str() {
        "install" => None,
        "demo" => Some(Credentials {
            name: "DemoPlayer".into(),
            uuid: "00000000000000000000000000000000".into(),
            token: "0".into(),
            demo: true,
            expires: None,
        }),
        "account" => Some(
            auth.credentials(
                account_id
                    .as_deref()
                    .ok_or_else(|| AppError::new(ErrorCode::InvalidInput))?,
            )
            .await?,
        ),
        _ => return Err(AppError::new(ErrorCode::InvalidInput)),
    };
    state.inner().start(profile, credentials).await
}

#[tauri::command]
pub fn launcher_capabilities() -> Capabilities {
    launcher::capabilities()
}
#[tauri::command]
pub async fn launcher_profiles() -> AppResult<Vec<Profile>> {
    launcher::profiles().await
}
#[tauri::command]
pub async fn launcher_create(input: CreateProfile) -> AppResult<Profile> {
    launcher::create(input).await
}
#[tauri::command]
pub async fn launcher_remove(id: uuid::Uuid) -> AppResult<()> {
    launcher::remove(id).await
}
#[tauri::command]
pub async fn launcher_versions() -> AppResult<Vec<GameVersion>> {
    launcher::versions().await
}

#[tauri::command]
pub async fn launcher_loader_versions(loader: launcher::Loader, game_version: String) -> AppResult<Vec<String>> {
    launcher::loaders::versions(&loader, &game_version).await
}
#[tauri::command]
pub async fn launcher_search(
    query: String,
    kind: ContentKind,
    offset: u32,
) -> AppResult<SearchResult> {
    launcher::search(query, kind, offset).await
}
#[tauri::command]
pub async fn launcher_login_begin(
    state: tauri::State<'_, AuthState>,
) -> AppResult<DeviceLogin> {
    state.begin().await
}
#[tauri::command]
pub async fn launcher_login_poll(
    state: tauri::State<'_, AuthState>,
    id: uuid::Uuid,
) -> AppResult<LoginResult> {
    state.poll(id).await
}
#[tauri::command]
pub async fn launcher_login_cancel(
    state: tauri::State<'_, AuthState>,
    id: uuid::Uuid,
) -> AppResult<()> {
    state.cancel(id).await;
    Ok(())
}
#[tauri::command]
pub async fn launcher_accounts(state: tauri::State<'_, AuthState>) -> AppResult<Vec<Account>> {
    Ok(state.accounts().await)
}
#[tauri::command]
pub async fn launcher_logout(state: tauri::State<'_, AuthState>, id: String) -> AppResult<()> {
    state.logout(id).await;
    Ok(())
}

async fn profile_by_id(id: uuid::Uuid) -> AppResult<Profile> {
    launcher::profiles()
        .await?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| crate::error::AppError::new(crate::error::ErrorCode::NotFound))
}

/// What an instance has installed: mods, resource packs, shaders.
#[tauri::command]
pub async fn launcher_content_list(id: uuid::Uuid) -> AppResult<Vec<launcher::content::Installed>> {
    launcher::content::list(&profile_by_id(id).await?)
}

/// Install a Modrinth mod, resource pack or shader into an instance.
#[tauri::command]
pub async fn launcher_content_install(id: uuid::Uuid, project_id: String, kind: ContentKind) -> AppResult<Vec<String>> {
    launcher::content::install(&profile_by_id(id).await?, &project_id, kind).await
}

#[tauri::command]
pub async fn launcher_content_remove(id: uuid::Uuid, kind: ContentKind, file: String) -> AppResult<()> {
    launcher::content::remove(&profile_by_id(id).await?, kind, &file)
}

#[tauri::command]
pub async fn launcher_content_set_enabled(id: uuid::Uuid, kind: ContentKind, file: String, enabled: bool) -> AppResult<()> {
    launcher::content::set_enabled(&profile_by_id(id).await?, kind, &file, enabled)
}

/// Create a new instance from a Modrinth modpack.
#[tauri::command]
pub async fn launcher_modpack_install(project_id: String) -> AppResult<Profile> {
    launcher::content::install_modpack(&project_id).await
}

/// The picture an installed mod, resource pack or shader carries (a data URL).
#[tauri::command]
pub async fn launcher_content_icon(id: uuid::Uuid, kind: ContentKind, file: String) -> AppResult<Option<String>> {
    let profile = profile_by_id(id).await?;
    tokio::task::spawn_blocking(move || launcher::content::icon(&profile, kind, &file))
        .await
        .map_err(crate::error::AppError::unexpected)?
}

/// Add a file from outside (a `.jar` mod, a `.zip` pack) to an instance. The
/// bytes arrive base64-encoded from the file the person picked.
#[tauri::command]
pub async fn launcher_content_import(id: uuid::Uuid, kind: ContentKind, name: String, data: String) -> AppResult<String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|_| crate::error::AppError::new(crate::error::ErrorCode::InvalidInput).with("reason", "file data"))?;
    let profile = profile_by_id(id).await?;
    tokio::task::spawn_blocking(move || launcher::content::import_file(&profile, kind, &name, &bytes))
        .await
        .map_err(crate::error::AppError::unexpected)?
}
