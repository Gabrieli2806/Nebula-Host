//! App logs on disk, so a crash leaves something to read — on this PC or on a
//! headless node — and Settings › Diagnostics can show it (locally or over a
//! remote connection).
//!
//! One file per process kind (`desktop.log`, `node.log`) under
//! `<config>/logs/`, rotated to `.1.log` past 5 MB. Panics are written there
//! too, with their location, before the process goes down.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::error::AppResult;
use crate::store::paths;

const MAX_BYTES: u64 = 5 * 1024 * 1024;

fn log_dir() -> AppResult<PathBuf> {
    let dir = paths::config_dir()?.join("logs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn current_file() -> Option<PathBuf> {
    let dir = log_dir().ok()?;
    ["desktop.log", "node.log"]
        .iter()
        .map(|name| dir.join(name))
        .filter(|path| path.is_file())
        .max_by_key(|path| path.metadata().and_then(|m| m.modified()).ok())
}

fn open(name: &str) -> Option<(PathBuf, File)> {
    let dir = log_dir().ok()?;
    let path = dir.join(format!("{name}.log"));
    if path.metadata().map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join(format!("{name}.1.log")));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
    Some((path, file))
}

/// Log to stdout and to `<config>/logs/<name>.log`. `name` is `desktop` or `node`.
pub fn init(name: &str) {
    let filter = || EnvFilter::try_from_env("NEBULA_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let file = open(name);
    let file_layer = file.as_ref().and_then(|(_, f)| f.try_clone().ok()).map(|f| {
        tracing_subscriber::fmt::layer().with_ansi(false).with_writer(Mutex::new(f))
    });
    let registry = tracing_subscriber::registry().with(filter()).with(tracing_subscriber::fmt::layer());
    match file_layer {
        Some(layer) => {
            let _ = registry.with(layer).try_init();
        }
        None => {
            let _ = registry.try_init();
        }
    }

    // A panic is the crash people report: record it before going down.
    if let Some((path, _)) = file {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Ok(mut f) = OpenOptions::new().append(true).open(&path) {
                let _ = writeln!(
                    f,
                    "{} PANIC {} (v{})",
                    chrono::Utc::now().to_rfc3339(),
                    info,
                    env!("CARGO_PKG_VERSION")
                );
            }
            previous(info);
        }));
    }
}

/// The tail of the newest app log, for Settings › Diagnostics.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppLog {
    pub path: Option<String>,
    pub text: String,
    pub version: &'static str,
    pub platform: String,
}

pub fn tail(max_lines: usize) -> AppLog {
    let path = current_file();
    let text = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|all| {
            let lines: Vec<&str> = all.lines().collect();
            lines[lines.len().saturating_sub(max_lines)..].join("\n")
        })
        .unwrap_or_default();
    AppLog {
        path: path.map(|p| p.display().to_string()),
        text,
        version: env!("CARGO_PKG_VERSION"),
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    }
}

/// The logs folder, for "Open folder" on this PC.
pub fn folder() -> AppResult<String> {
    Ok(log_dir()?.display().to_string())
}
