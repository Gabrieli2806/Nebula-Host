//! Running a transfer from the desktop: this machine may be the source, the
//! target, or only the relay between two other nodes. The steps and their
//! rules live in `service::transfer`; this file only calls them in order on
//! the right machines and moves the bytes between them.
//!
//! Bytes never pass through the webview. A local archive is read from disk;
//! a remote one comes from the node's `GET /download/{exportId}` and goes to
//! the target's `POST /upload` (or to this machine's uploads folder), with
//! progress reported on this machine's event channel.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::model::Node;
use crate::service::transfer::{AdoptReport, ExportInfo, FinalizeMode, TransferPhase};
use crate::service::InstanceService;

/// One end of a transfer.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Endpoint {
    /// The machine running the transfer.
    Local,
    /// A paired node, reached with this desktop's token for it.
    Remote { address: String, token: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequest {
    /// Tags the progress events; one is made up when absent.
    #[serde(default)]
    pub transfer_id: Option<String>,
    pub instance_id: String,
    pub source: Endpoint,
    pub target: Endpoint,
    #[serde(default)]
    pub mode: FinalizeMode,
    /// The user confirmed stopping a server with players on it.
    #[serde(default)]
    pub allow_players: bool,
    /// Adopt beside a diverged copy instead of refusing.
    #[serde(default)]
    pub keep_both: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferOutcome {
    pub transfer_id: String,
    pub export: ExportInfo,
    pub adopted: AdoptReport,
    pub mode: FinalizeMode,
    /// The target's clean-up failed; the transfer itself succeeded.
    pub commit_warning: Option<String>,
}

/// The shape of an `AppError` coming back from a node.
#[derive(Deserialize)]
struct WireError {
    code: ErrorCode,
    #[serde(default)]
    params: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    detail: Option<String>,
}

impl From<WireError> for AppError {
    fn from(wire: WireError) -> Self {
        let mut error = AppError::new(wire.code);
        error.params = wire.params;
        error.detail = wire.detail;
        error
    }
}

fn client() -> reqwest::Client {
    // No overall timeout: exporting or verifying a big world takes minutes.
    reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).build().unwrap_or_default()
}

fn unreachable(address: &str, error: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::NodeUnreachable).with("address", address).detail(error)
}

