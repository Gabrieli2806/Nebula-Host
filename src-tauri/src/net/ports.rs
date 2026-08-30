//! Port availability.

use std::net::{Ipv4Addr, TcpListener};

use crate::error::{AppError, AppResult, ErrorCode};

/// Ports below this need privileges on most systems and belong to services.
pub const MIN_USER_PORT: u16 = 1024;

/// Can we bind this port right now?
///
/// Binds and immediately drops. Never call this while holding an accepted
/// socket on the same port — the answer would be "no" because of *us*, which
/// is how a wake proxy once decided its own server could never start.
pub fn is_free(port: u16) -> bool {
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_ok()
}

/// The first free port at or after `start`, searching a bounded window.
pub fn first_free_from(start: u16) -> Option<u16> {
    (start..start.saturating_add(200)).find(|port| *port >= MIN_USER_PORT && is_free(*port))
}

/// Check a port the user typed.
pub fn validate(port: u16) -> AppResult<()> {
    if port < MIN_USER_PORT {
        return Err(AppError::new(ErrorCode::InvalidInput)
            .with("port", port)
            .with("min", MIN_USER_PORT));
    }
    if !is_free(port) {
        return Err(AppError::new(ErrorCode::PortInUse).with("port", port));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_we_are_holding_reads_as_taken() {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("ephemeral port");
        let port = listener.local_addr().expect("addr").port();
        assert!(!is_free(port));
        // Deliberately no "and free again after drop": once released, the port
        // goes back to the ephemeral pool and any other test — or anything
        // else on the machine — may take it before we look. That assertion
        // tested the OS, and failed about one run in ten.
    }

    #[test]
    fn privileged_ports_are_rejected_before_we_even_try() {
        let err = validate(80).expect_err("privileged");
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn a_busy_port_is_reported_as_in_use() {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("ephemeral port");
        let port = listener.local_addr().expect("addr").port();
        let err = validate(port).expect_err("busy");
        assert_eq!(err.code, ErrorCode::PortInUse);
        assert_eq!(err.params.get("port").map(String::as_str), Some(port.to_string().as_str()));
    }

    #[test]
    fn the_search_skips_a_taken_port() {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("ephemeral port");
        let taken = listener.local_addr().expect("addr").port();
        let found = first_free_from(taken).expect("something free nearby");
        assert_ne!(found, taken);
        assert!(found > taken);
    }
}
