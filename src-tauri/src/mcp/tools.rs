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

use super::console_wait::{self, Wait};
use super::{diagnose, profile};
use crate::service::clone::CloneOptions;
use crate::service::mod_share::ModShareDirection;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::registry::InstallProgress;
use crate::game::BuildSelector;
use crate::model::{InstanceStatus, InstanceView};
use crate::store::peers::{self, Peer};
use crate::node::health;
use crate::service::addons::BrowseRequest;
use crate::service::{backups, files, players, properties, CreateInstanceRequest, InstanceService};

/// The fixed guide. Its first paragraph stands on its own (what this is, what
/// to call first, the `machine` argument): some clients show an agent no more
/// than that. With `agreement_note` it has to fit in 4096 characters.
const INSTRUCTIONS: &str = "Nebula Host manages game servers (Minecraft today) and the Minecraft launcher on this PC, \
and on the machines paired with it. Call list_servers first: every `server` argument takes an id or a name it returns. \
Every tool takes an optional `machine`: leave it out for this PC, or pass a name from list_machines to act on a paired machine. \
Usual flow: list_servers, then start_server or install_local_addon, then wait_for_server, then get_console or diagnose_server to see what happened. \
\n\nCreating a server: create_server downloads files and takes a minute. Pass `start: true` to start it as soon as it is installed, \
then call wait_for_server, which returns when the server is online (or explains why it is not). Do not poll get_server in a loop. \
\n\nTesting a mod or plugin you just built: create (or reuse) a test server with the matching distribution and Minecraft version \
(paper/purpur/spigot for plugins; fabric, forge or neoforge for mods), then install_local_addon with the path of the built jar and \
`restart: true`; it stops the server if needed, replaces the jar and starts it again. Then wait_for_server, and read get_console \
(`only_problems: true`) or diagnose_server to check that it loaded. Run the commands under test with send_command and `wait_for` \
(the text that proves the effect), or wait_for_console for something that happens later, instead of sleeping and reading the console again. \
A development game client (`gradlew runClient`) has no Minecraft account: for it to join, the test server needs \
`online-mode` = `false` (create_server `settings`, or set_server_settings and a restart). Reuse one test server per \
loader and version instead of creating a new one for every build, and delete_server only the ones you created. \
\n\nWhen a server will not start or has crashed, call diagnose_server: it returns the latest console errors, \
the newest crash report and known-problem hints, which you can use to explain the cause and fix it. \
\n\nTo test against a real server's world and plugins without touching it, clone_server makes a stopped copy on its own port. \
Before a risky change call create_backup; restore_backup puts a backup back and keeps what it replaced as a new backup, so it can be undone. \
profile_server measures a running server (TPS, tick times, memory) and takes a spark profile when spark is installed. \
\n\nLong answers come in parts: lists take `limit` and `offset`, read_file takes `start_line`, and the answer says when there is more. \
A failed call says what to do next; read it before retrying. \
\n\nThe launcher_* tools manage Minecraft instances the user plays on this device (call launcher_list_instances first): create instances, \
search and install mods/resource packs/shaders from Modrinth, check updates and compatibility after a version change, change an instance's \
version, start or stop the game, and read its log or newest crash report. launcher_install_local_file copies a freshly built jar into a \
test instance; launcher_set_content_enabled helps bisect which mod breaks the game.";

/// What an agent is told when it connects: the fixed guide, plus what is true
/// on this machine now (whether the game's agreement still needs a person).
pub fn instructions() -> String {
    let mut text = INSTRUCTIONS.to_string();
    text.push_str("\n\n");
    text.push_str(agreement_note(agreements_accepted()));
    text
}

fn agreements_accepted() -> bool {
    crate::store::settings::load().map(|s| s.accept_agreements).unwrap_or(false)
}

fn agreement_note(accepted: bool) -> &'static str {
    if accepted {
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

/// The most text one answer carries: about 20 000 tokens of log. Tools that
/// can go over it answer in parts (`limit`/`offset`, `start_line`).
pub(super) const TEXT_BUDGET: usize = 60 * 1024;
/// The net under every tool, for an answer nobody expected to be this long.
const HARD_LIMIT: usize = 96 * 1024;

fn server_arg() -> Value {
    json!({ "type": "string", "description": "Server id or name from list_servers (case-insensitive; a unique start of the name is enough)." })
}

fn limit_arg(default: usize) -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": 1000, "default": default, "description": "How many to return." })
}

fn offset_arg() -> Value {
    json!({ "type": "integer", "minimum": 0, "default": 0, "description": "How many to skip: pass the `next_offset` of the previous answer for the next part." })
}

