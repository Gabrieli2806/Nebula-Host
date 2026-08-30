//! Running a server inside a container instead of directly on the host.
//!
//! Optional everywhere: the host process is the default, and Docker is for
//! people who want the server sandboxed or pinned to a specific runtime image.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::runtime::java::configure_no_window_command;
use crate::runtime::process::StartRequest;

const DEFAULT_IMAGE: &str = "eclipse-temurin:21-jre";

/// Allow pointing at podman or a rootless socket without a rebuild.
fn docker_command() -> String {
    std::env::var("NEBULA_DOCKER_CMD")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "docker".to_string())
}

/// Is a container runtime actually usable right now? Installed is not enough —
/// Docker Desktop is frequently installed but not running.
pub fn check_available() -> AppResult<()> {
    let docker = docker_command();
    let mut command = Command::new(&docker);
    let output = configure_no_window_command(&mut command)
        .args(["version", "--format", "{{.Server.Version}}"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            AppError::new(ErrorCode::DockerNotAvailable).with("command", &docker).detail(error)
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(AppError::new(ErrorCode::DockerNotAvailable)
        .with("command", docker)
        .detail(if stderr.is_empty() { "the daemon did not answer".into() } else { stderr }))
}

/// A container name a human can recognise in `docker ps`:
/// `"My SMP Server"` + id `a1b2c3d4-…` → `nebula-my-smp-server-a1b2c3d4`.
pub fn container_name(id: &str, name: &str) -> String {
    let mut sanitised = String::new();
    let mut last_was_dash = false;
    for ch in name.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            sanitised.push(ch);
            last_was_dash = false;
        } else if !last_was_dash && !sanitised.is_empty() {
            sanitised.push('-');
            last_was_dash = true;
        }
    }
    while sanitised.ends_with('-') {
        sanitised.pop();
    }
    sanitised.truncate(40);

    let short_id = id.split('-').next().unwrap_or(id).to_ascii_lowercase();
    if sanitised.is_empty() {
        format!("nebula-{short_id}")
    } else {
        format!("nebula-{sanitised}-{short_id}")
    }
}

/// Build the `docker run` invocation for a launch plan.
pub fn command_for(request: &StartRequest, container_name: &str) -> AppResult<Command> {
    check_available()?;
    remove_stale(container_name);

    let image = request.container_image.clone().unwrap_or_else(|| DEFAULT_IMAGE.to_string());
    let mount = format!(
        "type=bind,source={},target=/data",
        mount_source(&request.plan.working_dir)
    );

    let mut command = Command::new(docker_command());
    command
        .arg("run")
        .arg("--rm")
        .arg("-i")
        .arg("--name")
        .arg(container_name)
        .arg("--workdir")
        .arg("/data")
        .arg("--mount")
        .arg(mount)
        .arg("--publish")
        .arg(format!("{}:{}", request.port, request.port))
        .arg(image)
        // The plan's program is a host path; inside the image the runtime is
        // simply on PATH.
        .arg(program_name(&request.plan.program))
        .args(&request.plan.args);

    Ok(command)
}

/// A container left behind by a crash holds the name and the port.
fn remove_stale(container_name: &str) {
    let mut command = Command::new(docker_command());
    let _ = configure_no_window_command(&mut command)
        .args(["rm", "-f", container_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub fn stop(container_name: &str) -> AppResult<()> {
    let mut command = Command::new(docker_command());
    let output = configure_no_window_command(&mut command)
        .args(["stop", container_name])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| AppError::new(ErrorCode::DockerNotAvailable).detail(error))?;

    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(AppError::new(ErrorCode::InstanceStopTimedOut)
        .with("container", container_name)
        .detail(if stderr.is_empty() { "docker stop failed".into() } else { stderr }))
}

/// Windows canonical paths come back as `\\?\C:\…`, which the Docker CLI
/// rejects as a mount source.
fn mount_source(path: &Path) -> String {
    let normalised =
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        if let Some(stripped) = normalised.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{stripped}");
        }
        if let Some(stripped) = normalised.strip_prefix(r"\\?\") {
            return stripped.to_string();
        }
    }

    normalised
}

/// `C:/jdk21/bin/java.exe` on the host is just `java` in the image.
fn program_name(program: &str) -> String {
    Path::new(program)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| program.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_name_is_readable_and_unique() {
        assert_eq!(
            container_name("a1b2c3d4-e5f6-7890-abcd-ef1234567890", "My SMP Server"),
            "nebula-my-smp-server-a1b2c3d4"
        );
    }

    #[test]
    fn container_name_falls_back_to_the_id_when_nothing_survives_sanitising() {
        assert_eq!(container_name("a1b2c3d4-e5f6", "¡¡¡···!!!"), "nebula-a1b2c3d4");
    }

    #[test]
    fn container_name_collapses_runs_of_punctuation() {
        assert_eq!(container_name("abcd1234", "Sky   Block!!! 2"), "nebula-sky-block-2-abcd1234");
    }

    #[test]
    fn host_java_path_becomes_the_image_binary() {
        assert_eq!(program_name("C:/Program Files/jdk21/bin/java.exe"), "java");
        assert_eq!(program_name("/usr/bin/java"), "java");
        assert_eq!(program_name("java"), "java");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_extended_paths_are_stripped_for_the_mount() {
        // `canonicalize` only returns \\?\ paths for real files, so exercise
        // the stripping directly on a path that does not exist.
        let path = Path::new(r"\\?\C:\servers\smp");
        assert_eq!(mount_source(path), r"C:\servers\smp");
    }
}
