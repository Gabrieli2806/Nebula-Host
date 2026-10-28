//! Resolving and fetching Minecraft server builds.
//!
//! Every URL here is a real, verified integration. Notably: PaperMC's v2 API
//! is dead — Fill v3 is the one that answers — and NeoForge's version scheme
//! needs care (see [`neoforge_prefix_for_mc`]).

use std::path::Path;

use nebula_downloader::{download_file, Checksum, DownloadOptions};
use reqwest::Client;
use serde_json::Value;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::registry::{InstallProgress, ProgressSink};
use crate::runtime::java::configure_no_window_command;

use super::types::ServerType;

/// PaperMC's v2 API is deprecated and shutting down; Fill v3 is current.
const PAPER_FILL_API: &str = "https://fill.papermc.io/v3";
const FABRIC_API: &str = "https://meta.fabricmc.net/v2";
/// Fabric retired installer 1.0.1; 1.1.1+ generates the current server launcher.
const FABRIC_INSTALLER: &str = "1.1.1";
const MOJANG_MANIFEST: &str = "https://launchermeta.mojang.com/mc/game/version_manifest.json";
const FORGE_MAVEN_METADATA: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
const NEOFORGE_VERSIONS: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
const PURPUR_API: &str = "https://api.purpurmc.org/v2/purpur";

pub struct Downloader {
    client: Client,
}

/// A resolved artefact: where it is, and what it should hash to.
/// Forge's installer for one build, on its Maven (server and launcher).
pub fn forge_installer_url(build: &str) -> String {
    format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{build}/forge-{build}-installer.jar")
}

/// NeoForge's installer for one build, on its Maven (server and launcher).
pub fn neoforge_installer_url(build: &str) -> String {
    format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{build}/neoforge-{build}-installer.jar")
}

/// The SHA-1 a Maven repository publishes beside an artifact (`<url>.sha1`),
/// so installers are verified like every other download.
pub async fn maven_sha1(client: &Client, url: &str) -> AppResult<String> {
    let text = client.get(format!("{url}.sha1")).send().await?.error_for_status()?.text().await?;
    let hash = text.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AppError::new(ErrorCode::ChecksumMismatch).with("file", url).detail("Invalid published SHA-1"));
    }
    Ok(hash)
}

#[derive(Debug)]
struct DownloadTarget {
    url: String,
    checksum: Option<Checksum>,
}

impl Downloader {
    pub fn new() -> Self {
        Self { client: crate::game::http_client() }
    }

    pub fn with_client(client: Client) -> Self {
        Self { client }
    }

    // -- versions ----------------------------------------------------------

    /// Game versions this distribution can be installed at, newest first.
    pub async fn get_versions(&self, server_type: ServerType) -> AppResult<Vec<String>> {
        match server_type {
            ServerType::Vanilla => self.vanilla_versions().await,
            ServerType::Paper => self.paper_versions().await,
            ServerType::Fabric => self.fabric_versions().await,
            ServerType::Forge => self.forge_versions().await,
            ServerType::NeoForge => self.neoforge_versions().await,
            ServerType::Purpur => self.purpur_versions().await,
            // Spigot has no build API worth trusting; it is installed from a
            // jar the user supplies.
            ServerType::Spigot => Ok(Vec::new()),
        }
    }

    async fn vanilla_versions(&self) -> AppResult<Vec<String>> {
        let manifest: Value = self.get_json(MOJANG_MANIFEST).await?;
        Ok(manifest["versions"]
            .as_array()
            .ok_or_else(|| upstream("Mojang manifest has no version list"))?
            .iter()
            .filter(|v| v["type"].as_str() == Some("release"))
            .filter_map(|v| v["id"].as_str().map(String::from))
            .take(30)
            .collect())
    }

    /// Fill v3 groups versions by minor family:
    /// `{ "versions": { "1.21": ["1.21.4", …] } }`.
    async fn paper_versions(&self) -> AppResult<Vec<String>> {
        let resp: Value = self.get_json(&format!("{PAPER_FILL_API}/projects/paper")).await?;

        let mut versions = Vec::new();
        if let Some(groups) = resp["versions"].as_object() {
            for list in groups.values() {
                if let Some(items) = list.as_array() {
                    versions.extend(items.iter().filter_map(|v| v.as_str().map(String::from)));
                }
            }
        } else if let Some(items) = resp["versions"].as_array() {
            // Tolerate a flat array if the shape ever changes back.
            versions.extend(items.iter().filter_map(|v| v.as_str().map(String::from)));
        }

        if versions.is_empty() {
            return Err(upstream("Fill returned no Paper versions"));
        }
        Ok(versions)
    }

