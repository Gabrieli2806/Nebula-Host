//! The other machines this one is paired with, as the backend knows them.
//!
//! Pairing happens in the window, which keeps the list for itself; it hands a
//! copy here (`mcp_sync_machines`) so an agent connected to this machine can
//! reach the others. The file holds each machine's access token, exactly as
//! the window's own storage does: it is this user's key to their own machines.

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::store::paths;

const PEERS_FILE: &str = "machines.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    /// The machine's own id, when the window knows it.
    #[serde(default)]
    pub node_id: Option<String>,
    pub name: String,
    /// `host:port` of its remote API.
    pub address: String,
    pub token: String,
}

pub fn load() -> AppResult<Vec<Peer>> {
    let path = paths::config_dir()?.join(PEERS_FILE);
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
        Err(_) => Ok(Vec::new()),
    }
}

pub fn save(peers: &[Peer]) -> AppResult<()> {
    let path = paths::config_dir()?.join(PEERS_FILE);
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(peers)?)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

/// The machine a name, id or address refers to. Names match without regard
/// to case; a unique beginning of a name is enough.
pub fn find<'a>(peers: &'a [Peer], wanted: &str) -> Result<&'a Peer, Vec<&'a Peer>> {
    let lower = wanted.trim().to_lowercase();
    if let Some(found) = peers.iter().find(|p| {
        p.name.to_lowercase() == lower || p.address.to_lowercase() == lower || p.node_id.as_deref() == Some(wanted.trim())
    }) {
        return Ok(found);
    }
    let matches: Vec<&Peer> = peers.iter().filter(|p| p.name.to_lowercase().starts_with(&lower)).collect();
    match matches.as_slice() {
        [one] => Ok(one),
        _ => Err(matches),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(name: &str, address: &str) -> Peer {
        Peer { node_id: Some(format!("id-{name}")), name: name.into(), address: address.into(), token: "t".into() }
    }

    #[test]
    fn a_machine_is_found_by_name_id_address_or_unique_beginning() {
        let peers = vec![peer("Attic PC", "192.168.1.20:47400"), peer("Pi", "192.168.1.30:47400")];
        assert_eq!(find(&peers, "attic pc").map(|p| &p.name), Ok(&"Attic PC".to_string()));
        assert_eq!(find(&peers, "id-Pi").map(|p| &p.name), Ok(&"Pi".to_string()));
        assert_eq!(find(&peers, "192.168.1.30:47400").map(|p| &p.name), Ok(&"Pi".to_string()));
        assert_eq!(find(&peers, "att").map(|p| &p.name), Ok(&"Attic PC".to_string()));
        assert!(find(&peers, "nothing").is_err());
    }

    #[test]
    fn an_ambiguous_beginning_names_every_candidate() {
        let peers = vec![peer("Server A", "a:1"), peer("Server B", "b:1")];
        let candidates = find(&peers, "server").expect_err("two match");
        assert_eq!(candidates.len(), 2);
    }
}
