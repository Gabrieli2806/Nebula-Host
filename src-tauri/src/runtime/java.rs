//! Finding a JVM to run a game with.
//!
//! Games that need Java say so in their manifest and tell us which major
//! version a given build requires; this module only answers "where is a JVM of
//! at least major N".

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use nebula_downloader::{download_file, Checksum, DownloadOptions};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::store::settings;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Keep spawned helpers from flashing a console window on Windows.
pub fn configure_no_window_command(command: &mut Command) -> &mut Command {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// A command that runs `java`.
///
/// On Android an app may only execute what the package installed, never a
/// file in its own data folder, so a runtime kept there (the one that ships
/// in the app, or one the user picked) is started through the launcher
/// library instead: `libnebula_java.so`, which loads the runtime and runs it
/// in-process (see `src-tauri/android/launcher/nebula_java.c`). Anywhere else
/// this is just `Command::new(java)`.
pub fn java_command(java: &Path) -> Command {
    #[cfg(target_os = "android")]
    if let Some(lib_dir) = std::env::var_os("NEBULA_NATIVE_LIB_DIR") {
        // <home>/bin/java → <home>
        if let Some(home) = java.parent().and_then(Path::parent) {
            let mut command = Command::new(PathBuf::from(lib_dir).join("libnebula_java.so"));
            command.env("NEBULA_JRE_DIR", home);
            return command;
        }
    }
    Command::new(java)
}

/// A JVM the user has configured, or that was found on this machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaInstallation {
    pub path: String,
    pub major: u32,
    pub version: String,
    /// True when it came from `PATH` rather than the user's settings.
    #[serde(default)]
    pub from_path: bool,
    /// Who built it, as people know them ("Eclipse Temurin", "Azul Zulu").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// CPU architecture (`x86_64`, `aarch64`, `x86`). A 32-bit JVM caps the
    /// heap near 1.5 GB, which is worth flagging next to a 4 GB server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    /// Where it was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<JavaSource>,
}

/// Where a JVM was found, so the UI can say "bundled with the Minecraft
/// Launcher" instead of showing a path nobody recognises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JavaSource {
    /// Downloaded by this app into its own runtimes folder.
    Nebula,
    /// A path the user set in Settings.
    Settings,
    JavaHome,
    Path,
    /// The Windows registry (JavaSoft, Adoptium, Azul, Microsoft keys).
    Registry,
    /// The usual install folders (Program Files, /usr/lib/jvm, …).
    System,
    MinecraftLauncher,
    ModrinthApp,
    CurseForge,
    PrismLauncher,
    /// IDE- and tool-managed JDKs (~/.jdks, SDKMAN!).
    DevTools,
}

/// Resolve the Java executable to use.
///
/// Order: an explicit per-instance override, then the user's configured path
/// for a major version at least `required_major` (the lowest one that
/// satisfies it — newer JVMs run older servers, but a 1.20 server does not
/// need Java 25), then plain `java` from `PATH`.
pub fn resolve_java(override_path: Option<&str>, required_major: u32) -> AppResult<PathBuf> {
    if let Some(path) = override_path.map(str::trim).filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    if let Some(path) = configured_java_for_major(required_major) {
        return Ok(PathBuf::from(path));
    }

    if let Some(path) = discovered_java_for_major(required_major) {
        return Ok(path);
    }

    // The wider search: launcher-bundled runtimes, the registry, PATH, and
    // what this app downloaded. Exact major first — a 1.16 Forge server on
    // Java 21 fails in ways that look nothing like "wrong Java".
    if let Some(found) = pick_for_major(&detect_installations(false), required_major) {
        return Ok(PathBuf::from(found.path));
    }

    // Plain `java` only when there is one on PATH; otherwise say which Java
    // is missing, so the app can offer to install it instead of failing with
    // "file not found" when the server starts.
    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(java_exe()).is_file()));
    if on_path && !cfg!(target_os = "android") {
        return Ok(PathBuf::from("java"));
    }
    Err(AppError::new(ErrorCode::JavaNotFound).with("major", required_major))
}

/// The app can install a Java for this machine: from the package on Android,
/// from Eclipse Temurin elsewhere.
pub fn can_install() -> bool {
    #[cfg(target_os = "android")]
    {
        !crate::runtime::android_java::bundled_majors().is_empty()
    }
    #[cfg(not(target_os = "android"))]
    {
        adoptium_platform().is_some()
    }
}

/// Where JVMs usually live, per platform. JAVA_HOME first.
fn jvm_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(home) = std::env::var("JAVA_HOME") {
        if let Some(parent) = std::path::Path::new(&home).parent() {
            roots.push(parent.to_path_buf());
        }
    }
    if cfg!(target_os = "windows") {
        for base in [r"C:\Program Files", r"C:\Program Files (x86)"] {
            for vendor in ["Java", "Eclipse Adoptium", "Microsoft", "Zulu", "Amazon Corretto", "BellSoft"] {
                roots.push(PathBuf::from(base).join(vendor));
            }
        }
    } else if cfg!(target_os = "macos") {
        roots.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
    } else {
        roots.push(PathBuf::from("/usr/lib/jvm"));
        roots.push(PathBuf::from("/opt/java"));
    }
    roots
}

