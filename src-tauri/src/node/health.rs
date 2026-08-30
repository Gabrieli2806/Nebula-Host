//! What this machine can tell us about itself.
//!
//! The node is the identity in this app, so its health is header material, not
//! a diagnostics screen: CPU, memory, disk, and whether the things a server
//! needs are actually present.

use serde::{Deserialize, Serialize};
use sysinfo::{Disks, System};

use crate::error::AppResult;
use crate::model::NodeHealth;
use crate::net::lan;
use crate::runtime::{docker, java};

/// One prerequisite, as the first-run check reports it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    /// `java`, `docker`, `port`, `memory`, `disk`.
    pub id: String,
    pub status: RequirementStatus,
    /// Extra facts for the UI to interpolate ("found" → "21.0.5").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub found: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementStatus {
    /// Present and usable.
    Ready,
    /// Absent, but nothing breaks without it.
    Optional,
    /// Absent, and servers cannot run until it is fixed.
    Missing,
}

/// Sample the machine.
pub fn sample(instances_running: u32) -> NodeHealth {
    let mut system = System::new();
    system.refresh_memory();
    system.refresh_cpu_usage();

    let cpu_percent = system.global_cpu_usage();
    let memory_total_mb = system.total_memory() / 1024 / 1024;
    let memory_used_mb = system.used_memory() / 1024 / 1024;

    let disks = Disks::new_with_refreshed_list();
    let (disk_total, disk_available) = disks
        .list()
        .first()
        .map(|disk| (disk.total_space(), disk.available_space()))
        .unwrap_or((0, 0));

    NodeHealth {
        cpu_percent,
        memory_used_mb,
        memory_total_mb,
        disk_used_gb: (disk_total.saturating_sub(disk_available)) / 1024 / 1024 / 1024,
        disk_total_gb: disk_total / 1024 / 1024 / 1024,
        lan_address: lan::primary_address().map(|addr| addr.to_string()),
        instances_running,
    }
}

/// The first-run check: everything that has to be true before a server can
/// start, answered honestly rather than optimistically.
pub fn check_requirements() -> AppResult<Vec<Requirement>> {
    let mut requirements = Vec::new();

    let java = java::resolve_java(None, 21).ok().and_then(|path| java::probe(&path).ok());
    requirements.push(Requirement {
        id: "java".to_string(),
        status: if java.is_some() { RequirementStatus::Ready } else { RequirementStatus::Missing },
        found: java.map(|installation| installation.version),
    });

    requirements.push(Requirement {
        id: "docker".to_string(),
        status: if docker::check_available().is_ok() {
            RequirementStatus::Ready
        } else {
            // Docker is a choice, not a requirement: the default runtime is a
            // plain host process.
            RequirementStatus::Optional
        },
        found: None,
    });

    let health = sample(0);
    requirements.push(Requirement {
        id: "memory".to_string(),
        status: if health.memory_total_mb >= 4096 {
            RequirementStatus::Ready
        } else {
            RequirementStatus::Missing
        },
        found: Some(format!("{} MB", health.memory_total_mb)),
    });

    requirements.push(Requirement {
        id: "disk".to_string(),
        status: if health.disk_total_gb.saturating_sub(health.disk_used_gb) >= 10 {
            RequirementStatus::Ready
        } else {
            RequirementStatus::Missing
        },
        found: Some(format!("{} GB free", health.disk_total_gb.saturating_sub(health.disk_used_gb))),
    });

    Ok(requirements)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_reports_real_memory() {
        let health = sample(0);
        assert!(health.memory_total_mb > 0, "the machine must report some memory");
        assert!(health.memory_used_mb <= health.memory_total_mb);
    }

    #[test]
    fn the_running_count_is_passed_through() {
        assert_eq!(sample(3).instances_running, 3);
    }

    #[test]
    fn docker_is_never_a_hard_requirement() {
        let requirements = check_requirements().expect("check runs");
        let docker = requirements.iter().find(|r| r.id == "docker").expect("docker is checked");
        assert_ne!(
            docker.status,
            RequirementStatus::Missing,
            "the default runtime is a host process; Docker must stay optional"
        );
    }

    #[test]
    fn every_requirement_has_a_stable_id() {
        let requirements = check_requirements().expect("check runs");
        for id in ["java", "docker", "memory", "disk"] {
            assert!(requirements.iter().any(|r| r.id == id), "missing requirement: {id}");
        }
    }
}
