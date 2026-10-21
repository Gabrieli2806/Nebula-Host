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
    client_id: Option<String>,
) -> AppResult<DeviceLogin> {
    state.begin(client_id).await
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
