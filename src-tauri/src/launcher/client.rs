//! Windows x64 Vanilla client engine. Official metadata, verified cache, isolated
//! profile directories, argument arrays (never a shell), and supervised children.
use super::{Loader, Profile};
use crate::{
    error::{AppError, AppResult, ErrorCode},
    runtime::java,
    store::paths,
};
use futures_util::{stream, FutureExt, StreamExt, TryStreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{
    collections::{BTreeMap, HashMap},
    io::{BufRead, Read},
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

#[derive(Clone, Serialize, Default)]
pub struct ClientStatus {
    pub phase: String,
    pub stage: String,
    pub completed: u64,
    pub total: u64,
    pub error: Option<AppError>,
    pub exit_code: Option<i32>,
}

#[derive(Default)]
pub struct ClientEngine {
    operation: Mutex<()>,
    status: RwLock<HashMap<Uuid, ClientStatus>>,
    cancel: RwLock<HashMap<Uuid, Arc<AtomicBool>>>,
    children: Mutex<HashMap<Uuid, Child>>,
    logs: std::sync::Mutex<HashMap<Uuid, Vec<String>>>,
}

pub struct Credentials {
    pub name: String,
    pub uuid: String,
    pub token: String,
    pub demo: bool,
    pub expires: Option<std::time::Instant>,
}
pub fn supported() -> bool {
    cfg!(all(target_os = "windows", target_arch = "x86_64")) || cfg!(all(target_os = "android", target_arch = "aarch64", feature = "desktop"))
}
pub(super) fn failure(message: &str) -> AppError {
    AppError::new(ErrorCode::InstallFailed).detail(message)
}
pub(super) fn string<'a>(value: &'a Value, key: &str) -> AppResult<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| failure(&format!("Missing client metadata: {key}")))
}
pub(super) fn directory() -> AppResult<PathBuf> {
    let dir = paths::default_instances_dir()?.with_file_name("launcher");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
pub(super) fn check_cancel(cancel: &AtomicBool) -> AppResult<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(AppError::new(ErrorCode::Cancelled))
    } else {
        Ok(())
    }
}