/// Major version from a JVM folder name: `java-21-openjdk-arm64` → 21,
/// `jdk-25.0.2` → 25, `java-1.8.0-openjdk` → 8, `temurin-17` → 17.
fn major_from_dir_name(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    let start = lower.find(|c: char| c.is_ascii_digit())?;
    let digits: String = lower[start..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let mut parts = digits.split('.').filter(|p| !p.is_empty());
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// The lowest installed JVM (found in the usual folders) that satisfies
/// `required_major`. Lets a fresh machine with several JVMs — a Raspberry Pi
/// with OpenJDK 17, 21 and 25 — run each server on the right one without
/// configuring anything.
fn discovered_java_for_major(required_major: u32) -> Option<PathBuf> {
    let exe = if cfg!(target_os = "windows") { "java.exe" } else { "java" };
    let mut found: Vec<(u32, PathBuf)> = Vec::new();
    for root in jvm_roots() {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(major) = major_from_dir_name(&name) else { continue };
            if major < required_major {
                continue;
            }
            let base = entry.path();
            let candidates = [base.join("bin").join(exe), base.join("Contents/Home/bin").join(exe)];
            if let Some(java) = candidates.into_iter().find(|p| p.is_file()) {
                found.push((major, java));
            }
        }
    }
    found.sort_by_key(|(major, _)| *major);
    found.into_iter().next().map(|(_, path)| path)
}

/// The lowest configured JVM that still satisfies `required_major`.
fn configured_java_for_major(required_major: u32) -> Option<String> {
    let stored = settings::load().ok()?;

    let mut candidates: Vec<(u32, String)> = stored
        .java_paths
        .iter()
        .filter_map(|(major, path)| {
            let major: u32 = major.parse().ok()?;
            let path = path.trim();
            (!path.is_empty() && major >= required_major).then(|| (major, path.to_string()))
        })
        .collect();
    candidates.sort_by_key(|(major, _)| *major);

    candidates
        .into_iter()
        .next()
        .map(|(_, path)| path)
        .or_else(|| stored.java_path.filter(|p| !p.trim().is_empty()))
}

/// Ask a JVM which version it is. Used by the first-run health check, so it
/// must never hang: `java -version` on a broken install can block forever
/// otherwise.
pub fn probe(java: &std::path::Path) -> AppResult<JavaInstallation> {
    let text = version_output(java).map_err(|error| {
        AppError::new(ErrorCode::JavaNotFound)
            .with("path", java.display())
            .detail(error)
    })?;

    let version = parse_version_string(&text).ok_or_else(|| {
        AppError::new(ErrorCode::JavaNotFound)
            .with("path", java.display())
            .detail(text.trim().to_string())
    })?;

    Ok(JavaInstallation {
        path: java.display().to_string(),
        major: major_from_version(&version),
        version,
        from_path: java.parent().is_none_or(|parent| parent.as_os_str().is_empty()),
        vendor: vendor_from_version_output(&text),
        arch: None,
        source: None,
    })
}

/// Pull `21.0.5` out of `openjdk version "21.0.5" 2024-10-15`.
fn parse_version_string(output: &str) -> Option<String> {
    let line = output.lines().find(|line| line.contains("version"))?;
    let start = line.find('"')? + 1;
    let rest = line.get(start..)?;
    let end = rest.find('"')?;
    Some(rest.get(..end)?.to_string())
}

/// `1.8.0_402` is Java 8; `21.0.5` is Java 21.
fn major_from_version(version: &str) -> u32 {
    let mut parts = version.split(['.', '_', '-']);
    let first: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    if first == 1 {
        parts.next().and_then(|p| p.parse().ok()).unwrap_or(0)
    } else {
        first
    }
}

/// `java -version`, read from stderr (or stdout on the odd JVM), with a
/// time limit: a broken install that never answers must not hold up the
/// health check or a detection pass.
fn version_output(java: &Path) -> std::io::Result<String> {
    let mut command = java_command(java);
    let mut child = configure_no_window_command(&mut command)
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "java -version did not answer"));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let output = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(if text.trim().is_empty() { String::from_utf8_lossy(&output.stdout).to_string() } else { text })
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// How long one detection pass is reused. Starting a server resolves its JVM
/// and the settings card lists them; neither should rescan the disk each time.
const DETECT_TTL: Duration = Duration::from_secs(60);

/// The last detection pass and when it ran.
type Detected = Option<(Instant, Vec<JavaInstallation>)>;

fn detect_cache() -> &'static Mutex<Detected> {
    static CACHE: OnceLock<Mutex<Detected>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Forget the last detection pass (after a download, say).
pub fn forget_detected() {
    if let Ok(mut cache) = detect_cache().lock() {
        *cache = None;
    }
}

