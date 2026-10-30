//! Minecraft-only types. Nothing outside `game::minecraft` may import these —
//! the rest of the app talks in [`crate::model`] and [`crate::game::manifest`].

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};

/// The Minecraft server distributions we can install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerType {
    Vanilla,
    Paper,
    Spigot,
    Fabric,
    Forge,
    NeoForge,
    Purpur,
}

impl ServerType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Paper => "paper",
            Self::Spigot => "spigot",
            Self::Fabric => "fabric",
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
            Self::Purpur => "purpur",
        }
    }

    /// Maps a manifest distribution id onto the internal enum.
    pub fn parse(distribution: &str) -> AppResult<Self> {
        match distribution.trim().to_ascii_lowercase().as_str() {
            "vanilla" => Ok(Self::Vanilla),
            "paper" => Ok(Self::Paper),
            "spigot" => Ok(Self::Spigot),
            "fabric" => Ok(Self::Fabric),
            "forge" => Ok(Self::Forge),
            "neoforge" => Ok(Self::NeoForge),
            "purpur" => Ok(Self::Purpur),
            other => Err(AppError::new(ErrorCode::GameNotSupported)
                .with("game", "minecraft")
                .with("distribution", other)),
        }
    }

    /// Forge and NeoForge ship an installer jar that has to be run once.
    pub fn uses_installer(self) -> bool {
        matches!(self, Self::Forge | Self::NeoForge)
    }
}

// ---------------------------------------------------------------------------
// Modrinth
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModrinthProject {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    /// `mod` or `plugin`.
    pub project_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModrinthVersion {
    pub id: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<ModrinthFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModrinthFile {
    pub url: String,
    pub filename: String,
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_distributions_case_insensitively() {
        assert_eq!(ServerType::parse("NeoForge").expect("known"), ServerType::NeoForge);
        assert_eq!(ServerType::parse(" paper ").expect("known"), ServerType::Paper);
    }

    #[test]
    fn unknown_distribution_is_typed() {
        let err = ServerType::parse("bedrock").expect_err("unknown");
        assert_eq!(err.code, ErrorCode::GameNotSupported);
    }

    #[test]
    fn only_forge_family_uses_an_installer() {
        assert!(ServerType::Forge.uses_installer());
        assert!(ServerType::NeoForge.uses_installer());
        assert!(!ServerType::Paper.uses_installer());
    }
}
