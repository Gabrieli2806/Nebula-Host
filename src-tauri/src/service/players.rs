//! Player lists — who is allowed on, who runs the place, who is not welcome.
//!
//! Which files those live in comes from the manifest's [`PlayerSource`], so a
//! game that keeps its allow-list somewhere else needs no code here.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::manifest::PlayerSource;
use crate::store::paths::resolve_inside;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    /// Operator level, where the game has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
}

/// Every list the game declares, keyed by the manifest's own list names
/// (`allowed`, `operators`, `banned`).
pub type PlayerLists = BTreeMap<String, Vec<PlayerEntry>>;

pub fn read_lists(root: &Path, source: &PlayerSource) -> AppResult<PlayerLists> {
    let PlayerSource::Files { lists, .. } = source else {
        return Ok(PlayerLists::new());
    };

    let mut out = PlayerLists::new();
    for (name, file) in lists {
        out.insert(name.clone(), read_list(root, file).unwrap_or_default());
    }
    Ok(out)
}

fn read_list(root: &Path, file: &str) -> AppResult<Vec<PlayerEntry>> {
    let path = resolve_inside(root, file)?;
    let Ok(text) = std::fs::read_to_string(path) else {
        // A list the server has never written is an empty list, not an error.
        return Ok(Vec::new());
    };
    let parsed: Vec<Value> = serde_json::from_str(&text).unwrap_or_default();

    Ok(parsed
        .into_iter()
        .filter_map(|entry| {
            Some(PlayerEntry {
                name: entry["name"].as_str()?.to_string(),
                uuid: entry["uuid"].as_str().map(String::from),
                level: entry["level"].as_u64().map(|level| level as u8),
                reason: entry["reason"].as_str().map(String::from),
                expires: entry["expires"].as_str().map(String::from),
            })
        })
        .collect())
}

pub fn write_list(root: &Path, source: &PlayerSource, list: &str, entries: &[PlayerEntry]) -> AppResult<()> {
    let PlayerSource::Files { lists, .. } = source else {
        return Err(AppError::new(ErrorCode::GameNotSupported).with("list", list));
    };
    let file = lists
        .get(list)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound).with("list", list))?;

    let path = resolve_inside(root, file)?;
    let json: Vec<Value> = entries
        .iter()
        .map(|entry| {
            let mut object = serde_json::Map::new();
            object.insert("name".into(), Value::String(entry.name.clone()));
            if let Some(uuid) = &entry.uuid {
                object.insert("uuid".into(), Value::String(uuid.clone()));
            }
            if let Some(level) = entry.level {
                object.insert("level".into(), Value::Number(level.into()));
            }
            if let Some(reason) = &entry.reason {
                object.insert("reason".into(), Value::String(reason.clone()));
            }
            Value::Object(object)
        })
        .collect();

    let temp = path.with_extension("nebula-tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(&json)?)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

/// Name → UUID from everything the game keeps on disk: its lists and, when
/// the manifest names one, its name cache (`usercache.json`). Unreadable files
/// contribute nothing; this is for faces, not for anything that must be right.
pub fn read_identities(root: &Path, source: &PlayerSource) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let PlayerSource::Files { lists, identity_file } = source else { return out };

    if let Some(file) = identity_file {
        if let Ok(path) = resolve_inside(root, file) {
            let parsed: Vec<Value> = std::fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            for entry in parsed {
                if let (Some(name), Some(uuid)) = (entry["name"].as_str(), entry["uuid"].as_str()) {
                    out.insert(name.to_string(), uuid.to_string());
                }
            }
        }
    }
    // Lists are curated by the admin; let them win over the cache.
    for file in lists.values() {
        for entry in read_list(root, file).unwrap_or_default() {
            if let Some(uuid) = entry.uuid {
                out.insert(entry.name, uuid);
            }
        }
    }
    out
}

