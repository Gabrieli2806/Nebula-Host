//! The MCP tools: what an agent can ask this node to do.
//!
//! Every tool goes through the same services the UI uses, so the rules (path
//! safety, one status owner, typed errors) hold for the agent exactly as
//! they do for a person. Results are short, readable text or JSON — the
//! agent reads them, a person never parses them.
//!
//! Every tool also takes `machine`: a paired machine to run it on instead of
//! this one. The call is handed to that machine whole (`call_routed`), which
//! runs it with its own rules and only if its owner allows agents from
//! paired machines.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;

use serde::Serialize;
use serde_json::{json, Value};

use super::diagnose;
use crate::service::mod_share::ModShareDirection;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::registry::InstallProgress;
use crate::game::BuildSelector;
use crate::model::{InstanceStatus, InstanceView};
use crate::store::peers::{self, Peer};
use crate::node::health;
use crate::service::addons::BrowseRequest;
use crate::service::{backups, files, players, properties, CreateInstanceRequest, InstanceService};

const INSTRUCTIONS: &str = "Nebula Host manages game servers (Minecraft today) and the Minecraft launcher on this PC, \
and on the machines paired with it. Use list_servers first to find a server; every server argument accepts the server id or its name. \
Every tool takes an optional `machine`: leave it out for this PC, or name a paired machine from list_machines to act there. \
\n\nCreating a server: create_server downloads files and takes a minute. Pass `start: true` to start it as soon as it is installed, \
then call wait_for_server, which returns when the server is online (or explains why it is not). Do not poll get_server in a loop. \
\n\nTesting a mod or plugin you just built: create (or reuse) a test server with the matching distribution and Minecraft version \
(paper/purpur/spigot for plugins; fabric, forge or neoforge for mods), then install_local_addon with the path of the built jar and \
`restart: true`; it stops the server if needed, replaces the jar and starts it again. Then wait_for_server, and read get_console \
(`only_problems: true`) or diagnose_server to check that it loaded. Use send_command to run the commands under test. \
A development game client (`gradlew runClient`) has no Minecraft account: for it to join, the test server needs \
`online-mode` = `false` (create_server `settings`, or set_server_settings and a restart). Reuse one test server per \
loader and version instead of creating a new one for every build, and delete_server only the ones you created. \
\n\nWhen a server will not start or has crashed, call diagnose_server: it returns the latest console errors, \
the newest crash report and known-problem hints, which you can use to explain the cause and fix it. \
\n\nThe launcher_* tools manage Minecraft instances the user plays on this device: create instances, search and install mods/resource packs/shaders \
from Modrinth, check updates and compatibility after a version change, change an instance's version, start or stop the game, and read its log \
or newest crash report. launcher_install_local_file copies a freshly built jar into a test instance; \
launcher_set_content_enabled helps bisect which mod breaks the game.";

/// What an agent is told when it connects: the fixed guide, plus what is true
/// on this machine now (whether the game's agreement still needs a person).
pub fn instructions() -> String {
    let mut text = INSTRUCTIONS.to_string();
    text.push_str("\n\n");
    text.push_str(agreement_note());
    text
}

fn agreements_accepted() -> bool {
    crate::store::settings::load().map(|s| s.accept_agreements).unwrap_or(false)
}

fn agreement_note() -> &'static str {
    if agreements_accepted() {
        "The owner of this PC has accepted the Minecraft EULA for every server here: new servers start without asking. Do not ask about the EULA."
    } else {
        "A new server here cannot start until the owner of this PC accepts the Minecraft EULA in the Nebula Host app \
(once for every server: Settings > Servers). If start_server reports `agreement_required`, tell them that; you cannot accept it for them."
    }
}

/// What a tool call came to: the text the agent reads, and whether it failed.
pub struct Outcome {
    pub text: String,
    pub is_error: bool,
}

impl Outcome {
    fn from(result: AppResult<String>) -> Self {
        match result {
            Ok(text) => Self { text, is_error: false },
            Err(error) => Self { text: describe_error(&error), is_error: true },
        }
    }

    fn failed(text: impl Into<String>) -> Self {
        Self { text: text.into(), is_error: true }
    }
}

struct Tool {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    schema: Value,
    mutates: bool,
}

fn server_arg() -> Value {
    json!({ "type": "string", "description": "Server id or name (case-insensitive)." })
}

