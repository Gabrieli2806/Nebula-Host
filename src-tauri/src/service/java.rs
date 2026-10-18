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
            can_download: java::adoptium_platform().is_some(),
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
