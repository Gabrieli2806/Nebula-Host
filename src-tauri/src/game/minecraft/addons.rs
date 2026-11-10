//! Switching mods and plugins off and on: a disabled jar keeps its bytes and
//! gains a suffix, so the game skips it and nothing is lost.
//!
//! Searching, identifying and installing go through `modrinth.rs`, the one
//! Modrinth client (it verifies hashes and only downloads from Modrinth).

use std::path::Path;

use crate::error::{AppError, AppResult, ErrorCode};

use super::modrinth::DISABLED_SUFFIX;

/// True for a jar that was switched off (renamed with the disabled suffix).
pub fn is_disabled_name(file_name: &str) -> bool {
    file_name.ends_with(DISABLED_SUFFIX)
}

/// Toggle an addon by renaming it, so disabling never loses the file.
pub fn set_enabled(dir: &Path, file_name: &str, enabled: bool) -> AppResult<String> {
    let current = crate::store::paths::resolve_inside(dir, file_name)?;
    if !current.exists() {
        return Err(AppError::new(ErrorCode::FileNotFound).with("file", file_name));
    }

    let target_name = if enabled {
        file_name.trim_end_matches(DISABLED_SUFFIX).to_string()
    } else if file_name.ends_with(DISABLED_SUFFIX) {
        file_name.to_string()
    } else {
        format!("{file_name}{DISABLED_SUFFIX}")
    };

    if target_name != file_name {
        std::fs::rename(&current, dir.join(&target_name))?;
    }
    Ok(target_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabling_and_re_enabling_keeps_one_suffix() {
        let dir = std::env::temp_dir().join(format!("nebula-addons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("mod.jar"), b"x").expect("jar");

        let disabled = set_enabled(&dir, "mod.jar", false).expect("disable");
        assert_eq!(disabled, "mod.jar.disabled");
        assert!(dir.join("mod.jar.disabled").exists());

        let enabled = set_enabled(&dir, "mod.jar.disabled", true).expect("enable");
        assert_eq!(enabled, "mod.jar");
        assert!(dir.join("mod.jar").exists());

        assert!(!dir.join("mod.jar.disabled").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