fn catalog() -> Vec<Tool> {
    vec![
        Tool {
            name: "list_servers",
            title: "List servers",
            description: "Every server on this PC with its status, game, version, players online and address.",
            schema: json!({ "type": "object", "properties": {} }),
            mutates: false,
        },
        Tool {
            name: "get_server",
            title: "Server details",
            description: "Full details of one server: build, memory, ports, automation, live stats.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "get_console",
            title: "Read console",
            description: "The most recent console lines of a server (newest last). Optionally only warnings/errors.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "lines": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 150 },
                "only_problems": { "type": "boolean", "default": false, "description": "Only WARN/ERROR lines." }
            }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "diagnose_server",
            title: "Diagnose problems",
            description: "Collects what is needed to explain why a server crashed or will not start: status, recent errors, the newest crash report, the end of logs/latest.log, Java and memory, plus hints for known problems (wrong Java, port in use, out of memory, EULA, missing mod/plugin dependencies, incompatible mods).",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "get_machine_status",
            title: "Machine status",
            description: "CPU, memory, disk and LAN address of this PC, and how many servers run.",
            schema: json!({ "type": "object", "properties": {} }),
            mutates: false,
        },
        Tool {
            name: "list_players",
            title: "Players",
            description: "Players online now plus the allowed (whitelist), operators and banned lists.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "get_server_settings",
            title: "Game settings",
            description: "The game's own settings (server.properties for Minecraft) with current values.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "list_files",
            title: "List files",
            description: "Files and folders inside the server folder.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "path": { "type": "string", "description": "Folder relative to the server root; empty for the root." }
            }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "read_file",
            title: "Read file",
            description: "A text file inside the server folder (configs, logs). Large files are truncated to the last 200 KB.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "path": { "type": "string" }
            }, "required": ["server", "path"] }),
            mutates: false,
        },
        Tool {
            name: "list_backups",
            title: "Backups",
            description: "Backups of a server, newest first.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "list_versions",
            title: "Game versions",
            description: "Versions available for a distribution (vanilla, paper, purpur, spigot, fabric, forge, neoforge).",
            schema: json!({ "type": "object", "properties": {
                "distribution": { "type": "string" },
                "include_unstable": { "type": "boolean", "default": false }
            }, "required": ["distribution"] }),
            mutates: false,
        },
        Tool {
            name: "list_addons",
            title: "Installed mods/plugins",
            description: "Mods or plugins installed on a server, identified on Modrinth, with available updates.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "search_addons",
            title: "Search Modrinth",
            description: "Search Modrinth for mods/plugins compatible with the server's loader and game version.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "query": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 }
            }, "required": ["server", "query"] }),
            mutates: false,
        },
        // -- changes ------------------------------------------------------
        Tool {
            name: "start_server",
            title: "Start server",
            description: "Start a server (also wakes a sleeping one).",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: true,
        },
        Tool {
            name: "stop_server",
            title: "Stop server",
            description: "Stop a running server cleanly (saves the world).",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: true,
        },
        Tool {
            name: "restart_server",
            title: "Restart server",
            description: "Stop and start a server.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: true,
        },
        Tool {
            name: "send_command",
            title: "Run console command",
            description: "Send a console command to a running server (e.g. `say hi`, `op Steve`, `time set day`). Returns the console lines printed right after.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "command": { "type": "string", "description": "Without a leading slash." }
            }, "required": ["server", "command"] }),
            mutates: true,
        },
        Tool {
            name: "create_server",
            title: "Create server",
            description: "Create a new server and install it in the background; it shows in the Nebula Host app as getting ready. Use list_versions to pick a version. Pass `start: true` to start it once installed, then call wait_for_server.",
            schema: json!({ "type": "object", "properties": {
                "name": { "type": "string" },
                "distribution": { "type": "string", "description": "e.g. vanilla, paper, purpur, spigot, fabric, forge, neoforge. Omit for the default (Paper)." },
                "version": { "type": "string", "description": "Game version; omit for the newest stable." },
                "memory_mb": { "type": "integer", "minimum": 512, "description": "RAM for the server in MB." },
                "port": { "type": "integer", "minimum": 1024, "maximum": 65535 },
                "settings": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Game settings to apply once installed (keys as in get_server_settings), e.g. {\"online-mode\":\"false\"} for a server a development client joins." },
                "start": { "type": "boolean", "default": false, "description": "Start the server as soon as it is installed." }
            }, "required": ["name"] }),
            mutates: true,
        },
        Tool {
            name: "wait_for_server",
            title: "Wait for a server",
            description: "Wait until a server is `online` (default), `installed` or `offline`, instead of polling. Returns as soon as it is, with its status; if it crashes on the way it returns the diagnosis. Call it after create_server, start_server, restart_server or install_local_addon.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "until": { "type": "string", "enum": ["online", "installed", "offline"], "default": "online" },
                "timeout_seconds": { "type": "integer", "minimum": 5, "maximum": 600, "default": 180 }
            }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "list_machines",
            title: "Machines",
            description: "This PC and the machines paired with it, and whether each answers and allows agents. Pass a machine's name as `machine` to any tool to act there.",
            schema: json!({ "type": "object", "properties": {} }),
            mutates: false,
        },
        Tool {
            name: "install_local_addon",
            title: "Install a built mod/plugin jar",
            description: "Copy a .jar from this PC into a server's mods or plugins folder: the build output of the mod or plugin being developed (e.g. build/libs/myplugin-0.1.0.jar). A jar with the same file name is replaced. A running server holds its jars open: pass `restart: true` to stop it, replace the jar and start it again in one step.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "path": { "type": "string", "description": "Absolute path of the .jar on this PC." },
                "replace": { "type": "string", "description": "File name of an older build to remove first, when the version in the file name changed (e.g. myplugin-0.1.0.jar)." },
                "restart": { "type": "boolean", "default": false, "description": "If the server is running, stop it, install, and start it again." },
                "start": { "type": "boolean", "default": false, "description": "Start the server afterwards even if it was stopped." }
            }, "required": ["server", "path"] }),
            mutates: true,
        },
        Tool {
            name: "remove_addon",
            title: "Remove mod/plugin",
            description: "Delete an installed mod or plugin jar by file name (see list_addons). On a running server it is removed when the server stops.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "file": { "type": "string" }
            }, "required": ["server", "file"] }),
            mutates: true,
        },
        Tool {
            name: "set_addon_enabled",
            title: "Turn mod/plugin on/off",
            description: "Switch an installed mod or plugin on or off without deleting it (useful to find which one breaks the server). On a running server it applies when the server stops.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "file": { "type": "string" },
                "enabled": { "type": "boolean" }
            }, "required": ["server", "file", "enabled"] }),
            mutates: true,
        },
        Tool {
            name: "write_file",
            title: "Write file",
            description: "Create or replace a text file inside the server folder (a plugin's config.yml, a datapack file, bukkit.yml). Missing folders are created.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "path": { "type": "string", "description": "Relative to the server root." },
                "content": { "type": "string" }
            }, "required": ["server", "path", "content"] }),
            mutates: true,
        },
        Tool {
            name: "delete_file",
            title: "Delete file or folder",
            description: "Delete a file or a whole folder inside the server folder (e.g. a plugin's data folder, to test a first run again).",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "path": { "type": "string", "description": "Relative to the server root." }
            }, "required": ["server", "path"] }),
            mutates: true,
        },
        Tool {
            name: "delete_server",
            title: "Delete server",
            description: "Delete a stopped server and, unless `keep_files` is true, its folder with the world in it. This cannot be undone: only delete a server you created for a test, or one the user asked you to delete.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "keep_files": { "type": "boolean", "default": false }
            }, "required": ["server"] }),
            mutates: true,
        },
        Tool {
            name: "set_memory",
            title: "Set memory",
            description: "Change how much RAM a server gets (takes effect on next start).",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "memory_mb": { "type": "integer", "minimum": 512 }
            }, "required": ["server", "memory_mb"] }),
            mutates: true,
        },
        Tool {
            name: "set_server_settings",
            title: "Change game settings",
            description: "Change game settings by key (see get_server_settings), e.g. {\"max-players\":\"10\",\"pvp\":\"false\"}. Most take effect after a restart.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "values": { "type": "object", "additionalProperties": { "type": "string" } }
            }, "required": ["server", "values"] }),
            mutates: true,
        },
        Tool {
            name: "create_backup",
            title: "Back up",
            description: "Make a backup of a server's files now.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "label": { "type": "string" }
            }, "required": ["server"] }),
            mutates: true,
        },
        Tool {
            name: "install_addon",
            title: "Install mod/plugin",
            description: "Install the best compatible version of a Modrinth project (id or slug from search_addons). Required dependencies are reported, not installed automatically.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "project_id": { "type": "string" }
            }, "required": ["server", "project_id"] }),
            mutates: true,
        },
        Tool {
            name: "share_mods",
            title: "Copy mods between server and instance",
            description: "Copy the mods between a server and a launcher instance of the same Minecraft version and loader. Mods that cannot run on the receiving side stay behind and are reported; a jar of the same project there is replaced. Copying to a server requires it to be stopped.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "instance": { "type": "string", "description": "Launcher instance id or name." },
                "direction": { "type": "string", "enum": ["to_instance", "to_server"] }
            }, "required": ["server", "instance", "direction"] }),
            mutates: true,
        },
    ]
}

