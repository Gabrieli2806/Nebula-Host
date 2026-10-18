//! Moving a server to another machine, and back, without losing a block.
//!
//! The rules, in the order a transfer runs them:
//!
//! 1. **Export** (source): the server is stopped (refused while players are on
//!    unless the user confirmed), its wake listener and router lease let go,
//!    and it is *locked* — it cannot start until the transfer ends. The whole
//!    folder plus its record goes into one archive with a SHA-256.
//! 2. **Adopt** (target): the archive is verified, unpacked beside the
//!    instances folder and swapped into place. The copy takes the next
//!    generation, a free port, and is marked active. Nothing about the source
//!    has changed yet.
//! 3. **Finalize** (source): only now, after the target said yes, the source
//!    marks its copy as *transferred out* (kept as the older copy) or deletes
//!    it. A source that was started after the export refuses — its copy is
//!    newer than the one that left, so the target's is thrown away instead.
//! 4. **Commit** (target): forget what the adoption replaced.
//!
//! Anything that fails before step 3 leaves the source authoritative and the
//! target as it was. Moving back is the same thing in the other direction; an
//! older copy waiting on the target is replaced by the newer one coming in.
//!
//! An older copy can be forced into play anyway. That *forks* it: it takes a
//! new generation, records which copy it split from, and from then on the two
//! are different servers that happen to share an id — adopting one over the
//! other is refused (or done side by side, when asked).
//!
//! Backups stay with the machine that made them; they are not part of the
//! archive.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};
use zip::write::SimpleFileOptions;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::manifest::RuntimeSpec;
use crate::model::{ForkInfo, Instance, InstanceStatus, TransferState};
use crate::net::ports;
use crate::service::{AppEvent, InstanceService};
use crate::store::{instances::INSTANCE_FILE, paths, settings};

/// Bumped when the archive layout changes incompatibly.
pub const ARCHIVE_FORMAT: u32 = 1;
/// The record inside the archive: which server, from which machine.
const MANIFEST_FILE: &str = "nebula-transfer.json";
/// Folders the app itself creates inside an instance and never moves.
const SCRATCH_PREFIXES: [&str; 3] = [".nebula-import-", ".nebula-incoming-", ".nebula-replaced-"];
/// How long a server may take to stop before an export gives up.
const STOP_WAIT: Duration = Duration::from_secs(120);
/// Exports nobody collected (a desktop that vanished mid-transfer) are
/// cleaned after this long.
const STALE_EXPORT: chrono::TimeDelta = chrono::TimeDelta::hours(24);
/// Progress events at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

// -- wire types --------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferPhase {
    /// Stopping the server and locking it.
    Prepare,
    /// Packing the folder (source).
    Export,
    /// Moving the bytes (through the desktop that runs the transfer).
    Upload,
    /// Checking the archive arrived intact (target).
    Verify,
    /// Unpacking and registering (target).
    Adopt,
    /// Telling the source the move is done.
    Finalize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub transfer_id: String,
    pub phase: TransferPhase,
    pub bytes: u64,
    pub total: u64,
    /// The whole transfer is over (successfully or not).
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub failed: bool,
}

/// What finalize does with the source's copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizeMode {
    /// Keep it as the older copy, marked so it does not start by accident.
    #[default]
    Keep,
    /// Delete its files.
    Delete,
}

/// The source's answer to an export.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportInfo {
    pub export_id: String,
    pub instance_id: String,
    pub name: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub generation: u64,
    pub source_node_id: String,
    pub source_node_name: String,
    pub exported_at: DateTime<Utc>,
}

/// The record that travels inside the archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveManifest {
    format: u32,
    instance: Instance,
    source_node_id: String,
    source_node_name: String,
    exported_at: DateTime<Utc>,
}

/// Kept next to an export so finalize still works after a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportSidecar {
    instance_id: String,
    exported_at: DateTime<Utc>,
    /// What the folder and record looked like when packed. Finalize refuses
    /// when they changed since: those changes are not in the copy that left.
    #[serde(default)]
    fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortChange {
    pub from: u16,
    pub to: u16,
}

/// The target's answer to an adoption.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptReport {
    /// The record as it now stands on the target.
    pub instance: Instance,
    pub node_id: String,
    pub node_name: String,
    /// The port was taken on the target, so another was picked.
    pub port_changed: Option<PortChange>,
    /// The Java major this server needs and the target does not have.
    pub java_missing: Option<u32>,
    /// An older copy that was waiting here was replaced.
    pub replaced_older_copy: bool,
    /// A Java path chosen on the source was dropped (it is a path there).
    pub runtime_path_cleared: bool,
    /// Adopted next to an existing copy under a new id (`keepBoth`).
    pub kept_both: bool,
}

/// A server being exported. In memory: a node restart drops the lock, and
/// finalize then relies on the export's sidecar and `last_started_at`.
#[derive(Debug, Clone)]
pub struct ExportLock {
    pub export_id: String,
    /// The desktop's id for the whole transfer, so it can call the export
    /// off even when it never learned the export id (a lost answer).
    pub transfer_id: String,
    pub prior_status: InstanceStatus,
}

/// An adoption waiting for the source's confirmation.
#[derive(Debug, Clone)]
pub struct Adoption {
    /// The folder and record this adoption replaced, to put back on discard.
    replaced: Option<(PathBuf, Instance)>,
    /// The transfer that brought it, to undo it when its id is unknown.
    transfer_id: String,
}

// -- the rules ---------------------------------------------------------------

/// What adopting an incoming copy does, given what is already here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdoptDecision {
    /// Nothing with this id here: a new server.
    New,
    /// An older copy left behind by an earlier transfer: replace it.
    ReplaceOlder,
    /// Keep what is here and adopt the incoming one under a new id.
    NewCopy,
}

/// The lineage rule. An older copy (transferred out) is replaced only by a
/// strictly newer generation; an active copy is never overwritten — two
/// active copies of one server have diverged, and only the user can say
/// which one wins.
pub fn decide(existing: Option<&Instance>, incoming_generation: u64, keep_both: bool) -> Result<AdoptDecision, &'static str> {
    match existing {
        None => Ok(AdoptDecision::New),
        Some(here) if here.is_transferred_out() && incoming_generation > here.generation => {
            Ok(AdoptDecision::ReplaceOlder)
        }
        Some(_) if keep_both => Ok(AdoptDecision::NewCopy),
        Some(here) if here.is_transferred_out() => Err("transfer_not_newer"),
        Some(_) => Err("transfer_diverged"),
    }
}

/// The record as the target keeps it.
fn adopted_record(mut incoming: Instance, node_id: &str, path: PathBuf, fresh_id: bool) -> (Instance, bool) {
    if fresh_id {
        incoming.id = uuid::Uuid::new_v4().to_string();
    }
    incoming.node_id = node_id.to_string();
    incoming.path = path;
    incoming.generation += 1;
    incoming.transfer = TransferState::Active;
    incoming.automation.restart_attempts_remaining = 0;
    // A JVM path is a path on the source machine.
    let cleared = incoming.resources.runtime_path.take().is_some();
    (incoming, cleared)
}

/// Force an older copy into play: it becomes a fork of the one that left.
pub fn fork(instance: &mut Instance, now: DateTime<Utc>) -> AppResult<()> {
    let TransferState::TransferredOut { to_node_id, to_node_name, generation, .. } = instance.transfer.clone() else {
        return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "not_transferred_out"));
    };
    instance.forked_from = Some(ForkInfo { node_id: to_node_id, node_name: to_node_name, generation, at: now });
    instance.generation = instance.generation.max(generation) + 1;
    instance.transfer = TransferState::Active;
    Ok(())
}

/// The port to use on the target: the incoming one when it is free and no
/// other server here claims it, otherwise the next one that is.
pub fn pick_port(wanted: u16, claimed: &[u16], is_free: impl Fn(u16) -> bool) -> u16 {
    let usable = |port: u16| port >= ports::MIN_USER_PORT && !claimed.contains(&port) && is_free(port);
    if usable(wanted) {
        return wanted;
    }
    (wanted.saturating_add(1)..wanted.saturating_add(400)).find(|port| usable(*port)).unwrap_or(wanted)
}

// -- files -------------------------------------------------------------------

