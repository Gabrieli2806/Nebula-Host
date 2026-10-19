//! Where the app keeps things on disk, and the rule that keeps user-supplied
//! paths inside the directory they belong to.

use std::path::{Component, Path, PathBuf};

use crate::error::{AppError, AppResult, ErrorCode};

const APP_DIR: &str = "nebula-host";

/// Config and state (settings.json, nodes, instance index).
pub fn config_dir() -> AppResult<PathBuf> {
    let dir = dirs::config_dir()
        .ok_or_else(|| AppError::new(ErrorCode::IoFailed).detail("no config directory on this OS"))?
        .join(APP_DIR);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Where server files live by default. Overridable in settings — an old PC
/// used as a server often has the space on a second drive.
pub fn default_instances_dir() -> AppResult<PathBuf> {
    let dir = dirs::data_dir()
        .ok_or_else(|| AppError::new(ErrorCode::IoFailed).detail("no data directory on this OS"))?
        .join(APP_DIR)
        .join("instances");
    Ok(dir)
}

pub fn backups_dir() -> AppResult<PathBuf> {
    let dir = dirs::data_dir()
        .ok_or_else(|| AppError::new(ErrorCode::IoFailed).detail("no data directory on this OS"))?
        .join(APP_DIR)
        .join("backups");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Java runtimes the app downloaded itself. Kept apart from the system's
/// own JVMs so removing the app (or this folder) never breaks a Java the
/// user installed.
pub fn runtimes_dir() -> AppResult<PathBuf> {
    let dir = dirs::data_dir()
        .ok_or_else(|| AppError::new(ErrorCode::IoFailed).detail("no data directory on this OS"))?
        .join(APP_DIR)
        .join("runtimes");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Addon changes waiting for a server to stop: the queue files and the jars
/// already downloaded for them.
pub fn pending_dir() -> AppResult<PathBuf> {
    let dir = config_dir()?.join("pending-addons");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Resolve `relative` against `root`, refusing anything that escapes it.
///
/// This is the only way user-supplied paths reach the filesystem. The backend
/// reads and writes arbitrary files on request, so a `..` that slips through
/// here is the whole machine.
pub fn resolve_inside(root: &Path, relative: &str) -> AppResult<PathBuf> {
    // Paths arrive from any client: a Windows desktop managing a Linux node
    // sends `C:/…` or `\\server\share`, which Linux would read as ordinary
    // names. Read them the Windows way everywhere and refuse them alike.
    let normalised = relative.replace('\\', "/");
    let drive = normalised.as_bytes();
    if drive.len() >= 2 && drive[0].is_ascii_alphabetic() && drive[1] == b':' {
        return Err(outside(relative));
    }
    let candidate = Path::new(&normalised);

    if candidate.is_absolute() || normalised.starts_with('/') {
        return Err(outside(relative));
    }

    let mut resolved = root.to_path_buf();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => resolved.push(part),
            Component::CurDir => {}
            // `..`, a drive letter, or a UNC prefix: all of them are attempts
            // to leave, whether or not they were typed on purpose.
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(outside(relative));
            }
        }
    }

    // Symlinks can still point out of the tree, so re-check the real path when
    // it already exists. A path that does not exist yet cannot be a symlink.
    if resolved.exists() {
        let real_root = root.canonicalize()?;
        let real = resolved.canonicalize()?;
        if !real.starts_with(&real_root) {
            return Err(outside(relative));
        }
    }

    Ok(resolved)
}

fn outside(relative: &str) -> AppError {
    AppError::new(ErrorCode::PathOutsideInstance).with("path", relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_paths_inside_the_root() {
        let root = Path::new("/srv/instance");
        let resolved = resolve_inside(root, "world/level.dat").expect("inside");
        assert_eq!(resolved, root.join("world").join("level.dat"));
    }

    #[test]
    fn normalises_current_directory_segments() {
        let root = Path::new("/srv/instance");
        let resolved = resolve_inside(root, "./config/./paper.yml").expect("inside");
        assert_eq!(resolved, root.join("config").join("paper.yml"));
    }

    #[test]
    fn rejects_parent_traversal() {
        let root = Path::new("/srv/instance");
        for attempt in ["../secrets", "world/../../etc/passwd", "..", "a/../../b"] {
            let err = resolve_inside(root, attempt).expect_err(attempt);
            assert_eq!(err.code, ErrorCode::PathOutsideInstance, "{attempt}");
        }
    }

    #[test]
    fn rejects_absolute_paths() {
        let root = Path::new("/srv/instance");
        for attempt in ["/etc/passwd", "C:/Windows/System32/config/SAM", r"\\server\share"] {
            let err = resolve_inside(root, attempt).expect_err(attempt);
            assert_eq!(err.code, ErrorCode::PathOutsideInstance, "{attempt}");
        }
    }
}