    async fn fabric_versions(&self) -> AppResult<Vec<String>> {
        let resp: Vec<Value> = self.get_json(&format!("{FABRIC_API}/versions/game")).await?;
        Ok(resp
            .iter()
            .filter(|v| v["stable"].as_bool() == Some(true))
            .filter_map(|v| v["version"].as_str().map(String::from))
            .take(30)
            .collect())
    }

    async fn purpur_versions(&self) -> AppResult<Vec<String>> {
        let resp: Value = self.get_json(PURPUR_API).await?;
        Ok(resp["versions"]
            .as_array()
            .ok_or_else(|| upstream("Purpur returned no version list"))?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .rev()
            .collect())
    }

    async fn forge_versions(&self) -> AppResult<Vec<String>> {
        let metadata = self.get_text(FORGE_MAVEN_METADATA).await?;

        let mut versions: Vec<String> = Vec::new();
        for value in xml_tag_values(&metadata, "version").into_iter().rev() {
            let Some((mc_version, _)) = value.split_once('-') else { continue };
            if versions.iter().any(|existing| existing == mc_version) {
                continue;
            }
            versions.push(mc_version.to_string());
            if versions.len() >= 20 {
                break;
            }
        }

        if versions.is_empty() {
            return Err(upstream("Forge Maven listed no versions"));
        }
        Ok(versions)
    }

    async fn neoforge_version_list(&self) -> AppResult<Vec<String>> {
        let resp: Value = self.get_json(NEOFORGE_VERSIONS).await?;
        Ok(resp["versions"]
            .as_array()
            .ok_or_else(|| upstream("NeoForge Maven listed no versions"))?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect())
    }

    /// NeoForge versions are `MAJOR.MINOR.PATCH[-beta]`, where `MAJOR.MINOR`
    /// maps to Minecraft `1.MAJOR.MINOR` — except `MINOR == 0`, which maps to
    /// the un-suffixed release: NeoForge 21.0.x is Minecraft **1.21**, not
    /// 1.21.0.
    async fn neoforge_versions(&self) -> AppResult<Vec<String>> {
        let raw = self.neoforge_version_list().await?;

        let mut versions: Vec<String> = raw.iter().filter_map(|v| neoforge_to_mc(v)).collect();
        versions.sort_by(|a, b| super::compare_versions_desc(a, b));
        versions.dedup();
        versions.truncate(15);

        if versions.is_empty() {
            return Err(upstream("NeoForge Maven listed no usable versions"));
        }
        Ok(versions)
    }

    pub async fn get_fabric_loader_versions(&self) -> AppResult<Vec<String>> {
        let resp: Vec<Value> = self.get_json(&format!("{FABRIC_API}/versions/loader")).await?;
        Ok(resp
            .iter()
            .filter(|v| v["stable"].as_bool() == Some(true))
            .filter_map(|v| v["version"].as_str().map(String::from))
            .take(10)
            .collect())
    }

    pub async fn get_forge_loader_versions(&self, mc_version: &str) -> AppResult<Vec<String>> {
        let metadata = self.get_text(FORGE_MAVEN_METADATA).await?;
        let prefix = format!("{mc_version}-");
        let versions: Vec<String> = xml_tag_values(&metadata, "version")
            .into_iter()
            .rev()
            .filter(|value| value.starts_with(&prefix))
            .take(20)
            .collect();

        if versions.is_empty() {
            return Err(AppError::new(ErrorCode::VersionNotFound)
                .with("distribution", "forge")
                .with("version", mc_version));
        }
        Ok(versions)
    }

    /// NeoForge builds for one Minecraft version — newest first, stable before
    /// beta.
    pub async fn get_neoforge_loader_versions(&self, mc_version: &str) -> AppResult<Vec<String>> {
        let prefix = neoforge_prefix_for_mc(mc_version).ok_or_else(|| {
            AppError::new(ErrorCode::LoaderVersionMismatch)
                .with("distribution", "neoforge")
                .with("version", mc_version)
        })?;

        let raw = self.neoforge_version_list().await?;

        // Compare components numerically — recent builds have four parts, e.g.
        // "26.2.0.24-beta" — and keep betas behind stable releases.
        let mut stable: Vec<(Vec<u32>, String)> = Vec::new();
        let mut beta: Vec<(Vec<u32>, String)> = Vec::new();
        for version in raw {
            let Some(rest) = version.strip_prefix(&prefix) else { continue };
            let key: Vec<u32> = rest
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|value| value.parse().ok())
                .collect();
            if version.contains("-beta") {
                beta.push((key, version));
            } else {
                stable.push((key, version));
            }
        }
        stable.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        beta.sort_unstable_by(|a, b| b.0.cmp(&a.0));