fn catalog() -> Vec<Tool> {
    vec![
        Tool {
            name: "list_servers",
            title: "List servers",
            description: "List every game server on this PC (or on `machine`): id, name, status (offline, sleeping, starting, online, stopping, crashed), distribution, version, players online, memory and address. Call it first: the other tools take a server's id or name from here. Returns a JSON array, or a sentence when there are no servers.",
            schema: json!({ "type": "object", "properties": {} }),
            mutates: false,
        },
        Tool {
            name: "get_server",
            title: "Server details",
            description: "Full details of one server: what list_servers gives plus its ports, folder path, automation (auto-start, restart after a crash), Java version and start times. Use it to look at one server once; to wait for it to start or stop call wait_for_server instead of polling this. Returns a JSON object.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "get_console",
            title: "Read console",
            description: "Read the newest console lines of a server, oldest first, as `HH:MM:SS [LEVEL] message`. Use it after a start, a deploy or a command to see what the server printed; `only_problems: true` keeps only warnings, errors and exceptions. It holds what was printed since the app started (the last 1000 lines); for older output use read_file on logs/latest.log. Returns plain text, at most about 60 KB (the newest lines are kept).",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "lines": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 150, "description": "How many of the newest lines to return." },
                "only_problems": { "type": "boolean", "default": false, "description": "Only WARN/ERROR lines and exceptions." }
            }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "diagnose_server",
            title: "Diagnose problems",
            description: "Explain why a server crashed or will not start: call this first when a start fails, before reading logs by hand. Returns a Markdown report: status, build, Java and memory; hints for known problems (wrong Java, port in use, out of memory, EULA, missing mod/plugin dependencies, incompatible mods); recent warnings and errors; the newest crash report; and the end of logs/latest.log.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "get_machine_status",
            title: "Machine status",
            description: "CPU, memory, disk and LAN address of this PC (or of `machine`), and how many servers are running on it. Use it before creating a server or giving one more memory. Returns a JSON object.",
            schema: json!({ "type": "object", "properties": {} }),
            mutates: false,
        },
        Tool {
            name: "list_players",
            title: "Players",
            description: "Players online on a server now, plus its allowed (whitelist), operator and banned lists. To change those lists use send_command (`whitelist add Steve`, `op Steve`, `ban Steve`). Returns a JSON object: `online`, `lists`.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "get_server_settings",
            title: "Game settings",
            description: "Read the game settings Nebula Host manages for a server (the main keys of server.properties for Minecraft: online-mode, max-players, difficulty…), each with its current value, type and default. Use these keys with set_server_settings or create_server `settings`; any other key of server.properties is read and changed with read_file and write_file. Returns a JSON array.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "list_files",
            title: "List files",
            description: "List the files and folders in a server's folder, or in a subfolder of it: name, relative path, whether it is a folder, size in bytes and modification time, folders first. Use it to find a config, a log or a plugin's data folder before read_file, write_file or delete_file. Returns a JSON object: `entries`, `total`, `offset` and, when there are more, `next_offset`.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "path": { "type": "string", "default": "", "description": "Folder relative to the server root, with forward slashes (e.g. `plugins/MyPlugin`). Omit for the root." },
                "limit": limit_arg(200),
                "offset": offset_arg()
            }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "read_file",
            title: "Read file",
            description: "Read a text file in a server's folder (a config, logs/latest.log, a crash report). A file up to about 60 KB comes whole. A longer one comes in parts: without `start_line` you get its end (what matters in a log); with `start_line`, the lines from there on. A partial answer starts with a line saying which lines it holds and how many the file has. Returns plain text.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "path": { "type": "string", "description": "File relative to the server root, with forward slashes (e.g. `logs/latest.log`, `server.properties`)." },
                "start_line": { "type": "integer", "minimum": 1, "description": "First line to return (1 is the first line of the file). Omit to get the whole file, or the end of a long one." },
                "max_lines": { "type": "integer", "minimum": 1, "description": "Return at most this many lines. Omit for as many as fit in one answer." }
            }, "required": ["server", "path"] }),
            mutates: false,
        },
        Tool {
            name: "list_backups",
            title: "Backups",
            description: "List the backups of a server, newest first: id, label, creation time, size and file path. Use it to check that a backup exists before a risky change (create_backup makes one). Returns a JSON array.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "list_versions",
            title: "Game versions",
            description: "List the game versions a distribution offers, newest first (the newest 60): use it to pick `version` for create_server. Returns one line of text: `Newest first (paper): 1.21.4, 1.21.3, …`.",
            schema: json!({ "type": "object", "properties": {
                "distribution": { "type": "string", "description": "One of vanilla, paper, purpur, spigot (plugins: paper, purpur, spigot), fabric, forge, neoforge (mods)." },
                "include_unstable": { "type": "boolean", "default": false, "description": "Also list versions that are not stable releases." }
            }, "required": ["distribution"] }),
            mutates: false,
        },
        Tool {
            name: "list_addons",
            title: "Installed mods/plugins",
            description: "List the mods or plugins installed on a server: file name, whether it is on, size and, for jars Modrinth knows, the project, the installed version and a newer compatible version when there is one. Use a `file` from here with remove_addon and set_addon_enabled. Returns a JSON object: `addons`, `total`, `offset`, `next_offset` when there are more, `updates_available`, `unknown_count`, `modrinth_status`.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "limit": limit_arg(100),
                "offset": offset_arg()
            }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "search_addons",
            title: "Search Modrinth",
            description: "Search Modrinth for mods or plugins that run on a server (its loader and game version). Use a hit's `project_id` or `slug` with install_addon. Returns a JSON object: `total`, `hits` (project_id, slug, title, description, downloads, author).",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "query": { "type": "string", "description": "What to search for, e.g. `luckperms` or `world edit`." },
                "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10, "description": "How many hits to return." }
            }, "required": ["server", "query"] }),
            mutates: false,
        },
        // -- changes ------------------------------------------------------
        Tool {
            name: "start_server",
            title: "Start server",
            description: "Start a server (also wakes a sleeping one). Returns at once with a sentence: call wait_for_server to know when it is online. Fails with `agreement_required` while the owner of the PC has not accepted the Minecraft EULA in the app.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: true,
        },
        Tool {
            name: "stop_server",
            title: "Stop server",
            description: "Stop a running server cleanly (it saves the world first). Returns at once with a sentence: call wait_for_server with `until: \"offline\"` before deleting the server or replacing files it holds open.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: true,
        },
        Tool {
            name: "restart_server",
            title: "Restart server",
            description: "Stop and start a server, e.g. after set_server_settings or after installing a mod or plugin. Returns at once with a sentence: call wait_for_server to know when it is back online.",
            schema: json!({ "type": "object", "properties": { "server": server_arg() }, "required": ["server"] }),
            mutates: true,
        },
        Tool {
            name: "send_command",
            title: "Run console command",
            description: "Run a console command on a running server (e.g. `say hi`, `op Steve`, `time set day`, `whitelist add Steve`) and get what the console printed in the next second. To run a command and wait for its effect in one call, pass `wait_for` with the text that proves it (e.g. `Reload complete`): the call then waits for that line as wait_for_console does. Returns plain text; with `wait_for`, the JSON object wait_for_console returns, plus `command`.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "command": { "type": "string", "description": "The command as typed in the server console, without a leading slash." },
                "wait_for": { "type": "string", "maxLength": 200, "description": "Text of a console line to wait for after sending the command (anywhere in the line, any case). Omit to get what was printed in the next second." },
                "regex": { "type": "boolean", "default": false, "description": "Read `wait_for` as a regular expression (Rust `regex` syntax: no look-around, no backreferences) instead of plain text." },
                "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 100, "default": 60, "description": "With `wait_for`: how long to wait for the line before answering anyway." }
            }, "required": ["server", "command"] }),
            mutates: true,
        },
        Tool {
            name: "create_server",
            title: "Create server",
            description: "Create a new server and install it in the background (a download: about a minute); meanwhile the Nebula Host app shows it as getting ready. Use list_versions to pick a version. Pass `start: true` to start it once installed, then call wait_for_server. Returns a sentence with the new server's id and what to call next.",
            schema: json!({ "type": "object", "properties": {
                "name": { "type": "string", "description": "Name shown in the app, e.g. `Test Paper 1.21.4`. Other tools find the server by it." },
                "distribution": { "type": "string", "description": "vanilla, paper, purpur, spigot, fabric, forge or neoforge. Omit for the default (Paper)." },
                "version": { "type": "string", "description": "Game version from list_versions, e.g. `1.21.4`. Omit for the newest stable." },
                "memory_mb": { "type": "integer", "minimum": 512, "description": "RAM for the server in MB. Omit to let Nebula Host choose." },
                "port": { "type": "integer", "minimum": 1024, "maximum": 65535, "description": "Port players join on. Omit to let Nebula Host choose a free one." },
                "settings": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Game settings to apply once installed (keys as in get_server_settings), e.g. {\"online-mode\":\"false\"} for a server a development client joins." },
                "start": { "type": "boolean", "default": false, "description": "Start the server as soon as it is installed." }
            }, "required": ["name"] }),
            mutates: true,
        },
        Tool {
            name: "wait_for_server",
            title: "Wait for a server",
            description: "Wait until a server is `online` (default), `installed` or `offline`, instead of polling get_server. Call it after create_server, start_server, stop_server, restart_server or install_local_addon. Returns a sentence as soon as the server gets there; the diagnosis if it crashes on the way; or, after `timeout_seconds`, where it stands, so you can call it again.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "until": { "type": "string", "enum": ["online", "installed", "offline"], "default": "online", "description": "The state to wait for." },
                "timeout_seconds": { "type": "integer", "minimum": 5, "maximum": 600, "default": 180, "description": "How long to wait before answering anyway." }
            }, "required": ["server"] }),
            mutates: false,
        },
        Tool {
            name: "list_machines",
            title: "Machines",
            description: "List this PC and the machines paired with it: name, address, whether each one answers and whether it allows agents. Pass a machine's `name` as `machine` to any other tool to act there instead of here. Returns a JSON array.",
            schema: json!({ "type": "object", "properties": {} }),
            mutates: false,
        },
        Tool {
            name: "install_local_addon",
            title: "Install a built mod/plugin jar",
            description: "Copy a .jar from this PC into a server's mods or plugins folder: the build output of the mod or plugin being developed (e.g. build/libs/myplugin-0.1.0.jar). A jar with the same file name is replaced. A running server holds its jars open: pass `restart: true` to stop it, replace the jar and start it again in one step, then call wait_for_server. Returns a sentence saying what was installed and removed, and what to call next.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "path": { "type": "string", "description": "Absolute path of the .jar on this PC (the one this endpoint runs on), also when `machine` names another: the file is sent there." },
                "replace": { "type": "string", "description": "A file name, not a flag: the older build to remove first when the version in the file name changed (e.g. myplugin-0.1.0.jar)." },
                "restart": { "type": "boolean", "default": false, "description": "If the server is running, stop it, install, and start it again." },
                "start": { "type": "boolean", "default": false, "description": "Start the server afterwards even if it was stopped." }
            }, "required": ["server", "path"] }),
            mutates: true,
        },
        Tool {
            name: "remove_addon",
            title: "Remove mod/plugin",
            description: "Delete an installed mod or plugin jar from a server. On a running server it is removed when the server stops. To switch one off and keep the file, use set_addon_enabled instead. Returns a sentence.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "file": { "type": "string", "description": "File name of the jar, as list_addons gives it (`file`)." }
            }, "required": ["server", "file"] }),
            mutates: true,
        },
        Tool {
            name: "set_addon_enabled",
            title: "Turn mod/plugin on/off",
            description: "Switch an installed mod or plugin on or off without deleting it: useful to find which one breaks the server. On a running server it applies when the server stops. Returns a sentence.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "file": { "type": "string", "description": "File name of the jar, as list_addons gives it (`file`)." },
                "enabled": { "type": "boolean", "description": "true to switch it on, false to switch it off." }
            }, "required": ["server", "file", "enabled"] }),
            mutates: true,
        },
        Tool {
            name: "write_file",
            title: "Write file",
            description: "Create or replace a text file in a server's folder (a plugin's config.yml, a datapack file, bukkit.yml). The whole file is replaced and no copy is kept: read_file it first when you mean to change only part of it. Missing folders are created. For the settings get_server_settings lists, use set_server_settings instead. Returns a sentence.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "path": { "type": "string", "description": "File relative to the server root, with forward slashes." },
                "content": { "type": "string", "description": "The full new text of the file." }
            }, "required": ["server", "path", "content"] }),
            mutates: true,
        },
        Tool {
            name: "delete_file",
            title: "Delete file or folder",
            description: "Delete a file, or a folder with everything in it, inside a server's folder (e.g. a plugin's data folder, to test a first run again). This cannot be undone: create_backup first if it may be needed. Returns a sentence.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "path": { "type": "string", "description": "File or folder relative to the server root, with forward slashes." }
            }, "required": ["server", "path"] }),
            mutates: true,
        },
        Tool {
            name: "delete_server",
            title: "Delete server",
            description: "Delete a stopped server and, unless `keep_files` is true, its folder with the world in it. This cannot be undone: only delete a server you created for a test, or one the user asked you to delete. A running server has to be stopped first (stop_server, then wait_for_server until offline). Returns a sentence.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "keep_files": { "type": "boolean", "default": false, "description": "Remove the server from Nebula Host and leave its folder on disk." },
                "delete_backups": { "type": "boolean", "default": false, "description": "Also delete the server's backups. By default they are kept on disk as a last resort; pass true for a throwaway test server or a clone." }
            }, "required": ["server"] }),
            mutates: true,
        },
        Tool {
            name: "set_memory",
            title: "Set memory",
            description: "Change how much RAM a server gets. It takes effect the next time the server starts (restart_server if it is running). Returns a sentence.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "memory_mb": { "type": "integer", "minimum": 512, "description": "RAM in MB, e.g. 4096." }
            }, "required": ["server", "memory_mb"] }),
            mutates: true,
        },
        Tool {
            name: "set_server_settings",
            title: "Change game settings",
            description: "Change game settings of a server by key, e.g. {\"max-players\":\"10\",\"pvp\":\"false\"}. Only the keys get_server_settings lists can be set here; for any other key edit server.properties with read_file and write_file. Most take effect after restart_server. Returns a sentence naming what was saved and what was not.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "values": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Setting key to new value, both as text." }
            }, "required": ["server", "values"] }),
            mutates: true,
        },
        Tool {
            name: "create_backup",
            title: "Back up",
            description: "Back up a server's files now, e.g. before deleting files or changing many settings. It works while the server runs (the server is asked to save first), but a file the running server holds locked is left out: the answer names those in `skipped`. Stop the server first for a backup with everything. Returns a JSON object: the backup's id, label, creation time, size and file path, and `skipped` with a `note` when files were left out.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "label": { "type": "string", "description": "A short note to recognise the backup by, e.g. `before update`." }
            }, "required": ["server"] }),
            mutates: true,
        },
        Tool {
            name: "install_addon",
            title: "Install mod/plugin",
            description: "Install a mod or plugin from Modrinth on a server: the best version for its loader and game version. Find the project with search_addons. Required dependencies are named in the answer, not installed: install each the same way. Restart the server to load it. Returns a sentence.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "project_id": { "type": "string", "description": "Modrinth project id or slug, from search_addons." }
            }, "required": ["server", "project_id"] }),
            mutates: true,
        },
        Tool {
            name: "share_mods",
            title: "Copy mods between server and instance",
            description: "Copy the mods of a server to a launcher instance, or the other way, so both run the same set. They must have the same Minecraft version and loader. Mods that cannot run on the receiving side stay behind and are reported; a jar of the same project there is replaced. Copying to a server requires it to be stopped. Returns a JSON object: `copied`, `already`, `skipped`.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "instance": { "type": "string", "description": "Launcher instance id or name, from launcher_list_instances." },
                "direction": { "type": "string", "enum": ["to_instance", "to_server"], "description": "`to_instance` copies the server's mods to the instance; `to_server` the instance's to the server." }
            }, "required": ["server", "instance", "direction"] }),
            mutates: true,
        },
        Tool {
            name: "wait_for_console",
            title: "Wait for a console line",
            description: "Wait until a server's console prints a line containing `pattern`, instead of sleeping and reading get_console again. Use it after something whose effect shows in the console later (a plugin enabling, a world finishing to load, a scheduled task). It looks at lines printed after the call began, or after `since`; to run a command and wait for its answer, use send_command with `wait_for`, which cannot miss a fast answer. It ends early if the server stops or crashes. Returns a JSON object: `matched`, `reason` (matched, timeout, crashed, not_running), `status`, `cursor`, and either `line` with the lines `before` and `after` it, or `last_lines` and `next` (what to call).",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "pattern": { "type": "string", "maxLength": 200, "description": "Text to look for anywhere in a console line, any case (e.g. `[MyPlugin] Enabled`). At most 200 characters." },
                "regex": { "type": "boolean", "default": false, "description": "Read `pattern` as a regular expression (Rust `regex` syntax: no look-around, no backreferences; case-sensitive unless it starts with `(?i)`) instead of plain text." },
                "since": { "type": "integer", "minimum": 0, "description": "A `cursor` from an earlier wait_for_console or send_command answer: look at the lines printed after it. 0 looks at everything the console still holds. Omit to look only at lines printed from now on." },
                "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 100, "default": 60, "description": "How long to wait before answering anyway. To wait longer, call again with `since` set to the `cursor` returned." }
            }, "required": ["server", "pattern"] }),
            mutates: false,
        },
        Tool {
            name: "restore_backup",
            title: "Restore a backup",
            description: "Put a backup of a server back: the files in the backup (world, settings, mods or plugins) replace the current ones; files created since are left in place. Use it to undo a change that went wrong. The server has to be stopped: pass `stop: true` to stop it first. Unless `keep_current` is false, the current state is saved as a new backup before, so the restore itself can be undone. The server stays stopped afterwards. Can take minutes for a large world. Returns a sentence naming the backup restored and the backup that holds the state it replaced.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "backup": { "type": "string", "description": "Backup id from list_backups, a part of it that only one backup has (e.g. its date and time, `20261122-1830`), or `latest` for the newest." },
                "stop": { "type": "boolean", "default": false, "description": "If the server is running, stop it first. Without it a running server is refused." },
                "keep_current": { "type": "boolean", "default": true, "description": "Back up the current state first, so this restore can be undone. Pass false only when the current state is not worth keeping." }
            }, "required": ["server", "backup"] }),
            mutates: true,
        },
        Tool {
            name: "clone_server",
            title: "Copy a server",
            description: "Make a new server that is a copy of an existing one: same game version, world, settings and mods or plugins, under a new name and on its own free port. Use it to try a plugin, an update or a risky command on a copy instead of on the server people play on. The original is not changed; if it is running it is asked to save its world first. The copy is made in the background and the app shows it as getting ready: call wait_for_server with the new name and `until: \"installed\"`. The copy is left stopped. Returns a sentence with the new server's id, port, the size being copied and what to call next.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "name": { "type": "string", "description": "Name of the new server, e.g. `Test copy of Survival`. It must not be the name of another server." },
                "include_world": { "type": "boolean", "default": true, "description": "Copy the world. With false the copy generates a new world on its first start." },
                "include_addons": { "type": "boolean", "default": true, "description": "Copy the mods or plugins folder (jars and their data)." }
            }, "required": ["server", "name"] }),
            mutates: true,
        },
        Tool {
            name: "profile_server",
            title: "Measure performance",
            description: "Measure how a running server performs: use it when players report lag, or to compare before and after a change. It asks the server itself (`tps` and `mspt` on Paper and Purpur, `forge tps` or `neoforge tps`, `tick query` on vanilla and Fabric) and spark when it is installed (`spark tps`, `spark health`), without printing in the console, and adds what the app tracks (CPU, memory, players). With `duration_seconds` and spark installed it also runs a spark profile for that long: its report shows which plugins or mods take the time and the entities and chunks of each world. Returns a JSON object: `tracked_by_app`, `sources` (each command, whether it `answered`, its `output`), `spark`, `profiler` (with the report `url`) and `notes` on what was not available and how to get it.",
            schema: json!({ "type": "object", "properties": {
                "server": server_arg(),
                "duration_seconds": { "type": "integer", "minimum": 5, "maximum": 60, "description": "Run a spark profile for this many seconds (the call takes that long plus the upload). Needs spark installed. Omit for the quick measurements only." }
            }, "required": ["server"] }),
            mutates: true,
        },
    ]
}

