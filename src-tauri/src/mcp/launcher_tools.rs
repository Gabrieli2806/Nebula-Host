//! MCP tools for the Minecraft launcher: instances, their content (mods,
//! resource packs, shaders), versions, logs and launching. Useful to someone
//! who just wants "a 1.21 instance with Sodium" and to a mod developer who
//! wants to drop the jar they just built into a test instance, start it and
//! read the crash.
//!
//! Accounts never cross this boundary: tools see profile names only, and
//! launching uses an account that is already signed in on this device.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::launcher::{self, content, ContentKind, CreateProfile, Loader, Profile};

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub schema: Value,
    pub mutates: bool,
}

fn instance_arg() -> Value {
    json!({ "type": "string", "description": "Instance id or name (case-insensitive)." })
}
fn kind_arg() -> Value {
    json!({ "type": "string", "enum": ["mod", "resourcepack", "shader"], "description": "Content type (default mod)." })
}
fn loader_arg() -> Value {
    json!({ "type": "string", "enum": ["vanilla", "fabric", "forge", "neoforge"] })
}

pub fn catalog() -> Vec<Tool> {
    vec![
        Tool {
            name: "launcher_list_instances",
            title: "List game instances",
            description: "Every Minecraft instance in the launcher on this device: name, Minecraft version, loader, memory and whether the game is running.",
            schema: json!({ "type": "object", "properties": {} }),
            mutates: false,
        },
        Tool {
            name: "launcher_get_instance",
            title: "Instance details",
            description: "One instance: version, loader and loader build, memory, state, installed content, and folders (game, mods, logs, crash reports).",
            schema: json!({ "type": "object", "properties": { "instance": instance_arg() }, "required": ["instance"] }),
            mutates: false,
        },
        Tool {
            name: "launcher_list_versions",
            title: "Playable Minecraft versions",
            description: "Minecraft releases the launcher can play, newest first; with `loader` and `version`, the builds of that loader for that version.",
            schema: json!({ "type": "object", "properties": { "loader": loader_arg(), "version": { "type": "string" } } }),
            mutates: false,
        },
        Tool {
            name: "launcher_search_content",
            title: "Search Modrinth (launcher)",
            description: "Search Modrinth for mods, resource packs, shaders or modpacks. With `instance`, only results that fit its version and loader.",
            schema: json!({ "type": "object", "properties": {
                "query": { "type": "string" },
                "kind": { "type": "string", "enum": ["mod", "resourcepack", "shader", "modpack"] },
                "instance": instance_arg(),
                "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
            }, "required": ["query"] }),
            mutates: false,
        },
        Tool {
            name: "launcher_list_content",
            title: "Installed content",
            description: "What an instance has installed (file, type, on/off, Modrinth project when known).",
            schema: json!({ "type": "object", "properties": { "instance": instance_arg() }, "required": ["instance"] }),
            mutates: false,
        },
        Tool {
            name: "launcher_check_updates",
            title: "Check content updates",
            description: "Look up every installed file on Modrinth: its version, whether it is made for the instance's Minecraft version and loader, and a newer compatible file when there is one.",
            schema: json!({ "type": "object", "properties": { "instance": instance_arg() }, "required": ["instance"] }),
            mutates: false,
        },
        Tool {
            name: "launcher_read_log",
            title: "Read game log / crash report",
            description: "The game's output for an instance (latest.log, or the captured output), or with source=crash the newest crash report. Use it to explain why the game crashed or a mod failed to load.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "source": { "type": "string", "enum": ["log", "crash"] },
                "lines": { "type": "integer", "minimum": 10, "maximum": 2000, "description": "Last N lines (default 200)." },
                "errors_only": { "type": "boolean" }
            }, "required": ["instance"] }),
            mutates: false,
        },
        Tool {
            name: "launcher_list_accounts",
            title: "Signed-in accounts",
            description: "Minecraft profile names signed in on this device (no tokens). Launching with an account needs one of these.",
            schema: json!({ "type": "object", "properties": {} }),
            mutates: false,
        },
        Tool {
            name: "launcher_create_instance",
            title: "Create instance",
            description: "Create a new instance. Loader builds are picked automatically (the newest for that version).",
            schema: json!({ "type": "object", "properties": {
                "name": { "type": "string" },
                "version": { "type": "string", "description": "Minecraft version, e.g. 1.21.4." },
                "loader": loader_arg(),
                "memory_mb": { "type": "integer", "minimum": 512, "maximum": 32768 }
            }, "required": ["version"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_install_content",
            title: "Install from Modrinth",
            description: "Install the newest compatible version of a Modrinth project into an instance, with its required mods.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "project_id": { "type": "string", "description": "Modrinth project id or slug." },
                "kind": kind_arg()
            }, "required": ["instance", "project_id"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_install_local_file",
            title: "Install a local jar/zip",
            description: "Copy a file from this PC into an instance (a mod .jar, or a resource pack / shader .zip). For mod development: point it at your build output (e.g. build/libs/mymod-1.0.jar); an existing file with the same name is replaced.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "path": { "type": "string", "description": "Absolute path of the .jar or .zip." },
                "kind": kind_arg()
            }, "required": ["instance", "path"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_install_modpack",
            title: "Create instance from modpack",
            description: "Create a new instance from a Modrinth modpack (version, loader, mods and configs from the pack).",
            schema: json!({ "type": "object", "properties": { "project_id": { "type": "string" } }, "required": ["project_id"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_set_content_enabled",
            title: "Turn content on/off",
            description: "Switch an installed file on or off without deleting it (useful to bisect which mod breaks the game).",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "file": { "type": "string" },
                "enabled": { "type": "boolean" },
                "kind": kind_arg()
            }, "required": ["instance", "file", "enabled"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_remove_content",
            title: "Remove content",
            description: "Delete one installed file from an instance.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "file": { "type": "string" },
                "kind": kind_arg()
            }, "required": ["instance", "file"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_update_content",
            title: "Update content",
            description: "Update installed files to the newest version that fits the instance: one `file`, or all of them when omitted.",
            schema: json!({ "type": "object", "properties": { "instance": instance_arg(), "file": { "type": "string" } }, "required": ["instance"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_change_version",
            title: "Change instance version",
            description: "Move an instance to another Minecraft version and/or loader, keeping worlds and content. Run launcher_check_updates afterwards.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "version": { "type": "string" },
                "loader": loader_arg(),
                "loader_version": { "type": "string" }
            }, "required": ["instance", "version"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_set_memory",
            title: "Set instance memory",
            description: "Memory for the game, in MB (512–32768). Applies on the next start.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "memory_mb": { "type": "integer", "minimum": 512, "maximum": 32768 }
            }, "required": ["instance", "memory_mb"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_launch",
            title: "Start the game",
            description: "Start an instance with a signed-in account (by profile name; the first one when omitted), or install/repair only with install_only. Poll launcher_get_instance for the state and read launcher_read_log on failure.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "account": { "type": "string", "description": "Profile name of a signed-in account." },
                "install_only": { "type": "boolean" }
            }, "required": ["instance"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_stop",
            title: "Stop the game",
            description: "Close the running game (or cancel an install) of an instance.",
            schema: json!({ "type": "object", "properties": { "instance": instance_arg() }, "required": ["instance"] }),
            mutates: true,
        },
    ]
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput).detail(message.to_string())
}

fn engines() -> AppResult<(&'static Arc<launcher::client::ClientEngine>, &'static launcher::auth::AuthState)> {
    launcher::ENGINES
        .get()
        .map(|(engine, auth)| (engine, auth))
        .ok_or_else(|| AppError::new(ErrorCode::GameNotSupported).detail("The launcher is not available in this build."))
}

fn str_arg<'a>(args: &'a Value, key: &str) -> AppResult<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| invalid(&format!("`{key}` is required")))
}

fn kind_of(args: &Value) -> AppResult<ContentKind> {
    match args.get("kind").and_then(Value::as_str).unwrap_or("mod") {
        "mod" => Ok(ContentKind::Mod),
        "resourcepack" => Ok(ContentKind::Resourcepack),
        "shader" => Ok(ContentKind::Shader),
        "modpack" => Ok(ContentKind::Modpack),
        other => Err(invalid(&format!("unknown kind `{other}`"))),
    }
}

fn loader_of(value: Option<&str>) -> AppResult<Loader> {
    match value.unwrap_or("vanilla") {
        "vanilla" => Ok(Loader::Vanilla),
        "fabric" => Ok(Loader::Fabric),
        "forge" => Ok(Loader::Forge),
        "neoforge" => Ok(Loader::Neoforge),
        other => Err(invalid(&format!("unknown loader `{other}`"))),
    }
}

async fn resolve(args: &Value) -> AppResult<Profile> {
    let wanted = str_arg(args, "instance")?;
    let all = launcher::profiles().await?;
    let lower = wanted.to_lowercase();
    if let Some(found) = all.iter().find(|p| p.id.to_string() == wanted || p.name.to_lowercase() == lower) {
        return Ok(found.clone());
    }
    let matches: Vec<&Profile> = all.iter().filter(|p| p.name.to_lowercase().starts_with(&lower)).collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(AppError::new(ErrorCode::NotFound).with("instance", wanted)),
        many => Err(invalid(&format!(
            "`{wanted}` matches several instances: {}",
            many.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn loader_word(loader: &Loader) -> &'static str {
    match loader {
        Loader::Vanilla => "vanilla",
        Loader::Fabric => "fabric",
        Loader::Forge => "forge",
        Loader::Neoforge => "neoforge",
        Loader::Quilt => "quilt",
    }
}

fn tail(text: &str, lines: usize, errors_only: bool) -> String {
    let all: Vec<&str> = text
        .lines()
        .filter(|l| !errors_only || {
            let u = l.to_ascii_uppercase();
            u.contains("ERROR") || u.contains("WARN") || u.contains("EXCEPTION") || u.contains("CAUSED BY") || l.trim_start().starts_with("at ")
        })
        .collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

pub async fn call(name: &str, args: &Value) -> AppResult<String> {
    match name {
        "launcher_list_instances" => {
            let (engine, _) = engines()?;
            let statuses = engine.statuses().await;
            let list: Vec<Value> = launcher::profiles()
                .await?
                .iter()
                .map(|p| json!({
                    "id": p.id, "name": p.name, "version": p.game_version, "loader": loader_word(&p.loader),
                    "loader_version": p.loader_version, "memory_mb": p.memory_mb,
                    "state": statuses.get(&p.id).map(|s| s.phase.clone()).unwrap_or_else(|| "stopped".into()),
                }))
                .collect();
            if list.is_empty() {
                return Ok("No instances yet. Create one with launcher_create_instance.".into());
            }
            Ok(pretty(&Value::Array(list)))
        }
        "launcher_get_instance" => {
            let profile = resolve(args).await?;
            let (engine, _) = engines()?;
            let state = engine.statuses().await.remove(&profile.id);
            let dir = launcher::media::instance_folder(profile.id)?;
            let installed = content::list(&profile)?;
            Ok(pretty(&json!({
                "id": profile.id, "name": profile.name, "version": profile.game_version,
                "loader": loader_word(&profile.loader), "loader_version": profile.loader_version, "memory_mb": profile.memory_mb,
                "state": state.as_ref().map(|s| s.phase.clone()).unwrap_or_else(|| "stopped".into()),
                "progress": state.as_ref().map(|s| json!({ "stage": s.stage, "completed": s.completed, "total": s.total })),
                "error": state.as_ref().and_then(|s| s.error.as_ref()).map(|e| e.code.as_str()),
                "folders": { "game": dir, "mods": dir.join("mods"), "logs": dir.join("logs"), "crash_reports": dir.join("crash-reports") },
                "content": installed.iter().map(|i| json!({ "kind": i.kind, "file": i.file, "enabled": i.enabled })).collect::<Vec<_>>(),
            })))
        }
        "launcher_list_versions" => {
            if let (Some(loader), Some(version)) = (args.get("loader").and_then(Value::as_str), args.get("version").and_then(Value::as_str)) {
                let builds = launcher::loaders::versions(&loader_of(Some(loader))?, version).await?;
                return Ok(pretty(&json!({ "loader": loader, "version": version, "builds": builds.into_iter().take(30).collect::<Vec<_>>() })));
            }
            let versions: Vec<String> = launcher::versions().await?.into_iter().filter(|v| v.client_supported).map(|v| v.id).take(40).collect();
            Ok(pretty(&json!({ "playable_versions": versions })))
        }
        "launcher_search_content" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or("").to_string();
            let kind = kind_of(args)?;
            let fit = match args.get("instance").and_then(Value::as_str) {
                Some(_) => Some(resolve(args).await?),
                None => None,
            };
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10).clamp(1, 50) as u32;
            let result = launcher::search(query, kind, 0, limit, None, fit.as_ref()).await?;
            let hits: Vec<Value> = result
                .hits
                .iter()
                .map(|h| json!({ "project_id": h.project_id, "slug": h.slug, "title": h.title, "author": h.author, "downloads": h.downloads, "description": h.description }))
                .collect();
            Ok(pretty(&json!({ "total": result.total_hits, "hits": hits })))
        }
        "launcher_list_content" => {
            let profile = resolve(args).await?;
            let installed = content::list(&profile)?;
            Ok(pretty(&serde_json::to_value(installed).unwrap_or_default()))
        }
        "launcher_check_updates" => {
            let profile = resolve(args).await?;
            let report = content::check(&profile).await?;
            Ok(pretty(&serde_json::to_value(report).unwrap_or_default()))
        }
        "launcher_read_log" => {
            let profile = resolve(args).await?;
            let lines = args.get("lines").and_then(Value::as_u64).unwrap_or(200).clamp(10, 2000) as usize;
            let errors_only = args.get("errors_only").and_then(Value::as_bool).unwrap_or(false);
            let dir = launcher::media::instance_folder(profile.id)?;
            if args.get("source").and_then(Value::as_str) == Some("crash") {
                let newest = std::fs::read_dir(dir.join("crash-reports"))
                    .ok()
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().ends_with(".txt"))
                    .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
                let Some(entry) = newest else { return Ok("No crash reports for this instance.".into()) };
                let text = crate::service::logs::read_tail(&entry.path()).await?;
                return Ok(format!("{}\n\n{}", entry.file_name().to_string_lossy(), tail(&text, lines, errors_only)));
            }
            let latest = dir.join("logs").join("latest.log");
            let text = match crate::service::logs::read_tail(&latest).await {
                Ok(text) if !text.trim().is_empty() => text.lines().map(launcher::client::redact_line).collect::<Vec<_>>().join("\n"),
                _ => engines()?.0.logs(profile.id).join("\n"),
            };
            if text.trim().is_empty() {
                return Ok("No game output yet. Start the instance once.".into());
            }
            Ok(tail(&text, lines, errors_only))
        }
        "launcher_list_accounts" => {
            let (_, auth) = engines()?;
            let names: Vec<String> = auth.accounts().await.into_iter().map(|a| a.name).collect();
            if names.is_empty() {
                return Ok("No account is signed in. The user signs in from the launcher (top right / sidebar).".into());
            }
            Ok(pretty(&json!({ "accounts": names })))
        }
        "launcher_create_instance" => {
            let version = str_arg(args, "version")?.to_string();
            let loader = loader_of(args.get("loader").and_then(Value::as_str))?;
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(String::from)
                .unwrap_or_else(|| format!("{} {}", loader_word(&loader), version));
            let memory_mb = args.get("memory_mb").and_then(Value::as_u64).unwrap_or(4096) as u32;
            let profile = launcher::create(CreateProfile { name, game_version: version, loader, memory_mb }).await?;
            Ok(format!("Created instance `{}` ({}). Install content with launcher_install_content, then launcher_launch.", profile.name, profile.id))
        }
        "launcher_install_content" => {
            let profile = resolve(args).await?;
            let files = content::install(&profile, str_arg(args, "project_id")?, kind_of(args)?).await?;
            Ok(if files.is_empty() {
                format!("Already installed in `{}`.", profile.name)
            } else {
                format!("Installed into `{}`: {}", profile.name, files.join(", "))
            })
        }
        "launcher_install_local_file" => {
            let profile = resolve(args).await?;
            let path = Path::new(str_arg(args, "path")?);
            if !path.is_absolute() {
                return Err(invalid("`path` must be absolute"));
            }
            let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| invalid("`path` has no file name"))?.to_string();
            let data = tokio::fs::read(path).await?;
            let kind = kind_of(args)?;
            // Replace a previous build with the same name (and its disabled copy).
            for existing in [name.clone(), format!("{name}.disabled")] {
                let _ = content::remove(&profile, kind, &existing);
            }
            let saved = content::import_file(&profile, kind, &name, &data)?;
            Ok(format!("Copied `{saved}` into `{}`.", profile.name))
        }
        "launcher_install_modpack" => {
            let profile = content::install_modpack(str_arg(args, "project_id")?).await?;
            Ok(format!("Created instance `{}` ({}) from the modpack.", profile.name, profile.id))
        }
        "launcher_set_content_enabled" => {
            let profile = resolve(args).await?;
            let enabled = args.get("enabled").and_then(Value::as_bool).ok_or_else(|| invalid("`enabled` is required"))?;
            content::set_enabled(&profile, kind_of(args)?, str_arg(args, "file")?, enabled)?;
            Ok(format!("{} `{}`.", if enabled { "Enabled" } else { "Disabled" }, str_arg(args, "file")?))
        }
        "launcher_remove_content" => {
            let profile = resolve(args).await?;
            content::remove(&profile, kind_of(args)?, str_arg(args, "file")?)?;
            Ok(format!("Removed `{}` from `{}`.", str_arg(args, "file")?, profile.name))
        }
        "launcher_update_content" => {
            let profile = resolve(args).await?;
            let only = args.get("file").and_then(Value::as_str);
            let report = content::check(&profile).await?;
            let mut done = Vec::new();
            for item in report.iter().filter(|s| only.is_none_or(|f| f == s.file)) {
                let Some(next) = &item.update else { continue };
                let kind = match item.kind { "resourcepack" => ContentKind::Resourcepack, "shader" => ContentKind::Shader, _ => ContentKind::Mod };
                let file = content::update(&profile, kind, &item.file, &next.version_id).await?;
                done.push(format!("{} → {}", item.file, file));
            }
            Ok(if done.is_empty() { "Nothing to update.".into() } else { format!("Updated:\n{}", done.join("\n")) })
        }
        "launcher_change_version" => {
            let profile = resolve(args).await?;
            let loader = match args.get("loader").and_then(Value::as_str) {
                Some(l) => loader_of(Some(l))?,
                None => profile.loader.clone(),
            };
            let updated = launcher::change_version(
                profile.id,
                str_arg(args, "version")?.to_string(),
                loader,
                args.get("loader_version").and_then(Value::as_str).map(String::from),
            )
            .await?;
            Ok(format!(
                "`{}` is now Minecraft {} ({}{}). Check content with launcher_check_updates.",
                updated.name,
                updated.game_version,
                loader_word(&updated.loader),
                updated.loader_version.map(|v| format!(" {v}")).unwrap_or_default()
            ))
        }
        "launcher_set_memory" => {
            let profile = resolve(args).await?;
            let mb = args.get("memory_mb").and_then(Value::as_u64).ok_or_else(|| invalid("`memory_mb` is required"))? as u32;
            let updated = launcher::set_memory(profile.id, mb).await?;
            Ok(format!("`{}` now uses {} MB.", updated.name, updated.memory_mb))
        }
        "launcher_launch" => {
            let profile = resolve(args).await?;
            let (engine, auth) = engines()?;
            if args.get("install_only").and_then(Value::as_bool).unwrap_or(false) {
                engine.start(profile.clone(), None).await?;
                return Ok(format!("Checking and installing `{}`. Poll launcher_get_instance.", profile.name));
            }
            let accounts = auth.accounts().await;
            let account = match args.get("account").and_then(Value::as_str) {
                Some(name) => accounts.iter().find(|a| a.name.eq_ignore_ascii_case(name)),
                None => accounts.first(),
            }
            .ok_or_else(|| invalid("No signed-in account matches. Ask the user to sign in from the launcher."))?;
            let credentials = auth.credentials(&account.id).await?;
            engine.start(profile.clone(), Some(credentials)).await?;
            Ok(format!("Starting `{}` as {}. Poll launcher_get_instance; on failure read launcher_read_log (source=crash too).", profile.name, account.name))
        }
        "launcher_stop" => {
            let profile = resolve(args).await?;
            engines()?.0.stop(profile.id).await?;
            Ok(format!("Stopping `{}`.", profile.name))
        }
        _ => Err(invalid(&format!("unknown tool `{name}`"))),
    }
}
