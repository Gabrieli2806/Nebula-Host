//! Content for client instances: mods, resource packs and shaders from
//! Modrinth, installed into one instance's own folders, and whole modpacks
//! turned into a new instance.
//!
//! Everything fetched is checked: the file must come from a known host, match
//! the SHA-1 Modrinth publishes, and land inside the instance folder under a
//! plain file name. Nothing is run here; the files only sit in the folders
//! the game reads.

use super::{client, ContentKind, Loader, Profile};
use crate::error::{AppError, AppResult, ErrorCode};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const API: &str = "https://api.modrinth.com/v2";
/// Hosts a Modrinth file or modpack entry may be downloaded from.
const FILE_HOSTS: [&str; 4] = ["cdn.modrinth.com", "github.com", "raw.githubusercontent.com", "gitlab.com"];
const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_UNPACKED: u64 = 2 * 1024 * 1024 * 1024;

fn invalid(reason: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput).with("reason", reason)
}

/// A Modrinth project id or slug: letters, digits, `-` and `_`.
fn project_ref(value: &str) -> AppResult<&str> {
    if value.is_empty() || value.len() > 64 || !value.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)) {
        return Err(invalid("project"));
    }
    Ok(value)
}

fn folder(kind: ContentKind) -> AppResult<&'static str> {
    match kind {
        ContentKind::Mod => Ok("mods"),
        ContentKind::Resourcepack => Ok("resourcepacks"),
        ContentKind::Shader => Ok("shaderpacks"),
        ContentKind::Modpack => Err(invalid("modpack")),
    }
}

/// A file name from the outside: no folders, no dots-only names, sane length.
fn file_name(value: &str) -> AppResult<&str> {
    let ok = !value.is_empty()
        && value.len() <= 200
        && value != "."
        && value != ".."
        && !value.chars().any(|c| c.is_control() || "/\\:*?\"<>|".contains(c));
    if ok { Ok(value) } else { Err(invalid("file name")) }
}

/// A relative path from a modpack: only plain components, nothing that climbs.
fn relative_path(value: &str) -> AppResult<PathBuf> {
    let path = Path::new(value);
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => return Err(invalid("path")),
        }
    }
    if out.as_os_str().is_empty() || value.contains('\\') || value.contains(':') {
        return Err(invalid("path"));
    }
    Ok(out)
}

fn profile_dir(profile: &Profile) -> AppResult<PathBuf> {
    Ok(client::directory()?.join("profiles").join(profile.id.to_string()))
}

pub(super) fn loader_tag(loader: &Loader) -> Option<&'static str> {
    loader_name(loader)
}

fn loader_name(loader: &Loader) -> Option<&'static str> {
    match loader {
        Loader::Fabric => Some("fabric"),
        Loader::Forge => Some("forge"),
        Loader::Neoforge => Some("neoforge"),
        Loader::Quilt => Some("quilt"),
        Loader::Vanilla => None,
    }
}

// -- what is installed ---------------------------------------------------------

#[derive(Serialize)]
pub struct Installed {
    pub kind: &'static str,
    pub file: String,
    pub size: u64,
    pub enabled: bool,
    /// The Modrinth project the file came from, when Nebula installed it.
    pub project_id: Option<String>,
}

/// Which Modrinth project each installed file came from, so Discover can say
/// "Installed". Keyed by file name without the `.disabled` suffix.
fn sources_path(profile: &Profile) -> AppResult<PathBuf> {
    Ok(profile_dir(profile)?.join(".nebula-content.json"))
}
fn read_sources(profile: &Profile) -> std::collections::BTreeMap<String, String> {
    sources_path(profile)
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
fn record_sources(profile: &Profile, files: &[(String, String)]) -> AppResult<()> {
    if files.is_empty() {
        return Ok(());
    }
    let mut sources = read_sources(profile);
    for (file, project) in files {
        sources.insert(file.clone(), project.clone());
    }
    let path = sources_path(profile)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(&sources)?)?;
    Ok(())
}