/// Every JVM on this machine that answers, newest major first. Reads each
/// one's `release` file where it has one (instant), and only runs
/// `java -version` for the few that do not.
pub fn detect_installations(refresh: bool) -> Vec<JavaInstallation> {
    if !refresh {
        if let Some((at, found)) = detect_cache().lock().ok().and_then(|c| c.clone()) {
            if at.elapsed() < DETECT_TTL {
                return found;
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();
    for (java, source) in candidates() {
        // The same JVM is often reachable several ways (JAVA_HOME, PATH and
        // Program Files all pointing at one Temurin); list it once, under
        // the first — most specific — way it was found.
        let key = std::fs::canonicalize(&java).unwrap_or_else(|_| java.clone());
        let key = key.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        if let Some(mut install) = describe(&java) {
            install.source = Some(source);
            found.push(install);
        }
    }
    found.sort_by(|a, b| b.major.cmp(&a.major).then_with(|| a.path.cmp(&b.path)));

    if let Ok(mut cache) = detect_cache().lock() {
        *cache = Some((Instant::now(), found.clone()));
    }
    found
}

/// The JVM to use for a server that needs `required_major`: that exact
/// major if there is one, otherwise the lowest newer one. 64-bit and
/// app-managed copies win ties.
pub fn pick_for_major(found: &[JavaInstallation], required_major: u32) -> Option<JavaInstallation> {
    let rank = |install: &JavaInstallation| {
        (
            install.major != required_major,
            install.major,
            is_32_bit(install),
            install.source != Some(JavaSource::Nebula),
        )
    };
    found.iter().filter(|install| install.major >= required_major).min_by_key(|install| rank(install)).cloned()
}

fn is_32_bit(install: &JavaInstallation) -> bool {
    matches!(install.arch.as_deref(), Some("x86" | "i386" | "i586" | "i686" | "arm"))
}

/// Version, vendor and architecture of one `java` executable.
fn describe(java: &Path) -> Option<JavaInstallation> {
    let home = java.parent()?.parent()?;
    if let Ok(release) = std::fs::read_to_string(home.join("release")) {
        let fields = parse_release(&release);
        if let Some(version) = fields.get("JAVA_VERSION").filter(|v| !v.is_empty()) {
            return Some(JavaInstallation {
                path: java.display().to_string(),
                major: major_from_version(version),
                version: fields.get("JAVA_RUNTIME_VERSION").cloned().unwrap_or_else(|| version.clone()),
                from_path: false,
                vendor: fields
                    .get("IMPLEMENTOR")
                    .map(|name| vendor_name(name))
                    .or_else(|| fields.get("JAVA_VENDOR").map(|name| vendor_name(name)))
                    .filter(|name| !name.is_empty())
                    // Some builds leave the field out; their folder usually
                    // says who made them (`corretto-1.8.0_462`, `zulu21…`).
                    .or_else(|| vendor_from_version_output(&home.to_string_lossy())),
                arch: fields.get("OS_ARCH").map(|arch| normalise_arch(arch)),
                source: None,
            });
        }
    }
    // No release file (old Oracle JREs, the javapath stubs): ask the JVM.
    let mut install = probe(java).ok()?;
    install.from_path = false;
    Some(install)
}

/// `KEY="value"` lines of a JDK `release` file.
fn parse_release(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), value.trim().trim_matches('"').to_string()))
        })
        .collect()
}

/// The name people know a build by, from its `IMPLEMENTOR` field.
fn vendor_name(implementor: &str) -> String {
    let lower = implementor.to_ascii_lowercase();
    let known = [
        ("adoptium", "Eclipse Temurin"),
        ("adoptopenjdk", "AdoptOpenJDK"),
        ("azul", "Azul Zulu"),
        ("microsoft", "Microsoft"),
        ("amazon", "Amazon Corretto"),
        ("bellsoft", "BellSoft Liberica"),
        ("graalvm", "GraalVM"),
        ("oracle", "Oracle"),
        ("sap se", "SapMachine"),
        ("international business machines", "IBM Semeru"),
        ("ibm", "IBM Semeru"),
        ("jetbrains", "JetBrains Runtime"),
        ("red hat", "Red Hat"),
        ("debian", "Debian OpenJDK"),
        ("ubuntu", "Ubuntu OpenJDK"),
        ("homebrew", "Homebrew OpenJDK"),
    ];
    for (needle, name) in known {
        if lower.contains(needle) {
            return name.to_string();
        }
    }
    if lower == "n/a" {
        return String::new();
    }
    implementor.trim().to_string()
}

/// Best guess at the vendor from `java -version` text, for JVMs without a
/// release file: `OpenJDK Runtime Environment Temurin-21.0.5+11`,
/// `Java(TM) SE Runtime Environment`, `OpenJDK … Zulu21.38+21-CA`.
fn vendor_from_version_output(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let known = [
        ("temurin", "Eclipse Temurin"),
        ("adoptopenjdk", "AdoptOpenJDK"),
        ("zulu", "Azul Zulu"),
        ("microsoft", "Microsoft"),
        ("corretto", "Amazon Corretto"),
        ("liberica", "BellSoft Liberica"),
        ("graalvm", "GraalVM"),
        ("semeru", "IBM Semeru"),
        ("openj9", "IBM Semeru"),
        ("sapmachine", "SapMachine"),
        ("jbr", "JetBrains Runtime"),
        ("java(tm)", "Oracle"),
    ];
    known.iter().find(|(needle, _)| lower.contains(needle)).map(|(_, name)| name.to_string())
}