/// Archives a node exported and is waiting to have collected.
pub fn exports_dir() -> AppResult<PathBuf> {
    let dir = crate::service::world::uploads_dir()?.join("exports");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The archive an export id refers to. Ids are UUIDs we issued.
pub fn export_path(export_id: &str) -> AppResult<PathBuf> {
    export_path_in(&exports_dir()?, export_id)
}

fn export_path_in(dir: &Path, export_id: &str) -> AppResult<PathBuf> {
    if uuid::Uuid::parse_str(export_id).is_err() {
        return Err(AppError::new(ErrorCode::InvalidInput).with("field", "exportId"));
    }
    let path = dir.join(format!("{export_id}.zip"));
    if !path.is_file() {
        return Err(AppError::new(ErrorCode::FileNotFound).with("export", export_id));
    }
    Ok(path)
}

fn sidecar_path(archive: &Path) -> PathBuf {
    archive.with_extension("json")
}

fn remove_export(archive: &Path) {
    let _ = std::fs::remove_file(archive);
    let _ = std::fs::remove_file(sidecar_path(archive));
}

fn clean_stale_exports(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|modified| Utc::now().signed_duration_since(DateTime::<Utc>::from(modified)) > STALE_EXPORT)
            .unwrap_or(false);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Throttled progress reporting from blocking code.
struct Meter<'a> {
    report: &'a (dyn Fn(u64, u64) + Sync),
    total: u64,
    bytes: u64,
    last: Instant,
}

impl<'a> Meter<'a> {
    fn new(total: u64, report: &'a (dyn Fn(u64, u64) + Sync)) -> Self {
        report(0, total);
        Self { report, total, bytes: 0, last: Instant::now() }
    }

    fn add(&mut self, n: u64) {
        self.bytes += n;
        if self.last.elapsed() >= PROGRESS_EVERY {
            self.last = Instant::now();
            (self.report)(self.bytes.min(self.total), self.total);
        }
    }

    fn finish(&self) {
        (self.report)(self.total, self.total);
    }
}

fn copy_metered(reader: &mut impl Read, writer: &mut impl Write, meter: &mut Meter<'_>) -> std::io::Result<u64> {
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut copied = 0;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            return Ok(copied);
        }
        writer.write_all(&buffer[..n])?;
        copied += n as u64;
        meter.add(n as u64);
    }
}

/// SHA-256 of a file, hex.
pub fn sha256_file(path: &Path, report: &(dyn Fn(u64, u64) + Sync)) -> AppResult<String> {
    let mut file = std::fs::File::open(path)?;
    let total = file.metadata()?.len();
    let mut meter = Meter::new(total, report);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        meter.add(n as u64);
    }
    meter.finish();
    Ok(hex::encode(hasher.finalize()))
}

fn is_scratch(name: &str) -> bool {
    SCRATCH_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// Every file and folder to move, relative to `root`, with the byte total.
fn collect(root: &Path) -> AppResult<(Vec<(String, PathBuf, bool)>, u64)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf, bool)>, total: &mut u64) -> AppResult<()> {
        for entry in std::fs::read_dir(dir)?.flatten() {
            let kind = entry.file_type()?;
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let top = relative.split('/').next().unwrap_or_default();
            if relative == INSTANCE_FILE || relative == MANIFEST_FILE || is_scratch(top) {
                continue;
            }
            // A link (a world on a USB disk, say) points outside the folder:
            // skipping it would move the server without its data.
            if kind.is_symlink() {
                return Err(AppError::new(ErrorCode::InvalidInput)
                    .with("reason", "transfer_symlink")
                    .with("path", relative));
            }
            if kind.is_dir() {
                out.push((format!("{relative}/"), path.clone(), true));
                walk(root, &path, out, total)?;
            } else {
                *total += entry.metadata()?.len();
                out.push((relative, path, false));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    let mut total = 0;
    walk(root, root, &mut out, &mut total)?;
    Ok((out, total))
}

/// Checked before the server is stopped: everything in the folder can move
/// and arrive intact on any machine. Refuses links (their data would be left
/// behind) and names that differ only by case (one would overwrite the other
/// on Windows or macOS).
fn preflight(root: &Path) -> AppResult<()> {
    let (entries, _) = collect(root)?;
    let mut seen = std::collections::HashSet::new();
    for (name, _, _) in &entries {
        if !seen.insert(name.to_lowercase()) {
            return Err(AppError::new(ErrorCode::InvalidInput)
                .with("reason", "transfer_name_collision")
                .with("path", name.trim_end_matches('/')));
        }
    }
    Ok(())
}

/// A digest of every file's name, size and modification time, plus the
/// record. Anything written after packing changes it.
fn fingerprint(root: &Path, record: &Instance) -> AppResult<String> {
    let (mut entries, _) = collect(root)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(record)?);
    for (name, path, is_dir) in entries {
        hasher.update(name.as_bytes());
        if !is_dir {
            let meta = std::fs::metadata(&path)?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or_default();
            hasher.update(format!("\0{}\0{modified}", meta.len()).as_bytes());
        }
        hasher.update(b"\n");
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Already compressed (or huge): storing is as small and much faster on a Pi.
fn store_only(name: &str, size: u64) -> bool {
    const PACKED: [&str; 10] = [".mca", ".mcc", ".jar", ".zip", ".gz", ".png", ".jpg", ".ogg", ".7z", ".xz"];
    size > 256 * 1024 * 1024 || PACKED.iter().any(|ext| name.to_ascii_lowercase().ends_with(ext))
}

fn write_archive(root: &Path, archive: &Path, manifest: &ArchiveManifest, report: &(dyn Fn(u64, u64) + Sync)) -> AppResult<u64> {
    let (entries, total) = collect(root)?;
    let mut meter = Meter::new(total, report);
    let fail = |error: zip::result::ZipError| AppError::new(ErrorCode::IoFailed).with("reason", "archive").detail(error);

    let file = std::fs::File::create(archive)?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let deflate = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    zip.start_file(MANIFEST_FILE, deflate).map_err(fail)?;
    zip.write_all(&serde_json::to_vec_pretty(manifest)?)?;

    for (name, path, is_dir) in entries {
        if is_dir {
            zip.add_directory(name.trim_end_matches('/'), stored).map_err(fail)?;
            continue;
        }
        let size = std::fs::metadata(&path)?.len();
        let options = if store_only(&name, size) { stored } else { deflate }.large_file(size >= u32::MAX as u64);
        zip.start_file(name, options).map_err(fail)?;
        let mut source = std::fs::File::open(&path)?;
        copy_metered(&mut source, &mut zip, &mut meter)?;
    }

    let mut writer = zip.finish().map_err(fail)?;
    writer.flush()?;
    drop(writer);
    meter.finish();
    Ok(std::fs::metadata(archive)?.len())
}

fn read_manifest(archive: &Path) -> AppResult<ArchiveManifest> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|error| AppError::new(ErrorCode::InvalidInput).with("reason", "not_a_transfer").detail(error))?;
    let mut entry = zip
        .by_name(MANIFEST_FILE)
        .map_err(|error| AppError::new(ErrorCode::InvalidInput).with("reason", "not_a_transfer").detail(error))?;
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    let manifest: ArchiveManifest = serde_json::from_str(&text)
        .map_err(|error| AppError::new(ErrorCode::InvalidInput).with("reason", "not_a_transfer").detail(error))?;
    if manifest.format > ARCHIVE_FORMAT {
        return Err(AppError::new(ErrorCode::InvalidInput)
            .with("reason", "transfer_too_new")
            .with("format", manifest.format));
    }
    Ok(manifest)
}

fn extract_archive(archive: &Path, into: &Path, report: &(dyn Fn(u64, u64) + Sync)) -> AppResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|error| AppError::new(ErrorCode::InvalidInput).with("reason", "not_a_transfer").detail(error))?;
    let total: u64 = (0..zip.len()).filter_map(|i| zip.by_index_raw(i).ok().map(|e| e.size())).sum();
    let mut meter = Meter::new(total, report);
    std::fs::create_dir_all(into)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|error| AppError::new(ErrorCode::IoFailed).detail(error))?;
        // Never trust paths inside an archive.
        let Some(name) = entry.enclosed_name() else {
            return Err(AppError::new(ErrorCode::InvalidInput)
                .with("reason", "not_a_transfer")
                .detail(format!("unsafe path in archive: {}", entry.name())));
        };
        let name = name.to_string_lossy().replace('\\', "/");
        if name == MANIFEST_FILE {
            continue;
        }
        let target = paths::resolve_inside(into, &name)?;
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Staging is always fresh, so a file that already exists is a
        // collision: two names that differ only by case on this disk.
        let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                AppError::new(ErrorCode::InvalidInput).with("reason", "transfer_name_collision").with("path", &name)
            } else {
                error.into()
            }
        })?;
        let mut out = std::io::BufWriter::new(file);
        copy_metered(&mut entry, &mut out, &mut meter)?;
        out.flush()?;
    }
    meter.finish();
    Ok(())
}

/// A folder name under `parent` that is not taken yet.
fn free_folder(parent: &Path, base: &str) -> PathBuf {
    let first = parent.join(base);
    if !first.exists() {
        return first;
    }
    (2..1000).map(|n| parent.join(format!("{base}-{n}"))).find(|p| !p.exists()).unwrap_or(first)
}

// -- the service -------------------------------------------------------------

impl InstanceService {
    /// Where exports wait (overridable so tests never touch the real folders).
    fn exports_root(&self) -> AppResult<PathBuf> {
        match self.data_root_override() {
            Some(root) => {
                let dir = root.join("exports");
                std::fs::create_dir_all(&dir)?;
                Ok(dir)
            }
            None => exports_dir(),
        }
    }

