//! The node record. One file, because today there is one node — but it is a
//! *list* everywhere above this line, so adding a paired machine later is a
//! data change rather than a redesign.

use crate::error::AppResult;
use crate::model::Node;
use crate::store::paths;

const NODE_FILE: &str = "node.json";

/// Load this machine's node record, or `None` on a first run.
///
/// A record from before node ids were unique (every node was `"local"`) gets
/// a fresh id here, once, and is saved straight away; the instance records
/// that still point at `"local"` follow in `InstanceService::new`.
pub fn load() -> AppResult<Option<Node>> {
    let path = paths::config_dir()?.join(NODE_FILE);
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(None) };
    let Ok(mut node) = serde_json::from_str::<Node>(&text) else { return Ok(None) };
    if let Some(old) = migrate_id(&mut node) {
        tracing::info!(old, new = %node.id, "node id made unique");
        save(&node)?;
    }
    Ok(Some(node))
}

/// Give a node with the legacy shared id a unique one. Returns the old id
/// when it changed.
pub fn migrate_id(node: &mut Node) -> Option<String> {
    let legacy = node.id.trim().is_empty() || node.id == Node::LEGACY_LOCAL_ID;
    if !legacy {
        return None;
    }
    let old = std::mem::replace(&mut node.id, uuid::Uuid::new_v4().to_string());
    Some(old)
}

pub fn save(node: &Node) -> AppResult<()> {
    let path = paths::config_dir()?.join(NODE_FILE);
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(node)?)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

/// The machine's own name, as a starting suggestion the user can change.
pub fn suggested_name() -> String {
    // The Android app passes the phone's own name ("Pixel 8").
    if let Some(name) = std::env::var("NEBULA_DEVICE_NAME").ok().filter(|n| !n.trim().is_empty()) {
        return name.trim().to_string();
    }
    hostname().unwrap_or_else(|| "This PC".to_string())
}

fn hostname() -> Option<String> {
    let raw = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())?;
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_suggested_name_is_never_empty() {
        assert!(!suggested_name().trim().is_empty());
    }

    #[test]
    fn a_local_node_is_marked_local() {
        let node = Node::local("workshop-tower");
        assert!(node.is_local());
        assert_ne!(node.id, Node::LEGACY_LOCAL_ID, "new nodes get a unique id");
        assert!(uuid::Uuid::parse_str(&node.id).is_ok());
        assert_ne!(Node::local("a").id, Node::local("a").id);
        assert!(node.paired_devices.is_empty());
    }

    #[test]
    fn a_legacy_local_id_is_migrated_once() {
        let mut node = Node::local("pi");
        node.id = Node::LEGACY_LOCAL_ID.to_string();
        assert_eq!(migrate_id(&mut node).as_deref(), Some("local"));
        assert!(uuid::Uuid::parse_str(&node.id).is_ok());
        let migrated = node.id.clone();
        assert_eq!(migrate_id(&mut node), None, "a unique id is left alone");
        assert_eq!(node.id, migrated);
    }

    #[test]
    fn an_old_node_file_still_reads() {
        let text = r#"{"id":"local","name":"Pi","transport":{"kind":"local"},"created_at":"2025-01-01T00:00:00Z"}"#;
        let mut node: Node = serde_json::from_str(text).expect("old record");
        assert!(migrate_id(&mut node).is_some());
    }
}