fn normalise_arch(arch: &str) -> String {
    match arch.to_ascii_lowercase().as_str() {
        "amd64" | "x86_64" | "x64" => "x86_64".to_string(),
        "aarch64" | "arm64" => "aarch64".to_string(),
        "i386" | "i586" | "i686" | "x86" | "x32" => "x86".to_string(),
        other => other.to_string(),
    }
}

fn java_exe() -> &'static str {
    if cfg!(target_os = "windows") { "java.exe" } else { "java" }
}

/// Every place a JVM is likely to be, most specific first.
fn candidates() -> Vec<(PathBuf, JavaSource)> {
    let exe = java_exe();
    let mut out: Vec<(PathBuf, JavaSource)> = Vec::new();
    let push_home = |home: PathBuf, source: JavaSource, out: &mut Vec<(PathBuf, JavaSource)>| {
        for java in [home.join("bin").join(exe), home.join("Contents/Home/bin").join(exe)] {
            if java.is_file() {
                out.push((java, source));
                return;
            }
        }
    };

    // What this app downloaded.
    if let Ok(dir) = crate::store::paths::runtimes_dir() {
        for java in find_java_under(&dir, 4) {
            out.push((java, JavaSource::Nebula));
        }
    }

    // What the user pointed at in Settings.
    if let Ok(stored) = settings::load() {
        for path in stored.java_paths.values().chain(stored.java_path.iter()) {
            let path = PathBuf::from(path.trim());
            if path.is_file() {
                out.push((path, JavaSource::Settings));
            }
        }
    }

    if let Ok(home) = std::env::var("JAVA_HOME") {
        push_home(PathBuf::from(home), JavaSource::JavaHome, &mut out);
    }

    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let java = dir.join(exe);
            if java.is_file() {
                out.push((java, JavaSource::Path));
            }
        }
    }

    for home in registry_java_homes() {
        push_home(home, JavaSource::Registry, &mut out);
    }

    for (root, source) in system_roots() {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for entry in entries.flatten() {
            push_home(entry.path(), source, &mut out);
        }
    }

    for (root, source) in launcher_roots() {
        for java in find_java_under(&root, 5) {
            out.push((java, source));
        }
    }

    out
}

/// Folders whose children are JVM homes, per platform.
fn system_roots() -> Vec<(PathBuf, JavaSource)> {
    let mut roots = Vec::new();
    let home = dirs::home_dir();
    if cfg!(target_os = "windows") {
        let bases = [
            std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into()),
            std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into()),
        ];
        let vendors = [
            "Java",
            "Eclipse Adoptium",
            "Eclipse Foundation",
            "AdoptOpenJDK",
            "Microsoft",
            "Zulu",
            r"Azul Systems\Zulu",
            "Amazon Corretto",
            "BellSoft",
            "Semeru",
            "IBM",
            "GraalVM",
            "SapMachine",
            "OpenJDK",
        ];
        for base in bases {
            for vendor in vendors {
                roots.push(PathBuf::from(&base).join(vendor));
            }
        }
    } else if cfg!(target_os = "macos") {
        roots.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
        if let Some(home) = &home {
            roots.push(home.join("Library/Java/JavaVirtualMachines"));
        }
        // Homebrew keeps each JDK under opt/openjdk@N/libexec/openjdk.jdk.
        for brew in ["/opt/homebrew/opt", "/usr/local/opt"] {
            if let Ok(entries) = std::fs::read_dir(brew) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if name.starts_with("openjdk") || name.starts_with("temurin") {
                        roots.push(entry.path().join("libexec"));
                    }
                }
            }
        }
    } else {
        for root in ["/usr/lib/jvm", "/usr/lib64/jvm", "/usr/java", "/opt/java", "/opt/jdk", "/opt"] {
            roots.push(PathBuf::from(root));
        }
    }
    roots.extend(jvm_roots());
    let mut roots: Vec<(PathBuf, JavaSource)> = roots.into_iter().map(|root| (root, JavaSource::System)).collect();
    // Every platform: JDKs managed by IntelliJ and SDKMAN!.
    if let Some(home) = &home {
        roots.push((home.join(".jdks"), JavaSource::DevTools));
        roots.push((home.join(".sdkman/candidates/java"), JavaSource::DevTools));
    }
    roots
}

