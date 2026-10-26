//! Bringing files from the user's machine into an instance: what a drag and
//! drop onto the Files panel turns into.
//!
//! The UI hands over *absolute paths from the OS drop event* — never bytes —
//! so a 4 GB world folder copies at disk speed instead of crawling through
//! IPC. Sources are the user's own files (they just dragged them); the
//! *destination* is what must stay inside the instance, so every target goes
//! through [`resolve_inside`] and every name is a single plain component.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::store::paths::resolve_inside;

/// Progress is reported at most this often, so a folder of ten thousand tiny
/// files does not flood the event channel.
const PROGRESS_EVERY: Duration = Duration::from_millis(150);
const COPY_BUFFER: usize = 1024 * 1024;
/// More sources than this in one drop is a mistake, not an import.
const MAX_SOURCES: usize = 256;

/// What to do when an item of the same name is already there.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Replace files; merge folders into the existing folder.
    Overwrite,
    /// Import beside it as `name (1).ext` / `folder (1)`.
    #[default]
    KeepBoth,
    /// Leave the existing item alone and do not import this one.
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    /// File or folder name as it would land in the target folder.
    pub name: String,
    pub is_dir: bool,
    /// Something of this name already exists in the target folder.
    pub exists: bool,
    /// What exists is the other kind (a file where a folder is dropped, or
    /// the reverse). Overwrite cannot merge those, so it keeps both instead.
    #[serde(default)]
    pub type_mismatch: bool,
    pub files: u64,
    pub bytes: u64,
}

/// A dry run, so the UI can ask about conflicts before copying anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCheck {
    pub items: Vec<ImportItem>,
    /// Names from `items` that already exist.
    pub conflicts: Vec<String>,
    pub total_files: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub import_id: String,
    pub copied_files: u64,
    pub total_files: u64,
    pub copied_bytes: u64,
    pub total_bytes: u64,
    /// Instance-relative path of the file being copied.
    pub current: Option<String>,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedItem {
    pub source_name: String,
    /// Instance-relative path it landed at (forward slashes).
    pub path: String,
    /// Landed under a new name because of `keep_both`.
    pub renamed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    /// Source path or name.
    pub path: String,
    pub code: ErrorCode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub import_id: String,
    pub copied_files: u64,
    pub copied_bytes: u64,
    pub imported: Vec<ImportedItem>,
    /// Source names skipped by the `skip` policy.
    pub skipped: Vec<String>,
    /// Sources that could not be imported, and why. Symlinks land here.
    pub failed: Vec<ImportFailure>,
}

/// The source as a validated (path, landing name, is_dir).
fn source_entry(source: &str) -> AppResult<(PathBuf, String, bool)> {
    let path = PathBuf::from(source);
    if !path.is_absolute() {
        return Err(AppError::new(ErrorCode::InvalidInput).with("path", source));
    }
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| AppError::new(ErrorCode::FileNotFound).with("path", source))?;
    if metadata.file_type().is_symlink() {
        // Following a link could import half the disk; refuse it outright.
        return Err(AppError::new(ErrorCode::InvalidInput).with("path", source).with("reason", "symlink"));
    }
    let name = plain_name(&path).ok_or_else(|| AppError::new(ErrorCode::InvalidInput).with("path", source))?;
    Ok((path, name, metadata.is_dir()))
}

/// The final component, if it is a plain name (not `..`, not a drive root).
fn plain_name(path: &Path) -> Option<String> {
    match path.components().next_back()? {
        Component::Normal(name) => {
            let name = name.to_string_lossy().to_string();
            (!name.is_empty() && !name.contains(['/', '\\'])).then_some(name)
        }
        _ => None,
    }
}

fn target_dir(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let dir = if relative.trim().is_empty() { root.to_path_buf() } else { resolve_inside(root, relative)? };
    if dir.exists() && !dir.is_dir() {
        return Err(AppError::new(ErrorCode::InvalidInput).with("path", relative));
    }
    Ok(dir)
}

