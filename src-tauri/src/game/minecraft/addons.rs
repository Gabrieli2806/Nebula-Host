//! Mods and plugins, via Modrinth.
//!
//! Two things here are worth keeping: the search is *facet-filtered by the
//! server's own loader and version*, so the list only ever shows things that
//! will actually load; and installed jars are identified by SHA-1 hash rather
//! than by filename, which is the only way to recognise a file the user
//! dropped in themselves.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use nebula_downloader::{download_file, DownloadOptions};
use reqwest::Client;
use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::error::{AppError, AppResult, ErrorCode};

use super::types::{
    AddonCatalogEntry, AddonCatalogVersion, AddonModrinthMeta, AddonSearchResult, InstalledAddon,
    ServerType,
};

const MODRINTH_API: &str = "https://api.modrinth.com/v2";
/// Jars the user disabled keep their bytes and gain a suffix.
const DISABLED_SUFFIX: &str = ".disabled";

/// What kind of addon a distribution takes, and which loader tags identify it.
struct Compatibility {
    project_type: &'static str,
    loaders: &'static [&'static str],
}

fn compatibility(server_type: ServerType) -> Option<Compatibility> {
    match server_type {
        ServerType::Fabric => Some(Compatibility { project_type: "mod", loaders: &["fabric"] }),
        ServerType::Forge => Some(Compatibility { project_type: "mod", loaders: &["forge"] }),
        ServerType::NeoForge => Some(Compatibility { project_type: "mod", loaders: &["neoforge"] }),
        ServerType::Paper => {
            Some(Compatibility { project_type: "plugin", loaders: &["paper", "spigot", "bukkit"] })
        }
        ServerType::Purpur => Some(Compatibility {
            project_type: "plugin",
            loaders: &["purpur", "paper", "spigot", "bukkit"],
        }),
        ServerType::Spigot => {
            Some(Compatibility { project_type: "plugin", loaders: &["spigot", "bukkit"] })
        }
        ServerType::Vanilla => None,
    }
}

pub struct AddonClient {
    client: Client,
}

impl AddonClient {
    pub fn new() -> Self {
        Self { client: crate::game::http_client() }
    }

