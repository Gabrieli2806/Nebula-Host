//! Which Java a server runs on: what is installed, which one its game
//! version wants, and fetching that one when it is missing.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::manifest::RuntimeSpec;
use crate::runtime::java::{self, JavaInstallation};
use crate::service::{AppEvent, InstanceService};

/// Everything the per-server Java picker shows.
#[derive(Debug, Clone, Serialize)]
pub struct JavaRuntimes {
    /// False for games that do not run on a JVM; the picker hides itself.
    pub applies: bool,
    /// The major this server's game version is meant for.
    pub recommended_major: Option<u32>,
    /// The JVM chosen for this server, when one was (`None` = automatic).
    pub selected: Option<String>,
    /// What "automatic" resolves to right now.
    pub automatic: Option<String>,
    /// Every JVM found on the machine, newest major first.
    pub installations: Vec<JavaInstallation>,
    /// Temurin publishes builds for this OS and CPU.
    pub can_download: bool,
}

impl InstanceService {
    pub async fn java_runtimes(&self, instance_id: &str, refresh: bool) -> AppResult<JavaRuntimes> {
        let instance = self.store.get(instance_id).await?;
        let manifest = self.registry.manifest(&instance.game)?;
        if !matches!(manifest.runtime, RuntimeSpec::Java { .. }) {
            return Ok(JavaRuntimes {
                applies: false,
                recommended_major: None,
                selected: None,
                automatic: None,
                installations: Vec::new(),
                can_download: false,
            });
        }
        let recommended_major = self
            .registry
            .get(&instance.game)?
            .runtime_major(&instance.build)
            .or(instance.install.as_ref().and_then(|report| report.java_major));

        // Detection reads folders and may run `java -version` a few times;
        // keep it off the async workers.
        let (installations, automatic) = tokio::task::spawn_blocking(move || {
            let installations = java::detect_installations(refresh);
            let automatic = recommended_major
                .and_then(|major| java::resolve_java(None, major).ok())
                .filter(|path| path.parent().is_some_and(|parent| !parent.as_os_str().is_empty()) || path.is_file())
                .map(|path| path.display().to_string());
            (installations, automatic)
        })
        .await
        .map_err(AppError::unexpected)?;

        Ok(JavaRuntimes {
            applies: true,
            recommended_major,
            selected: instance.resources.runtime_path.clone(),
            automatic,
            installations,
            can_download: java::can_install(),
        })
    }

    /// Download Temurin `major` for this machine, reporting progress as
    /// `javaDownload` events. One download per major at a time.
    pub async fn java_download(&self, major: u32) -> AppResult<JavaInstallation> {
        if !(8..=99).contains(&major) {
            return Err(AppError::new(ErrorCode::InvalidInput).with("field", "major"));
        }
        let _guard = DownloadGuard::claim(major)?;
        let events = self.events.clone();
        java::download_temurin(major, move |progress| {
            let _ = events.send(AppEvent::JavaDownload { major, progress });
        })
        .await
    }
}

/// Java `major` for a caller that only needs it to exist (the launcher):
/// downloads it under the same per-major lock as `java_download`, or waits
/// for a download already running (a server's, say) and uses its result.
pub async fn ensure_java(major: u32) -> AppResult<JavaInstallation> {
    loop {
        if let Ok(_guard) = DownloadGuard::claim(major) {
            return java::download_temurin(major, |_| {}).await;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let found = tokio::task::spawn_blocking(move || java::resolve_java(None, major).and_then(|path| java::probe(&path)))
            .await
            .map_err(AppError::unexpected)?;
        if let Ok(found) = found {
            if found.major == major {
                return Ok(found);
            }
        }
    }
}

/// The Java majors games are built for, newest first — the slots App
/// Settings shows (as Modrinth does): Minecraft 26.1+ → 25, 1.20.5+ → 21,
/// 1.17–1.20.4 → 17, older → 8.
pub const JAVA_SLOTS: [u32; 4] = [25, 21, 17, 8];

/// One slot in Settings › Java.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaSlot {
    pub major: u32,
    /// The path the user pinned for this major, if any.
    pub configured: Option<String>,
    /// What servers needing this major will actually run on right now.
    pub effective: Option<JavaInstallation>,
    /// True when `effective` was found automatically (nothing pinned).
    pub automatic: bool,
}

/// App-level Java overview: every JVM on the machine and what each slot uses.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaOverview {
    pub slots: Vec<JavaSlot>,
    pub installations: Vec<JavaInstallation>,
    pub can_download: bool,
}

pub async fn java_overview(refresh: bool) -> AppResult<JavaOverview> {
    tokio::task::spawn_blocking(move || {
        let installations = java::detect_installations(refresh);
        let configured = crate::store::settings::load().map(|s| s.java_paths).unwrap_or_default();
        let slots = JAVA_SLOTS
            .iter()
            .map(|&major| {
                let pinned = configured.get(&major.to_string()).map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
                let effective = match &pinned {
                    Some(path) => installations
                        .iter()
                        .find(|i| i.path.eq_ignore_ascii_case(path))
                        .cloned()
                        .or_else(|| java::probe(std::path::Path::new(path)).ok()),
                    None => java::pick_for_major(&installations, major),
                };
                JavaSlot { major, automatic: pinned.is_none(), configured: pinned, effective }
            })
            .collect();
        Ok(JavaOverview { slots, installations, can_download: java::can_install() })
    })
    .await
    .map_err(AppError::unexpected)?
}

fn in_flight() -> &'static Mutex<HashSet<u32>> {
    static SET: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Marks a major as downloading until dropped, so two clicks (or two
/// servers asking for Java 21 at once) do not unpack over each other.
struct DownloadGuard(u32);

impl DownloadGuard {
    fn claim(major: u32) -> AppResult<Self> {
        let mut set = in_flight().lock().map_err(AppError::unexpected)?;
        if !set.insert(major) {
            return Err(AppError::new(ErrorCode::NodeBusy).with("reason", "java_download"));
        }
        Ok(Self(major))
    }
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = in_flight().lock() {
            set.remove(&self.0);
        }
    }
}
