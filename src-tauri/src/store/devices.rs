//! Credentials of devices paired with this node.
//!
//! The public half (name, when it paired) lives on the node record and is
//! shown in the UI. The secret half — a hash of the device's access token —
//! lives here, in its own file, so it is never serialised to a client.

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::error::AppResult;
use crate::store::paths;

const DEVICES_FILE: &str = "devices.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCredential {
    pub device_id: String,
    /// SHA-1 of the bearer token. Tokens are 256 random bits, so a fast hash
    /// is enough: there is nothing to brute-force from it.
    pub token_hash: String,
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn new_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

pub fn load() -> AppResult<Vec<DeviceCredential>> {
    let path = paths::config_dir()?.join(DEVICES_FILE);
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
        Err(_) => Ok(Vec::new()),
    }
}

pub fn save(devices: &[DeviceCredential]) -> AppResult<()> {
    let path = paths::config_dir()?.join(DEVICES_FILE);
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(devices)?)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

/// The device a token belongs to, if any.
pub fn device_for_token(devices: &[DeviceCredential], token: &str) -> Option<String> {
    let hash = hash_token(token);
    devices
        .iter()
        .find(|d| crate::node::pairing::constant_time_eq(d.token_hash.as_bytes(), hash.as_bytes()))
        .map(|d| d.device_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_resolve_to_their_device_only() {
        let token = new_token();
        let devices = vec![DeviceCredential { device_id: "phone".into(), token_hash: hash_token(&token) }];
        assert_eq!(device_for_token(&devices, &token).as_deref(), Some("phone"));
        assert_eq!(device_for_token(&devices, "nope"), None);
        assert_eq!(token.len(), 64);
    }
}