impl ClientEngine {
    pub async fn statuses(&self) -> HashMap<Uuid, ClientStatus> {
        self.status.read().await.clone()
    }
    pub fn logs(&self, id: Uuid) -> Vec<String> {
        #[cfg(target_os = "android")]
        if let Ok(root) = directory() {
            use std::io::{Seek, SeekFrom};
            if let Ok(mut file) = std::fs::File::open(root.join("profiles").join(id.to_string()).join("latest-client.log")) {
                let length = file.metadata().map(|m| m.len()).unwrap_or(0);
                let _ = file.seek(SeekFrom::Start(length.saturating_sub(512 * 1024)));
                let mut bytes = Vec::new();
                if file.take(512 * 1024).read_to_end(&mut bytes).is_ok() {
                    let text = String::from_utf8_lossy(&bytes);
                    let mut lines: Vec<_> = text.lines().rev().take(300).map(|line| redact_line(line).chars().take(2000).collect()).collect();
                    lines.reverse();
                    return lines;
                }
            }
        }
        self.logs
            .lock()
            .map(|logs| logs.get(&id).cloned().unwrap_or_default())
            .unwrap_or_default()
    }
    fn log(&self, id: Uuid, line: String) {
        if let Ok(mut logs) = self.logs.lock() {
            let lines = logs.entry(id).or_default();
            if lines.len() >= 300 {
                lines.remove(0);
            }
            lines.push(line);
        }
    }
    async fn stage(&self, id: Uuid, stage: &str, completed: u64, total: u64) {
        self.status.write().await.insert(
            id,
            ClientStatus {
                phase: "installing".into(),
                stage: stage.into(),
                completed,
                total,
                ..Default::default()
            },
        );
    }
    pub async fn start(
        self: &Arc<Self>,
        profile: Profile,
        credentials: Option<Credentials>,
    ) -> AppResult<()> {
        if !supported() || matches!(profile.loader, Loader::Quilt) {
            return Err(AppError::new(ErrorCode::GameNotSupported)
                .with("platform", "Windows x64 / Vanilla, Fabric, Forge, NeoForge"));
        }
        let guard = self
            .operation
            .try_lock()
            .map_err(|_| AppError::new(ErrorCode::NodeBusy))?;
        let mut states = self.status.write().await;
        if states
            .get(&profile.id)
            .is_some_and(|s| matches!(s.phase.as_str(), "installing" | "running" | "stopping"))
        {
            return Err(AppError::new(ErrorCode::InstanceAlreadyRunning));
        }
        states.insert(
            profile.id,
            ClientStatus {
                phase: "installing".into(),
                stage: "metadata".into(),
                ..Default::default()
            },
        );
        drop(states);
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel.write().await.insert(profile.id, cancel.clone());
        drop(guard);
        let engine = self.clone();
        tokio::spawn(async move {
            let _guard = engine.operation.lock().await;
            let result = engine.prepare(&profile, &cancel).await;
            let result = match result {
                Ok(prepared) => {
                    if let Some(credentials) = credentials {
                        match check_cancel(&cancel) {
                            Ok(()) => engine
                                .spawn_game(&profile, prepared, credentials)
                                .await
                                .map(|_| true),
                            Err(error) => Err(error),
                        }
                    } else {
                        Ok(false)
                    }
                }
                Err(error) => Err(error),
            };
            engine.cancel.write().await.remove(&profile.id);
            let mut states = engine.status.write().await;
            states.insert(
                profile.id,
                match result {
                    Ok(running) => ClientStatus {
                        phase: if running { "running" } else { "ready" }.into(),
                        ..Default::default()
                    },
                    Err(error) => ClientStatus {
                        phase: if error.code == ErrorCode::Cancelled {
                            "cancelled"
                        } else {
                            "failed"
                        }
                        .into(),
                        error: Some(error),
                        ..Default::default()
                    },
                },
            );
            drop(states);
            drop(_guard);
            if matches!(result_phase(&engine, profile.id).await.as_str(), "running") {
                engine.watch(profile.id).await;
            }
        });
        Ok(())
    }
    /// Bring a running game's window back to the front (Android: the game is
    /// its own window and the person may have left it for the launcher).
    pub async fn resume(&self, id: Uuid) -> AppResult<()> {
        #[cfg(all(target_os = "android", feature = "desktop"))]
        if self.status.read().await.get(&id).is_some_and(|s| s.phase == "running") {
            return super::android::resume();
        }
        let _ = id;
        Ok(())
    }

