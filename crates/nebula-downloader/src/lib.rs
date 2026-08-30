//! Shared download utility for Nebula Hosting.
//!
//! Provides `download_file` with:
//! - HTTP status validation (no silent write of 4xx/5xx bodies)
//! - Retry on transient errors (timeouts, conn resets, 5xx, 408, 429)
//! - `Range` resume on retry — only re-fetches the missing tail when the server supports it
//! - Optional integrity verification (SHA-1 / SHA-256)
//! - Size cap to prevent disk exhaustion
//! - Guaranteed cleanup of the partial destination on any failure

use futures_util::StreamExt;
use reqwest::Client;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

/// Default maximum download file size (500 MB).
pub const DEFAULT_MAX_DOWNLOAD_SIZE: u64 = 500 * 1024 * 1024;

/// Default number of attempts before giving up (retries happen on transient errors only).
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;

/// Algorithm + hex digest for integrity verification.
#[derive(Debug, Clone)]
pub enum Checksum {
    Sha1(String),
    Sha256(String),
}

impl Checksum {
    fn name(&self) -> &'static str {
        match self {
            Checksum::Sha1(_) => "SHA-1",
            Checksum::Sha256(_) => "SHA-256",
        }
    }

    fn expected_hex(&self) -> &str {
        match self {
            Checksum::Sha1(s) | Checksum::Sha256(s) => s,
        }
    }
}

/// Type-erased progress callback so the public API stays object-safe.
pub type ProgressCallback = Box<dyn Fn(u64, u64) + Send + Sync>;

/// Download options. Construct with [`DownloadOptions::new`] and chain setters.
pub struct DownloadOptions<'a> {
    pub url: &'a str,
    pub destination: &'a Path,
    pub expected_checksum: Option<&'a Checksum>,
    pub max_size: u64,
    pub max_attempts: u32,
    pub progress: Option<ProgressCallback>,
}

impl<'a> DownloadOptions<'a> {
    pub fn new(url: &'a str, destination: &'a Path) -> Self {
        Self {
            url,
            destination,
            expected_checksum: None,
            max_size: DEFAULT_MAX_DOWNLOAD_SIZE,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            progress: None,
        }
    }

    pub fn with_checksum(mut self, checksum: &'a Checksum) -> Self {
        self.expected_checksum = Some(checksum);
        self
    }

    pub fn with_max_size(mut self, max_size: u64) -> Self {
        self.max_size = max_size;
        self
    }

    pub fn with_progress<F>(mut self, progress: F) -> Self
    where
        F: Fn(u64, u64) + Send + Sync + 'static,
    {
        self.progress = Some(Box::new(progress));
        self
    }
}

/// Download a file with retry, cleanup, and optional integrity verification.
///
/// On transient failures (timeouts, conn resets, 5xx, 408, 429) the function retries up to
/// `max_attempts` times with exponential backoff. When the server advertises `Accept-Ranges: bytes`
/// and the previous attempt wrote some bytes to disk, retries resume from the last known offset
/// via a `Range: bytes=N-` request, avoiding wasted bandwidth on large installers.
///
/// Returns `Err` with a descriptive message on permanent failure (4xx, integrity mismatch,
/// oversized body). The destination file is removed whenever the function returns `Err`.
pub async fn download_file(client: &Client, opts: DownloadOptions<'_>) -> Result<(), String> {
    let attempts = opts.max_attempts.max(1);
    let mut resume_state = ResumeState::default();
    let mut last_err = String::new();

    for attempt in 1..=attempts {
        match download_once(client, &opts, &mut resume_state).await {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = err;
                if attempt == attempts || !is_transient_error(&last_err) {
                    break;
                }
                let backoff_ms = 500u64 * (1u64 << (attempt - 1));
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
            }
        }
    }

    // Any failure path removes the destination — callers never see a half-written file.
    let _ = std::fs::remove_file(opts.destination);
    Err(last_err)
}

/// Tracks progress across retries so we can resume with a `Range` request when the server
/// supports it. Cleared on any non-resumable response (e.g. server ignores `Range`).
#[derive(Default)]
struct ResumeState {
    bytes_on_disk: u64,
    server_supports_range: bool,
}

