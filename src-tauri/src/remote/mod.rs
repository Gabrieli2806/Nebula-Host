//! Remote access: another Nebula Host (or, later, the phone app) controls this
//! node over the LAN.
//!
//! The node runs a small HTTP API:
//!
//! - `GET  /info`   — who this node is (no auth; lets a client check an address).
//! - `POST /pair`   — trade the 6-digit code shown on the node for an access
//!   token. Codes expire after 5 minutes, are single-use, and five wrong
//!   guesses burn the code.
//! - `POST /rpc`    — `{command, args}`: the same commands the desktop UI sends
//!   over IPC (see [`dispatch`]). Bearer token required.
//! - `GET  /events` — server-sent events: the same `AppEvent` stream the
//!   desktop UI listens to. Bearer token required.
//! - `GET  /local/pairing-code` — issue a code, loopback callers only (how a
//!   headless node shows a code: `nebula-node pair`).
//!
//! Transport is plain HTTP on the LAN; the token is the authentication. This
//! is a LAN feature — never forward this port to the internet.

pub mod dispatch;

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State as AxumState};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{oneshot, Mutex};
use tracing::{info, warn};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::model::PairedDevice;
use crate::node::pairing;
use crate::service::InstanceService;
use crate::store::devices::{self, DeviceCredential};

pub const DEFAULT_PORT: u16 = 47400;
/// Bumped when the remote API changes incompatibly.
pub const API_VERSION: u32 = 1;
const MAX_PAIRING_FAILURES: u32 = 5;

#[derive(Clone)]
struct Ctx {
    service: Arc<InstanceService>,
    failures: Arc<AtomicU32>,
}

/// Origins allowed to call from a browser engine: the desktop app's own
/// webview (Windows / macOS-Linux) and the dev servers.
fn origin_allowed(origin: &str) -> bool {
    matches!(origin, "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost")
        || origin.starts_with("http://localhost:")
        || origin.starts_with("http://127.0.0.1:")
}

async fn cors(request: Request, next: Next) -> Response {
    let origin = request.headers().get("origin").and_then(|v| v.to_str().ok()).map(String::from);
    if let Some(origin) = &origin {
        if !origin_allowed(origin) {
            return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
        }
    }
    let preflight = request.method() == Method::OPTIONS;
    let mut response = if preflight { StatusCode::NO_CONTENT.into_response() } else { next.run(request).await };
    if let Some(origin) = origin.and_then(|o| HeaderValue::from_str(&o).ok()) {
        let headers = response.headers_mut();
        headers.insert("Access-Control-Allow-Origin", origin);
        headers.insert("Access-Control-Allow-Headers", HeaderValue::from_static("authorization, content-type"));
        headers.insert("Access-Control-Allow-Methods", HeaderValue::from_static("GET, POST, OPTIONS"));
        headers.insert("Access-Control-Max-Age", HeaderValue::from_static("600"));
        headers.insert("Vary", HeaderValue::from_static("Origin"));
    }
    response
}

fn error_response(status: StatusCode, error: AppError) -> Response {
    (status, Json(error)).into_response()
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get("authorization")?.to_str().ok()?;
    value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer ")).map(|t| t.trim().to_string())
}

fn authenticate(headers: &HeaderMap) -> Result<String, Response> {
    let unauthorized = || {
        error_response(
            StatusCode::UNAUTHORIZED,
            AppError::new(ErrorCode::InvalidInput).with("reason", "not_paired"),
        )
    };
    let token = bearer(headers).ok_or_else(unauthorized)?;
    let creds = devices::load().map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    devices::device_for_token(&creds, &token).ok_or_else(unauthorized)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    app: &'static str,
    version: &'static str,
    api: u32,
    node_id: String,
    node_name: String,
    platform: &'static str,
}

