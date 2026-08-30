//! Starting, watching and stopping server processes.
//!
//! Game-agnostic on purpose: it is handed a [`LaunchPlan`] and a compiled
//! [`ConsoleParser`] and does the same work for Minecraft, Valheim or anything
//! else. Everything it learns about the server it learns from those two.

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock as StdRwLock};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, RwLock};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::registry::LaunchPlan;
use crate::model::{ConsoleLevel, ConsoleLine, InstanceStatus, RuntimeKind};
use crate::runtime::console::{read_line_lossy, ConsoleParser};
use crate::runtime::docker;
use crate::runtime::java::configure_no_window_command;

/// How long output stays hidden after the app issues a command of its own.
/// Long enough to cover the server's reply, short enough that a command the
/// user types a moment later is still shown.
const SILENCE_WINDOW: Duration = Duration::from_secs(2);

/// Something the runtime noticed. The supervisor never decides what a status
/// *means* — it reports, and the instance service decides.
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    Status(String, InstanceStatus),
    PlayerJoined(String, String),
    PlayerLeft(String, String),
    TickRate(String, f32),
    /// The process ended. `expected` is false for a crash.
    Exited { instance_id: String, expected: bool, code: Option<i32> },
}

/// Everything needed to start one server.
pub struct StartRequest {
    pub instance_id: String,
    pub instance_name: String,
    pub plan: LaunchPlan,
    pub runtime_kind: RuntimeKind,
    pub parser: Arc<ConsoleParser>,
    pub stop_command: String,
    /// Container image, when `runtime_kind` is Docker.
    pub container_image: Option<String>,
    /// Port to publish from the container.
    pub port: u16,
}

/// A running server.
pub struct ProcessHandle {
    stdin: Option<std::process::ChildStdin>,
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    stop_requested: Arc<AtomicBool>,
    runtime_kind: RuntimeKind,
    container_name: String,
}

/// Measured CPU and memory for a running process, refreshed by the poller.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessResources {
    pub cpu_percent: f32,
    pub memory_used_mb: u64,
}

#[derive(Default)]
pub struct ProcessSupervisor {
    processes: Arc<RwLock<HashMap<String, ProcessHandle>>>,
    tick_rates: Arc<StdRwLock<HashMap<String, f32>>>,
    resources: Arc<StdRwLock<HashMap<String, ProcessResources>>>,
    /// Per-instance deadline until which console output is swallowed, set when
    /// the app issues a background command. Commands the user typed set none.
    silence_until: Arc<StdRwLock<HashMap<String, Instant>>>,
}