const KINDS: [(&str, ContentKind, &str); 3] = [
    ("mod", ContentKind::Mod, "mods"),
    ("resourcepack", ContentKind::Resourcepack, "resourcepacks"),
    ("shader", ContentKind::Shader, "shaderpacks"),
];

pub fn list(profile: &Profile) -> AppResult<Vec<Installed>> {
    let root = profile_dir(profile)?;
    let sources = read_sources(profile);
    let mut out = Vec::new();
    for (kind, _, dir) in KINDS {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else { continue };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            let name = entry.file_name().to_string_lossy().to_string();
            if !meta.is_file() || name.starts_with('.') || name.ends_with(".nebula-part") {
                continue;
            }
            let enabled = !name.ends_with(".disabled");
            let project_id = sources.get(name.trim_end_matches(".disabled")).cloned();
            out.push(Installed { kind, file: name, size: meta.len(), enabled, project_id });
        }
    }
    out.sort_by(|a, b| a.kind.cmp(b.kind).then_with(|| a.file.to_lowercase().cmp(&b.file.to_lowercase())));
    Ok(out)
}

fn existing(profile: &Profile, kind: ContentKind, file: &str) -> AppResult<PathBuf> {
    let path = profile_dir(profile)?.join(folder(kind)?).join(file_name(file)?);
    if path.is_file() { Ok(path) } else { Err(AppError::new(ErrorCode::NotFound)) }
}

pub fn remove(profile: &Profile, kind: ContentKind, file: &str) -> AppResult<()> {
    Ok(std::fs::remove_file(existing(profile, kind, file)?)?)
}

/// A disabled mod keeps its place but gets a `.disabled` suffix, which the game
/// ignores; the same convention other launchers use.
pub fn set_enabled(profile: &Profile, kind: ContentKind, file: &str, enabled: bool) -> AppResult<()> {
    let path = existing(profile, kind, file)?;
    let name = file_name(file)?;
    let target = if enabled {
        match name.strip_suffix(".disabled") {
            Some(base) => path.with_file_name(base),
            None => return Ok(()),
        }
    } else if name.ends_with(".disabled") {
        return Ok(());
    } else {
        path.with_file_name(format!("{name}.disabled"))
    };
    Ok(std::fs::rename(path, target)?)
}

// -- icons ---------------------------------------------------------------------