/// Refuse to copy a folder into itself (it would recurse until the disk is
/// full) — e.g. dragging the instance folder onto its own Files panel.
fn refuse_into_itself(source: &Path, target: &Path) -> AppResult<()> {
    let source = source.canonicalize()?;
    let target = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
    if target.starts_with(&source) {
        return Err(AppError::new(ErrorCode::InvalidInput)
            .with("path", source.display())
            .with("reason", "into_itself"));
    }
    Ok(())
}

/// What is already at `path`, without following a link: `None` when
/// nothing is there (a dangling link counts as something).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Existing {
    File,
    Dir,
    Link,
}

fn existing(path: &Path) -> Option<Existing> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    let kind = metadata.file_type();
    Some(if kind.is_symlink() {
        Existing::Link
    } else if kind.is_dir() {
        // A Windows junction reports as a directory here but is a reparse
        // point: treat it as a link, never write through it.
        if is_reparse_point(&metadata) {
            Existing::Link
        } else {
            Existing::Dir
        }
    } else {
        Existing::File
    })
}

#[cfg(windows)]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_: &std::fs::Metadata) -> bool {
    false
}

/// A link (or junction) inside the instance may point anywhere; writing
/// through it would put the import outside the instance.
fn refuse_link(path: &Path) -> AppResult<()> {
    if existing(path) == Some(Existing::Link) {
        return Err(AppError::new(ErrorCode::PathOutsideInstance)
            .with("path", path.display())
            .with("reason", "symlink"));
    }
    Ok(())
}

/// Something is there and it is the other kind (file vs folder).
fn mismatched(path: &Path, is_dir: bool) -> bool {
    matches!(
        (existing(path), is_dir),
        (Some(Existing::File), true) | (Some(Existing::Dir), false)
    )
}

/// Count files and bytes under a path, not following symlinks.
fn measure(path: &Path) -> (u64, u64) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else { return (0, 0) };
    if metadata.file_type().is_symlink() {
        return (0, 0);
    }
    if metadata.is_file() {
        return (1, metadata.len());
    }
    let mut files = 0;
    let mut bytes = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let (f, b) = measure(&entry.path());
            files += f;
            bytes += b;
        }
    }
    (files, bytes)
}

pub fn check(root: &Path, target: &str, sources: &[String]) -> AppResult<ImportCheck> {
    if sources.is_empty() || sources.len() > MAX_SOURCES {
        return Err(AppError::new(ErrorCode::InvalidInput).with("field", "sources"));
    }
    let dir = target_dir(root, target)?;
    let mut items = Vec::new();
    for source in sources {
        let (path, name, is_dir) = source_entry(source)?;
        let (files, bytes) = measure(&path);
        let landing = dir.join(&name);
        items.push(ImportItem {
            exists: existing(&landing).is_some(),
            type_mismatch: mismatched(&landing, is_dir),
            name,
            is_dir,
            files,
            bytes,
        });
    }
    Ok(ImportCheck {
        conflicts: items.iter().filter(|i| i.exists).map(|i| i.name.clone()).collect(),
        total_files: items.iter().map(|i| i.files).sum(),
        total_bytes: items.iter().map(|i| i.bytes).sum(),
        items,
    })
}

// -- bundles sent from another machine ---------------------------------------
//
// A remote UI cannot hand over paths on its own disk, so its backend zips the
// dropped items (each under `<index>/<name>`) and streams that to `/upload`.
// The node unpacks it next to the upload and imports from there.

fn stage_dir(upload_id: &str) -> AppResult<PathBuf> {
    if uuid::Uuid::parse_str(upload_id).is_err() {
        return Err(AppError::new(ErrorCode::InvalidInput).with("field", "uploadId"));
    }
    Ok(crate::service::world::uploads_dir()?.join(format!("{upload_id}.files")))
}

