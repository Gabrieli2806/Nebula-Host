//! Starting, watching and stopping server processes.
//!
//! Game-agnostic on purpose: it is handed a [`LaunchPlan`] and a compiled
//! [`ConsoleParser`] and does the same work for Minecraft, Valheim or anything
//! else. Everything it learns about the server it learns from those two.

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use regex::Regex;
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
    /// `(instance, name, uuid)`: the server said who a connecting player is.
    PlayerIdentified(String, String, String),
    TickRate(String, f32),
    /// The process ended. `expected` is false for a crash.
    Exited { instance_id: String, expected: bool, code: Option<i32> },
}

/// What the server's own output said about how it is ending, read by the
/// reaper to tell a stop from a crash.
#[derive(Default)]
struct ExitHints {
    /// The server logged its normal shutdown (a player's `/stop`, say).
    stopping: AtomicBool,
    /// The server logged something only a crash produces.
    crashed: AtomicBool,
    /// stdout reached its end, so nothing more will be learned from it.
    output_closed: AtomicBool,
}

/// Was this exit a stop rather than a crash?
///
/// A stop Nebula asked for is always a stop. Otherwise the server's own word
/// decides: a crash line makes it a crash whatever the code (a crashing
/// Minecraft still logs its shutdown, and can exit with 0); a clean exit code
/// or a logged normal shutdown makes it a stop — that is a player's `/stop`,
/// or `stop` typed in the console. Anything else ended without warning.
pub fn exit_was_expected(requested: bool, code: Option<i32>, stopping_logged: bool, crash_logged: bool) -> bool {
    if requested {
        return true;
    }
    if crash_logged {
        return false;
    }
    code == Some(0) || stopping_logged
}

/// How long the reaper waits for the last of stdout after the process ends,
/// so a shutdown line still in the pipe is not missed.
const OUTPUT_DRAIN: Duration = Duration::from_secs(2);

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
    /// Active output captures, at most one per instance. See [`Capture`].
    captures: Arc<StdMutex<HashMap<String, CaptureSink>>>,
}

/// Where captured lines go while a [`Capture`] is open.
struct CaptureSink {
    matches: Arc<Vec<Regex>>,
    tx: mpsc::UnboundedSender<String>,
    /// Shared with the [`Capture`]; set when the user's own command closed
    /// it, and used to tell this sink from a later one for the same server.
    interrupted: Arc<AtomicBool>,
}

/// An open capture of one server's output.
///
/// While it is open, every stdout line matching one of its patterns is handed
/// to the capture *instead of* the console. That is how the app asks the
/// server something (`help`) without the answer landing in front of the user.
/// Lines that do not match flow to the console as usual, so a player joining
/// mid-capture is never lost. Dropping the capture closes it.
///
/// A command the user types closes any open capture at once: from then on
/// the output is the user's answer and must reach the console, even if it
/// looks like the one the app was waiting for (they may have typed `help`).
/// The capture then reports [`Capture::interrupted`].
pub struct Capture {
    instance_id: String,
    captures: Arc<StdMutex<HashMap<String, CaptureSink>>>,
    rx: mpsc::UnboundedReceiver<String>,
    interrupted: Arc<AtomicBool>,
}

impl Capture {
    /// Collect lines until the output goes quiet for `idle`, or `max` passes.
    /// Waits up to `first` for the first line: a busy server answers late.
    pub async fn collect(&mut self, first: Duration, idle: Duration, max: Duration) -> Vec<String> {
        let started = Instant::now();
        let mut lines = Vec::new();
        loop {
            let wait = if lines.is_empty() { first } else { idle };
            let remaining = max.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(wait.min(remaining), self.rx.recv()).await {
                Ok(Some(line)) => lines.push(line),
                Ok(None) | Err(_) => break,
            }
        }
        lines
    }