/// Game launchers that bundle their own Java. Any of these is a real,
/// working JRE of a known version — often the only one on a gaming PC.
fn launcher_roots() -> Vec<(PathBuf, JavaSource)> {
    let mut roots = Vec::new();
    let home = dirs::home_dir();
    let roaming = dirs::config_dir();
    let local = dirs::data_local_dir();
    let data = dirs::data_dir();

    if cfg!(target_os = "windows") {
        if let Some(roaming) = &roaming {
            roots.push((roaming.join(r".minecraft\runtime"), JavaSource::MinecraftLauncher));
            roots.push((roaming.join(r"ModrinthApp\meta\java_versions"), JavaSource::ModrinthApp));
            roots.push((roaming.join(r"com.modrinth.theseus\meta\java_versions"), JavaSource::ModrinthApp));
            roots.push((roaming.join(r"PrismLauncher\java"), JavaSource::PrismLauncher));
        }
        if let Some(local) = &local {
            // The Microsoft Store edition of the launcher.
            roots.push((
                local.join(r"Packages\Microsoft.4297127D64EC6_8wekyb3d8bbwe\LocalCache\Local\runtime"),
                JavaSource::MinecraftLauncher,
            ));
        }
        for base in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Ok(base) = std::env::var(base) {
                roots.push((PathBuf::from(base).join(r"Minecraft Launcher\runtime"), JavaSource::MinecraftLauncher));
            }
        }
        if let Some(home) = &home {
            roots.push((home.join(r"curseforge\minecraft\Install\runtime"), JavaSource::CurseForge));
        }
    } else if cfg!(target_os = "macos") {
        if let Some(data) = &data {
            roots.push((data.join("minecraft/runtime"), JavaSource::MinecraftLauncher));
            roots.push((data.join("ModrinthApp/meta/java_versions"), JavaSource::ModrinthApp));
            roots.push((data.join("com.modrinth.theseus/meta/java_versions"), JavaSource::ModrinthApp));
            roots.push((data.join("PrismLauncher/java"), JavaSource::PrismLauncher));
        }
        if let Some(home) = &home {
            roots.push((home.join("Documents/curseforge/minecraft/Install/runtime"), JavaSource::CurseForge));
        }
    } else {
        if let Some(home) = &home {
            roots.push((home.join(".minecraft/runtime"), JavaSource::MinecraftLauncher));
        }
        if let Some(data) = &data {
            roots.push((data.join("ModrinthApp/meta/java_versions"), JavaSource::ModrinthApp));
            roots.push((data.join("com.modrinth.theseus/meta/java_versions"), JavaSource::ModrinthApp));
            roots.push((data.join("PrismLauncher/java"), JavaSource::PrismLauncher));
        }
    }
    roots
}

/// `bin/java` anywhere up to `depth` folders below `root`. Launcher layouts
/// nest (`runtime/java-runtime-delta/windows-x64/java-runtime-delta/bin`),
/// so a fixed pattern would miss half of them.
fn find_java_under(root: &Path, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let exe = java_exe();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, level)) = stack.pop() {
        let java = dir.join("bin").join(exe);
        if java.is_file() {
            found.push(java);
            continue;
        }
        if level >= depth {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            // Never follow links out of the tree, and skip our own temp work.
            let Ok(kind) = entry.file_type() else { continue };
            let name = entry.file_name();
            if kind.is_dir() && !name.to_string_lossy().starts_with('.') {
                stack.push((entry.path(), level + 1));
            }
        }
    }
    found.sort();
    found
}

/// JVM homes registered in the Windows registry. Read with `reg query`
/// rather than a registry crate: it is always there and never needs admin.
#[cfg(target_os = "windows")]
fn registry_java_homes() -> Vec<PathBuf> {
    let keys = [
        r"HKLM\SOFTWARE\JavaSoft\JDK",
        r"HKLM\SOFTWARE\JavaSoft\JRE",
        r"HKLM\SOFTWARE\JavaSoft\Java Runtime Environment",
        r"HKLM\SOFTWARE\JavaSoft\Java Development Kit",
        r"HKLM\SOFTWARE\WOW6432Node\JavaSoft\Java Runtime Environment",
        r"HKLM\SOFTWARE\Eclipse Adoptium",
        r"HKLM\SOFTWARE\Eclipse Foundation",
        r"HKLM\SOFTWARE\AdoptOpenJDK",
        r"HKLM\SOFTWARE\Azul Systems\Zulu",
        r"HKLM\SOFTWARE\Microsoft\JDK",
        r"HKLM\SOFTWARE\BellSoft\Liberica",
        r"HKLM\SOFTWARE\Amazon Corretto",
    ];
    let mut homes = Vec::new();
    for key in keys {
        let mut command = Command::new("reg");
        let output = configure_no_window_command(&mut command)
            .args(["query", key, "/s"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        let Ok(output) = output else { continue };
        homes.extend(parse_reg_homes(&String::from_utf8_lossy(&output.stdout)));
    }
    homes
}

#[cfg(not(target_os = "windows"))]
fn registry_java_homes() -> Vec<PathBuf> {
    Vec::new()
}

/// `JavaHome    REG_SZ    C:\Program Files\…` and Adoptium's `Path` values.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn parse_reg_homes(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (name, rest) = line.split_once("REG_SZ")?;
            let name = name.trim().to_ascii_lowercase();
            (name == "javahome" || name == "path" || name == "installationpath")
                .then(|| PathBuf::from(rest.trim().trim_end_matches('\\')))
        })
        .filter(|path| !path.as_os_str().is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Download (Eclipse Temurin, from the Adoptium API)
// ---------------------------------------------------------------------------

/// Where a runtime download has got to.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum JavaDownloadProgress {
    Resolving,
    Downloading { received: u64, total: Option<u64> },
    Extracting,
    Done { path: String },
    Failed { code: ErrorCode },
}

