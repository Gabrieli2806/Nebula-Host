//! Client loader installation using the publishers' metadata and installer recipes.
//! Server-only processors are never run. Generated client jars stay in the verified
//! loader cache, shared by profiles with the same Minecraft and loader version.
use super::{client::{cached, check_cancel, failure, fetch, string}, Loader, Profile};
use crate::{error::{AppError, AppResult, ErrorCode}, runtime::java, store::paths};
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Read, path::{Path, PathBuf}, sync::atomic::AtomicBool};

pub async fn versions(loader: &Loader, minecraft: &str) -> AppResult<Vec<String>> {
    if minecraft.len() > 64 || !minecraft.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c)) {
        return Err(AppError::new(ErrorCode::InvalidInput));
    }
    let downloader = crate::game::minecraft::downloader::Downloader::new();
    match loader {
        Loader::Vanilla => Ok(Vec::new()),
        Loader::Fabric => {
            let entries: Value = crate::game::http_client().get(format!("https://meta.fabricmc.net/v2/versions/loader/{minecraft}"))
                .send().await?.error_for_status()?.json().await?;
            Ok(entries.as_array().into_iter().flatten()
                .filter(|e| e["loader"]["stable"] == true)
                .filter_map(|e| e["loader"]["version"].as_str().map(String::from)).take(20).collect())
        }
        Loader::Forge => downloader.get_forge_loader_versions(minecraft).await,
        Loader::Neoforge => downloader.get_neoforge_loader_versions(minecraft).await,
        Loader::Quilt => Ok(Vec::new()),
    }
}

pub fn maven_path(coordinate: &str) -> AppResult<String> {
    let (coordinate, extension) = coordinate.split_once('@').unwrap_or((coordinate, "jar"));
    let pieces: Vec<_> = coordinate.split(':').collect();
    if !(3..=4).contains(&pieces.len()) || pieces.iter().chain(std::iter::once(&extension))
        .any(|p| p.is_empty() || *p == "." || *p == ".." || !p.chars().all(|c| c.is_ascii_alphanumeric() || ".-_+".contains(c))) {
        return Err(failure("Invalid Maven coordinate"));
    }
    if pieces[0].split('.').any(str::is_empty) { return Err(failure("Invalid Maven group")); }
    let classifier = pieces.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    Ok(format!("{}/{}/{}/{}-{}{}.{}", pieces[0].replace('.', "/"), pieces[1], pieces[2], pieces[1], pieces[2], classifier, extension))
}

pub async fn artifact(library: &Value, root: &Path, cancel: &AtomicBool) -> AppResult<PathBuf> {
    let mut descriptor = library["downloads"]["artifact"].clone();
    if !descriptor.is_object() {
        let path = maven_path(string(library, "name")?)?;
        let repository = library["url"].as_str().unwrap_or("https://libraries.minecraft.net/");
        descriptor = json!({"path": path, "url": format!("{}/{}", repository.trim_end_matches('/'), path)});
    }
    let destination = paths::resolve_inside(root, string(&descriptor, "path")?)?;
    if descriptor["url"].as_str().unwrap_or_default().is_empty() {
        if !destination.is_file() { return Err(failure("Missing installer-generated library")); }
        if let Some(hash) = descriptor["sha1"].as_str() {
            if !cached(&destination, hash, descriptor["size"].as_u64()) { return Err(AppError::new(ErrorCode::ChecksumMismatch)); }
        }
        return Ok(destination);
    }
    download(&mut descriptor, &destination, cancel).await?;
    Ok(destination)
}

async fn download(descriptor: &mut Value, path: &Path, cancel: &AtomicBool) -> AppResult<()> {
    let url = string(descriptor, "url")?.to_owned();
    super::client::validate_download_url(&url)?;
    if descriptor["sha1"].as_str().filter(|s| !s.is_empty()).is_none() {
        let response = crate::game::http_client().get(format!("{url}.sha1"))
            .send().await?.error_for_status()?;
        let text = response.text().await?;
        let hash = text.split_whitespace().next().unwrap_or_default();
        super::client::validate_hash(hash)?;
        descriptor["sha1"] = json!(hash);
    }
    fetch(descriptor, path, cancel).await
}