    /// Search for addons this server can actually run.
    pub async fn search(
        &self,
        query: &str,
        server_type: ServerType,
        mc_version: &str,
        offset: u32,
        limit: u32,
    ) -> AppResult<AddonSearchResult> {
        let Some(profile) = compatibility(server_type) else {
            return Ok(AddonSearchResult { entries: vec![], total_hits: 0 });
        };
        let limit = limit.clamp(1, 100);

        let facets = serde_json::to_string(&vec![
            vec![format!("project_type:{}", profile.project_type)],
            vec![format!("versions:{mc_version}")],
            profile.loaders.iter().map(|l| format!("categories:{l}")).collect::<Vec<_>>(),
        ])?;

        // An empty query is a browse, and browsing should surface what people
        // actually use.
        let index = if query.trim().is_empty() { "downloads" } else { "relevance" };

        let response: Value = self
            .client
            .get(format!("{MODRINTH_API}/search"))
            .query(&[
                ("query", query.trim()),
                ("facets", facets.as_str()),
                ("limit", limit.to_string().as_str()),
                ("offset", offset.to_string().as_str()),
                ("index", index),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let hits = response["hits"]
            .as_array()
            .ok_or_else(|| AppError::new(ErrorCode::UpstreamRejected).detail("no hits array"))?;
        let total_hits = response["total_hits"].as_u64().unwrap_or(hits.len() as u64);

        let mut entries = Vec::new();
        for project in hits {
            let Some(project_id) = project["project_id"].as_str() else { continue };
            let versions =
                self.compatible_versions(project_id, mc_version, profile.loaders).await?;
            let Some(featured_version) = best_version(versions, profile.loaders) else { continue };

            entries.push(AddonCatalogEntry {
                project_id: project_id.to_string(),
                slug: project["slug"].as_str().unwrap_or_default().to_string(),
                title: project["title"].as_str().unwrap_or_default().to_string(),
                description: project["description"].as_str().unwrap_or_default().to_string(),
                icon_url: project["icon_url"].as_str().map(String::from),
                downloads: project["downloads"].as_u64().unwrap_or(0),
                project_type: profile.project_type.to_string(),
                featured_version,
            });
        }

        Ok(AddonSearchResult { entries, total_hits })
    }

    /// Versions of one project that match this server's version and loader.
    async fn compatible_versions(
        &self,
        project_id: &str,
        mc_version: &str,
        loaders: &[&str],
    ) -> AppResult<Vec<AddonCatalogVersion>> {
        let response: Vec<Value> = self
            .client
            .get(format!("{MODRINTH_API}/project/{project_id}/version"))
            .query(&[
                ("game_versions", serde_json::to_string(&vec![mc_version])?),
                ("loaders", serde_json::to_string(&loaders)?),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let mut versions = Vec::new();
        for version in response {
            let game_versions = string_array(&version["game_versions"]);
            // Modrinth's facet filter is fuzzy about point releases; insist on
            // an exact match so a 1.21.1 mod never lands on a 1.21.4 server.
            if !game_versions.iter().any(|value| value == mc_version) {
                continue;
            }

            let version_loaders = string_array(&version["loaders"]);
            if loader_rank(&version_loaders, loaders).is_none() {
                continue;
            }

            let Some(files) = version["files"].as_array() else { continue };
            let Some((download_url, file_name, file_size)) = primary_file(files) else { continue };

            versions.push(AddonCatalogVersion {
                id: version["id"].as_str().unwrap_or_default().to_string(),
                name: version["name"]
                    .as_str()
                    .or_else(|| version["version_number"].as_str())
                    .unwrap_or("Version")
                    .to_string(),
                version_number: version["version_number"].as_str().unwrap_or_default().to_string(),
                game_versions,
                loaders: version_loaders,
                download_url,
                file_name,
                file_size,
            });
        }

        Ok(versions)
    }

    /// Download an addon into the instance's mods/plugins directory.
    pub async fn install(&self, url: &str, dir: &Path, file_name: &str) -> AppResult<()> {
        std::fs::create_dir_all(dir)?;
        let dest = dir.join(sanitise_file_name(file_name)?);
        download_file(&self.client, DownloadOptions::new(url, dest.as_path()))
            .await
            .map_err(|error| {
                AppError::new(ErrorCode::DownloadFailed).with("file", file_name).detail(error)
            })
    }

    /// Identify the jars already in a directory by hashing them and asking
    /// Modrinth what they are — which also tells us what has an update.
    pub async fn identify_installed(
        &self,
        dir: &Path,
        server_type: ServerType,
        mc_version: &str,
    ) -> AppResult<Vec<AddonModrinthMeta>> {
        let Some(profile) = compatibility(server_type) else { return Ok(Vec::new()) };

        let hash_to_file = hash_directory(dir)?;
        if hash_to_file.is_empty() {
            return Ok(Vec::new());
        }

        let hashes: Vec<&str> = hash_to_file.keys().map(String::as_str).collect();
        let version_map: HashMap<String, Value> = self
            .client
            .post(format!("{MODRINTH_API}/version_files"))
            .json(&serde_json::json!({ "hashes": hashes, "algorithm": "sha1" }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        if version_map.is_empty() {
            return Ok(Vec::new());
        }

        let mut project_ids: Vec<String> = Vec::new();
        let mut by_project: HashMap<String, (String, &Value)> = HashMap::new();
        for (hash, version) in &version_map {
            if let Some(project_id) = version["project_id"].as_str() {
                if !project_ids.iter().any(|id| id == project_id) {
                    project_ids.push(project_id.to_string());
                }
                by_project.insert(project_id.to_string(), (hash.clone(), version));
            }
        }

        let projects: Vec<Value> = self
            .client
            .get(format!("{MODRINTH_API}/projects"))
            .query(&[("ids", serde_json::to_string(&project_ids)?)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let mut results = Vec::new();
        for project in &projects {
            let Some(project_id) = project["id"].as_str() else { continue };
            let Some((hash, version)) = by_project.get(project_id) else { continue };
            let Some(file_name) = hash_to_file.get(hash.as_str()) else { continue };

            let installed_version_id = version["id"].as_str().unwrap_or_default().to_string();
            let installed_version_number =
                version["version_number"].as_str().unwrap_or_default().to_string();

            // A failed update check must not hide the addon itself.
            let latest = self
                .compatible_versions(project_id, mc_version, profile.loaders)
                .await
                .ok()
                .and_then(|versions| best_version(versions, profile.loaders));

            results.push(AddonModrinthMeta {
                file_name: file_name.clone(),
                project_id: project_id.to_string(),
                slug: project["slug"].as_str().unwrap_or_default().to_string(),
                title: project["title"].as_str().unwrap_or_default().to_string(),
                description: project["description"].as_str().unwrap_or_default().to_string(),
                icon_url: project["icon_url"].as_str().map(String::from),
                update_available: latest
                    .as_ref()
                    .is_some_and(|version| version.id != installed_version_id),
                installed_version_id,
                installed_version_number,
                latest_version_id: latest.as_ref().map(|v| v.id.clone()),
                latest_version_number: latest.as_ref().map(|v| v.version_number.clone()),
                latest_download_url: latest.as_ref().map(|v| v.download_url.clone()),
                latest_file_name: latest.as_ref().map(|v| v.file_name.clone()),
                latest_file_size: latest.as_ref().map(|v| v.file_size),
            });
        }

        Ok(results)
    }
}

impl Default for AddonClient {
    fn default() -> Self {
        Self::new()
    }
}

/// List the jars in an addon directory, enabled or not.
pub fn list_installed(dir: &Path) -> AppResult<Vec<InstalledAddon>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut addons = Vec::new();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let enabled = file_name.ends_with(".jar");
        if !enabled && !file_name.ends_with(".jar.disabled") {
            continue;
        }

        let display = file_name.trim_end_matches(DISABLED_SUFFIX).trim_end_matches(".jar");
        addons.push(InstalledAddon {
            id: file_name.clone(),
            name: display.to_string(),
            version: String::new(),
            enabled,
            file_name,
            icon_data_url: None,
        });
    }

    addons.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(addons)
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

fn hash_directory(dir: &Path) -> AppResult<HashMap<String, String>> {
    let mut hashes = HashMap::new();
    if !dir.exists() {
        return Ok(hashes);
    }

    for entry in std::fs::read_dir(dir)?.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.ends_with(".jar") && !file_name.ends_with(".jar.disabled") {
            continue;
        }

        let mut file = std::fs::File::open(entry.path())?;
        let mut hasher = Sha1::new();
        let mut buf = [0u8; 8192];
        loop {
            let read = file.read(&mut buf)?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
        }
        hashes.insert(hex::encode(hasher.finalize()), file_name);
    }

    Ok(hashes)
}

/// Prefer the loader the server actually runs: a mod published for both
/// `neoforge` and `forge` should install its NeoForge build on NeoForge.
fn loader_rank(version_loaders: &[String], preferred: &[&str]) -> Option<usize> {
    preferred.iter().enumerate().find_map(|(index, loader)| {
        version_loaders.iter().any(|value| value.eq_ignore_ascii_case(loader)).then_some(index)
    })
}

fn best_version(
    versions: Vec<AddonCatalogVersion>,
    preferred: &[&str],
) -> Option<AddonCatalogVersion> {
    versions
        .into_iter()
        .min_by_key(|version| loader_rank(&version.loaders, preferred).unwrap_or(usize::MAX))
}

fn primary_file(files: &[Value]) -> Option<(String, String, u64)> {
    let selected =
        files.iter().find(|file| file["primary"].as_bool() == Some(true)).or_else(|| files.first())?;
    Some((
        selected["url"].as_str()?.to_string(),
        selected["filename"].as_str()?.to_string(),
        selected["size"].as_u64().unwrap_or(0),
    ))
}

/// Upstream chooses the filename, so treat it as untrusted.
fn sanitise_file_name(file_name: &str) -> AppResult<String> {
    let trimmed = file_name.trim();
    let invalid = trimmed.is_empty()
        || trimmed.contains(['/', '\\'])
        || trimmed.contains("..")
        || !trimmed.ends_with(".jar");
    if invalid {
        return Err(AppError::new(ErrorCode::InvalidInput).with("file", file_name));
    }
    Ok(trimmed.to_string())
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

    #[test]
    fn vanilla_takes_no_addons() {
        assert!(compatibility(ServerType::Vanilla).is_none());
    }

    #[test]
    fn neoforge_prefers_its_own_build_over_forge() {
        let loaders = compatibility(ServerType::NeoForge).expect("mods").loaders;
        let both = vec!["forge".to_string(), "neoforge".to_string()];
        assert_eq!(loader_rank(&both, loaders), Some(0));
        assert_eq!(loader_rank(&["forge".to_string()], loaders), None);
    }

    #[test]
    fn paper_accepts_spigot_and_bukkit_plugins_in_priority_order() {
        let loaders = compatibility(ServerType::Paper).expect("plugins").loaders;
        assert_eq!(loader_rank(&["paper".to_string()], loaders), Some(0));
        assert_eq!(loader_rank(&["bukkit".to_string()], loaders), Some(2));
    }

    #[test]
    fn primary_file_is_preferred_over_the_first_one() {
        let files = serde_json::json!([
            { "url": "u1", "filename": "sources.jar", "size": 1, "primary": false },
            { "url": "u2", "filename": "mod.jar", "size": 2, "primary": true }
        ]);
        let picked = primary_file(files.as_array().expect("array")).expect("a file");
        assert_eq!(picked.1, "mod.jar");
    }

    #[test]
    fn upstream_filenames_that_try_to_escape_are_rejected() {
        for name in ["../evil.jar", "sub/dir.jar", "notajar.txt", "  "] {
            assert!(sanitise_file_name(name).is_err(), "{name}");
        }
        assert_eq!(sanitise_file_name(" fabric-api.jar ").expect("ok"), "fabric-api.jar");
    }

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

        let listed = list_installed(&dir).expect("list");
        assert_eq!(listed.len(), 1);
        assert!(listed[0].enabled);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
