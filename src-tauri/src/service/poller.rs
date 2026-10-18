//! The background tick: resource usage, tick-rate polling, sleep-when-empty,
//! and schedules.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Timelike, Utc};
use sysinfo::{CpuRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tracing::warn;

use crate::model::{InstanceStatus, ScheduleAction};
use crate::runtime::process::ProcessResources;
use crate::service::instances::{AppEvent, InstanceService};

const TICK: Duration = Duration::from_secs(3);
/// Ask for the tick rate every few ticks; it is a console command, not a free
/// reading, so polling it every three seconds would spam the server.
const TICK_RATE_EVERY: u32 = 5;

/// Start the background loop. Runs for the life of the app.
pub fn spawn(service: Arc<InstanceService>) {
    tokio::spawn(async move {
        let mut system = System::new();
        let mut tick: u32 = 0;
        let mut last_schedule_minute: Option<u32> = None;

        loop {
            tokio::time::sleep(TICK).await;
            tick = tick.wrapping_add(1);

            refresh_resources(&service, &mut system).await;
            publish_states(&service).await;

            if tick % TICK_RATE_EVERY == 0 {
                poll_tick_rates(&service).await;
            }

            apply_sleep_when_empty(&service).await;

            // Schedules fire on the minute, once.
            let now = Utc::now();
            let minute = now.minute();
            if last_schedule_minute != Some(minute) {
                last_schedule_minute = Some(minute);
                run_due_schedules(&service).await;
                run_auto_backups(&service).await;
            }
        }
    });
}

async fn refresh_resources(service: &Arc<InstanceService>, system: &mut System) {
    let running = service.processes.running_pids().await;
    if running.is_empty() {
        return;
    }

    let pids: Vec<Pid> = running.iter().map(|(_, pid)| Pid::from_u32(*pid)).collect();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );

    // sysinfo reports per-core percent (400% = four cores busy). The UI shows
    // a share of the whole machine, the same scale as the node's own CPU.
    // Divide by the CPU count sysinfo itself multiplied by (not
    // available_parallelism, which follows the affinity mask and, past 64
    // logical CPUs on Windows, counts only one processor group).
    if system.cpus().is_empty() {
        system.refresh_cpu_list(CpuRefreshKind::nothing());
    }
    let cores = system.cpus().len().max(1) as f32;

    for (instance_id, pid) in running {
        if let Some(process) = system.process(Pid::from_u32(pid)) {
            service.processes.set_resources(
                &instance_id,
                ProcessResources {
                    cpu_percent: (process.cpu_usage() / cores).clamp(0.0, 100.0),
                    memory_used_mb: process.memory() / 1024 / 1024,
                },
            );
        }
    }
}

/// Publish the numbers.
///
/// Deliberately does **not** derive status from "is a process running?". A
/// sleeping server has no process, and a poller that inferred status would
/// flip it back to offline every three seconds — which is exactly the bug this
/// shape avoids.
async fn publish_states(service: &Arc<InstanceService>) {
    for view in service.list().await.unwrap_or_default() {
        let id = &view.instance.id;
        let mut state = service.state_of(id).await;

        if state.status.expects_process() {
            let resources = service.processes.resources(id);
            state.cpu_percent = resources.cpu_percent;
            state.memory_used_mb = resources.memory_used_mb;
            state.uptime_seconds = service.processes.uptime_seconds(id).await;
            state.tick_rate = service.processes.tick_rate(id);
            state.players_online = service.online_players(id).await.len() as u32;
        }

        service.publish_state(id, state).await;
    }
}

async fn poll_tick_rates(service: &Arc<InstanceService>) {
    for view in service.list().await.unwrap_or_default() {
        if view.state.status != InstanceStatus::Online {
            continue;
        }
        let Ok(manifest) = service.registry.manifest(&view.instance.game) else { continue };
        let command = manifest
            .distribution(&view.instance.build.distribution)
            .and_then(|dist| dist.tick_rate_command.clone());
        let Some(command) = command else { continue };

        // Silenced: the user did not ask for this, so its answer stays out of
        // their console.
        let _ = service.processes.send_internal_command(&view.instance.id, &command).await;
    }
}