        let versions: Vec<String> =
            stable.into_iter().chain(beta).map(|(_, version)| version).take(10).collect();

        if versions.is_empty() {
            return Err(AppError::new(ErrorCode::VersionNotFound)
                .with("distribution", "neoforge")
                .with("version", mc_version));
        }
        Ok(versions)
    }

    // -- install -----------------------------------------------------------

    /// Fetch (and, for the Forge family, run) the server build.
    ///
    /// Returns the loader version actually used, so "latest" can be recorded
    /// as the concrete build the instance now runs.
    pub async fn install(
        &self,
        server_type: ServerType,
        mc_version: &str,
        loader_version: Option<&str>,
        root: &Path,
        java: Option<&str>,
        progress: ProgressSink,
    ) -> AppResult<Option<String>> {
        progress.report(InstallProgress::Resolving);

        let (target, resolved_loader) =
            self.resolve_download(server_type, mc_version, loader_version).await?;

        let file_name = if server_type.uses_installer() {
            "server-installer.jar"
        } else {
            "server.jar"
        };
        let dest = root.join(file_name);

        let reporter = progress.clone();
        let label = file_name.to_string();
        let mut options = DownloadOptions::new(&target.url, dest.as_path()).with_progress(
            move |received, total| {
                reporter.report(InstallProgress::Downloading {
                    file: label.clone(),
                    received,
                    total: (total > 0).then_some(total),
                });
            },
        );
        if let Some(checksum) = target.checksum.as_ref() {
            options = options.with_checksum(checksum);
        }

        download_file(&crate::game::download_client(), options)
            .await
            .map_err(|error| crate::error::download_error(file_name, error))?;

        if server_type.uses_installer() {
            progress.report(InstallProgress::InstallingLoader {
                distribution: server_type.as_str().to_string(),
            });
            self.run_installer(server_type, mc_version, root, java)?;
        }

        progress.report(InstallProgress::Configuring);
        Ok(resolved_loader)
    }

    /// Run the Forge/NeoForge installer jar, which unpacks libraries and
    /// writes the arg file the launcher will use.
    fn run_installer(
        &self,
        server_type: ServerType,
        mc_version: &str,
        root: &Path,
        java: Option<&str>,
    ) -> AppResult<()> {
        let installer = root.join("server-installer.jar");
        let java = crate::runtime::java::resolve_java(
            java,
            super::launch::min_java_version(mc_version),
        )?;

        let mut command = std::process::Command::new(&java);
        let output = configure_no_window_command(&mut command)
            .arg("-jar")
            .arg(&installer)
            .arg("--installServer")
            .current_dir(root)
            .output()
            .map_err(|error| {
                AppError::new(ErrorCode::InstallFailed)
                    .with("distribution", server_type.as_str())
                    .with("java", java.display())
                    .detail(error)
            })?;

        if output.status.success() {
            let _ = std::fs::remove_file(&installer);
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        Err(AppError::new(ErrorCode::InstallFailed)
            .with("distribution", server_type.as_str())
            .detail(if detail.is_empty() { "installer produced no output".into() } else { detail }))
    }

    /// Where to fetch this build from, plus the loader version we settled on.
    async fn resolve_download(
        &self,
        server_type: ServerType,
        mc_version: &str,
        loader_version: Option<&str>,
    ) -> AppResult<(DownloadTarget, Option<String>)> {
        let requested = loader_version.map(str::trim).filter(|v| !v.is_empty());

        match server_type {
            ServerType::Vanilla => {
                let manifest: Value = self.get_json(MOJANG_MANIFEST).await?;
                let entry = manifest["versions"]
                    .as_array()
                    .ok_or_else(|| upstream("Mojang manifest has no version list"))?
                    .iter()
                    .find(|v| v["id"].as_str() == Some(mc_version))
                    .ok_or_else(|| {
                        AppError::new(ErrorCode::VersionNotFound)
                            .with("distribution", "vanilla")
                            .with("version", mc_version)
                    })?;

                let details_url = entry["url"]
                    .as_str()
                    .ok_or_else(|| upstream("Mojang version entry has no url"))?;
                let details: Value = self.get_json(details_url).await?;

                let url = details["downloads"]["server"]["url"]
                    .as_str()
                    .map(String::from)
                    .ok_or_else(|| {
                        AppError::new(ErrorCode::VersionNotFound)
                            .with("distribution", "vanilla")
                            .with("version", mc_version)
                    })?;
                // Mojang publishes a SHA-1 for the server jar; verify it.
                let checksum = details["downloads"]["server"]["sha1"]
                    .as_str()
                    .map(|hex| Checksum::Sha1(hex.to_string()));

                Ok((DownloadTarget { url, checksum }, None))
            }

            ServerType::Paper => {
                // Fill v3: the latest build carries a ready-to-use URL.
                let url =
                    format!("{PAPER_FILL_API}/projects/paper/versions/{mc_version}/builds/latest");
                let build: Value = self.get_json(&url).await?;
                let download = &build["downloads"]["server:default"];

                let url = download["url"].as_str().map(String::from).ok_or_else(|| {
                    AppError::new(ErrorCode::VersionNotFound)
                        .with("distribution", "paper")
                        .with("version", mc_version)
                })?;
                let checksum =
                    download["checksums"]["sha256"].as_str().map(|hex| Checksum::Sha256(hex.to_string()));

                Ok((DownloadTarget { url, checksum }, None))
            }

            ServerType::Purpur => {
                let build: Value = self.get_json(&format!("{PURPUR_API}/{mc_version}")).await?;
                let latest = build["builds"]["latest"].as_str().ok_or_else(|| {
                    AppError::new(ErrorCode::VersionNotFound)
                        .with("distribution", "purpur")
                        .with("version", mc_version)
                })?;
                Ok((
                    DownloadTarget {
                        url: format!("{PURPUR_API}/{mc_version}/{latest}/download"),
                        checksum: None,
                    },
                    None,
                ))
            }

            ServerType::Fabric => {
                let loader = match requested {
                    Some(value) => value.to_string(),
                    None => self
                        .get_fabric_loader_versions()
                        .await?
                        .first()
                        .cloned()
                        .ok_or_else(|| upstream("Fabric published no stable loader"))?,
                };
                Ok((
                    DownloadTarget {
                        url: format!(
                            "{FABRIC_API}/versions/loader/{mc_version}/{loader}/{FABRIC_INSTALLER}/server/jar"
                        ),
                        checksum: None,
                    },
                    Some(loader),
                ))
            }

            ServerType::Forge => {
                let loader = match requested {
                    Some(value) => value.to_string(),
                    None => self
                        .get_forge_loader_versions(mc_version)
                        .await?
                        .first()
                        .cloned()
                        .ok_or_else(|| {
                            AppError::new(ErrorCode::VersionNotFound)
                                .with("distribution", "forge")
                                .with("version", mc_version)
                        })?,
                };
                let url = forge_installer_url(&loader);
                let checksum = Some(Checksum::Sha1(maven_sha1(&self.client, &url).await?));
                Ok((DownloadTarget { url, checksum }, Some(loader)))
            }

            ServerType::NeoForge => {
                let loader = match requested {
                    Some(value) => value.to_string(),
                    None => self
                        .get_neoforge_loader_versions(mc_version)
                        .await?
                        .first()
                        .cloned()
                        .ok_or_else(|| {
                            AppError::new(ErrorCode::VersionNotFound)
                                .with("distribution", "neoforge")
                                .with("version", mc_version)
                        })?,
                };
                let url = neoforge_installer_url(&loader);
                let checksum = Some(Checksum::Sha1(maven_sha1(&self.client, &url).await?));
                Ok((DownloadTarget { url, checksum }, Some(loader)))
            }

            ServerType::Spigot => Err(AppError::new(ErrorCode::GameNotSupported)
                .with("distribution", "spigot")
                .detail("Spigot must be built locally; install it from a jar you supply")),
        }
    }

    // -- http --------------------------------------------------------------

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> AppResult<T> {
        Ok(self.client.get(url).send().await?.error_for_status()?.json().await?)
    }

    async fn get_text(&self, url: &str) -> AppResult<String> {
        Ok(self.client.get(url).send().await?.error_for_status()?.text().await?)
    }
}

