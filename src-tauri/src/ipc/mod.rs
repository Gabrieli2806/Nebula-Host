//! The IPC surface.
//!
//! Commands are thin on purpose: they validate what came from the UI, call a
//! service, and hand back typed data or a typed [`AppError`]. No business
//! rules live here, because the mobile app will not come through this door.

pub mod addons;
pub mod backups;
pub mod files;
pub mod games;
pub mod instances;
pub mod launcher;
pub mod mcp;
pub mod node;
pub mod players;
pub mod remote;
pub mod settings;
pub mod transfer;

use std::sync::Arc;

use crate::service::InstanceService;

/// What every command reaches for.
pub type State<'a> = tauri::State<'a, Arc<InstanceService>>;

/// Open a folder on this PC in the system file manager.
///
/// Goes through the opener plugin's Rust API, which is not bound by the
/// webview's path scope (that scope cannot know where the user keeps their
/// instances).
pub fn open_folder(app: &tauri::AppHandle, folder: &std::path::Path) -> crate::error::AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(folder.to_string_lossy(), None::<&str>)
        .map_err(|error| {
            crate::error::AppError::new(crate::error::ErrorCode::IoFailed)
                .with("path", folder.display().to_string())
                .detail(error.to_string())
        })
}

/// Register every command with the Tauri builder.
///
/// One list, in one place: a command that is not here does not exist, and
/// nothing else in the codebase has to know the whole set.
#[macro_export]
macro_rules! ipc_handlers {
    () => {
        tauri::generate_handler![
            $crate::ipc::launcher::launcher_capabilities,
            $crate::ipc::launcher::launcher_pin_shortcut,
            $crate::ipc::launcher::launcher_take_shortcut,
            $crate::ipc::launcher::launcher_status,
            $crate::ipc::launcher::launcher_client_start,
            $crate::ipc::launcher::launcher_loader_versions,
            $crate::ipc::launcher::launcher_client_stop,
            $crate::ipc::launcher::launcher_client_resume,
            $crate::ipc::launcher::launcher_client_logs,
            $crate::ipc::launcher::launcher_profiles,
            $crate::ipc::launcher::launcher_create,
            $crate::ipc::launcher::launcher_remove,
            $crate::ipc::launcher::launcher_versions,
            $crate::ipc::launcher::launcher_search,
            $crate::ipc::launcher::launcher_content_list,
            $crate::ipc::launcher::launcher_content_icon,
            $crate::ipc::launcher::launcher_content_import,
            $crate::ipc::launcher::launcher_content_install,
            $crate::ipc::launcher::launcher_content_remove,
            $crate::ipc::launcher::launcher_content_set_enabled,
            $crate::ipc::launcher::launcher_modpack_install,
            $crate::ipc::launcher::launcher_login_begin,
            $crate::ipc::launcher::launcher_login_poll,
            $crate::ipc::launcher::launcher_login_cancel,
            $crate::ipc::launcher::launcher_accounts,
            $crate::ipc::launcher::launcher_logout,
            $crate::ipc::launcher::launcher_login_wait,
            $crate::ipc::launcher::launcher_skin,
            $crate::ipc::launcher::launcher_set_skin,
            $crate::ipc::launcher::launcher_set_cape,
            $crate::ipc::launcher::launcher_set_memory,
            $crate::ipc::launcher::launcher_update_details,
            $crate::ipc::launcher::launcher_log_share,
            // node
            $crate::ipc::node::node_get,
            $crate::ipc::node::node_rename,
            $crate::ipc::node::node_health,
            $crate::ipc::node::node_requirements,
            $crate::ipc::node::node_pairing_code,
            // games
            $crate::ipc::games::games_list,
            $crate::ipc::games::game_versions,
            $crate::ipc::games::game_loader_versions,
            // instances
            $crate::ipc::instances::instances_list,
            $crate::ipc::instances::instance_get,
            $crate::ipc::instances::instance_create,
            $crate::ipc::instances::instance_install,
            $crate::ipc::instances::instance_accept_agreement,
            $crate::ipc::instances::instance_start,
            $crate::ipc::instances::instance_stop,
            $crate::ipc::instances::instance_restart,
            $crate::ipc::instances::instance_sleep,
            $crate::ipc::instances::instance_delete,
            $crate::ipc::instances::instance_command,
            $crate::ipc::instances::instance_console,
            $crate::ipc::instances::console_completions,
            $crate::ipc::instances::instance_update,
            $crate::ipc::instances::instance_forward_port,
            $crate::ipc::instances::instance_release_port,
            $crate::ipc::instances::instance_icon_set,
            $crate::ipc::instances::instance_icon_clear,
            $crate::ipc::instances::instance_log_files,
            $crate::ipc::instances::instance_log_share,
            // files
            $crate::ipc::files::files_list,
            $crate::ipc::files::file_read,
            $crate::ipc::files::file_write,
            $crate::ipc::files::file_delete,
            $crate::ipc::files::file_rename,
            $crate::ipc::files::folder_create,
            $crate::ipc::files::instance_open_folder,
            $crate::ipc::files::files_import_check,
            $crate::ipc::files::files_import,
            $crate::ipc::files::instance_disk_usage,
            $crate::ipc::files::world_import,
            $crate::ipc::files::upload_to_node,
            // mcp
            $crate::ipc::mcp::mcp_status,
            $crate::ipc::mcp::mcp_configure,
            $crate::ipc::mcp::mcp_regenerate_token,
            // remote + upnp
            $crate::ipc::remote::remote_status,
            $crate::ipc::remote::remote_configure,
            $crate::ipc::remote::node_forget_device,
            $crate::ipc::remote::upnp_check,
            // backups
            $crate::ipc::backups::backups_list,
            $crate::ipc::backups::backup_create,
            $crate::ipc::backups::backup_restore,
            $crate::ipc::backups::backup_delete,
            // players
            $crate::ipc::players::players_online,
            $crate::ipc::players::players_lists,
            $crate::ipc::players::players_write_list,
            $crate::ipc::players::players_identities,
            // addons
            $crate::ipc::addons::addons_search,
            $crate::ipc::addons::addons_installed,
            $crate::ipc::addons::addon_install,
            $crate::ipc::addons::addon_set_enabled,
            $crate::ipc::addons::addon_remove,
            $crate::ipc::addons::addons_context,
            $crate::ipc::addons::addons_list,
            $crate::ipc::addons::addons_browse,
            $crate::ipc::addons::addon_versions,
            $crate::ipc::addons::addon_dependencies,
            $crate::ipc::addons::addon_install_version,
            $crate::ipc::addons::addon_update,
            $crate::ipc::addons::addons_pending,
            $crate::ipc::addons::addon_pending_cancel,
            $crate::ipc::addons::addons_pending_apply,
            // settings
            $crate::ipc::settings::settings_get,
            $crate::ipc::settings::settings_set,
            $crate::ipc::settings::instance_settings_get,
            $crate::ipc::settings::instance_settings_set,
            $crate::ipc::settings::java_probe,
            $crate::ipc::settings::java_runtimes,
            $crate::ipc::settings::java_download,
            $crate::ipc::settings::java_overview,
            $crate::ipc::settings::storage_locations,
            $crate::ipc::settings::app_log,
            $crate::ipc::settings::app_open_log_folder,
            // transfers
            $crate::ipc::transfer::transfer_run,
            $crate::ipc::transfer::transfer_cancel,
            $crate::ipc::transfer::instance_force_play,
            $crate::ipc::transfer::instance_dismiss_fork,
        ]
    };
}
