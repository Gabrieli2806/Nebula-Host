//! A native MCP (Model Context Protocol) endpoint, so AI agents — Claude
//! Code, Claude Desktop, Cursor, anything that speaks MCP — can manage this
//! machine's servers and launcher instances, and those of the machines paired
//! with it: create and inspect them, start and stop them, install the jar a
//! build just produced, read the console and diagnose crashes.
//!
//! Transport: MCP "Streamable HTTP", answered with plain JSON (no SSE stream —
//! nothing here needs server-initiated messages). One `POST /mcp` per JSON-RPC
//! message or batch.
//!
//! Security posture, matching the rest of the app:
//! - Off until the user turns it on (`AppSettings::mcp_enabled`).
//! - Bound to loopback only: an agent on this PC, never the LAN. There is no
//!   token to paste: whoever can run a program as this user can already do
//!   everything this endpoint does.
//! - Closed to web pages, which is what loopback alone does not stop. A
//!   request must name this machine as its `Host` (a DNS-rebinding page names
//!   its own domain), carry no browser `Origin` other than localhost, and be
//!   `application/json` (a page cannot send that across origins without a
//!   preflight, which gets no permission here).
//! - `mcp_read_only` refuses every tool that changes something.
//! - Another machine is reached only when it is paired and its own owner
//!   turned on `AppSettings::mcp_remote` there (see `remote::agent`).

pub mod activity;
mod diagnose;
mod launcher_tools;
pub(crate) mod tools;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State as AxumState;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::{oneshot, Mutex};
use tracing::{info, warn};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::service::InstanceService;
use crate::store::settings::{self, AppSettings};

/// MCP protocol revisions this server understands, newest first.
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// What the UI shows about the endpoint.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub enabled: bool,
    pub running: bool,
    pub read_only: bool,
    pub launcher_play: bool,
    /// Agents connected to a paired machine may act on this one.
    pub remote: bool,
    pub port: u16,
    pub url: String,
    /// Why it is not running although enabled (port taken…).
    pub error: Option<String>,
    /// The paired machines an agent here can ask for, by name.
    pub machines: Vec<String>,
    /// The agent that connected last, as it named itself.
    pub client: Option<String>,
    /// What agents did most recently, newest first.
    pub activity: Vec<activity::Entry>,
}

struct Running {
    port: u16,
    shutdown: oneshot::Sender<()>,
}

/// Owns the live endpoint. One per app.
#[derive(Default)]
pub struct McpServer {
    running: Mutex<Option<Running>>,
    last_error: Mutex<Option<String>>,
}

#[derive(Clone)]
struct Ctx {
    service: Arc<InstanceService>,
    read_only: bool,
}

impl McpServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bring the endpoint in line with settings: start, stop or restart it.
    pub async fn reconcile(&self, service: Arc<InstanceService>) -> AppResult<McpStatus> {
        let config = settings::load()?;

        let mut running = self.running.lock().await;
        if let Some(current) = running.take() {
            let _ = current.shutdown.send(());
            // Give the listener a moment to release the port before rebinding.
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
        *self.last_error.lock().await = None;

        if config.mcp_enabled {
            let ctx = Ctx { service, read_only: config.mcp_read_only };
            match serve(ctx, config.mcp_port).await {
                Ok(shutdown) => {
                    *running = Some(Running { port: config.mcp_port, shutdown });
                }
                Err(error) => {
                    warn!(%error, port = config.mcp_port, "MCP endpoint could not start");
                    *self.last_error.lock().await = Some(error.to_string());
                }
            }
        }
        drop(running);
        self.status_from(&config).await
    }

    pub async fn status(&self) -> AppResult<McpStatus> {
        let config = settings::load()?;
        self.status_from(&config).await
    }

    async fn status_from(&self, config: &AppSettings) -> AppResult<McpStatus> {
        let running = self.running.lock().await;
        let port = running.as_ref().map(|r| r.port).unwrap_or(config.mcp_port);
        Ok(McpStatus {
            enabled: config.mcp_enabled,
            running: running.is_some(),
            read_only: config.mcp_read_only,
            launcher_play: config.mcp_launcher_play,
            remote: config.mcp_remote,
            port,
            url: format!("http://127.0.0.1:{port}/mcp"),
            error: self.last_error.lock().await.clone(),
            machines: crate::store::peers::load().unwrap_or_default().into_iter().map(|p| p.name).collect(),
            client: activity::client(),
            activity: activity::recent(),
        })
    }

    pub async fn stop(&self) {
        if let Some(current) = self.running.lock().await.take() {
            let _ = current.shutdown.send(());
        }
    }
}

async fn serve(ctx: Ctx, port: u16) -> AppResult<oneshot::Sender<()>> {
    let addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| AppError::new(ErrorCode::PortInUse).with("port", port.to_string()).detail(e.to_string()))?;

    let app = Router::new()
        .route("/mcp", post(handle_post).get(method_not_allowed).delete(method_not_allowed))
        .with_state(ctx);

    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        info!(%addr, "MCP endpoint listening");
        let result = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
        if let Err(error) = result {
            warn!(%error, "MCP endpoint stopped with an error");
        }
        info!("MCP endpoint stopped");
    });
    Ok(tx)
}