/// Tools that delete or overwrite something the user may want back.
const DESTRUCTIVE: &[&str] =
    &["launcher_remove_content", "delete_server", "delete_file", "remove_addon", "write_file", "restore_backup"];

/// Tools that change something and leave it the same when called again with
/// the same arguments. (Every tool that only reads is idempotent.)
const IDEMPOTENT_CHANGES: &[&str] = &[
    "set_memory",
    "set_server_settings",
    "set_addon_enabled",
    "write_file",
    "launcher_set_memory",
    "launcher_set_content_enabled",
    "launcher_change_version",
];

/// Tools that reach beyond the paired machines: Modrinth, or the game's and
/// the loaders' download servers.
const OPEN_WORLD: &[&str] = &[
    "list_versions",
    "list_addons",
    "search_addons",
    "create_server",
    "install_addon",
    // spark uploads its profile to spark.lucko.me.
    "profile_server",
    "launcher_list_versions",
    "launcher_search_content",
    "launcher_check_updates",
    "launcher_create_instance",
    "launcher_install_content",
    "launcher_install_modpack",
    "launcher_update_content",
    "launcher_change_version",
    "launcher_launch",
];

/// Every tool, in the order `tools/list` gives them: servers, then launcher.
fn all_tools() -> impl Iterator<Item = Tool> {
    catalog().into_iter().chain(launcher_catalog())
}