impl ProcessSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn start(
        &self,
        request: StartRequest,
        console_tx: mpsc::Sender<ConsoleLine>,
        events_tx: mpsc::Sender<RuntimeEvent>,
    ) -> AppResult<()> {
        if self.is_running(&request.instance_id).await {
            return Err(AppError::new(ErrorCode::InstanceAlreadyRunning)
                .with("instance", &request.instance_id));
        }

        let container_name = docker::container_name(&request.instance_id, &request.instance_name);
        let mut child = spawn(&request, &container_name)?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let pid = child.id();
        let stop_requested = Arc::new(AtomicBool::new(false));

        self.processes.write().await.insert(
            request.instance_id.clone(),
            ProcessHandle {
                stdin,
                pid,
                started_at: Utc::now(),
                stop_requested: stop_requested.clone(),
                runtime_kind: request.runtime_kind,
                container_name,
            },
        );

        let _ = events_tx
            .send(RuntimeEvent::Status(request.instance_id.clone(), InstanceStatus::Starting))
            .await;

        if let Some(stdout) = stdout {
            self.pump_stdout(&request, stdout, console_tx.clone(), events_tx.clone());
        }
        if let Some(stderr) = stderr {
            pump_stderr(request.instance_id.clone(), stderr, console_tx.clone());
        }

        self.watch_exit(request.instance_id.clone(), child, stop_requested, events_tx);
        Ok(())
    }

    /// Read stdout on a blocking thread. `read_line` on a child's pipe cannot
    /// be polled, and a dedicated thread per server is cheaper than the async
    /// gymnastics that would avoid it.
    fn pump_stdout(
        &self,
        request: &StartRequest,
        stdout: std::process::ChildStdout,
        console_tx: mpsc::Sender<ConsoleLine>,
        events_tx: mpsc::Sender<RuntimeEvent>,
    ) {
        let instance_id = request.instance_id.clone();
        let parser = request.parser.clone();
        let tick_rates = self.tick_rates.clone();
        let silence_until = self.silence_until.clone();

        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();

            while let Some(line) = read_line_lossy(&mut reader, &mut buf) {
                let reading = parser.read(&line);

                if reading.ready {
                    let _ = events_tx
                        .blocking_send(RuntimeEvent::Status(instance_id.clone(), InstanceStatus::Online));
                }
                if let Some(name) = &reading.joined {
                    let _ = events_tx
                        .blocking_send(RuntimeEvent::PlayerJoined(instance_id.clone(), name.clone()));
                }
                if let Some(name) = &reading.left {
                    let _ = events_tx
                        .blocking_send(RuntimeEvent::PlayerLeft(instance_id.clone(), name.clone()));
                }
                if let Some(rate) = reading.tick_rate {
                    if let Ok(mut map) = tick_rates.write() {
                        map.insert(instance_id.clone(), rate);
                    }
                    let _ = events_tx.blocking_send(RuntimeEvent::TickRate(instance_id.clone(), rate));
                }

                let silenced = silence_until
                    .read()
                    .ok()
                    .and_then(|map| map.get(&instance_id).copied())
                    .is_some_and(|deadline| Instant::now() < deadline);

                if reading.visible && !(silenced && reading.tick_rate.is_some()) {
                    let _ = console_tx.blocking_send(ConsoleLine {
                        timestamp: Utc::now(),
                        level: reading.level,
                        message: reading.text,
                    });
                }
            }
        });
    }

    /// Reap the child and report why it ended.
    fn watch_exit(
        &self,
        instance_id: String,
        mut child: Child,
        stop_requested: Arc<AtomicBool>,
        events_tx: mpsc::Sender<RuntimeEvent>,
    ) {
        let processes = self.processes.clone();
        let tick_rates = self.tick_rates.clone();
        let resources = self.resources.clone();

        thread::spawn(move || {
            let status = child.wait();
            let code = status.as_ref().ok().and_then(|s| s.code());
            // A non-zero exit we asked for is not a crash; a zero exit we did
            // not ask for is still an unexpected stop.
            let expected = stop_requested.load(Ordering::SeqCst);

            if let Ok(mut map) = tick_rates.write() {
                map.remove(&instance_id);
            }
            if let Ok(mut map) = resources.write() {
                map.remove(&instance_id);
            }

            let cleanup_id = instance_id.clone();
            tokio::runtime::Handle::try_current().ok().map(|handle| {
                handle.spawn(async move {
                    processes.write().await.remove(&cleanup_id);
                })
            });

            let _ = events_tx.blocking_send(RuntimeEvent::Exited { instance_id, expected, code });
        });
    }

    /// Ask the server to stop the way its manifest says, then wait.
    pub async fn stop(&self, instance_id: &str, stop_command: &str) -> AppResult<()> {
        let mut processes = self.processes.write().await;
        let handle = processes
            .get_mut(instance_id)
            .ok_or_else(|| AppError::new(ErrorCode::InstanceNotRunning).with("instance", instance_id))?;

        handle.stop_requested.store(true, Ordering::SeqCst);

        match handle.runtime_kind {
            RuntimeKind::Docker => docker::stop(&handle.container_name)?,
            RuntimeKind::HostProcess => {
                if let Some(stdin) = handle.stdin.as_mut() {
                    let _ = writeln!(stdin, "{stop_command}");
                    let _ = stdin.flush();
                }
            }
        }

        Ok(())
    }

    /// Send a command the user typed.
    pub async fn send_command(&self, instance_id: &str, command: &str) -> AppResult<()> {
        self.write_command(instance_id, command, false).await
    }

    /// Send a command the app issued; its answer stays out of the console.
    pub async fn send_internal_command(&self, instance_id: &str, command: &str) -> AppResult<()> {
        self.write_command(instance_id, command, true).await
    }

    async fn write_command(&self, instance_id: &str, command: &str, silent: bool) -> AppResult<()> {
        if silent {
            if let Ok(mut map) = self.silence_until.write() {
                map.insert(instance_id.to_string(), Instant::now() + SILENCE_WINDOW);
            }
        }

        let mut processes = self.processes.write().await;
        let handle = processes
            .get_mut(instance_id)
            .ok_or_else(|| AppError::new(ErrorCode::InstanceNotRunning).with("instance", instance_id))?;

        let stdin = handle
            .stdin
            .as_mut()
            .ok_or_else(|| AppError::new(ErrorCode::InstanceNotRunning).with("instance", instance_id))?;

        writeln!(stdin, "{command}")?;
        stdin.flush()?;
        Ok(())
    }

    pub async fn is_running(&self, instance_id: &str) -> bool {
        self.processes.read().await.contains_key(instance_id)
    }

    pub async fn uptime_seconds(&self, instance_id: &str) -> Option<u64> {
        let processes = self.processes.read().await;
        let handle = processes.get(instance_id)?;
        Utc::now().signed_duration_since(handle.started_at).num_seconds().try_into().ok()
    }

    pub async fn pid(&self, instance_id: &str) -> Option<u32> {
        self.processes.read().await.get(instance_id).map(|handle| handle.pid)
    }

    pub async fn running_pids(&self) -> Vec<(String, u32)> {
        self.processes
            .read()
            .await
            .iter()
            .map(|(id, handle)| (id.clone(), handle.pid))
            .collect()
    }

    pub fn tick_rate(&self, instance_id: &str) -> Option<f32> {
        self.tick_rates.read().ok()?.get(instance_id).copied()
    }

    pub fn resources(&self, instance_id: &str) -> ProcessResources {
        self.resources
            .read()
            .ok()
            .and_then(|map| map.get(instance_id).copied())
            .unwrap_or_default()
    }

    pub fn set_resources(&self, instance_id: &str, resources: ProcessResources) {
        if let Ok(mut map) = self.resources.write() {
            map.insert(instance_id.to_string(), resources);
        }
    }

    /// Stop everything, for app shutdown.
    pub async fn stop_all(&self, stop_command: &str) {
        let ids: Vec<String> = self.processes.read().await.keys().cloned().collect();
        for id in ids {
            let _ = self.stop(&id, stop_command).await;
        }
    }
}