    /// Where adopted servers go: the instances folder from settings.
    fn instances_root(&self) -> AppResult<PathBuf> {
        let dir = match self.data_root_override() {
            Some(root) => root.join("instances"),
            None => settings::load()?.instances_dir()?,
        };
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn transfer_event(&self, instance_id: &str, progress: TransferProgress) {
        self.emit(AppEvent::Transfer { instance_id: instance_id.to_string(), progress });
    }

    /// A progress callback for blocking code, tagged with this transfer.
    fn transfer_reporter(&self, instance_id: &str, transfer_id: &str, phase: TransferPhase) -> impl Fn(u64, u64) + Send + Sync + 'static {
        let events = self.events.clone();
        let instance_id = instance_id.to_string();
        let transfer_id = transfer_id.to_string();
        move |bytes, total| {
            let _ = events.send(AppEvent::Transfer {
                instance_id: instance_id.clone(),
                progress: TransferProgress { transfer_id: transfer_id.clone(), phase, bytes, total, done: false, failed: false },
            });
        }
    }

    /// Refuse to run a copy that is not the one to play: an older copy left
    /// behind by a transfer, or one being exported right now.
    pub(crate) async fn ensure_playable(&self, instance: &Instance) -> AppResult<()> {
        if let TransferState::TransferredOut { to_node_name, .. } = &instance.transfer {
            return Err(AppError::new(ErrorCode::InvalidInput)
                .with("reason", "transferred_out")
                .with("node", to_node_name));
        }
        self.ensure_not_transferring(&instance.id).await
    }

    /// Refuse to change a server that is being packed for a transfer, or
    /// that just arrived and waits for the source to confirm: the change
    /// would be missing from the copy that left, or thrown away on undo.
    pub(crate) async fn ensure_not_transferring(&self, instance_id: &str) -> AppResult<()> {
        let busy = self.transfer_locks.lock().await.contains_key(instance_id)
            || self.adoptions.lock().await.contains_key(instance_id);
        if busy {
            return Err(AppError::new(ErrorCode::NodeBusy).with("reason", "transferring"));
        }
        Ok(())
    }