/// The tool an agent asked for, by the name this server gives it. `None`
/// when there is no such tool.
pub fn known_name(name: &str) -> Option<&'static str> {
    all_tools().map(|tool| tool.name).find(|known| *known == name)
}

pub fn definitions(read_only: bool) -> Vec<Value> {
    all_tools()
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
                    "destructiveHint": tool.mutates && DESTRUCTIVE.contains(&tool.name),
                    "idempotentHint": !tool.mutates || IDEMPOTENT_CHANGES.contains(&tool.name),
                    "openWorldHint": OPEN_WORLD.contains(&tool.name),
                }
            })
        })
        .collect()
}

/// What to do about a failure, for the ones an agent can do something about.
fn next_step(error: &AppError) -> Option<&'static str> {
    let reason = error.params.get("reason").map(String::as_str);
    Some(match error.code {
        ErrorCode::InvalidInput if reason == Some("agreement_required") => {
            "The owner of this PC has to accept the Minecraft EULA in the Nebula Host app (on the server's page, or once for every server in Settings > Servers). You cannot accept it for them: tell them, then call start_server again."
        }
        ErrorCode::InstanceNotFound => "No server has that id or name. Call list_servers for the ones there are (with `machine` for a paired machine).",
        ErrorCode::NotFound if error.params.contains_key("instance") => {
            "No launcher instance has that id or name. Call launcher_list_instances for the ones there are."
        }
        ErrorCode::InstanceAlreadyRunning => "The server is running. Use restart_server, or stop_server and then wait_for_server with until=offline.",
        ErrorCode::InstanceNotRunning => "The server is not running. Call start_server, then wait_for_server.",
        ErrorCode::ConsoleDetached => "The app restarted while this server ran, so its console takes no input. Call restart_server, then wait_for_server.",
        ErrorCode::InstanceStartFailed | ErrorCode::InstanceCrashed | ErrorCode::InstanceLaunchFilesMissing => {
            "Call diagnose_server to see why."
        }
        ErrorCode::VersionNotFound => "Call list_versions (launcher_list_versions for an instance) for the versions there are.",
        ErrorCode::FileNotFound => "Check the path: list_files shows what a server's folder holds.",
        ErrorCode::BackupNotFound => "Call list_backups for the ids of this server's backups (create_backup makes one).",
        ErrorCode::BackupFailed if reason == Some("requires_stop") => {
            "This game cannot be backed up while it runs. Call stop_server, then wait_for_server with until=offline."
        }
        ErrorCode::PathOutsideInstance => "Paths are relative to the server's folder and cannot leave it.",
        ErrorCode::FileTooLarge => "The file is too large to read or write through this tool.",
        ErrorCode::PortInUse => "Another program uses that port. Pass a different `port`.",
        ErrorCode::NotEnoughMemory => "Call get_machine_status for the memory there is, and ask for less.",
        ErrorCode::UpstreamUnavailable | ErrorCode::DownloadFailed => {
            "A download server (Modrinth, Mojang or the loader's) did not answer. Try again in a moment."
        }
        _ => return None,
    })
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
    if let Some(step) = next_step(error) {
        text.push_str(&format!("\n{step}"));
    }
    text
}

