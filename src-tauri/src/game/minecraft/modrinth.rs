//! Modrinth, done properly: hash identification, batch update checks,
//! faceted search, exact-version installs with integrity checks.
//!
//! What the server can run is described by an [`AddonProfile`] — a project
//! type, the loader tags it accepts in priority order, and its exact game
//! version — built from the manifest's distribution. Nothing here asks which
//! distribution it is.
//!
//! Etiquette: every request carries the app's descriptive user agent
//! ([`crate::game::USER_AGENT`]), identical GETs are cached for a few minutes,
//! hash lookups are batched (one request for the whole folder, not one per
//! jar), and a 429 is honoured once using the `X-Ratelimit-Reset` header
//! rather than hammered.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Utc};
use nebula_downloader::{download_file, Checksum, DownloadOptions};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::error::{AppError, AppResult, ErrorCode};

const API: &str = "https://api.modrinth.com/v2";
/// Jars the user disabled keep their bytes and gain this suffix.
pub const DISABLED_SUFFIX: &str = ".disabled";
/// How long an identical GET is answered from memory.
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const CACHE_MAX_ENTRIES: usize = 512;
/// Never wait longer than this for a rate-limit window to reset.
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(10);
/// Suffix an old jar is parked under while its replacement is put in place.
/// The folder scan only reads `.jar` and `.jar.disabled`, so it never shows.
const PARKED_SUFFIX: &str = ".nebula-old";

// ---------------------------------------------------------------------------
// Types the UI sees (snake_case, like the other addon types)
// ---------------------------------------------------------------------------

/// What a server can run, derived from its manifest distribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddonProfile {
    /// `mod` or `plugin`.
    pub project_type: String,
    /// Loader tags in priority order (`["purpur", "paper", "spigot", "bukkit"]`).
    pub loaders: Vec<String>,
    /// Exact game version (`1.21.4`).
    pub game_version: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddonSort {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

impl AddonSort {
    fn index(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Downloads => "downloads",
            Self::Follows => "follows",
            Self::Newest => "newest",
            Self::Updated => "updated",
        }
    }
}