    /// Step 1 (source): stop, lock and pack the server.
    pub async fn transfer_export(
        self: &Arc<Self>,
        instance_id: &str,
        transfer_id: &str,
        allow_players: bool,
    ) -> AppResult<ExportInfo> {
        let instance = self.store.get(instance_id).await?;
        if instance.is_transferred_out() {
            // Only the copy that is in play can move; an older one would
            // overwrite the newer one wherever it lands.
            return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "transferred_out"));
        }
        if self.adoptions.lock().await.contains_key(instance_id) {
            // Arrived here and not confirmed by its source yet.
            return Err(AppError::new(ErrorCode::NodeBusy).with("reason", "transferring"));
        }
        let root = instance.path.clone();
        tokio::task::spawn_blocking(move || preflight(&root)).await.map_err(AppError::unexpected)??;
        let players = self.online_players(instance_id).await.len();
        if players > 0 && !allow_players {
            return Err(AppError::new(ErrorCode::InvalidInput)
                .with("reason", "players_online")
                .with("count", players));
        }

        let export_id = uuid::Uuid::new_v4().to_string();
        let prior_status = self.state_of(instance_id).await.status;
        {
            let mut locks = self.transfer_locks.lock().await;
            if locks.contains_key(instance_id) {
                return Err(AppError::new(ErrorCode::NodeBusy).with("reason", "transferring"));
            }
            locks.insert(
                instance_id.to_string(),
                ExportLock { export_id: export_id.clone(), transfer_id: transfer_id.to_string(), prior_status },
            );
        }

        // On its own task: when the caller goes away mid-pack (the desktop
        // disconnected), packing still ends and a failure still unlocks.
        let service = self.clone();
        let transfer_id = transfer_id.to_string();
        tokio::spawn(async move {
            let result = service.export_locked(&instance, &export_id, &transfer_id).await;
            if result.is_err() {
                service.release_export(&instance.id, &export_id).await;
            }
            result
        })
        .await
        .map_err(AppError::unexpected)?
    }

    async fn export_locked(self: &Arc<Self>, instance: &Instance, export_id: &str, transfer_id: &str) -> AppResult<ExportInfo> {
        let instance_id = instance.id.as_str();
        self.transfer_event(
            instance_id,
            TransferProgress { transfer_id: transfer_id.into(), phase: TransferPhase::Prepare, bytes: 0, total: 0, done: false, failed: false },
        );

        // A consistent copy needs a stopped server: no half-written region.
        if self.processes.is_running(instance_id).await {
            self.stop(instance_id).await?;
            let deadline = Instant::now() + STOP_WAIT;
            while self.processes.is_running(instance_id).await {
                if Instant::now() > deadline {
                    return Err(AppError::new(ErrorCode::InstanceStopTimedOut).with("instance", instance_id));
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        // The old machine must not keep answering for the server.
        self.wake.stop(instance_id).await;
        self.forwarder.release(instance_id).await;
        if self.state_of(instance_id).await.status != InstanceStatus::Offline {
            self.set_status(instance_id, InstanceStatus::Offline).await;
        }
        // Queued addon changes live outside the folder: put them in first.
        if let Err(error) = self.apply_pending_addons(instance_id).await {
            warn!(instance_id, %error, "pending addon changes were not applied before export");
        }

        let node = self.node().await;
        let exported_at = Utc::now();
        let instance = self.store.get(instance_id).await?;
        let manifest = ArchiveManifest {
            format: ARCHIVE_FORMAT,
            instance: instance.clone(),
            source_node_id: node.id.clone(),
            source_node_name: node.name.clone(),
            exported_at,
        };

        let dir = self.exports_root()?;
        let archive = dir.join(format!("{export_id}.zip"));
        let root = instance.path.clone();
        let export_report = self.transfer_reporter(instance_id, transfer_id, TransferPhase::Export);
        // Hashing is the last part of packing, as far as the progress shows.
        let verify_report = self.transfer_reporter(instance_id, transfer_id, TransferPhase::Export);
        let target = archive.clone();
        let packed = tokio::task::spawn_blocking(move || {
            clean_stale_exports(&dir);
            let size = write_archive(&root, &target, &manifest, &export_report)?;
            let sha = sha256_file(&target, &verify_report)?;
            let print = fingerprint(&root, &manifest.instance)?;
            Ok::<_, AppError>((size, sha, print))
        })
        .await
        .map_err(AppError::unexpected)?;
        let (size_bytes, sha256, print) = match packed {
            Ok(done) => done,
            Err(error) => {
                remove_export(&archive);
                return Err(error);
            }
        };
        std::fs::write(
            sidecar_path(&archive),
            serde_json::to_vec(&ExportSidecar {
                instance_id: instance_id.to_string(),
                exported_at,
                fingerprint: Some(print),
            })?,
        )?;

        info!(instance_id, export_id, size_bytes, "server exported for transfer");
        Ok(ExportInfo {
            export_id: export_id.to_string(),
            instance_id: instance_id.to_string(),
            name: instance.name.clone(),
            size_bytes,
            sha256,
            generation: instance.generation,
            source_node_id: node.id,
            source_node_name: node.name,
            exported_at,
        })
    }

    /// Unlock after an export, putting a server that was asleep back to sleep.
    async fn release_export(self: &Arc<Self>, instance_id: &str, export_id: &str) -> Option<ExportLock> {
        let lock = {
            let mut locks = self.transfer_locks.lock().await;
            match locks.get(instance_id) {
                Some(lock) if lock.export_id == export_id => locks.remove(instance_id),
                _ => None,
            }
        };
        if let Ok(path) = self.exports_root().map(|dir| dir.join(format!("{export_id}.zip"))) {
            remove_export(&path);
        }
        lock
    }

    /// Called off before finalize: the source stays authoritative, as it was.
    pub async fn transfer_abort(self: &Arc<Self>, instance_id: &str, export_id: &str) -> AppResult<()> {
        if uuid::Uuid::parse_str(export_id).is_err() {
            return Err(AppError::new(ErrorCode::InvalidInput).with("field", "exportId"));
        }
        let lock = self.release_export(instance_id, export_id).await;
        self.after_abort(instance_id, export_id, lock).await
    }

    /// Call off an export the desktop never heard back about: it knows only
    /// its own transfer id. Nothing to do when no such export is held.
    pub async fn transfer_abort_by_transfer(self: &Arc<Self>, instance_id: &str, transfer_id: &str) -> AppResult<()> {
        if transfer_id.is_empty() {
            return Err(AppError::new(ErrorCode::InvalidInput).with("field", "transferId"));
        }
        let export_id = match self.transfer_locks.lock().await.get(instance_id) {
            Some(lock) if lock.transfer_id == transfer_id => lock.export_id.clone(),
            _ => return Ok(()),
        };
        let lock = self.release_export(instance_id, &export_id).await;
        self.after_abort(instance_id, &export_id, lock).await
    }

    async fn after_abort(self: &Arc<Self>, instance_id: &str, export_id: &str, lock: Option<ExportLock>) -> AppResult<()> {
        let instance = self.store.get(instance_id).await?;
        let was_asleep = lock.is_some_and(|lock| lock.prior_status == InstanceStatus::Sleeping);
        if was_asleep && instance.automation.wake_on_join && !instance.is_transferred_out() {
            if let Err(error) = self.sleep(instance_id).await {
                warn!(instance_id, %error, "could not put the server back to sleep after a cancelled transfer");
            }
        }
        info!(instance_id, export_id, "transfer called off; this copy stays the server");
        Ok(())
    }

    /// Step 3 (source): the target has the server; retire this copy.
    ///
    /// Safe to call again: a copy already retired toward the same machine
    /// and generation (or already deleted, in delete mode) answers yes, so
    /// a desktop that lost the first answer can simply ask once more. Once
    /// the copy is marked as the older one this never fails: deleting its
    /// files is best effort, and a copy that could not be deleted stays as
    /// the older copy.
    pub async fn transfer_finalize(
        self: &Arc<Self>,
        instance_id: &str,
        export_id: &str,
        mode: FinalizeMode,
        to_node_id: &str,
        to_node_name: &str,
        generation: u64,
    ) -> AppResult<()> {
        match self.store.get(instance_id).await {
            Ok(instance) => {
                if let TransferState::TransferredOut { to_node_id: to, generation: at, .. } = &instance.transfer {
                    if to == to_node_id && *at == generation {
                        if mode == FinalizeMode::Delete {
                            self.delete_retired(instance_id).await;
                        }
                        return Ok(());
                    }
                }
            }
            Err(_) if mode == FinalizeMode::Delete => return Ok(()),
            Err(error) => return Err(error),
        }

        let archive = export_path_in(&self.exports_root()?, export_id)?;
        let sidecar: ExportSidecar = serde_json::from_slice(&std::fs::read(sidecar_path(&archive))?)?;
        if sidecar.instance_id != instance_id {
            return Err(AppError::new(ErrorCode::InvalidInput).with("field", "exportId"));
        }
        let instance = self.store.get(instance_id).await?;
        // Played after the copy left (the lock is gone after a restart): this
        // copy is the newer one, so it must not be retired.
        let started_since = instance.last_started_at.is_some_and(|at| at > sidecar.exported_at);
        if started_since || self.processes.is_running(instance_id).await {
            return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "started_during_transfer"));
        }
        // Changed after packing (a file edit, a restore, a setting): those
        // changes exist only here.
        if let Some(expected) = sidecar.fingerprint.clone() {
            let (root, record) = (instance.path.clone(), instance.clone());
            let now = tokio::task::spawn_blocking(move || fingerprint(&root, &record))
                .await
                .map_err(AppError::unexpected)??;
            if now != expected {
                return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "changed_during_transfer"));
            }
        }

        // The commit point: from here on this copy is the older one.
        self.store
            .update(instance_id, |instance| {
                instance.transfer = TransferState::TransferredOut {
                    to_node_id: to_node_id.to_string(),
                    to_node_name: to_node_name.to_string(),
                    at: Utc::now(),
                    generation,
                };
            })
            .await?;
        self.transfer_locks.lock().await.remove(instance_id);
        remove_export(&archive);
        // An older copy must not keep answering for the server (a node that
        // restarted mid-transfer may have put it back to sleep).
        self.wake.stop(instance_id).await;
        self.forwarder.release(instance_id).await;
        self.set_status(instance_id, InstanceStatus::Offline).await;

        if mode == FinalizeMode::Delete {
            self.delete_retired(instance_id).await;
        }
        info!(instance_id, ?mode, to_node_id, "transfer finalized on the source");
        Ok(())
    }

    /// Delete a copy already marked as the older one. A failure (a file held
    /// open) leaves it listed as the older copy, to delete later.
    async fn delete_retired(self: &Arc<Self>, instance_id: &str) {
        if let Err(error) = self.delete_unchecked(instance_id, true).await {
            warn!(instance_id, %error, "older copy could not be deleted after the transfer; it stays as the older copy");
        }
    }

    /// Step 2 (target): verify, unpack and register an incoming server.
    pub async fn transfer_adopt(
        self: &Arc<Self>,
        archive: PathBuf,
        expected_sha256: &str,
        keep_both: bool,
        transfer_id: &str,
    ) -> AppResult<AdoptReport> {
        let result = self.adopt_inner(&archive, expected_sha256, keep_both, transfer_id).await;
        // The upload has done its job either way.
        let _ = std::fs::remove_file(&archive);
        result
    }

    async fn adopt_inner(
        self: &Arc<Self>,
        archive: &Path,
        expected_sha256: &str,
        keep_both: bool,
        transfer_id: &str,
    ) -> AppResult<AdoptReport> {
        let path = archive.to_path_buf();
        let manifest = tokio::task::spawn_blocking(move || read_manifest(&path)).await.map_err(AppError::unexpected)??;
        let incoming_id = manifest.instance.id.clone();

        let verify_report = self.transfer_reporter(&incoming_id, transfer_id, TransferPhase::Verify);
        let path = archive.to_path_buf();
        let actual = tokio::task::spawn_blocking(move || sha256_file(&path, &verify_report))
            .await
            .map_err(AppError::unexpected)??;
        if !actual.eq_ignore_ascii_case(expected_sha256.trim()) {
            return Err(AppError::new(ErrorCode::ChecksumMismatch).with("reason", "transfer").detail(format!(
                "expected {expected_sha256}, got {actual}"
            )));
        }

        let game_manifest = self.registry.manifest(&manifest.instance.game)?;
        let existing = self.store.get(&incoming_id).await.ok();
        let decision = decide(existing.as_ref(), manifest.instance.generation, keep_both).map_err(|reason| {
            AppError::new(ErrorCode::InvalidInput)
                .with("reason", reason)
                .with("node", &manifest.source_node_name)
        })?;
        if let (AdoptDecision::ReplaceOlder, Some(here)) = (decision, existing.as_ref()) {
            if self.processes.is_running(&here.id).await {
                return Err(AppError::new(ErrorCode::InstanceAlreadyRunning).with("instance", &here.id));
            }
            self.wake.stop(&here.id).await;
        }

        // Where the copy lives here.
        let instances_dir = self.instances_root()?;
        let destination = match (decision, existing.as_ref()) {
            (AdoptDecision::ReplaceOlder, Some(here)) => here.path.clone(),
            _ => free_folder(&instances_dir, &crate::service::instances::slug(&manifest.instance.name)),
        };
        let parent = destination.parent().map(Path::to_path_buf).unwrap_or_else(|| instances_dir.clone());
        let staging = parent.join(format!(".nebula-incoming-{}", uuid::Uuid::new_v4().simple()));

        let adopt_report = self.transfer_reporter(&incoming_id, transfer_id, TransferPhase::Adopt);
        let (from, into) = (archive.to_path_buf(), staging.clone());
        let unpacked = tokio::task::spawn_blocking(move || extract_archive(&from, &into, &adopt_report))
            .await
            .map_err(AppError::unexpected)?;
        if let Err(error) = unpacked {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(error);
        }

        // Swap into place. The replaced copy is set aside, not deleted, until
        // the source confirms.
        let mut replaced = None;
        if let (AdoptDecision::ReplaceOlder, Some(here)) = (decision, existing.as_ref()) {
            let aside = parent.join(format!(".nebula-replaced-{}-{}", here.id, here.generation));
            let _ = std::fs::remove_dir_all(&aside);
            if let Err(error) = std::fs::rename(&here.path, &aside) {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(error.into());
            }
            replaced = Some((aside, here.clone()));
        }
        if let Err(error) = std::fs::rename(&staging, &destination) {
            if let Some((aside, here)) = &replaced {
                let _ = std::fs::rename(aside, &here.path);
            }
            let _ = std::fs::remove_dir_all(&staging);
            return Err(error.into());
        }

        let node = self.node().await;
        let fresh_id = decision == AdoptDecision::NewCopy;
        let (mut record, runtime_path_cleared) =
            adopted_record(manifest.instance.clone(), &node.id, destination.clone(), fresh_id);
        if fresh_id {
            let name: String = format!("{} ({})", record.name, manifest.source_node_name).chars().take(64).collect();
            record.name = crate::service::instances::validate_name(&name).unwrap_or(record.name);
        }

        // A port another server here (or anything else) holds is not used.
        let mut port_changed = None;
        if let Some(spec) = game_manifest.primary_port() {
            let wanted = record.primary_port(&game_manifest);
            let claimed: Vec<u16> = self
                .store
                .list()
                .await
                .iter()
                .filter(|other| other.id != record.id)
                .map(|other| other.primary_port(&game_manifest))
                .collect();
            let port = pick_port(wanted, &claimed, ports::is_free);
            if port != wanted {
                record.ports.insert(spec.key.clone(), port);
                if let Err(error) = crate::service::properties::set_internal(&destination, &spec.key, &port.to_string()) {
                    warn!(%error, "port not written to the game's settings after transfer");
                }
                port_changed = Some(PortChange { from: wanted, to: port });
            }
        }

        let java_missing = self.missing_java(&record).await;

        if let Err(error) = self.store.put(record.clone()).await {
            // Put things back exactly as they were.
            let _ = std::fs::remove_dir_all(&destination);
            if let Some((aside, here)) = &replaced {
                let _ = std::fs::rename(aside, &here.path);
            }
            return Err(error);
        }
        self.after_folder_change(&record.id).await;
        self.adoptions
            .lock()
            .await
            .insert(record.id.clone(), Adoption { replaced, transfer_id: transfer_id.to_string() });

        info!(instance_id = %record.id, generation = record.generation, from = %manifest.source_node_name, "server adopted");
        Ok(AdoptReport {
            instance: record,
            node_id: node.id,
            node_name: node.name,
            port_changed,
            java_missing,
            replaced_older_copy: decision == AdoptDecision::ReplaceOlder,
            runtime_path_cleared,
            kept_both: fresh_id,
        })
    }

    /// The Java major the server needs, when this machine has none for it.
    async fn missing_java(&self, instance: &Instance) -> Option<u32> {
        let manifest = self.registry.manifest(&instance.game).ok()?;
        if !matches!(manifest.runtime, RuntimeSpec::Java { .. }) {
            return None;
        }
        let major = self
            .registry
            .get(&instance.game)
            .ok()?
            .runtime_major(&instance.build)
            .or(instance.install.as_ref().and_then(|report| report.java_major))?;
        let found = tokio::task::spawn_blocking(move || {
            crate::runtime::java::pick_for_major(&crate::runtime::java::detect_installations(false), major).is_some()
                || crate::runtime::java::resolve_java(None, major).is_ok_and(|path| path.is_absolute())
        })
        .await
        .unwrap_or(true);
        (!found).then_some(major)
    }

    /// Step 4 (target): the source confirmed; forget what was replaced.
    pub async fn transfer_commit(self: &Arc<Self>, instance_id: &str) -> AppResult<()> {
        let adoption = self.adoptions.lock().await.remove(instance_id);
        if let Some(Adoption { replaced: Some((aside, _)), .. }) = adoption {
            let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(aside)).await;
        }
        let instance = self.store.get(instance_id).await?;
        let wakeable = instance.automation.wake_on_join
            && instance.install.is_some()
            && self.registry.manifest(&instance.game).is_ok_and(|m| m.wake.is_some());
        if wakeable {
            if let Err(error) = self.sleep(instance_id).await {
                warn!(instance_id, %error, "transferred server could not be put to sleep");
            }
        }
        Ok(())
    }

    /// The source did not confirm: undo the adoption, putting back whatever
    /// older copy it replaced. The adoption is forgotten only once undone, so
    /// a refusal (the copy was started) can be retried.
    pub async fn transfer_discard(self: &Arc<Self>, instance_id: &str) -> AppResult<()> {
        let Some(adoption) = self.adoptions.lock().await.get(instance_id).cloned() else {
            return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "no_adoption"));
        };
        if self.processes.is_running(instance_id).await {
            return Err(AppError::new(ErrorCode::InstanceAlreadyRunning).with("instance", instance_id));
        }
        match adoption.replaced {
            None => self.delete_unchecked(instance_id, true).await?,
            Some((aside, previous)) => {
                let adopted = self.store.get(instance_id).await?;
                self.wake.stop(instance_id).await;
                std::fs::remove_dir_all(&adopted.path)?;
                std::fs::rename(&aside, &previous.path)?;
                self.store.put(previous).await?;
                self.after_folder_change(instance_id).await;
            }
        }
        self.adoptions.lock().await.remove(instance_id);
        info!(instance_id, "adoption undone");
        Ok(())
    }

    /// Undo the adoption a transfer made, when the desktop never learned the
    /// adopted id (a lost answer, or one adopted beside a diverged copy).
    /// Nothing to do when this machine adopted nothing for it.
    pub async fn transfer_discard_by_transfer(self: &Arc<Self>, transfer_id: &str) -> AppResult<()> {
        if transfer_id.is_empty() {
            return Err(AppError::new(ErrorCode::InvalidInput).with("field", "transferId"));
        }
        let found = self
            .adoptions
            .lock()
            .await
            .iter()
            .find(|(_, adoption)| adoption.transfer_id == transfer_id)
            .map(|(id, _)| id.clone());
        match found {
            Some(instance_id) => self.transfer_discard(&instance_id).await,
            None => Ok(()),
        }
    }

    /// Play an older copy anyway. It forks from the copy that left.
    pub async fn force_play(&self, instance_id: &str) -> AppResult<Instance> {
        let mut instance = self.store.get(instance_id).await?;
        fork(&mut instance, Utc::now())?;
        let updated = instance.clone();
        let result = self
            .store
            .update(instance_id, move |stored| {
                stored.generation = updated.generation;
                stored.transfer = updated.transfer.clone();
                stored.forked_from = updated.forked_from.clone();
            })
            .await?;
        warn!(instance_id, generation = result.generation, "older copy forced into play: copies have diverged");
        Ok(result)
    }

    /// Tell listeners a transfer this machine ran is over.
    pub fn transfer_finished(&self, instance_id: &str, transfer_id: &str, failed: bool) {
        self.transfer_event(
            instance_id,
            TransferProgress { transfer_id: transfer_id.into(), phase: TransferPhase::Finalize, bytes: 0, total: 0, done: true, failed },
        );
    }

    /// Report bytes moved by the desktop that runs the transfer.
    pub fn transfer_progress(&self, instance_id: &str, transfer_id: &str, phase: TransferPhase, bytes: u64, total: u64) {
        self.transfer_event(
            instance_id,
            TransferProgress { transfer_id: transfer_id.into(), phase, bytes, total, done: false, failed: false },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::BuildSelector;

    fn instance(generation: u64, transfer: TransferState) -> Instance {
        let mut instance = Instance::new(
            "node-a",
            crate::game::MINECRAFT,
            "Survival",
            BuildSelector::new("paper", "1.21.4"),
            PathBuf::from("/srv/survival"),
        );
        instance.generation = generation;
        instance.transfer = transfer;
        instance
    }

    fn moved_to(generation: u64) -> TransferState {
        TransferState::TransferredOut {
            to_node_id: "node-b".into(),
            to_node_name: "Tower".into(),
            at: Utc::now(),
            generation,
        }
    }

    #[test]
    fn a_new_server_is_adopted_as_new() {
        assert_eq!(decide(None, 0, false), Ok(AdoptDecision::New));
    }

    #[test]
    fn a_newer_copy_replaces_the_older_one_left_behind() {
        let older = instance(0, moved_to(1));
        assert_eq!(decide(Some(&older), 1, false), Ok(AdoptDecision::ReplaceOlder));
        assert_eq!(decide(Some(&older), 5, true), Ok(AdoptDecision::ReplaceOlder));
    }

    #[test]
    fn a_copy_that_is_not_newer_is_refused() {
        let older = instance(3, moved_to(4));
        assert_eq!(decide(Some(&older), 3, false), Err("transfer_not_newer"));
        assert_eq!(decide(Some(&older), 2, false), Err("transfer_not_newer"));
        assert_eq!(decide(Some(&older), 2, true), Ok(AdoptDecision::NewCopy));
    }

    #[test]
    fn an_active_copy_is_never_overwritten() {
        let active = instance(1, TransferState::Active);
        assert_eq!(decide(Some(&active), 9, false), Err("transfer_diverged"));
        assert_eq!(decide(Some(&active), 9, true), Ok(AdoptDecision::NewCopy));
    }

    #[test]
    fn adopting_takes_the_next_generation_and_drops_machine_paths() {
        let mut incoming = instance(4, TransferState::Active);
        incoming.resources.runtime_path = Some("C:/Java/bin/java.exe".into());
        let id = incoming.id.clone();
        let (record, cleared) = adopted_record(incoming, "node-b", PathBuf::from("/data/survival"), false);
        assert_eq!(record.generation, 5);
        assert_eq!(record.id, id);
        assert_eq!(record.node_id, "node-b");
        assert_eq!(record.path, PathBuf::from("/data/survival"));
        assert_eq!(record.transfer, TransferState::Active);
        assert!(cleared && record.resources.runtime_path.is_none());

        let (copy, _) = adopted_record(instance(0, TransferState::Active), "node-b", PathBuf::from("/x"), true);
        assert_ne!(copy.id, id, "a side-by-side copy gets its own id");
    }

    #[test]
    fn forcing_an_older_copy_forks_it_past_the_newer_one() {
        let mut older = instance(2, moved_to(3));
        fork(&mut older, Utc::now()).expect("fork");
        assert_eq!(older.transfer, TransferState::Active);
        assert_eq!(older.generation, 4, "past the generation that left");
        let fork_info = older.forked_from.clone().expect("fork recorded");
        assert_eq!(fork_info.node_id, "node-b");
        assert_eq!(fork_info.generation, 3);
        // The fork and the copy it split from have diverged.
        assert_eq!(decide(Some(&older), 9, false), Err("transfer_diverged"));

        let mut active = instance(0, TransferState::Active);
        assert!(fork(&mut active, Utc::now()).is_err(), "only an older copy can be forced");
    }

    #[test]
    fn a_taken_port_moves_to_the_next_free_one() {
        assert_eq!(pick_port(25565, &[], |_| true), 25565);
        assert_eq!(pick_port(25565, &[25565, 25566], |_| true), 25567);
        assert_eq!(pick_port(25565, &[], |p| p != 25565), 25566);
    }

    fn temp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nebula-transfer-{label}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).expect("temp");
        dir
    }

    fn manifest_for(instance: &Instance) -> ArchiveManifest {
        ArchiveManifest {
            format: ARCHIVE_FORMAT,
            instance: instance.clone(),
            source_node_id: "node-a".into(),
            source_node_name: "Pi".into(),
            exported_at: Utc::now(),
        }
    }

    #[test]
    fn an_archive_round_trips_the_whole_folder_and_its_record() {
        let root = temp("src");
        std::fs::create_dir_all(root.join("world/region")).expect("tree");
        std::fs::create_dir_all(root.join("empty")).expect("empty dir");
        std::fs::create_dir_all(root.join(".nebula-import-abc")).expect("scratch");
        std::fs::write(root.join("world/region/r.0.0.mca"), vec![7u8; 300_000]).expect("region");
        std::fs::write(root.join("server.properties"), "server-port=25565\n").expect("props");
        std::fs::write(root.join(INSTANCE_FILE), "{}").expect("record");
        std::fs::write(root.join(".nebula-import-abc/x"), "junk").expect("junk");

        let source = instance(2, TransferState::Active);
        let out = temp("archive");
        let archive = out.join("export.zip");
        let size = write_archive(&root, &archive, &manifest_for(&source), &|_, _| {}).expect("archive");
        assert!(size > 0);

        let manifest = read_manifest(&archive).expect("manifest");
        assert_eq!(manifest.instance.id, source.id);
        assert_eq!(manifest.instance.generation, 2);

        let into = temp("dst").join("adopted");
        extract_archive(&archive, &into, &|_, _| {}).expect("extract");
        assert_eq!(std::fs::read(into.join("world/region/r.0.0.mca")).expect("region").len(), 300_000);
        assert!(into.join("empty").is_dir(), "empty folders survive");
        assert!(!into.join(INSTANCE_FILE).exists(), "the record travels in the manifest");
        assert!(!into.join(MANIFEST_FILE).exists());
        assert!(!into.join(".nebula-import-abc").exists(), "scratch folders stay behind");

        for dir in [root, out, into] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn sha256_detects_a_single_changed_byte() {
        let dir = temp("sha");
        let file = dir.join("a.bin");
        std::fs::write(&file, b"hello").expect("write");
        let first = sha256_file(&file, &|_, _| {}).expect("hash");
        assert_eq!(first, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        std::fs::write(&file, b"hellp").expect("write");
        assert_ne!(sha256_file(&file, &|_, _| {}).expect("hash"), first);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn something_that_is_not_a_transfer_is_refused() {
        let dir = temp("bogus");
        let file = dir.join("x.zip");
        std::fs::write(&file, b"not a zip").expect("write");
        let error = read_manifest(&file).expect_err("refused");
        assert_eq!(error.params.get("reason").map(String::as_str), Some("not_a_transfer"));
        let _ = std::fs::remove_dir_all(dir);
    }

    // -- two machines in one process ---------------------------------------

    struct Machine {
        root: PathBuf,
        service: Arc<InstanceService>,
    }

    async fn machine(name: &str) -> Machine {
        let root = temp(name);
        let store = crate::store::InstanceStore::load_from(root.join("instances.json")).await.expect("store");
        let service = InstanceService::with_store(crate::model::Node::local(name), store).await.expect("service");
        service.set_data_root(root.clone());
        Machine { root, service }
    }

    /// What the desktop does between export and adopt: move the bytes.
    fn ship(from: &Machine, export: &ExportInfo, to: &Machine) -> PathBuf {
        let archive = from.root.join("exports").join(format!("{}.zip", export.export_id));
        let upload = to.root.join(format!("upload-{}.zip", uuid::Uuid::new_v4().simple()));
        std::fs::copy(archive, &upload).expect("ship");
        upload
    }

    async fn server_on(machine: &Machine, world: &str) -> Instance {
        let node = machine.service.node().await;
        let dir = machine.root.join("instances").join("survival");
        std::fs::create_dir_all(dir.join("world")).expect("world dir");
        std::fs::write(dir.join("world/level.dat"), world).expect("level");
        let instance = Instance::new(
            node.id,
            crate::game::MINECRAFT,
            "Survival",
            BuildSelector::new("vanilla", "1.21.4"),
            dir,
        );
        machine.service.store.put(instance.clone()).await.expect("put");
        instance
    }

    fn level(instance: &Instance) -> String {
        std::fs::read_to_string(instance.path.join("world/level.dat")).expect("level.dat")
    }

    #[tokio::test]
    async fn a_server_moves_away_and_back_without_losing_data() {
        let pi = machine("pi").await;
        let tower = machine("tower").await;
        let tower_node = tower.service.node().await;
        let pi_node = pi.service.node().await;
        let original = server_on(&pi, "day-1").await;
        let id = original.id.clone();

        // Pi to Tower.
        let export = pi.service.transfer_export(&id, "t1", false).await.expect("export");
        assert_eq!(export.generation, 0);
        let locked = pi.service.start(&id).await.expect_err("locked while exporting");
        assert_eq!(locked.code, ErrorCode::NodeBusy);

        let upload = ship(&pi, &export, &tower);
        let adopted = tower.service.transfer_adopt(upload.clone(), &export.sha256, false, "t1").await.expect("adopt");
        assert!(!upload.exists(), "the upload is cleaned up");
        assert_eq!(adopted.instance.generation, 1);
        assert_eq!(adopted.instance.node_id, tower_node.id);
        assert!(adopted.instance.path.starts_with(tower.root.join("instances")));
        assert_eq!(level(&adopted.instance), "day-1");

        pi.service
            .transfer_finalize(&id, &export.export_id, FinalizeMode::Keep, &tower_node.id, &tower_node.name, 1)
            .await
            .expect("finalize");
        tower.service.transfer_commit(&id).await.expect("commit");

        let left_behind = pi.service.store.get(&id).await.expect("older copy kept");
        assert!(left_behind.is_transferred_out());
        assert_eq!(level(&left_behind), "day-1", "files kept as the older copy");
        let refused = pi.service.start(&id).await.expect_err("older copy does not start");
        assert_eq!(refused.params.get("reason").map(String::as_str), Some("transferred_out"));

        // Played on the Tower for a while.
        let on_tower = tower.service.store.get(&id).await.expect("on tower");
        std::fs::write(on_tower.path.join("world/level.dat"), "day-9").expect("play");

        // Tower back to Pi, deleting the Tower's copy: the older copy is replaced.
        let back = tower.service.transfer_export(&id, "t2", false).await.expect("export back");
        assert_eq!(back.generation, 1);
        let upload = ship(&tower, &back, &pi);
        let returned = pi.service.transfer_adopt(upload, &back.sha256, false, "t2").await.expect("adopt back");
        assert!(returned.replaced_older_copy);
        assert_eq!(returned.instance.generation, 2);
        assert_eq!(returned.instance.transfer, TransferState::Active);
        assert_eq!(level(&returned.instance), "day-9", "the newer world won");

        tower.service
            .transfer_finalize(&id, &back.export_id, FinalizeMode::Delete, &pi_node.id, &pi_node.name, 2)
            .await
            .expect("finalize back");
        pi.service.transfer_commit(&id).await.expect("commit back");
        assert!(tower.service.store.get(&id).await.is_err(), "moved and deleted");
        assert!(!on_tower.path.exists());
        let leftovers: Vec<_> = std::fs::read_dir(pi.root.join("instances"))
            .expect("instances")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".nebula-"))
            .collect();
        assert!(leftovers.is_empty(), "no staging or replaced folders left");

        for machine in [pi, tower] {
            let _ = std::fs::remove_dir_all(machine.root);
        }
    }

    #[tokio::test]
    async fn a_failed_confirmation_leaves_the_source_in_charge_and_the_target_as_it_was() {
        let pi = machine("pi-fail").await;
        let tower = machine("tower-fail").await;
        let tower_node = tower.service.node().await;
        let original = server_on(&pi, "v1").await;
        let id = original.id.clone();

        // A first move leaves an older copy on the Pi.
        let first = pi.service.transfer_export(&id, "a", false).await.expect("export");
        tower.service.transfer_adopt(ship(&pi, &first, &tower), &first.sha256, false, "a").await.expect("adopt");
        pi.service
            .transfer_finalize(&id, &first.export_id, FinalizeMode::Keep, &tower_node.id, &tower_node.name, 1)
            .await
            .expect("finalize");
        tower.service.transfer_commit(&id).await.expect("commit");

        let on_tower = tower.service.store.get(&id).await.expect("tower");
        std::fs::write(on_tower.path.join("world/level.dat"), "v2").expect("play");
        let back = tower.service.transfer_export(&id, "b", false).await.expect("export back");

        // A damaged archive is refused and nothing on the Pi changes.
        let damaged = pi.service.transfer_adopt(ship(&tower, &back, &pi), &"0".repeat(64), false, "b").await;
        assert_eq!(damaged.expect_err("refused").code, ErrorCode::ChecksumMismatch);
        assert_eq!(level(&pi.service.store.get(&id).await.expect("pi")), "v1");

        // Adopted, then the Tower is started before it confirms.
        pi.service.transfer_adopt(ship(&tower, &back, &pi), &back.sha256, false, "b").await.expect("adopt");
        tower.service.transfer_locks.lock().await.clear(); // as after a restart
        tower
            .service
            .store
            .update(&id, |instance| instance.last_started_at = Some(Utc::now()))
            .await
            .expect("simulate a start");
        let refused = tower
            .service
            .transfer_finalize(&id, &back.export_id, FinalizeMode::Delete, "pi", "Pi", 3)
            .await
            .expect_err("played since the export");
        assert_eq!(refused.params.get("reason").map(String::as_str), Some("started_during_transfer"));
        assert!(tower.service.store.get(&id).await.is_ok(), "the source keeps its copy");

        // The desktop undoes the adoption: the Pi's older copy is back.
        pi.service.transfer_discard(&id).await.expect("discard");
        let restored = pi.service.store.get(&id).await.expect("restored");
        assert!(restored.is_transferred_out());
        assert_eq!(restored.generation, 0);
        assert_eq!(level(&restored), "v1");
        tower.service.transfer_abort(&id, &back.export_id).await.expect("abort");
        assert!(tower.service.ensure_playable(&tower.service.store.get(&id).await.expect("t")).await.is_ok(), "unlocked");

        for machine in [pi, tower] {
            let _ = std::fs::remove_dir_all(machine.root);
        }
    }

    #[tokio::test]
    async fn adopting_over_a_diverged_copy_is_refused_unless_kept_side_by_side() {
        let pi = machine("pi-fork").await;
        let tower = machine("tower-fork").await;
        let tower_node = tower.service.node().await;
        let id = server_on(&pi, "v1").await.id;

        let export = pi.service.transfer_export(&id, "x", false).await.expect("export");
        tower.service.transfer_adopt(ship(&pi, &export, &tower), &export.sha256, false, "x").await.expect("adopt");
        pi.service
            .transfer_finalize(&id, &export.export_id, FinalizeMode::Keep, &tower_node.id, &tower_node.name, 1)
            .await
            .expect("finalize");
        tower.service.transfer_commit(&id).await.expect("commit");

        // Forced into play on the Pi: a fork.
        let forked = pi.service.force_play(&id).await.expect("force");
        assert_eq!(forked.generation, 2);
        assert_eq!(forked.forked_from.as_ref().map(|f| f.node_id.as_str()), Some(tower_node.id.as_str()));

        let back = tower.service.transfer_export(&id, "y", false).await.expect("export back");
        let refused = pi.service.transfer_adopt(ship(&tower, &back, &pi), &back.sha256, false, "y").await;
        assert_eq!(
            refused.expect_err("diverged").params.get("reason").map(String::as_str),
            Some("transfer_diverged")
        );
        let both = pi.service.transfer_adopt(ship(&tower, &back, &pi), &back.sha256, true, "y").await.expect("keep both");
        assert!(both.kept_both);
        assert_ne!(both.instance.id, id);
        assert_eq!(pi.service.store.list().await.len(), 2);

        for machine in [pi, tower] {
            let _ = std::fs::remove_dir_all(machine.root);
        }
    }
}