    pub async fn stop(&self, id: Uuid) -> AppResult<()> {
        #[cfg(all(target_os = "android", feature = "desktop"))]
        if self.status.read().await.get(&id).is_some_and(|s| s.phase == "running") {
            super::android::stop()?;
            self.status.write().await.insert(id, ClientStatus { phase: "ready".into(), ..Default::default() });
            return Ok(());
        }
        if let Some(cancel) = self.cancel.read().await.get(&id) {
            cancel.store(true, Ordering::Relaxed);
            return Ok(());
        }
        if let Some(child) = self.children.lock().await.get_mut(&id) {
            child.kill()?;
            if let Some(state) = self.status.write().await.get_mut(&id) {
                state.phase = "stopping".into();
            }
        }
        Ok(())
    }
    async fn watch(&self, id: Uuid) {
        #[cfg(all(target_os = "android", feature = "desktop"))]
        {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if self.status.read().await.get(&id).is_some_and(|s| s.phase != "running") { return; }
                match super::android::running() {
                    Ok(true) => {},
                    Ok(false) => {
                        let code = directory().ok().and_then(|root| std::fs::read_to_string(root.join("profiles").join(id.to_string()).join(".nebula-game-exit")).ok()).and_then(|value| value.trim().parse::<i32>().ok()).unwrap_or(-1);
                        self.status.write().await.insert(id, ClientStatus {
                            phase: if code == 0 { "ready" } else { "failed" }.into(), exit_code: Some(code),
                            error: if code == 0 { None } else { Some(AppError::new(ErrorCode::InstanceCrashed).with("code", code)) },
                            ..Default::default()
                        });
                        return;
                    },
                    Err(error) => {
                        self.status.write().await.insert(id, ClientStatus { phase: "failed".into(), error: Some(error), ..Default::default() });
                        return;
                    }
                }
            }
        }
        #[cfg(not(target_os = "android"))]
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let mut children = self.children.lock().await;
            let Some(child) = children.get_mut(&id) else {
                break;
            };
            match child.try_wait() {
                Ok(Some(exit)) => {
                    children.remove(&id);
                    let mut states = self.status.write().await;
                    let expected =
                        exit.success() || states.get(&id).is_some_and(|s| s.phase == "stopping");
                    states.insert(
                        id,
                        ClientStatus {
                            phase: if expected { "ready" } else { "failed" }.into(),
                            exit_code: exit.code(),
                            error: if expected {
                                None
                            } else {
                                Some(
                                    AppError::new(ErrorCode::InstanceCrashed)
                                        .with("code", exit.code().unwrap_or(-1)),
                                )
                            },
                            ..Default::default()
                        },
                    );
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    self.log(id, format!("Process monitor: {error}"));
                    break;
                }
            }
        }
    }
    async fn prepare(&self, profile: &Profile, cancel: &Arc<AtomicBool>) -> AppResult<Prepared> {
        let root = directory()?;
        let version_dir = root.join("versions").join(&profile.game_version);
        let game_dir = root.join("profiles").join(profile.id.to_string());
        std::fs::create_dir_all(&version_dir)?;
        std::fs::create_dir_all(&game_dir)?;
        check_cancel(cancel)?;
        let manifest: Value = crate::game::http_client()
            .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let version = manifest["versions"]
            .as_array()
            .and_then(|versions| versions.iter().find(|v| v["id"] == profile.game_version))
            .ok_or_else(|| AppError::new(ErrorCode::VersionNotFound))?;
        let metadata_path = version_dir.join("version.json");
        fetch(version, &metadata_path, cancel).await?;
        let mut metadata: Value = serde_json::from_slice(&std::fs::read(&metadata_path)?)?;
        // Only modern clients with the current assets layout and patched Log4j.
        if !modern_version(&profile.game_version) || !metadata["arguments"]["game"].is_array() {
            return Err(AppError::new(ErrorCode::GameNotSupported)
                .detail("Vanilla releases 1.20.5 and newer are supported."));
        }
        self.stage(profile.id, "java", 0, 0).await;
        let major = metadata["javaVersion"]["majorVersion"]
            .as_u64()
            .ok_or_else(|| failure("Missing Java major"))? as u32;
        let java = tokio::task::spawn_blocking(move || {
            java::resolve_java(None, major).and_then(|path| java::probe(&path))
        })
        .await
        .map_err(AppError::unexpected)?;
        let java = match java {
            Ok(found) if found.major == major => found,
            _ => java::download_temurin(major, |_| {}).await?,
        };
        if java.major != major {
            return Err(AppError::new(ErrorCode::JavaTooOld).with("major", major));
        }
        check_cancel(cancel)?;
        // NeoForge's published ignoreList identifies the untouched client by
        // version_name.jar. A generic client.jar becomes a duplicate module.
        let client_jar = version_dir.join(format!("{}.jar", profile.game_version));
        self.stage(profile.id, "client", 0, 1).await;
        fetch(&metadata["downloads"]["client"], &client_jar, cancel).await?;
        if !matches!(profile.loader, Loader::Vanilla) {
            self.stage(profile.id, "loader", 0, 0).await;
            let overlay = super::loaders::install(profile, &root, Path::new(&java.path), &client_jar, cancel).await?;
            super::loaders::merge(&mut metadata, overlay)?;
        }
        let libraries_dir = root.join("libraries");
        let libraries = metadata["libraries"]
            .as_array()
            .ok_or_else(|| failure("Missing libraries"))?;
        let libraries: Vec<_> = libraries
            .iter()
            .filter(|lib| rules_allow(&lib["rules"], false))
            .filter(|lib| !cfg!(target_os = "android") || (!lib["name"].as_str().unwrap_or_default().starts_with("org.lwjgl:") && !lib["name"].as_str().unwrap_or_default().contains(":natives-")))
            .collect();
        let mut classpath = Vec::new();
        let natives = version_dir.join("natives");
        std::fs::create_dir_all(&natives)?;
        for (i, library) in libraries.iter().enumerate() {
            self.stage(profile.id, "libraries", i as u64, libraries.len() as u64)
                .await;
            if library["downloads"].get("artifact").is_some() || library.get("downloads").is_none() {
                let path = super::loaders::artifact(library, &libraries_dir, cancel).await?;
                classpath.push(path);
            }
            // Legacy native classifiers are extracted through a bounded path.
            if let Some(classifier) = library["natives"]["windows"].as_str() {
                let classifier = classifier.replace("${arch}", "64");
                let artifact = &library["downloads"]["classifiers"][&classifier];
                let path = paths::resolve_inside(&libraries_dir, string(artifact, "path")?)?;
                fetch(artifact, &path, cancel).await?;
                extract_natives(&path, &natives)?;
            }
        }
        classpath.push(client_jar);
        let assets = root.join("assets");
        let index_id = string(&metadata["assetIndex"], "id")?;
        let index_path =
            paths::resolve_inside(&assets.join("indexes"), &format!("{index_id}.json"))?;
        fetch(&metadata["assetIndex"], &index_path, cancel).await?;
        let index: Value = serde_json::from_slice(&std::fs::read(index_path)?)?;
        let objects = index["objects"]
            .as_object()
            .ok_or_else(|| failure("Missing assets"))?;
        if objects.len() > 50000 {
            return Err(failure("Too many asset objects"));
        }
        self.stage(profile.id, "assets", 0, objects.len() as u64)
            .await;
        let mut jobs = Vec::new();
        let mut unique_hashes = std::collections::HashSet::new();
        for object in objects.values() {
            let hash = string(object, "hash")?;
            validate_hash(hash)?;
            if !unique_hashes.insert(hash.to_string()) {
                continue;
            }
            let object_path = assets.join("objects").join(&hash[..2]).join(hash);
            let desc = json!({ "url": format!("https://resources.download.minecraft.net/{}/{hash}", &hash[..2]), "sha1": hash, "size": object["size"] });
            jobs.push((desc, object_path));
        }
        let total = jobs.len() as u64;
        let done = Arc::new(std::sync::atomic::AtomicU64::new(0));
        stream::iter(jobs.into_iter().map(|(desc, object_path)| {
            let done = done.clone();
            let cancel = cancel.clone();
            async move {
                fetch(&desc, &object_path, &cancel).await?;
                let completed = done.fetch_add(1, Ordering::Relaxed) + 1;
                self.stage(profile.id, "assets", completed, total).await;
                Ok::<_, AppError>(())
            }
            .boxed()
        }))
        .buffer_unordered(8)
        .try_collect::<Vec<_>>()
        .await?;
        check_cancel(cancel)?;
        std::fs::write(
            game_dir.join(".nebula-installed.json"),
            serde_json::to_vec(&json!({"version": profile.game_version, "java": major}))?,
        )?;
        Ok(Prepared {
            java: java.path.into(),
            metadata,
            game_dir,
            natives,
            assets,
            classpath,
        })
    }

    async fn spawn_game(
        self: &Arc<Self>,
        profile: &Profile,
        prepared: Prepared,
        credentials: Credentials,
    ) -> AppResult<()> {
        if credentials
            .expires
            .is_some_and(|expires| expires <= std::time::Instant::now())
        {
            return Err(AppError::new(ErrorCode::UpstreamRejected)
                .detail("Minecraft session expired. Sign in again."));
        }
        let classpath = std::env::join_paths(&prepared.classpath)
            .map_err(AppError::unexpected)?
            .to_string_lossy()
            .to_string();
        let classpath = if cfg!(target_os = "android") { "__NEBULA_ANDROID_CLASSPATH__".into() } else { classpath };
        let mut substitutions = BTreeMap::new();
        for (key, value) in [
            ("auth_player_name", credentials.name.clone()),
            ("auth_uuid", credentials.uuid),
            ("auth_access_token", credentials.token.clone()),
            ("auth_xuid", String::new()),
            (
                "clientid",
                super::auth::client_id().to_string(),
            ),
            ("user_type", "msa".into()),
            ("version_name", profile.game_version.clone()),
            ("version_type", "release".into()),
            ("game_directory", prepared.game_dir.display().to_string()),
            ("assets_root", prepared.assets.display().to_string()),
            (
                "assets_index_name",
                string(&prepared.metadata["assetIndex"], "id")?.into(),
            ),
            ("natives_directory", prepared.natives.display().to_string()),
            ("launcher_name", "Nebula".into()),
            ("launcher_version", env!("CARGO_PKG_VERSION").into()),
            ("classpath", classpath),
            ("classpath_separator", if cfg!(windows) { ";" } else { ":" }.into()),
            ("library_directory", directory()?.join("libraries").display().to_string()),
            ("resolution_width", "1100".into()),
            ("resolution_height", "720".into()),
        ] {
            substitutions.insert(key, value);
        }
        let mut args = vec![format!("-Xmx{}M", profile.memory_mb), "-Xms512M".into()];
        args.extend(expand_arguments(
            &prepared.metadata["arguments"]["jvm"],
            &substitutions,
            credentials.demo,
        )?);
        args.push(string(&prepared.metadata, "mainClass")?.to_string());
        args.extend(expand_arguments(
            &prepared.metadata["arguments"]["game"],
            &substitutions,
            credentials.demo,
        )?);
        #[cfg(all(target_os = "android", feature = "desktop"))]
        {
            let _ = std::fs::remove_file(prepared.game_dir.join(".nebula-game-exit"));
            super::android::start(json!({
                "id": profile.id.to_string(), "name": profile.name, "memory": profile.memory_mb,
                "root": directory()?, "gameDir": prepared.game_dir,
                "metadata": prepared.metadata, "classpath": prepared.classpath, "args": args,
            }))
        }
        #[cfg(not(all(target_os = "android", feature = "desktop")))]
        {
        let mut children = self.children.lock().await;
        let mut command = java::java_command(&prepared.java);
        java::configure_no_window_command(&mut command);
        let mut child = command
            .args(args)
            .current_dir(&prepared.game_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::new(ErrorCode::InstanceStartFailed).detail(e))?;
        let id = profile.id;
        for pipe in [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let engine = self.clone();
            let token = credentials.token.clone();
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(pipe).lines().map_while(Result::ok) {
                    let line = if token.len() > 8 {
                        line.replace(&token, "[redacted]")
                    } else {
                        line
                    };
                    // The client prints its launch arguments in some error paths.
                    let line = redact_line(&line);
                    engine.log(id, line.chars().take(2000).collect());
                }
            });
        }
        children.insert(profile.id, child);
        Ok(())
        }
    }
}

