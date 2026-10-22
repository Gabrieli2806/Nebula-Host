//! A native MCP (Model Context Protocol) endpoint, so AI assistants — Claude
//! Code, Claude Desktop, Cursor, anything that speaks MCP — can manage this
//! node's servers: list and inspect them, start and stop them, create new ones,
//! read their console and diagnose crashes.
//!
//! Transport: MCP "Streamable HTTP", answered with plain JSON (no SSE stream —
//! nothing here needs server-initiated messages). One `POST /mcp` per JSON-RPC
//! message or batch.
//!
//! Security posture, matching the rest of the app:
//! - Off until the user turns it on (`AppSettings::mcp_enabled`).
//! - Bound to loopback only: an assistant on this PC, never the LAN.
//! - Every request needs `Authorization: Bearer <token>`; the token is random
//!   and lives in settings.json.
//! - Requests carrying a browser `Origin` other than localhost are refused, so
//!   a web page cannot drive the endpoint (DNS-rebinding defence).
//! - `mcp_read_only` refuses every tool that changes something.

mod diagnose;
mod launcher_tools;
mod tools;

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
    pub port: u16,
    pub url: String,
    pub token: Option<String>,
    /// Why it is not running although enabled (port taken…).
    pub error: Option<String>,
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
    token: Arc<String>,
    read_only: bool,
}

pub fn new_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

impl McpServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bring the endpoint in line with settings: start, stop or restart it.
    pub async fn reconcile(&self, service: Arc<InstanceService>) -> AppResult<McpStatus> {
        let mut config = settings::load()?;
        if config.mcp_enabled && config.mcp_token.as_deref().is_none_or(str::is_empty) {
            config.mcp_token = Some(new_token());
            settings::save(&config)?;
        }

        let mut running = self.running.lock().await;
        if let Some(current) = running.take() {
            let _ = current.shutdown.send(());
            // Give the listener a moment to release the port before rebinding.
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
        *self.last_error.lock().await = None;

        if config.mcp_enabled {
            let ctx = Ctx {
                service,
                token: Arc::new(config.mcp_token.clone().unwrap_or_default()),
                read_only: config.mcp_read_only,
            };
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
            port,
            url: format!("http://127.0.0.1:{port}/mcp"),
            token: config.mcp_token.clone(),
            error: self.last_error.lock().await.clone(),
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

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    let Some(value) = headers.get("authorization").and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(given) = value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer ")) else {
        return false;
    };
    constant_time_eq(given.trim().as_bytes(), token.as_bytes()) && !token.is_empty()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn handle_post(AxumState(ctx): AxumState<Ctx>, headers: HeaderMap, body: Bytes) -> Response {
    if !origin_allowed(&headers) {
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }
    if !authorized(&headers, &ctx.token) {
        return (
            StatusCode::UNAUTHORIZED,
            [("WWW-Authenticate", "Bearer")],
            "Missing or wrong bearer token",
        )
            .into_response();
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
                    "instructions": tools::INSTRUCTIONS,
                }),
            )
        }
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({ "tools": tools::definitions(ctx.read_only) })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let outcome = tools::call(&ctx.service, ctx.read_only, &name, &args).await;
            let (text, is_error) = match outcome {
                Ok(text) => (text, false),
                Err(error) => (tools::describe_error(&error), true),
            };
            rpc_result(id, json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
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
    fn bearer_token_must_match_exactly() {
        let mut headers = HeaderMap::new();
        assert!(!authorized(&headers, "abc"));
        headers.insert("authorization", HeaderValue::from_static("Bearer abc"));
        assert!(authorized(&headers, "abc"));
        assert!(!authorized(&headers, "abcd"));
        assert!(!authorized(&headers, ""), "an empty token never authorizes");
        headers.insert("authorization", HeaderValue::from_static("Basic abc"));
        assert!(!authorized(&headers, "abc"));
    }

    #[test]
    fn tokens_are_long_and_unique() {
        let a = new_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, new_token());
    }
}