/// Unpack an uploaded bundle (once) and list its items, in the order sent.
pub fn stage_upload(upload_id: &str) -> AppResult<Vec<String>> {
    let dir = stage_dir(upload_id)?;
    if !dir.is_dir() {
        let archive = crate::service::world::upload_path(upload_id)?;
        let partial = dir.with_extension("partial");
        let _ = std::fs::remove_dir_all(&partial);
        // `extract` refuses entries that would land outside the folder.
        zip::ZipArchive::new(File::open(&archive)?)
            .and_then(|mut zip| zip.extract(&partial))
            .map_err(|e| AppError::new(ErrorCode::InvalidInput).with("reason", "bad_bundle").detail(e.to_string()))?;
        std::fs::rename(&partial, &dir)?;
        let _ = std::fs::remove_file(&archive);
    }
    let mut items = Vec::new();
    for index in 0.. {
        let slot = dir.join(index.to_string());
        if !slot.is_dir() {
            break;
        }
        for entry in std::fs::read_dir(&slot)? {
            items.push(entry?.path().to_string_lossy().to_string());
        }
    }
    Ok(items)
}

/// Every source must be an item of this upload, nothing else on the disk.
pub fn ensure_staged(upload_id: &str, sources: &[String]) -> AppResult<()> {
    let dir = stage_dir(upload_id)?.canonicalize()?;
    for source in sources {
        let path = Path::new(source).canonicalize()?;
        if !path.starts_with(&dir) || path == dir {
            return Err(AppError::new(ErrorCode::PathOutsideInstance).with("path", source));
        }
    }
    Ok(())
}

/// Forget an upload: the bundle and what was unpacked from it.
pub fn discard_upload(upload_id: &str) -> AppResult<()> {
    let dir = stage_dir(upload_id)?;
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(dir.with_extension("zip"));
    Ok(())
}

/// Zip local items for `stage_upload` on another machine: each source under
/// `<index>/<its name>`, folders recursively, links skipped.
pub fn bundle(sources: &[String], out: &Path) -> AppResult<()> {
    if sources.is_empty() || sources.len() > MAX_SOURCES {
        return Err(AppError::new(ErrorCode::InvalidInput).with("field", "sources"));
    }
    let mut zip = zip::ZipWriter::new(File::create(out)?);
    // Mods and worlds are mostly compressed already: store, don't deflate.
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    fn add(zip: &mut zip::ZipWriter<File>, options: zip::write::SimpleFileOptions, path: &Path, name: &str) -> AppResult<()> {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            return Ok(());
        }
        let wrap = |e: zip::result::ZipError| AppError::new(ErrorCode::IoFailed).detail(e.to_string());
        if metadata.is_dir() {
            zip.add_directory(format!("{name}/"), options).map_err(wrap)?;
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let child = entry.file_name().to_string_lossy().to_string();
                add(zip, options, &entry.path(), &format!("{name}/{child}"))?;
            }
        } else {
            zip.start_file(name, options).map_err(wrap)?;
            std::io::copy(&mut File::open(path)?, zip)?;
        }
        Ok(())
    }
    for (index, source) in sources.iter().enumerate() {
        let path = PathBuf::from(source);
        let name = plain_name(&path).ok_or_else(|| AppError::new(ErrorCode::InvalidInput).with("path", source))?;
        add(&mut zip, options, &path, &format!("{index}/{name}"))?;
    }
    zip.finish().map_err(|e| AppError::new(ErrorCode::IoFailed).detail(e.to_string()))?;
    Ok(())
}

/// `name (n).ext` for files, `name (n)` for folders, first free n.
pub fn free_name(dir: &Path, name: &str, is_dir: bool) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if !is_dir && dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    for n in 1..10_000 {
        let candidate = format!("{stem} ({n}){ext}");
        if existing(&dir.join(&candidate)).is_none() {
            return candidate;
        }
    }
    format!("{stem} ({}){ext}", uuid::Uuid::new_v4().simple())
}

struct Copier<'a> {
    root: &'a Path,
    progress: ImportProgress,
    last_report: Instant,
    report: &'a dyn Fn(ImportProgress),
}

