//! Turning an installed Minecraft server directory into a command line.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::minecraft::types::ServerType;

/// The JVM a given Minecraft version needs.
pub fn min_java_version(mc_version: &str) -> u32 {
    let release = mc_version.split(['-', ' ', '_']).next().unwrap_or(mc_version).trim();
    let mut parts = release.split('.');
    let major = parts.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);

    if major > 1 || (major == 1 && minor >= 25) {
        25
    } else if major > 1 || (major == 1 && (minor > 20 || (minor == 20 && patch >= 5))) {
        21
    } else if major == 1 && minor >= 17 {
        17
    } else {
        8
    }
}

/// JVM arguments, in the order Java wants them.
pub fn jvm_args(memory_mb: u32, extra: &[String]) -> Vec<String> {
    // Equal min and max heap: a server that grows its heap under load stutters
    // exactly when players notice.
    let mut args = vec![format!("-Xms{memory_mb}M"), format!("-Xmx{memory_mb}M")];
    args.extend_from_slice(extra);
    args
}

/// The `-jar server.jar` / `@arg-file` tail of the command.
pub fn entry_args(server_type: ServerType, root: &Path, prefer_host_os: bool) -> AppResult<Vec<String>> {
    if server_type.uses_installer() {
        if let Some(arg_file) = installer_arg_file(root, prefer_host_os) {
            return Ok(vec![format!("@{arg_file}"), "nogui".to_string()]);
        }
    }

    if root.join("server.jar").exists() {
        return Ok(vec!["-jar".to_string(), "server.jar".to_string(), "nogui".to_string()]);
    }

    Err(AppError::new(ErrorCode::InstanceLaunchFilesMissing).with("path", root.display()))
}

/// True when this directory has something we can actually launch.
pub fn has_launch_artifact(server_type: ServerType, root: &Path, prefer_host_os: bool) -> bool {
    root.join("server.jar").exists()
        || (server_type.uses_installer() && installer_arg_file(root, prefer_host_os).is_some())
}

/// Locate the Forge/NeoForge generated arg file, as a path *relative to the
/// server root*.
///
/// The working directory at launch is the server root, so a path relative to
/// the nested `libraries/…` directory the file was found in would not open.
pub fn installer_arg_file(root: &Path, prefer_host_os: bool) -> Option<String> {
    for candidate in preferred_names(prefer_host_os) {
        if root.join(candidate).exists() {
            return Some(candidate.to_string());
        }
    }
    nested_arg_file(&root.join("libraries"), prefer_host_os)
}

fn preferred_names(prefer_host_os: bool) -> [&'static str; 2] {
    if prefer_host_os && cfg!(target_os = "windows") {
        ["win_args.txt", "unix_args.txt"]
    } else {
        // Inside a Linux container, the Windows arg file would point at the
        // wrong path separators.
        ["unix_args.txt", "win_args.txt"]
    }
}

