//! What agents did here, most recent first: the app shows it, so the person
//! at the machine can see an agent at work instead of finding the results.
//!
//! Kept in memory only. It is a view of the present, not an audit log.

use std::collections::VecDeque;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

const KEEP: usize = 40;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub at: DateTime<Utc>,
    /// The tool, by its MCP name (`create_server`).
    pub tool: String,
    /// What it acted on: a server or instance, as the agent named it.
    pub target: Option<String>,
    /// The paired machine it was sent to, or the one that sent it here.
    pub machine: Option<String>,
    pub ok: bool,
}

static LOG: Mutex<VecDeque<Entry>> = Mutex::new(VecDeque::new());
static CLIENT: Mutex<Option<String>> = Mutex::new(None);

/// `from` names the paired device an agent's request came through, when it
/// did not come from this machine.
pub fn record(tool: &str, args: &Value, from: Option<&str>, ok: bool) {
    let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_string);
    let entry = Entry {
        at: Utc::now(),
        tool: tool.to_string(),
        target: text("server").or_else(|| text("instance")).or_else(|| text("name")),
        machine: from.map(str::to_string).or_else(|| text("machine")),
        ok,
    };
    if let Ok(mut log) = LOG.lock() {
        log.push_front(entry);
        log.truncate(KEEP);
    }
}

pub fn recent() -> Vec<Entry> {
    LOG.lock().map(|log| log.iter().cloned().collect()).unwrap_or_default()
}

pub fn set_client(name: &str) {
    if let Ok(mut client) = CLIENT.lock() {
        *client = Some(name.chars().take(60).collect());
    }
}

pub fn client() -> Option<String> {
    CLIENT.lock().ok().and_then(|client| client.clone())
}