async fn read_answer<T: serde::de::DeserializeOwned>(response: reqwest::Response, address: &str) -> AppResult<T> {
    let status = response.status();
    let bytes = response.bytes().await.map_err(|e| unreachable(address, e))?;
    if !status.is_success() {
        if let Ok(wire) = serde_json::from_slice::<WireError>(&bytes) {
            return Err(wire.into());
        }
        return Err(AppError::new(ErrorCode::Unexpected)
            .with("status", status.as_u16())
            .detail(String::from_utf8_lossy(&bytes)));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

/// A transfer the person called off. Checked between steps and while bytes
/// move; never once the source has started to retire its copy.
#[derive(Default)]
struct CancelToken(AtomicBool);

impl CancelToken {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn tokens() -> &'static Mutex<HashMap<String, Arc<CancelToken>>> {
    static TOKENS: OnceLock<Mutex<HashMap<String, Arc<CancelToken>>>> = OnceLock::new();
    TOKENS.get_or_init(Default::default)
}

/// Call off a running transfer. False when there is none by that id (it
/// already ended, or is past the point where it can be undone).
pub fn cancel(transfer_id: &str) -> bool {
    let token = tokens().lock().ok().and_then(|map| map.get(transfer_id).cloned());
    match token {
        Some(token) => {
            token.0.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

fn cancelled() -> AppError {
    AppError::new(ErrorCode::InvalidInput).with("reason", "transfer_cancelled")
}

struct Runner {
    service: Arc<InstanceService>,
    http: reqwest::Client,
    transfer_id: String,
    instance_id: String,
    cancel: Arc<CancelToken>,
}

impl Runner {
    async fn call<T: serde::de::DeserializeOwned>(&self, endpoint: &Endpoint, command: &str, args: Value) -> AppResult<T> {
        match endpoint {
            Endpoint::Local => {
                let value = crate::remote::dispatch::dispatch(&self.service, command, &args).await?;
                Ok(serde_json::from_value(value)?)
            }
            Endpoint::Remote { address, token } => {
                let response = self
                    .http
                    .post(format!("http://{address}/rpc"))
                    .bearer_auth(token)
                    .json(&json!({ "command": command, "args": args }))
                    .send()
                    .await
                    .map_err(|e| unreachable(address, e))?;
                read_answer(response, address).await
            }
        }
    }

    fn progress(&self, bytes: u64, total: u64) {
        self.service.transfer_progress(&self.instance_id, &self.transfer_id, TransferPhase::Upload, bytes, total);
    }

    /// Move the export from the source to the target; returns the target's
    /// upload id.
    async fn move_bytes(&self, source: &Endpoint, target: &Endpoint, export: &ExportInfo) -> AppResult<String> {
        let total = export.size_bytes;
        let mut last = Instant::now();
        let mut moved = 0u64;
        self.progress(0, total);
        let mut tick = |n: usize, runner: &Runner| {
            moved += n as u64;
            if last.elapsed() >= Duration::from_millis(250) {
                last = Instant::now();
                runner.progress(moved.min(total), total);
            }
        };

        match (source, target) {
            (Endpoint::Local, Endpoint::Local) => Err(AppError::new(ErrorCode::InvalidInput).with("reason", "same_machine")),

            // This machine → a node: read the archive, post it.
            (Endpoint::Local, Endpoint::Remote { address, token }) => {
                let path = crate::service::transfer::export_path(&export.export_id)?;
                let file = tokio::fs::File::open(&path).await?;
                let events = self.service.clone();
                let (instance_id, transfer_id) = (self.instance_id.clone(), self.transfer_id.clone());
                let mut sent = 0u64;
                let mut last = Instant::now();
                let cancel = self.cancel.clone();
                let stream = crate::remote::file_stream(file).map(move |chunk| {
                    if cancel.is_cancelled() {
                        return Err(std::io::Error::other("transfer cancelled"));
                    }
                    if let Ok(bytes) = &chunk {
                        sent += bytes.len() as u64;
                        if last.elapsed() >= Duration::from_millis(250) {
                            last = Instant::now();
                            events.transfer_progress(&instance_id, &transfer_id, TransferPhase::Upload, sent.min(total), total);
                        }
                    }
                    chunk
                });
                let response = self
                    .http
                    .post(format!("http://{address}/upload"))
                    .bearer_auth(token)
                    .header(reqwest::header::CONTENT_LENGTH, total)
                    .body(reqwest::Body::wrap_stream(stream))
                    .send()
                    .await
                    .map_err(|e| unreachable(address, e))?;
                let answer: Value = read_answer(response, address).await?;
                self.progress(total, total);
                upload_id(&answer)
            }

            // A node → this machine: download into the uploads folder.
            (Endpoint::Remote { address, token }, Endpoint::Local) => {
                let response = self.download(address, token, &export.export_id).await?;
                let id = uuid::Uuid::new_v4().to_string();
                let path = crate::service::world::uploads_dir()?.join(format!("{id}.zip"));
                let result: AppResult<()> = async {
                    let mut file = tokio::fs::File::create(&path).await?;
                    let mut stream = response.bytes_stream();
                    while let Some(chunk) = stream.next().await {
                        if self.cancel.is_cancelled() {
                            return Err(cancelled());
                        }
                        let chunk = chunk.map_err(|e| unreachable(address, e))?;
                        file.write_all(&chunk).await?;
                        tick(chunk.len(), self);
                    }
                    file.flush().await?;
                    Ok(())
                }
                .await;
                if let Err(error) = result {
                    let _ = tokio::fs::remove_file(&path).await;
                    return Err(error);
                }
                self.progress(total, total);
                Ok(id)
            }

            // Node → node, relayed: the download body becomes the upload body.
            (Endpoint::Remote { address: from, token: from_token }, Endpoint::Remote { address: to, token: to_token }) => {
                let response = self.download(from, from_token, &export.export_id).await?;
                let events = self.service.clone();
                let (instance_id, transfer_id) = (self.instance_id.clone(), self.transfer_id.clone());
                let mut sent = 0u64;
                let mut last = Instant::now();
                let cancel = self.cancel.clone();
                let stream = response.bytes_stream().map(move |chunk| {
                    if cancel.is_cancelled() {
                        return Err(std::io::Error::other("transfer cancelled"));
                    }
                    if let Ok(bytes) = &chunk {
                        sent += bytes.len() as u64;
                        if last.elapsed() >= Duration::from_millis(250) {
                            last = Instant::now();
                            events.transfer_progress(&instance_id, &transfer_id, TransferPhase::Upload, sent.min(total), total);
                        }
                    }
                    chunk.map_err(std::io::Error::other)
                });
                let response = self
                    .http
                    .post(format!("http://{to}/upload"))
                    .bearer_auth(to_token)
                    .header(reqwest::header::CONTENT_LENGTH, total)
                    .body(reqwest::Body::wrap_stream(stream))
                    .send()
                    .await
                    .map_err(|e| unreachable(to, e))?;
                let answer: Value = read_answer(response, to).await?;
                self.progress(total, total);
                upload_id(&answer)
            }
        }
    }

    async fn download(&self, address: &str, token: &str, export_id: &str) -> AppResult<reqwest::Response> {
        let response = self
            .http
            .get(format!("http://{address}/download/{export_id}"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| unreachable(address, e))?;
        if !response.status().is_success() {
            return read_answer::<Value>(response, address).await.and(Err(AppError::new(ErrorCode::FileNotFound)));
        }
        Ok(response)
    }
}

fn upload_id(answer: &Value) -> AppResult<String> {
    answer
        .get("uploadId")
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| AppError::unexpected("no upload id"))
}

/// Run a whole transfer. On any failure before the source retires its copy,
/// the source stays the server and the target is left as it was.
pub async fn run(service: Arc<InstanceService>, request: TransferRequest) -> AppResult<TransferOutcome> {
    let transfer_id = request.transfer_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let cancel = Arc::new(CancelToken::default());
    if let Ok(mut map) = tokens().lock() {
        map.insert(transfer_id.clone(), cancel.clone());
    }
    let runner = Runner {
        service: service.clone(),
        http: client(),
        transfer_id: transfer_id.clone(),
        instance_id: request.instance_id.clone(),
        cancel,
    };
    let result = run_steps(&runner, &request).await;
    if let Ok(mut map) = tokens().lock() {
        map.remove(&transfer_id);
    }
    service.transfer_finished(&request.instance_id, &transfer_id, result.is_err());
    result
}

/// An error the desktop cannot read as the other machine's answer: the
/// call may or may not have run there. Errors from this machine itself are
/// always definite.
fn uncertain(endpoint: &Endpoint, error: &AppError) -> bool {
    matches!(endpoint, Endpoint::Remote { .. }) && matches!(error.code, ErrorCode::NodeUnreachable | ErrorCode::Unexpected)
}

/// How often, and how far apart, a finalize whose answer was lost is asked
/// again. It is safe to repeat.
const FINALIZE_RETRIES: [Duration; 4] =
    [Duration::from_secs(2), Duration::from_secs(5), Duration::from_secs(10), Duration::from_secs(20)];

async fn run_steps(runner: &Runner, request: &TransferRequest) -> AppResult<TransferOutcome> {
    let (source, target) = (&request.source, &request.target);
    let source_node: Node = runner.call(source, "node_get", json!({})).await?;
    let target_node: Node = runner.call(target, "node_get", json!({})).await?;
    if source_node.id == target_node.id {
        return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "same_machine"));
    }
    if runner.cancel.is_cancelled() {
        return Err(cancelled());
    }

    // 1–2. Stop, lock and pack on the source.
    let export: ExportInfo = match runner
        .call(
            source,
            "transfer_export",
            json!({
                "instanceId": request.instance_id,
                "transferId": runner.transfer_id,
                "allowPlayers": request.allow_players,
            }),
        )
        .await
    {
        Ok(export) => export,
        Err(error) => {
            // The answer may be lost while the source holds the lock: call it
            // off by this transfer's id (a no-op when nothing is held).
            if uncertain(source, &error) {
                let args = json!({ "instanceId": request.instance_id, "transferId": runner.transfer_id });
                if let Err(undo) = runner.call::<Value>(source, "transfer_abort", args).await {
                    warn!(%undo, "source could not be unlocked after a lost export answer; it unlocks on restart");
                }
            }
            return Err(error);
        }
    };

    let abort = || async {
        let args = json!({ "instanceId": request.instance_id, "exportId": export.export_id });
        if let Err(error) = runner.call::<Value>(source, "transfer_abort", args).await {
            warn!(%error, "source could not be unlocked after a failed transfer; it unlocks on restart");
        }
    };

    // Called off while the source was stopping and packing: nothing has left
    // it, so unlocking it is all there is to undo.
    if runner.cancel.is_cancelled() {
        abort().await;
        return Err(cancelled());
    }

    // 3. Bytes, source → target.
    let upload_id = match runner.move_bytes(source, target, &export).await {
        Ok(id) => id,
        Err(error) => {
            abort().await;
            // A stream cut on purpose fails like a broken connection; say what it was.
            return Err(if runner.cancel.is_cancelled() { cancelled() } else { error });
        }
    };
    if runner.cancel.is_cancelled() {
        abort().await;
        return Err(cancelled());
    }

    // 4. Verify and adopt on the target.
    let adopted: AdoptReport = match runner
        .call(
            target,
            "transfer_adopt",
            json!({
                "uploadId": upload_id,
                "sha256": export.sha256,
                "keepBoth": request.keep_both,
                "transferId": runner.transfer_id,
            }),
        )
        .await
    {
        Ok(report) => report,
        Err(error) => {
            // The target may have adopted it and the answer been lost: undo
            // whatever this transfer adopted there before unlocking the source.
            if uncertain(target, &error) {
                let args = json!({ "transferId": runner.transfer_id });
                if let Err(undo) = runner.call::<Value>(target, "transfer_discard", args).await {
                    warn!(%undo, "target could not be checked for a half-finished adoption");
                }
            }
            abort().await;
            return Err(error);
        }
    };

    // Last chance to call it off: the target has the copy but the source has
    // not retired anything yet, so undo the adoption and unlock the source.
    if runner.cancel.is_cancelled() {
        if let Err(undo) =
            runner.call::<Value>(target, "transfer_discard", json!({ "instanceId": adopted.instance.id })).await
        {
            warn!(%undo, "target could not undo its adoption; it holds an extra copy");
        }
        abort().await;
        return Err(cancelled());
    }

    // 5. Only now does the source retire its copy. From here it cannot be
    // called off.
    if let Ok(mut map) = tokens().lock() {
        map.remove(&runner.transfer_id);
    }
    runner.service.transfer_progress(&request.instance_id, &runner.transfer_id, TransferPhase::Finalize, 0, 0);
    let finalize_args = json!({
        "instanceId": request.instance_id,
        "exportId": export.export_id,
        "mode": request.mode,
        "toNodeId": target_node.id,
        "toNodeName": target_node.name,
        "generation": adopted.instance.generation,
    });
    let mut finalized = runner.call::<Value>(source, "transfer_finalize", finalize_args.clone()).await;
    for wait in FINALIZE_RETRIES {
        match &finalized {
            Err(error) if uncertain(source, error) => {
                warn!(%error, "no answer from the source to finalize; asking again");
                tokio::time::sleep(wait).await;
                finalized = runner.call::<Value>(source, "transfer_finalize", finalize_args.clone()).await;
            }
            _ => break,
        }
    }
    match finalized {
        Ok(_) => {}
        Err(error) if uncertain(source, &error) => {
            // Unknown whether the source retired its copy. Undoing the target
            // could leave no copy at all, so both stay: the target's copy
            // waits (locked) and the source's is either the older copy or
            // still locked. Restarting either machine releases its lock.
            warn!(%error, "finalize outcome unknown; both copies kept");
            return Err(AppError::new(ErrorCode::NodeUnreachable)
                .with("reason", "finalize_unknown")
                .with("source", &source_node.name)
                .with("target", &target_node.name)
                .detail(error.to_string()));
        }
        Err(error) => {
            // The source refused, so it still has the server: undo the
            // target, unlock the source.
            if let Err(undo) =
                runner.call::<Value>(target, "transfer_discard", json!({ "instanceId": adopted.instance.id })).await
            {
                warn!(%undo, "target could not undo its adoption; it holds an extra copy");
            }
            abort().await;
            return Err(error);
        }
    }

    // 6. The target forgets what it replaced. Not fatal.
    let commit_warning = runner
        .call::<Value>(target, "transfer_commit", json!({ "instanceId": adopted.instance.id }))
        .await
        .err()
        .map(|error| error.to_string());

    info!(
        instance_id = %request.instance_id,
        from = %source_node.name,
        to = %target_node.name,
        generation = adopted.instance.generation,
        "server transferred"
    );
    Ok(TransferOutcome { transfer_id: runner.transfer_id.clone(), export, adopted, mode: request.mode, commit_warning })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_running_transfer_can_be_called_off() {
        assert!(!cancel("nothing-by-this-id"));

        let token = Arc::new(CancelToken::default());
        tokens().lock().unwrap().insert("t-cancel".into(), token.clone());
        assert!(!token.is_cancelled());
        assert!(cancel("t-cancel"));
        assert!(token.is_cancelled());

        // Past the point of no return the token is gone: asking is a no-op.
        tokens().lock().unwrap().remove("t-cancel");
        assert!(!cancel("t-cancel"));
    }

    #[test]
    fn a_cancelled_transfer_has_its_own_reason() {
        let error = cancelled();
        assert_eq!(error.params.get("reason").map(String::as_str), Some("transfer_cancelled"));
    }
}