/// One search result. No version is resolved here — that would be one extra
/// request per hit; the install call resolves it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub follows: u64,
    /// Display categories (`utility`, `worldgen`…), loaders removed.
    pub categories: Vec<String>,
    pub date_modified: Option<String>,
    pub project_type: String,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
    /// Already in this server's addon folder (by hash identification).
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonBrowseResult {
    pub hits: Vec<AddonHit>,
    pub total_hits: u64,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonFile {
    pub url: String,
    pub file_name: String,
    pub size: u64,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonDependencyRef {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub file_name: Option<String>,
    /// `required`, `optional`, `incompatible` or `embedded`.
    pub dependency_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonVersion {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    /// `release`, `beta` or `alpha`.
    pub version_type: String,
    pub date_published: Option<String>,
    pub downloads: u64,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    /// The primary file; what an install downloads.
    pub file: AddonFile,
    pub dependencies: Vec<AddonDependencyRef>,
    /// Runs on this server: exact game version and an accepted loader.
    pub compatible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonProjectSummary {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub project_type: String,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
}

/// What Modrinth knows about one jar in the folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModrinthMatch {
    pub project: AddonProjectSummary,
    pub installed_version: AddonVersion,
    /// Newest version for this server's loaders + game version.
    pub latest_version: Option<AddonVersion>,
    pub update_available: bool,
}

/// One jar in the addon folder, identified or not.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledAddonEntry {
    /// Exact file name on disk, including `.disabled` when disabled.
    pub file_name: String,
    /// File name without `.jar` / `.jar.disabled`; the fallback title.
    pub display_name: String,
    pub enabled: bool,
    pub size_bytes: u64,
    pub modified: Option<DateTime<Utc>>,
    pub sha1: String,
    /// Absent for jars Modrinth does not know (CurseForge, custom builds).
    pub modrinth: Option<ModrinthMatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModrinthStatus {
    /// Identification ran.
    Ok,
    /// Modrinth could not be reached; the jars are still listed.
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledAddonsReport {
    pub addons: Vec<InstalledAddonEntry>,
    pub modrinth_status: ModrinthStatus,
    /// Jars Modrinth did not recognise.
    pub unknown_count: u32,
    pub updates_available: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonDependency {
    pub project_id: String,
    pub dependency_type: String,
    pub project: Option<AddonProjectSummary>,
    /// A jar of this project is already in the folder.
    pub installed: bool,
    /// Best compatible version, resolved for `required` and `optional` deps.
    pub version: Option<AddonVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonInstallResult {
    pub file_name: String,
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    /// The file that was removed because this one replaces it.
    pub replaced: Option<String>,
    /// The server was running: the jar is downloaded and waits in the
    /// pending queue, to go in when the server stops.
    #[serde(default)]
    pub pending: bool,
}

// ---------------------------------------------------------------------------
// HTTP with cache and rate-limit manners
// ---------------------------------------------------------------------------

fn cache() -> &'static Mutex<HashMap<String, (Instant, Value)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Value)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get(key: &str) -> Option<Value> {
    let map = cache().lock().ok()?;
    let (at, value) = map.get(key)?;
    (at.elapsed() < CACHE_TTL).then(|| value.clone())
}

fn cache_put(key: String, value: Value) {
    let Ok(mut map) = cache().lock() else { return };
    if map.len() >= CACHE_MAX_ENTRIES {
        map.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
        if map.len() >= CACHE_MAX_ENTRIES {
            if let Some(oldest) = map.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| k.clone()) {
                map.remove(&oldest);
            }
        }
    }
    map.insert(key, (Instant::now(), value));
}

/// Drop every cached answer, e.g. after an install changed what is "latest".
pub fn clear_cache() {
    if let Ok(mut map) = cache().lock() {
        map.clear();
    }
}

pub struct Modrinth {
    client: Client,
}

impl Default for Modrinth {
    fn default() -> Self {
        Self::new()
    }
}

impl Modrinth {
    pub fn new() -> Self {
        Self { client: crate::game::http_client() }
    }

    async fn send(&self, build: impl Fn() -> reqwest::RequestBuilder) -> AppResult<Value> {
        for attempt in 0..2 {
            let response = build().send().await?;
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                // A second 429 in a row is "try again later", not a refusal.
                if attempt > 0 {
                    return Err(AppError::new(ErrorCode::UpstreamUnavailable).detail("modrinth rate limit"));
                }
                let wait = response
                    .headers()
                    .get("x-ratelimit-reset")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(Duration::from_secs(2))
                    .min(MAX_RATE_LIMIT_WAIT);
                tokio::time::sleep(wait).await;
                continue;
            }
            if response.status() == StatusCode::NOT_FOUND {
                return Err(AppError::new(ErrorCode::NotFound).detail(response.url().as_str()));
            }
            return Ok(response.error_for_status()?.json().await?);
        }
        Err(AppError::new(ErrorCode::UpstreamUnavailable).detail("modrinth rate limit"))
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> AppResult<Value> {
        let url = format!("{API}{path}");
        let key = format!("GET {url}?{query:?}");
        if let Some(hit) = cache_get(&key) {
            return Ok(hit);
        }
        let value = self.send(|| self.client.get(&url).query(query)).await?;
        cache_put(key, value.clone());
        Ok(value)
    }

    async fn post(&self, path: &str, body: &Value, cached: bool) -> AppResult<Value> {
        let url = format!("{API}{path}");
        let key = format!("POST {url} {body}");
        if cached {
            if let Some(hit) = cache_get(&key) {
                return Ok(hit);
            }
        }
        let value = self.send(|| self.client.post(&url).json(body)).await?;
        if cached {
            cache_put(key, value.clone());
        }
        Ok(value)
    }

    // -- search ------------------------------------------------------------

    pub async fn search(
        &self,
        profile: &AddonProfile,
        query: &str,
        sort: AddonSort,
        offset: u32,
        limit: u32,
        categories: &[String],
        installed_projects: &HashSet<String>,
    ) -> AppResult<AddonBrowseResult> {
        let limit = limit.clamp(1, 100);
        let facets = serde_json::to_string(&search_facets(profile, categories))?;
        // An empty query sorted by relevance is noise; browsing should
        // surface what people actually use.
        let sort = if query.trim().is_empty() && sort == AddonSort::Relevance {
            AddonSort::Downloads
        } else {
            sort
        };

        let response = self
            .get(
                "/search",
                &[
                    ("query", query.trim().to_string()),
                    ("facets", facets),
                    ("index", sort.index().to_string()),
                    ("offset", offset.to_string()),
                    ("limit", limit.to_string()),
                ],
            )
            .await?;

        let hits = response["hits"]
            .as_array()
            .ok_or_else(|| AppError::new(ErrorCode::UpstreamRejected).detail("no hits array"))?
            .iter()
            .filter_map(|hit| parse_hit(hit, profile, installed_projects))
            .collect();

        Ok(AddonBrowseResult {
            hits,
            total_hits: response["total_hits"].as_u64().unwrap_or(0),
            offset,
            limit,
        })
    }

    // -- versions ----------------------------------------------------------

    /// Versions of a project. With `only_compatible`, filtered to exactly this
    /// server's game version and loaders; newest first either way.
    pub async fn project_versions(
        &self,
        profile: &AddonProfile,
        project_id: &str,
        only_compatible: bool,
    ) -> AppResult<Vec<AddonVersion>> {
        let mut query = vec![("include_changelog", "false".to_string())];
        if only_compatible {
            query.push(("loaders", serde_json::to_string(&profile.loaders)?));
            query.push(("game_versions", serde_json::to_string(&[&profile.game_version])?));
        }
        let response = self.get(&format!("/project/{}/version", path_segment(project_id)?), &query).await?;
        let versions: Vec<AddonVersion> = response
            .as_array()
            .map(|items| items.iter().filter_map(|v| parse_version(v, profile)).collect())
            .unwrap_or_default();
        Ok(if only_compatible { versions.into_iter().filter(|v| v.compatible).collect() } else { versions })
    }

    pub async fn version(&self, profile: &AddonProfile, version_id: &str) -> AppResult<AddonVersion> {
        let value = self.get(&format!("/version/{}", path_segment(version_id)?), &[]).await?;
        parse_version(&value, profile)
            .ok_or_else(|| AppError::new(ErrorCode::UpstreamRejected).detail("unreadable version"))
    }

    pub async fn best_version(&self, profile: &AddonProfile, project_id: &str) -> AppResult<AddonVersion> {
        let versions = self.project_versions(profile, project_id, true).await?;
        best_version(versions, &profile.loaders).ok_or_else(|| {
            AppError::new(ErrorCode::VersionNotFound)
                .with("project", project_id)
                .with("version", &profile.game_version)
        })
    }

    pub async fn projects(&self, ids: &[String]) -> AppResult<Vec<AddonProjectSummary>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let response = self.get("/projects", &[("ids", serde_json::to_string(ids)?)]).await?;
        Ok(response
            .as_array()
            .map(|items| items.iter().filter_map(parse_project).collect())
            .unwrap_or_default())
    }

    // -- identification ----------------------------------------------------

    /// Identify every jar in `dir` by SHA-1 and check each for an update.
    /// Three requests in total, whatever the number of jars.
    pub async fn identify_dir(&self, dir: &Path, profile: &AddonProfile) -> AppResult<InstalledAddonsReport> {
        let local = tokio::task::spawn_blocking({
            let dir = dir.to_path_buf();
            move || scan_dir(&dir)
        })
        .await
        .map_err(AppError::unexpected)??;

        if local.is_empty() {
            return Ok(InstalledAddonsReport {
                addons: Vec::new(),
                modrinth_status: ModrinthStatus::Ok,
                unknown_count: 0,
                updates_available: 0,
            });
        }

        let lookups = self.lookup(&local, profile).await;
        let (known, updates, projects, status) = match lookups {
            Ok((known, updates, projects)) => (known, updates, projects, ModrinthStatus::Ok),
            Err(error) => {
                tracing::warn!(%error, "modrinth identification failed; listing jars without metadata");
                (HashMap::new(), HashMap::new(), Vec::new(), ModrinthStatus::Unavailable)
            }
        };

        Ok(assemble_report(local, &known, &updates, &projects, profile, status))
    }

    async fn lookup(
        &self,
        local: &[LocalJar],
        profile: &AddonProfile,
    ) -> AppResult<(HashMap<String, Value>, HashMap<String, Value>, Vec<AddonProjectSummary>)> {
        let hashes: Vec<&str> = local.iter().map(|jar| jar.sha1.as_str()).collect();

        // A hash names one immutable file, so this answer is cacheable.
        let known = self
            .post("/version_files", &serde_json::json!({ "hashes": hashes, "algorithm": "sha1" }), true)
            .await?;
        let known: HashMap<String, Value> = serde_json::from_value(known).unwrap_or_default();

        let known_hashes: Vec<&String> = known.keys().collect();
        let updates: HashMap<String, Value> = if known_hashes.is_empty() {
            HashMap::new()
        } else {
            let body = serde_json::json!({
                "hashes": known_hashes,
                "algorithm": "sha1",
                "loaders": profile.loaders,
                "game_versions": [profile.game_version],
            });
            // Update checks are not cached for long: "latest" moves.
            match self.post("/version_files/update", &body, false).await {
                Ok(value) => serde_json::from_value(value).unwrap_or_default(),
                Err(error) => {
                    tracing::warn!(%error, "modrinth update check failed");
                    HashMap::new()
                }
            }
        };

        let mut project_ids: Vec<String> = known
            .values()
            .filter_map(|version| version["project_id"].as_str().map(String::from))
            .collect();
        project_ids.sort();
        project_ids.dedup();
        let projects = self.projects(&project_ids).await.unwrap_or_default();

        Ok((known, updates, projects))
    }

    // -- dependencies ------------------------------------------------------

    pub async fn dependencies(
        &self,
        profile: &AddonProfile,
        version: &AddonVersion,
        installed_projects: &HashSet<String>,
    ) -> AppResult<Vec<AddonDependency>> {
        // A dependency may name only a version; resolve those to projects.
        let mut refs: Vec<(String, String)> = Vec::new();
        for dep in &version.dependencies {
            let project_id = match (&dep.project_id, &dep.version_id) {
                (Some(project_id), _) => project_id.clone(),
                (None, Some(version_id)) => match self.version(profile, version_id).await {
                    Ok(resolved) => resolved.project_id,
                    Err(_) => continue,
                },
                (None, None) => continue,
            };
            if !refs.iter().any(|(id, _)| id == &project_id) {
                refs.push((project_id, dep.dependency_type.clone()));
            }
        }

        let ids: Vec<String> = refs.iter().map(|(id, _)| id.clone()).collect();
        let projects = self.projects(&ids).await.unwrap_or_default();

        let mut out = Vec::new();
        for (project_id, dependency_type) in refs {
            let wants_version = matches!(dependency_type.as_str(), "required" | "optional");
            let resolved = if wants_version {
                self.best_version(profile, &project_id).await.ok()
            } else {
                None
            };
            out.push(AddonDependency {
                project: projects.iter().find(|p| p.project_id == project_id).cloned(),
                installed: installed_projects.contains(&project_id),
                version: resolved,
                dependency_type,
                project_id,
            });
        }
        Ok(out)
    }

    // -- installing --------------------------------------------------------

    /// Download a version's primary file into `dir`, verify its SHA-1, and
    /// swap it for `replace` (the old jar). Either the new jar ends up in
    /// place of the old one, or the folder is left as it was.
    pub async fn install(
        &self,
        dir: &Path,
        version: &AddonVersion,
        replace: Option<&str>,
    ) -> AppResult<AddonInstallResult> {
        let file_name = sanitise_file_name(&version.file.file_name)?;
        std::fs::create_dir_all(dir)?;

        let final_path = dir.join(&file_name);
        let part_path = dir.join(format!("{file_name}.nebula-part"));
        let checksum = version.file.sha1.clone().map(Checksum::Sha1);
        let mut options = DownloadOptions::new(&version.file.url, part_path.as_path());
        if let Some(checksum) = checksum.as_ref() {
            options = options.with_checksum(checksum);
        }

        download_file(&self.client, options).await.map_err(|error| {
            let code = if error.contains("integrity check failed") {
                ErrorCode::ChecksumMismatch
            } else {
                ErrorCode::DownloadFailed
            };
            AppError::new(code).with("file", &file_name).detail(error)
        })?;

        let replaced = commit_jar(dir, &part_path, &final_path, &file_name, replace)?;

        forget_hash(&final_path);
        Ok(AddonInstallResult {
            file_name,
            project_id: version.project_id.clone(),
            version_id: version.id.clone(),
            version_number: version.version_number.clone(),
            replaced,
            pending: false,
        })
    }

    /// Download a version's jar into `staging` (verified, under a unique
    /// name) without touching any server folder. Used while the server is
    /// running: the bytes arrive now, the jar goes in when it stops.
    /// Returns the staged file and the jar's own name.
    pub async fn stage(&self, staging: &Path, version: &AddonVersion) -> AppResult<(PathBuf, String)> {
        let file_name = sanitise_file_name(&version.file.file_name)?;
        std::fs::create_dir_all(staging)?;
        let staged = staging.join(format!("{}.jar", uuid::Uuid::new_v4()));
        let checksum = version.file.sha1.clone().map(Checksum::Sha1);
        let mut options = DownloadOptions::new(&version.file.url, staged.as_path());
        if let Some(checksum) = checksum.as_ref() {
            options = options.with_checksum(checksum);
        }
        download_file(&self.client, options).await.map_err(|error| {
            let code = if error.contains("integrity check failed") {
                ErrorCode::ChecksumMismatch
            } else {
                ErrorCode::DownloadFailed
            };
            AppError::new(code).with("file", &file_name).detail(error)
        })?;
        Ok((staged, file_name))
    }
}

/// Put a staged jar (from [`Modrinth::stage`]) into `dir` as `file_name`,
/// replacing `replace`, with the same never-two-copies guarantee as a direct
/// install. The staged file is left where it is; the caller removes it once
/// the change is recorded as done.
pub fn commit_staged(dir: &Path, staged: &Path, file_name: &str, replace: Option<&str>) -> AppResult<Option<String>> {
    let file_name = sanitise_file_name(file_name)?;
    std::fs::create_dir_all(dir)?;
    let final_path = dir.join(&file_name);
    let part_path = dir.join(format!("{file_name}.nebula-part"));
    // Copy rather than rename: the staging folder may be on another drive.
    if let Err(error) = std::fs::copy(staged, &part_path) {
        let _ = std::fs::remove_file(&part_path);
        return Err(AppError::from(error).with("file", &file_name));
    }
    let replaced = commit_jar(dir, &part_path, &final_path, &file_name, replace)?;
    forget_hash(&final_path);
    Ok(replaced)
}

// ---------------------------------------------------------------------------
// Pure helpers (tested)

/// Put a downloaded jar (`part_path`) in place at `final_path`, swapping out
/// `replace` (the old jar's name). Returns the replaced name, if one went.
///
/// Commits without ever leaving two copies, or none. On Windows a running
/// JVM holds its jars open: renaming one then fails, and that failure must
/// leave the folder exactly as it was (no new jar next to the old one —
/// Fabric refuses duplicate mod ids, Paper warns of an ambiguous plugin). So
/// the old jar (and anything already at the new name) is parked under
/// [`PARKED_SUFFIX`], which the folder scan ignores, the new jar goes in, and
/// only then is what was parked deleted. Every failure removes `part_path`.
fn commit_jar(
    dir: &Path,
    part_path: &Path,
    final_path: &Path,
    file_name: &str,
    replace: Option<&str>,
) -> AppResult<Option<String>> {
    let discard_part = |error: AppError| {
        let _ = std::fs::remove_file(part_path);
        error
    };
    let old_path = match replace {
        Some(old) if old != file_name => {
            let path = crate::store::paths::resolve_inside(dir, old).map_err(discard_part)?;
            path.exists().then_some(path)
        }
        _ => None,
    };
    let replaced = old_path.as_ref().and(replace).map(str::to_string);

    let mut parked: Vec<(PathBuf, PathBuf)> = Vec::new();
    let unpark = |parked: &[(PathBuf, PathBuf)]| {
        for (original, aside) in parked.iter().rev() {
            let _ = std::fs::rename(aside, original);
        }
    };
    for path in [old_path, final_path.exists().then(|| final_path.to_path_buf())].into_iter().flatten() {
        let mut aside = path.clone().into_os_string();
        aside.push(PARKED_SUFFIX);
        let aside = PathBuf::from(aside);
        if let Err(error) = std::fs::rename(&path, &aside) {
            unpark(&parked);
            // 32 = ERROR_SHARING_VIOLATION: the server has the jar open.
            let in_use = error.raw_os_error() == Some(32) || error.kind() == std::io::ErrorKind::PermissionDenied;
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let mut error = AppError::from(error).with("file", name);
            if in_use {
                error = error.with("reason", "in_use");
            }
            return Err(discard_part(error));
        }
        parked.push((path, aside));
    }
    if let Err(error) = std::fs::rename(part_path, final_path) {
        unpark(&parked);
        return Err(discard_part(AppError::from(error).with("file", file_name)));
    }
    for (_, aside) in &parked {
        let _ = std::fs::remove_file(aside);
    }
    Ok(replaced)
}
// ---------------------------------------------------------------------------

/// Facets: right project type, this exact game version, any accepted loader,
/// and — for mods — something that actually runs on a server.
pub fn search_facets(profile: &AddonProfile, categories: &[String]) -> Vec<Vec<String>> {
    let mut facets = vec![
        vec![format!("project_type:{}", profile.project_type)],
        vec![format!("versions:{}", profile.game_version)],
        profile.loaders.iter().map(|l| format!("categories:{l}")).collect(),
    ];
    if profile.project_type == "mod" {
        facets.push(vec!["server_side:required".into(), "server_side:optional".into()]);
    }
    for category in categories.iter().filter(|c| is_tag(c)) {
        facets.push(vec![format!("categories:{category}")]);
    }
    facets
}

/// Loader tags Modrinth mixes into a project's categories. Shown as badges
/// they are noise: the search is already filtered to the server's loaders.
const LOADER_TAGS: &[&str] = &[
    "fabric", "forge", "neoforge", "quilt", "liteloader", "modloader", "rift", "paper", "purpur",
    "spigot", "bukkit", "folia", "sponge", "velocity", "bungeecord", "waterfall", "datapack",
];

/// Facet values are user input in a query string; keep them to tag shape.
fn is_tag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Project and version ids go into URL paths.
fn path_segment(id: &str) -> AppResult<&str> {
    if is_tag(id) {
        Ok(id)
    } else {
        Err(AppError::new(ErrorCode::InvalidInput).with("id", id))
    }
}

fn parse_hit(hit: &Value, profile: &AddonProfile, installed: &HashSet<String>) -> Option<AddonHit> {
    let project_id = hit["project_id"].as_str()?.to_string();
    let categories = string_array(&hit["display_categories"])
        .into_iter()
        .filter(|c| !LOADER_TAGS.contains(&c.as_str()))
        .collect();
    // A project published as both a mod and a plugin reports `mod` as its
    // primary type; on a plugin server it is a plugin.
    let project_type = if string_array(&hit["all_project_types"]).contains(&profile.project_type) {
        profile.project_type.clone()
    } else {
        hit["project_type"].as_str().unwrap_or(&profile.project_type).to_string()
    };
    Some(AddonHit {
        installed: installed.contains(&project_id),
        project_id,
        slug: str_of(hit, "slug"),
        title: str_of(hit, "title"),
        description: str_of(hit, "description"),
        author: str_of(hit, "author"),
        icon_url: opt_str(hit, "icon_url"),
        downloads: hit["downloads"].as_u64().unwrap_or(0),
        follows: hit["follows"].as_u64().unwrap_or(0),
        categories,
        date_modified: opt_str(hit, "date_modified"),
        project_type,
        client_side: opt_str(hit, "client_side"),
        server_side: opt_str(hit, "server_side"),
    })
}

pub fn parse_project(project: &Value) -> Option<AddonProjectSummary> {
    Some(AddonProjectSummary {
        project_id: project["id"].as_str()?.to_string(),
        slug: str_of(project, "slug"),
        title: str_of(project, "title"),
        description: str_of(project, "description"),
        icon_url: opt_str(project, "icon_url"),
        project_type: str_of(project, "project_type"),
        client_side: opt_str(project, "client_side"),
        server_side: opt_str(project, "server_side"),
    })
}

pub fn parse_version(version: &Value, profile: &AddonProfile) -> Option<AddonVersion> {
    let files = version["files"].as_array()?;
    let file = files.iter().find(|f| f["primary"].as_bool() == Some(true)).or_else(|| files.first())?;
    let game_versions = string_array(&version["game_versions"]);
    let loaders = string_array(&version["loaders"]);
    // Modrinth's facet filter is fuzzy about point releases; insist on an
    // exact match so a 1.21.1 mod never lands on a 1.21.4 server.
    let compatible = game_versions.iter().any(|v| v == &profile.game_version)
        && loader_rank(&loaders, &profile.loaders).is_some();

    Some(AddonVersion {
        id: version["id"].as_str()?.to_string(),
        project_id: str_of(version, "project_id"),
        name: version["name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .or_else(|| version["version_number"].as_str())
            .unwrap_or_default()
            .to_string(),
        version_number: str_of(version, "version_number"),
        version_type: version["version_type"].as_str().unwrap_or("release").to_string(),
        date_published: opt_str(version, "date_published"),
        downloads: version["downloads"].as_u64().unwrap_or(0),
        game_versions,
        loaders,
        file: AddonFile {
            url: file["url"].as_str()?.to_string(),
            file_name: file["filename"].as_str()?.to_string(),
            size: file["size"].as_u64().unwrap_or(0),
            sha1: file["hashes"]["sha1"].as_str().map(String::from),
            sha512: file["hashes"]["sha512"].as_str().map(String::from),
        },
        dependencies: version["dependencies"]
            .as_array()
            .map(|deps| {
                deps.iter()
                    .map(|d| AddonDependencyRef {
                        project_id: opt_str(d, "project_id"),
                        version_id: opt_str(d, "version_id"),
                        file_name: opt_str(d, "file_name"),
                        dependency_type: d["dependency_type"].as_str().unwrap_or("required").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        compatible,
    })
}

/// Prefer the loader the server actually runs: a mod published for both
/// `neoforge` and `forge` should install its NeoForge build on NeoForge.
pub fn loader_rank(version_loaders: &[String], preferred: &[String]) -> Option<usize> {
    preferred.iter().enumerate().find_map(|(index, loader)| {
        version_loaders.iter().any(|value| value.eq_ignore_ascii_case(loader)).then_some(index)
    })
}

fn channel_rank(version_type: &str) -> u8 {
    match version_type {
        "release" => 0,
        "beta" => 1,
        _ => 2,
    }
}

/// The version to install: compatible, a release over a beta over an alpha,
/// the server's own loader first, then newest (Modrinth lists newest first).
pub fn best_version(versions: Vec<AddonVersion>, preferred: &[String]) -> Option<AddonVersion> {
    versions.into_iter().filter(|v| v.compatible).min_by_key(|v| {
        (channel_rank(&v.version_type), loader_rank(&v.loaders, preferred).unwrap_or(usize::MAX))
    })
}

/// Upstream chooses the filename, so treat it as untrusted.
pub fn sanitise_file_name(file_name: &str) -> AppResult<String> {
    let trimmed = file_name.trim();
    let invalid = trimmed.is_empty()
        || trimmed.contains(['/', '\\', ':'])
        || trimmed.contains("..")
        || !trimmed.ends_with(".jar");
    if invalid {
        return Err(AppError::new(ErrorCode::InvalidInput).with("file", file_name));
    }
    Ok(trimmed.to_string())
}

/// `Foo-1.2.jar.disabled` → `Foo-1.2`.
pub fn display_name(file_name: &str) -> String {
    file_name.trim_end_matches(DISABLED_SUFFIX).trim_end_matches(".jar").to_string()
}

#[derive(Debug, Clone)]
pub struct LocalJar {
    pub file_name: String,
    pub enabled: bool,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
    pub sha1: String,
}

/// Put the pieces together. Pure, so the mapping is testable without a
/// network: hash → version → project, and hash → latest.
pub fn assemble_report(
    local: Vec<LocalJar>,
    known: &HashMap<String, Value>,
    updates: &HashMap<String, Value>,
    projects: &[AddonProjectSummary],
    profile: &AddonProfile,
    modrinth_status: ModrinthStatus,
) -> InstalledAddonsReport {
    let mut addons: Vec<InstalledAddonEntry> = local
        .into_iter()
        .map(|jar| {
            let modrinth = known.get(&jar.sha1).and_then(|raw| {
                let installed_version = parse_version(raw, profile)?;
                let project = projects
                    .iter()
                    .find(|p| p.project_id == installed_version.project_id)
                    .cloned()
                    .unwrap_or_else(|| AddonProjectSummary {
                        project_id: installed_version.project_id.clone(),
                        slug: String::new(),
                        title: display_name(&jar.file_name),
                        description: String::new(),
                        icon_url: None,
                        project_type: profile.project_type.clone(),
                        client_side: None,
                        server_side: None,
                    });
                let latest_version = updates
                    .get(&jar.sha1)
                    .and_then(|raw| parse_version(raw, profile))
                    .filter(|latest| latest.project_id == installed_version.project_id);
                let update_available = latest_version
                    .as_ref()
                    .is_some_and(|latest| is_newer(latest, &installed_version));
                Some(ModrinthMatch { project, installed_version, latest_version, update_available })
            });
            InstalledAddonEntry {
                display_name: display_name(&jar.file_name),
                file_name: jar.file_name,
                enabled: jar.enabled,
                size_bytes: jar.size,
                modified: jar.modified,
                sha1: jar.sha1,
                modrinth,
            }
        })
        .collect();

    addons.sort_by_key(|a| {
        a.modrinth.as_ref().map(|m| m.project.title.clone()).unwrap_or_else(|| a.display_name.clone()).to_lowercase()
    });

    let unknown_count = addons.iter().filter(|a| a.modrinth.is_none()).count() as u32;
    let updates_available =
        addons.iter().filter(|a| a.modrinth.as_ref().is_some_and(|m| m.update_available)).count() as u32;
    InstalledAddonsReport {
        addons,
        modrinth_status,
        unknown_count: if modrinth_status == ModrinthStatus::Ok { unknown_count } else { 0 },
        updates_available,
    }
}

/// "Update" means a different, *later* build — never offer a downgrade just
/// because the installed jar targets a newer game version than the server.
fn is_newer(latest: &AddonVersion, installed: &AddonVersion) -> bool {
    if latest.id == installed.id {
        return false;
    }
    match (&latest.date_published, &installed.date_published) {
        (Some(a), Some(b)) => a > b,
        _ => true,
    }
}

fn hash_cache() -> &'static Mutex<HashMap<PathBuf, (u64, SystemTime, String)>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (u64, SystemTime, String)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn forget_hash(path: &Path) {
    if let Ok(mut map) = hash_cache().lock() {
        map.remove(path);
    }
}

/// SHA-1 of a file, remembered by (size, mtime) so a 200-mod folder is not
/// re-hashed on every visit to the panel.
pub fn sha1_file(path: &Path) -> AppResult<String> {
    let metadata = std::fs::metadata(path)?;
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    if let Ok(map) = hash_cache().lock() {
        if let Some((size, at, hash)) = map.get(path) {
            if *size == metadata.len() && *at == modified {
                return Ok(hash.clone());
            }
        }
    }

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    let hash = hex::encode(hasher.finalize());
    if let Ok(mut map) = hash_cache().lock() {
        map.insert(path.to_path_buf(), (metadata.len(), modified, hash.clone()));
    }
    Ok(hash)
}

/// Every `.jar` / `.jar.disabled` in the folder, hashed.
pub fn scan_dir(dir: &Path) -> AppResult<Vec<LocalJar>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut jars = Vec::new();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let enabled = file_name.ends_with(".jar");
        if !enabled && !file_name.ends_with(".jar.disabled") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else { continue };
        if !metadata.is_file() {
            continue;
        }
        let Ok(sha1) = sha1_file(&entry.path()) else { continue };
        jars.push(LocalJar {
            file_name,
            enabled,
            size: metadata.len(),
            modified: metadata.modified().ok().map(DateTime::<Utc>::from),
            sha1,
        });
    }
    Ok(jars)
}

fn str_of(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_string()
}

fn opt_str(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().filter(|s| !s.is_empty()).map(String::from)
}

fn string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn commit_swaps_the_old_jar_for_the_new_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("Foo-1.0.jar"), b"old").expect("old");
        std::fs::write(dir.path().join("Foo-2.0.jar.nebula-part"), b"new").expect("part");
        let replaced = commit_jar(
            dir.path(),
            &dir.path().join("Foo-2.0.jar.nebula-part"),
            &dir.path().join("Foo-2.0.jar"),
            "Foo-2.0.jar",
            Some("Foo-1.0.jar"),
        )
        .expect("commit");
        assert_eq!(replaced.as_deref(), Some("Foo-1.0.jar"));
        assert_eq!(names(dir.path()), vec!["Foo-2.0.jar"]);
        assert_eq!(std::fs::read(dir.path().join("Foo-2.0.jar")).expect("read"), b"new");
    }

    #[test]
    fn commit_over_the_same_name_replaces_it_and_leaves_no_part_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("Foo.jar"), b"old").expect("old");
        std::fs::write(dir.path().join("Foo.jar.nebula-part"), b"new").expect("part");
        let replaced = commit_jar(
            dir.path(),
            &dir.path().join("Foo.jar.nebula-part"),
            &dir.path().join("Foo.jar"),
            "Foo.jar",
            Some("Foo.jar"),
        )
        .expect("commit");
        assert_eq!(replaced, None);
        assert_eq!(names(dir.path()), vec!["Foo.jar"]);
        assert_eq!(std::fs::read(dir.path().join("Foo.jar")).expect("read"), b"new");
    }

    #[cfg(windows)]
    #[test]
    fn commit_leaves_the_folder_unchanged_when_the_old_jar_is_in_use() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let old = dir.path().join("Foo-1.0.jar");
        std::fs::write(&old, b"old").expect("old");
        std::fs::write(dir.path().join("Foo-2.0.jar.nebula-part"), b"new").expect("part");
        // Like a running JVM: open with no FILE_SHARE_DELETE, so it cannot be renamed.
        let _held = std::fs::OpenOptions::new().read(true).share_mode(0x1).open(&old).expect("hold");
        let error = commit_jar(
            dir.path(),
            &dir.path().join("Foo-2.0.jar.nebula-part"),
            &dir.path().join("Foo-2.0.jar"),
            "Foo-2.0.jar",
            Some("Foo-1.0.jar"),
        )
        .expect_err("in use");
        assert_eq!(error.params.get("reason").map(String::as_str), Some("in_use"));
        assert_eq!(names(dir.path()), vec!["Foo-1.0.jar"], "no new jar beside the old one");
    }

    fn paper() -> AddonProfile {
        AddonProfile {
            project_type: "plugin".into(),
            loaders: vec!["paper".into(), "spigot".into(), "bukkit".into()],
            game_version: "1.21.4".into(),
        }
    }

    fn version_json(id: &str, project: &str, game: &str, loaders: &[&str], kind: &str, date: &str, sha1: &str) -> Value {
        json!({
            "id": id, "project_id": project, "name": format!("v {id}"), "version_number": id,
            "version_type": kind, "date_published": date, "downloads": 10,
            "game_versions": [game], "loaders": loaders,
            "files": [
                { "url": "https://cdn/x-sources.jar", "filename": "x-sources.jar", "size": 1, "primary": false, "hashes": {} },
                { "url": format!("https://cdn/{id}.jar"), "filename": format!("{id}.jar"), "size": 2, "primary": true,
                  "hashes": { "sha1": sha1, "sha512": "ff" } }
            ],
            "dependencies": [ { "project_id": "dep1", "version_id": null, "dependency_type": "required" } ]
        })
    }

    #[test]
    fn facets_filter_type_version_loaders_and_server_side_for_mods() {
        let facets = search_facets(&paper(), &["utility".into(), "bad tag!".into()]);
        assert_eq!(facets[0], vec!["project_type:plugin"]);
        assert_eq!(facets[1], vec!["versions:1.21.4"]);
        assert_eq!(facets[2], vec!["categories:paper", "categories:spigot", "categories:bukkit"]);
        assert_eq!(facets[3], vec!["categories:utility"], "malformed categories are dropped");
        assert_eq!(facets.len(), 4, "plugins get no server_side facet");

        let fabric = AddonProfile { project_type: "mod".into(), loaders: vec!["fabric".into()], game_version: "1.21.1".into() };
        assert!(search_facets(&fabric, &[]).iter().any(|f| f.contains(&"server_side:required".to_string())));
    }

    #[test]
    fn hits_drop_loader_badges_and_take_the_servers_project_type() {
        let hit = json!({
            "project_id": "OhduvhIc", "project_type": "mod", "all_project_types": ["datapack", "mod", "plugin"],
            "slug": "veinminer", "title": "VeinMiner", "author": "M", "display_categories": ["paper", "fabric", "utility"],
            "downloads": 5, "follows": 1
        });
        let parsed = parse_hit(&hit, &paper(), &HashSet::from(["OhduvhIc".to_string()])).expect("parses");
        assert_eq!(parsed.project_type, "plugin");
        assert_eq!(parsed.categories, vec!["utility".to_string()]);
        assert!(parsed.installed);
    }

    #[test]
    fn versions_parse_the_primary_file_with_its_hash_and_dependencies() {
        let v = parse_version(&version_json("a", "p", "1.21.4", &["paper"], "release", "2024-01-01", "abc"), &paper())
            .expect("parses");
        assert_eq!(v.file.file_name, "a.jar");
        assert_eq!(v.file.sha1.as_deref(), Some("abc"));
        assert_eq!(v.dependencies[0].project_id.as_deref(), Some("dep1"));
        assert!(v.compatible);

        let other = parse_version(&version_json("b", "p", "1.21.1", &["paper"], "release", "2024-01-01", "x"), &paper())
            .expect("parses");
        assert!(!other.compatible, "exact game version required");
        let fabric = parse_version(&version_json("c", "p", "1.21.4", &["fabric"], "release", "2024-01-01", "x"), &paper())
            .expect("parses");
        assert!(!fabric.compatible, "loader must be accepted");
    }

    #[test]
    fn best_version_prefers_release_then_own_loader_then_newest() {
        let profile = paper();
        let parse = |v: Value| parse_version(&v, &profile).expect("parses");
        let versions = vec![
            parse(version_json("beta-paper", "p", "1.21.4", &["paper"], "beta", "2024-03-01", "1")),
            parse(version_json("rel-bukkit", "p", "1.21.4", &["bukkit"], "release", "2024-02-01", "2")),
            parse(version_json("rel-paper", "p", "1.21.4", &["paper"], "release", "2024-01-01", "3")),
            parse(version_json("rel-paper-old", "p", "1.21.4", &["paper"], "release", "2023-01-01", "4")),
        ];
        assert_eq!(best_version(versions, &profile.loaders).expect("one").id, "rel-paper");

        let only_beta = vec![parse(version_json("b", "p", "1.21.4", &["paper"], "beta", "2024-01-01", "1"))];
        assert_eq!(best_version(only_beta, &profile.loaders).expect("beta is fine").id, "b");
        let incompatible = vec![parse(version_json("x", "p", "1.20.1", &["paper"], "release", "2024-01-01", "1"))];
        assert!(best_version(incompatible, &profile.loaders).is_none());
    }

    #[test]
    fn identification_maps_hashes_to_projects_and_flags_real_updates() {
        let profile = paper();
        let jar = |name: &str, sha1: &str| LocalJar {
            file_name: name.into(), enabled: !name.ends_with(".disabled"), size: 1, modified: None, sha1: sha1.into(),
        };
        let local = vec![jar("Essentials.jar", "h1"), jar("custom.jar", "h2"), jar("WorldEdit.jar.disabled", "h3"), jar("Old.jar", "h4")];
        let known = HashMap::from([
            ("h1".to_string(), version_json("ess-1", "ess", "1.21.4", &["paper"], "release", "2024-01-01", "h1")),
            ("h3".to_string(), version_json("we-1", "we", "1.21.4", &["bukkit"], "release", "2024-01-01", "h3")),
            ("h4".to_string(), version_json("old-2", "old", "1.21.5", &["paper"], "release", "2025-01-01", "h4")),
        ]);
        let updates = HashMap::from([
            ("h1".to_string(), version_json("ess-2", "ess", "1.21.4", &["paper"], "release", "2024-06-01", "n1")),
            ("h3".to_string(), version_json("we-1", "we", "1.21.4", &["bukkit"], "release", "2024-01-01", "h3")),
            // An older build for the server's version must not read as an update.
            ("h4".to_string(), version_json("old-1", "old", "1.21.4", &["paper"], "release", "2024-01-01", "o1")),
        ]);
        let projects = vec![AddonProjectSummary {
            project_id: "ess".into(), slug: "essentialsx".into(), title: "EssentialsX".into(), description: String::new(),
            icon_url: None, project_type: "plugin".into(), client_side: None, server_side: None,
        }];

        let report = assemble_report(local, &known, &updates, &projects, &profile, ModrinthStatus::Ok);
        assert_eq!(report.addons.len(), 4);
        assert_eq!(report.unknown_count, 1);
        assert_eq!(report.updates_available, 1);

        let ess = report.addons.iter().find(|a| a.file_name == "Essentials.jar").expect("ess");
        let m = ess.modrinth.as_ref().expect("identified");
        assert_eq!(m.project.title, "EssentialsX");
        assert!(m.update_available);
        assert_eq!(m.latest_version.as_ref().map(|v| v.id.as_str()), Some("ess-2"));

        let we = report.addons.iter().find(|a| a.file_name == "WorldEdit.jar.disabled").expect("we");
        assert!(!we.enabled);
        assert!(!we.modrinth.as_ref().expect("identified").update_available, "same version");
        assert_eq!(we.modrinth.as_ref().expect("identified").project.title, "WorldEdit", "falls back to file name");

        let old = report.addons.iter().find(|a| a.file_name == "Old.jar").expect("old");
        assert!(!old.modrinth.as_ref().expect("identified").update_available, "no downgrade offered");
        assert!(!old.modrinth.as_ref().expect("identified").installed_version.compatible);

        let custom = report.addons.iter().find(|a| a.file_name == "custom.jar").expect("custom");
        assert!(custom.modrinth.is_none(), "unknown jars are listed, not dropped");
    }

    #[test]
    fn unavailable_modrinth_reports_no_unknowns() {
        let local = vec![LocalJar { file_name: "a.jar".into(), enabled: true, size: 1, modified: None, sha1: "h".into() }];
        let report = assemble_report(local, &HashMap::new(), &HashMap::new(), &[], &paper(), ModrinthStatus::Unavailable);
        assert_eq!(report.unknown_count, 0, "we do not know they are unknown");
        assert_eq!(report.addons.len(), 1);
    }

    #[test]
    fn upstream_filenames_and_ids_that_try_to_escape_are_rejected() {
        for name in ["../evil.jar", "sub/dir.jar", "C:evil.jar", "notajar.txt", "  "] {
            assert!(sanitise_file_name(name).is_err(), "{name}");
        }
        assert_eq!(sanitise_file_name(" fabric-api.jar ").expect("ok"), "fabric-api.jar");
        assert!(path_segment("../projects").is_err());
        assert!(path_segment("AANobbMI").is_ok());
    }

    #[test]
    fn hashing_matches_a_known_digest_and_is_cached() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.jar");
        std::fs::write(&path, b"abc").expect("write");
        assert_eq!(sha1_file(&path).expect("hash"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(sha1_file(&path).expect("cached"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        let jars = scan_dir(dir.path()).expect("scan");
        assert_eq!(jars.len(), 1);
        assert_eq!(display_name("Foo-1.2.jar.disabled"), "Foo-1.2");
    }
}