/// Adoptium names for this machine, or `None` where Temurin has no build.
pub fn adoptium_platform() -> Option<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "linux" => "linux",
        "macos" => "mac",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        "x86" => "x32",
        "arm" => "arm",
        _ => return None,
    };
    Some((os, arch))
}

/// The folder a downloaded Temurin of `major` lives in.
#[cfg_attr(target_os = "android", allow(dead_code))]
fn managed_home(major: u32) -> AppResult<PathBuf> {
    Ok(crate::store::paths::runtimes_dir()?.join(format!("temurin-{major}")))
}

/// Download Eclipse Temurin `major` (a JRE where one is published, else the
/// JDK) into the app's runtimes folder, checksum-verified, and return it.
/// Replaces an earlier download of the same major, so this doubles as
/// "update the runtime".
pub async fn download_temurin(
    major: u32,
    progress: impl Fn(JavaDownloadProgress) + Send + Sync + 'static,
) -> AppResult<JavaInstallation> {
    let progress = std::sync::Arc::new(progress);
    let report = progress.clone();
    // Android installs the runtime that ships in the package instead.
    #[cfg(target_os = "android")]
    let result = {
        let progress = progress.clone();
        tokio::task::spawn_blocking(move || crate::runtime::android_java::install(major, &*progress))
            .await
            .map_err(AppError::unexpected)
            .and_then(|result| result)
    };
    #[cfg(not(target_os = "android"))]
    let result = download_temurin_inner(major, progress).await;
    if let Err(error) = &result {
        report(JavaDownloadProgress::Failed { code: error.code });
    }
    result
}