/// Tools that delete something the user may want back.
const DESTRUCTIVE: &[&str] = &["launcher_remove_content", "delete_server", "delete_file", "remove_addon"];

pub fn definitions(read_only: bool) -> Vec<Value> {
    catalog()
        .into_iter()
        .chain(launcher_catalog())
        .filter(|tool| !(read_only && tool.mutates))
        .map(|tool| {
            let mut schema = tool.schema;
            // Every tool but the one that lists the machines can run on one.
            if tool.name != "list_machines" {
                if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
                    properties.insert(
                        "machine".into(),
                        json!({ "type": "string", "description": "A paired machine to act on, by name (see list_machines). Omit for this PC." }),
                    );
                }
            }
            json!({
                "name": tool.name,
                "title": tool.title,
                "description": tool.description,
                "inputSchema": schema,
                "annotations": {
                    "readOnlyHint": !tool.mutates,
                    "destructiveHint": DESTRUCTIVE.contains(&tool.name),
                    "openWorldHint": tool.name.contains("addon") || tool.name.contains("content") || tool.name.contains("version") || tool.name == "create_server" || tool.name == "launcher_install_modpack",
                }
            })
        })
        .collect()
}

pub fn describe_error(error: &AppError) -> String {
    let mut text = format!("Error: {}", error.code.as_str());
    if !error.params.is_empty() {
        let params: Vec<String> = error.params.iter().map(|(k, v)| format!("{k}={v}")).collect();
        text.push_str(&format!(" ({})", params.join(", ")));
    }
    if let Some(detail) = &error.detail {
        text.push_str(&format!("\n{detail}"));
    }
    text
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput).detail(message)
}

fn str_arg<'a>(args: &'a Value, key: &str) -> AppResult<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(&format!("missing argument `{key}`")))
}

fn pretty<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into())
}