impl Default for Downloader {
    fn default() -> Self {
        Self::new()
    }
}

fn upstream(detail: &str) -> AppError {
    AppError::new(ErrorCode::UpstreamUnavailable).detail(detail)
}

/// The NeoForge `MAJOR.MINOR.` prefix for a Minecraft version.
///
/// The trailing dot matters: without it, Minecraft 1.21 (prefix "21") matches
/// every `21.x.y` build and installs a loader for the wrong version, and the
/// server then refuses to start.
pub fn neoforge_prefix_for_mc(mc_version: &str) -> Option<String> {
    let parts: Vec<&str> = mc_version.trim().split('.').collect();
    let num = |i: usize| -> Option<u32> { parts.get(i)?.parse().ok() };
    if parts.first() == Some(&"1") {
        // Old scheme: Minecraft 1.21.4 → NeoForge 21.4.x; 1.21 → 21.0.x.
        let major = num(1)?;
        let minor = if parts.len() > 2 { num(2)? } else { 0 };
        return Some(format!("{major}.{minor}."));
    }
    // Year scheme (Minecraft 26.1 and later): NeoForge repeats the game
    // version and adds its build — 26.1 → 26.1.0.x, 26.1.2 → 26.1.2.x.
    let major = num(0)?;
    if major < 25 {
        return None;
    }
    let minor = num(1)?;
    let patch = if parts.len() > 2 { num(2)? } else { 0 };
    Some(format!("{major}.{minor}.{patch}."))
}