/// The picture a mod, resource pack or shader carries, as a `data:` URL, or
/// `None` when it has none. Mods name theirs in their metadata (Fabric and
/// Quilt: `icon`; Forge and NeoForge: `logoFile`); resource packs use
/// `pack.png`. Only small PNGs are read, and nothing is run.
pub fn icon(profile: &Profile, kind: ContentKind, file: &str) -> AppResult<Option<String>> {
    let path = existing(profile, kind, file)?;
    let Ok(handle) = std::fs::File::open(path) else { return Ok(None) };
    let Ok(mut zip) = zip::ZipArchive::new(handle) else { return Ok(None) };
    let name = match kind {
        ContentKind::Resourcepack => Some("pack.png".to_string()),
        ContentKind::Mod => mod_icon_name(&mut zip),
        _ => None,
    };
    let Some(name) = name else { return Ok(None) };
    let name = name.trim_start_matches('/').replace('\\', "/");
    if name.split('/').any(|part| part == ".." || part.is_empty()) {
        return Ok(None);
    }
    let Ok(entry) = zip.by_name(&name) else { return Ok(None) };
    if entry.size() > 512 * 1024 {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    if entry.take(512 * 1024).read_to_end(&mut bytes).is_err() || !bytes.starts_with(b"\x89PNG") {
        return Ok(None);
    }
    use base64::Engine;
    Ok(Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}

fn read_text(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut text = String::new();
    zip.by_name(name).ok()?.take(1024 * 1024).read_to_string(&mut text).ok()?;
    Some(text)
}

fn mod_icon_name(zip: &mut zip::ZipArchive<std::fs::File>) -> Option<String> {
    for metadata in ["fabric.mod.json", "quilt.mod.json"] {
        if let Some(text) = read_text(zip, metadata) {
            let json: Value = serde_json::from_str(&text).ok()?;
            let icon = if metadata == "quilt.mod.json" { &json["quilt_loader"]["metadata"]["icon"] } else { &json["icon"] };
            return match icon {
                Value::String(path) => Some(path.clone()),
                // A size → path map: the largest picture.
                Value::Object(sizes) => sizes
                    .iter()
                    .filter_map(|(size, path)| Some((size.parse::<u32>().ok()?, path.as_str()?.to_string())))
                    .max_by_key(|(size, _)| *size)
                    .map(|(_, path)| path),
                _ => None,
            };
        }
    }
    for metadata in ["META-INF/mods.toml", "META-INF/neoforge.mods.toml"] {
        if let Some(text) = read_text(zip, metadata) {
            return text.lines().find_map(|line| {
                let line = line.trim();
                let value = line.strip_prefix("logoFile")?.trim_start().strip_prefix('=')?.trim();
                Some(value.trim_matches(|c| c == '"' || c == '\'').to_string())
            });
        }
    }
    None
}

// -- a file from outside --------------------------------------------------------

/// Add a file the person picked (a mod `.jar`, a resource pack or shader
/// `.zip`) to an instance. It must be a real archive of the right kind and
/// name; an existing file of the same name is replaced.
pub fn import_file(profile: &Profile, kind: ContentKind, name: &str, data: &[u8]) -> AppResult<String> {
    let name = file_name(name)?;
    let lower = name.to_lowercase();
    let allowed: &[&str] = match kind {
        ContentKind::Mod => &[".jar"],
        ContentKind::Resourcepack | ContentKind::Shader => &[".zip"],
        ContentKind::Modpack => return Err(invalid("modpack")),
    };
    if !allowed.iter().any(|ext| lower.ends_with(ext)) {
        return Err(invalid("file type"));
    }
    if data.len() as u64 > MAX_FILE || !data.starts_with(b"PK\x03\x04") {
        return Err(invalid("not a valid archive"));
    }
    let target = profile_dir(profile)?.join(folder(kind)?).join(name);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = target.with_extension("nebula-part");
    std::fs::write(&temp, data)?;
    std::fs::rename(&temp, &target)?;
    Ok(name.to_string())
}

// -- Modrinth ------------------------------------------------------------------

async fn get_json(url: &str, query: &[(&str, String)]) -> AppResult<Value> {
    Ok(crate::game::http_client().get(url).query(query).send().await?.error_for_status()?.json().await?)
}

/// The newest release of `project` that fits this instance (its Minecraft
/// version and, for mods, its loader); a beta or alpha only when no release fits.
async fn best_version(profile: &Profile, project: &str, kind: ContentKind) -> AppResult<Value> {
    let mut query = vec![("game_versions", serde_json::json!([profile.game_version]).to_string())];
    if matches!(kind, ContentKind::Mod) {
        let loader = loader_name(&profile.loader).ok_or_else(|| invalid("A vanilla instance has no mod loader"))?;
        query.push(("loaders", serde_json::json!([loader]).to_string()));
    }
    let versions = get_json(&format!("{API}/project/{}/version", project_ref(project)?), &query).await?;
    let list = versions.as_array().cloned().unwrap_or_default();
    list.iter()
        .find(|v| v["version_type"] == "release")
        .or_else(|| list.first())
        .cloned()
        .ok_or_else(|| AppError::new(ErrorCode::VersionNotFound).with("version", &profile.game_version))
}

struct RemoteFile {
    url: reqwest::Url,
    name: String,
    sha1: String,
}

fn primary_file(version: &Value) -> AppResult<RemoteFile> {
    let files = version["files"].as_array().ok_or_else(|| invalid("version has no files"))?;
    let file = files.iter().find(|f| f["primary"] == true).or_else(|| files.first()).ok_or_else(|| invalid("version has no files"))?;
    remote_file(file["url"].as_str().unwrap_or_default(), file["filename"].as_str().unwrap_or_default(), file["hashes"]["sha1"].as_str().unwrap_or_default())
}

fn remote_file(url: &str, name: &str, sha1: &str) -> AppResult<RemoteFile> {
    let url = reqwest::Url::parse(url).map_err(|_| invalid("download address"))?;
    if url.scheme() != "https" || !url.host_str().is_some_and(|host| FILE_HOSTS.contains(&host)) {
        return Err(AppError::new(ErrorCode::UpstreamRejected).detail("Untrusted download host"));
    }
    if sha1.len() != 40 || !sha1.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid("checksum"));
    }
    Ok(RemoteFile { url, name: file_name(name)?.to_string(), sha1: sha1.to_lowercase() })
}

