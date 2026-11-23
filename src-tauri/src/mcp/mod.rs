//! A native MCP (Model Context Protocol) endpoint, so AI agents — Claude
//! Code, Codex, Claude Desktop, Cursor, anything that speaks MCP — can manage
//! this machine's servers and launcher instances, and those of the machines
//! paired with it: create and inspect them, start and stop them, install the
//! jar a build just produced, read the console and diagnose crashes.
//!
//! Transport: MCP "Streamable HTTP", answered with plain JSON (no SSE stream,
//! no sessions — nothing here needs server-initiated messages). One
//! `POST /mcp` per JSON-RPC message. Two eras of the protocol share the
//! endpoint: the ones that open with `initialize` (2024-11-05 to 2025-11-25)
//! and the stateless one (2026-07-28), where every request names its version
//! in `_meta` and `server/discover` replaces the handshake. The endpoint
//! follows a contract the author's apps share, kept beside this repository
//! with the script that checks a running app against it (`mcp-check.py`); the
//! tests below follow that script.
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
mod console_wait;
mod diagnose;
mod launcher_tools;
mod profile;
mod prompts;
pub(crate) mod tools;

use std::future::Future;
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

/// The stateless revision: no `initialize`, the version travels in `_meta`.
const MODERN_VERSION: &str = "2026-07-28";
/// The revisions that open with `initialize`, newest first. A client asking
/// for one this server does not know is offered the first.
const LEGACY_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CLIENT: &str = "io.modelcontextprotocol/clientInfo";
const META_SERVER: &str = "io.modelcontextprotocol/serverInfo";

/// How long a stateless client may keep `server/discover` and the lists. The
/// tools change only when the owner flips "view only", which is rare.
const TTL_MS: u64 = 60_000;

/// JSON-RPC: the request names a protocol version this server does not speak.
const UNSUPPORTED_VERSION: i64 = -32022;

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

/// What the endpoint needs from the app: whether tools that change something
/// are refused, and a way to run a tool. The app's is `Ctx`; the tests bring
/// one that needs no servers.
trait Host: Clone + Send + Sync + 'static {
    fn read_only(&self) -> bool;
    fn call(&self, name: &str, args: &Value) -> impl Future<Output = tools::Outcome> + Send;
}

impl Host for Ctx {
    fn read_only(&self) -> bool {
        self.read_only
    }

    async fn call(&self, name: &str, args: &Value) -> tools::Outcome {
        tools::call_routed(&self.service, self.read_only, name, args).await
    }
}

fn router<H: Host>(host: H) -> Router {
    Router::new()
        .route("/mcp", post(handle_post::<H>).get(method_not_allowed).delete(method_not_allowed))
        .with_state(host)
}

async fn serve(ctx: Ctx, port: u16) -> AppResult<oneshot::Sender<()>> {
    let addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| AppError::new(ErrorCode::PortInUse).with("port", port.to_string()).detail(e.to_string()))?;

    let app = router(ctx);

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
    (StatusCode::METHOD_NOT_ALLOWED, [("Allow", "POST")], "Use POST").into_response()
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

async fn handle_post<H: Host>(AxumState(host): AxumState<H>, headers: HeaderMap, body: Bytes) -> Response {
    if !origin_allowed(&headers) || !host_allowed(&headers) {
        return (StatusCode::FORBIDDEN, "Only programs on this PC can use this endpoint").into_response();
    }
    if !is_json(&headers) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "Content-Type must be application/json").into_response();
    }

    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                rpc_error(Value::Null, -32700, &format!("Parse error: {error}")),
            );
        }
    };
    // Clients name the revision they speak in this header after the handshake
    // (and, in the stateless era, on every request).
    let header_version = headers.get("mcp-protocol-version").and_then(|v| v.to_str().ok()).map(str::trim);

    match parsed {
        // Batches left MCP in 2025-06-18. Older clients may still send one,
        // and answering it costs nothing.
        Value::Array(messages) if messages.is_empty() => {
            json_response(StatusCode::BAD_REQUEST, rpc_error(Value::Null, -32600, "Invalid request: empty batch"))
        }
        Value::Array(messages) => {
            let mut replies = Vec::new();
            for message in messages {
                if let Some(reply) = handle_message(&host, header_version, message).await {
                    replies.push(reply.body);
                }
            }
            if replies.is_empty() {
                StatusCode::ACCEPTED.into_response()
            } else {
                json_response(StatusCode::OK, Value::Array(replies))
            }
        }
        message => match handle_message(&host, header_version, message).await {
            Some(reply) => json_response(reply.status, reply.body),
            None => StatusCode::ACCEPTED.into_response(),
        },
    }
}