#[cfg(test)]
mod review {
    //! Regressions from the adversarial review: each test pins down one path
    //! that used to lose data.
    use super::*;
    use crate::game::BuildSelector;

    struct Machine {
        root: PathBuf,
        service: Arc<InstanceService>,
    }

    fn temp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nebula-review-{label}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).expect("temp");
        dir
    }

    async fn machine(name: &str) -> Machine {
        let root = temp(name);
        let store = crate::store::InstanceStore::load_from(root.join("instances.json")).await.expect("store");
        let service = InstanceService::with_store(crate::model::Node::local(name), store).await.expect("service");
        service.set_data_root(root.clone());
        Machine { root, service }
    }

    fn ship(from: &Machine, export: &ExportInfo, to: &Machine) -> PathBuf {
        let archive = from.root.join("exports").join(format!("{}.zip", export.export_id));
        let upload = to.root.join(format!("upload-{}.zip", uuid::Uuid::new_v4().simple()));
        std::fs::copy(archive, &upload).expect("ship");
        upload
    }

    async fn server_on(machine: &Machine, world: &str) -> Instance {
        let node = machine.service.node().await;
        let dir = machine.root.join("instances").join("survival");
        std::fs::create_dir_all(dir.join("world")).expect("world dir");
        std::fs::write(dir.join("world/level.dat"), world).expect("level");
        let instance = Instance::new(node.id, crate::game::MINECRAFT, "Survival", BuildSelector::new("vanilla", "1.21.4"), dir);
        machine.service.store.put(instance.clone()).await.expect("put");
        instance
    }

    fn reason(error: &AppError) -> Option<&str> {
        error.params.get("reason").map(String::as_str)
    }

    /// A finalize that ran but whose answer was lost is asked again: the
    /// retry must answer yes, so the desktop commits instead of discarding
    /// the only copy left.
    #[tokio::test]
    async fn finalize_can_be_repeated_after_a_lost_answer() {
        for mode in [FinalizeMode::Delete, FinalizeMode::Keep] {
            let pi = machine("rv-pi").await;
            let tower = machine("rv-tower").await;
            let tn = tower.service.node().await;
            let id = server_on(&pi, "only-copy").await.id;

            let export = pi.service.transfer_export(&id, "t", false).await.expect("export");
            tower.service.transfer_adopt(ship(&pi, &export, &tower), &export.sha256, false, "t").await.expect("adopt");
            pi.service
                .transfer_finalize(&id, &export.export_id, mode, &tn.id, &tn.name, 1)
                .await
                .expect("finalize applied");
            pi.service
                .transfer_finalize(&id, &export.export_id, mode, &tn.id, &tn.name, 1)
                .await
                .expect("asking again answers yes");
            tower.service.transfer_commit(&id).await.expect("commit");

            let kept = tower.service.store.get(&id).await.expect("the target has the server");
            assert_eq!(std::fs::read_to_string(kept.path.join("world/level.dat")).expect("lvl"), "only-copy");
            match mode {
                FinalizeMode::Delete => assert!(pi.service.store.get(&id).await.is_err()),
                FinalizeMode::Keep => assert!(pi.service.store.get(&id).await.expect("older").is_transferred_out()),
            }
            // Retiring toward another machine or generation is not a repeat.
            if mode == FinalizeMode::Keep {
                let other = pi.service.transfer_finalize(&id, &export.export_id, mode, "elsewhere", "X", 7).await;
                assert!(other.is_err());
            }
            for m in [pi, tower] {
                let _ = std::fs::remove_dir_all(m.root);
            }
        }
    }

    /// Delete-mode finalize retires the copy before deleting it: a file held
    /// open makes the delete fail, and the copy stays as the older one.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_delete_that_fails_half_way_leaves_the_older_copy() {
        use std::os::windows::fs::OpenOptionsExt;
        let pi = machine("rv-pi2").await;
        let tower = machine("rv-tower2").await;
        let tn = tower.service.node().await;
        let original = server_on(&pi, "only-copy").await;
        let id = original.id.clone();
        std::fs::write(original.path.join("server.log"), "x").expect("log");

        let export = pi.service.transfer_export(&id, "t", false).await.expect("export");
        tower.service.transfer_adopt(ship(&pi, &export, &tower), &export.sha256, false, "t").await.expect("adopt");
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(original.path.join("server.log")).expect("hold");
        pi.service
            .transfer_finalize(&id, &export.export_id, FinalizeMode::Delete, &tn.id, &tn.name, 1)
            .await
            .expect("finalize does not fail after the commit point");
        let left = pi.service.store.get(&id).await.expect("the record stays");
        assert!(left.is_transferred_out(), "as the older copy");
        tower.service.transfer_commit(&id).await.expect("commit");
        assert!(tower.service.store.get(&id).await.is_ok());

        // Once the file is let go, asking again finishes the delete.
        drop(held);
        pi.service
            .transfer_finalize(&id, &export.export_id, FinalizeMode::Delete, &tn.id, &tn.name, 1)
            .await
            .expect("repeat");
        assert!(pi.service.store.get(&id).await.is_err());
        for m in [pi, tower] {
            let _ = std::fs::remove_dir_all(m.root);
        }
    }

    /// The store keeps a record whose folder could not be deleted.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_record_is_kept_when_its_folder_cannot_be_deleted() {
        use std::os::windows::fs::OpenOptionsExt;
        let pi = machine("rv-pi5").await;
        let original = server_on(&pi, "v1").await;
        std::fs::write(original.path.join("server.log"), "x").expect("log");
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(original.path.join("server.log")).expect("hold");
        assert!(pi.service.delete(&original.id, true).await.is_err());
        assert!(pi.service.store.get(&original.id).await.is_ok(), "still listed");
        drop(held);
        pi.service.delete(&original.id, true).await.expect("deleted once let go");
        let _ = std::fs::remove_dir_all(pi.root);
    }

    /// Two entries that differ only by case (legal on the Pi's ext4) are a
    /// collision on Windows: refused, never one silently overwriting the other.
    #[cfg(windows)]
    #[test]
    fn case_colliding_entries_are_refused() {
        let dir = temp("rv-case");
        let archive = dir.join("a.zip");
        let instance = Instance::new("n", crate::game::MINECRAFT, "S", BuildSelector::new("vanilla", "1.21.4"), dir.clone());
        let manifest = ArchiveManifest {
            format: ARCHIVE_FORMAT,
            instance,
            source_node_id: "n".into(),
            source_node_name: "Pi".into(),
            exported_at: Utc::now(),
        };
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).expect("zip"));
            let o = SimpleFileOptions::default();
            zip.start_file(MANIFEST_FILE, o).expect("m");
            zip.write_all(&serde_json::to_vec(&manifest).expect("json")).expect("w");
            zip.start_file("plugins/Config.yml", o).expect("a");
            zip.write_all(b"first").expect("w");
            zip.start_file("plugins/config.yml", o).expect("b");
            zip.write_all(b"second").expect("w");
            zip.finish().expect("finish");
        }
        let error = extract_archive(&archive, &dir.join("out"), &|_, _| {}).expect_err("collision");
        assert_eq!(reason(&error), Some("transfer_name_collision"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The source refuses up front to pack names that would collide.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn the_source_refuses_names_that_differ_only_by_case() {
        let pi = machine("rv-pi6").await;
        let original = server_on(&pi, "v1").await;
        std::fs::create_dir_all(original.path.join("plugins")).expect("dir");
        std::fs::write(original.path.join("plugins/Config.yml"), "a").expect("a");
        std::fs::write(original.path.join("plugins/config.yml"), "b").expect("b");
        let error = pi.service.transfer_export(&original.id, "t", false).await.expect_err("refused");
        assert_eq!(reason(&error), Some("transfer_name_collision"));
        assert!(pi.service.ensure_playable(&original).await.is_ok(), "not left locked");
        let _ = std::fs::remove_dir_all(pi.root);
    }

    /// A linked folder (a world on a USB disk) would be left behind: refused.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_linked_folder_is_refused() {
        let pi = machine("rv-pi7").await;
        let original = server_on(&pi, "v1").await;
        let elsewhere = temp("rv-usb");
        std::os::unix::fs::symlink(&elsewhere, original.path.join("plugins")).expect("link");
        let error = pi.service.transfer_export(&original.id, "t", false).await.expect_err("refused");
        assert_eq!(reason(&error), Some("transfer_symlink"));
        let _ = std::fs::remove_dir_all(pi.root);
        let _ = std::fs::remove_dir_all(elsewhere);
    }

    /// An adoption waiting for its source cannot start, cannot be sent on,
    /// and can still be undone; once committed it plays.
    #[tokio::test]
    async fn a_pending_adoption_is_locked_until_committed() {
        let pi = machine("rv-pi3").await;
        let tower = machine("rv-tower3").await;
        let tn = tower.service.node().await;
        let id = server_on(&pi, "v1").await.id;
        let export = pi.service.transfer_export(&id, "t", false).await.expect("export");
        tower.service.transfer_adopt(ship(&pi, &export, &tower), &export.sha256, false, "t").await.expect("adopt");
        let adopted = tower.service.store.get(&id).await.expect("adopted");
        let locked = tower.service.ensure_playable(&adopted).await.expect_err("locked");
        assert_eq!(locked.code, ErrorCode::NodeBusy);
        let resent = tower.service.transfer_export(&id, "u", false).await.expect_err("not sent on");
        assert_eq!(resent.code, ErrorCode::NodeBusy);

        pi.service
            .transfer_finalize(&id, &export.export_id, FinalizeMode::Keep, &tn.id, &tn.name, 1)
            .await
            .expect("finalize");
        tower.service.transfer_commit(&id).await.expect("commit");
        assert!(tower.service.ensure_playable(&adopted).await.is_ok(), "plays once confirmed");
        for m in [pi, tower] {
            let _ = std::fs::remove_dir_all(m.root);
        }
    }

    /// Changes to a server being packed are refused; a change that slips past
    /// (a file edited by hand) makes finalize refuse, so it is not lost.
    #[tokio::test]
    async fn changes_while_transferring_are_refused_or_caught() {
        let pi = machine("rv-pi4").await;
        let tower = machine("rv-tower4").await;
        let tn = tower.service.node().await;
        let original = server_on(&pi, "v1").await;
        let id = original.id.clone();
        let export = pi.service.transfer_export(&id, "t", false).await.expect("export");

        let patch: crate::service::instances::InstancePatch =
            serde_json::from_value(serde_json::json!({ "name": "Renamed while moving" })).expect("patch");
        let refused = pi.service.apply_patch(&id, patch).await.expect_err("refused while locked");
        assert_eq!(reason(&refused), Some("transferring"));
        assert_eq!(pi.service.delete(&id, true).await.expect_err("no delete").code, ErrorCode::NodeBusy);

        std::fs::write(original.path.join("world/level.dat"), "edited-while-moving").expect("edit");
        tower.service.transfer_adopt(ship(&pi, &export, &tower), &export.sha256, false, "t").await.expect("adopt");
        let error = pi
            .service
            .transfer_finalize(&id, &export.export_id, FinalizeMode::Delete, &tn.id, &tn.name, 1)
            .await
            .expect_err("caught");
        assert_eq!(reason(&error), Some("changed_during_transfer"));
        assert_eq!(std::fs::read_to_string(original.path.join("world/level.dat")).expect("lvl"), "edited-while-moving");

        // What the desktop then does: undo the target, unlock the source.
        tower.service.transfer_discard(&id).await.expect("discard");
        assert!(tower.service.store.get(&id).await.is_err());
        pi.service.transfer_abort(&id, &export.export_id).await.expect("abort");
        assert!(pi.service.ensure_playable(&pi.service.store.get(&id).await.expect("pi")).await.is_ok());
        for m in [pi, tower] {
            let _ = std::fs::remove_dir_all(m.root);
        }
    }

    /// When the desktop never heard back, it undoes by its transfer id.
    #[tokio::test]
    async fn a_lost_answer_is_undone_by_transfer_id() {
        let pi = machine("rv-pi8").await;
        let tower = machine("rv-tower8").await;
        let id = server_on(&pi, "v1").await.id;

        let export = pi.service.transfer_export(&id, "lost-1", false).await.expect("export");
        tower
            .service
            .transfer_adopt(ship(&pi, &export, &tower), &export.sha256, false, "lost-1")
            .await
            .expect("adopt");
        // Unknown transfers are a no-op, not an error.
        tower.service.transfer_discard_by_transfer("someone-else").await.expect("no-op");
        assert!(tower.service.store.get(&id).await.is_ok());
        pi.service.transfer_abort_by_transfer(&id, "someone-else").await.expect("no-op");
        assert!(pi.service.ensure_playable(&pi.service.store.get(&id).await.expect("pi")).await.is_err(), "still locked");

        tower.service.transfer_discard_by_transfer("lost-1").await.expect("discard");
        assert!(tower.service.store.get(&id).await.is_err(), "target undone");
        pi.service.transfer_abort_by_transfer(&id, "lost-1").await.expect("abort");
        assert!(pi.service.ensure_playable(&pi.service.store.get(&id).await.expect("pi")).await.is_ok(), "unlocked");
        assert!(!pi.root.join("exports").join(format!("{}.zip", export.export_id)).exists(), "export removed");
        for m in [pi, tower] {
            let _ = std::fs::remove_dir_all(m.root);
        }
    }
}
