//! Importing a Minecraft world into a server: a singleplayer save or the
//! world folders of another server, already unpacked into a staging folder.
//!
//! The two layouts differ in where the other dimensions live:
//!
//! - Vanilla / Fabric / Forge / NeoForge and singleplayer: one folder,
//!   the Nether in `DIM-1/` and the End in `DIM1/` inside it.
//! - Bukkit family (Paper, Purpur, Spigot): `world/`, `world_nether/DIM-1`,
//!   `world_the_end/DIM1`.
//!
//! Whatever comes in, it lands in the layout the target server reads. (A
//! Bukkit server also migrates DIM-1/DIM1 out of a vanilla world by itself on
//! first start, so a vanilla-shaped world is always safe there.)

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult, ErrorCode};

pub use crate::game::registry::WorldImportReport;

/// Distributions that keep each dimension in its own world folder.
pub fn uses_bukkit_layout(distribution: &str) -> bool {
    matches!(distribution, "paper" | "purpur" | "spigot" | "folia")
}

/// `level-name` from server.properties, or `world`.
pub fn level_name(root: &Path) -> String {
    fs::read_to_string(root.join("server.properties"))
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "level-name").then(|| value.trim().to_string())
            })
        })
        .filter(|name| !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != "..")
        .unwrap_or_else(|| "world".to_string())
}

