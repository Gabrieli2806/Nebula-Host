//! Local client launcher workspace. Deliberately separate from server instances
//! and never exposed through the remote-node dispatch surface.
pub mod auth;
pub mod client;
pub mod content;
pub mod loaders;
#[cfg(all(target_os = "android", feature = "desktop"))]
pub mod android;

use crate::{
    error::{AppError, AppResult, ErrorCode},
    store::paths,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::OnceLock};

static STORE_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub id: uuid::Uuid,
    pub name: String,
    pub game_version: String,
    pub loader: Loader,
    pub memory_mb: u32,
    #[serde(default)]
    pub loader_version: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Loader {
    Vanilla,
    Fabric,
    Forge,
    Neoforge,
    Quilt,
}

#[derive(Deserialize)]
pub struct CreateProfile {
    pub name: String,
    pub game_version: String,
    pub loader: Loader,
    pub memory_mb: u32,
}

#[derive(Serialize)]
pub struct Capabilities {
    pub platform: &'static str,
    pub can_launch: bool,
    pub microsoft_configured: bool,
}

pub fn capabilities() -> Capabilities {
    Capabilities {
        platform: std::env::consts::OS,
        // The server JVM cannot run a graphical Minecraft client on Android.
        can_launch: client::supported(),
        microsoft_configured: auth::configured_client_id().is_some(),
    }
}

fn index_path() -> AppResult<PathBuf> {
    Ok(paths::config_dir()?.join("launcher-profiles.json"))
}

fn read_profiles() -> AppResult<Vec<Profile>> {
    match std::fs::read(index_path()?) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

fn save_profiles(profiles: &[Profile]) -> AppResult<()> {
    let path = index_path()?;
    let temporary = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&temporary)?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(profiles)?)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(temporary, path)?;
    Ok(())
}

pub async fn profiles() -> AppResult<Vec<Profile>> {
    let _guard = STORE_LOCK.get_or_init(Default::default).lock().await;
    read_profiles()
}

pub fn validate_profile(input: &CreateProfile) -> AppResult<()> {
    if input.name.trim().is_empty()
        || input.name.chars().count() > 64
        || input.name.chars().any(char::is_control)
        || !(512..=32768).contains(&input.memory_mb)
        || input.game_version.is_empty()
        || input.game_version.len() > 64
        || !input
            .game_version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c))
    {
        return Err(AppError::new(ErrorCode::InvalidInput));
    }
    Ok(())
}

pub async fn create(input: CreateProfile) -> AppResult<Profile> {
    validate_profile(&input)?;
    // Accept only IDs from Mojang's manifest, never user-supplied paths or URLs.
    let manifest = versions().await?;
    if !manifest.iter().any(|v| v.id == input.game_version) {
        return Err(AppError::new(ErrorCode::VersionNotFound));
    }
    let loader_version = if matches!(input.loader, Loader::Vanilla | Loader::Quilt) { None } else {
        Some(loaders::versions(&input.loader, &input.game_version).await?.into_iter().next()
            .ok_or_else(|| AppError::new(ErrorCode::VersionNotFound).with("version", &input.game_version))?)
    };
    let _guard = STORE_LOCK.get_or_init(Default::default).lock().await;
    let mut profiles = read_profiles()?;
    let profile = Profile {
        id: uuid::Uuid::new_v4(),
        name: input.name.trim().into(),
        game_version: input.game_version,
        loader: input.loader,
        memory_mb: input.memory_mb,
        loader_version,
    };
    profiles.push(profile.clone());
    save_profiles(&profiles)?;
    Ok(profile)
}

/// Pin the loader build an instance uses (a modpack names its own).
pub async fn set_loader_version(id: uuid::Uuid, version: String) -> AppResult<Profile> {
    let _guard = STORE_LOCK.get_or_init(Default::default).lock().await;
    let mut profiles = read_profiles()?;
    let profile = profiles.iter_mut().find(|p| p.id == id).ok_or_else(|| AppError::new(ErrorCode::NotFound))?;
    profile.loader_version = Some(version);
    let updated = profile.clone();
    save_profiles(&profiles)?;
    Ok(updated)
}

pub async fn remove(id: uuid::Uuid) -> AppResult<()> {
    let _guard = STORE_LOCK.get_or_init(Default::default).lock().await;
    let mut profiles = read_profiles()?;
    let old_len = profiles.len();
    profiles.retain(|p| p.id != id);
    if old_len == profiles.len() {
        return Err(AppError::new(ErrorCode::NotFound));
    }
    save_profiles(&profiles)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GameVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub client_supported: bool,
}

pub async fn versions() -> AppResult<Vec<GameVersion>> {
    #[derive(Deserialize)]
    struct Manifest {
        versions: Vec<GameVersion>,
    }
    let result: Manifest = crate::game::http_client()
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(result.versions.into_iter().map(|mut version| {
        version.client_supported = version.kind == "release" && client::modern_version(&version.id);
        version
    }).collect())
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Mod,
    Modpack,
    Resourcepack,
    Shader,
}

impl ContentKind {
    fn tag(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::Modpack => "modpack",
            Self::Resourcepack => "resourcepack",
            Self::Shader => "shader",
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Project {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    pub project_type: String,
    pub icon_url: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SearchResult {
    pub hits: Vec<Project>,
    pub total_hits: u64,
}

pub async fn search(query: String, kind: ContentKind, offset: u32) -> AppResult<SearchResult> {
    if query.len() > 200 || offset > 10000 {
        return Err(AppError::new(ErrorCode::InvalidInput));
    }
    let facets = serde_json::json!([
        [format!("project_type:{}", kind.tag())],
        ["client_side:required", "client_side:optional"]
    ]);
    Ok(crate::game::http_client()
        .get("https://api.modrinth.com/v2/search")
        .query(&[
            ("query", query.trim().to_string()),
            ("facets", facets.to_string()),
            ("limit", "20".into()),
            ("offset", offset.to_string()),
            (
                "index",
                if query.trim().is_empty() {
                    "downloads"
                } else {
                    "relevance"
                }
                .into(),
            ),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_paths_and_unreasonable_memory() {
        let mut input = CreateProfile {
            name: "Survival".into(),
            game_version: "1.21.4".into(),
            loader: Loader::Vanilla,
            memory_mb: 4096,
        };
        assert!(validate_profile(&input).is_ok());
        input.game_version = "../escape".into();
        assert!(validate_profile(&input).is_err());
        input.game_version = "1.21.4".into();
        input.memory_mb = 0;
        assert!(validate_profile(&input).is_err());
        input.memory_mb = 4096;
        input.name = "\n".into();
        assert!(validate_profile(&input).is_err());
    }
}
