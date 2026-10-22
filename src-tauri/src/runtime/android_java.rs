//! The Java runtimes that ship inside the Android app.
//!
//! Android may not download and run a runtime (Google Play forbids fetching
//! executable code, and the system will not execute files from the app's
//! data folder), so the runtimes are part of the package, as assets. They are
//! not unpacked on their own: a server that needs one says so, and the person
//! installs it from the app — the same "Install Java" step the desktop offers,
//! reading from the package instead of the internet.
//!
//! The package path comes from the activity (`NEBULA_APK_PATH`); an APK is a
//! zip, so the assets are read straight out of it.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::runtime::java::{JavaDownloadProgress, JavaInstallation};

fn abi() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64-v8a",
        "x86_64" => "x86_64",
        "arm" => "armeabi-v7a",
        other => other,
    }
}

fn apk() -> Option<PathBuf> {
    std::env::var_os("NEBULA_APK_PATH").map(PathBuf::from)
}

fn prefix() -> String {
    format!("assets/jre-{}/", abi())
}

/// Where installed runtimes live: `<runtimes>/android-jre/<major>` beside the
/// shared `extra` libraries the launcher expects at `<home>/../extra`.
fn install_root() -> AppResult<PathBuf> {
    Ok(crate::store::paths::runtimes_dir()?.join("android-jre"))
}

/// The Java majors this package carries, newest first.
pub fn bundled_majors() -> Vec<u32> {
    let Some(apk) = apk() else { return Vec::new() };
    let Ok(file) = std::fs::File::open(apk) else { return Vec::new() };
    let Ok(archive) = zip::ZipArchive::new(file) else { return Vec::new() };
    let prefix = prefix();
    let mut majors: Vec<u32> = archive
        .file_names()
        .filter_map(|name| name.strip_prefix(&prefix)?.strip_suffix("/release")?.parse().ok())
        .collect();
    majors.sort_unstable_by(|a, b| b.cmp(a));
    majors.dedup();
    majors
}

/// The runtime to install for a server needing `major`: that one if the
/// package has it, else the lowest newer one (newer Java runs older servers).
pub fn bundled_for(major: u32) -> Option<u32> {
    let majors = bundled_majors();
    majors.iter().copied().find(|&m| m == major).or_else(|| majors.iter().copied().filter(|&m| m > major).min())
}

/// Unpack the runtime for `major` (and the shared libraries) from the package.
/// Written beside the final folder and renamed when complete, so a cut-off
/// install never looks like a working runtime.
pub fn install(major: u32, progress: &dyn Fn(JavaDownloadProgress)) -> AppResult<JavaInstallation> {
    let chosen = bundled_for(major)
        .ok_or_else(|| AppError::new(ErrorCode::JavaNotFound).with("major", major).detail("not in this package"))?;
    let apk = apk().ok_or_else(|| AppError::unexpected("package path unknown"))?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&apk)?).map_err(AppError::unexpected)?;
    let prefix = prefix();
    let root = install_root()?;
    std::fs::create_dir_all(&root)?;

    let wanted = |name: &str| -> Option<String> {
        let rest = name.strip_prefix(&prefix)?;
        (rest.starts_with(&format!("{chosen}/")) || rest.starts_with("extra/")).then(|| rest.to_string())
    };
    let entries: Vec<(usize, String, u64)> = (0..archive.len())
        .filter_map(|index| {
            let file = archive.by_index(index).ok()?;
            let rest = wanted(file.name())?;
            (!file.is_dir()).then(|| (index, rest, file.size()))
        })
        .collect();
    let total: u64 = entries.iter().map(|(_, _, size)| size).sum();

    let staging = root.join(format!(".installing-{chosen}"));
    let _ = std::fs::remove_dir_all(&staging);
    let mut done = 0u64;
    progress(JavaDownloadProgress::Downloading { received: 0, total: Some(total) });
    for (index, rest, _) in &entries {
        let mut file = archive.by_index(*index).map_err(AppError::unexpected)?;
        let target = staging.join(rest);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            std::io::Write::write_all(&mut out, &buffer[..read])?;
            done += read as u64;
        }
        progress(JavaDownloadProgress::Downloading { received: done, total: Some(total) });
    }
    progress(JavaDownloadProgress::Extracting);

    // Move into place: the runtime folder, and the shared libraries.
    let home = root.join(chosen.to_string());
    let _ = std::fs::remove_dir_all(&home);
    std::fs::rename(staging.join(chosen.to_string()), &home)?;
    let extra = root.join("extra");
    let _ = std::fs::remove_dir_all(&extra);
    std::fs::rename(staging.join("extra"), &extra)?;
    let _ = std::fs::remove_dir_all(&staging);

    crate::runtime::java::forget_detected();
    let java = home.join("bin").join("java");
    let installation = describe_installed(&java, chosen);
    progress(JavaDownloadProgress::Done { path: installation.path.clone() });
    Ok(installation)
}

fn describe_installed(java: &Path, major: u32) -> JavaInstallation {
    let release = java.parent().and_then(Path::parent).map(|home| home.join("release"));
    let version = release
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("JAVA_RUNTIME_VERSION=").map(|v| v.trim_matches('"').to_string()))
        })
        .unwrap_or_else(|| major.to_string());
    JavaInstallation {
        path: java.display().to_string(),
        major,
        version,
        from_path: false,
        vendor: Some("OpenJDK".into()),
        arch: Some(std::env::consts::ARCH.into()),
        source: Some(crate::runtime::java::JavaSource::Nebula),
    }
}