/// "Sleep when empty": stop a server that has had nobody on it for a while.
async fn apply_sleep_when_empty(service: &Arc<InstanceService>) {
    let now = Utc::now();

    for view in service.list().await.unwrap_or_default() {
        let automation = &view.instance.automation;
        if !automation.sleep_when_empty || view.state.status != InstanceStatus::Online {
            continue;
        }
        if view.state.players_online > 0 {
            continue;
        }
        let Some(since) = view.state.last_player_seen_at else { continue };

        let idle_minutes = now.signed_duration_since(since).num_minutes();
        if idle_minutes < i64::from(automation.sleep_after_minutes) {
            continue;
        }

        if let Err(error) = service.stop(&view.instance.id).await {
            warn!(instance_id = %view.instance.id, %error, "sleep-when-empty could not stop the server");
        }
    }
}

/// Servers whose automatic backup is running right now (one at a time each).
fn auto_in_flight() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
    SET.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// "Back up every N hours, keep K": take one when due, then drop the oldest
/// automatic backups beyond K. A stopped server that has not run since its
/// last automatic backup is skipped — its world has not changed.
async fn run_auto_backups(service: &Arc<InstanceService>) {
    let now = Utc::now();
    for view in service.list().await.unwrap_or_default() {
        let automation = &view.instance.automation;
        if automation.auto_backup_hours == 0 || view.instance.install.is_none() {
            continue;
        }
        let id = view.instance.id.clone();
        let last = super::backups::last_auto(&id);
        let due = last.is_none_or(|at| now.signed_duration_since(at).num_minutes() >= i64::from(automation.auto_backup_hours) * 60);
        if !due {
            continue;
        }
        let running = matches!(view.state.status, InstanceStatus::Online | InstanceStatus::Starting | InstanceStatus::Stopping);
        let ran_since = match (last, view.instance.last_started_at) {
            (Some(backup), Some(started)) => started > backup,
            (None, _) => true,
            (Some(_), None) => false,
        };
        if !running && !ran_since {
            continue;
        }
        if view.state.status == InstanceStatus::Starting || view.state.status == InstanceStatus::Stopping {
            continue; // try again next minute, not mid-transition
        }
        if service.ensure_not_transferring(&id).await.is_err() {
            continue;
        }
        if !auto_in_flight().lock().map(|mut set| set.insert(id.clone())).unwrap_or(false) {
            continue;
        }
        let keep = automation.auto_backup_keep.max(1) as usize;
        let service = service.clone();
        tokio::spawn(async move {
            match super::backups::create_auto(&service, &id).await {
                Ok(backup) => {
                    let removed = super::backups::prune_auto(&id, keep).unwrap_or(0);
                    tracing::info!(instance_id = %id, backup = %backup.id, removed, "automatic backup taken");
                }
                Err(error) => warn!(instance_id = %id, %error, "automatic backup failed"),
            }
            if let Ok(mut set) = auto_in_flight().lock() {
                set.remove(&id);
            }
        });
    }
}

async fn run_due_schedules(service: &Arc<InstanceService>) {
    let now = chrono::Local::now();
    let today = now.format("%w").to_string().parse::<u8>().unwrap_or(0);
    let time = now.format("%H:%M").to_string();

    for view in service.list().await.unwrap_or_default() {
        for rule in &view.instance.automation.schedules {
            if !rule.enabled || rule.time != time || !rule.days.contains(&today) {
                continue;
            }

            let id = view.instance.id.clone();
            let result = match rule.action {
                ScheduleAction::Start => service.start(&id).await,
                ScheduleAction::Stop => service.stop(&id).await,
                ScheduleAction::Restart => service.restart(&id).await,
                ScheduleAction::Backup => {
                    crate::service::backups::create(service, &id, None).await.map(|_| ())
                }
            };

            if let Err(error) = result {
                warn!(instance_id = %id, ?rule.action, %error, "scheduled action failed");
            }
        }
    }
}

/// Re-export so callers do not need to know the event type's home module.
pub type Event = AppEvent;
