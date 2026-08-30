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
pub mod node;
pub mod players;
pub mod settings;

use std::sync::Arc;

use crate::service::InstanceService;

/// What every command reaches for.
pub type State<'a> = tauri::State<'a, Arc<InstanceService>>;

/// Register every command with the Tauri builder.
///
/// One list, in one place: a command that is not here does not exist, and
/// nothing else in the codebase has to know the whole set.
#[macro_export]
macro_rules! ipc_handlers {
    () => {
        tauri::generate_handler![
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
            $crate::ipc::instances::instance_start,
            $crate::ipc::instances::instance_stop,
            $crate::ipc::instances::instance_restart,
            $crate::ipc::instances::instance_sleep,
            $crate::ipc::instances::instance_delete,
            $crate::ipc::instances::instance_command,
            $crate::ipc::instances::instance_console,
            $crate::ipc::instances::instance_update,
            $crate::ipc::instances::instance_forward_port,
            $crate::ipc::instances::instance_release_port,
            // files
            $crate::ipc::files::files_list,
            $crate::ipc::files::file_read,
            $crate::ipc::files::file_write,
            $crate::ipc::files::file_delete,
            $crate::ipc::files::file_rename,
            $crate::ipc::files::folder_create,
            $crate::ipc::files::instance_reveal,
            // backups
            $crate::ipc::backups::backups_list,
            $crate::ipc::backups::backup_create,
            $crate::ipc::backups::backup_restore,
            $crate::ipc::backups::backup_delete,
            // players
            $crate::ipc::players::players_online,
            $crate::ipc::players::players_lists,
            $crate::ipc::players::players_write_list,
            // addons
            $crate::ipc::addons::addons_search,
            $crate::ipc::addons::addons_installed,
            $crate::ipc::addons::addon_install,
            $crate::ipc::addons::addon_set_enabled,
            $crate::ipc::addons::addon_remove,
            // settings
            $crate::ipc::settings::settings_get,
            $crate::ipc::settings::settings_set,
            $crate::ipc::settings::instance_settings_get,
            $crate::ipc::settings::instance_settings_set,
            $crate::ipc::settings::java_probe,
        ]
    };
}
