//! The node record. One file, because today there is one node — but it is a
//! *list* everywhere above this line, so adding a paired machine later is a
//! data change rather than a redesign.

use crate::error::AppResult;
use crate::model::Node;
use crate::store::paths;

const NODE_FILE: &str = "node.json";

/// Load this machine's node record, or `None` on a first run.
pub fn load() -> AppResult<Option<Node>> {
    let path = paths::config_dir()?.join(NODE_FILE);
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text).ok()),
        Err(_) => Ok(None),
    }
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
    hostname().unwrap_or_else(|| "This PC".to_string())
}

fn hostname() -> Option<String> {
    let raw = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok()?;
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
        assert_eq!(node.id, "local");
        assert!(node.paired_devices.is_empty());
    }
}