pub(super) fn invalid(message: &str) -> AppError {
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

/// One part of a list, as every tool that answers in parts gives it:
/// `{ <key>: [...], total, offset }` and `next_offset` while there is more.
pub(super) fn paged(key: &str, items: Vec<Value>, args: &Value, default_limit: usize) -> Value {
    let total = items.len();
    let number = |name: &str| args.get(name).and_then(Value::as_u64).map(|n| usize::try_from(n).unwrap_or(usize::MAX));
    let offset = number("offset").unwrap_or(0).min(total);
    let limit = number("limit").unwrap_or(default_limit).clamp(1, 1000);
    let part: Vec<Value> = items.into_iter().skip(offset).take(limit).collect();
    let end = offset + part.len();
    let mut out = json!({ "total": total, "offset": offset });
    out[key] = Value::Array(part);
    if end < total {
        out["next_offset"] = json!(end);
    }
    out
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
        // While it is being set up: the install phase (`downloading`…) or `failed`.
        "setup": view.setup.as_ref().and_then(|s| serde_json::to_value(s).ok()).and_then(|s| s.get("phase").cloned()),
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
    let Some(tool) = all_tools().find(|t| t.name == name) else {
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
    // wait_for_server may hold the call for as long as it was asked to, and
    // a backup or a restore of a large world takes minutes.
    let timeout = Duration::from_secs(match name {
        "wait_for_server" => 660,
        "restore_backup" | "create_backup" => 600,
        _ => 180,
    });
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
            "{} does not allow agents from paired machines. Only its owner can turn that on, in the Nebula Host app there: Settings > AI agents (MCP) > Details > Allow agents from paired machines.",
            peer.name
        ),
        (401, _) => format!("{} no longer accepts this PC: pair them again in the Nebula Host app.", peer.name),
        (404 | 405, _) => format!("{} runs an older Nebula Host without agent access. Update it there.", peer.name),
        _ => format!("{} answered with an error ({status}): {body}", peer.name),
    })
}

fn read_only_error() -> AppError {
    AppError::new(ErrorCode::InvalidInput)
        .detail("Agents may only look at this machine, not change it. Only its owner can allow changes, in the Nebula Host app: Settings > AI agents (MCP) > Details > View only. Tell them; do not retry.")
}

/// Run a tool on this machine. Whatever it answers, an agent can read: an
/// answer nobody expected to be this long is cut, saying so.
pub async fn call(service: &Arc<InstanceService>, read_only: bool, name: &str, args: &Value) -> AppResult<String> {
    let text = run(service, read_only, name, args).await?;
    if text.len() <= HARD_LIMIT {
        return Ok(text);
    }
    Ok(format!(
        "{}\nThe answer was too long to send whole. Ask for less: fewer `lines`, a smaller `limit`, a narrower `path` or query.",
        diagnose::head_bytes(&text, HARD_LIMIT)
    ))
}