fn json_response(status: StatusCode, value: Value) -> Response {
    (status, [("Content-Type", "application/json")], value.to_string()).into_response()
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// The answer to one message, and the HTTP status it travels with.
struct Reply {
    status: StatusCode,
    body: Value,
}

impl Reply {
    fn ok(body: Value) -> Self {
        Self { status: StatusCode::OK, body }
    }
}

fn supported_versions() -> Vec<&'static str> {
    std::iter::once(MODERN_VERSION).chain(LEGACY_VERSIONS.iter().copied()).collect()
}

fn server_info() -> Value {
    json!({ "name": "nebula-host", "title": "Nebula Host", "version": env!("CARGO_PKG_VERSION") })
}

fn capabilities() -> Value {
    json!({ "tools": { "listChanged": false }, "prompts": { "listChanged": false } })
}

/// What a stateless result carries on top of its own fields: that it is
/// whole and, for what a client may keep (the lists, `server/discover`), for
/// how long.
fn modern_result(mut result: Value, cacheable: bool) -> Value {
    result["resultType"] = json!("complete");
    if cacheable {
        result["ttlMs"] = json!(TTL_MS);
        result["cacheScope"] = json!("public");
    }
    result
}

/// The tool's text, and the same thing as data when the text is JSON: an
/// object as it is, a list as `{ "items": [...] }` (`structuredContent` has
/// to be an object).
fn tool_result(outcome: &tools::Outcome) -> Value {
    let mut result = json!({ "content": [{ "type": "text", "text": outcome.text }], "isError": outcome.is_error });
    let text = outcome.text.trim_start();
    if !outcome.is_error && (text.starts_with('{') || text.starts_with('[')) {
        match serde_json::from_str::<Value>(text) {
            Ok(value @ Value::Object(_)) => result["structuredContent"] = value,
            Ok(Value::Array(items)) => result["structuredContent"] = json!({ "items": items }),
            _ => {}
        }
    }
    result
}