/// Find a server by id, exact name, or unique name prefix.
async fn resolve(service: &InstanceService, args: &Value) -> AppResult<InstanceView> {
    let wanted = str_arg(args, "server")?;
    let all = service.list().await?;
    let lower = wanted.to_lowercase();
    if let Some(found) = all.iter().find(|v| v.instance.id == wanted || v.instance.name.to_lowercase() == lower) {
        return Ok(found.clone());
    }
    let matches: Vec<&InstanceView> =
        all.iter().filter(|v| v.instance.name.to_lowercase().starts_with(&lower)).collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(AppError::new(ErrorCode::InstanceNotFound).with("instance", wanted)),
        many => Err(invalid(&format!(
            "`{wanted}` matches several servers: {}",
            many.iter().map(|v| v.instance.name.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

fn status_word(status: InstanceStatus) -> String {
    serde_json::to_value(status).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

fn summary(view: &InstanceView) -> Value {
    let i = &view.instance;
    json!({
        "id": i.id,
        "name": i.name,
        "status": status_word(view.state.status),
        "game": i.game,
        "distribution": i.build.distribution,
        "version": i.build.version,
        "loader_version": i.build.loader_version,
        "players_online": view.state.players_online,
        "players_max": view.state.players_max,
        "memory_allocated_mb": i.resources.memory_mb,
        "memory_used_mb": view.state.memory_used_mb,
        "cpu_percent": (view.state.cpu_percent * 10.0).round() / 10.0,
        "tick_rate": view.state.tick_rate,
        "uptime_seconds": view.state.uptime_seconds,
        "address": view.address,
        "installed": i.install.is_some(),
    })
}

/// The launcher tools in this module's shape.
fn launcher_catalog() -> impl Iterator<Item = Tool> {
    super::launcher_tools::catalog().into_iter().map(|t| Tool {
        name: t.name,
        title: t.title,
        description: t.description,
        schema: t.schema,
        mutates: t.mutates,
    })
}

/// The words an agent may use for the machine it is connected to.
async fn is_this_machine(service: &InstanceService, wanted: &str) -> bool {
    let lower = wanted.trim().to_lowercase();
    if matches!(lower.as_str(), "" | "local" | "this" | "this pc" | "this machine" | "localhost") {
        return true;
    }
    let node = service.node().await;
    node.id == wanted.trim() || node.name.to_lowercase() == lower
}

/// A tool call, wherever it is meant to run: here, or on the paired machine
/// the agent named in `machine`.
pub async fn call_routed(service: &Arc<InstanceService>, read_only: bool, name: &str, args: &Value) -> Outcome {
    let machine = args.get("machine").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty());
    let Some(machine) = machine else {
        return Outcome::from(call(service, read_only, name, args).await);
    };
    if is_this_machine(service, machine).await {
        return Outcome::from(call(service, read_only, name, args).await);
    }

    // This machine's own rule holds for what leaves through it, too.
    let Some(tool) = catalog().into_iter().chain(launcher_catalog()).find(|t| t.name == name) else {
        return Outcome::from(Err(invalid(&format!("unknown tool `{name}`"))));
    };
    if read_only && tool.mutates {
        return Outcome::from(Err(read_only_error()));
    }

    let known = peers::load().unwrap_or_default();
    let peer = match peers::find(&known, machine) {
        Ok(peer) => peer.clone(),
        Err(candidates) if candidates.is_empty() => {
            let names: Vec<&str> = known.iter().map(|p| p.name.as_str()).collect();
            return Outcome::failed(if names.is_empty() {
                format!("No machine is paired with this PC, so `{machine}` cannot be reached. Leave `machine` out to act on this PC.")
            } else {
                format!("No paired machine is called `{machine}`. Paired machines: {}.", names.join(", "))
            });
        }
        Err(candidates) => {
            let names: Vec<&str> = candidates.iter().map(|p| p.name.as_str()).collect();
            return Outcome::failed(format!("`{machine}` matches several machines: {}.", names.join(", ")));
        }
    };

    let mut forwarded = args.clone();
    if let Some(object) = forwarded.as_object_mut() {
        object.remove("machine");
    }
    // A file on this PC travels with the call: the other machine cannot read
    // this one's disk.
    if matches!(name, "install_local_addon" | "launcher_install_local_file") {
        if let Err(error) = attach_file(&mut forwarded).await {
            return Outcome::from(Err(error));
        }
    }
    remote_call(&peer, name, forwarded).await
}

/// Replace `path` (a file on this PC) with the file itself.
async fn attach_file(args: &mut Value) -> AppResult<()> {
    let (name, data) = local_file(args).await?;
    if let Some(object) = args.as_object_mut() {
        object.remove("path");
        object.insert("file_name".into(), json!(name));
        object.insert("content_base64".into(), json!(base64::engine::general_purpose::STANDARD.encode(data)));
    }
    Ok(())
}

/// The file a tool was given: by `path` on this machine, or carried in the
/// call (`file_name` + `content_base64`) when it came from another one.
pub(super) async fn local_file(args: &Value) -> AppResult<(String, Vec<u8>)> {
    if let Some(encoded) = args.get("content_base64").and_then(Value::as_str) {
        let name = str_arg(args, "file_name")?.to_string();
        let data = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| invalid("`content_base64` is not valid base64"))?;
        return Ok((name, data));
    }
    let path = std::path::Path::new(str_arg(args, "path")?);
    if !path.is_absolute() {
        return Err(invalid("`path` must be absolute"));
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| invalid("`path` has no file name"))?
        .to_string();
    let data = tokio::fs::read(path)
        .await
        .map_err(|e| AppError::new(ErrorCode::FileNotFound).with("file", path.display()).detail(e.to_string()))?;
    Ok((name, data))
}

/// Hand a tool call to a paired machine and return what it answered.
async fn remote_call(peer: &Peer, name: &str, args: Value) -> Outcome {
    // wait_for_server may hold the call for as long as it was asked to.
    let timeout = Duration::from_secs(if name == "wait_for_server" { 660 } else { 180 });
    let response = crate::game::http_client()
        .post(format!("http://{}/agent", peer.address))
        .bearer_auth(&peer.token)
        .timeout(timeout)
        .json(&json!({ "tool": name, "arguments": args }))
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return Outcome::failed(format!(
                "{} ({}) is not answering: it may be off, asleep or on another network. ({error})",
                peer.name, peer.address
            ));
        }
    };
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        return Outcome {
            text: body.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
            is_error: body.get("isError").and_then(Value::as_bool).unwrap_or(false),
        };
    }
    let reason = body.pointer("/params/reason").and_then(Value::as_str).unwrap_or("");
    Outcome::failed(match (status.as_u16(), reason) {
        (_, "agents_not_allowed") => format!(
            "{} does not allow agents from paired machines. Its owner turns that on there: Settings > AI agents > Allow agents from paired machines.",
            peer.name
        ),
        (401, _) => format!("{} no longer accepts this PC: pair them again in the Nebula Host app.", peer.name),
        (404 | 405, _) => format!("{} runs an older Nebula Host without agent access. Update it there.", peer.name),
        _ => format!("{} answered with an error ({status}): {body}", peer.name),
    })
}

fn read_only_error() -> AppError {
    AppError::new(ErrorCode::InvalidInput)
        .detail("Agents may only look at this machine, not change it. Its owner allows changes in Settings > AI agents.")
}

pub async fn call(service: &Arc<InstanceService>, read_only: bool, name: &str, args: &Value) -> AppResult<String> {
    let Some(tool) = catalog().into_iter().chain(launcher_catalog()).find(|t| t.name == name) else {
        return Err(invalid(&format!("unknown tool `{name}`")));
    };
    if read_only && tool.mutates {
        return Err(read_only_error());
    }

    if name.starts_with("launcher_") {
        return super::launcher_tools::call(name, args).await;
    }

    match name {
        "list_servers" => {
            let all = service.list().await?;
            if all.is_empty() {
                return Ok("No servers yet. Use create_server to make one.".into());
            }
            Ok(pretty(&all.iter().map(summary).collect::<Vec<_>>()))
        }
        "get_server" => {
            let view = resolve(service, args).await?;
            let i = &view.instance;
            let mut value = summary(&view);
            value["ports"] = json!(i.ports);
            value["path"] = json!(i.path);
            value["automation"] = json!(i.automation);
            value["java_major"] = json!(i.install.as_ref().and_then(|r| r.java_major));
            value["created_at"] = json!(i.created_at);
            value["last_started_at"] = json!(i.last_started_at);
            Ok(pretty(&value))
        }
        "get_console" => {
            let view = resolve(service, args).await?;
            let lines = args.get("lines").and_then(Value::as_u64).unwrap_or(150).clamp(1, 1000) as usize;
            let only_problems = args.get("only_problems").and_then(Value::as_bool).unwrap_or(false);
            let tail = service.console_tail(&view.instance.id, if only_problems { 1000 } else { lines }).await;
            let mut out: Vec<String> = tail
                .iter()
                .filter(|l| !only_problems || diagnose::is_problem(l))
                .map(diagnose::format_line)
                .collect();
            if out.len() > lines {
                out.drain(..out.len() - lines);
            }
            if out.is_empty() {
                return Ok("The console is empty (the server has not run since the app started).".into());
            }
            Ok(out.join("\n"))
        }
        "diagnose_server" => {
            let view = resolve(service, args).await?;
            diagnose::report(service, &view).await
        }
        "get_machine_status" => {
            let running = service.processes.running_pids().await.len() as u32;
            Ok(pretty(&health::sample(running)))
        }
        "list_players" => {
            let view = resolve(service, args).await?;
            let manifest = service.registry.manifest(&view.instance.game)?;
            let lists = players::read_lists(&view.instance.path, &manifest.players)?;
            Ok(pretty(&json!({
                "online": service.online_players(&view.instance.id).await,
                "lists": lists,
            })))
        }
        "get_server_settings" => {
            let view = resolve(service, args).await?;
            let manifest = service.registry.manifest(&view.instance.game)?;
            Ok(pretty(&properties::read(&view.instance.path, &manifest)?))
        }
        "list_files" => {
            let view = resolve(service, args).await?;
            let path = args.get("path").and_then(Value::as_str).unwrap_or("");
            let entries = files::list(&view.instance.path, path)?;
            Ok(pretty(&entries))
        }
        "read_file" => {
            let view = resolve(service, args).await?;
            let path = str_arg(args, "path")?;
            let text = files::read_text(&view.instance.path, path)?;
            Ok(diagnose::tail_bytes(&text, 200 * 1024))
        }
        "list_backups" => {
            let view = resolve(service, args).await?;
            Ok(pretty(&backups::list(&view.instance.id)?))
        }
        "list_versions" => {
            let distribution = str_arg(args, "distribution")?.to_lowercase();
            let unstable = args.get("include_unstable").and_then(Value::as_bool).unwrap_or(false);
            let game = match args.get("game").and_then(Value::as_str) {
                Some(g) => g.to_string(),
                None => service.registry.manifests().first().map(|m| m.id.clone()).unwrap_or_default(),
            };
            let versions = service.registry.get(&game)?.versions(&distribution).await?;
            let ids: Vec<&str> = versions
                .iter()
                .filter(|v| unstable || v.stable)
                .take(60)
                .map(|v| v.id.as_str())
                .collect();
            Ok(format!("Newest first ({distribution}): {}", ids.join(", ")))
        }
        "list_addons" => {
            let view = resolve(service, args).await?;
            Ok(pretty(&service.addons_list(&view.instance.id).await?))
        }
        "search_addons" => {
            let view = resolve(service, args).await?;
            let query = str_arg(args, "query")?.to_string();
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10).clamp(1, 50) as u32;
            let request = BrowseRequest { query, limit: Some(limit), ..Default::default() };
            let result = service.addons_browse(&view.instance.id, request).await?;
            let hits: Vec<Value> = result
                .hits
                .iter()
                .map(|h| json!({ "project_id": h.project_id, "slug": h.slug, "title": h.title, "description": h.description, "downloads": h.downloads, "author": h.author }))
                .collect();
            Ok(pretty(&json!({ "total": result.total_hits, "hits": hits })))
        }
        "start_server" => {
            let view = resolve(service, args).await?;
            service.start(&view.instance.id).await?;
            Ok(format!("Starting {}. Use get_server to watch it reach `online`.", view.instance.name))
        }
        "stop_server" => {
            let view = resolve(service, args).await?;
            service.stop(&view.instance.id).await?;
            Ok(format!("Stopping {}.", view.instance.name))
        }
        "restart_server" => {
            let view = resolve(service, args).await?;
            let id = view.instance.id.clone();
            let svc = service.clone();
            tokio::spawn(async move {
                let _ = svc.restart(&id).await;
            });
            Ok(format!("Restarting {}.", view.instance.name))
        }
        "send_command" => {
            let view = resolve(service, args).await?;
            let command = str_arg(args, "command")?.trim_start_matches('/').to_string();
            let before = service.console_tail(&view.instance.id, 1000).await.len();
            service.send_command(&view.instance.id, &command).await?;
            tokio::time::sleep(std::time::Duration::from_millis(900)).await;
            let after = service.console_tail(&view.instance.id, 1000).await;
            let new: Vec<String> = after.iter().skip(before.min(after.len())).map(diagnose::format_line).collect();
            Ok(if new.is_empty() {
                format!("Sent `{command}`. No output yet.")
            } else {
                format!("Sent `{command}`. Output:\n{}", new.join("\n"))
            })
        }
        "create_server" => create_server(service, args).await,
        "wait_for_server" => wait_for_server(service, args).await,
        "list_machines" => list_machines(service).await,
        "install_local_addon" => install_local_addon(service, args).await,
        "remove_addon" => {
            let view = resolve(service, args).await?;
            let file = str_arg(args, "file")?;
            service.addon_delete(&view.instance.id, file).await?;
            Ok(if view.state.status.expects_process() {
                format!("`{file}` will be removed from {} when it stops.", view.instance.name)
            } else {
                format!("Removed `{file}` from {}.", view.instance.name)
            })
        }
        "set_addon_enabled" => {
            let view = resolve(service, args).await?;
            let file = str_arg(args, "file")?;
            let enabled = args.get("enabled").and_then(Value::as_bool).ok_or_else(|| invalid("missing `enabled`"))?;
            let name = service.addon_toggle(&view.instance.id, file, enabled).await?;
            let word = if enabled { "on" } else { "off" };
            Ok(if view.state.status.expects_process() {
                format!("`{file}` will be switched {word} when {} stops.", view.instance.name)
            } else {
                format!("Switched `{name}` {word} on {}.", view.instance.name)
            })
        }
        "write_file" => {
            let view = resolve(service, args).await?;
            service.ensure_not_transferring(&view.instance.id).await?;
            let path = str_arg(args, "path")?;
            let content = args.get("content").and_then(Value::as_str).ok_or_else(|| invalid("missing `content`"))?;
            files::write_text(&view.instance.path, path, content)?;
            service.after_folder_change(&view.instance.id).await;
            Ok(format!("Wrote `{path}` ({} bytes) on {}.", content.len(), view.instance.name))
        }
        "delete_file" => {
            let view = resolve(service, args).await?;
            service.ensure_not_transferring(&view.instance.id).await?;
            let path = str_arg(args, "path")?;
            files::delete(&view.instance.path, path)?;
            service.after_folder_change(&view.instance.id).await;
            Ok(format!("Deleted `{path}` on {}.", view.instance.name))
        }
        "delete_server" => {
            let view = resolve(service, args).await?;
            let keep_files = args.get("keep_files").and_then(Value::as_bool).unwrap_or(false);
            if view.state.status.expects_process() {
                return Err(invalid("the server is running: stop_server and wait_for_server (until offline) first"));
            }
            if matches!(&view.setup, Some(progress) if !matches!(progress, InstallProgress::Failed { .. })) {
                return Err(invalid("the server is still installing: wait_for_server (until installed) first"));
            }
            service.delete(&view.instance.id, !keep_files).await?;
            Ok(if keep_files {
                format!("Removed {} from Nebula Host. Its folder was kept: {}", view.instance.name, view.instance.path.display())
            } else {
                format!("Deleted {} and its files.", view.instance.name)
            })
        }
        "set_memory" => {
            let view = resolve(service, args).await?;
            let memory = args.get("memory_mb").and_then(Value::as_u64).ok_or_else(|| invalid("missing `memory_mb`"))?;
            let memory = u32::try_from(memory).map_err(|_| invalid("memory_mb too large"))?.max(512);
            service.store.update(&view.instance.id, |i| i.resources.memory_mb = memory).await?;
            Ok(format!("{} will use {memory} MB from its next start.", view.instance.name))
        }
        "set_server_settings" => {
            let view = resolve(service, args).await?;
            let values = args.get("values").and_then(Value::as_object).ok_or_else(|| invalid("missing `values` object"))?;
            let edits: BTreeMap<String, String> = values
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().map(String::from).unwrap_or_else(|| v.to_string())))
                .collect();
            let manifest = service.registry.manifest(&view.instance.game)?;
            properties::write(&view.instance.path, &manifest, &edits)?;
            Ok(format!("Saved {} setting(s) on {}. Restart it for most to apply.", edits.len(), view.instance.name))
        }
        "create_backup" => {
            let view = resolve(service, args).await?;
            let label = args.get("label").and_then(Value::as_str).map(String::from);
            let backup = backups::create(service, &view.instance.id, label).await?;
            Ok(pretty(&backup))
        }
        "install_addon" => {
            let view = resolve(service, args).await?;
            let project = str_arg(args, "project_id")?;
            let result = service.addon_install_version(&view.instance.id, project, None, None).await?;
            let mut text = format!("Installed {} {} as {}.", project, result.version_number, result.file_name);
            if let Ok(deps) = service.addon_dependencies(&view.instance.id, &result.version_id).await {
                let missing: Vec<String> = deps
                    .iter()
                    .filter(|d| d.dependency_type == "required" && !d.installed)
                    .map(|d| d.project.as_ref().map(|p| p.title.clone()).unwrap_or_else(|| d.project_id.clone()))
                    .collect();
                if !missing.is_empty() {
                    text.push_str(&format!(" Required dependencies not installed yet: {}.", missing.join(", ")));
                }
            }
            text.push_str(" Restart the server to load it.");
            Ok(text)
        }
        "share_mods" => {
            let view = resolve(service, args).await?;
            let profile = super::launcher_tools::resolve(args).await?;
            let direction = match str_arg(args, "direction")? {
                "to_instance" => ModShareDirection::ToInstance,
                "to_server" => ModShareDirection::ToServer,
                other => return Err(invalid(&format!("unknown direction `{other}`"))),
            };
            let report = service.share_mods(&view.instance.id, &profile, direction).await?;
            Ok(pretty(&report))
        }
        _ => Err(invalid(&format!("unknown tool `{name}`"))),
    }
}