    /// The user sent a command while this was open, so it closed early and
    /// what it collected may be incomplete.
    pub fn interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Ok(mut map) = self.captures.lock() {
            // Only our own sink: an interrupted capture's slot may already
            // belong to a newer one.
            if map.get(&self.instance_id).is_some_and(|sink| Arc::ptr_eq(&sink.interrupted, &self.interrupted)) {
                map.remove(&self.instance_id);
            }
        }
    }
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

        let hints = Arc::new(ExitHints::default());
        match stdout {
            Some(stdout) => self.pump_stdout(&request, stdout, hints.clone(), console_tx.clone(), events_tx.clone()),
            None => hints.output_closed.store(true, Ordering::SeqCst),
        }
        if let Some(stderr) = stderr {
            pump_stderr(request.instance_id.clone(), stderr, console_tx.clone());
        }

        self.watch_exit(request.instance_id.clone(), child, stop_requested, hints, events_tx);
        Ok(())
    }

    /// Read stdout on a blocking thread. `read_line` on a child's pipe cannot
    /// be polled, and a dedicated thread per server is cheaper than the async
    /// gymnastics that would avoid it.
    fn pump_stdout(
        &self,
        request: &StartRequest,
        stdout: std::process::ChildStdout,
        hints: Arc<ExitHints>,
        console_tx: mpsc::Sender<ConsoleLine>,
        events_tx: mpsc::Sender<RuntimeEvent>,
    ) {
        let instance_id = request.instance_id.clone();
        let parser = request.parser.clone();
        let tick_rates = self.tick_rates.clone();
        let silence_until = self.silence_until.clone();
        let captures = self.captures.clone();

        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();

            while let Some(line) = read_line_lossy(&mut reader, &mut buf) {
                let reading = parser.read(&line);

                if reading.ready {
                    let _ = events_tx
                        .blocking_send(RuntimeEvent::Status(instance_id.clone(), InstanceStatus::Online));
                }
                if reading.crashed {
                    hints.crashed.store(true, Ordering::SeqCst);
                }
                if reading.stopping && !hints.stopping.swap(true, Ordering::SeqCst) {
                    // Stopped from inside (a player's `/stop`): show it
                    // stopping, as if the button had been pressed.
                    let _ = events_tx
                        .blocking_send(RuntimeEvent::Status(instance_id.clone(), InstanceStatus::Stopping));
                }
                if let Some(name) = &reading.joined {
                    let _ = events_tx
                        .blocking_send(RuntimeEvent::PlayerJoined(instance_id.clone(), name.clone()));
                }
                if let Some(name) = &reading.left {
                    let _ = events_tx
                        .blocking_send(RuntimeEvent::PlayerLeft(instance_id.clone(), name.clone()));
                }
                if let Some((name, uuid)) = &reading.identity {
                    let _ = events_tx.blocking_send(RuntimeEvent::PlayerIdentified(
                        instance_id.clone(),
                        name.clone(),
                        uuid.clone(),
                    ));
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

                let captured = captures.lock().ok().is_some_and(|map| {
                    map.get(&instance_id).is_some_and(|sink| {
                        sink.matches.iter().any(|re| re.is_match(&reading.text))
                            && sink.tx.send(reading.text.clone()).is_ok()
                    })
                });

                if !captured && reading.visible && !(silenced && reading.tick_rate.is_some()) {
                    let speaker = reading.speaker();
                    let _ = console_tx.blocking_send(ConsoleLine {
                        timestamp: Utc::now(),
                        level: reading.level,
                        message: reading.text,
                        body: reading.body,
                        speaker,
                    });
                }
            }
            hints.output_closed.store(true, Ordering::SeqCst);
        });
    }

    /// Reap the child and report why it ended.
    fn watch_exit(
        &self,
        instance_id: String,
        mut child: Child,
        stop_requested: Arc<AtomicBool>,
        hints: Arc<ExitHints>,
        events_tx: mpsc::Sender<RuntimeEvent>,
    ) {
        let processes = self.processes.clone();
        let tick_rates = self.tick_rates.clone();
        let resources = self.resources.clone();
        // Captured here, on the async caller: the reaper below is a plain OS
        // thread with no runtime context, so `Handle::try_current()` inside it
        // always failed and the exited server stayed registered as running —
        // every later start (and every wake-on-join) then hit "already running".
        let runtime = tokio::runtime::Handle::try_current().ok();

        thread::spawn(move || {
            let status = child.wait();
            let code = status.as_ref().ok().and_then(|s| s.code());
            // The last lines may still be in the pipe; give the reader a
            // moment to reach them before judging.
            let drained_by = Instant::now() + OUTPUT_DRAIN;
            while !hints.output_closed.load(Ordering::SeqCst) && Instant::now() < drained_by {
                thread::sleep(Duration::from_millis(25));
            }
            let expected = exit_was_expected(
                stop_requested.load(Ordering::SeqCst),
                code,
                hints.stopping.load(Ordering::SeqCst),
                hints.crashed.load(Ordering::SeqCst),
            );

            if let Ok(mut map) = tick_rates.write() {
                map.remove(&instance_id);
            }
            if let Ok(mut map) = resources.write() {
                map.remove(&instance_id);
            }

            // Deregister BEFORE reporting the exit, so whoever reacts to
            // `Exited` (auto-restart, wake-on-join, a user pressing Start)
            // already sees the server as not running.
            match &runtime {
                Some(handle) => handle.block_on(async {
                    processes.write().await.remove(&instance_id);
                }),
                None => {
                    if let Ok(mut map) = processes.try_write() {
                        map.remove(&instance_id);
                    }
                }
            }

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

    /// Send a command the user typed. Closes any open capture first, so the
    /// answer is shown rather than swallowed.
    pub async fn send_command(&self, instance_id: &str, command: &str) -> AppResult<()> {
        if let Ok(mut map) = self.captures.lock() {
            if let Some(sink) = map.remove(instance_id) {
                sink.interrupted.store(true, Ordering::SeqCst);
            }
        }
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

    /// Open a capture of this server's output for lines matching `matches`.
    /// Refused while another capture of the same server is open.
    pub fn capture(&self, instance_id: &str, matches: Vec<Regex>) -> AppResult<Capture> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut map = self
            .captures
            .lock()
            .map_err(|_| AppError::unexpected("capture registry poisoned"))?;
        if map.contains_key(instance_id) {
            return Err(AppError::new(ErrorCode::NodeBusy).with("instance", instance_id));
        }
        let interrupted = Arc::new(AtomicBool::new(false));
        map.insert(
            instance_id.to_string(),
            CaptureSink { matches: Arc::new(matches), tx, interrupted: interrupted.clone() },
        );
        Ok(Capture { instance_id: instance_id.to_string(), captures: self.captures.clone(), rx, interrupted })
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
            let _ = console_tx.blocking_send(ConsoleLine::new(ConsoleLevel::Error, line));
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

    /// Regression: the reaper thread had no runtime context, so an exited
    /// server stayed registered and every later start or wake-on-join failed
    /// with "already running".
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_exited_process_is_no_longer_running_when_exit_is_reported() {
        let supervisor = ProcessSupervisor::new();
        let (program, args) = if cfg!(windows) {
            ("cmd".to_string(), vec!["/C".to_string(), "exit 0".to_string()])
        } else {
            ("sh".to_string(), vec!["-c".to_string(), "exit 0".to_string()])
        };
        let request = StartRequest {
            instance_id: "short".into(),
            instance_name: "short".into(),
            plan: LaunchPlan { program, args, working_dir: std::env::temp_dir(), env: Default::default() },
            runtime_kind: RuntimeKind::HostProcess,
            parser: Arc::new(
                ConsoleParser::new(&crate::game::minecraft::manifest().console).expect("patterns compile"),
            ),
            stop_command: "stop".into(),
            container_image: None,
            port: 0,
        };
        let (console_tx, _console_rx) = mpsc::channel(64);
        let (events_tx, mut events_rx) = mpsc::channel(64);
        supervisor.start(request, console_tx, events_tx).await.expect("starts");

        let exited = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = events_rx.recv().await {
                if matches!(event, RuntimeEvent::Exited { .. }) {
                    return true;
                }
            }
            false
        })
        .await
        .expect("exit reported in time");
        assert!(exited);
        assert!(!supervisor.is_running("short").await, "deregistered before Exited is sent");
    }

    #[test]
    fn an_exit_is_a_stop_or_a_crash() {
        // Nebula asked: always a stop, even when the server exits non-zero.
        assert!(exit_was_expected(true, Some(1), false, false));
        assert!(exit_was_expected(true, None, false, true));
        // A player's `/stop`: the server said so, whatever the code.
        assert!(exit_was_expected(false, Some(1), true, false));
        assert!(exit_was_expected(false, None, true, false));
        // A clean exit code.
        assert!(exit_was_expected(false, Some(0), false, false));
        // Ended without warning.
        assert!(!exit_was_expected(false, Some(1), false, false));
        assert!(!exit_was_expected(false, None, false, false));
        // A crash logs its shutdown too, and may exit 0: still a crash.
        assert!(!exit_was_expected(false, Some(0), true, true));
    }

    /// Run a shell line as a server and report whether its exit was expected.
    async fn exit_of(script: &str) -> bool {
        let supervisor = ProcessSupervisor::new();
        let (program, args) = if cfg!(windows) {
            ("cmd".to_string(), vec!["/C".to_string(), script.to_string()])
        } else {
            ("sh".to_string(), vec!["-c".to_string(), script.to_string()])
        };
        let request = StartRequest {
            instance_id: "exit".into(),
            instance_name: "exit".into(),
            plan: LaunchPlan { program, args, working_dir: std::env::temp_dir(), env: Default::default() },
            runtime_kind: RuntimeKind::HostProcess,
            parser: Arc::new(
                ConsoleParser::new(&crate::game::minecraft::manifest().console).expect("patterns compile"),
            ),
            stop_command: "stop".into(),
            container_image: None,
            port: 0,
        };
        let (console_tx, _console_rx) = mpsc::channel(64);
        let (events_tx, mut events_rx) = mpsc::channel(64);
        supervisor.start(request, console_tx, events_tx).await.expect("starts");
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = events_rx.recv().await {
                if let RuntimeEvent::Exited { expected, .. } = event {
                    return expected;
                }
            }
            panic!("no exit reported");
        })
        .await
        .expect("exit reported in time")
    }

    /// Regression: a player's `/stop` (or `stop` typed in the console) made
    /// Nebula report a crash, because only its own Stop button counted.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stop_from_inside_the_server_is_not_a_crash() {
        assert!(exit_of("echo [12:00:00] [Server thread/INFO]: Stopping the server&& exit 1").await);
        assert!(!exit_of("echo [12:00:00] [Server thread/INFO]: Preparing level&& exit 1").await);
        assert!(exit_of("exit 0").await);
    }

    /// A frozen Paper server: the watchdog logs it, then Spigot's restart
    /// path logs a normal shutdown and exits 0. Still a crash.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_watchdog_shutdown_is_a_crash() {
        assert!(
            !exit_of(
                "echo [12:00:00 ERROR]: The server has stopped responding! This is not a Paper bug.&& echo [12:00:01 INFO]: Stopping server&& exit 0"
            )
            .await
        );
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

    #[tokio::test]
    async fn one_capture_per_instance_and_dropping_closes_it() {
        let supervisor = ProcessSupervisor::new();
        let first = supervisor.capture("a", vec![]).expect("first capture");
        assert!(supervisor.capture("a", vec![]).is_err(), "second capture refused");
        assert!(supervisor.capture("b", vec![]).is_ok(), "other instances unaffected");
        drop(first);
        assert!(supervisor.capture("a", vec![]).is_ok(), "reopened after drop");
    }

    #[tokio::test]
    async fn a_user_command_closes_an_open_capture() {
        let supervisor = ProcessSupervisor::new();
        let mut capture = supervisor.capture("a", vec![]).expect("capture");
        // Refused (nothing is running), but the capture must close anyway.
        let _ = supervisor.send_command("a", "help").await;
        assert!(capture.interrupted());
        assert!(supervisor.captures.lock().expect("lock").get("a").is_none());
        let lines = capture
            .collect(Duration::from_secs(5), Duration::from_millis(20), Duration::from_secs(5))
            .await;
        assert!(lines.is_empty(), "closed at once, no waiting");
        // A newer capture is not torn down when the interrupted one drops.
        let newer = supervisor.capture("a", vec![]).expect("reopen");
        drop(capture);
        assert!(supervisor.captures.lock().expect("lock").get("a").is_some());
        drop(newer);
    }

    #[tokio::test]
    async fn a_capture_collects_until_quiet() {
        let supervisor = ProcessSupervisor::new();
        let mut capture = supervisor.capture("a", vec![]).expect("capture");
        {
            let map = supervisor.captures.lock().expect("lock");
            let sink = map.get("a").expect("registered");
            sink.tx.send("one".into()).expect("send");
            sink.tx.send("two".into()).expect("send");
        }
        let lines = capture
            .collect(Duration::from_millis(50), Duration::from_millis(20), Duration::from_secs(1))
            .await;
        assert_eq!(lines, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn tick_rate_is_absent_until_one_is_reported() {
        let supervisor = ProcessSupervisor::new();
        assert_eq!(supervisor.tick_rate("missing"), None);
    }
}
