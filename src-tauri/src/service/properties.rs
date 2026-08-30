//! Reading and writing a game's own settings file, generically.
//!
//! The manifest lists the fields and their types; this reads the values behind
//! them and writes edits back without disturbing anything it does not know
//! about — a hand-edited comment or a setting we have never heard of survives
//! a save.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::game::manifest::{GameManifest, SettingField, SettingKind};
use crate::store::paths::resolve_inside;

/// Minecraft's `server.properties` shape: `key=value`, `#` comments.
/// Other games get their own reader when one is added; the manifest already
/// carries the field list either way.
const PROPERTIES_FILE: &str = "server.properties";

/// One setting, as the UI renders it: its declaration plus its current value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingValue {
    #[serde(flatten)]
    pub field: SettingField,
    /// Present value as text; the UI parses per `field.kind`.
    pub value: String,
}

pub fn read(root: &Path, manifest: &GameManifest) -> AppResult<Vec<SettingValue>> {
    let stored = read_properties(root)?;

    Ok(manifest
        .settings
        .iter()
        .map(|field| SettingValue {
            value: stored.get(&field.key).cloned().unwrap_or_else(|| default_of(&field.kind)),
            field: field.clone(),
        })
        .collect())
}

/// Apply edits. Only keys the manifest declares can be written — the file
/// editor is where arbitrary changes belong, with the warnings that come with
/// it.
pub fn write(root: &Path, manifest: &GameManifest, edits: &BTreeMap<String, String>) -> AppResult<()> {
    let path = resolve_inside(root, PROPERTIES_FILE)?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let declared: BTreeMap<&str, &SettingField> =
        manifest.settings.iter().map(|field| (field.key.as_str(), field)).collect();

    let mut written: Vec<String> = Vec::new();
    let mut out = String::with_capacity(existing.len() + 64);

    for line in existing.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || !trimmed.contains('=') {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        let key = trimmed.split('=').next().unwrap_or_default().trim();
        match edits.get(key).filter(|_| declared.contains_key(key)) {
            Some(value) => {
                out.push_str(&format!("{key}={value}\n"));
                written.push(key.to_string());
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    // Settings the file does not mention yet get appended.
    for (key, value) in edits {
        if !written.contains(key) && declared.contains_key(key.as_str()) {
            out.push_str(&format!("{key}={value}\n"));
        }
    }

    let temp = path.with_extension("nebula-tmp");
    std::fs::write(&temp, out)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

/// The value of one key, for callers that need a single setting.
pub fn get(root: &Path, key: &str) -> AppResult<Option<String>> {
    Ok(read_properties(root)?.get(key).cloned())
}

fn read_properties(root: &Path) -> AppResult<BTreeMap<String, String>> {
    let path = resolve_inside(root, PROPERTIES_FILE)?;
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(BTreeMap::new());
    };

    Ok(text
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, value) = trimmed.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect())
}

fn default_of(kind: &SettingKind) -> String {
    match kind {
        SettingKind::Bool { default } => default.to_string(),
        SettingKind::Int { default, .. } => default.to_string(),
        SettingKind::Text { default, .. } => default.clone(),
        SettingKind::Choice { default, .. } => default.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("nebula-props-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[test]
    fn missing_settings_fall_back_to_the_manifest_defaults() {
        let root = temp_root("defaults");
        let manifest = crate::game::minecraft::manifest();
        let values = read(&root, &manifest).expect("read");
        let motd = values.iter().find(|v| v.field.key == "motd").expect("motd is declared");
        assert_eq!(motd.value, "A Minecraft Server");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn editing_preserves_comments_and_unknown_keys() {
        let root = temp_root("preserve");
        std::fs::write(
            root.join(PROPERTIES_FILE),
            "#Minecraft server properties\nmotd=old\nsome-mod-setting=keepme\n",
        )
        .expect("seed");

        let manifest = crate::game::minecraft::manifest();
        write(&root, &manifest, &BTreeMap::from([("motd".to_string(), "new".to_string())]))
            .expect("write");

        let text = std::fs::read_to_string(root.join(PROPERTIES_FILE)).expect("read back");
        assert!(text.contains("#Minecraft server properties"), "comment lost:\n{text}");
        assert!(text.contains("some-mod-setting=keepme"), "unknown key lost:\n{text}");
        assert!(text.contains("motd=new"));
        assert!(!text.contains("motd=old"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn undeclared_keys_cannot_be_written_through_the_settings_panel() {
        let root = temp_root("undeclared");
        std::fs::write(root.join(PROPERTIES_FILE), "motd=hi\n").expect("seed");
        let manifest = crate::game::minecraft::manifest();

        write(&root, &manifest, &BTreeMap::from([("rcon.password".to_string(), "hunter2".into())]))
            .expect("write");

        let text = std::fs::read_to_string(root.join(PROPERTIES_FILE)).expect("read back");
        assert!(!text.contains("rcon.password"), "only declared settings may be written");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_declared_key_is_appended() {
        let root = temp_root("append");
        std::fs::write(root.join(PROPERTIES_FILE), "motd=hi\n").expect("seed");
        let manifest = crate::game::minecraft::manifest();

        write(&root, &manifest, &BTreeMap::from([("pvp".to_string(), "false".to_string())]))
            .expect("write");

        assert_eq!(get(&root, "pvp").expect("read").as_deref(), Some("false"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
