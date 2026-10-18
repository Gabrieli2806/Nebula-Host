//! The layer between the IPC surface and the machinery: it owns the rules
//! ("a clean stop with wake-on-join means sleep, not offline") so that both
//! the desktop UI and, later, a paired phone get the same behaviour.

pub mod addons;
pub mod backups;
pub mod completion;
pub mod files;
pub mod icon;
pub mod import;
pub mod instances;
pub mod java;
pub mod logs;
pub mod pending;
pub mod players;
pub mod poller;
pub mod properties;

pub use instances::{AppEvent, CreateInstanceRequest, InstancePatch, InstanceService};