async fn run(service: &Arc<InstanceService>, read_only: bool, name: &str, args: &Value) -> AppResult<String> {
    let Some(tool) = all_tools().find(|t| t.name == name) else {
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
            // `errors_only` is what launcher_read_log calls it: either is taken.
            let only_problems =
                ["only_problems", "errors_only"].iter().any(|key| args.get(*key).and_then(Value::as_bool).unwrap_or(false));
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
                return Ok(if only_problems && !tail.is_empty() {
                    "No warnings or errors in the console.".into()
                } else {
                    "The console is empty (the server has not run since the app started). For older output use read_file on logs/latest.log.".into()
                });
            }
            Ok(diagnose::tail_bytes(&out.join("\n"), TEXT_BUDGET))
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
            let items = entries.iter().filter_map(|entry| serde_json::to_value(entry).ok()).collect();
            let mut out = paged("entries", items, args, 200);
            out["path"] = json!(path);
            Ok(pretty(&out))
        }
        "read_file" => {
            let view = resolve(service, args).await?;
            let path = str_arg(args, "path")?;
            let text = files::read_text(&view.instance.path, path)?;
            let line = |name: &str| args.get(name).and_then(Value::as_u64).map(|n| usize::try_from(n).unwrap_or(usize::MAX));
            Ok(diagnose::page_lines(&text, line("start_line"), line("max_lines"), TEXT_BUDGET))
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
            let report = service.addons_list(&view.instance.id).await?;
            // What an agent acts on. The app's own view (every version with
            // its game versions and files) is many times an answer's size.
            let items = report
                .addons
                .iter()
                .map(|addon| {
                    let known = addon.modrinth.as_ref();
                    json!({
                        "file": addon.file_name,
                        "enabled": addon.enabled,
                        "size_bytes": addon.size_bytes,
                        "project_id": known.map(|m| m.project.project_id.as_str()),
                        "title": known.map(|m| m.project.title.as_str()),
                        "version": known.map(|m| m.installed_version.version_number.as_str()),
                        "update": known
                            .filter(|m| m.update_available)
                            .and_then(|m| m.latest_version.as_ref())
                            .map(|v| v.version_number.as_str()),
                    })
                })
                .collect();
            let mut out = paged("addons", items, args, 100);
            out["modrinth_status"] = json!(report.modrinth_status);
            out["unknown_count"] = json!(report.unknown_count);
            out["updates_available"] = json!(report.updates_available);
            Ok(pretty(&out))
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
            Ok(format!("Starting {}. Call wait_for_server to know when it is online.", view.instance.name))
        }
        "stop_server" => {
            let view = resolve(service, args).await?;
            service.stop(&view.instance.id).await?;
            Ok(format!("Stopping {}. Call wait_for_server with until=offline to know when it has stopped.", view.instance.name))
        }
        "restart_server" => {
            let view = resolve(service, args).await?;
            let id = view.instance.id.clone();
            let svc = service.clone();
            tokio::spawn(async move {
                let _ = svc.restart(&id).await;
            });
            Ok(format!("Restarting {}. Call wait_for_server to know when it is back online.", view.instance.name))
        }
        "send_command" => {
            let view = resolve(service, args).await?;
            let command = str_arg(args, "command")?.trim_start_matches('/').to_string();
            let id = view.instance.id.as_str();
            let wait = match args.get("wait_for") {
                Some(wanted) if !wanted.is_null() => Some(Wait::from_args(args, "wait_for")?),
                _ => None,
            };
            // Where the console is before the command: its answer is what
            // comes after, however fast it is and however full the buffer.
            let before = service.console_cursor(id).await;
            service.send_command(id, &command).await?;
            if let Some(mut wait) = wait {
                wait.since = Some(before);
                let mut answer = console_wait::wait(service, id, wait).await;
                answer["command"] = json!(command);
                return Ok(pretty(&answer));
            }
            tokio::time::sleep(std::time::Duration::from_millis(900)).await;
            let (after, _) = service.console_since(id, before).await;
            let new: Vec<String> = after.iter().map(diagnose::format_line).collect();
            Ok(if new.is_empty() {
                format!("Sent `{command}`. No output yet.")
            } else {
                format!("Sent `{command}`. Output:\n{}", diagnose::tail_bytes(&new.join("\n"), 24 * 1024))
            })
        }
        "create_server" => create_server(service, args).await,
        "wait_for_server" => wait_for_server(service, args).await,
        "wait_for_console" => {
            let view = resolve(service, args).await?;
            let wait = Wait::from_args(args, "pattern")?;
            Ok(pretty(&console_wait::wait(service, &view.instance.id, wait).await))
        }
        "restore_backup" => restore_backup(service, args).await,
        "clone_server" => clone_server(service, args).await,
        "profile_server" => {
            let view = resolve(service, args).await?;
            profile::report(service, &view, profile::profile_seconds(args)).await
        }
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
            let delete_backups = args.get("delete_backups").and_then(Value::as_bool).unwrap_or(false);
            let kept = backups::list(&view.instance.id).unwrap_or_default();
            service.delete(&view.instance.id, !keep_files).await?;
            let mut text = if keep_files {
                format!("Removed {} from Nebula Host. Its folder was kept: {}", view.instance.name, view.instance.path.display())
            } else {
                format!("Deleted {} and its files.", view.instance.name)
            };
            text.push_str(&backups_after_delete(&view.instance.id, &kept, delete_backups));
            Ok(text)
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
            // Only the settings the app manages are written: say which were
            // not, instead of letting the agent believe they were.
            let (saved, unknown): (Vec<&str>, Vec<&str>) = edits
                .keys()
                .map(String::as_str)
                .partition(|key| manifest.settings.iter().any(|field| field.key == *key));
            if saved.is_empty() {
                return Err(invalid(&format!(
                    "none of these is a setting Nebula Host manages: {}. Call get_server_settings for the keys it does; for any other key edit server.properties with read_file and write_file",
                    unknown.join(", ")
                )));
            }
            service.write_game_settings(&view.instance.id, &edits).await?;
            let mut text = format!(
                "Saved on {}: {}. Call restart_server for most to apply.",
                view.instance.name,
                saved.join(", ")
            );
            if !unknown.is_empty() {
                text.push_str(&format!(
                    " NOT saved (not settings Nebula Host manages; edit server.properties with read_file and write_file): {}.",
                    unknown.join(", ")
                ));
            }
            Ok(text)
        }
        "create_backup" => {
            let view = resolve(service, args).await?;
            let label = args.get("label").and_then(Value::as_str).map(String::from);
            let backup = backups::create(service, &view.instance.id, label).await?;
            let mut out = serde_json::to_value(&backup).unwrap_or_default();
            if !backup.skipped.is_empty() {
                out["note"] = json!("The server is running and holds the `skipped` files locked: they are not in this backup. For a backup with everything, call stop_server, wait_for_server with until=offline, then create_backup.");
            }
            Ok(pretty(&out))
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

/// What became of a deleted server's backups, for the answer. They outlive
/// the server unless asked otherwise: the last way back from a delete.
fn backups_after_delete(instance_id: &str, all: &[backups::Backup], delete: bool) -> String {
    if all.is_empty() {
        return String::new();
    }
    let size = megabytes(all.iter().map(|backup| backup.size_bytes).sum());
    if delete {
        let removed = all.iter().filter(|backup| backups::delete(instance_id, &backup.id).is_ok()).count();
        return format!(" Its {removed} backup(s) ({size}) were deleted too.");
    }
    let folder = all[0].path.parent().map(|dir| dir.display().to_string()).unwrap_or_default();
    format!(
        " Its {} backup(s) ({size}) were kept, as a last resort, in {folder} (files named {instance_id}-*.zip). No server lists them any more: they can be unzipped by hand, and deleted there when no longer wanted.",
        all.len()
    )
}

/// A size the way a person reads it.
fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

/// The backup an agent means: its id, `latest`, or a piece of the id that
/// only one backup has (the date and time are the telling part). `all` is
/// newest first.
fn pick_backup<'a>(all: &'a [backups::Backup], wanted: &str) -> AppResult<&'a backups::Backup> {
    let wanted = wanted.trim();
    let lower = wanted.to_lowercase();
    let not_found = || AppError::new(ErrorCode::BackupNotFound).with("backup", wanted);
    if all.is_empty() {
        return Err(not_found().detail("this server has no backups"));
    }
    if matches!(lower.as_str(), "latest" | "newest" | "last") {
        return Ok(&all[0]);
    }
    if let Some(exact) = all.iter().find(|backup| backup.id.to_lowercase() == lower) {
        return Ok(exact);
    }
    let matches: Vec<&backups::Backup> = all.iter().filter(|backup| backup.id.to_lowercase().contains(&lower)).collect();
    match matches.as_slice() {
        [one] => Ok(one),
        [] => Err(not_found()),
        many => Err(invalid(&format!(
            "`{wanted}` matches several backups: {}. Pass a whole id",
            many.iter().take(8).map(|backup| backup.id.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

/// What restore_backup was asked for besides the server and the backup.
fn restore_options(args: &Value) -> (bool, bool) {
    let flag = |key: &str, default: bool| args.get(key).and_then(Value::as_bool).unwrap_or(default);
    (flag("stop", false), flag("keep_current", true))
}

/// Put a backup back over a stopped server, keeping what it replaces.
async fn restore_backup(service: &Arc<InstanceService>, args: &Value) -> AppResult<String> {
    let view = resolve(service, args).await?;
    let id = view.instance.id.as_str();
    let name = &view.instance.name;
    let all = backups::list(id)?;
    let backup = pick_backup(&all, str_arg(args, "backup")?)?.clone();
    let (stop, keep_current) = restore_options(args);
    service.ensure_not_transferring(id).await?;

    let was_running = view.state.status.expects_process();
    if was_running {
        if !stop {
            return Err(invalid(
                "the server is running, and a restore under a running server leaves a world half old and half new: pass stop=true to stop it first, or call stop_server and wait_for_server with until=offline",
            ));
        }
        service.stop(id).await?;
        wait_until_stopped(service, id, 90).await?;
    }

    let safety = if keep_current {
        // Backups are named by the second: never write this one over the
        // backup about to be restored.
        if (chrono::Utc::now() - backup.created_at).num_seconds().abs() < 2 {
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        Some(backups::create(service, id, Some("before restore".into())).await?)
    } else {
        None
    };
    backups::restore(service, id, &backup.id).await?;
    service.after_folder_change(id).await;

    let mut text = format!(
        "Restored backup `{}` ({}, made {}) over {name}: its files replaced the current ones; files created since then were left in place.",
        backup.id,
        megabytes(backup.size_bytes),
        backup.created_at.format("%Y-%m-%d %H:%M UTC"),
    );
    match &safety {
        Some(safety) => text.push_str(&format!(
            " The state it replaced is kept as backup `{}` ({}): call restore_backup with that id to undo this restore.",
            safety.id,
            megabytes(safety.size_bytes)
        )),
        None => text.push_str(" The state it replaced was not kept (keep_current=false)."),
    }
    text.push_str(if was_running {
        " The server was stopped for this and is still stopped: call start_server, then wait_for_server."
    } else {
        " The server is stopped: call start_server, then wait_for_server."
    });
    Ok(text)
}

/// What clone_server was asked to copy.
fn clone_options(args: &Value) -> CloneOptions {
    let flag = |key: &str| args.get(key).and_then(Value::as_bool).unwrap_or(true);
    CloneOptions { include_world: flag("include_world"), include_addons: flag("include_addons") }
}

/// Begin a copy of a server; wait_for_server (until=installed) says when it is done.
async fn clone_server(service: &Arc<InstanceService>, args: &Value) -> AppResult<String> {
    let view = resolve(service, args).await?;
    let name = str_arg(args, "name")?;
    // Tools find a server by its name: two with the same one could not be told apart.
    if service.list().await?.iter().any(|other| other.instance.name.eq_ignore_ascii_case(name)) {
        return Err(invalid(&format!("a server called `{name}` already exists: pass another `name`")));
    }
    let options = clone_options(args);
    let started = service.clone_instance(&view.instance.id, name, options).await?;

    let copy = &started.instance;
    let manifest = service.registry.manifest(&copy.game)?;
    let mut text = format!(
        "Copying {} to \"{}\" (id {}, port {}): {} in {} files. ",
        view.instance.name,
        copy.name,
        copy.id,
        copy.primary_port(&manifest),
        megabytes(started.bytes),
        started.files
    );
    if started.flushed {
        text.push_str("The original is running: it was asked to save its world first, and keeps running. ");
    }
    if !options.include_world {
        text.push_str("The world is not copied: the copy generates a new one on its first start. ");
    }
    if !options.include_addons {
        text.push_str("The mods or plugins are not copied. ");
    }
    text.push_str(&format!(
        "The copy is made in the background and shows in the app as getting ready. Call wait_for_server with server=\"{}\" and until=installed to know when it is done (call it again if it times out: a large world takes minutes). It is left stopped: start_server starts it.",
        copy.name
    ));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn is_name(name: &str) -> bool {
        let mut chars = name.chars();
        chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && name.len() <= 64
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }

    /// What the shared checker (`mcp-check.py`) asks of every tool.
    #[test]
    fn every_tool_keeps_the_contract() {
        let tools = definitions(false);
        assert!(!tools.is_empty());
        let mut seen = std::collections::BTreeSet::new();
        for tool in &tools {
            let name = tool["name"].as_str().unwrap();
            assert!(is_name(name), "{name}: short snake_case");
            assert!(!name.starts_with("nebula_host_"), "{name}: the client already says whose it is");
            assert!(seen.insert(name), "{name}: listed twice");
            assert!(tool["title"].as_str().is_some_and(|t| !t.is_empty()), "{name}: title");
            let description = tool["description"].as_str().unwrap_or("");
            assert!((20..=4096).contains(&description.len()), "{name}: description");
            assert!(description.contains("Returns"), "{name}: says what it returns");

            for hint in ["readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint"] {
                assert!(tool["annotations"][hint].is_boolean(), "{name}: {hint}");
            }
            if tool["annotations"]["readOnlyHint"] == true {
                assert_eq!(tool["annotations"]["destructiveHint"], false, "{name}");
                assert_eq!(tool["annotations"]["idempotentHint"], true, "{name}");
            }

            let schema = &tool["inputSchema"];
            assert_eq!(schema["type"], "object", "{name}");
            for word in ["anyOf", "oneOf", "allOf"] {
                assert!(schema.get(word).is_none(), "{name}: {word} at the root");
            }
            let properties = schema["properties"].as_object().unwrap_or_else(|| panic!("{name}: properties"));
            for (key, property) in properties {
                assert!(
                    !key.is_empty() && key.len() <= 64 && key.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)),
                    "{name}.{key}"
                );
                assert!(property["type"].is_string(), "{name}.{key}: type");
                assert!(property["description"].as_str().is_some_and(|d| !d.is_empty()), "{name}.{key}: description");
            }
            for required in schema["required"].as_array().into_iter().flatten() {
                assert!(properties.contains_key(required.as_str().unwrap()), "{name}: requires {required}, which it does not have");
            }
            assert_eq!(properties.contains_key("machine"), name != "list_machines", "{name}: machine");
        }
        // The lists that set the annotations name tools that exist.
        for name in DESTRUCTIVE.iter().chain(IDEMPOTENT_CHANGES).chain(OPEN_WORLD) {
            assert!(seen.contains(name), "{name} is not a tool");
        }
    }

    #[test]
    fn the_list_is_stable_and_view_only_leaves_what_only_reads() {
        let names = |tools: &[Value]| tools.iter().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let all = definitions(false);
        assert_eq!(names(&all), names(&definitions(false)));
        assert_eq!(all[0]["name"], "list_servers", "what the guide says to call first comes first");

        let visible = definitions(true);
        assert!(visible.len() < all.len());
        assert!(visible.iter().all(|t| t["annotations"]["readOnlyHint"] == true));
        assert_eq!(known_name("delete_server"), Some("delete_server"), "hidden, and still a tool: calling it is refused, not unknown");
        assert_eq!(known_name("no_such_tool"), None);
    }

    #[test]
    fn annotations_say_what_a_tool_does() {
        let tools = definitions(false);
        let hints = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("{name}"))["annotations"].clone();
        let flags = |name: &str| {
            let h = hints(name);
            ["readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint"].map(|k| h[k] == true)
        };
        assert_eq!(flags("list_servers"), [true, false, true, false]);
        assert_eq!(flags("wait_for_server"), [true, false, true, false]);
        assert_eq!(flags("search_addons"), [true, false, true, true]);
        assert_eq!(flags("start_server"), [false, false, false, false]);
        assert_eq!(flags("set_memory"), [false, false, true, false]);
        assert_eq!(flags("delete_server"), [false, true, false, false]);
        assert_eq!(flags("write_file"), [false, true, true, false]);
        assert_eq!(flags("install_addon"), [false, false, false, true]);
        assert_eq!(flags("launcher_remove_content"), [false, true, false, false]);
        assert_eq!(flags("launcher_set_content_enabled"), [false, false, true, false]);
    }

    #[test]
    fn the_guide_fits_and_its_start_stands_alone() {
        for accepted in [true, false] {
            let whole = format!("{INSTRUCTIONS}\n\n{}", agreement_note(accepted));
            assert!(whole.chars().count() <= 4096, "{} characters", whole.chars().count());
        }
        let start: String = INSTRUCTIONS.chars().take(500).collect();
        for word in ["Nebula Host", "game servers", "list_servers first", "`machine`", "list_machines"] {
            assert!(start.contains(word), "the first 500 characters do not mention {word}");
        }
    }

    #[test]
    fn a_failure_says_what_to_do_next() {
        let missing = describe_error(&AppError::new(ErrorCode::InstanceNotFound).with("instance", "nope"));
        assert!(missing.starts_with("Error: instance_not_found (instance=nope)"), "{missing}");
        assert!(missing.contains("list_servers"));

        let eula = describe_error(&AppError::new(ErrorCode::InvalidInput).with("reason", "agreement_required"));
        assert!(eula.contains("agreement_required") && eula.contains("EULA"));

        let plain = describe_error(&invalid("missing argument `server`"));
        assert_eq!(plain, "Error: invalid_input\nmissing argument `server`");
    }

    fn backup(id: &str, age_hours: i64) -> backups::Backup {
        backups::Backup {
            id: id.to_string(),
            instance_id: "srv".into(),
            label: None,
            created_at: chrono::Utc::now() - chrono::Duration::hours(age_hours),
            size_bytes: 1024,
            path: std::path::Path::new("backups").join(format!("{id}.zip")),
            skipped: Vec::new(),
        }
    }

    #[test]
    fn a_backup_is_found_by_id_by_a_piece_of_it_or_as_the_latest() {
        // Newest first, as list_backups gives them.
        let all = [backup("srv-20261122-183000", 1), backup("srv-20261121-090000-auto", 30), backup("srv-20261120-090000-auto", 54)];
        let picked = |wanted: &str| pick_backup(&all, wanted).map(|found| found.id.clone());

        assert_eq!(picked("srv-20261121-090000-auto").unwrap(), "srv-20261121-090000-auto");
        assert_eq!(picked(" SRV-20261122-183000 ").unwrap(), "srv-20261122-183000");
        assert_eq!(picked("latest").unwrap(), "srv-20261122-183000");
        assert_eq!(picked("20261120").unwrap(), "srv-20261120-090000-auto");

        let several = picked("090000").unwrap_err();
        assert!(describe_error(&several).contains("matches several backups: srv-20261121-090000-auto, srv-20261120-090000-auto"));
        let missing = describe_error(&picked("20250101").unwrap_err());
        assert!(missing.starts_with("Error: backup_not_found (backup=20250101)") && missing.contains("list_backups"), "{missing}");
        let none = describe_error(&pick_backup(&[], "latest").unwrap_err());
        assert!(none.contains("no backups") && none.contains("create_backup"), "{none}");
    }

    #[test]
    fn a_restore_keeps_the_current_state_and_refuses_a_running_server_unless_told() {
        assert_eq!(restore_options(&json!({})), (false, true), "does not stop, does keep");
        assert_eq!(restore_options(&json!({ "stop": true, "keep_current": false })), (true, false));
        assert_eq!(restore_options(&json!({ "stop": "yes", "keep_current": 0 })), (false, true), "anything but a boolean is the default");
    }

    #[test]
    fn a_copy_takes_everything_unless_told() {
        let all = clone_options(&json!({}));
        assert!(all.include_world && all.include_addons);
        let bare = clone_options(&json!({ "include_world": false, "include_addons": false }));
        assert!(!bare.include_world && !bare.include_addons);
    }

    #[test]
    fn deleting_a_server_says_what_became_of_its_backups() {
        assert_eq!(backups_after_delete("srv", &[], false), "", "nothing to say when there were none");
        let kept = backups_after_delete("srv", &[backup("srv-20261122-183000", 1), backup("srv-20261121-090000-auto", 30)], false);
        assert!(kept.contains("2 backup(s)") && kept.contains("were kept") && kept.contains("backups") && kept.contains("srv-*.zip"), "{kept}");
    }

    #[test]
    fn sizes_read_like_sizes() {
        assert_eq!(megabytes(0), "0.0 MB");
        assert_eq!(megabytes(5 * 1024 * 1024 + 512 * 1024), "5.5 MB");
        assert_eq!(megabytes(250 * 1024 * 1024), "250 MB");
        assert_eq!(megabytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn the_new_tools_are_marked_for_what_they_do() {
        let tools = definitions(false);
        let flags = |name: &str| {
            let tool = tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("{name}"));
            ["readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint"].map(|k| tool["annotations"][k] == true)
        };
        assert_eq!(flags("wait_for_console"), [true, false, true, false]);
        assert_eq!(flags("restore_backup"), [false, true, false, false]);
        assert_eq!(flags("clone_server"), [false, false, false, false]);
        assert_eq!(flags("profile_server"), [false, false, false, true]);

        // View only: waiting on the console is looking; the rest is not offered.
        let visible: Vec<String> = definitions(true).iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert!(visible.contains(&"wait_for_console".to_string()));
        for hidden in ["restore_backup", "clone_server", "profile_server", "send_command"] {
            assert!(!visible.contains(&hidden.to_string()), "{hidden}");
        }
    }

    #[test]
    fn a_list_comes_in_parts() {
        let items = |n: usize| (0..n).map(|i| json!(i)).collect::<Vec<_>>();

        let all = paged("entries", items(3), &json!({}), 200);
        assert_eq!(all, json!({ "entries": [0, 1, 2], "total": 3, "offset": 0 }));

        let first = paged("entries", items(5), &json!({ "limit": 2 }), 200);
        assert_eq!(first, json!({ "entries": [0, 1], "total": 5, "offset": 0, "next_offset": 2 }));
        let last = paged("entries", items(5), &json!({ "limit": 2, "offset": 4 }), 200);
        assert_eq!(last, json!({ "entries": [4], "total": 5, "offset": 4 }));
        let past = paged("entries", items(5), &json!({ "offset": 99 }), 200);
        assert_eq!(past, json!({ "entries": [], "total": 5, "offset": 5 }));
        assert_eq!(paged("entries", items(5), &json!({}), 2)["next_offset"], 2);
    }
}
