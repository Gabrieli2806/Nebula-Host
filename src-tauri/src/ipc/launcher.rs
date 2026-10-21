use crate::{
    error::AppResult,
    launcher::{
        self,
        auth::{Account, AuthState, DeviceLogin, LoginResult},
        Capabilities, ContentKind, CreateProfile, GameVersion, Profile, SearchResult,
    },
};

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
pub async fn launcher_login_cancel(state: tauri::State<'_, AuthState>, id: uuid::Uuid) -> AppResult<()> {
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