async fn method_not_allowed() -> Response {
    // No server-initiated stream and no sessions to delete.
    (StatusCode::METHOD_NOT_ALLOWED, "Use POST").into_response()
}

fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return true; // Native clients send no Origin.
    };
    let host = origin
        .split("://")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("");
    let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// The request names this machine, by loopback address or `localhost`. A
/// DNS-rebinding page reaches the port through its own domain, and that
/// domain is what its requests carry here.
fn host_allowed(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host),
    };
    matches!(name, "localhost" | "127.0.0.1" | "::1")
}

/// JSON, said so. A web page cannot send this content type to another origin
/// without asking first (a preflight), and nothing here grants it.
fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false)
}

async fn handle_post(AxumState(ctx): AxumState<Ctx>, headers: HeaderMap, body: Bytes) -> Response {
    if !origin_allowed(&headers) || !host_allowed(&headers) {
        return (StatusCode::FORBIDDEN, "Only programs on this PC can use this endpoint").into_response();
    }
    if !is_json(&headers) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "Content-Type must be application/json").into_response();
    }

    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            return json_response(rpc_error(Value::Null, -32700, &format!("Parse error: {error}")));
        }
    };

    match parsed {
        Value::Array(messages) => {
            let mut replies = Vec::new();
            for message in messages {
                if let Some(reply) = handle_message(&ctx, message).await {
                    replies.push(reply);
                }
            }
            if replies.is_empty() {
                StatusCode::ACCEPTED.into_response()
            } else {
                json_response(Value::Array(replies))
            }
        }
        message => match handle_message(&ctx, message).await {
            Some(reply) => json_response(reply),
            None => StatusCode::ACCEPTED.into_response(),
        },
    }
}

fn json_response(value: Value) -> Response {
    (StatusCode::OK, [("Content-Type", "application/json")], value.to_string()).into_response()
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// One JSON-RPC message. `None` for notifications and responses.
async fn handle_message(ctx: &Ctx, message: Value) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    // Notifications (no id) and client responses get no reply.
    let id = id?;
    if method.is_empty() {
        return None;
    }

    let reply = match method {
        "initialize" => {
            if let Some(client) = params.pointer("/clientInfo/name").and_then(Value::as_str) {
                activity::set_client(client);
            }
            let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| **v == requested)
                .copied()
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            rpc_result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "nebula-host", "title": "Nebula Host", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": tools::instructions(),
                }),
            )
        }
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({ "tools": tools::definitions(ctx.read_only) })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let outcome = tools::call_routed(&ctx.service, ctx.read_only, &name, &args).await;
            activity::record(&name, &args, None, !outcome.is_error);
            rpc_result(
                id,
                json!({ "content": [{ "type": "text", "text": outcome.text }], "isError": outcome.is_error }),
            )
        }
        "resources/list" => rpc_result(id, json!({ "resources": [] })),
        "prompts/list" => rpc_result(id, json!({ "prompts": [] })),
        other => rpc_error(id, -32601, &format!("Method not found: {other}")),
    };
    Some(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn browser_origins_other_than_localhost_are_refused() {
        let mut headers = HeaderMap::new();
        assert!(origin_allowed(&headers), "native clients send no Origin");
        headers.insert("origin", HeaderValue::from_static("http://localhost:5173"));
        assert!(origin_allowed(&headers));
        headers.insert("origin", HeaderValue::from_static("http://127.0.0.1"));
        assert!(origin_allowed(&headers));
        headers.insert("origin", HeaderValue::from_static("https://evil.example"));
        assert!(!origin_allowed(&headers));
        headers.insert("origin", HeaderValue::from_static("http://localhost.evil.example"));
        assert!(!origin_allowed(&headers));
    }

    #[test]
    fn only_a_request_that_names_this_machine_is_answered() {
        let mut headers = HeaderMap::new();
        assert!(!host_allowed(&headers), "no Host is not this machine");
        for good in ["127.0.0.1:47391", "localhost:47391", "localhost", "[::1]:47391"] {
            headers.insert("host", HeaderValue::from_static(good));
            assert!(host_allowed(&headers), "{good}");
        }
        // What a DNS-rebinding page sends: its own name, resolved to loopback.
        for bad in ["evil.example:47391", "127.0.0.1.evil.example", "192.168.1.10:47391"] {
            headers.insert("host", HeaderValue::from_static(bad));
            assert!(!host_allowed(&headers), "{bad}");
        }
    }

    #[test]
    fn only_json_is_read() {
        let mut headers = HeaderMap::new();
        assert!(!is_json(&headers));
        headers.insert("content-type", HeaderValue::from_static("application/json; charset=utf-8"));
        assert!(is_json(&headers));
        // The types a web page may send across origins without asking.
        for simple in ["text/plain", "application/x-www-form-urlencoded", "multipart/form-data"] {
            headers.insert("content-type", HeaderValue::from_static(simple));
            assert!(!is_json(&headers), "{simple}");
        }
    }
}