/// The Minecraft version a NeoForge version is built for — the inverse of
/// [`neoforge_prefix_for_mc`]. `21.4.12` → 1.21.4, `21.0.3` → 1.21,
/// `26.2.0.24-beta` → 26.2, `26.1.2.7` → 26.1.2.
pub fn neoforge_to_mc(neoforge: &str) -> Option<String> {
    let release = neoforge.split('-').next()?;
    let parts: Vec<u32> = release.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    match parts.as_slice() {
        [major, minor, patch, _build, ..] if *major >= 25 => Some(if *patch == 0 {
            format!("{major}.{minor}")
        } else {
            format!("{major}.{minor}.{patch}")
        }),
        [major, minor, _patch] if *major < 25 => Some(if *minor == 0 {
            format!("1.{major}")
        } else {
            format!("1.{major}.{minor}")
        }),
        _ => None,
    }
}

/// Pull the text of every `<tag>` out of a Maven metadata document.
pub fn xml_tag_values(document: &str, tag_name: &str) -> Vec<String> {
    let start_tag = format!("<{tag_name}>");
    let end_tag = format!("</{tag_name}>");
    let mut values = Vec::new();
    let mut cursor = 0usize;

    while let Some(start_offset) = document[cursor..].find(&start_tag) {
        let value_start = cursor + start_offset + start_tag.len();
        let Some(end_offset) = document[value_start..].find(&end_tag) else { break };
        let value_end = value_start + end_offset;
        let value = document[value_start..value_end].trim();
        if !value.is_empty() {
            values.push(value.to_string());
        }
        cursor = value_end + end_tag.len();
    }

    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_prefix_keeps_the_trailing_dot() {
        // Without the dot, MC 1.21 would match 21.4.x, 21.10.x and so on.
        assert_eq!(neoforge_prefix_for_mc("1.21").as_deref(), Some("21.0."));
        assert_eq!(neoforge_prefix_for_mc("1.21.4").as_deref(), Some("21.4."));
        assert_eq!(neoforge_prefix_for_mc("1.20.6").as_deref(), Some("20.6."));
    }

    #[test]
    fn neoforge_prefix_rejects_versions_it_cannot_map() {
        assert_eq!(neoforge_prefix_for_mc("24w45a"), None);
        assert_eq!(neoforge_prefix_for_mc("2.0"), None);
        assert_eq!(neoforge_prefix_for_mc("26.1").as_deref(), Some("26.1.0."));
        assert_eq!(neoforge_prefix_for_mc("26.1.2").as_deref(), Some("26.1.2."));
        assert_eq!(neoforge_to_mc("21.4.12").as_deref(), Some("1.21.4"));
        assert_eq!(neoforge_to_mc("21.0.3-beta").as_deref(), Some("1.21"));
        assert_eq!(neoforge_to_mc("26.2.0.24-beta").as_deref(), Some("26.2"));
        assert_eq!(neoforge_to_mc("26.1.2.7").as_deref(), Some("26.1.2"));
        assert_eq!(neoforge_to_mc("garbage"), None);
    }

    #[test]
    fn parses_maven_metadata() {
        let doc = "<metadata><versioning><versions>\
            <version>1.20.1-47.2.0</version><version>1.21-51.0.33</version>\
            <version>  </version></versions></versioning></metadata>";
        assert_eq!(
            xml_tag_values(doc, "version"),
            vec!["1.20.1-47.2.0".to_string(), "1.21-51.0.33".to_string()]
        );
    }

    #[test]
    fn parses_metadata_with_no_matching_tags() {
        assert!(xml_tag_values("<metadata></metadata>", "version").is_empty());
    }

    #[tokio::test]
    async fn spigot_reports_that_it_cannot_be_fetched() {
        let downloader = Downloader::new();
        let err = downloader
            .resolve_download(ServerType::Spigot, "1.21.4", None)
            .await
            .expect_err("no Spigot build API");
        assert_eq!(err.code, ErrorCode::GameNotSupported);
    }
}