async fn download(file: &RemoteFile, destination: &Path) -> AppResult<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = destination.with_extension("nebula-part");
    let checksum = nebula_downloader::Checksum::Sha1(file.sha1.clone());
    let options = nebula_downloader::DownloadOptions::new(file.url.as_str(), &temp).with_checksum(&checksum).with_max_size(MAX_FILE);
    let client = crate::game::http_client();
    if nebula_downloader::download_file(&client, options).await.is_err() {
        let _ = std::fs::remove_file(&temp);
        return Err(AppError::new(ErrorCode::DownloadFailed).with("file", &file.name));
    }
    Ok(std::fs::rename(temp, destination)?)
}

/// Install a project (a mod, resource pack or shader) into an instance, with
/// the mods it requires. Returns the files that were added.
pub async fn install(profile: &Profile, project: &str, kind: ContentKind) -> AppResult<Vec<String>> {
    let root = profile_dir(profile)?;
    let mut added = Vec::new();
    let mut seen = HashSet::new();
    let mut sources = Vec::new();
    let mut queue = vec![(project.to_string(), kind, 0u8)];
    while let Some((project, kind, depth)) = queue.pop() {
        if !seen.insert(project.clone()) {
            continue;
        }
        let version = best_version(profile, &project, kind).await?;
        let file = primary_file(&version)?;
        let target = root.join(folder(kind)?).join(&file.name);
        if let Some(id) = version["project_id"].as_str() {
            sources.push((file.name.clone(), id.to_string()));
        }
        if !target.exists() && !target.with_file_name(format!("{}.disabled", file.name)).exists() {
            download(&file, &target).await?;
            added.push(file.name.clone());
        }
        // Required mods come along (two levels deep: enough for the usual
        // library mods, never a runaway chain).
        if matches!(kind, ContentKind::Mod) && depth < 2 {
            for dependency in version["dependencies"].as_array().into_iter().flatten() {
                if dependency["dependency_type"] == "required" {
                    if let Some(id) = dependency["project_id"].as_str() {
                        queue.push((id.to_string(), ContentKind::Mod, depth + 1));
                    }
                }
            }
        }
    }
    record_sources(profile, &sources)?;
    Ok(added)
}

// -- modpacks ------------------------------------------------------------------

