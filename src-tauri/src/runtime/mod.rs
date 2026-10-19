//! Running a server. Nothing here knows which game it is running: it is handed
//! a `LaunchPlan` and a compiled `ConsoleParser` and does the same work either
//! way.

pub mod adopt;
#[cfg(target_os = "android")]
pub mod android_java;
pub mod console;
pub mod docker;
pub mod java;
pub mod process;

pub use console::{ConsoleParser, LineReading};
pub use process::{ProcessHandle, ProcessSupervisor};
