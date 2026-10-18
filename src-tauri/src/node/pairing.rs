//! Device pairing.
//!
//! Identity in this app is pairing, not login: there is no account server, and
//! there never will be one. A phone joins a node the way it joins Syncthing —
//! a short code shown on the machine, exchanged on the LAN.
//!
//! Remote access is not shipped yet. What is shipped is the shape: codes that
//! expire, a device list on the node, and no path by which a pairing secret
//! becomes a permanent key.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};

/// How long a pairing code is worth anything. Short on purpose: the code is
/// the whole secret, and a code that never expires is a permanent key to a
/// machine that reads files and spawns processes.
const CODE_LIFETIME_MINUTES: i64 = 5;
const CODE_DIGITS: usize = 6;

/// A pairing code the user is currently reading off their screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingCode {
    pub code: String,
    pub expires_at: DateTime<Utc>,
}

impl PairingCode {
    pub fn is_valid_at(&self, now: DateTime<Utc>) -> bool {
        now < self.expires_at
    }
}

/// Codes issued but not yet used. In memory only — a pairing code must not
/// survive a restart of the app that displayed it.
fn pending() -> &'static Mutex<HashMap<String, PairingCode>> {
    static PENDING: OnceLock<Mutex<HashMap<String, PairingCode>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Issue a code for a node, replacing any previous one.
pub fn issue(node_id: &str) -> AppResult<PairingCode> {
    let code = PairingCode {
        code: random_digits(CODE_DIGITS),
        expires_at: Utc::now() + Duration::minutes(CODE_LIFETIME_MINUTES),
    };

    let mut guard = pending().lock().map_err(|_| AppError::new(ErrorCode::Unexpected))?;
    guard.insert(node_id.to_string(), code.clone());
    Ok(code)
}

/// Consume a code. Single use: a code that can be redeemed twice is a code
/// that can be redeemed by someone who was merely looking over a shoulder.
pub fn redeem(node_id: &str, offered: &str) -> AppResult<()> {
    let mut guard = pending().lock().map_err(|_| AppError::new(ErrorCode::Unexpected))?;

    let Some(code) = guard.get(node_id).cloned() else {
        return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "no_pending_code"));
    };

    if !code.is_valid_at(Utc::now()) {
        guard.remove(node_id);
        return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "code_expired"));
    }

    if !constant_time_eq(code.code.as_bytes(), offered.trim().as_bytes()) {
        return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "code_mismatch"));
    }

    guard.remove(node_id);
    Ok(())
}

/// Drop the pending code for a node (the user closed the pairing screen).
pub fn cancel(node_id: &str) {
    if let Ok(mut guard) = pending().lock() {
        guard.remove(node_id);
    }
}

/// Compare without leaking how much of the code matched through timing.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A code the user has to read aloud, so digits rather than base64.
fn random_digits(len: usize) -> String {
    let mut out = String::with_capacity(len);
    while out.len() < len {
        // uuid v4 is backed by the OS random source; four bits of it per digit
        // is plenty for a code that lives five minutes.
        for byte in uuid::Uuid::new_v4().as_bytes() {
            out.push(char::from(b'0' + (byte % 10)));
            if out.len() == len {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_six_digits() {
        let code = issue("node-digits").expect("issued");
        assert_eq!(code.code.len(), CODE_DIGITS);
        assert!(code.code.chars().all(|c| c.is_ascii_digit()));
        cancel("node-digits");
    }

    #[test]
    fn a_code_expires() {
        let code = issue("node-expiry").expect("issued");
        assert!(code.is_valid_at(Utc::now()));
        assert!(
            !code.is_valid_at(Utc::now() + Duration::minutes(CODE_LIFETIME_MINUTES + 1)),
            "a pairing code must not outlive its window"
        );
        cancel("node-expiry");
    }

    #[test]
    fn a_code_works_once() {
        let code = issue("node-once").expect("issued");
        redeem("node-once", &code.code).expect("first use");
        let err = redeem("node-once", &code.code).expect_err("second use");
        assert_eq!(err.params.get("reason").map(String::as_str), Some("no_pending_code"));
    }

    #[test]
    fn a_wrong_code_is_rejected_without_consuming_the_right_one() {
        let code = issue("node-wrong").expect("issued");
        let err = redeem("node-wrong", "000000").expect_err("mismatch");
        assert_eq!(err.params.get("reason").map(String::as_str), Some("code_mismatch"));
        redeem("node-wrong", &code.code).expect("the real code still works");
    }

    #[test]
    fn issuing_again_replaces_the_previous_code() {
        let first = issue("node-replace").expect("issued");
        let second = issue("node-replace").expect("reissued");
        assert!(redeem("node-replace", &first.code).is_err(), "the old code must stop working");
        redeem("node-replace", &second.code).expect("the new code works");
    }

    #[test]
    fn comparison_rejects_different_lengths() {
        assert!(!constant_time_eq(b"12345", b"123456"));
        assert!(constant_time_eq(b"123456", b"123456"));
    }
}
