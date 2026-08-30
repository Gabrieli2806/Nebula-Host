//! The background tick: resource usage, tick-rate polling, sleep-when-empty,
//! and schedules.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Timelike, Utc};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
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

    for (instance_id, pid) in running {
        if let Some(process) = system.process(Pid::from_u32(pid)) {
            service.processes.set_resources(
                &instance_id,
                ProcessResources {
                    cpu_percent: process.cpu_usage(),
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