impl Copier<'_> {
    fn tick(&mut self, force: bool) {
        if force || self.last_report.elapsed() >= PROGRESS_EVERY {
            self.last_report = Instant::now();
            (self.report)(self.progress.clone());
        }
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(self.root).unwrap_or(path).to_string_lossy().replace('\\', "/")
    }

    /// Copy one file to `to`, replacing a file there. Never deletes a folder
    /// and never writes through a link: callers pick another name for those.
    fn copy_file(&mut self, from: &Path, to: &Path) -> AppResult<()> {
        refuse_link(to)?;
        if existing(to) == Some(Existing::Dir) {
            return Err(AppError::new(ErrorCode::InvalidInput)
                .with("path", self.relative(to))
                .with("reason", "type_mismatch"));
        }
        self.progress.current = Some(self.relative(to));
        // Copy beside the target and rename: a cancelled or failed copy must
        // not leave a truncated `level.dat` where a good one was.
        let part = to.with_file_name(format!(
            "{}.nebula-part",
            to.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        ));
        let result = (|| -> AppResult<()> {
            let mut input = File::open(from)?;
            let mut output = File::create(&part)?;
            let mut buf = vec![0u8; COPY_BUFFER];
            loop {
                let read = input.read(&mut buf)?;
                if read == 0 {
                    break;
                }
                output.write_all(&buf[..read])?;
                self.progress.copied_bytes += read as u64;
                self.tick(false);
            }
            output.flush()?;
            drop(output);
            if existing(to) == Some(Existing::File) {
                std::fs::remove_file(to)?;
            }
            std::fs::rename(&part, to)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        result?;
        self.progress.copied_files += 1;
        self.tick(false);
        Ok(())
    }

    /// Copy a tree, merging into the folder already at `to`. A nested entry
    /// that meets the other kind (a file where a folder comes in, or the
    /// reverse) lands beside it under a free name; nothing existing is
    /// deleted to make room.
    fn copy_tree(&mut self, from: &Path, to: &Path) -> AppResult<()> {
        refuse_link(to)?;
        if existing(to) == Some(Existing::File) {
            return Err(AppError::new(ErrorCode::InvalidInput)
                .with("path", self.relative(to))
                .with("reason", "type_mismatch"));
        }
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)?.flatten() {
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_symlink() {
                continue;
            }
            let Some(name) = plain_name(&entry.path()) else { continue };
            let is_dir = file_type.is_dir();
            let dest = if mismatched(&to.join(&name), is_dir) {
                to.join(free_name(to, &name, is_dir))
            } else {
                to.join(&name)
            };
            if is_dir {
                self.copy_tree(&entry.path(), &dest)?;
            } else {
                self.copy_file(&entry.path(), &dest)?;
            }
        }
        Ok(())
    }
}

