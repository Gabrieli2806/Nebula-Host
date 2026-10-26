//! The remote command surface: the same commands the desktop UI sends over
//! Tauri IPC, by the same names and argument keys (camelCase, as the UI sends
//! them), answered by the same services. A UI connected to another node calls
//! these over HTTP instead of IPC and cannot tell the difference — apart from
//! the few commands that only make sense on the machine you are sitting at.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::node::health;
use crate::node::pairing;
use crate::runtime::java;
use crate::service::addons::BrowseRequest;
use crate::service::icon::IconFit;
use crate::service::logs::LogSource;
use crate::service::players::PlayerEntry;
use crate::service::transfer::FinalizeMode;
use crate::service::{backups, files, players, properties, CreateInstanceRequest, InstancePatch, InstanceService};
use crate::store::settings::{self as settings_store, AppSettings};

fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> AppResult<T> {
    let value = args.get(key).cloned().unwrap_or(Value::Null);
    serde_json::from_value(value).map_err(|e| {
        AppError::new(ErrorCode::InvalidInput).with("argument", key).detail(e.to_string())
    })
}

fn opt<T: DeserializeOwned>(args: &Value, key: &str) -> AppResult<Option<T>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => arg(args, key).map(Some),
    }
}

fn ok<T: Serialize>(value: T) -> AppResult<Value> {
    Ok(serde_json::to_value(value)?)
}

fn local_only(command: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput)
        .with("reason", "local_only")
        .detail(format!("`{command}` only works on the machine itself, not over a remote connection"))
}