#[cfg_attr(target_os = "android", allow(dead_code))]
async fn download_temurin_inner(
    major: u32,
    progress: std::sync::Arc<impl Fn(JavaDownloadProgress) + Send + Sync + 'static>,
) -> AppResult<JavaInstallation> {
    let (os, arch) = adoptium_platform()
        .ok_or_else(|| AppError::new(ErrorCode::GameNotSupported).with("platform", std::env::consts::OS))?;
    progress(JavaDownloadProgress::Resolving);

    // No overall timeout: a 50 MB runtime on a slow line takes minutes. Stall
    // detection comes from the read timeout instead.
    let client = reqwest::Client::builder()
        .user_agent(crate::game::USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(AppError::unexpected)?;

    let mut package = None;
    for image in ["jre", "jdk"] {
        let url = format!("https://api.adoptium.net/v3/assets/latest/{major}/hotspot");
        let assets: serde_json::Value = client
            .get(&url)
            .query(&[("os", os), ("architecture", arch), ("image_type", image), ("vendor", "eclipse")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if let Some(found) = assets.as_array().and_then(|list| list.first()).and_then(|asset| {
            let package = asset.get("binary")?.get("package")?;
            Some((
                package.get("link")?.as_str()?.to_string(),
                package.get("name")?.as_str()?.to_string(),
                package.get("checksum").and_then(|c| c.as_str()).map(str::to_string),
            ))
        }) {
            package = Some(found);
            break;
        }
    }
    let (link, name, checksum) = package
        .ok_or_else(|| AppError::new(ErrorCode::VersionNotFound).with("distribution", "Java").with("version", major))?;
    // A plain archive name, never a path; and only the formats unpacked below.
    let lower = name.to_ascii_lowercase();
    if name.contains(['/', '\\', ':']) || name.contains("..") || !(lower.ends_with(".zip") || lower.ends_with(".tar.gz")) {
        return Err(AppError::new(ErrorCode::InstallFailed).with("distribution", "Java").detail(name));
    }

    let runtimes = crate::store::paths::runtimes_dir()?;
    let downloads = runtimes.join(".downloads");
    std::fs::create_dir_all(&downloads)?;
    let archive = downloads.join(&name);
    let checksum = checksum.map(Checksum::Sha256);
    let mut options = DownloadOptions::new(&link, archive.as_path());
    if let Some(checksum) = checksum.as_ref() {
        options = options.with_checksum(checksum);
    }
    let sink = progress.clone();
    options = options.with_progress(move |received, total| {
        sink(JavaDownloadProgress::Downloading { received, total: (total > 0).then_some(total) });
    });
    download_file(&client, options).await.map_err(|error| {
        let code = if error.contains("integrity check failed") { ErrorCode::ChecksumMismatch } else { ErrorCode::DownloadFailed };
        AppError::new(code).with("file", &name).detail(error)
    })?;

    progress(JavaDownloadProgress::Extracting);
    let home = managed_home(major)?;
    let java = tokio::task::spawn_blocking(move || {
        let result = install_archive(&archive, &runtimes, &home);
        let _ = std::fs::remove_file(&archive);
        result
    })
    .await
    .map_err(AppError::unexpected)??;

    forget_detected();
    let mut install = describe(&java).ok_or_else(|| AppError::new(ErrorCode::JavaNotFound).with("path", java.display()))?;
    install.source = Some(JavaSource::Nebula);
    progress(JavaDownloadProgress::Done { path: install.path.clone() });
    Ok(install)
}

/// Unpack into a scratch folder, then swap it in as `home` in one rename, so
/// a half-extracted runtime is never what a server starts with.
#[cfg_attr(target_os = "android", allow(dead_code))]
fn install_archive(archive: &Path, runtimes: &Path, home: &Path) -> AppResult<PathBuf> {
    let scratch = runtimes.join(format!(".extract-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&scratch)?;
    let cleanup = |error: AppError| {
        let _ = std::fs::remove_dir_all(&scratch);
        error
    };

    let name = archive.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let unpacked = if name.ends_with(".zip") {
        std::fs::File::open(archive)
            .map_err(AppError::from)
            .and_then(|file| zip::ZipArchive::new(file).map_err(|e| AppError::new(ErrorCode::InstallFailed).detail(e)))
            .and_then(|mut zip| zip.extract(&scratch).map_err(|e| AppError::new(ErrorCode::InstallFailed).detail(e)))
    } else {
        std::fs::File::open(archive).map_err(AppError::from).and_then(|file| {
            tar::Archive::new(flate2::read::GzDecoder::new(file))
                .unpack(&scratch)
                .map_err(|e| AppError::new(ErrorCode::InstallFailed).detail(e))
        })
    };
    unpacked.map_err(|e| cleanup(e.with("distribution", "Java")))?;

    // Archives hold one top folder (`jdk-21.0.5+11-jre`); that becomes `home`.
    let java = find_java_under(&scratch, 4)
        .into_iter()
        .next()
        .ok_or_else(|| cleanup(AppError::new(ErrorCode::InstallFailed).with("distribution", "Java")))?;
    let relative = java.strip_prefix(&scratch).map_err(|e| cleanup(AppError::unexpected(e)))?.to_path_buf();
    let top = relative.components().next().map(|c| scratch.join(c.as_os_str())).unwrap_or(scratch.clone());
    let inner = java.strip_prefix(&top).map(Path::to_path_buf).unwrap_or(relative);

    // Move an earlier copy aside rather than deleting it in place: on Windows
    // a server running on it holds its files open, and a delete that gets
    // halfway would leave a runtime that no longer starts. A rename either
    // happens or leaves everything as it was.
    let previous = runtimes.join(format!(".previous-{}", uuid::Uuid::new_v4()));
    let moved_aside = home.exists();
    if moved_aside {
        std::fs::rename(home, &previous).map_err(|e| cleanup(AppError::from(e).with("reason", "in_use")))?;
    }
    if let Err(error) = std::fs::rename(&top, home) {
        if moved_aside {
            let _ = std::fs::rename(&previous, home);
        }
        return Err(cleanup(AppError::from(error)));
    }
    let _ = std::fs::remove_dir_all(&scratch);
    if moved_aside {
        let _ = std::fs::remove_dir_all(&previous);
    }
    Ok(home.join(inner))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jvm_folder_names_give_their_major() {
        assert_eq!(major_from_dir_name("java-21-openjdk-arm64"), Some(21));
        assert_eq!(major_from_dir_name("jdk-25.0.2"), Some(25));
        assert_eq!(major_from_dir_name("java-1.8.0-openjdk-amd64"), Some(8));
        assert_eq!(major_from_dir_name("java-1.21.0-openjdk-arm64"), Some(21));
        assert_eq!(major_from_dir_name("temurin-17-jdk"), Some(17));
        assert_eq!(major_from_dir_name("default-java"), None);
    }

    #[test]
    fn explicit_override_wins() {
        let resolved = resolve_java(Some("C:/jdk21/bin/java.exe"), 21).expect("override accepted");
        assert_eq!(resolved, PathBuf::from("C:/jdk21/bin/java.exe"));
    }

    #[test]
    fn blank_override_is_ignored() {
        let resolved = resolve_java(Some("   "), 8).expect("falls through");
        assert_ne!(resolved, PathBuf::from("   "));
    }

    #[test]
    fn parses_version_strings_from_real_jvms() {
        assert_eq!(
            parse_version_string("openjdk version \"21.0.5\" 2024-10-15").as_deref(),
            Some("21.0.5")
        );
        assert_eq!(
            parse_version_string("java version \"1.8.0_402\"\nJava(TM) SE Runtime").as_deref(),
            Some("1.8.0_402")
        );
        assert_eq!(parse_version_string("command not found"), None);
    }

    #[test]
    fn legacy_one_dot_x_versions_report_the_second_component() {
        assert_eq!(major_from_version("1.8.0_402"), 8);
        assert_eq!(major_from_version("21.0.5"), 21);
        assert_eq!(major_from_version("25-ea"), 25);
    }

    fn install(major: u32, arch: &str, source: JavaSource) -> JavaInstallation {
        JavaInstallation {
            path: format!("/jvm/{major}-{arch}-{source:?}/bin/java"),
            major,
            version: major.to_string(),
            from_path: false,
            vendor: None,
            arch: Some(arch.to_string()),
            source: Some(source),
        }
    }

    #[test]
    fn the_exact_major_wins_then_the_lowest_newer_one() {
        let found = vec![
            install(25, "x86_64", JavaSource::System),
            install(21, "x86_64", JavaSource::MinecraftLauncher),
            install(17, "x86_64", JavaSource::System),
            install(8, "x86_64", JavaSource::System),
        ];
        assert_eq!(pick_for_major(&found, 21).map(|i| i.major), Some(21));
        assert_eq!(pick_for_major(&found, 16).map(|i| i.major), Some(17));
        assert_eq!(pick_for_major(&found, 8).map(|i| i.major), Some(8));
        assert!(pick_for_major(&found, 26).is_none(), "nothing older than required");
    }

    #[test]
    fn sixty_four_bit_and_our_own_download_win_ties() {
        let found = vec![
            install(21, "x86", JavaSource::System),
            install(21, "x86_64", JavaSource::System),
            install(21, "x86_64", JavaSource::Nebula),
        ];
        let picked = pick_for_major(&found, 21).expect("one fits");
        assert_eq!(picked.arch.as_deref(), Some("x86_64"));
        assert_eq!(picked.source, Some(JavaSource::Nebula));
    }

    #[test]
    fn release_files_give_version_vendor_and_arch() {
        let text = "IMPLEMENTOR=\"Eclipse Adoptium\"
JAVA_VERSION=\"21.0.5\"
OS_ARCH=\"amd64\"
";
        let fields = parse_release(text);
        assert_eq!(fields.get("JAVA_VERSION").map(String::as_str), Some("21.0.5"));
        assert_eq!(vendor_name(&fields["IMPLEMENTOR"]), "Eclipse Temurin");
        assert_eq!(normalise_arch(&fields["OS_ARCH"]), "x86_64");
        assert_eq!(vendor_name("Azul Systems, Inc."), "Azul Zulu");
        assert_eq!(vendor_name("N/A"), "");
    }

    #[test]
    fn version_output_names_the_vendor_when_there_is_no_release_file() {
        let temurin = "openjdk version \"21.0.5\" 2024-10-15 LTS
OpenJDK Runtime Environment Temurin-21.0.5+11 (build 21.0.5+11-LTS)";
        assert_eq!(vendor_from_version_output(temurin).as_deref(), Some("Eclipse Temurin"));
        let oracle = "java version \"1.8.0_401\"
Java(TM) SE Runtime Environment (build 1.8.0_401-b10)";
        assert_eq!(vendor_from_version_output(oracle).as_deref(), Some("Oracle"));
    }

    #[test]
    fn registry_output_yields_java_homes() {
        let text = r"HKEY_LOCAL_MACHINE\SOFTWARE\JavaSoft\JDK\21
    JavaHome    REG_SZ    C:\Program Files\Java\jdk-21
    MicroVersion    REG_SZ    0
";
        assert_eq!(parse_reg_homes(text), vec![PathBuf::from(r"C:\Program Files\Java\jdk-21")]);
    }

    #[test]
    fn launcher_layouts_are_found_however_deep_they_nest() {
        let root = tempfile::tempdir().expect("tempdir");
        let bin = root.path().join("java-runtime-delta/windows-x64/java-runtime-delta/bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        std::fs::write(bin.join(java_exe()), b"").expect("touch");
        assert_eq!(find_java_under(root.path(), 5), vec![bin.join(java_exe())]);
        assert!(find_java_under(root.path(), 2).is_empty(), "depth is a limit");
    }

    #[test]
    fn downloaded_archives_unpack_into_their_own_home_and_replace_an_older_copy() {
        use std::io::Write;
        let root = tempfile::tempdir().expect("tempdir");
        let runtimes = root.path().join("runtimes");
        std::fs::create_dir_all(&runtimes).expect("runtimes");
        let home = runtimes.join("temurin-21");

        // A zip laid out like Temurin's: one top folder, bin/ inside.
        let zip_path = runtimes.join("OpenJDK21U-jre.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).expect("zip"));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(format!("jdk-21.0.5+11-jre/bin/{}", java_exe()), options).expect("entry");
        zip.write_all(b"java").expect("write");
        zip.start_file("jdk-21.0.5+11-jre/release", options).expect("entry");
        zip.write_all(b"JAVA_VERSION=\"21.0.5\"
").expect("write");
        zip.finish().expect("finish");

        let java = install_archive(&zip_path, &runtimes, &home).expect("unpacks");
        assert_eq!(java, home.join("bin").join(java_exe()));
        assert!(home.join("release").is_file());

        // A tar.gz of the next build replaces it in place.
        let tar_path = runtimes.join("OpenJDK21U-jre.tar.gz");
        {
            let encoder = flate2::write::GzEncoder::new(std::fs::File::create(&tar_path).expect("tar"), flate2::Compression::fast());
            let mut tar = tar::Builder::new(encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(4);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, format!("jdk-21.0.6+7-jre/bin/{}", java_exe()), &b"java"[..]).expect("append");
            tar.into_inner().expect("tar").finish().expect("gz");
        }
        let java = install_archive(&tar_path, &runtimes, &home).expect("unpacks");
        assert!(java.is_file());
        assert!(!home.join("release").exists(), "the old copy is gone, not merged");
        let leftovers: Vec<_> = std::fs::read_dir(&runtimes)
            .expect("read")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "no scratch folders left behind: {leftovers:?}");
    }
}
