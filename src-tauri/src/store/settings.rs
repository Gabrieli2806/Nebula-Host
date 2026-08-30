//! App-wide settings. Small, boring, and written atomically — a half-written
//! settings.json would lose the user's node name and Java paths.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::store::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// UI language: `en`, `es`, or `system`.
    pub language: String,
    /// Accent colour id the UI themes itself with (`cyan`, `amber`, …).
    /// Stored as a name, not a hex value: the palette that name resolves to is
    /// the UI's business, and a stored `#22d3ee` would outlive the design.
    pub accent: String,
    /// `cards` or `list`. Ten to twenty servers on an old PC is a list.
    pub density: String,
    /// Where new instances are created.
    pub instances_dir: Option<String>,
    /// JVM path per Java major version, e.g. `{"21": "C:/jdk21/bin/java.exe"}`.
    pub java_paths: BTreeMap<String, String>,
    /// Single JVM path from before per-version support existed.
    pub java_path: Option<String>,
    /// Start hosting again on login.
    pub launch_at_startup: bool,
    /// Keep running in the tray when the window is closed.
    pub run_in_tray: bool,
    /// Ask the router to forward ports over UPnP.
    pub allow_upnp: bool,
    /// Answer requests from other machines on the LAN. Loopback-only until the
    /// user turns this on knowingly; see the security notes in the README.
    pub listen_on_lan: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: "system".to_string(),
            accent: "cyan".to_string(),
            density: "cards".to_string(),
            instances_dir: None,
            java_paths: BTreeMap::new(),
            java_path: None,
            launch_at_startup: false,
            run_in_tray: true,
            allow_upnp: false,
            listen_on_lan: false,
        }
    }
}

impl AppSettings {
    pub fn instances_dir(&self) -> AppResult<PathBuf> {
        match self.instances_dir.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            Some(dir) => Ok(PathBuf::from(dir)),
            None => paths::default_instances_dir(),
        }
    }
}

static CACHE: RwLock<Option<AppSettings>> = RwLock::new(None);

fn settings_file() -> AppResult<PathBuf> {
    Ok(paths::config_dir()?.join("settings.json"))
}

/// Read settings, falling back to defaults for a first run or a corrupt file.
/// Losing settings is annoying; refusing to start is worse.
pub fn load() -> AppResult<AppSettings> {
    if let Some(cached) = CACHE.read().ok().and_then(|guard| guard.clone()) {
        return Ok(cached);
    }

    let path = settings_file()?;
    let settings = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
            tracing::warn!(?path, %error, "settings.json is unreadable; using defaults");
            AppSettings::default()
        }),
        Err(_) => AppSettings::default(),
    };

    if let Ok(mut guard) = CACHE.write() {
        *guard = Some(settings.clone());
    }
    Ok(settings)
}

pub fn save(settings: &AppSettings) -> AppResult<()> {
    let path = settings_file()?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(settings)?)?;
    std::fs::rename(&temp, &path)?;

    if let Ok(mut guard) = CACHE.write() {
        *guard = Some(settings.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_lan_closed_and_upnp_off() {
        let settings = AppSettings::default();
        assert!(!settings.listen_on_lan, "the LAN must be opt-in, never a default");
        assert!(!settings.allow_upnp, "port forwarding must be opt-in");
    }

    #[test]
    fn unknown_fields_do_not_break_older_or_newer_files() {
        let json = r#"{"language":"es","somethingFromTheFuture":true}"#;
        let settings: AppSettings = serde_json::from_str(json).expect("tolerant parse");
        assert_eq!(settings.language, "es");
        assert_eq!(settings.density, "cards");
    }

    #[test]
    fn a_settings_file_written_before_accents_existed_still_loads() {
        let json = r#"{"language":"en","density":"list"}"#;
        let settings: AppSettings = serde_json::from_str(json).expect("tolerant parse");
        assert_eq!(settings.accent, "cyan", "the default accent fills itself in");
    }

    #[test]
    fn java_paths_round_trip() {
        let mut settings = AppSettings::default();
        settings.java_paths.insert("21".into(), "C:/jdk21/bin/java.exe".into());
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: AppSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.java_paths.get("21").map(String::as_str), Some("C:/jdk21/bin/java.exe"));
    }
}
