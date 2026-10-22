//! The MCP tools: what an assistant can ask this node to do.
//!
//! Every tool goes through the same services the UI uses, so the rules (path
//! safety, one status owner, typed errors) hold for the assistant exactly as
//! they do for a person. Results are short, readable text or JSON — the
//! assistant reads them, a person never parses them.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};

use super::diagnose;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::BuildSelector;
use crate::model::{InstanceStatus, InstanceView};
use crate::node::health;
use crate::service::addons::BrowseRequest;
use crate::service::{backups, files, players, properties, CreateInstanceRequest, InstanceService};

pub const INSTRUCTIONS: &str = "Nebula Host manages game servers (Minecraft today) running on this PC. \
Use list_servers first to find a server; every server argument accepts the server id or its name. \
When a server will not start or has crashed, call diagnose_server: it returns the latest console errors, \
the newest crash report and known-problem hints, which you can use to explain the cause and suggest a fix. \
Prefer explaining and asking before destructive changes. Creating a server downloads files and takes a minute; \
poll get_server until status is offline (installed) before starting it. \
The launcher_* tools manage Minecraft instances the user plays on this device: create instances, search and install mods/resource packs/shaders \
from Modrinth, check updates and compatibility after a version change, change an instance's version, start or stop the game, and read its log \
or newest crash report. For mod development, launcher_install_local_file copies a freshly built jar into a test instance; \
launcher_set_content_enabled helps bisect which mod breaks the game.";

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
            description: "Create a new server and install it in the background. Use list_versions to pick a version. Poll get_server until it is installed (status offline) before starting. The server cannot start until its owner accepts the Minecraft EULA in the Nebula Host app; tell them to do so.",
            schema: json!({ "type": "object", "properties": {
                "name": { "type": "string" },
                "distribution": { "type": "string", "description": "e.g. vanilla, paper, purpur, spigot, fabric, forge, neoforge. Omit for the default (Paper)." },
                "version": { "type": "string", "description": "Game version; omit for the newest stable." },
                "memory_mb": { "type": "integer", "minimum": 512, "description": "RAM for the server in MB." },
                "port": { "type": "integer", "minimum": 1024, "maximum": 65535 }
            }, "required": ["name"] }),
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
    ]
}

pub fn definitions(read_only: bool) -> Vec<Value> {
    catalog()
        .into_iter()
        .chain(launcher_catalog())
        .filter(|tool| !(read_only && tool.mutates))
        .map(|tool| {
            json!({
                "name": tool.name,
                "title": tool.title,
                "description": tool.description,
                "inputSchema": tool.schema,
                "annotations": {
                    "readOnlyHint": !tool.mutates,
                    "destructiveHint": tool.name == "launcher_remove_content",
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

pub async fn call(service: &Arc<InstanceService>, read_only: bool, name: &str, args: &Value) -> AppResult<String> {
    let Some(tool) = catalog().into_iter().chain(launcher_catalog()).find(|t| t.name == name) else {
        return Err(invalid(&format!("unknown tool `{name}`")));
    };
    if read_only && tool.mutates {
        return Err(AppError::new(ErrorCode::InvalidInput)
            .detail("Nebula Host's MCP access is read-only. The user can allow changes in Settings → AI assistants (MCP)."));
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
        // server waits until they accept it in the app.
        accept_agreement: false,
    };
    let instance = service.create(request).await?;
    let id = instance.id.clone();
    let svc = service.clone();
    tokio::spawn(async move {
        if let Err(error) = svc.install(&id).await {
            tracing::warn!(instance_id = %id, %error, "MCP-created server failed to install");
        }
    });
    Ok(format!(
        "Created \"{name}\" ({distribution} {version}), id {}. Installing in the background — poll get_server until `installed` is true, then start_server.",
        instance.id
    ))
}