/// Where to fetch a face for a player.
///
/// Servers in offline mode hand out **version-3 UUIDs derived from the name**,
/// not real accounts. Looking a skin up by that UUID returns a placeholder, so
/// offline players must be looked up by name instead.
pub fn avatar_url(name: &str, uuid: Option<&str>) -> String {
    match uuid.filter(|uuid| !is_offline_uuid(uuid)) {
        Some(uuid) => format!("https://crafatar.com/avatars/{uuid}?size=64&overlay"),
        None => format!("https://mc-heads.net/avatar/{name}/64"),
    }
}

/// Version 3 UUIDs are name-hashed: the version nibble is `3`.
pub fn is_offline_uuid(uuid: &str) -> bool {
    let normalised: String = uuid.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    normalised.len() == 32 && normalised.as_bytes().get(12) == Some(&b'3')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuids_are_recognised_by_their_version_nibble() {
        // v3, name-hashed: an offline-mode player.
        assert!(is_offline_uuid("069a79f4-44e9-3726-a5b8-12a8b1e0e0c1"));
        // v4, a real Mojang account.
        assert!(!is_offline_uuid("f84c6a79-0a4e-45e0-879b-cd49ebd4c4e2"));
        assert!(!is_offline_uuid("not-a-uuid"));
    }

    #[test]
    fn offline_players_are_looked_up_by_name() {
        let url = avatar_url("Steve", Some("069a79f4-44e9-3726-a5b8-12a8b1e0e0c1"));
        assert!(url.contains("Steve"), "offline UUIDs give placeholder skins: {url}");
    }

    #[test]
    fn online_players_are_looked_up_by_uuid() {
        let uuid = "f84c6a79-0a4e-45e0-879b-cd49ebd4c4e2";
        assert!(avatar_url("Steve", Some(uuid)).contains(uuid));
    }

    #[test]
    fn identities_come_from_the_name_cache_and_the_lists() {
        let root = std::env::temp_dir().join(format!("nebula-identities-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        std::fs::write(
            root.join("usercache.json"),
            r#"[{"name":"Steve","uuid":"8667ba71-b85a-4004-af54-457a9734eed7","expiresOn":"2026-01-01"}]"#,
        )
        .expect("cache");
        std::fs::write(root.join("ops.json"), r#"[{"name":"Alex","uuid":"ec561538-f3fd-461d-aff5-086b22154bce","level":4}]"#)
            .expect("ops");
        let source = PlayerSource::Files {
            lists: BTreeMap::from([("operators".to_string(), "ops.json".to_string())]),
            identity_file: Some("usercache.json".into()),
        };
        let ids = read_identities(&root, &source);
        assert_eq!(ids.get("Steve").map(String::as_str), Some("8667ba71-b85a-4004-af54-457a9734eed7"));
        assert_eq!(ids.get("Alex").map(String::as_str), Some("ec561538-f3fd-461d-aff5-086b22154bce"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_game_without_player_files_reports_no_lists() {
        let lists = read_lists(Path::new("/tmp"), &PlayerSource::Console).expect("no lists");
        assert!(lists.is_empty());
    }

    #[test]
    fn lists_round_trip_through_the_game_files() {
        let root = std::env::temp_dir().join(format!("nebula-players-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");

        let source = PlayerSource::Files {
            lists: BTreeMap::from([("allowed".to_string(), "whitelist.json".to_string())]),
            identity_file: None,
        };
        let entries = vec![PlayerEntry {
            name: "Gabrieli2806".into(),
            uuid: Some("f84c6a79-0a4e-45e0-879b-cd49ebd4c4e2".into()),
            level: None,
            reason: None,
            expires: None,
        }];

        write_list(&root, &source, "allowed", &entries).expect("write");
        let read = read_lists(&root, &source).expect("read");
        assert_eq!(read["allowed"][0].name, "Gabrieli2806");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_list_the_server_has_not_written_yet_is_empty_not_an_error() {
        let root = std::env::temp_dir().join(format!("nebula-players-empty-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temp root");
        let source = PlayerSource::Files {
            lists: BTreeMap::from([("banned".to_string(), "banned-players.json".to_string())]),
            identity_file: None,
        };
        assert!(read_lists(&root, &source).expect("read")["banned"].is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
