//! Finding a JVM to run a game with.
//!
//! Games that need Java say so in their manifest and tell us which major
//! version a given build requires; this module only answers "where is a JVM of
//! at least major N".

use std::path::PathBuf;
use std::process::{Command, Stdio};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::store::settings;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Keep spawned helpers from flashing a console window on Windows.
pub fn configure_no_window_command(command: &mut Command) -> &mut Command {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// A JVM the user has configured or that we found on PATH.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaInstallation {
    pub path: String,
    pub major: u32,
    pub version: String,
    /// True when it came from `PATH` rather than the user's settings.
    #[serde(default)]
    pub from_path: bool,
}

/// Resolve the Java executable to use.
///
/// Order: an explicit per-instance override, then the user's configured path
/// for a major version at least `required_major` (the lowest one that
/// satisfies it — newer JVMs run older servers, but a 1.20 server does not
/// need Java 25), then plain `java` from `PATH`.
pub fn resolve_java(override_path: Option<&str>, required_major: u32) -> AppResult<PathBuf> {
    if let Some(path) = override_path.map(str::trim).filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    if let Some(path) = configured_java_for_major(required_major) {
        return Ok(PathBuf::from(path));
    }

    if cfg!(target_os = "android") {
        return Err(AppError::new(ErrorCode::JavaNotFound).with("major", required_major));
    }

    Ok(PathBuf::from("java"))
}

/// The lowest configured JVM that still satisfies `required_major`.
fn configured_java_for_major(required_major: u32) -> Option<String> {
    let stored = settings::load().ok()?;

    let mut candidates: Vec<(u32, String)> = stored
        .java_paths
        .iter()
        .filter_map(|(major, path)| {
            let major: u32 = major.parse().ok()?;
            let path = path.trim();
            (!path.is_empty() && major >= required_major).then(|| (major, path.to_string()))
        })
        .collect();
    candidates.sort_by_key(|(major, _)| *major);

    candidates
        .into_iter()
        .next()
        .map(|(_, path)| path)
        .or_else(|| stored.java_path.filter(|p| !p.trim().is_empty()))
}

/// Ask a JVM which version it is. Used by the first-run health check, so it
/// must never hang: `java -version` on a broken install can block forever
/// otherwise.
pub fn probe(java: &std::path::Path) -> AppResult<JavaInstallation> {
    let mut command = Command::new(java);
    let output = configure_no_window_command(&mut command)
        .arg("-version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            AppError::new(ErrorCode::JavaNotFound)
                .with("path", java.display())
                .detail(error)
        })?;

    // `java -version` writes to stderr on every JVM worth supporting.
    let text = String::from_utf8_lossy(&output.stderr);
    let text = if text.trim().is_empty() { String::from_utf8_lossy(&output.stdout) } else { text };

    let version = parse_version_string(&text).ok_or_else(|| {
        AppError::new(ErrorCode::JavaNotFound)
            .with("path", java.display())
            .detail(text.trim().to_string())
    })?;

    Ok(JavaInstallation {
        path: java.display().to_string(),
        major: major_from_version(&version),
        version,
        from_path: java.parent().is_none(),
    })
}

/// Pull `21.0.5` out of `openjdk version "21.0.5" 2024-10-15`.
fn parse_version_string(output: &str) -> Option<String> {
    let line = output.lines().find(|line| line.contains("version"))?;
    let start = line.find('"')? + 1;
    let rest = line.get(start..)?;
    let end = rest.find('"')?;
    Some(rest.get(..end)?.to_string())
}

/// `1.8.0_402` is Java 8; `21.0.5` is Java 21.
fn major_from_version(version: &str) -> u32 {
    let mut parts = version.split(['.', '_', '-']);
    let first: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    if first == 1 {
        parts.next().and_then(|p| p.parse().ok()).unwrap_or(0)
    } else {
        first
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_override_wins() {
        let resolved = resolve_java(Some("C:/jdk21/bin/java.exe"), 21).expect("override accepted");
        assert_eq!(resolved, PathBuf::from("C:/jdk21/bin/java.exe"));
    }

    #[test]
    fn blank_override_is_ignored() {
        let resolved = resolve_java(Some("   "), 8).expect("falls through");
        assert_ne!(resolved, PathBuf::from("   "));
    }

    #[test]
    fn parses_version_strings_from_real_jvms() {
        assert_eq!(
            parse_version_string("openjdk version \"21.0.5\" 2024-10-15").as_deref(),
            Some("21.0.5")
        );
        assert_eq!(
            parse_version_string("java version \"1.8.0_402\"\nJava(TM) SE Runtime").as_deref(),
            Some("1.8.0_402")
        );
        assert_eq!(parse_version_string("command not found"), None);
    }

    #[test]
    fn legacy_one_dot_x_versions_report_the_second_component() {
        assert_eq!(major_from_version("1.8.0_402"), 8);
        assert_eq!(major_from_version("21.0.5"), 21);
        assert_eq!(major_from_version("25-ea"), 25);
    }
}