/// One JSON-RPC message. `None` for notifications and responses.
async fn handle_message<H: Host>(host: &H, header_version: Option<&str>, message: Value) -> Option<Reply> {
    if !message.is_object() {
        return Some(Reply::ok(rpc_error(Value::Null, -32600, "Invalid request: a JSON-RPC message is an object")));
    }
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let meta = params.get("_meta");

    // Which revision the request speaks: named in `_meta` (stateless), in the
    // header, or not at all (the handshake, and clients that never say).
    let version = meta.and_then(|m| m.get(META_VERSION)).and_then(Value::as_str).or(header_version);
    let supported = supported_versions();
    if let Some(requested) = version.filter(|v| !supported.contains(v)) {
        let mut error = rpc_error(
            id.unwrap_or(Value::Null),
            UNSUPPORTED_VERSION,
            &format!("Unsupported protocol version: {requested}. Use one of: {}.", supported.join(", ")),
        );
        error["error"]["data"] = json!({ "supported": supported, "requested": requested });
        return Some(Reply { status: StatusCode::BAD_REQUEST, body: error });
    }
    let modern = version == Some(MODERN_VERSION);
    if let Some(client) = meta.and_then(|m| m.get(META_CLIENT)).and_then(|c| c.get("name")).and_then(Value::as_str) {
        activity::set_client(client);
    }

    // Notifications (no id) and client responses get no reply.
    let id = id?;
    if method.is_empty() {
        return None;
    }
    let finish = |result: Value, cacheable: bool| if modern { modern_result(result, cacheable) } else { result };

    let reply = match method {
        "initialize" => {
            if let Some(client) = params.pointer("/clientInfo/name").and_then(Value::as_str) {
                activity::set_client(client);
            }
            let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
            let version = LEGACY_VERSIONS
                .iter()
                .find(|v| **v == requested)
                .copied()
                .unwrap_or(LEGACY_VERSIONS[0]);
            rpc_result(
                id,
                finish(
                    json!({
                        "protocolVersion": version,
                        "capabilities": capabilities(),
                        "serverInfo": server_info(),
                        "instructions": tools::instructions(),
                    }),
                    false,
                ),
            )
        }
        // The stateless handshake: what `initialize` says, for a client to keep.
        "server/discover" => rpc_result(
            id,
            modern_result(
                json!({
                    "supportedVersions": supported,
                    "capabilities": capabilities(),
                    "instructions": tools::instructions(),
                    "_meta": { META_SERVER: server_info() },
                }),
                true,
            ),
        ),
        "ping" => rpc_result(id, finish(json!({}), false)),
        "tools/list" => rpc_result(id, finish(json!({ "tools": tools::definitions(host.read_only()) }), true)),
        "tools/call" => {
            let asked = params.get("name").and_then(Value::as_str).unwrap_or("");
            // A tool that does not exist is a malformed request; one that
            // exists and fails is a result with `isError`.
            let Some(name) = tools::known_name(asked) else {
                return Some(Reply::ok(rpc_error(
                    id,
                    -32602,
                    &format!("Unknown tool: {asked}. Call tools/list for the tools this server has."),
                )));
            };
            let args = match params.get("arguments") {
                Some(args) if args.is_object() => args.clone(),
                _ => json!({}),
            };
            let outcome = host.call(name, &args).await;
            activity::record(name, &args, None, !outcome.is_error);
            rpc_result(id, finish(tool_result(&outcome), false))
        }
        "resources/list" => rpc_result(id, finish(json!({ "resources": [] }), true)),
        "resources/templates/list" => rpc_result(id, finish(json!({ "resourceTemplates": [] }), true)),
        "prompts/list" => rpc_result(id, finish(json!({ "prompts": prompts::list(host.read_only()) }), true)),
        "prompts/get" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            match prompts::get(host.read_only(), name, &arguments) {
                Ok(prompt) => rpc_result(id, finish(prompt, false)),
                Err(message) => rpc_error(id, -32602, &message),
            }
        }
        other => rpc_error(id, -32601, &format!("Method not found: {other}")),
    };
    Some(Reply::ok(reply))
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

    // -- The endpoint over real HTTP, with tools that need no servers. These
    // -- follow the shared checker (`mcp-check.py`), case by case.

    #[derive(Clone)]
    struct Fake {
        read_only: bool,
    }

    impl Host for Fake {
        fn read_only(&self) -> bool {
            self.read_only
        }

        async fn call(&self, name: &str, _args: &Value) -> tools::Outcome {
            let (text, is_error) = match name {
                "list_servers" => ("[\n  { \"name\": \"Test Paper\", \"status\": \"online\" }\n]", false),
                "get_machine_status" => ("{\n  \"cpu_percent\": 3.5\n}", false),
                "get_console" => ("12:00:00 [INFO] Done (3.2s)! {not json}", false),
                _ => ("Error: instance_not_found (instance=nope)\nCall list_servers for the servers there are.", true),
            };
            tools::Outcome { text: text.into(), is_error }
        }
    }

    /// Serve the real router on a free loopback port; its URL.
    async fn endpoint(read_only: bool) -> String {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router(Fake { read_only })).await;
        });
        format!("http://{addr}/mcp")
    }

    /// One exchange: status, content type, body.
    async fn send(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&str>) -> (u16, String, String) {
        let mut request = reqwest::Client::new().request(method.parse().unwrap(), url);
        if !headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("content-type")) {
            request = request.header("Content-Type", "application/json");
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if let Some(body) = body {
            request = request.body(body.to_string());
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let content_type =
            response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        (status, content_type, response.text().await.unwrap())
    }

    async fn rpc_with(url: &str, headers: &[(&str, &str)], method: &str, params: Value) -> (u16, Value) {
        let mut message = json!({ "jsonrpc": "2.0", "id": 7, "method": method });
        if !params.is_null() {
            message["params"] = params;
        }
        let (status, _, text) = send("POST", url, headers, Some(&message.to_string())).await;
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn rpc(url: &str, method: &str, params: Value) -> (u16, Value) {
        rpc_with(url, &[], method, params).await
    }

    fn modern_meta(version: &str) -> Value {
        json!({ "_meta": {
            META_VERSION: version,
            "io.modelcontextprotocol/clientCapabilities": {},
            META_CLIENT: { "name": "mcp-check", "version": "1" },
        } })
    }

    fn names(result: &Value) -> Vec<String> {
        result["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
    }

    /// To run the shared checker (`mcp-check.py nebula-host`) without the app open:
    /// `cargo test -p nebula-host --lib serve_for_the_checker -- --ignored`
    /// serves this endpoint (with the tools above) on the app's port for a minute.
    /// With the app open the port is taken: set `NEBULA_MCP_CHECK_PORT` to
    /// another one and give the checker the address instead of the name.
    #[tokio::test]
    #[ignore = "holds the app's port for a minute, for the checker"]
    async fn serve_for_the_checker() {
        let port = std::env::var("NEBULA_MCP_CHECK_PORT").ok().and_then(|port| port.parse().ok()).unwrap_or(47391);
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await.expect("the port is free");
        let serving = axum::serve(listener, router(Fake { read_only: false }));
        let _ = tokio::time::timeout(std::time::Duration::from_secs(60), async { serving.await }).await;
    }

    #[tokio::test]
    async fn the_handshake_answers_every_legacy_version() {
        let url = endpoint(false).await;
        let initialize = |version: &str| json!({ "protocolVersion": version, "capabilities": {}, "clientInfo": { "name": "mcp-check", "version": "1" } });

        let (status, content_type, text) = send(
            "POST",
            &url,
            &[],
            Some(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": initialize("2025-06-18") }).to_string()),
        )
        .await;
        assert_eq!(status, 200);
        assert!(content_type.contains("application/json"), "{content_type}");
        let result = &serde_json::from_str::<Value>(&text).unwrap()["result"];
        assert_eq!(result["protocolVersion"], "2025-06-18");
        assert_eq!(result["serverInfo"]["name"], "nebula-host");
        assert_eq!(result["serverInfo"]["title"], "Nebula Host");
        assert!(result["serverInfo"]["version"].as_str().is_some_and(|v| !v.is_empty()));
        assert!(result["capabilities"]["tools"].is_object());
        let instructions = result["instructions"].as_str().unwrap();
        assert!(!instructions.is_empty() && instructions.chars().count() <= 4096);
        assert!(result.get("resultType").is_none(), "the handshake is not a stateless result");

        for version in LEGACY_VERSIONS {
            let (_, body) = rpc(&url, "initialize", initialize(version)).await;
            assert_eq!(body["result"]["protocolVersion"], *version);
        }
        // A version nobody knows is offered the newest of this era.
        for unknown in ["1999-01-01", MODERN_VERSION] {
            let (_, body) = rpc(&url, "initialize", initialize(unknown)).await;
            assert_eq!(body["result"]["protocolVersion"], "2025-11-25", "{unknown}");
        }
    }

    #[tokio::test]
    async fn the_basic_methods_answer_as_the_contract_says() {
        let url = endpoint(false).await;

        let (status, _, text) =
            send("POST", &url, &[], Some(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)).await;
        assert_eq!((status, text.as_str()), (202, ""));

        let (status, body) = rpc(&url, "ping", Value::Null).await;
        assert_eq!(status, 200);
        assert_eq!(body["result"], json!({}));
        assert_eq!(body["id"], 7);

        let (_, body) = rpc(&url, "no/such_method", Value::Null).await;
        assert_eq!(body["error"]["code"], -32601);

        for (method, key) in [("resources/list", "resources"), ("resources/templates/list", "resourceTemplates")] {
            let (_, body) = rpc(&url, method, Value::Null).await;
            assert_eq!(body["result"][key], json!([]), "{method}");
        }
    }

    #[tokio::test]
    async fn prompts_are_listed_and_given_as_a_task_for_the_agent() {
        let url = endpoint(false).await;

        let (_, body) = rpc(&url, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {} })).await;
        assert!(body["result"]["capabilities"]["prompts"].is_object());

        let (_, body) = rpc(&url, "prompts/list", Value::Null).await;
        let names: Vec<&str> = body["result"]["prompts"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
        assert_eq!(names, ["test_plugin", "diagnose_crash"]);
        assert!(body["result"].get("resultType").is_none());

        let (status, body) =
            rpc(&url, "prompts/get", json!({ "name": "test_plugin", "arguments": { "jar": "C:\\dev\\p.jar", "server": "Test Paper" } })).await;
        assert_eq!(status, 200);
        let message = &body["result"]["messages"][0];
        assert_eq!(message["role"], "user");
        assert_eq!(message["content"]["type"], "text");
        let task = message["content"]["text"].as_str().unwrap();
        assert!(task.contains("C:\\dev\\p.jar") && task.contains("Test Paper") && task.contains("install_local_addon"));
        assert!(body["result"]["description"].is_string());

        // What is wrong with a request is a JSON-RPC error that says so.
        let (_, body) = rpc(&url, "prompts/get", json!({ "name": "test_plugin" })).await;
        assert_eq!(body["error"]["code"], -32602);
        assert!(body["error"]["message"].as_str().unwrap().contains("`jar`"));
        let (_, body) = rpc(&url, "prompts/get", json!({ "name": "nope" })).await;
        assert_eq!(body["error"]["code"], -32602);

        // Stateless: the list may be kept, the prompt is complete.
        let (_, body) = rpc(&url, "prompts/list", modern_meta(MODERN_VERSION)).await;
        assert_eq!(body["result"]["cacheScope"], "public");
        let mut get = modern_meta(MODERN_VERSION);
        get["name"] = json!("diagnose_crash");
        let (_, body) = rpc(&url, "prompts/get", get).await;
        assert_eq!(body["result"]["resultType"], "complete");
        assert!(body["result"]["messages"][0]["content"]["text"].as_str().unwrap().contains("diagnose_server"));

        // View only: the prompt that changes things is neither listed nor given.
        let view_only = endpoint(true).await;
        let (_, body) = rpc(&view_only, "prompts/list", Value::Null).await;
        assert_eq!(body["result"]["prompts"].as_array().map(Vec::len), Some(1));
        let (_, body) = rpc(&view_only, "prompts/get", json!({ "name": "test_plugin", "arguments": { "jar": "C:\\p.jar" } })).await;
        assert_eq!(body["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn web_pages_and_other_verbs_are_refused_and_nothing_asks_for_a_token() {
        let url = endpoint(false).await;
        let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let port = url.trim_end_matches("/mcp").rsplit(':').next().unwrap().to_string();
        let status = |headers: Vec<(&'static str, String)>| {
            let url = url.clone();
            async move {
                let headers: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
                send("POST", &url, &headers, Some(ping)).await.0
            }
        };

        assert_eq!(send("GET", &url, &[], None).await.0, 405);
        assert_eq!(send("DELETE", &url, &[], None).await.0, 405);
        assert_eq!(status(vec![("Origin", "https://evil.example".into())]).await, 403);
        assert_eq!(status(vec![("Origin", "http://localhost:5173".into())]).await, 200);
        assert_eq!(status(vec![("Host", format!("evil.example:{port}"))]).await, 403);
        assert_eq!(status(vec![("Host", format!("localhost:{port}"))]).await, 200);
        assert_eq!(status(vec![("Content-Type", "text/plain".into())]).await, 415);
        // No Authorization header, and still served: never 401.
        assert_eq!(status(vec![]).await, 200);
    }

    #[tokio::test]
    async fn broken_and_malformed_json_get_a_json_rpc_error() {
        let url = endpoint(false).await;

        let (status, content_type, text) = send("POST", &url, &[], Some("{not json")).await;
        assert_eq!(status, 400);
        assert!(content_type.contains("application/json"));
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["error"]["code"], -32700);
        assert_eq!(body["id"], Value::Null);

        let (_, _, text) = send("POST", &url, &[], Some("42")).await;
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn a_batch_is_still_answered() {
        let url = endpoint(false).await;
        let batch = json!([
            { "jsonrpc": "2.0", "id": 1, "method": "ping" },
            { "jsonrpc": "2.0", "method": "notifications/initialized" },
            { "jsonrpc": "2.0", "id": 2, "method": "nope" },
        ]);
        let (status, _, text) = send("POST", &url, &[], Some(&batch.to_string())).await;
        assert_eq!(status, 200);
        let replies: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(replies.as_array().map(Vec::len), Some(2));
        assert_eq!(replies[0]["result"], json!({}));
        assert_eq!(replies[1]["error"]["code"], -32601);

        let (status, _, text) =
            send("POST", &url, &[], Some(r#"[{"jsonrpc":"2.0","method":"notifications/initialized"}]"#)).await;
        assert_eq!((status, text.as_str()), (202, ""));

        let (status, _, text) = send("POST", &url, &[], Some("[]")).await;
        assert_eq!(status, 400);
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn tool_results_carry_is_error_and_their_json_as_data() {
        let url = endpoint(false).await;
        let call = |name: &str| rpc(&url, "tools/call", json!({ "name": name, "arguments": {} }));

        // A list: the text is the list, the data wraps it in an object.
        let (status, body) = call("list_servers").await;
        let result = &body["result"];
        assert_eq!(status, 200);
        assert_eq!(result["isError"], false);
        assert_eq!(result["content"][0]["type"], "text");
        assert!(serde_json::from_str::<Value>(result["content"][0]["text"].as_str().unwrap()).unwrap().is_array());
        assert_eq!(result["structuredContent"]["items"][0]["name"], "Test Paper");

        // An object goes as it is.
        let (_, body) = call("get_machine_status").await;
        assert_eq!(body["result"]["structuredContent"], json!({ "cpu_percent": 3.5 }));

        // Plain text has no data, and a failure is a result, not a protocol error.
        let (_, body) = call("get_console").await;
        assert_eq!(body["result"]["isError"], false);
        assert!(body["result"].get("structuredContent").is_none());
        let (_, body) = call("start_server").await;
        assert_eq!(body["result"]["isError"], true);
        assert!(body["result"].get("structuredContent").is_none());
        assert!(body.get("error").is_none());

        // A tool that does not exist is a malformed request.
        let (status, body) = call("no_such_tool").await;
        assert_eq!(status, 200);
        assert_eq!(body["error"]["code"], -32602);
        assert!(body["error"]["message"].as_str().unwrap().contains("tools/list"));
    }

    #[tokio::test]
    async fn the_tool_list_is_stable_and_view_only_hides_what_changes_things() {
        let url = endpoint(false).await;
        let (_, first) = rpc(&url, "tools/list", Value::Null).await;
        let (_, again) = rpc(&url, "tools/list", Value::Null).await;
        let all = names(&first["result"]);
        assert!(!all.is_empty());
        assert_eq!(all, names(&again["result"]));
        assert!(first["result"].get("resultType").is_none() && first["result"].get("ttlMs").is_none());

        let (_, body) = rpc(&endpoint(true).await, "tools/list", Value::Null).await;
        let visible = names(&body["result"]);
        assert!(visible.len() < all.len());
        assert!(visible.contains(&"list_servers".to_string()));
        assert!(!visible.contains(&"delete_server".to_string()));
        assert!(body["result"]["tools"].as_array().unwrap().iter().all(|t| t["annotations"]["readOnlyHint"] == true));
    }

    #[tokio::test]
    async fn a_stateless_client_needs_no_handshake() {
        let url = endpoint(false).await;
        let headers = [("MCP-Protocol-Version", MODERN_VERSION), ("Mcp-Method", "server/discover")];

        let (status, body) = rpc_with(&url, &headers, "server/discover", modern_meta(MODERN_VERSION)).await;
        let result = &body["result"];
        assert_eq!(status, 200);
        let versions: Vec<&str> = result["supportedVersions"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(versions, supported_versions());
        assert_eq!(versions[0], MODERN_VERSION);
        assert_eq!(result["_meta"][META_SERVER]["name"], "nebula-host");
        assert!(result["capabilities"]["tools"].is_object());
        assert!(!result["instructions"].as_str().unwrap().is_empty());
        assert_eq!(result["ttlMs"], TTL_MS);
        assert_eq!(result["cacheScope"], "public");
        assert_eq!(result["resultType"], "complete");
        assert_eq!(activity::client().as_deref(), Some("mcp-check"));

        let (_, legacy) = rpc(&url, "tools/list", Value::Null).await;
        let (_, body) = rpc_with(&url, &headers, "tools/list", modern_meta(MODERN_VERSION)).await;
        let result = &body["result"];
        assert_eq!(names(result), names(&legacy["result"]));
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["ttlMs"], TTL_MS);
        assert_eq!(result["cacheScope"], "public");

        for method in ["resources/list", "prompts/list"] {
            let (_, body) = rpc(&url, method, modern_meta(MODERN_VERSION)).await;
            assert_eq!(body["result"]["resultType"], "complete", "{method}");
            assert_eq!(body["result"]["cacheScope"], "public", "{method}");
        }

        // Results that are not lists are complete, and not for keeping.
        let (_, body) = rpc(&url, "ping", modern_meta(MODERN_VERSION)).await;
        assert_eq!(body["result"], json!({ "resultType": "complete" }));
        let mut call = modern_meta(MODERN_VERSION);
        call["name"] = json!("get_machine_status");
        let (_, body) = rpc(&url, "tools/call", call).await;
        assert_eq!(body["result"]["resultType"], "complete");
        assert_eq!(body["result"]["isError"], false);
        assert!(body["result"]["structuredContent"].is_object());
        assert!(body["result"].get("ttlMs").is_none());
    }

    #[tokio::test]
    async fn a_version_this_server_does_not_speak_is_refused_with_the_ones_it_does() {
        let url = endpoint(false).await;
        let refused = |status: u16, body: &Value| {
            assert_eq!(status, 400);
            assert_eq!(body["error"]["code"], UNSUPPORTED_VERSION);
            assert_eq!(body["error"]["data"]["requested"], "2031-01-01");
            let supported = body["error"]["data"]["supported"].as_array().unwrap();
            assert!(supported.contains(&json!(MODERN_VERSION)) && supported.contains(&json!("2025-06-18")));
            assert_eq!(body["id"], 7);
        };

        // Named in `_meta` (with the header a stateless client also sends).
        let headers = [("MCP-Protocol-Version", "2031-01-01"), ("Mcp-Method", "tools/list")];
        let (status, body) = rpc_with(&url, &headers, "tools/list", modern_meta("2031-01-01")).await;
        refused(status, &body);
        // In `_meta` alone, and in the header alone.
        let (status, body) = rpc(&url, "tools/list", modern_meta("2031-01-01")).await;
        refused(status, &body);
        let (status, body) = rpc_with(&url, &headers[..1], "tools/list", Value::Null).await;
        refused(status, &body);

        // A legacy version in the header is what clients send after the handshake.
        let (status, body) = rpc_with(&url, &[("MCP-Protocol-Version", "2025-06-18")], "tools/list", Value::Null).await;
        assert_eq!(status, 200);
        assert!(body["result"]["tools"].is_array());
        assert!(body["result"].get("resultType").is_none());
    }
}