fn nested_arg_file(libraries: &Path, prefer_host_os: bool) -> Option<String> {
    if !libraries.exists() {
        return None;
    }
    let base = libraries.parent().unwrap_or(libraries);
    for name in preferred_names(prefer_host_os) {
        if let Some(found) = find_file_named(libraries, name) {
            let relative = found.strip_prefix(base).unwrap_or(&found);
            return Some(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    None
}

fn find_file_named(root: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_named(&path, name) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(name))
        {
            return Some(path);
        }
    }
    None
}

/// Record the operator's acceptance of the Minecraft EULA. Called only after
/// the user ticked "I accept" in the UI for this server; the date goes in a
/// comment so the file says when and where it was accepted.
pub fn accept_eula(root: &Path, at: chrono::DateTime<chrono::Utc>) -> AppResult<()> {
    fs::create_dir_all(root)?;
    let contents = format!(
        "#By changing the setting below to TRUE you are indicating your agreement to the EULA (https://aka.ms/MinecraftEULA).\n#Accepted by the server's operator in Nebula Host on {}\neula=true\n",
        at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    );
    fs::write(root.join("eula.txt"), contents)?;
    Ok(())
}

/// Whether `eula.txt` says the EULA was accepted, the way the server reads it.
pub fn eula_accepted(root: &Path) -> bool {
    let Ok(text) = fs::read_to_string(root.join("eula.txt")) else {
        return false;
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .any(|(key, value)| key.trim() == "eula" && value.trim().eq_ignore_ascii_case("true"))
}

/// What a new server calls itself in the multiplayer list. The manifest's
/// `motd` setting declares the same default (a test keeps them in step).
pub const DEFAULT_MOTD: &str = "A Nebula Host Server";

/// Seed `server.properties` so the first launch already uses the right port.
pub fn write_initial_properties(root: &Path, port: u16) -> AppResult<()> {
    let path = root.join("server.properties");
    if path.exists() {
        return Ok(());
    }
    let contents = format!(
        "#Minecraft server properties\nserver-port={port}\nmotd={DEFAULT_MOTD}\nmax-players=20\nonline-mode=true\n"
    );
    fs::write(path, contents)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eula_counts_only_once_it_is_accepted() {
        let root = std::env::temp_dir().join(format!("nebula-eula-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("dir");
        assert!(!eula_accepted(&root), "no file is not accepted");

        // What the server writes itself on a first launch without consent.
        fs::write(root.join("eula.txt"), "#eula=true is not it\neula=false\n").expect("write");
        assert!(!eula_accepted(&root), "false and comments are not accepted");

        accept_eula(&root, chrono::Utc::now()).expect("accept");
        assert!(eula_accepted(&root));
        let text = fs::read_to_string(root.join("eula.txt")).expect("read");
        assert!(text.contains("https://aka.ms/MinecraftEULA") && text.contains("Nebula Host on "));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_server_is_named_after_nebula_and_an_existing_one_is_left_alone() {
        let root = std::env::temp_dir().join(format!("nebula-initprops-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp root");
        write_initial_properties(&root, 25570).expect("write");
        let text = fs::read_to_string(root.join("server.properties")).expect("read");
        assert!(text.contains("motd=A Nebula Host Server"));
        assert!(text.contains("server-port=25570"));

        fs::write(root.join("server.properties"), "motd=Mine\n").expect("rewrite");
        write_initial_properties(&root, 25570).expect("no-op");
        assert_eq!(fs::read_to_string(root.join("server.properties")).expect("read"), "motd=Mine\n");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn min_java_version_matches_mojang_requirements() {
        assert_eq!(min_java_version("1.16.5"), 8);
        assert_eq!(min_java_version("1.17"), 17);
        assert_eq!(min_java_version("1.20.4"), 17);
        assert_eq!(min_java_version("1.20.5"), 21);
        assert_eq!(min_java_version("1.21.4"), 21);
        assert_eq!(min_java_version("1.25"), 25);
    }

    #[test]
    fn snapshot_names_fall_back_to_the_release_prefix() {
        assert_eq!(min_java_version("1.21.4-pre1"), 21);
        assert_eq!(min_java_version("1.18 Snapshot"), 17);
    }

    #[test]
    fn heap_is_pinned_to_one_size() {
        assert_eq!(jvm_args(4096, &[]), vec!["-Xms4096M", "-Xmx4096M"]);
    }

    #[test]
    fn extra_args_follow_the_heap_flags() {
        let args = jvm_args(2048, &["-XX:+UseG1GC".to_string()]);
        assert_eq!(args.last().map(String::as_str), Some("-XX:+UseG1GC"));
    }

    #[test]
    fn nested_arg_file_path_stays_relative_to_the_server_root() {
        // The bug this guards: the recursive search used to return
        // "21.1.238/win_args.txt", relative to the innermost directory, and
        // Java could not open it.
        let root = std::env::temp_dir().join(format!("nebula-argfile-{}", std::process::id()));
        let nested = root
            .join("libraries")
            .join("net")
            .join("neoforged")
            .join("neoforge")
            .join("21.1.238");
        fs::create_dir_all(&nested).expect("temp tree");
        fs::write(nested.join("win_args.txt"), "-jar x").expect("arg file");

        let found = nested_arg_file(&root.join("libraries"), true);
        assert_eq!(
            found.as_deref(),
            Some("libraries/net/neoforged/neoforge/21.1.238/win_args.txt")
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_launch_files_are_a_typed_error() {
        let root = std::env::temp_dir().join(format!("nebula-empty-{}", std::process::id()));
        fs::create_dir_all(&root).expect("temp dir");
        let err = entry_args(ServerType::Vanilla, &root, true).expect_err("nothing installed");
        assert_eq!(err.code, ErrorCode::InstanceLaunchFilesMissing);
        assert!(!has_launch_artifact(ServerType::Vanilla, &root, true));
        let _ = fs::remove_dir_all(&root);
    }
}