/// Turn a Modrinth modpack into a new instance: the pack names the Minecraft
/// version and loader, lists every file with its checksum, and carries
/// override files (configs, options) that are copied over.
pub async fn install_modpack(project: &str) -> AppResult<Profile> {
    let versions = get_json(&format!("{API}/project/{}/version", project_ref(project)?), &[]).await?;
    let list = versions.as_array().cloned().unwrap_or_default();
    let version = list
        .iter()
        .find(|v| v["version_type"] == "release")
        .or_else(|| list.first())
        .ok_or_else(|| AppError::new(ErrorCode::VersionNotFound))?;
    let files = version["files"].as_array().ok_or_else(|| invalid("version has no files"))?;
    let pack = files
        .iter()
        .find(|f| f["filename"].as_str().is_some_and(|n| n.ends_with(".mrpack")))
        .ok_or_else(|| invalid("not a modpack file"))?;
    let remote = remote_file(pack["url"].as_str().unwrap_or_default(), pack["filename"].as_str().unwrap_or_default(), pack["hashes"]["sha1"].as_str().unwrap_or_default())?;

    let scratch = client::directory()?.join("downloads");
    let archive = scratch.join(format!("{}.mrpack", uuid::Uuid::new_v4()));
    download(&remote, &archive).await?;
    let result = build_from_pack(&archive, version["name"].as_str().unwrap_or("Modpack")).await;
    let _ = std::fs::remove_file(&archive);
    result
}

struct PackFile {
    path: PathBuf,
    remote: RemoteFile,
}

async fn build_from_pack(archive: &Path, fallback_name: &str) -> AppResult<Profile> {
    let (index, overrides) = {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?).map_err(AppError::unexpected)?;
        let mut text = String::new();
        zip.by_name("modrinth.index.json")
            .map_err(|_| invalid("not a Modrinth modpack"))?
            .take(8 * 1024 * 1024)
            .read_to_string(&mut text)?;
        (serde_json::from_str::<Value>(&text)?, zip.len())
    };
    let _ = overrides;
    let minecraft = index["dependencies"]["minecraft"].as_str().ok_or_else(|| invalid("pack has no Minecraft version"))?;
    let (loader, loader_version) = [
        ("fabric-loader", Loader::Fabric),
        ("neoforge", Loader::Neoforge),
        ("forge", Loader::Forge),
        ("quilt-loader", Loader::Quilt),
    ]
    .into_iter()
    .find_map(|(key, loader)| index["dependencies"][key].as_str().map(|v| (loader, Some(v.to_string()))))
    .unwrap_or((Loader::Vanilla, None));

    let name: String = index["name"].as_str().unwrap_or(fallback_name).chars().filter(|c| !c.is_control()).take(64).collect();
    let profile = super::create(super::CreateProfile {
        name: if name.trim().is_empty() { "Modpack".into() } else { name },
        game_version: minecraft.to_string(),
        loader,
        memory_mb: 4096,
    })
    .await?;
    // The pack pins its own loader build; the newest one would not match.
    let profile = match loader_version {
        Some(version) => super::set_loader_version(profile.id, version).await?,
        None => profile,
    };

    let outcome = fill_instance(&profile, archive, &index).await;
    if outcome.is_err() {
        let _ = super::remove(profile.id).await;
        if let Ok(dir) = profile_dir(&profile) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    outcome.map(|()| profile)
}