async fn create_server(service: &Arc<InstanceService>, args: &Value) -> AppResult<String> {
    let name = str_arg(args, "name")?.to_string();
    // The first registered game (the manifest decides; nothing here knows game ids).
    let game = match args.get("game").and_then(Value::as_str) {
        Some(g) => g.to_string(),
        None => service
            .registry
            .manifests()
            .first()
            .map(|m| m.id.clone())
            .ok_or_else(|| invalid("no games are registered"))?,
    };
    let adapter = service.registry.get(&game)?;
    let manifest = service.registry.manifest(&game)?;
    let distribution = match args.get("distribution").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        Some(d) => d.to_lowercase(),
        // Default: the first distribution that takes addons, else the first.
        None => manifest
            .distributions
            .iter()
            .find(|d| d.addon_dir.is_some())
            .or_else(|| manifest.distributions.first())
            .map(|d| d.id.clone())
            .ok_or_else(|| invalid("this game has no distributions"))?,
    };
    let dist = manifest
        .distributions
        .iter()
        .find(|d| d.id == distribution)
        .ok_or_else(|| invalid(&format!("unknown distribution `{distribution}`")))?;

    let version = match args.get("version").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        Some(v) => v.to_string(),
        None => adapter
            .versions(&distribution)
            .await?
            .into_iter()
            .find(|v| v.stable)
            .map(|v| v.id)
            .ok_or_else(|| AppError::new(ErrorCode::VersionNotFound).with("distribution", &distribution))?,
    };
    let loader_version = if dist.has_loader_version {
        adapter
            .loader_versions(&distribution, &version)
            .await?
            .into_iter()
            .find(|v| v.stable)
            .map(|v| v.id)
    } else {
        None
    };

    let request = CreateInstanceRequest {
        game,
        name: name.clone(),
        build: BuildSelector { distribution: distribution.clone(), version: version.clone(), loader_version },
        memory_mb: args.get("memory_mb").and_then(Value::as_u64).and_then(|m| u32::try_from(m).ok()),
        port: args.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()),
        runtime_kind: None,
        directory: None,
        // An agent can never accept the Minecraft EULA for the user: the
        // server waits until they accept it in the app, unless they already
        // did for every server in Settings (then starting it applies that).
        accept_agreement: false,
    };
    let settings: BTreeMap<String, String> = args
        .get("settings")
        .and_then(Value::as_object)
        .map(|values| {
            values
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().map(String::from).unwrap_or_else(|| v.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let start = args.get("start").and_then(Value::as_bool).unwrap_or(false);

    let instance = service.create(request).await?;
    let id = instance.id.clone();
    let svc = service.clone();
    tokio::spawn(async move {
        if let Err(error) = svc.install(&id).await {
            tracing::warn!(instance_id = %id, %error, "agent-created server failed to install");
            return;
        }
        if !settings.is_empty() {
            let applied = async {
                let instance = svc.store.get(&id).await?;
                let manifest = svc.registry.manifest(&instance.game)?;
                properties::write(&instance.path, &manifest, &settings)
            };
            if let Err(error) = applied.await {
                tracing::warn!(instance_id = %id, %error, "agent-created server: settings not applied");
            }
        }
        if start {
            if let Err(error) = svc.start(&id).await {
                tracing::warn!(instance_id = %id, %error, "agent-created server did not start");
            }
        }
    });

    let mut text = format!(
        "Created \"{name}\" ({distribution} {version}), id {}. It is installing in the background and shows in the app as getting ready. ",
        instance.id
    );
    let needs_person = manifest.agreement.is_some() && !agreements_accepted();
    text.push_str(match (start, needs_person) {
        (true, false) => "It starts by itself once installed: call wait_for_server to know when it is online.",
        (false, false) => "Call wait_for_server with until=installed, then start_server.",
        (true, true) => "It cannot start yet: the owner of this PC has to accept the Minecraft EULA in the Nebula Host app first (on the server's page, or once for every server in Settings > Servers). Tell them, then start_server.",
        (false, true) => "Before it can start, the owner of this PC has to accept the Minecraft EULA in the Nebula Host app (on the server's page, or once for every server in Settings > Servers). Tell them.",
    });
    Ok(text)
}

/// Hold the call until the server is where the agent wants it, so it does
/// not poll. Returns early, saying why, when it cannot get there.
async fn wait_for_server(service: &Arc<InstanceService>, args: &Value) -> AppResult<String> {
    let found = resolve(service, args).await?;
    let id = found.instance.id.clone();
    let name = found.instance.name.clone();
    let until = args.get("until").and_then(Value::as_str).unwrap_or("online");
    if !matches!(until, "online" | "installed" | "offline") {
        return Err(invalid("`until` is online, installed or offline"));
    }
    let timeout = args.get("timeout_seconds").and_then(Value::as_u64).unwrap_or(180).clamp(5, 600);
    let started = Instant::now();
    let mut moved = false;

    loop {
        let view = service.view(&id).await?;
        let status = view.state.status;
        let installed = view.instance.install.is_some();
        if let Some(InstallProgress::Failed { code }) = &view.setup {
            return Err(AppError::new(*code).detail(format!("{name} could not be installed. Delete it and create it again, or check the version.")));
        }
        if status != InstanceStatus::Offline {
            moved = true;
        }
        let waited = started.elapsed().as_secs();
        match until {
            "installed" if installed => return Ok(format!("{name} is installed (status: {}).", status_word(status))),
            "offline" if installed && !status.expects_process() => {
                return Ok(format!("{name} is stopped (status: {}).", status_word(status)));
            }
            "online" if status == InstanceStatus::Online => {
                let address = service.list().await.ok().and_then(|all| all.into_iter().find(|v| v.instance.id == id)).and_then(|v| v.address);
                return Ok(format!(
                    "{name} is online after {waited}s{}.",
                    address.map(|a| format!(", at {a}")).unwrap_or_default()
                ));
            }
            "online" if status == InstanceStatus::Crashed => {
                let report = diagnose::report(service, &view).await.unwrap_or_default();
                return Ok(format!("{name} CRASHED while starting. Diagnosis:\n{report}"));
            }
            // Installed, stopped and nothing moving: nobody asked it to start.
            "online" if installed && status == InstanceStatus::Offline && !moved && waited >= 12 => {
                return Ok(format!(
                    "{name} is installed but not starting (status: offline). Call start_server first; if it then reports `agreement_required`, the owner has to accept the EULA in the app."
                ));
            }
            _ => {}
        }
        if waited >= timeout {
            let doing = match &view.setup {
                Some(_) => "still installing".to_string(),
                None => format!("status: {}", status_word(status)),
            };
            return Ok(format!("{name} is not {until} yet after {timeout}s ({doing}). Call wait_for_server again to keep waiting."));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// This PC and the ones paired with it, each asked whether it answers.
async fn list_machines(service: &Arc<InstanceService>) -> AppResult<String> {
    let node = service.node().await;
    let mut machines = vec![json!({
        "name": node.name,
        "this_pc": true,
        "note": "Leave `machine` out to act here.",
    })];
    for peer in peers::load()? {
        let info = crate::game::http_client()
            .get(format!("http://{}/info", peer.address))
            .timeout(Duration::from_secs(3))
            .send()
            .await;
        let info: Option<Value> = match info {
            Ok(response) => response.json().await.ok(),
            Err(_) => None,
        };
        let agents = info.as_ref().and_then(|i| i.get("agents")).and_then(Value::as_bool);
        machines.push(json!({
            "name": peer.name,
            "address": peer.address,
            "reachable": info.is_some(),
            "platform": info.as_ref().and_then(|i| i.get("platform")).cloned(),
            "version": info.as_ref().and_then(|i| i.get("version")).cloned(),
            "allows_agents": match (info.is_some(), agents) {
                (false, _) => json!("unknown (not answering)"),
                (true, Some(allowed)) => json!(allowed),
                (true, None) => json!("no (its Nebula Host is too old: update it)"),
            },
        }));
    }
    Ok(pretty(&machines))
}

async fn wait_until_stopped(service: &Arc<InstanceService>, id: &str, seconds: u64) -> AppResult<()> {
    let started = Instant::now();
    loop {
        if !service.state_of(id).await.status.expects_process() {
            return Ok(());
        }
        if started.elapsed().as_secs() >= seconds {
            return Err(AppError::new(ErrorCode::InstanceAlreadyRunning)
                .with("instance", id)
                .detail(format!("the server did not stop within {seconds}s")));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Put a jar built on the developer's machine into a server's addon folder.
async fn install_local_addon(service: &Arc<InstanceService>, args: &Value) -> AppResult<String> {
    let view = resolve(service, args).await?;
    let id = view.instance.id.clone();
    let (file_name, data) = local_file(args).await?;
    if !file_name.to_lowercase().ends_with(".jar") {
        return Err(invalid("only .jar files go in a server's mods or plugins folder"));
    }
    service.ensure_not_transferring(&id).await?;
    let (dir, _) = service.addon_profile(&view.instance)?;
    let restart = args.get("restart").and_then(Value::as_bool).unwrap_or(false);
    let start = args.get("start").and_then(Value::as_bool).unwrap_or(false);

    let was_running = view.state.status.expects_process();
    if was_running {
        if !restart {
            return Err(invalid(
                "the server is running and holds its jars open: pass restart=true to stop it, install and start it again",
            ));
        }
        service.stop(&id).await?;
        wait_until_stopped(service, &id, 90).await?;
    }

    std::fs::create_dir_all(&dir)?;
    let mut removed = Vec::new();
    let replace = args.get("replace").and_then(Value::as_str).map(str::trim).filter(|r| !r.is_empty());
    for old in replace.into_iter().chain(std::iter::once(file_name.as_str())) {
        for candidate in [old.to_string(), format!("{old}.disabled")] {
            let path = crate::store::paths::resolve_inside(&dir, &candidate)?;
            if path.is_file() && candidate != file_name {
                std::fs::remove_file(&path)?;
                removed.push(candidate);
            }
        }
    }
    let target = crate::store::paths::resolve_inside(&dir, &file_name)?;
    let temp = target.with_extension("nebula-tmp");
    std::fs::write(&temp, &data)?;
    std::fs::rename(&temp, &target)?;
    service.after_addon_change(&id).await;

    let mut text = format!("Installed `{file_name}` ({} KB) on {}.", data.len() / 1024, view.instance.name);
    if !removed.is_empty() {
        text.push_str(&format!(" Removed: {}.", removed.join(", ")));
    }
    if was_running || start {
        service.start(&id).await?;
        text.push_str(" The server is starting: call wait_for_server, then get_console with only_problems=true to check it loaded.");
    } else {
        text.push_str(" The server is stopped: start_server to load it.");
    }
    Ok(text)
}