/// Returns a version overlay; Vanilla metadata remains the parent.
pub async fn install(profile: &Profile, root: &Path, java_path: &Path, client_jar: &Path, cancel: &AtomicBool) -> AppResult<Value> {
    if matches!(profile.loader, Loader::Vanilla) { return Ok(json!({})); }
    let version = match profile.loader_version.as_deref().filter(|s| !s.is_empty()) {
        Some(version) => version.to_string(),
        None => versions(&profile.loader, &profile.game_version).await?.into_iter().next()
            .ok_or_else(|| AppError::new(ErrorCode::VersionNotFound))?,
    };
    let loader = match profile.loader { Loader::Fabric => "fabric", Loader::Forge => "forge", Loader::Neoforge => "neoforge", _ => return Err(AppError::new(ErrorCode::GameNotSupported)) };
    let installation = paths::resolve_inside(root, &format!("loader-cache/{loader}/{version}/{}", profile.game_version))?;
    std::fs::create_dir_all(&installation)?;
    if matches!(profile.loader, Loader::Fabric) {
        let metadata: Value = crate::game::http_client().get(format!("https://meta.fabricmc.net/v2/versions/loader/{}/{version}/profile/json", profile.game_version))
            .send().await?.error_for_status()?.json().await?;
        if metadata["inheritsFrom"] != profile.game_version { return Err(failure("Loader Minecraft version mismatch")); }
        return Ok(metadata);
    }
    let (repository, coordinate) = if matches!(profile.loader, Loader::Forge) {
        ("https://maven.minecraftforge.net", format!("net/minecraftforge/forge/{version}/forge-{version}-installer.jar"))
    } else {
        ("https://maven.neoforged.net/releases", format!("net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"))
    };
    let installer = installation.join("installer.jar");
    download(&mut json!({"url":format!("{repository}/{coordinate}")}), &installer, cancel).await?;
    let (recipe, overlay) = {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&installer)?).map_err(AppError::unexpected)?;
        let recipe: Value = serde_json::from_slice(&zip_file(&mut archive, "install_profile.json")?)?;
        let overlay: Value = serde_json::from_slice(&zip_file(&mut archive, "version.json")?)?;
        if recipe["minecraft"] != profile.game_version || overlay["inheritsFrom"] != profile.game_version {
            return Err(failure("Installer Minecraft version mismatch"));
        }
        // Only embedded Maven artifacts and the selected client's data are extracted.
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(AppError::unexpected)?;
            if entry.is_dir() || !(entry.name().starts_with("maven/") || entry.name().starts_with("data/")) { continue; }
            if entry.size() > 256 * 1024 * 1024 || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                return Err(failure("Unsafe installer entry"));
            }
            let target = if let Some(path) = entry.name().strip_prefix("maven/") {
                paths::resolve_inside(&root.join("libraries"), path)?
            } else { paths::resolve_inside(&installation, entry.name())? };
            std::fs::create_dir_all(target.parent().ok_or_else(|| failure("Invalid installer path"))?)?;
            std::io::copy(&mut entry, &mut std::fs::File::create(target)?)?;
        }
        (recipe, overlay)
    };
    let libraries = root.join("libraries");
    for library in recipe["libraries"].as_array().ok_or_else(|| failure("Missing installer libraries"))? {
        check_cancel(cancel)?;
        artifact(library, &libraries, cancel).await?;
    }
    let mut data = BTreeMap::from([
        ("SIDE".to_owned(), "client".to_owned()),
        ("ROOT".into(), root.display().to_string()),
        ("LIBRARY_DIR".into(), libraries.display().to_string()),
        ("MINECRAFT_JAR".into(), client_jar.display().to_string()),
        ("MINECRAFT_VERSION".into(), profile.game_version.clone()),
        ("INSTALLER".into(), installer.display().to_string()),
    ]);
    if let Some(entries) = recipe["data"].as_object() {
        for (key, values) in entries {
            if let Some(value) = values["client"].as_str() {
                let value = if value.starts_with('/') { paths::resolve_inside(&installation, value.trim_start_matches('/'))?.display().to_string() } else { value.to_string() };
                data.insert(key.to_owned(), value);
            }
        }
    }
    let processors = recipe["processors"].as_array().ok_or_else(|| failure("Missing installer processors"))?;
    for (index, processor) in processors.iter().enumerate() {
        check_cancel(cancel)?;
        if processor["sides"].as_array().is_some_and(|sides| !sides.iter().any(|s| s == "client")) { continue; }
        let outputs: Vec<_> = processor["outputs"].as_object().into_iter().flatten().map(|(path, hash)| {
            Ok((processor_value(path, &data, &libraries)?, processor_value(hash.as_str().ok_or_else(|| failure("Invalid processor output"))?, &data, &libraries)?))
        }).collect::<AppResult<_>>()?;
        if !outputs.is_empty() && outputs.iter().all(|(path, hash)| cached(Path::new(path), hash, None)) { continue; }
        let jar = paths::resolve_inside(&libraries, &maven_path(string(processor, "jar")?)?)?;
        let mut classpath = vec![jar.clone()];
        for coordinate in processor["classpath"].as_array().into_iter().flatten() {
            classpath.push(paths::resolve_inside(&libraries, &maven_path(coordinate.as_str().ok_or_else(|| failure("Invalid processor classpath"))?)?)?);
        }
        if classpath.iter().any(|p| !p.is_file()) { return Err(failure("Missing processor dependency")); }
        let manifest = {
            let mut archive = zip::ZipArchive::new(std::fs::File::open(&jar)?).map_err(AppError::unexpected)?;
            String::from_utf8(zip_file(&mut archive, "META-INF/MANIFEST.MF")?).map_err(AppError::unexpected)?
        };
        let manifest = manifest.replace("\r\n ", "").replace("\n ", "");
        let main = manifest.lines().find_map(|line| line.strip_prefix("Main-Class: ")).map(str::trim)
            .ok_or_else(|| failure("Missing processor main class"))?;
        let args: Vec<_> = processor["args"].as_array().into_iter().flatten().map(|arg| {
            processor_value(arg.as_str().ok_or_else(|| failure("Invalid processor argument"))?, &data, &libraries)
        }).collect::<AppResult<_>>()?;
        let log_path = installation.join(format!("processor-{index}.log"));
        let log = std::fs::File::create(&log_path)?;
        let mut command: tokio::process::Command = java::java_command(java_path).into();
        command.args(["-Xmx2048M", "-cp"]).arg(std::env::join_paths(&classpath).map_err(AppError::unexpected)?)
            .arg(main).args(args).current_dir(root).stdin(std::process::Stdio::null())
            .stdout(log.try_clone()?).stderr(log).kill_on_drop(true);
        let mut child = command.spawn().map_err(|e| failure(&format!("Cannot start client processor: {e}")))?;
        let status = tokio::select! {
            result = child.wait() => result?,
            _ = tokio::time::sleep(std::time::Duration::from_secs(1200)) => return Err(failure("Client processor timed out")),
            _ = async { while !cancel.load(std::sync::atomic::Ordering::Relaxed) { tokio::time::sleep(std::time::Duration::from_millis(200)).await; } } => return Err(AppError::new(ErrorCode::Cancelled)),
        };
        if !status.success() { return Err(failure(&format!("Client processor {index} failed; see {}", log_path.display()))); }
        for (path, hash) in outputs {
            if !cached(Path::new(&path), &hash, None) { return Err(AppError::new(ErrorCode::ChecksumMismatch).with("file", path)); }
        }
    }
    Ok(overlay)
}

