//! Sharing a log on mclo.gs, the paste site the Minecraft community reads
//! logs on: one link in a support channel instead of a screenshot of a
//! console, and mclo.gs points out the usual problems on its own.
//!
//! The upload happens here rather than in the webview so it works the same
//! for a remote node (whose files are not on this PC), and so the size rules
//! live in one place. The UI asks before anything leaves the machine: logs
//! carry player names and IP addresses.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::model::{ConsoleLevel, ConsoleLine};
use crate::service::InstanceService;

const MCLOGS_API: &str = "https://api.mclo.gs/1/log";

/// mclo.gs keeps the first 10 MiB and 25 000 lines of whatever it is sent
/// and drops the rest silently. Trimming here, below those limits, means the
/// part that survives is the part that matters (the end, where it crashed)
/// and the reader is told something is missing.
const MAX_BYTES: usize = 9 * 1024 * 1024;
const MAX_LINES: usize = 24_000;

/// Crash reports listed, newest first. Older ones are rarely the question.
const MAX_CRASH_REPORTS: usize = 10;

/// What to share.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogSource {
    /// What the console panel holds: this run, as the app saw it.
    Console,
    /// The game's own log of the current or last run.
    Latest,
    /// One file from the game's crash-report folder, by name.
    CrashReport { name: String },
}

/// The logs an instance has on disk, for the share menu.
#[derive(Debug, Clone, Serialize)]
pub struct LogFiles {
    pub latest: Option<LogFile>,
    pub crash_reports: Vec<LogFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogFile {
    pub name: String,
    pub size_bytes: u64,
    pub modified: Option<DateTime<Utc>>,
}

/// A log that is now on mclo.gs.
#[derive(Debug, Clone, Serialize)]
pub struct SharedLog {
    pub id: String,
    /// The page to hand to someone.
    pub url: String,
    /// The plain-text version.
    pub raw: Option<String>,
    /// Lines left out to fit mclo.gs's limits (the oldest ones).
    pub omitted_lines: usize,
}

impl InstanceService {
    pub async fn log_files(&self, instance_id: &str) -> AppResult<LogFiles> {
        let instance = self.store.get(instance_id).await?;
        let Some(spec) = self.registry.manifest(&instance.game)?.logs else {
            return Ok(LogFiles { latest: None, crash_reports: Vec::new() });
        };
        let latest = crate::store::paths::resolve_inside(&instance.path, &spec.latest)
            .ok()
            .and_then(|path| describe(&path));
        let crash_reports = match spec.crash_dir {
            Some(dir) => crate::store::paths::resolve_inside(&instance.path, &dir)
                .map(|dir| list_reports(&dir))
                .unwrap_or_default(),
            None => Vec::new(),
        };
        Ok(LogFiles { latest, crash_reports })
    }

    /// Upload one log to mclo.gs and return its link.
    pub async fn share_log(&self, instance_id: &str, source: LogSource) -> AppResult<SharedLog> {
        let instance = self.store.get(instance_id).await?;
        let text = match source {
            LogSource::Console => {
                let lines = self.console_tail(instance_id, usize::MAX).await;
                if lines.is_empty() {
                    return Err(AppError::new(ErrorCode::NotFound).with("reason", "log_empty"));
                }
                console_text(&lines)
            }
            LogSource::Latest | LogSource::CrashReport { .. } => {
                let spec = self
                    .registry
                    .manifest(&instance.game)?
                    .logs
                    .ok_or_else(|| AppError::new(ErrorCode::GameNotSupported).with("game", &instance.game))?;
                let path = match &source {
                    LogSource::CrashReport { name } => {
                        let dir = spec
                            .crash_dir
                            .ok_or_else(|| AppError::new(ErrorCode::GameNotSupported).with("game", &instance.game))?;
                        // One plain file name, nothing that walks elsewhere.
                        if name.contains(['/', '\\']) {
                            return Err(AppError::new(ErrorCode::PathOutsideInstance).with("path", name));
                        }
                        crate::store::paths::resolve_inside(&instance.path, &format!("{dir}/{name}"))?
                    }
                    _ => crate::store::paths::resolve_inside(&instance.path, &spec.latest)?,
                };
                read_tail(&path).await?
            }
        };
        if text.trim().is_empty() {
            return Err(AppError::new(ErrorCode::NotFound).with("reason", "log_empty"));
        }
        let (content, omitted_lines) = fit_for_upload(&text);
        upload(&content, omitted_lines).await
    }
}

fn describe(path: &Path) -> Option<LogFile> {
    let meta = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    Some(LogFile {
        name: path.file_name()?.to_string_lossy().to_string(),
        size_bytes: meta.len(),
        modified: meta.modified().ok().map(DateTime::<Utc>::from),
    })
}

fn list_reports(dir: &Path) -> Vec<LogFile> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut reports: Vec<LogFile> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            name.ends_with(".txt") || name.ends_with(".log")
        })
        .filter_map(|entry| describe(&entry.path()))
        .collect();
    reports.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.name.cmp(&a.name)));
    reports.truncate(MAX_CRASH_REPORTS);
    reports
}

/// The console as text a reader recognises: the game's own lines as the
/// game wrote them, the app's own notes marked as such.
fn console_text(lines: &[ConsoleLine]) -> String {
    let mut out = String::new();
    for line in lines {
        if line.level == ConsoleLevel::System {
            let time = line.timestamp.with_timezone(&Local).format("%H:%M:%S");
            out.push_str(&format!("[{time}] [Nebula Host]: {}\n", line.message));
        } else {
            out.push_str(&line.message);
            out.push('\n');
        }
    }
    out
}

