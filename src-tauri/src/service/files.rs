//! Browsing and editing the files inside an instance.
//!
//! Every path here comes from the UI, which means from a text box. All of it
//! goes through [`crate::store::paths::resolve_inside`]; there is no other way
//! in.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::store::paths::resolve_inside;

/// Refuse to load an "editable" file bigger than this into the editor. A
/// 400 MB region file would take the UI down with it.
const MAX_EDITABLE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    /// Path relative to the instance root, with forward slashes.
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

pub fn list(root: &Path, relative: &str) -> AppResult<Vec<FileEntry>> {
    let dir = if relative.trim().is_empty() {
        root.to_path_buf()
    } else {
        resolve_inside(root, relative)?
    };

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir)?.flatten() {
        let metadata = entry.metadata()?;
        let name = entry.file_name().to_string_lossy().to_string();
        let path = join_relative(relative, &name);
        entries.push(FileEntry {
            name,
            path,
            is_dir: metadata.is_dir(),
            size: if metadata.is_dir() { 0 } else { metadata.len() },
            modified: metadata.modified().ok().map(DateTime::<Utc>::from),
        });
    }

    // Folders first, then case-insensitive by name — how a file manager reads.
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

pub fn read_text(root: &Path, relative: &str) -> AppResult<String> {
    let path = resolve_inside(root, relative)?;
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() > MAX_EDITABLE_BYTES {
        return Err(AppError::new(ErrorCode::FileTooLarge)
            .with("file", relative)
            .with("limit_mb", MAX_EDITABLE_BYTES / 1024 / 1024));
    }
    Ok(std::fs::read_to_string(&path)?)
}

pub fn write_text(root: &Path, relative: &str, contents: &str) -> AppResult<()> {
    let path = resolve_inside(root, relative)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Write beside the target and rename: a half-written server.properties is
    // a server that will not start.
    let temp = path.with_extension("nebula-tmp");
    std::fs::write(&temp, contents)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

pub fn create_dir(root: &Path, relative: &str) -> AppResult<()> {
    std::fs::create_dir_all(resolve_inside(root, relative)?)?;
    Ok(())
}

pub fn delete(root: &Path, relative: &str) -> AppResult<()> {
    let path = resolve_inside(root, relative)?;
    if path == root {
        return Err(AppError::new(ErrorCode::PathOutsideInstance).with("path", relative));
    }
    if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn rename(root: &Path, relative: &str, new_name: &str) -> AppResult<()> {
    if new_name.trim().is_empty() || new_name.contains(['/', '\\']) || new_name.contains("..") {
        return Err(AppError::new(ErrorCode::InvalidInput).with("name", new_name));
    }
    let source = resolve_inside(root, relative)?;
    let parent = source.parent().unwrap_or(root);
    std::fs::rename(&source, parent.join(new_name.trim()))?;
    Ok(())
}

/// How much disk an instance folder takes, and what takes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    pub total_bytes: u64,
    pub file_count: u64,
    /// The biggest top-level entries, largest first (at most ten).
    pub largest: Vec<DiskUsageEntry>,
    pub computed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageEntry {
    pub name: String,
    pub is_dir: bool,
    pub bytes: u64,
}

/// Walk the instance folder. Blocking and potentially slow on a big world:
/// call it from `spawn_blocking`, and cache the answer. Symlinks are not
/// followed, so a link to another drive is not counted against the server.
pub fn usage(root: &Path) -> AppResult<DiskUsage> {
    fn walk(path: &Path, files: &mut u64) -> u64 {
        let Ok(metadata) = std::fs::symlink_metadata(path) else { return 0 };
        if metadata.file_type().is_symlink() {
            return 0;
        }
        if metadata.is_file() {
            *files += 1;
            return metadata.len();
        }
        std::fs::read_dir(path)
            .map(|entries| entries.flatten().map(|entry| walk(&entry.path(), files)).sum())
            .unwrap_or(0)
    }

    let mut file_count = 0;
    let mut largest = Vec::new();
    for entry in std::fs::read_dir(root)?.flatten() {
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let bytes = walk(&entry.path(), &mut file_count);
        largest.push(DiskUsageEntry { name: entry.file_name().to_string_lossy().to_string(), is_dir, bytes });
    }
    largest.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    let total_bytes = largest.iter().map(|e| e.bytes).sum();
    largest.truncate(10);
    Ok(DiskUsage { total_bytes, file_count, largest, computed_at: Utc::now() })
}

fn join_relative(base: &str, name: &str) -> String {
    if base.trim().is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", base.trim_end_matches('/'), name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("nebula-files-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[test]
    fn lists_folders_before_files() {
        let root = temp_root("order");
        std::fs::create_dir(root.join("world")).expect("dir");
        std::fs::write(root.join("server.properties"), "a=b").expect("file");

        let entries = list(&root, "").expect("list");
        assert_eq!(entries[0].name, "world");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "server.properties");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn relative_paths_use_forward_slashes() {
        let root = temp_root("slashes");
        std::fs::create_dir_all(root.join("config")).expect("dir");
        std::fs::write(root.join("config").join("paper.yml"), "x: 1").expect("file");

        let entries = list(&root, "config").expect("list");
        assert_eq!(entries[0].path, "config/paper.yml");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn traversal_is_refused_on_every_operation() {
        let root = temp_root("traversal");
        assert!(list(&root, "../").is_err());
        assert!(read_text(&root, "../secrets.txt").is_err());
        assert!(write_text(&root, "../evil.txt", "x").is_err());
        assert!(delete(&root, "..").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_huge_file_is_refused_rather_than_loaded() {
        let root = temp_root("big");
        let path = root.join("big.dat");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(MAX_EDITABLE_BYTES + 1).expect("grow");
        drop(file);

        let err = read_text(&root, "big.dat").expect_err("too big");
        assert_eq!(err.code, ErrorCode::FileTooLarge);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn writing_replaces_atomically_and_leaves_no_temp_file() {
        let root = temp_root("write");
        write_text(&root, "server.properties", "motd=hi").expect("write");
        assert_eq!(read_text(&root, "server.properties").expect("read"), "motd=hi");

        write_text(&root, "server.properties", "motd=bye").expect("rewrite");
        assert_eq!(read_text(&root, "server.properties").expect("read"), "motd=bye");
        assert_eq!(list(&root, "").expect("list").len(), 1, "no temp file left behind");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn usage_sums_the_tree_and_ranks_the_biggest_entries() {
        let root = temp_root("usage");
        std::fs::create_dir_all(root.join("world").join("region")).expect("dir");
        std::fs::write(root.join("world").join("region").join("r.mca"), vec![0u8; 1000]).expect("file");
        std::fs::write(root.join("server.jar"), vec![0u8; 300]).expect("file");
        let usage = usage(&root).expect("usage");
        assert_eq!(usage.total_bytes, 1300);
        assert_eq!(usage.file_count, 2);
        assert_eq!(usage.largest[0].name, "world");
        assert!(usage.largest[0].is_dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renaming_rejects_a_name_that_would_move_the_file() {
        let root = temp_root("rename");
        std::fs::write(root.join("a.txt"), "x").expect("file");
        assert!(rename(&root, "a.txt", "../b.txt").is_err());
        rename(&root, "a.txt", "b.txt").expect("plain rename");
        assert!(root.join("b.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