async fn fill_instance(profile: &Profile, archive: &Path, index: &Value) -> AppResult<()> {
    let root = profile_dir(profile)?;
    std::fs::create_dir_all(&root)?;

    let mut jobs = Vec::new();
    for entry in index["files"].as_array().into_iter().flatten() {
        // Files the pack marks as server-only are not for a client.
        if entry["env"]["client"] == "unsupported" {
            continue;
        }
        let path = relative_path(entry["path"].as_str().unwrap_or_default())?;
        let url = entry["downloads"].as_array().and_then(|d| d.first()).and_then(Value::as_str).unwrap_or_default();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let remote = remote_file(url, name, entry["hashes"]["sha1"].as_str().unwrap_or_default())?;
        jobs.push(PackFile { path: root.join(path), remote });
    }
    use futures_util::{stream, StreamExt};
    let mut pending = stream::iter(jobs.into_iter().map(|job| async move {
        if job.path.exists() { Ok(()) } else { download(&job.remote, &job.path).await }
    }))
    .buffer_unordered(6);
    while let Some(done) = pending.next().await {
        done?;
    }

    // Overrides: `overrides/` for everyone, `client-overrides/` for clients.
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?).map_err(AppError::unexpected)?;
    let mut unpacked = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(AppError::unexpected)?;
        let name = entry.name().replace('\\', "/");
        let Some(relative) = name.strip_prefix("overrides/").or_else(|| name.strip_prefix("client-overrides/")) else { continue };
        if entry.is_dir() || relative.is_empty() {
            continue;
        }
        let target = root.join(relative_path(relative)?);
        unpacked += entry.size();
        if unpacked > MAX_UNPACKED {
            return Err(invalid("modpack is too large"));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut entry.by_ref().take(MAX_FILE), &mut out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to Modrinth: run with `cargo test --lib live_ -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_picks_and_downloads_a_compatible_mod() {
        let profile = Profile {
            id: uuid::Uuid::new_v4(),
            name: "t".into(),
            game_version: "1.21.4".into(),
            loader: Loader::Fabric,
            memory_mb: 2048,
            loader_version: None,
            icon: None,
        };
        let version = best_version(&profile, "fabric-api", ContentKind::Mod).await.expect("a version");
        let file = primary_file(&version).expect("a safe file");
        let target = std::env::temp_dir().join(format!("nebula-live-{}", file.name));
        download(&file, &target).await.expect("downloads and matches its checksum");
        assert!(std::fs::metadata(&target).unwrap().len() > 1000);
        let _ = std::fs::remove_file(target);
        let pack = get_json(&format!("{API}/search"), &[("query", "fabulously optimized".into()), ("facets", "[[\"project_type:modpack\"]]".into()), ("limit", "1".into())]).await.unwrap();
        assert!(pack["hits"].as_array().is_some_and(|h| !h.is_empty()));
    }

    #[test]
    fn file_names_cannot_climb_or_hide_folders() {
        assert!(file_name("sodium-1.0.jar").is_ok());
        for bad in ["", ".", "..", "../x.jar", "a/b.jar", "a\\b.jar", "c:x.jar", "x\n.jar"] {
            assert!(file_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn pack_paths_stay_inside_the_instance() {
        assert_eq!(relative_path("mods/a.jar").unwrap(), PathBuf::from("mods/a.jar"));
        for bad in ["../a", "/etc/passwd", "mods/../../a", "C:/a", "mods\\a", ""] {
            assert!(relative_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn only_known_hosts_with_a_checksum_are_downloaded() {
        let sha = "a".repeat(40);
        assert!(remote_file("https://cdn.modrinth.com/data/x/versions/y/a.jar", "a.jar", &sha).is_ok());
        assert!(remote_file("http://cdn.modrinth.com/a.jar", "a.jar", &sha).is_err());
        assert!(remote_file("https://evil.example/a.jar", "a.jar", &sha).is_err());
        assert!(remote_file("https://cdn.modrinth.com/a.jar", "a.jar", "short").is_err());
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut out = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        for (name, bytes) in entries {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
        out.into_inner()
    }

    fn profile_in(dir: &Path) -> Profile {
        let _ = dir;
        Profile { id: uuid::Uuid::new_v4(), name: "t".into(), game_version: "1.21.4".into(), loader: Loader::Fabric, memory_mb: 2048, loader_version: None, icon: None }
    }

    #[test]
    fn a_picked_file_must_be_the_right_kind_of_archive() {
        let profile = profile_in(Path::new("."));
        let jar = zip_with(&[("fabric.mod.json", b"{}")]);
        assert!(import_file(&profile, ContentKind::Mod, "a.exe", &jar).is_err());
        assert!(import_file(&profile, ContentKind::Mod, "../a.jar", &jar).is_err());
        assert!(import_file(&profile, ContentKind::Mod, "a.jar", b"not a zip").is_err());
        assert!(import_file(&profile, ContentKind::Resourcepack, "a.jar", &jar).is_err());
        assert!(import_file(&profile, ContentKind::Modpack, "a.zip", &jar).is_err());
    }

    #[test]
    fn mod_icons_are_found_from_each_loader_s_metadata() {
        let dir = std::env::temp_dir().join(format!("nebula-icons-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let png: &[u8] = b"\x89PNG\r\n\x1a\nrest";
        let write = |name: &str, entries: &[(&str, &[u8])]| {
            let path = dir.join(name);
            std::fs::write(&path, zip_with(entries)).unwrap();
            zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap()
        };
        let mut fabric = write("f.jar", &[("fabric.mod.json", br#"{"icon":"assets/x/icon.png"}"#), ("assets/x/icon.png", png)]);
        assert_eq!(mod_icon_name(&mut fabric).as_deref(), Some("assets/x/icon.png"));
        let mut sized = write("s.jar", &[("fabric.mod.json", br#"{"icon":{"16":"a16.png","128":"a128.png"}}"#)]);
        assert_eq!(mod_icon_name(&mut sized).as_deref(), Some("a128.png"));
        let mut forge = write("g.jar", &[("META-INF/mods.toml", b"[[mods]]\nlogoFile=\"logo.png\"\n")]);
        assert_eq!(mod_icon_name(&mut forge).as_deref(), Some("logo.png"));
        let mut none = write("n.jar", &[("readme.txt", b"hi")]);
        assert_eq!(mod_icon_name(&mut none), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn project_references_are_plain() {
        assert!(project_ref("AANobbMI").is_ok());
        assert!(project_ref("fabric-api").is_ok());
        assert!(project_ref("../x").is_err());
        assert!(project_ref("").is_err());
    }
}

// -- updates and compatibility --------------------------------------------------

/// What Modrinth says about one installed file, for this instance's version
/// and loader.
#[derive(Serialize)]
pub struct ContentStatus {
    pub kind: &'static str,
    pub file: String,
    pub project_id: Option<String>,
    pub title: Option<String>,
    pub icon_url: Option<String>,
    /// The installed version's number, when Modrinth knows the file.
    pub version: Option<String>,
    /// False when the installed version is not made for this Minecraft
    /// version (or, for mods, this loader).
    pub compatible: bool,
    /// A newer file that fits, when there is one.
    pub update: Option<UpdateOffer>,
}
#[derive(Serialize)]
pub struct UpdateOffer {
    pub version_id: String,
    pub version_number: String,
    pub file_name: String,
}

fn sha1_of(path: &Path) -> Option<String> {
    use sha1::{Digest, Sha1};
    let mut file = std::fs::File::open(path).ok()?;
    let mut digest = Sha1::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).ok()?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Some(digest.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Look every installed file up on Modrinth by its hash (so even jars added
/// by hand are recognised), then ask which newer file fits this instance.
pub async fn check(profile: &Profile) -> AppResult<Vec<ContentStatus>> {
    let installed = list(profile)?;
    let root = profile_dir(profile)?;
    let mut hashes: Vec<(String, &Installed)> = Vec::new();
    for item in &installed {
        let dir = KINDS.iter().find(|(kind, _, _)| *kind == item.kind).map(|(_, _, dir)| *dir).unwrap_or("mods");
        let path = root.join(dir).join(&item.file);
        let item_ref = item;
        if let Some(hash) = tokio::task::spawn_blocking(move || sha1_of(&path)).await.map_err(AppError::unexpected)? {
            hashes.push((hash, item_ref));
        }
    }
    if hashes.is_empty() {
        return Ok(Vec::new());
    }
    let client = crate::game::http_client();
    let all: Vec<&str> = hashes.iter().map(|(h, _)| h.as_str()).collect();
    let known: Value = client
        .post(format!("{API}/version_files"))
        .json(&serde_json::json!({ "hashes": all, "algorithm": "sha1" }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut updates = serde_json::Map::new();
    for (kind, content_kind, _) in KINDS {
        let group: Vec<&str> = hashes.iter().filter(|(_, i)| i.kind == kind).map(|(h, _)| h.as_str()).collect();
        if group.is_empty() {
            continue;
        }
        let mut body = serde_json::json!({ "hashes": group, "algorithm": "sha1", "game_versions": [profile.game_version] });
        if matches!(content_kind, ContentKind::Mod) {
            match loader_name(&profile.loader) {
                Some(loader) => body["loaders"] = serde_json::json!([loader]),
                None => continue,
            }
        }
        let answer: Value = client
            .post(format!("{API}/version_files/update"))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if let Value::Object(map) = answer {
            updates.extend(map);
        }
    }
    // Project titles and icons in one request.
    let ids: Vec<String> = known
        .as_object()
        .map(|m| m.values().filter_map(|v| v["project_id"].as_str().map(String::from)).collect::<HashSet<_>>().into_iter().collect())
        .unwrap_or_default();
    let projects: Value = if ids.is_empty() {
        Value::Array(Vec::new())
    } else {
        get_json(&format!("{API}/projects"), &[("ids", serde_json::json!(ids).to_string())]).await?
    };
    let project = |id: &str| projects.as_array().and_then(|list| list.iter().find(|p| p["id"] == id)).cloned();

    let loader = loader_name(&profile.loader);
    let mut out = Vec::new();
    for (hash, item) in &hashes {
        let current = &known[hash.as_str()];
        let project_id = current["project_id"].as_str().map(String::from).or_else(|| item.project_id.clone());
        let fits_version = current["game_versions"].as_array().is_some_and(|v| v.iter().any(|g| g == profile.game_version.as_str()));
        let fits_loader = item.kind != "mod"
            || loader.is_some_and(|l| current["loaders"].as_array().is_some_and(|v| v.iter().any(|x| x == l)));
        let known_file = current.is_object();
        let update = updates.get(hash.as_str()).and_then(|next| {
            if next["id"] == current["id"] {
                return None;
            }
            let file = next["files"].as_array()?.iter().find(|f| f["primary"] == true).or_else(|| next["files"].as_array()?.first())?;
            Some(UpdateOffer {
                version_id: next["id"].as_str()?.into(),
                version_number: next["version_number"].as_str().unwrap_or_default().into(),
                file_name: file["filename"].as_str()?.into(),
            })
        });
        let info = project_id.as_deref().and_then(project);
        out.push(ContentStatus {
            kind: item.kind,
            file: item.file.clone(),
            project_id,
            title: info.as_ref().and_then(|p| p["title"].as_str().map(String::from)),
            icon_url: info.as_ref().and_then(|p| p["icon_url"].as_str().map(String::from)),
            version: current["version_number"].as_str().map(String::from),
            // Unknown files are not called incompatible: there is nothing to go on.
            compatible: !known_file || (fits_version && fits_loader),
            update,
        });
    }
    Ok(out)
}

/// Replace an installed file with the given Modrinth version (from `check`),
/// keeping it switched off if it was off.
pub async fn update(profile: &Profile, kind: ContentKind, file: &str, version_id: &str) -> AppResult<String> {
    let old = existing(profile, kind, file)?;
    let version = get_json(&format!("{API}/version/{}", project_ref(version_id)?), &[]).await?;
    let next = primary_file(&version)?;
    let disabled = file.ends_with(".disabled");
    let name = if disabled { format!("{}.disabled", next.name) } else { next.name.clone() };
    let target = profile_dir(profile)?.join(folder(kind)?).join(&name);
    download(&next, &target).await?;
    if target != old {
        let _ = std::fs::remove_file(&old);
    }
    if let Some(id) = version["project_id"].as_str() {
        record_sources(profile, &[(next.name.clone(), id.to_string())])?;
    }
    Ok(name)
}