/// Read a log file, keeping only its end when it is huge: a server left up
/// for a week writes a `latest.log` far past anything worth uploading.
pub(crate) async fn read_tail(path: &Path) -> AppResult<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> AppResult<String> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(&path)?;
        let len = file.metadata()?.len();
        // Twice the upload budget is plenty to trim from by lines.
        let budget = (MAX_BYTES as u64) * 2;
        if len > budget {
            file.seek(SeekFrom::Start(len - budget))?;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut text = String::from_utf8_lossy(&bytes).to_string();
        if len > budget {
            // The cut landed mid-line; drop that partial line.
            if let Some(newline) = text.find('\n') {
                text.drain(..=newline);
            }
        }
        Ok(text)
    })
    .await
    .map_err(AppError::unexpected)?
}

/// Keep the newest lines that fit mclo.gs's limits. Returns the text and how
/// many lines were left out; when any were, the upload says so in its first
/// line, so nobody reads a trimmed log as the whole story.
fn fit_for_upload(text: &str) -> (String, usize) {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = 0usize;
    let mut bytes = 0usize;
    for line in lines.iter().rev() {
        if kept >= MAX_LINES || bytes + line.len() + 1 > MAX_BYTES {
            break;
        }
        bytes += line.len() + 1;
        kept += 1;
    }
    let omitted = lines.len() - kept;
    let body = lines[omitted..].join("\n");
    if omitted == 0 {
        (body, 0)
    } else {
        (format!("[Nebula Host] {omitted} earlier lines were left out to fit mclo.gs's size limit.\n{body}"), omitted)
    }
}

/// Upload any text (trimmed to fit) — used by the launcher's game logs.
pub(crate) async fn share_text(text: &str) -> AppResult<SharedLog> {
    let (content, omitted) = fit_for_upload(text);
    upload(&content, omitted).await
}

async fn upload(content: &str, omitted_lines: usize) -> AppResult<SharedLog> {
    let client = reqwest::Client::builder()
        .user_agent(crate::game::USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(AppError::unexpected)?;
    let response = client
        .post(MCLOGS_API)
        .form(&[("content", content)])
        .send()
        .await
        .map_err(|error| AppError::from(error).with("service", "mclo.gs"))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|error| AppError::new(ErrorCode::UpstreamRejected).with("service", "mclo.gs").detail(error))?;
    parse_response(&body, status.as_u16(), omitted_lines)
}

/// `{"success":true,"id":"…","url":"…","raw":"…"}`, or
/// `{"success":false,"error":"…"}`.
fn parse_response(body: &serde_json::Value, status: u16, omitted_lines: usize) -> AppResult<SharedLog> {
    let text = |key: &str| body.get(key).and_then(|v| v.as_str()).map(str::to_string);
    if body.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return Err(AppError::new(ErrorCode::UpstreamRejected)
            .with("service", "mclo.gs")
            .detail(text("error").unwrap_or_else(|| format!("HTTP {status}"))));
    }
    let (Some(id), Some(url)) = (text("id"), text("url")) else {
        return Err(AppError::new(ErrorCode::UpstreamRejected).with("service", "mclo.gs").detail(body.to_string()));
    };
    Ok(SharedLog { id, url, raw: text("raw"), omitted_lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_logs_go_up_untouched() {
        let (content, omitted) = fit_for_upload("a\nb\nc");
        assert_eq!(content, "a\nb\nc");
        assert_eq!(omitted, 0);
    }

    #[test]
    fn long_logs_keep_their_end_and_say_what_is_missing() {
        let text: String = (0..MAX_LINES + 500).map(|i| format!("line {i}\n")).collect();
        let (content, omitted) = fit_for_upload(&text);
        assert_eq!(omitted, 500);
        assert!(content.starts_with("[Nebula Host] 500 earlier lines"));
        assert!(content.ends_with(&format!("line {}", MAX_LINES + 499)), "the end survives");
        assert!(!content.contains("line 499\n"), "the start is what goes");
    }

    #[test]
    fn huge_lines_are_limited_by_bytes_too() {
        let line = "x".repeat(1024 * 1024);
        let text = vec![line; 20].join("\n");
        let (content, omitted) = fit_for_upload(&text);
        assert!(content.len() <= MAX_BYTES + 200);
        assert!(omitted > 0);
    }

    #[test]
    fn mclogs_answers_parse_into_a_link_or_a_typed_error() {
        let ok = serde_json::json!({"success": true, "id": "8FlTowW", "url": "https://mclo.gs/8FlTowW", "raw": "https://api.mclo.gs/1/raw/8FlTowW"});
        let shared = parse_response(&ok, 200, 0).expect("shared");
        assert_eq!(shared.url, "https://mclo.gs/8FlTowW");
        assert_eq!(shared.id, "8FlTowW");

        let refused = serde_json::json!({"success": false, "error": "Required POST argument 'content' is empty."});
        let error = parse_response(&refused, 400, 0).expect_err("refused");
        assert_eq!(error.code, ErrorCode::UpstreamRejected);
        assert!(error.detail.unwrap_or_default().contains("content"));
    }

    #[test]
    fn crash_reports_are_listed_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, age) in [("crash-old-server.txt", 3), ("crash-new-server.txt", 1), ("notes.md", 0)] {
            let path = dir.path().join(name);
            std::fs::write(&path, "report").expect("write");
            let when = std::time::SystemTime::now() - Duration::from_secs(age * 3600);
            std::fs::File::options().write(true).open(&path).expect("open").set_modified(when).expect("mtime");
        }
        let names: Vec<String> = list_reports(dir.path()).into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["crash-new-server.txt", "crash-old-server.txt"]);
    }
}