async fn result_phase(engine: &ClientEngine, id: Uuid) -> String {
    engine
        .status
        .read()
        .await
        .get(&id)
        .map(|s| s.phase.clone())
        .unwrap_or_default()
}
struct Prepared {
    java: PathBuf,
    metadata: Value,
    game_dir: PathBuf,
    natives: PathBuf,
    assets: PathBuf,
    classpath: Vec<PathBuf>,
}
pub fn modern_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').map(str::parse::<u32>).collect();
    if parts.iter().any(Result::is_err) { return false; }
    match parts.as_slice() {
        [Ok(1), Ok(minor), rest @ ..] => {
            *minor > 20
                || (*minor == 20
                    && rest
                        .first()
                        .is_some_and(|p| p.as_ref().is_ok_and(|p| *p >= 5)))
        }
        [Ok(major), ..] => *major >= 26 && parts.iter().all(Result::is_ok),
        _ => false,
    }
}

pub(super) fn validate_hash(hash: &str) -> AppResult<()> {
    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(failure("Invalid SHA-1"));
    }
    Ok(())
}
pub(super) fn cached(path: &Path, hash: &str, size: Option<u64>) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    if size.is_some_and(|size| file.metadata().map(|m| m.len() != size).unwrap_or(true)) {
        return false;
    }
    let mut digest = Sha1::new();
    let mut buffer = [0; 65536];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => digest.update(&buffer[..count]),
            Err(_) => return false,
        }
    }
    hex::encode(digest.finalize()).eq_ignore_ascii_case(hash)
}
pub(super) async fn fetch(desc: &Value, destination: &Path, cancel: &AtomicBool) -> AppResult<()> {
    check_cancel(cancel)?;
    let hash = string(desc, "sha1")?;
    validate_hash(hash)?;
    let size = desc["size"].as_u64();
    if cached(destination, hash, size) {
        return Ok(());
    }
    let url = validate_download_url(string(desc, "url")?)?;
    std::fs::create_dir_all(
        destination.parent().ok_or_else(|| failure("Invalid cache path"))?,
    )?;
    let temp = destination.with_extension("nebula-part");
    let checksum = nebula_downloader::Checksum::Sha1(hash.into());
    let client = crate::game::http_client();
    let options = nebula_downloader::DownloadOptions::new(url.as_str(), &temp)
        .with_checksum(&checksum)
        .with_max_size(1024 * 1024 * 1024);
    let result = tokio::select! {
        result = nebula_downloader::download_file(&client, options) => result.map_err(|_| AppError::new(ErrorCode::DownloadFailed).with("file", destination.file_name().unwrap_or_default().to_string_lossy())),
        _ = async { while !cancel.load(Ordering::Relaxed) { tokio::time::sleep(std::time::Duration::from_millis(200)).await; } } => Err(AppError::new(ErrorCode::Cancelled)),
    };
    if let Err(error) = result {
        let _ = std::fs::remove_file(temp);
        return Err(error);
    }
    if !cached(&temp, hash, size) {
        let _ = std::fs::remove_file(temp);
        return Err(AppError::new(ErrorCode::ChecksumMismatch));
    }
    std::fs::rename(temp, destination)?;
    Ok(())
}