/// Copy `sources` into `target` (instance-relative). Blocking: call it from
/// `spawn_blocking`. Per-source failures are collected, not fatal.
pub fn import(
    root: &Path,
    target: &str,
    sources: &[String],
    policy: ConflictPolicy,
    import_id: &str,
    report: &dyn Fn(ImportProgress),
) -> AppResult<ImportSummary> {
    let check = check(root, target, sources)?;
    let dir = target_dir(root, target)?;
    std::fs::create_dir_all(&dir)?;

    let mut copier = Copier {
        root,
        progress: ImportProgress {
            import_id: import_id.to_string(),
            copied_files: 0,
            total_files: check.total_files,
            copied_bytes: 0,
            total_bytes: check.total_bytes,
            current: None,
            done: false,
        },
        last_report: Instant::now(),
        report,
    };
    copier.tick(true);

    let mut summary = ImportSummary {
        import_id: import_id.to_string(),
        copied_files: 0,
        copied_bytes: 0,
        imported: Vec::new(),
        skipped: Vec::new(),
        failed: Vec::new(),
    };

    for source in sources {
        let outcome = (|| -> AppResult<Option<ImportedItem>> {
            let (path, name, is_dir) = source_entry(source)?;
            if is_dir {
                refuse_into_itself(&path, &dir)?;
            }
            let here = dir.join(&name);
            let exists = existing(&here).is_some();
            // Overwrite merges folders and replaces files; it cannot turn a
            // folder into a file or the reverse without deleting data, so a
            // mismatch keeps both instead.
            let policy = if mismatched(&here, is_dir) && policy == ConflictPolicy::Overwrite {
                ConflictPolicy::KeepBoth
            } else {
                policy
            };
            let (landing, renamed) = match (exists, policy) {
                (true, ConflictPolicy::Skip) => return Ok(None),
                (true, ConflictPolicy::KeepBoth) => (free_name(&dir, &name, is_dir), true),
                _ => (name.clone(), false),
            };
            let dest = dir.join(&landing);
            refuse_link(&dest)?;
            if is_dir {
                copier.copy_tree(&path, &dest)?;
            } else {
                copier.copy_file(&path, &dest)?;
            }
            Ok(Some(ImportedItem { source_name: name, path: copier.relative(&dest), renamed }))
        })();

        match outcome {
            Ok(Some(item)) => summary.imported.push(item),
            Ok(None) => summary.skipped.push(plain_name(Path::new(source)).unwrap_or_else(|| source.clone())),
            Err(error) => summary.failed.push(ImportFailure { path: source.clone(), code: error.code }),
        }
    }

    copier.progress.done = true;
    copier.progress.current = None;
    copier.tick(true);
    summary.copied_files = copier.progress.copied_files;
    summary.copied_bytes = copier.progress.copied_bytes;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        outside: PathBuf,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("instance");
        let outside = tmp.path().join("desktop");
        std::fs::create_dir_all(root.join("plugins")).expect("root");
        std::fs::create_dir_all(outside.join("world").join("region")).expect("world");
        std::fs::write(outside.join("world").join("level.dat"), b"level").expect("level");
        std::fs::write(outside.join("world").join("region").join("r.0.0.mca"), vec![7u8; 3000]).expect("region");
        std::fs::write(outside.join("Essentials.jar"), b"jar").expect("jar");
        Fixture { _tmp: tmp, root, outside }
    }

    fn s(path: PathBuf) -> String {
        path.to_string_lossy().to_string()
    }

    #[test]
    fn imports_files_and_folders_recursively_with_progress() {
        let f = fixture();
        let events = RefCell::new(Vec::new());
        let summary = import(
            &f.root,
            "",
            &[s(f.outside.join("world")), s(f.outside.join("Essentials.jar"))],
            ConflictPolicy::KeepBoth,
            "t1",
            &|p| events.borrow_mut().push(p),
        )
        .expect("import");

        assert_eq!(summary.copied_files, 3);
        assert_eq!(summary.copied_bytes, 5 + 3000 + 3);
        assert!(f.root.join("world/region/r.0.0.mca").exists());
        assert!(f.root.join("Essentials.jar").exists());
        assert_eq!(summary.imported[0].path, "world");
        let last = events.borrow().last().cloned().expect("progress");
        assert!(last.done);
        assert_eq!(last.copied_files, last.total_files);
        assert!(!f.root.join("Essentials.jar.nebula-part").exists(), "no temp files left");
    }

    #[test]
    fn imports_into_a_subfolder_and_reports_relative_paths() {
        let f = fixture();
        let summary = import(&f.root, "plugins", &[s(f.outside.join("Essentials.jar"))], ConflictPolicy::Overwrite, "t", &|_| {})
            .expect("import");
        assert_eq!(summary.imported[0].path, "plugins/Essentials.jar");
    }

    #[test]
    fn conflict_policies_do_what_they_say() {
        let f = fixture();
        std::fs::write(f.root.join("Essentials.jar"), b"old").expect("existing");
        let jar = vec![s(f.outside.join("Essentials.jar"))];

        let check = check(&f.root, "", &jar).expect("check");
        assert_eq!(check.conflicts, vec!["Essentials.jar".to_string()]);

        let skipped = import(&f.root, "", &jar, ConflictPolicy::Skip, "a", &|_| {}).expect("skip");
        assert_eq!(skipped.skipped, vec!["Essentials.jar".to_string()]);
        assert_eq!(std::fs::read(f.root.join("Essentials.jar")).expect("read"), b"old");

        let both = import(&f.root, "", &jar, ConflictPolicy::KeepBoth, "b", &|_| {}).expect("keep both");
        assert_eq!(both.imported[0].path, "Essentials (1).jar");
        assert!(both.imported[0].renamed);

        import(&f.root, "", &jar, ConflictPolicy::Overwrite, "c", &|_| {}).expect("overwrite");
        assert_eq!(std::fs::read(f.root.join("Essentials.jar")).expect("read"), b"jar");
    }

    #[test]
    fn keep_both_names_folders_without_splitting_dots() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join("my.world")).expect("dir");
        assert_eq!(free_name(&f.root, "my.world", true), "my.world (1)");
        assert_eq!(free_name(&f.root, ".env", false), ".env (1)");
        assert_eq!(free_name(&f.root, "a.tar.gz", false), "a.tar (1).gz");
    }

    #[test]
    fn the_destination_can_never_leave_the_instance() {
        let f = fixture();
        let jar = vec![s(f.outside.join("Essentials.jar"))];
        for target in ["..", "../desktop", "plugins/../../desktop", "/etc", "C:/Windows"] {
            let err = import(&f.root, target, &jar, ConflictPolicy::Overwrite, "x", &|_| {}).expect_err(target);
            assert_eq!(err.code, ErrorCode::PathOutsideInstance, "{target}");
        }
        assert!(!f.outside.join("desktop").exists());
    }

    #[test]
    fn relative_or_missing_sources_and_self_copies_are_refused() {
        let f = fixture();
        let err = check(&f.root, "", &["relative/thing.jar".into()]).expect_err("relative");
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let err = check(&f.root, "", &[s(f.outside.join("nope.jar"))]).expect_err("missing");
        assert_eq!(err.code, ErrorCode::FileNotFound);

        // Dragging the instance onto its own Files panel.
        let summary =
            import(&f.root, "plugins", &[s(f.root.clone())], ConflictPolicy::KeepBoth, "x", &|_| {}).expect("runs");
        assert_eq!(summary.failed.len(), 1);
        assert_eq!(summary.failed[0].code, ErrorCode::InvalidInput);
    }

    #[test]
    fn overwrite_merges_folders_instead_of_deleting_them() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join("world")).expect("dir");
        std::fs::write(f.root.join("world").join("keep.txt"), b"mine").expect("keep");
        import(&f.root, "", &[s(f.outside.join("world"))], ConflictPolicy::Overwrite, "m", &|_| {}).expect("merge");
        assert!(f.root.join("world/keep.txt").exists(), "existing files survive a merge");
        assert!(f.root.join("world/level.dat").exists());
    }

    #[test]
    fn overwrite_never_deletes_the_other_kind() {
        let f = fixture();
        // A file named like an existing folder: the folder must survive.
        std::fs::create_dir_all(f.root.join("plugins").join("Essentials")).expect("dir");
        std::fs::write(f.root.join("plugins/Essentials/config.yml"), b"cfg").expect("cfg");
        std::fs::write(f.outside.join("plugins"), b"a file").expect("file");
        let check = check(&f.root, "", &[s(f.outside.join("plugins"))]).expect("check");
        assert!(check.items[0].type_mismatch);
        let summary =
            import(&f.root, "", &[s(f.outside.join("plugins"))], ConflictPolicy::Overwrite, "k", &|_| {})
                .expect("import");
        assert!(f.root.join("plugins/Essentials/config.yml").exists(), "folder kept");
        assert!(summary.imported[0].renamed);
        assert!(f.root.join("plugins (1)").is_file());

        // A folder named like an existing file: the file must survive.
        std::fs::write(f.root.join("world"), b"not a folder").expect("file");
        import(&f.root, "", &[s(f.outside.join("world"))], ConflictPolicy::Overwrite, "k2", &|_| {})
            .expect("import");
        assert_eq!(std::fs::read(f.root.join("world")).expect("read"), b"not a folder");
        assert!(f.root.join("world (1)/level.dat").exists());
    }

    #[cfg(windows)]
    #[test]
    fn never_writes_through_a_junction_inside_the_instance() {
        let f = fixture();
        let elsewhere = f.outside.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("elsewhere");
        let link = f.root.join("world");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&elsewhere)
            .output()
            .expect("mklink");
        if !made.status.success() {
            return; // junctions unavailable on this filesystem
        }
        let summary =
            import(&f.root, "", &[s(f.outside.join("world"))], ConflictPolicy::Overwrite, "j", &|_| {})
                .expect("import");
        assert_eq!(summary.failed.len(), 1);
        assert_eq!(summary.failed[0].code, ErrorCode::PathOutsideInstance);
        assert!(std::fs::read_dir(&elsewhere).expect("read").next().is_none(), "nothing written outside");
    }
}