async fn info(AxumState(ctx): AxumState<Ctx>) -> Json<Info> {
    let node = ctx.service.node().await;
    Json(Info {
        app: "nebula-host",
        version: env!("CARGO_PKG_VERSION"),
        api: API_VERSION,
        node_id: node.id,
        node_name: node.name,
        platform: std::env::consts::OS,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairRequest {
    code: String,
    device_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairResponse {
    token: String,
    device_id: String,
    node: crate::model::Node,
}

async fn pair(AxumState(ctx): AxumState<Ctx>, Json(request): Json<PairRequest>) -> Response {
    let node = ctx.service.node().await;
    if let Err(error) = pairing::redeem(&node.id, &request.code) {
        if error.params.get("reason").map(String::as_str) == Some("code_mismatch") {
            let failures = ctx.failures.fetch_add(1, Ordering::SeqCst) + 1;
            if failures >= MAX_PAIRING_FAILURES {
                // Too many guesses: the code is burnt; the user issues a new one.
                pairing::cancel(&node.id);
                ctx.failures.store(0, Ordering::SeqCst);
                warn!("pairing code cancelled after repeated wrong guesses");
            }
        }
        return error_response(StatusCode::FORBIDDEN, error);
    }
    ctx.failures.store(0, Ordering::SeqCst);

    let token = devices::new_token();
    let device_id = uuid::Uuid::new_v4().to_string();
    let name: String = request.device_name.trim().chars().take(60).collect();
    let name = if name.is_empty() { "Device".to_string() } else { name };

    let result: AppResult<crate::model::Node> = async {
        let mut creds = devices::load()?;
        creds.push(DeviceCredential { device_id: device_id.clone(), token_hash: devices::hash_token(&token) });
        devices::save(&creds)?;
        ctx.service
            .add_paired_device(PairedDevice {
                id: device_id.clone(),
                name,
                public_key: devices::hash_token(&token)[..12].to_string(),
                paired_at: Utc::now(),
                last_seen: None,
            })
            .await
    }
    .await;

    match result {
        Ok(node) => {
            info!(%device_id, "device paired");
            Json(PairResponse { token, device_id, node }).into_response()
        }
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

#[derive(Deserialize)]
struct RpcRequest {
    command: String,
    #[serde(default)]
    args: Value,
}

async fn rpc(AxumState(ctx): AxumState<Ctx>, headers: HeaderMap, Json(request): Json<RpcRequest>) -> Response {
    if let Err(response) = authenticate(&headers) {
        return response;
    }
    let args = if request.args.is_null() { json!({}) } else { request.args };
    match dispatch::dispatch(&ctx.service, &request.command, &args).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(StatusCode::BAD_REQUEST, error),
    }
}

async fn events(
    AxumState(ctx): AxumState<Ctx>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, Response> {
    authenticate(&headers)?;
    let receiver = ctx.service.subscribe();
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    let data = serde_json::to_string(&event).unwrap_or_default();
                    return Some((Ok(Event::default().data(data)), receiver));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// Host is an IP literal or `localhost` — never a DNS name, which is what a
/// DNS-rebinding page would present.
fn host_is_literal(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) else { return false };
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host)
    };
    name.eq_ignore_ascii_case("localhost") || name.parse::<IpAddr>().is_ok()
}

/// Issue a pairing code for the CLI on the node itself. Locked down so no web
/// page can trigger or read it: loopback peer only, POST with a custom header
/// (a cross-origin browser request would need a preflight we never grant), no
/// `Origin` at all, and a literal Host (defeats DNS rebinding).
/// Largest archive a device may upload (a big modded world plus margin).
const MAX_UPLOAD_BYTES: u64 = 32 * 1024 * 1024 * 1024;

/// Stream an archive (a world `.zip`) to the node, for `world_import`.
/// The body is written to disk as it arrives; nothing is held in memory.
async fn upload(headers: HeaderMap, body: Body) -> Response {
    if let Err(response) = authenticate(&headers) {
        return response;
    }
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let result: AppResult<String> = async {
        let id = uuid::Uuid::new_v4().to_string();
        let path = crate::service::world::uploads_dir()?.join(format!("{id}.zip"));
        let mut file = tokio::fs::File::create(&path).await?;
        let mut stream = body.into_data_stream();
        let mut written: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| AppError::new(ErrorCode::InvalidInput).detail(e.to_string()))?;
            written += chunk.len() as u64;
            if written > MAX_UPLOAD_BYTES {
                drop(file);
                let _ = tokio::fs::remove_file(&path).await;
                return Err(AppError::new(ErrorCode::InvalidInput).with("reason", "archive_too_large"));
            }
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(id)
    }
    .await;

    match result {
        Ok(id) => Json(json!({ "uploadId": id })).into_response(),
        Err(error) => error_response(StatusCode::BAD_REQUEST, error),
    }
}

async fn local_pairing_code(
    AxumState(ctx): AxumState<Ctx>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let from_cli = peer.ip().is_loopback()
        && headers.get("origin").is_none()
        && headers.get("x-nebula-local").and_then(|v| v.to_str().ok()) == Some("1")
        && host_is_literal(&headers);
    if !from_cli {
        return (StatusCode::FORBIDDEN, "Only from the node itself").into_response();
    }
    match pairing::issue(&ctx.service.node().await.id) {
        Ok(code) => Json(code).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

fn router(ctx: Ctx) -> Router {
    Router::new()
        .route("/info", get(info))
        .route("/pair", post(pair))
        .route("/rpc", post(rpc))
        .route("/events", get(events))
        .route("/upload", post(upload).layer(axum::extract::DefaultBodyLimit::disable()))
        .route("/local/pairing-code", post(local_pairing_code))
        .fallback(|| async { (StatusCode::NOT_FOUND, Body::empty()) })
        .layer(middleware::from_fn(cors))
        .with_state(ctx)
}

/// Serve the remote API until the returned sender is used or dropped.
pub async fn serve(service: Arc<InstanceService>, port: u16) -> AppResult<oneshot::Sender<()>> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| AppError::new(ErrorCode::PortInUse).with("port", port).detail(e.to_string()))?;
    let app = router(Ctx { service, failures: Arc::new(AtomicU32::new(0)) });
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        info!(%addr, "remote API listening");
        let result = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
        if let Err(error) = result {
            warn!(%error, "remote API stopped with an error");
        }
    });
    Ok(tx)
}

/// What the desktop UI shows about letting other devices in.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    pub addresses: Vec<String>,
    pub error: Option<String>,
}