pub(super) fn validate_download_url(value: &str) -> AppResult<reqwest::Url> {
    let url = reqwest::Url::parse(value).map_err(AppError::unexpected)?;
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some(
                "piston-meta.mojang.com"
                    | "piston-data.mojang.com"
                    | "libraries.minecraft.net"
                    | "resources.download.minecraft.net"
                    | "launchermeta.mojang.com"
                    | "launcher.mojang.com"
                    | "maven.fabricmc.net"
                    | "maven.minecraftforge.net"
                    | "maven.neoforged.net"
                    | "repo.maven.apache.org"
            )
        )
    {
        return Err(failure("Untrusted client download host"));
    }
    Ok(url)
}

pub fn rules_allow(rules: &Value, demo: bool) -> bool {
    let Some(rules) = rules.as_array() else {
        return true;
    };
    let mut allowed = false;
    for rule in rules {
        let os = &rule["os"];
        let os_name = if cfg!(target_os = "android") { "linux" } else { "windows" };
        if os["name"].as_str().is_some_and(|name| name != os_name) {
            continue;
        }
        if os["arch"]
            .as_str()
            .is_some_and(|arch| arch != "x86_64" && arch != "amd64")
        {
            continue;
        }
        // Metadata version regexes refer to Windows releases. Fail closed for
        // unsupported conditions rather than including the wrong native library.
        if os.get("version").is_some() {
            continue;
        }
        if let Some(features) = rule["features"].as_object() {
            if !features.iter().all(|(key, value)| {
                value.as_bool()
                    == Some(match key.as_str() {
                        "is_demo_user" => demo,
                        "has_custom_resolution" => true,
                        _ => false,
                    })
            }) {
                continue;
            }
        }
        allowed = rule["action"] == "allow";
    }
    allowed
}
fn expand_arguments(
    arguments: &Value,
    substitutions: &BTreeMap<&str, String>,
    demo: bool,
) -> AppResult<Vec<String>> {
    let mut result = Vec::new();
    for arg in arguments
        .as_array()
        .ok_or_else(|| failure("Missing launch arguments"))?
    {
        let values = if let Some(text) = arg.as_str() {
            vec![text]
        } else if rules_allow(&arg["rules"], demo) {
            match &arg["value"] {
                Value::String(text) => vec![text.as_str()],
                Value::Array(values) => values.iter().filter_map(Value::as_str).collect(),
                _ => return Err(failure("Invalid argument")),
            }
        } else {
            continue;
        };
        for value in values {
            let mut value = value.to_string();
            for (key, replacement) in substitutions {
                value = value.replace(&format!("${{{key}}}"), replacement);
            }
            if value.contains("${") {
                return Err(failure("Unsupported client launch placeholder"));
            }
            result.push(value);
        }
    }
    Ok(result)
}
fn redact_line(line: &str) -> String {
    static REDACT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    REDACT
        .get_or_init(|| {
            regex::Regex::new(r"(?i)(--accessToken[= ,:]+|Bearer\s+)[^\s,\]]+").unwrap()
        })
        .replace_all(line, "${1}[redacted]")
        .into_owned()
}
fn extract_natives(archive: &Path, root: &Path) -> AppResult<()> {
    let mut zip =
        zip::ZipArchive::new(std::fs::File::open(archive)?).map_err(AppError::unexpected)?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(AppError::unexpected)?;
        if entry.is_dir() || entry.name().starts_with("META-INF/") {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(failure("Native symlink refused"));
        }
        total = total.saturating_add(entry.size());
        if total > 256 * 1024 * 1024 {
            return Err(failure("Native archive too large"));
        }
        let path = paths::resolve_inside(root, entry.name())?;
        std::fs::create_dir_all(
            path.parent()
                .ok_or_else(|| failure("Invalid native path"))?,
        )?;
        std::io::copy(&mut entry, &mut std::fs::File::create(path)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules_handle_os_arch_features_and_last_match() {
        assert!(rules_allow(&Value::Null, false));
        assert!(!rules_allow(
            &json!([{ "action":"allow", "os":{"name":"linux"} }]),
            false
        ));
        assert!(!rules_allow(
            &json!([{ "action":"allow", "os":{"arch":"x86"} }]),
            false
        ));
        assert!(!rules_allow(
            &json!([{ "action":"allow" }, { "action":"disallow", "os":{"name":"windows"} }]),
            false
        ));
        let demo = json!([{ "action":"allow", "features":{"is_demo_user":true} }]);
        assert!(rules_allow(&demo, true));
        assert!(!rules_allow(&demo, false));
    }
    #[test]
    fn expands_paths_as_one_argument_and_demo_only_when_requested() {
        let args = json!(["--gameDir", "${game_directory}", {"rules":[{"action":"allow","features":{"is_demo_user":true}}],"value":"--demo"}]);
        let substitutions = BTreeMap::from([("game_directory", "C:/Games/My profile".into())]);
        let normal = expand_arguments(&args, &substitutions, false).unwrap();
        assert_eq!(normal, ["--gameDir", "C:/Games/My profile"]);
        assert_eq!(
            expand_arguments(&args, &substitutions, true)
                .unwrap()
                .last()
                .unwrap(),
            "--demo"
        );
        assert!(expand_arguments(&json!(["${unknown}"]), &substitutions, false).is_err());
    }
    #[test]
    fn cache_rejects_truncation_and_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("asset");
        std::fs::write(&path, b"hello").unwrap();
        let hash = hex::encode(Sha1::digest(b"hello"));
        assert!(cached(&path, &hash, Some(5)));
        assert!(!cached(&path, &hash, Some(6)));
        std::fs::write(&path, b"other").unwrap();
        assert!(!cached(&path, &hash, Some(5)));
        assert!(validate_hash("../../bad").is_err());
    }
    #[test]
    fn modern_releases_only_and_logs_redact_credentials() {
        assert!(modern_version("1.21.4"));
        assert!(modern_version("1.20.5"));
        assert!(modern_version("26.1.2"));
        assert!(!modern_version("1.20.4"));
        assert!(!modern_version("1.21-pre1"));
        assert!(!redact_line("--accessToken SECRET, Bearer SECRET2").contains("SECRET"));
    }
}
