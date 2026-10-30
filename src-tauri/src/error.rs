//! Typed, translatable errors.
//!
//! The backend never returns a human sentence. It returns a *code* plus named
//! parameters; the UI owns the wording and the language. This is the one rule
//! that keeps Spanish users from seeing English error strings, so every
//! fallible boundary in this crate returns [`AppError`], never `String`.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Every error the UI can be asked to render.
///
/// Adding a variant here is a promise to add a translation for it in
/// `src/i18n/*`; the frontend maps this union exhaustively, so a missing entry
/// fails `npm run typecheck`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    // --- generic -----------------------------------------------------------
    /// Anything we failed to classify. Always carries `detail`.
    Unexpected,
    NotFound,
    InvalidInput,
    Cancelled,

    // --- node --------------------------------------------------------------
    NodeUnreachable,
    NodeBusy,

    // --- instance lifecycle ------------------------------------------------
    InstanceNotFound,
    InstanceAlreadyRunning,
    InstanceNotRunning,
    InstanceLaunchFilesMissing,
    InstanceStartFailed,
    InstanceStopTimedOut,
    InstanceCrashed,
    /// A server taken back after an app restart: its console input is gone.
    ConsoleDetached,

    // --- installation ------------------------------------------------------
    GameNotSupported,
    VersionNotFound,
    LoaderVersionMismatch,
    InstallFailed,
    DownloadFailed,
    ChecksumMismatch,

    // --- runtime prerequisites ---------------------------------------------
    JavaNotFound,
    JavaTooOld,
    DockerNotAvailable,
    NotEnoughMemory,
    NotEnoughDisk,

    // --- network -----------------------------------------------------------
    PortInUse,
    PortForwardFailed,
    RouterNotFound,
    WakeProxyBindFailed,

    // --- files -------------------------------------------------------------
    PathOutsideInstance,
    FileTooLarge,
    FileNotFound,
    IoFailed,

    // --- backups -----------------------------------------------------------
    BackupFailed,
    RestoreFailed,
    BackupNotFound,

    // --- external services -------------------------------------------------
    UpstreamUnavailable,
    UpstreamRejected,
}

impl ErrorCode {
    /// Stable string form, used in logs and as the frontend translation key.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unexpected => "unexpected",
            Self::NotFound => "not_found",
            Self::InvalidInput => "invalid_input",
            Self::Cancelled => "cancelled",
            Self::NodeUnreachable => "node_unreachable",
            Self::NodeBusy => "node_busy",
            Self::InstanceNotFound => "instance_not_found",
            Self::InstanceAlreadyRunning => "instance_already_running",
            Self::InstanceNotRunning => "instance_not_running",
            Self::InstanceLaunchFilesMissing => "instance_launch_files_missing",
            Self::InstanceStartFailed => "instance_start_failed",
            Self::InstanceStopTimedOut => "instance_stop_timed_out",
            Self::InstanceCrashed => "instance_crashed",
            Self::ConsoleDetached => "console_detached",
            Self::GameNotSupported => "game_not_supported",
            Self::VersionNotFound => "version_not_found",
            Self::LoaderVersionMismatch => "loader_version_mismatch",
            Self::InstallFailed => "install_failed",
            Self::DownloadFailed => "download_failed",
            Self::ChecksumMismatch => "checksum_mismatch",
            Self::JavaNotFound => "java_not_found",
            Self::JavaTooOld => "java_too_old",
            Self::DockerNotAvailable => "docker_not_available",
            Self::NotEnoughMemory => "not_enough_memory",
            Self::NotEnoughDisk => "not_enough_disk",
            Self::PortInUse => "port_in_use",
            Self::PortForwardFailed => "port_forward_failed",
            Self::RouterNotFound => "router_not_found",
            Self::WakeProxyBindFailed => "wake_proxy_bind_failed",
            Self::PathOutsideInstance => "path_outside_instance",
            Self::FileTooLarge => "file_too_large",
            Self::FileNotFound => "file_not_found",
            Self::IoFailed => "io_failed",
            Self::BackupFailed => "backup_failed",
            Self::RestoreFailed => "restore_failed",
            Self::BackupNotFound => "backup_not_found",
            Self::UpstreamUnavailable => "upstream_unavailable",
            Self::UpstreamRejected => "upstream_rejected",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A failure crossing the IPC boundary.
///
/// `params` are substituted into the translated string (`{port}`, `{version}`).
/// `detail` is raw upstream text, shown only behind "technical details" and
/// never as the primary message.
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: ErrorCode,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode) -> Self {
        Self { code, params: BTreeMap::new(), detail: None }
    }

    /// Attach a substitution parameter, e.g. `.with("port", 25565)`.
    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.params.insert(key.to_string(), value.to_string());
        self
    }

    /// Attach raw upstream text for the "technical details" disclosure.
    pub fn detail(mut self, detail: impl fmt::Display) -> Self {
        self.detail = Some(detail.to_string());
        self
    }

    pub fn unexpected(detail: impl fmt::Display) -> Self {
        Self::new(ErrorCode::Unexpected).detail(detail)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code.as_str())?;
        if let Some(detail) = &self.detail {
            write!(f, ": {detail}")?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        let code = match value.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::FileNotFound,
            std::io::ErrorKind::AddrInUse => ErrorCode::PortInUse,
            _ => ErrorCode::IoFailed,
        };
        Self::new(code).detail(value)
    }
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        let code = if value.is_status() {
            ErrorCode::UpstreamRejected
        } else {
            ErrorCode::UpstreamUnavailable
        };
        Self::new(code).detail(value)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::new(ErrorCode::Unexpected).detail(value)
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// Convenience for the common `code` (+ formatted `detail`) construction.
#[macro_export]
macro_rules! err {
    ($code:expr) => { $crate::error::AppError::new($code) };
    ($code:expr, $($arg:tt)*) => {
        $crate::error::AppError::new($code).detail(format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_code_as_snake_case_string() {
        let json = serde_json::to_value(AppError::new(ErrorCode::PortInUse).with("port", 25565))
            .expect("serialize");
        assert_eq!(json["code"], "port_in_use");
        assert_eq!(json["params"]["port"], "25565");
        assert!(json.get("detail").is_none(), "absent detail must not serialize");
    }

    #[test]
    fn io_not_found_maps_to_file_not_found() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "nope");
        assert_eq!(AppError::from(io).code, ErrorCode::FileNotFound);
    }

    #[test]
    fn code_str_matches_serde_representation() {
        for code in [ErrorCode::Unexpected, ErrorCode::JavaTooOld, ErrorCode::WakeProxyBindFailed] {
            let json = serde_json::to_string(&code).expect("serialize");
            assert_eq!(json, format!("\"{}\"", code.as_str()));
        }
    }
}

/// A `nebula_downloader` failure as an app error: a checksum mismatch keeps
/// its own code (the file was corrupt or tampered with), the rest are
/// download failures. One mapping for every downloader caller.
pub fn download_error(file: impl ToString, error: String) -> AppError {
    let code = if nebula_downloader::is_integrity_error(&error) {
        ErrorCode::ChecksumMismatch
    } else {
        ErrorCode::DownloadFailed
    };
    AppError::new(code).with("file", file).detail(error)
}