/// The desktop app's own remote API, opt-in (`AppSettings::remote_enabled`).
#[derive(Default)]
pub struct RemoteServer {
    running: Mutex<Option<(u16, oneshot::Sender<()>)>>,
    last_error: Mutex<Option<String>>,
}

impl RemoteServer {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn reconcile(&self, service: Arc<InstanceService>) -> AppResult<RemoteStatus> {
        let config = crate::store::settings::load()?;
        let mut running = self.running.lock().await;
        if let Some((_, shutdown)) = running.take() {
            let _ = shutdown.send(());
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        *self.last_error.lock().await = None;
        if config.remote_enabled {
            match serve(service, config.remote_port).await {
                Ok(shutdown) => *running = Some((config.remote_port, shutdown)),
                Err(error) => *self.last_error.lock().await = Some(error.to_string()),
            }
        }
        drop(running);
        self.status().await
    }

    pub async fn status(&self) -> AppResult<RemoteStatus> {
        let config = crate::store::settings::load()?;
        let running = self.running.lock().await;
        let port = running.as_ref().map(|(p, _)| *p).unwrap_or(config.remote_port);
        let addresses = crate::net::lan::primary_address()
            .map(|ip| vec![format!("{ip}:{port}")])
            .unwrap_or_default();
        Ok(RemoteStatus {
            enabled: config.remote_enabled,
            running: running.is_some(),
            port,
            addresses,
            error: self.last_error.lock().await.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_endpoint_needs_a_literal_host() {
        let mut h = HeaderMap::new();
        assert!(!host_is_literal(&h));
        h.insert("host", HeaderValue::from_static("127.0.0.1:47400"));
        assert!(host_is_literal(&h));
        h.insert("host", HeaderValue::from_static("localhost:47400"));
        assert!(host_is_literal(&h));
        h.insert("host", HeaderValue::from_static("[::1]:47400"));
        assert!(host_is_literal(&h));
        h.insert("host", HeaderValue::from_static("rebind.evil.example:47400"));
        assert!(!host_is_literal(&h));
    }

    #[test]
    fn only_the_app_and_dev_servers_may_call_from_a_webview() {
        assert!(origin_allowed("http://tauri.localhost"));
        assert!(origin_allowed("tauri://localhost"));
        assert!(origin_allowed("http://localhost:4173"));
        assert!(!origin_allowed("https://evil.example"));
        assert!(!origin_allowed("http://localhost.evil.example"));
    }
}
