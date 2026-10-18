//! Everything that touches disk: settings, instance records, and the path
//! rules that keep user-supplied filenames where they belong.

pub mod devices;
pub mod instances;
pub mod nodes;
pub mod paths;
pub mod settings;

pub use instances::InstanceStore;
