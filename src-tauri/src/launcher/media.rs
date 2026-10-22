//! Screenshots across instances, and desktop shortcuts that start an
//! instance (the Android home-screen shortcut's counterpart on Windows and
//! Linux).

use super::{client, Profile};
use crate::error::{AppError, AppResult, ErrorCode};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;

pub fn instance_folder(id: uuid::Uuid) -> AppResult<PathBuf> {
    instance_dir(id)
}

fn instance_dir(id: uuid::Uuid) -> AppResult<PathBuf> {
    Ok(client::directory()?.join("profiles").join(id.to_string()))
}

pub fn screenshots_dir(id: uuid::Uuid) -> AppResult<PathBuf> {
    Ok(instance_dir(id)?.join("screenshots"))
}

/// A plain image file name inside an instance's screenshots folder.
fn shot_path(id: uuid::Uuid, file: &str) -> AppResult<PathBuf> {
    let plain = !file.is_empty()
        && !file.contains(['/', '\\', ':'])
        && !file.starts_with('.')
        && file.to_ascii_lowercase().ends_with(".png");
    if !plain {
        return Err(AppError::new(ErrorCode::InvalidInput).with("field", "file"));
    }
    Ok(screenshots_dir(id)?.join(file))
}

#[derive(Serialize)]
pub struct Screenshot {
    pub instance_id: uuid::Uuid,
    pub file: String,
    pub size: u64,
    /// Seconds since the epoch, for newest-first sorting and dates.
    pub taken: u64,
}

/// Every screenshot of every instance, newest first.
pub fn screenshots(profiles: &[Profile]) -> Vec<Screenshot> {
    let mut out = Vec::new();
    for profile in profiles {
        let Ok(dir) = screenshots_dir(profile.id) else { continue };
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() || !name.to_ascii_lowercase().ends_with(".png") {
                continue;
            }
            let taken = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push(Screenshot { instance_id: profile.id, file: name, size: meta.len(), taken });
        }
    }
    out.sort_by(|a, b| b.taken.cmp(&a.taken));
    out
}

/// The image as a data URL (the webview cannot read the folder itself).
pub fn screenshot_data(id: uuid::Uuid, file: &str) -> AppResult<String> {
    use base64::Engine;
    let path = shot_path(id, file)?;
    if std::fs::metadata(&path)?.len() > 40 * 1024 * 1024 {
        return Err(AppError::new(ErrorCode::InvalidInput).detail("Screenshot too large"));
    }
    let bytes = std::fs::read(path)?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

pub fn delete_screenshot(id: uuid::Uuid, file: &str) -> AppResult<()> {
    Ok(std::fs::remove_file(shot_path(id, file)?)?)
}

// -- desktop shortcuts --------------------------------------------------------

/// A shortcut start waiting for the UI: `--play-instance <uuid>`.
static PENDING: Mutex<Option<serde_json::Value>> = Mutex::new(None);

/// Read a shortcut request from command-line arguments (first launch, or a
/// second launch forwarded by the single-instance plugin).
pub fn note_args<I: IntoIterator<Item = String>>(args: I) {
    let args: Vec<String> = args.into_iter().collect();
    let Some(at) = args.iter().position(|a| a == "--play-instance") else { return };
    let Some(id) = args.get(at + 1).and_then(|v| uuid::Uuid::parse_str(v).ok()) else { return };
    let request = serde_json::json!({ "id": id, "requestId": uuid::Uuid::new_v4() });
    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some(request);
    }
}

pub fn take_pending() -> serde_json::Value {
    PENDING.lock().ok().and_then(|mut p| p.take()).unwrap_or(serde_json::Value::Null)
}

/// The instance picture as an .ico (a PNG wrapped in an ICO header, which
/// Windows reads since Vista), so the shortcut shows it.
#[cfg(windows)]
fn icon_file(profile: &Profile) -> Option<PathBuf> {
    use base64::Engine;
    let data = profile.icon.as_deref()?.strip_prefix("data:image/png;base64,")?;
    let png = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
    let mut ico = vec![0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 32, 0];
    ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(&png);
    let path = instance_dir(profile.id).ok()?.join("shortcut.ico");
    std::fs::write(&path, ico).ok()?;
    Some(path)
}

/// Put a "Play <instance>" shortcut on the desktop.
pub fn create_shortcut(profile: &Profile) -> AppResult<PathBuf> {
    let exe = std::env::current_exe()?;
    let desktop = dirs::desktop_dir().ok_or_else(|| AppError::new(ErrorCode::IoFailed).detail("No desktop folder"))?;
    let safe: String = profile
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_()".contains(c) { c } else { '_' })
        .collect();
    let safe = safe.trim();
    let safe = if safe.is_empty() { "Minecraft" } else { safe };

    #[cfg(windows)]
    {
        let link = desktop.join(format!("{safe}.lnk"));
        let icon = icon_file(profile).unwrap_or_else(|| exe.clone());
        // WScript.Shell writes .lnk files; arguments travel as environment
        // variables so no value is ever spliced into the script.
        let script = "$s=(New-Object -ComObject WScript.Shell).CreateShortcut($env:NB_LINK);\
            $s.TargetPath=$env:NB_EXE;$s.Arguments=$env:NB_ARGS;$s.IconLocation=$env:NB_ICON;\
            $s.Description=$env:NB_DESC;$s.Save()";
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("NB_LINK", &link)
            .env("NB_EXE", &exe)
            .env("NB_ARGS", format!("--play-instance {}", profile.id))
            .env("NB_ICON", &icon)
            .env("NB_DESC", format!("Play {} with Nebula Host", profile.name))
            .status()?;
        if !status.success() {
            return Err(AppError::new(ErrorCode::IoFailed).detail("Could not create the shortcut"));
        }
        Ok(link)
    }
    #[cfg(not(windows))]
    {
        use base64::Engine;
        let dir = instance_dir(profile.id)?;
        let icon = profile
            .icon
            .as_deref()
            .and_then(|i| i.strip_prefix("data:image/png;base64,"))
            .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok())
            .and_then(|png| {
                let path = dir.join("shortcut.png");
                std::fs::write(&path, png).ok().map(|_| path.display().to_string())
            })
            .unwrap_or_else(|| "nebula-host".into());
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName={}\nComment=Play with Nebula Host\nExec=\"{}\" --play-instance {}\nIcon={}\nTerminal=false\nCategories=Game;\n",
            profile.name.replace('\n', " "),
            exe.display(),
            profile.id,
            icon
        );
        let link = desktop.join(format!("nebula-{}.desktop", profile.id));
        std::fs::write(&link, &entry)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&link, std::fs::Permissions::from_mode(0o755));
        }
        if let Some(apps) = dirs::data_dir().map(|d| d.join("applications")) {
            let _ = std::fs::create_dir_all(&apps);
            let _ = std::fs::write(apps.join(format!("nebula-{}.desktop", profile.id)), &entry);
        }
        Ok(link)
    }
}