fn spawn(request: &StartRequest, container_name: &str) -> AppResult<Child> {
    let mut command = match request.runtime_kind {
        RuntimeKind::HostProcess => {
            let mut command = Command::new(&request.plan.program);
            command.args(&request.plan.args).current_dir(&request.plan.working_dir);
            for (key, value) in &request.plan.env {
                command.env(key, value);
            }
            command
        }
        RuntimeKind::Docker => docker::command_for(request, container_name)?,
    };

    configure_no_window_command(&mut command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            let code = if error.kind() == std::io::ErrorKind::NotFound {
                ErrorCode::JavaNotFound
            } else {
                ErrorCode::InstanceStartFailed
            };
            AppError::new(code).with("program", &request.plan.program).detail(error)
        })
}

fn pump_stderr(
    _instance_id: String,
    stderr: std::process::ChildStderr,
    console_tx: mpsc::Sender<ConsoleLine>,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buf = Vec::new();
        while let Some(line) = read_line_lossy(&mut reader, &mut buf) {
            let _ = console_tx.blocking_send(ConsoleLine {
                timestamp: Utc::now(),
                level: ConsoleLevel::Error,
                message: line,
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stopping_something_that_is_not_running_is_a_typed_error() {
        let supervisor = ProcessSupervisor::new();
        let err = supervisor.stop("missing", "stop").await.expect_err("nothing running");
        assert_eq!(err.code, ErrorCode::InstanceNotRunning);
    }

    #[tokio::test]
    async fn commands_to_a_stopped_server_are_refused() {
        let supervisor = ProcessSupervisor::new();
        let err = supervisor.send_command("missing", "say hi").await.expect_err("not running");
        assert_eq!(err.code, ErrorCode::InstanceNotRunning);
    }

    #[test]
    fn resources_default_to_zero_for_unknown_instances() {
        let supervisor = ProcessSupervisor::new();
        let resources = supervisor.resources("missing");
        assert_eq!(resources.cpu_percent, 0.0);
        assert_eq!(resources.memory_used_mb, 0);
    }

    #[test]
    fn tick_rate_is_absent_until_one_is_reported() {
        let supervisor = ProcessSupervisor::new();
        assert_eq!(supervisor.tick_rate("missing"), None);
    }
}
