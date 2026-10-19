//! Moving a server to another machine.

use crate::error::AppResult;
use crate::ipc::State;
use crate::model::Instance;
use crate::remote::transfer::{self, TransferOutcome, TransferRequest};

/// Run a whole transfer from this desktop (source, target, or relay between
/// two nodes). Progress arrives as `transfer` events on this machine's
/// channel. Local only: the request carries this desktop's node tokens.
#[tauri::command]
pub async fn transfer_run(state: State<'_>, request: TransferRequest) -> AppResult<TransferOutcome> {
    transfer::run(state.inner().clone(), request).await
}

/// Call off a transfer this desktop is running. It stops at the next safe
/// point and leaves the server where it was; false when it is not running
/// any more or has passed the point of no return.
#[tauri::command]
pub fn transfer_cancel(transfer_id: String) -> bool {
    transfer::cancel(&transfer_id)
}

/// Play an older copy left behind by a transfer anyway. It forks: from now
/// on this copy and the one on the other machine are different servers.
#[tauri::command]
pub async fn instance_force_play(state: State<'_>, instance_id: String) -> AppResult<Instance> {
    state.force_play(&instance_id).await
}

/// Stop showing the "this copy split from the one on X" notice.
#[tauri::command]
pub async fn instance_dismiss_fork(state: State<'_>, instance_id: String) -> AppResult<Instance> {
    state.dismiss_fork_notice(&instance_id).await
}