fn processor_value(text: &str, data: &BTreeMap<String, String>, libraries: &Path) -> AppResult<String> {
    let mut value = text.to_owned();
    for _ in 0..16 {
        let previous = value.clone();
        for (key, replacement) in data { value = value.replace(&format!("{{{key}}}"), replacement); }
        if value == previous { break; }
    }
    if value.contains('{') || value.contains('}') { return Err(failure("Unknown installer variable")); }
    if let Some(coordinate) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        return Ok(paths::resolve_inside(libraries, &maven_path(coordinate)?)?.display().to_string());
    }
    Ok(value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')).unwrap_or(&value).to_owned())
}

fn zip_file(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> AppResult<Vec<u8>> {
    let entry = archive.by_name(name).map_err(AppError::unexpected)?;
    if entry.size() > 8 * 1024 * 1024 { return Err(failure("Installer metadata too large")); }
    let mut bytes = Vec::new();
    entry.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub fn merge(base: &mut Value, overlay: Value) -> AppResult<()> {
    if let Some(main) = overlay.get("mainClass") { base["mainClass"] = main.clone(); }
    for kind in ["jvm", "game"] {
        if let Some(extra) = overlay["arguments"][kind].as_array() {
            base["arguments"][kind].as_array_mut().ok_or_else(|| failure("Missing parent arguments"))?.extend(extra.clone());
        }
    }
    if let Some(extra) = overlay["libraries"].as_array() {
        let libraries = base["libraries"].as_array_mut().ok_or_else(|| failure("Missing parent libraries"))?;
        for library in extra {
            let key = |v: &Value| {
                let name = v["name"].as_str().unwrap_or_default();
                let (coordinate, extension) = name.split_once('@').unwrap_or((name, "jar"));
                let pieces: Vec<_> = coordinate.split(':').collect();
                format!("{}:{}:{}@{}", pieces.first().unwrap_or(&""), pieces.get(1).unwrap_or(&""), pieces.get(3).unwrap_or(&""), extension)
            };
            let identity = key(library);
            libraries.retain(|existing| key(existing) != identity);
            libraries.push(library.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coordinates_reject_traversal_and_keep_classifiers() {
        assert_eq!(maven_path("net.neoforged:neoform:1.21.4:mappings@txt").unwrap(), "net/neoforged/neoform/1.21.4/neoform-1.21.4-mappings.txt");
        for invalid in ["a:b:../bad", "..:b:c", "a::c", "a:b:c@../../zip"] { assert!(maven_path(invalid).is_err(), "{invalid}"); }
    }
    #[test]
    fn processor_variables_expand_recursively_and_fail_closed() {
        let data = BTreeMap::from([("A".into(), "{B}".into()), ("B".into(), "'value'".into())]);
        assert_eq!(processor_value("{A}", &data, Path::new("libs")).unwrap(), "value");
        assert!(processor_value("{MISSING}", &data, Path::new("libs")).is_err());
    }
    #[test]
    fn overlay_replaces_conflicting_libraries_without_losing_game_arguments() {
        let mut base = json!({"mainClass":"Vanilla", "arguments":{"jvm":["-cp","${classpath}"],"game":["--demo"]}, "libraries":[{"name":"org.test:asm:1"}]});
        merge(&mut base, json!({"mainClass":"Loader","libraries":[{"name":"org.test:asm:2"}],"arguments":{"game":["--loader"]}})).unwrap();
        assert_eq!(base["libraries"].as_array().unwrap().len(), 1);
        assert_eq!(base["arguments"]["game"], json!(["--demo","--loader"]));
    }
}
