//! The layer between the IPC surface and the machinery: it owns the rules
//! ("a clean stop with wake-on-join means sleep, not offline") so that both
//! the desktop UI and, later, a paired phone get the same behaviour.

pub mod backups;
pub mod files;
pub mod instances;
pub mod players;
pub mod poller;
pub mod properties;

pub use instances::{AppEvent, CreateInstanceRequest, InstanceService};