/// Every folder under `dir` (depth ≤ 4) that holds a `level.dat`.
fn find_worlds(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if dir.join("level.dat").is_file() {
        out.push(dir.to_path_buf());
        // A world folder can contain DIM-1/DIM1 but never another world.
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() && !kind.is_symlink() {
            find_worlds(&entry.path(), depth - 1, out);
        }
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

fn move_dir(from: &Path, to: &Path) -> AppResult<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_dir(from, to)?;
    fs::remove_dir_all(from)?;
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> AppResult<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)?.flatten() {
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_symlink() {
            continue;
        } else if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Put the world found in `staged` where the server at `root` loads it,
/// replacing the current one (the caller backs it up first).
pub fn import(root: &Path, staged: &Path, bukkit_layout: bool) -> AppResult<WorldImportReport> {
    let mut worlds = Vec::new();
    find_worlds(staged, 4, &mut worlds);
    let is_extra = |p: &PathBuf| {
        let name = name_of(p);
        name.ends_with("_nether") || name.ends_with("_the_end")
    };
    // The overworld: not a *_nether / *_the_end folder; the shallowest wins.
    let main = worlds
        .iter()
        .filter(|p| !is_extra(p))
        .min_by_key(|p| p.components().count())
        .cloned()
        .ok_or_else(|| {
            AppError::new(ErrorCode::InvalidInput)
                .with("reason", "no_world")
                .detail("No Minecraft world found: there is no level.dat in what was imported.")
        })?;
    let source_name = name_of(&main);
    let sibling = |suffix: &str| {
        let wanted = format!("{source_name}{suffix}");
        worlds
            .iter()
            .find(|p| name_of(p) == wanted)
            .or_else(|| worlds.iter().find(|p| name_of(p).ends_with(suffix)))
            .cloned()
    };
    let nether = sibling("_nether");
    let end = sibling("_the_end");

    let level = level_name(root);
    let target = root.join(&level);
    let nether_dir = root.join(format!("{level}_nether"));
    let end_dir = root.join(format!("{level}_the_end"));

    // Never delete the current world before the new one is in place: set it
    // aside, place the import, and only then drop what was set aside. If any
    // step fails, the import is removed and the old world is put back — so
    // even an import without a backup cannot lose the world it replaces.
    let aside = root.join(format!(".nebula-replaced-{}", uuid::Uuid::new_v4().simple()));
    let mut set_aside: Vec<(PathBuf, PathBuf)> = Vec::new();
    for old in [target.clone(), nether_dir.clone(), end_dir.clone()] {
        if old.exists() {
            fs::create_dir_all(&aside)?;
            let parked = aside.join(name_of(&old));
            if let Err(error) = fs::rename(&old, &parked) {
                // Undo what was already parked before giving up.
                for (original, parked) in set_aside.iter().rev() {
                    let _ = fs::rename(parked, original);
                }
                let _ = fs::remove_dir_all(&aside);
                return Err(error.into());
            }
            set_aside.push((old, parked));
        }
    }

    let placed = (|| -> AppResult<()> {
        move_dir(&main, &target)?;
        // A singleplayer save carries the game's lock; the server makes its own.
        let _ = fs::remove_file(target.join("session.lock"));

        if bukkit_layout {
            if let Some(nether) = &nether {
                move_dir(nether, &nether_dir)?;
            }
            if let Some(end) = &end {
                move_dir(end, &end_dir)?;
            }
        } else {
            // Fold Bukkit-style dimension folders back into the one world.
            if let Some(dim) = nether.as_ref().map(|n| n.join("DIM-1")).filter(|d| d.is_dir()) {
                if !target.join("DIM-1").exists() {
                    move_dir(&dim, &target.join("DIM-1"))?;
                }
            }
            if let Some(dim) = end.as_ref().map(|e| e.join("DIM1")).filter(|d| d.is_dir()) {
                if !target.join("DIM1").exists() {
                    move_dir(&dim, &target.join("DIM1"))?;
                }
            }
        }
        Ok(())
    })();

    if let Err(error) = placed {
        for new in [&target, &nether_dir, &end_dir] {
            if new.exists() {
                let _ = fs::remove_dir_all(new);
            }
        }
        for (original, parked) in set_aside.iter().rev() {
            let _ = fs::rename(parked, original);
        }
        let _ = fs::remove_dir_all(&aside);
        return Err(error);
    }
    let _ = fs::remove_dir_all(&aside);

    let mut dimensions = vec!["overworld".to_string()];
    if target.join("DIM-1").is_dir() || root.join(format!("{level}_nether")).is_dir() {
        dimensions.push("nether".into());
    }
    if target.join("DIM1").is_dir() || root.join(format!("{level}_the_end")).is_dir() {
        dimensions.push("end".into());
    }
    Ok(WorldImportReport { level_name: level, dimensions, source_name })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nebula-world-{label}-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn world(dir: &Path) {
        fs::create_dir_all(dir.join("region")).unwrap();
        fs::write(dir.join("level.dat"), b"x").unwrap();
    }

    #[test]
    fn singleplayer_save_goes_to_level_name_on_vanilla() {
        let root = temp("root");
        fs::write(root.join("server.properties"), "level-name=mundo\n").unwrap();
        let staged = temp("sp");
        let save = staged.join("My Survival");
        world(&save);
        fs::create_dir_all(save.join("DIM-1/region")).unwrap();
        fs::write(save.join("session.lock"), b"").unwrap();
        let report = import(&root, &staged, false).unwrap();
        assert_eq!(report.level_name, "mundo");
        assert_eq!(report.source_name, "My Survival");
        assert!(root.join("mundo/level.dat").is_file());
        assert!(!root.join("mundo/session.lock").exists());
        assert_eq!(report.dimensions, ["overworld", "nether"]);
    }

    #[test]
    fn bukkit_export_is_folded_for_vanilla_and_kept_for_paper() {
        for bukkit in [false, true] {
            let root = temp("root");
            let staged = temp("export");
            world(&staged.join("server/world"));
            world(&staged.join("server/world_nether"));
            fs::create_dir_all(staged.join("server/world_nether/DIM-1/region")).unwrap();
            world(&staged.join("server/world_the_end"));
            fs::create_dir_all(staged.join("server/world_the_end/DIM1/region")).unwrap();
            let report = import(&root, &staged, bukkit).unwrap();
            assert_eq!(report.dimensions, ["overworld", "nether", "end"]);
            if bukkit {
                assert!(root.join("world_nether/DIM-1").is_dir());
                assert!(root.join("world_the_end/DIM1").is_dir());
            } else {
                assert!(root.join("world/DIM-1").is_dir());
                assert!(root.join("world/DIM1").is_dir());
                assert!(!root.join("world_nether").exists());
            }
        }
    }

    #[test]
    fn a_failed_import_puts_the_old_world_back() {
        let root = temp("root");
        world(&root.join("world"));
        fs::write(root.join("world/keep.txt"), b"old").unwrap();
        // Nothing importable: fails before touching anything.
        let staged = temp("bad");
        assert!(import(&root, &staged, false).is_err());
        assert!(root.join("world/keep.txt").is_file(), "old world untouched");
        // A successful import replaces it and leaves no parked copy behind.
        let staged = temp("good");
        world(&staged.join("New"));
        import(&root, &staged, false).unwrap();
        assert!(!root.join("world/keep.txt").exists());
        let leftovers = fs::read_dir(&root).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(".nebula-replaced")).count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn nothing_to_import_is_a_typed_error() {
        let root = temp("root");
        let staged = temp("empty");
        fs::write(staged.join("readme.txt"), b"hi").unwrap();
        let err = import(&root, &staged, false).unwrap_err();
        assert_eq!(err.params.get("reason").map(String::as_str), Some("no_world"));
    }
}