async fn download_once(
    client: &Client,
    opts: &DownloadOptions<'_>,
    resume: &mut ResumeState,
) -> Result<(), String> {
    let should_resume = resume.bytes_on_disk > 0 && resume.server_supports_range;

    let mut request = client.get(opts.url);
    if should_resume {
        request = request.header(reqwest::header::RANGE, format!("bytes={}-", resume.bytes_on_disk));
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("download request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("download failed with HTTP {status}"));
    }

    let is_partial = status.as_u16() == 206;
    let accepts_range = response
        .headers()
        .get(reqwest::header::ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("bytes"))
        .unwrap_or(false);

    // Reject oversized downloads upfront when Content-Length is present.
    // For 206 responses, Content-Length is the *remaining* bytes; add the bytes already on disk.
    if let Some(content_length) = response.content_length() {
        let declared_total = if is_partial {
            content_length.saturating_add(resume.bytes_on_disk)
        } else {
            content_length
        };
        if declared_total > opts.max_size {
            return Err(format!(
                "file too large ({} MB), maximum is {} MB",
                declared_total / (1024 * 1024),
                opts.max_size / (1024 * 1024)
            ));
        }
    }

    // Decide whether to append (server honored Range) or start fresh (server ignored it).
    let append_mode = should_resume && is_partial;
    let mut file = if append_mode {
        let mut f = OpenOptions::new()
            .write(true)
            .open(opts.destination)
            .map_err(|e| format!("failed to open destination for resume: {e}"))?;
        f.seek(SeekFrom::Start(resume.bytes_on_disk))
            .map_err(|e| format!("failed to seek for resume: {e}"))?;
        f
    } else {
        resume.bytes_on_disk = 0;
        File::create(opts.destination)
            .map_err(|e| format!("failed to create destination file: {e}"))?
    };

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = resume.bytes_on_disk;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("failed to read download chunk: {e}"))?;
        downloaded += chunk.len() as u64;
        if downloaded > opts.max_size {
            return Err(format!(
                "download exceeded maximum size of {} MB",
                opts.max_size / (1024 * 1024)
            ));
        }
        file.write_all(&chunk)
            .map_err(|e| format!("failed to write download chunk: {e}"))?;
        if let Some(cb) = &opts.progress {
            cb(downloaded, 0);
        }

        // Update persistent offset so a future retry can resume from here.
        resume.bytes_on_disk = downloaded;
        resume.server_supports_range = accepts_range || resume.server_supports_range;
    }

    file.flush().map_err(|e| format!("failed to flush download: {e}"))?;
    drop(file);

    if let Some(expected) = opts.expected_checksum {
        verify_checksum(opts.destination, expected)?;
    }

    Ok(())
}

/// Reads the final file back from disk and compares its digest against `expected`.
/// This keeps the streaming path simple and works correctly even when a retry resumed
/// via `Range` (the in-memory hasher would otherwise miss the previously-written bytes).
fn verify_checksum(path: &Path, expected: &Checksum) -> Result<(), String> {
    use sha1::Digest as _;
    use std::io::Read;

    let mut file = File::open(path)
        .map_err(|e| format!("failed to open file for integrity check: {e}"))?;
    let mut buf = [0u8; 64 * 1024];

    let actual_hex = match expected {
        Checksum::Sha1(_) => {
            let mut hasher = sha1::Sha1::new();
            loop {
                let n = file
                    .read(&mut buf)
                    .map_err(|e| format!("failed to read file for integrity check: {e}"))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            hex::encode(hasher.finalize())
        }
        Checksum::Sha256(_) => {
            use sha2::Digest as _;
            let mut hasher = sha2::Sha256::new();
            loop {
                let n = file
                    .read(&mut buf)
                    .map_err(|e| format!("failed to read file for integrity check: {e}"))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            hex::encode(hasher.finalize())
        }
    };

    if !actual_hex.eq_ignore_ascii_case(expected.expected_hex()) {
        return Err(format!(
            "integrity check failed: expected {} {}, got {}",
            expected.name(),
            expected.expected_hex(),
            actual_hex
        ));
    }

    Ok(())
}

/// Classifies an error message as transient (retry) or permanent (fail fast).
/// Integrity failures and 4xx are never retried — they will not resolve by retrying.
pub fn is_transient_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    if lower.contains("integrity check failed") {
        return false;
    }
    if let Some(idx) = lower.find("http ") {
        let rest = &lower[idx + 5..];
        if let Some(code) = rest.split_whitespace().next().and_then(|s| s.parse::<u16>().ok()) {
            return (500..600).contains(&code) || code == 408 || code == 429;
        }
    }
    lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("connection")
        || lower.contains("reset")
        || lower.contains("eof")
        || lower.contains("dns")
        || lower.contains("tls")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_classification() {
        assert!(is_transient_error("connection reset by peer"));
        assert!(is_transient_error("download failed with HTTP 502 Bad Gateway"));
        assert!(is_transient_error("download failed with HTTP 429 Too Many Requests"));
        assert!(is_transient_error("request timed out"));
        assert!(!is_transient_error("download failed with HTTP 404 Not Found"));
        assert!(!is_transient_error("integrity check failed: expected SHA-1 ..."));
    }
}
