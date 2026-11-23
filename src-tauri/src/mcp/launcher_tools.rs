//! MCP tools for the Minecraft launcher: instances, their content (mods,
//! resource packs, shaders), versions, logs and launching. Useful to someone
//! who just wants "a 1.21 instance with Sodium" and to a mod developer who
//! wants to drop the jar they just built into a test instance, start it and
//! read the crash.
//!
//! Accounts never cross this boundary: tools see profile names only, and
//! launching uses an account that is already signed in on this device.

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
    json!({ "type": "string", "description": "Instance id or name from launcher_list_instances (case-insensitive; a unique start of the name is enough)." })
}
fn kind_arg() -> Value {
    json!({ "type": "string", "enum": ["mod", "resourcepack", "shader"], "default": "mod", "description": "What the file is: it decides the folder." })
}
fn loader_arg(default: &str) -> Value {
    json!({ "type": "string", "enum": ["vanilla", "fabric", "forge", "neoforge"], "description": default })
}
fn file_arg() -> Value {
    json!({ "type": "string", "description": "File name as launcher_list_content gives it (`file`), with `.disabled` when it is off." })
}

pub fn catalog() -> Vec<Tool> {
    vec![
        Tool {
            name: "launcher_list_instances",
            title: "List game instances",
            description: "List every Minecraft instance in the launcher on this device: id, name, Minecraft version, loader, memory and state (stopped, installing, ready, running, stopping…). Call it first for anything about the game itself (not a server): the other launcher_* tools take an instance's id or name from here. Returns a JSON array, or a sentence when there are none.",
            schema: json!({ "type": "object", "properties": {} }),
            mutates: false,
        },
        Tool {
            name: "launcher_get_instance",
            title: "Instance details",
            description: "One instance in full: version, loader and loader build, memory, state with install progress and last error, its folders (game, mods, logs, crash reports) and its installed content (the first 200 files; launcher_list_content has them all). Use it to follow an install or a start after launcher_launch. Returns a JSON object.",
            schema: json!({ "type": "object", "properties": { "instance": instance_arg() }, "required": ["instance"] }),
            mutates: false,
        },
        Tool {
            name: "launcher_list_versions",
            title: "Playable Minecraft versions",
            description: "List the Minecraft releases the launcher can play, newest first (the newest 40): use it to pick `version` for launcher_create_instance or launcher_change_version. With both `loader` and `version`, lists instead the builds of that loader for that version (the newest 30). Returns a JSON object: `playable_versions`, or `loader`, `version`, `builds`.",
            schema: json!({ "type": "object", "properties": {
                "loader": loader_arg("With `version`: the loader whose builds to list."),
                "version": { "type": "string", "description": "With `loader`: the Minecraft version whose loader builds to list, e.g. `1.21.4`." }
            } }),
            mutates: false,
        },
        Tool {
            name: "launcher_search_content",
            title: "Search Modrinth (launcher)",
            description: "Search Modrinth for mods, resource packs, shaders or modpacks for the game. With `instance`, only results that fit its Minecraft version and loader. Use a hit's `project_id` or `slug` with launcher_install_content (launcher_install_modpack for a modpack). Returns a JSON object: `total`, `hits` (project_id, slug, title, author, downloads, description).",
            schema: json!({ "type": "object", "properties": {
                "query": { "type": "string", "description": "What to search for, e.g. `sodium`." },
                "kind": { "type": "string", "enum": ["mod", "resourcepack", "shader", "modpack"], "default": "mod", "description": "What to search for." },
                "instance": instance_arg(),
                "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10, "description": "How many hits to return." }
            }, "required": ["query"] }),
            mutates: false,
        },
        Tool {
            name: "launcher_list_content",
            title: "Installed content",
            description: "List what an instance has installed: each file with its type (mod, resourcepack, shader), size, whether it is on, and its Modrinth project when Nebula Host installed it. Use a `file` from here with launcher_set_content_enabled, launcher_remove_content and launcher_update_content. Returns a JSON object: `content`, `total`, `offset` and, when there are more, `next_offset`.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 200, "description": "How many to return." },
                "offset": { "type": "integer", "minimum": 0, "default": 0, "description": "How many to skip: pass the `next_offset` of the previous answer for the next part." }
            }, "required": ["instance"] }),
            mutates: false,
        },
        Tool {
            name: "launcher_check_updates",
            title: "Check content updates",
            description: "Check every installed file of an instance against Modrinth: its version, whether it is made for the instance's Minecraft version and loader (`compatible`), and a newer file that fits when there is one (`update`). Call it after launcher_change_version, or when the game crashes on start. Apply updates with launcher_update_content. Returns a JSON object: `content`, `total`, `offset` and, when there are more, `next_offset`.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 100, "description": "How many to return." },
                "offset": { "type": "integer", "minimum": 0, "default": 0, "description": "How many to skip: pass the `next_offset` of the previous answer for the next part." }
            }, "required": ["instance"] }),
            mutates: false,
        },
        Tool {
            name: "launcher_read_log",
            title: "Read game log / crash report",
            description: "Read the end of the game's output for an instance (logs/latest.log, or the output captured at start), or with `source: \"crash\"` the newest crash report. Use it to explain why the game crashed or a mod failed to load. Returns plain text, at most about 60 KB (the newest lines are kept).",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "source": { "type": "string", "enum": ["log", "crash"], "default": "log", "description": "`log` for the game's output, `crash` for the newest crash report." },
                "lines": { "type": "integer", "minimum": 10, "maximum": 2000, "default": 200, "description": "How many of the last lines to return." },
                "errors_only": { "type": "boolean", "default": false, "description": "Only warnings, errors, exceptions and stack trace lines." }
            }, "required": ["instance"] }),
            mutates: false,
        },
        Tool {
            name: "launcher_list_accounts",
            title: "Signed-in accounts",
            description: "List the Minecraft profile names signed in on this device (names only, never tokens). launcher_launch needs one of them; signing in is something only the user does, in the app. Returns a JSON object: `accounts`, or a sentence when nobody is signed in.",
            schema: json!({ "type": "object", "properties": {} }),
            mutates: false,
        },
        Tool {
            name: "launcher_create_instance",
            title: "Create instance",
            description: "Create a new Minecraft instance in the launcher. The loader build is picked for you (the newest for that version). Then add content with launcher_install_content and start it with launcher_launch. Returns a sentence with the new instance's name and id.",
            schema: json!({ "type": "object", "properties": {
                "name": { "type": "string", "description": "Name shown in the launcher. Omit for `<loader> <version>`." },
                "version": { "type": "string", "description": "Minecraft version from launcher_list_versions, e.g. `1.21.4`." },
                "loader": loader_arg("Mod loader. Omit for vanilla (no mods)."),
                "memory_mb": { "type": "integer", "minimum": 512, "maximum": 32768, "default": 4096, "description": "RAM for the game in MB." }
            }, "required": ["version"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_install_content",
            title: "Install from Modrinth",
            description: "Install a Modrinth project into an instance: the newest version that fits its Minecraft version and loader, together with the mods it requires. Find the project with launcher_search_content. Returns a sentence listing the files installed.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "project_id": { "type": "string", "description": "Modrinth project id or slug, from launcher_search_content." },
                "kind": kind_arg()
            }, "required": ["instance", "project_id"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_install_local_file",
            title: "Install a local jar/zip",
            description: "Copy a file from this PC into an instance: a mod .jar, or a resource pack or shader .zip. For mod development, point it at your build output (e.g. build/libs/mymod-1.0.jar); a file with the same name already there is replaced. Returns a sentence.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "path": { "type": "string", "description": "Absolute path of the .jar or .zip on this PC (the one this endpoint runs on)." },
                "kind": kind_arg()
            }, "required": ["instance", "path"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_install_modpack",
            title: "Create instance from modpack",
            description: "Create a new instance from a Modrinth modpack: its Minecraft version, loader, mods and configs. Find the pack with launcher_search_content and `kind: \"modpack\"`. It downloads every mod of the pack, which can take minutes. Returns a sentence with the new instance's name and id.",
            schema: json!({ "type": "object", "properties": {
                "project_id": { "type": "string", "description": "Modrinth project id or slug of the modpack." }
            }, "required": ["project_id"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_set_content_enabled",
            title: "Turn content on/off",
            description: "Switch an installed file of an instance on or off without deleting it: useful to bisect which mod breaks the game. It applies the next time the game starts. Returns a sentence.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "file": file_arg(),
                "enabled": { "type": "boolean", "description": "true to switch it on, false to switch it off." },
                "kind": kind_arg()
            }, "required": ["instance", "file", "enabled"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_remove_content",
            title: "Remove content",
            description: "Delete one installed file (a mod, resource pack or shader) from an instance. To switch it off and keep it, use launcher_set_content_enabled instead. Returns a sentence.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "file": file_arg(),
                "kind": kind_arg()
            }, "required": ["instance", "file"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_update_content",
            title: "Update content",
            description: "Update the installed files of an instance to the newest version that fits it, as launcher_check_updates reports: one `file`, or every file with an update when `file` is omitted. Returns the list of files updated, or a sentence when there was nothing to update.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "file": { "type": "string", "description": "File name to update, as launcher_list_content gives it. Omit to update everything." }
            }, "required": ["instance"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_change_version",
            title: "Change instance version",
            description: "Move an instance to another Minecraft version, another loader, or both, keeping its worlds and content. Content made for the old version stays as it is: call launcher_check_updates afterwards, then launcher_update_content. Returns a sentence with the new version and loader.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "version": { "type": "string", "description": "Minecraft version to move to, from launcher_list_versions." },
                "loader": loader_arg("Loader to move to. Omit to keep the instance's loader."),
                "loader_version": { "type": "string", "description": "A loader build from launcher_list_versions. Omit for the newest." }
            }, "required": ["instance", "version"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_set_memory",
            title: "Set instance memory",
            description: "Change how much RAM the game of an instance gets. It applies the next time the game starts. Returns a sentence.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "memory_mb": { "type": "integer", "minimum": 512, "maximum": 32768, "description": "RAM in MB, e.g. 4096." }
            }, "required": ["instance", "memory_mb"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_launch",
            title: "Start the game",
            description: "Start the game of an instance on this device, signed in with one of the user's accounts; or, with `install_only: true`, only download and repair its files. Starting the game needs the owner's permission in the app (off by default); `install_only` does not. Returns at once with a sentence: follow the state with launcher_get_instance and, if it fails, read launcher_read_log.",
            schema: json!({ "type": "object", "properties": {
                "instance": instance_arg(),
                "account": { "type": "string", "description": "Profile name of a signed-in account, from launcher_list_accounts. Omit for the first one." },
                "install_only": { "type": "boolean", "default": false, "description": "Download and check the game's files without starting it." }
            }, "required": ["instance"] }),
            mutates: true,
        },
        Tool {
            name: "launcher_stop",
            title: "Stop the game",
            description: "Close the running game of an instance, or cancel its install. Unsaved progress in a single-player world may be lost: use it for test instances, or when the user asks. Returns a sentence.",
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

pub(super) async fn resolve(args: &Value) -> AppResult<Profile> {
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

/// The last `lines` lines of a log, within what one answer carries.
fn tail(text: &str, lines: usize, errors_only: bool) -> String {
    let all: Vec<&str> = text
        .lines()
        .filter(|l| !errors_only || {
            let u = l.to_ascii_uppercase();
            u.contains("ERROR") || u.contains("WARN") || u.contains("EXCEPTION") || u.contains("CAUSED BY") || l.trim_start().starts_with("at ")
        })
        .collect();
    super::diagnose::tail_bytes(&all[all.len().saturating_sub(lines)..].join("\n"), super::tools::TEXT_BUDGET)
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
                "content_total": installed.len(),
                "content": installed.iter().take(200).map(|i| json!({ "kind": i.kind, "file": i.file, "enabled": i.enabled })).collect::<Vec<_>>(),
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
            let items = installed.iter().filter_map(|i| serde_json::to_value(i).ok()).collect();
            Ok(pretty(&super::tools::paged("content", items, args, 200)))
        }
        "launcher_check_updates" => {
            let profile = resolve(args).await?;
            let report = content::check(&profile).await?;
            let items = report
                .iter()
                .filter_map(|status| serde_json::to_value(status).ok())
                .map(|mut status| {
                    // A picture's address is of no use to an agent.
                    if let Some(fields) = status.as_object_mut() {
                        fields.remove("icon_url");
                    }
                    status
                })
                .collect();
            Ok(pretty(&super::tools::paged("content", items, args, 100)))
        }
        "launcher_read_log" => {
            let profile = resolve(args).await?;
            let lines = args.get("lines").and_then(Value::as_u64).unwrap_or(200).clamp(10, 2000) as usize;
            // `only_problems` is what get_console calls it: either is taken.
            let errors_only = ["errors_only", "only_problems"].iter().any(|key| args.get(*key).and_then(Value::as_bool).unwrap_or(false));
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
                let raw = crate::service::logs::read_tail(&entry.path()).await?;
                let text = raw.lines().map(launcher::client::redact_line).collect::<Vec<_>>().join("\n");
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
            // By path on this PC, or carried in the call from a paired one.
            let (name, data) = super::tools::local_file(args).await?;
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
            // Playing with the account is a separate, opt-in permission.
            if !crate::store::settings::load().map(|s| s.mcp_launcher_play).unwrap_or(false) {
                return Err(AppError::new(ErrorCode::InvalidInput).detail(
                    "Starting the game with the user's account is off for agents. Only the user can allow it, in the Nebula Host app: Settings > AI agents (MCP) > Details > What agents may do > Start the game. Tell them; `install_only: true` works without it.",
                ));
            }
            let accounts = auth.accounts().await;
            let account = match args.get("account").and_then(Value::as_str) {
                Some(name) => accounts.iter().find(|a| a.name.eq_ignore_ascii_case(name)),
                None => accounts.first(),
            }
            .ok_or_else(|| invalid("No signed-in account matches: launcher_list_accounts gives the names. If there is none, ask the user to sign in from the launcher."))?;
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