pub async fn dispatch(service: &Arc<InstanceService>, command: &str, args: &Value) -> AppResult<Value> {
    let id = || arg::<String>(args, "instanceId");
    // A server being packed for a transfer, or one that just arrived and
    // waits for its source, does not take changes: they would be missing
    // from the copy that left, or thrown away on undo. (The service guards
    // its own mutating calls; these write the folder directly.)
    if matches!(
        command,
        "file_write" | "file_delete" | "file_rename" | "folder_create" | "players_write_list" | "instance_settings_set"
            | "files_upload_import"
    ) {
        service.ensure_not_transferring(&id()?).await?;
    }
    match command {
        // -- node -----------------------------------------------------------
        "node_get" => ok(service.node().await),
        "node_rename" => ok(service.rename_node(arg(args, "name")?).await?),
        "node_health" => {
            let running = service.processes.running_pids().await.len() as u32;
            ok(health::sample(running))
        }
        "node_requirements" => ok(health::check_requirements()?),
        "node_pairing_code" => ok(pairing::issue(&service.node().await.id)?),
        "node_forget_device" => ok(service.remove_paired_device(&arg::<String>(args, "deviceId")?).await?),

        // -- games ------------------------------------------------------------
        "games_list" => ok(service.registry.manifests()),
        "game_versions" => {
            let game: String = arg(args, "game")?;
            ok(service.registry.get(&game)?.versions(&arg::<String>(args, "distribution")?).await?)
        }
        "game_loader_versions" => {
            let game: String = arg(args, "game")?;
            ok(service
                .registry
                .get(&game)?
                .loader_versions(&arg::<String>(args, "distribution")?, &arg::<String>(args, "version")?)
                .await?)
        }

        // -- instances --------------------------------------------------------
        "instances_list" => ok(service.list().await?),
        "instance_get" => ok(service.view(&id()?).await?),
        "instance_create" => ok(service.create(arg::<CreateInstanceRequest>(args, "request")?).await?),
        "instance_install" => ok(service.install(&id()?).await?),
        "instance_accept_agreement" => ok(service.accept_agreement(&id()?).await?),
        "instance_start" => ok(service.start(&id()?).await?),
        "instance_stop" => ok(service.stop(&id()?).await?),
        "instance_restart" => ok(service.restart(&id()?).await?),
        "instance_sleep" => ok(service.sleep(&id()?).await?),
        "instance_delete" => ok(service.delete(&id()?, arg(args, "deleteFiles")?).await?),
        "instance_command" => ok(service.send_command(&id()?, &arg::<String>(args, "command")?).await?),
        "instance_console" => {
            let limit: Option<usize> = opt(args, "limit")?;
            ok(service.console_tail(&id()?, limit.unwrap_or(500)).await)
        }
        "console_completions" => {
            let refresh: Option<bool> = opt(args, "refresh")?;
            ok(service.completions(&id()?, refresh.unwrap_or(false)).await?)
        }
        "instance_disk_usage" => {
            let refresh: Option<bool> = opt(args, "refresh")?;
            ok(service.disk_usage(&id()?, refresh.unwrap_or(false)).await?)
        }
        "instance_update" => ok(service.apply_patch(&id()?, arg::<InstancePatch>(args, "patch")?).await?),
        "instance_forward_port" => {
            let instance_id = id()?;
            let instance = service.store.get(&instance_id).await?;
            let manifest = service.registry.manifest(&instance.game)?;
            let port = instance.primary_port(&manifest);
            ok(service.forwarder.forward(&instance_id, port, &format!("Nebula — {}", instance.name)).await?)
        }
        "instance_firewall" => ok(service.firewall_status(&id()?).await?),
        "instance_firewall_fix" => ok(service.firewall_fix(&id()?).await?),
        "instance_release_port" => {
            service.forwarder.release(&id()?).await;
            ok(())
        }
        "instance_icon_set" => {
            let fit: Option<IconFit> = opt(args, "fit")?;
            ok(service.set_icon(&id()?, &arg::<String>(args, "data")?, fit.unwrap_or_default()).await?)
        }
        "instance_icon_clear" => ok(service.clear_icon(&id()?).await?),
        "instance_log_files" => ok(service.log_files(&id()?).await?),
        "instance_log_share" => ok(service.share_log(&id()?, arg::<LogSource>(args, "source")?).await?),

        // -- files --------------------------------------------------------------
        "files_list" => {
            let instance = service.store.get(&id()?).await?;
            let path: Option<String> = opt(args, "path")?;
            ok(files::list(&instance.path, path.as_deref().unwrap_or(""))?)
        }
        "file_read" => {
            let instance = service.store.get(&id()?).await?;
            ok(files::read_text(&instance.path, &arg::<String>(args, "path")?)?)
        }
        "file_write" => {
            let instance_id = id()?;
            let instance = service.store.get(&instance_id).await?;
            files::write_text(&instance.path, &arg::<String>(args, "path")?, &arg::<String>(args, "contents")?)?;
            service.after_folder_change(&instance_id).await;
            ok(())
        }
        "file_delete" => {
            let instance_id = id()?;
            let instance = service.store.get(&instance_id).await?;
            files::delete(&instance.path, &arg::<String>(args, "path")?)?;
            service.after_folder_change(&instance_id).await;
            ok(())
        }
        "file_rename" => {
            let instance_id = id()?;
            let instance = service.store.get(&instance_id).await?;
            files::rename(&instance.path, &arg::<String>(args, "path")?, &arg::<String>(args, "newName")?)?;
            service.after_folder_change(&instance_id).await;
            ok(())
        }
        "folder_create" => {
            let instance = service.store.get(&id()?).await?;
            ok(files::create_dir(&instance.path, &arg::<String>(args, "path")?)?)
        }

        // Files dropped on another machine arrive as one bundle (`/upload`):
        // unpack it once, then check and copy from there like a local drop.
        // Sources must be inside that upload's own folder.
        "files_upload_check" => {
            let instance = service.store.get(&id()?).await?;
            let staged = crate::service::import::stage_upload(&arg::<String>(args, "uploadId")?)?;
            let check = crate::service::import::check(&instance.path, &arg::<String>(args, "target")?, &staged)?;
            ok(json!({ "check": check, "staged": staged }))
        }
        "files_upload_import" => {
            let upload_id: String = arg(args, "uploadId")?;
            let sources: Vec<String> = arg(args, "sources")?;
            crate::service::import::ensure_staged(&upload_id, &sources)?;
            let conflict: Option<crate::service::import::ConflictPolicy> = opt(args, "conflict")?;
            let import_id: Option<String> = opt(args, "importId")?;
            ok(service
                .import_files(
                    &id()?,
                    arg(args, "target")?,
                    sources,
                    conflict.unwrap_or_default(),
                    import_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                )
                .await?)
        }
        "files_upload_discard" => ok(crate::service::import::discard_upload(&arg::<String>(args, "uploadId")?)?),

        // -- worlds --------------------------------------------------------------
        "world_import" => {
            let upload_id: String = arg(args, "uploadId")?;
            let archive = crate::service::world::upload_path(&upload_id)?;
            let skip_backup: Option<bool> = opt(args, "skipBackup")?;
            let result = service.import_world(&id()?, archive.clone(), skip_backup.unwrap_or(false)).await;
            let _ = std::fs::remove_file(&archive);
            ok(result?)
        }

        // -- transfers (each step of a move between machines) ------------------
        "transfer_export" => {
            let allow: Option<bool> = opt(args, "allowPlayers")?;
            let transfer_id: Option<String> = opt(args, "transferId")?;
            ok(service
                .transfer_export(&id()?, &transfer_id.unwrap_or_default(), allow.unwrap_or(false))
                .await?)
        }
        "transfer_abort" => match opt::<String>(args, "exportId")? {
            Some(export_id) => ok(service.transfer_abort(&id()?, &export_id).await?),
            None => ok(service.transfer_abort_by_transfer(&id()?, &arg::<String>(args, "transferId")?).await?),
        },
        "transfer_finalize" => {
            let mode: Option<FinalizeMode> = opt(args, "mode")?;
            ok(service
                .transfer_finalize(
                    &id()?,
                    &arg::<String>(args, "exportId")?,
                    mode.unwrap_or_default(),
                    &arg::<String>(args, "toNodeId")?,
                    &arg::<String>(args, "toNodeName")?,
                    arg(args, "generation")?,
                )
                .await?)
        }
        "transfer_adopt" => {
            let archive = crate::service::world::upload_path(&arg::<String>(args, "uploadId")?)?;
            let keep_both: Option<bool> = opt(args, "keepBoth")?;
            let transfer_id: Option<String> = opt(args, "transferId")?;
            ok(service
                .transfer_adopt(
                    archive,
                    &arg::<String>(args, "sha256")?,
                    keep_both.unwrap_or(false),
                    &transfer_id.unwrap_or_default(),
                )
                .await?)
        }
        "transfer_commit" => ok(service.transfer_commit(&id()?).await?),
        "transfer_discard" => match opt::<String>(args, "instanceId")? {
            Some(instance_id) => ok(service.transfer_discard(&instance_id).await?),
            None => ok(service.transfer_discard_by_transfer(&arg::<String>(args, "transferId")?).await?),
        },
        "instance_force_play" => ok(service.force_play(&id()?).await?),
        "instance_dismiss_fork" => ok(service.dismiss_fork_notice(&id()?).await?),

        // -- backups --------------------------------------------------------------
        "backups_list" => {
            let instance_id = id()?;
            let _ = service.store.get(&instance_id).await?;
            ok(backups::list(&instance_id)?)
        }
        "backup_create" => ok(backups::create(service, &id()?, opt(args, "label")?).await?),
        "backup_restore" => {
            let instance_id = id()?;
            backups::restore(service, &instance_id, &arg::<String>(args, "backupId")?).await?;
            service.after_folder_change(&instance_id).await;
            ok(())
        }
        "backup_delete" => {
            let instance_id = id()?;
            let _ = service.store.get(&instance_id).await?;
            ok(backups::delete(&instance_id, &arg::<String>(args, "backupId")?)?)
        }

        // -- players --------------------------------------------------------------
        "players_online" => ok(service.online_players(&id()?).await),
        "players_lists" => {
            let instance = service.store.get(&id()?).await?;
            let manifest = service.registry.manifest(&instance.game)?;
            ok(players::read_lists(&instance.path, &manifest.players)?)
        }
        "players_write_list" => {
            let instance = service.store.get(&id()?).await?;
            let manifest = service.registry.manifest(&instance.game)?;
            let entries: Vec<PlayerEntry> = arg(args, "entries")?;
            ok(players::write_list(&instance.path, &manifest.players, &arg::<String>(args, "list")?, &entries)?)
        }
        "players_identities" => ok::<HashMap<String, String>>(service.player_identities(&id()?).await?),

        // -- addons ---------------------------------------------------------------
        "addons_context" => ok(service.addon_context(&id()?).await?),
        "addons_list" => ok(service.addons_list(&id()?).await?),
        "addons_browse" => ok(service.addons_browse(&id()?, arg::<BrowseRequest>(args, "request")?).await?),
        "addon_versions" => {
            let all: Option<bool> = opt(args, "all")?;
            ok(service.addon_versions(&id()?, &arg::<String>(args, "projectId")?, all.unwrap_or(false)).await?)
        }
        "addon_dependencies" => ok(service.addon_dependencies(&id()?, &arg::<String>(args, "versionId")?).await?),
        "addon_install_version" => {
            let version: Option<String> = opt(args, "versionId")?;
            let replace: Option<String> = opt(args, "replace")?;
            ok(service
                .addon_install_version(&id()?, &arg::<String>(args, "projectId")?, version.as_deref(), replace.as_deref())
                .await?)
        }
        "addon_update" => ok(service.addon_update(&id()?, &arg::<String>(args, "fileName")?).await?),
        "addon_set_enabled" => {
            ok(service.addon_toggle(&id()?, &arg::<String>(args, "fileName")?, arg(args, "enabled")?).await?)
        }
        "addon_remove" => ok(service.addon_delete(&id()?, &arg::<String>(args, "fileName")?).await?),
        "addons_pending" => ok(service.pending_addons(&id()?).await?),
        "addon_pending_cancel" => {
            ok(service.cancel_pending_addon(&id()?, &arg::<String>(args, "changeId")?).await?)
        }
        "addons_pending_apply" => ok(service.apply_pending_addons(&id()?).await?),

        // -- settings (the node's own) ---------------------------------------------
        "settings_get" => ok(settings_store::load()?),
        "settings_set" => {
            let mut next: AppSettings = arg(args, "settings")?;
            let current = settings_store::load()?;
            // MCP is configured on the machine itself, never from afar.
            next.mcp_enabled = current.mcp_enabled;
            next.mcp_port = current.mcp_port;
            next.mcp_token = current.mcp_token;
            next.mcp_read_only = current.mcp_read_only;
            next.remote_enabled = current.remote_enabled;
            next.remote_port = current.remote_port;
            next.web_panel = current.web_panel;
            settings_store::save(&next)?;
            ok(settings_store::load()?)
        }
        "instance_settings_get" => {
            let instance = service.store.get(&id()?).await?;
            let manifest = service.registry.manifest(&instance.game)?;
            ok(properties::read(&instance.path, &manifest)?)
        }
        "instance_settings_set" => {
            let instance = service.store.get(&id()?).await?;
            let manifest = service.registry.manifest(&instance.game)?;
            let values: BTreeMap<String, String> = arg(args, "values")?;
            ok(properties::write(&instance.path, &manifest, &values)?)
        }
        "java_probe" => ok(java::probe(std::path::Path::new(&arg::<String>(args, "path")?))?),
        "java_runtimes" => {
            let refresh: Option<bool> = opt(args, "refresh")?;
            ok(service.java_runtimes(&id()?, refresh.unwrap_or(false)).await?)
        }
        "storage_locations" => ok(crate::node::health::storage_locations()),
        "java_overview" => {
            let refresh: Option<bool> = opt(args, "refresh")?;
            ok(crate::service::java::java_overview(refresh.unwrap_or(false)).await?)
        }
        "app_log" => {
            let lines: Option<usize> = opt(args, "lines")?;
            ok(crate::logging::tail(lines.unwrap_or(400).min(5000)))
        }
        "java_download" => ok(service.java_download(arg(args, "major")?).await?),
        "upnp_check" => ok(crate::net::upnp::check().await),

        // Commands that act on the local machine's files or desktop.
        "instance_open_folder" | "app_open_log_folder" | "transfer_run" | "transfer_cancel" | "files_import" | "files_import_check" | "mcp_status" | "mcp_configure"
        | "mcp_regenerate_token" | "addons_search" | "addons_installed" | "addon_install" => {
            Err(local_only(command))
        }
        other => Err(AppError::new(ErrorCode::InvalidInput)
            .with("command", other)
            .detail(json!({ "unknown_command": other }).to_string())),
    }
}
