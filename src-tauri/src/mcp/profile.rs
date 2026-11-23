//! How a running server is performing, from what it can say without anything
//! being installed, and from spark when it is there.
//!
//! The sources differ by server type, so the answer names each one it asked
//! and whether it answered, instead of presenting a guess as a measurement:
//! - what the app already tracks (CPU, memory, players, the last tick rate);
//! - the server's own console commands (`tps`, `mspt`, `forge tps`, `tick query`);
//! - spark (`spark tps`, `spark health`, and a timed `spark profiler`).
//!
//! The commands are sent the way the app sends its own (`help` for the
//! console's suggestions): their answers are captured, not printed in the
//! console in front of whoever is reading it.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::model::{InstanceStatus, InstanceView};
use crate::service::InstanceService;

/// What a server says to a command it does not have.
const UNKNOWN_COMMAND: &str = r"Unknown (or incomplete )?command|Incorrect argument for command|<--\[HERE\]";
/// How spark marks its lines, and the lines themselves in case the console
/// cannot print the mark.
const SPARK_LINES: &[&str] = &[
    "⚡",
    "TPS from last 5s",
    "Tick durations",
    "CPU usage from last",
    "Memory usage:",
    "Disk usage:",
    r"spark\.lucko\.me",
    "(?i)profiler",
];

const MAX_LINES: usize = 40;
const MAX_LINE: usize = 500;
pub const MIN_PROFILE_SECONDS: u64 = 5;
pub const MAX_PROFILE_SECONDS: u64 = 60;

/// One question put to the server's console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub command: String,
    /// What the answer tells.
    pub what: &'static str,
    /// The lines that are its answer.
    pub patterns: Vec<&'static str>,
}

fn probe(command: &str, what: &'static str, patterns: &[&'static str]) -> Probe {
    Probe { command: command.to_string(), what, patterns: patterns.to_vec() }
}

/// What to ask a server of this distribution, most telling first.
pub fn probes(distribution: &str, spark: bool) -> Vec<Probe> {
    let mut list = Vec::new();
    match distribution {
        "paper" | "purpur" | "folia" => {
            list.push(probe("tps", "Ticks per second over the last 1, 5 and 15 minutes (20 is full speed).", &["TPS from last"]));
            list.push(probe(
                "mspt",
                "Milliseconds per tick (average/minimum/maximum) over the last 5 s, 10 s and 1 min. Above 50 the server falls behind.",
                &["tick times", r"\d+(\.\d+)?/\d+(\.\d+)?/\d+(\.\d+)?"],
            ));
        }
        "spigot" => {
            list.push(probe("tps", "Ticks per second over the last 1, 5 and 15 minutes (20 is full speed).", &["TPS from last"]));
        }
        "forge" | "neoforge" => {
            list.push(probe(
                &format!("{distribution} tps"),
                "Mean tick time and ticks per second, for each dimension and overall.",
                &["Mean tick time", "Mean TPS"],
            ));
            list.push(probe(
                &format!("{distribution} entity list"),
                "How many entities of each type are loaded, in the overworld.",
                &[r"Total: \d+", r"\d+: [a-z0-9_.-]+:[a-z0-9_/.-]+"],
            ));
        }
        // Vanilla and what runs its commands unchanged. The command exists from Minecraft 1.20.3.
        _ => {
            list.push(probe(
                "tick query",
                "The target tick rate, the average time per tick and its percentiles (Minecraft 1.20.3 and newer).",
                &["tick rate", "time per tick", "Percentiles", "running normally", "is frozen", "sprinting"],
            ));
        }
    }
    if spark {
        list.push(probe("spark tps", "spark: ticks per second, tick durations (min/median/95th percentile/max) and CPU usage.", SPARK_LINES));
        list.push(probe("spark health", "spark: TPS, CPU, memory in use by the server and the machine, and disk.", SPARK_LINES));
    }
    list
}

/// The spark jar among a server's mods or plugins, if it is installed and on.
pub fn spark_file<I: IntoIterator<Item = String>>(names: I) -> Option<String> {
    names.into_iter().find(|name| {
        let lower = name.to_lowercase();
        lower.ends_with(".jar") && lower.starts_with("spark")
    })
}

fn installed_spark(dir: &Path) -> Option<String> {
    let names = std::fs::read_dir(dir).ok()?.flatten().map(|entry| entry.file_name().to_string_lossy().to_string());
    spark_file(names)
}

/// The address of a profile spark uploaded, in what it printed.
pub fn spark_url(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        let start = line.find("https://spark.lucko.me/")?;
        let url: String = line[start..].chars().take_while(|c| !c.is_whitespace()).collect();
        Some(url)
    })
}

/// Whether what came back says the server has no such command.
pub fn is_unknown_command(lines: &[String]) -> bool {
    let unknown = Regex::new(UNKNOWN_COMMAND).expect("a fixed pattern");
    lines.iter().any(|line| unknown.is_match(line))
}

/// Captured lines as an answer carries them: no more than fit, none endless.
fn tidy(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .take(MAX_LINES)
        .map(|line| if line.chars().count() > MAX_LINE { line.chars().take(MAX_LINE).chain(['…']).collect() } else { line })
        .collect()
}

fn compile(patterns: &[&str]) -> Vec<Regex> {
    patterns.iter().chain(std::iter::once(&UNKNOWN_COMMAND)).filter_map(|pattern| Regex::new(pattern).ok()).collect()
}

/// `duration_seconds`, when given: how long to run spark's profiler.
pub fn profile_seconds(args: &Value) -> Option<u64> {
    args.get("duration_seconds").and_then(Value::as_u64).map(|seconds| seconds.clamp(MIN_PROFILE_SECONDS, MAX_PROFILE_SECONDS))
}

pub async fn report(service: &Arc<InstanceService>, view: &InstanceView, profile_for: Option<u64>) -> AppResult<String> {
    let instance = &view.instance;
    let id = instance.id.as_str();
    if view.state.status != InstanceStatus::Online {
        return Err(AppError::new(ErrorCode::InstanceNotRunning)
            .with("instance", &instance.name)
            .detail("a server is measured while it is online"));
    }

    let spark = service.addon_profile(instance).ok().and_then(|(dir, _)| installed_spark(&dir));
    let mut notes: Vec<String> = Vec::new();
    let mut sources = Vec::new();

    // One capture per server at a time: the console's suggestions use the
    // same mechanism, and wait their turn behind this lock.
    let turn = service.completion_lock(id).await;
    let _turn = turn.lock().await;

    for probe in probes(&instance.build.distribution, spark.is_some()) {
        let mut capture = match service.processes.capture(id, compile(&probe.patterns)) {
            Ok(capture) => capture,
            Err(_) => {
                notes.push("The console was busy with another question, so some commands were not asked. Call profile_server again.".into());
                break;
            }
        };
        service.processes.send_internal_command(id, &probe.command).await?;
        let lines = capture.collect(Duration::from_secs(3), Duration::from_millis(600), Duration::from_secs(5)).await;
        let interrupted = capture.interrupted();
        drop(capture);

        let unknown = is_unknown_command(&lines);
        sources.push(json!({
            "command": probe.command,
            "what": probe.what,
            "answered": !lines.is_empty() && !unknown,
            "output": tidy(lines),
        }));
        if interrupted {
            notes.push("Someone typed a command in the console meanwhile, so the answers may be incomplete. Call profile_server again.".into());
            break;
        }
    }

    let mut out = json!({
        "server": instance.name,
        "status": "online",
        "build": format!("{} {}", instance.build.distribution, instance.build.version),
        "tracked_by_app": {
            "tps": view.state.tick_rate,
            "cpu_percent": (view.state.cpu_percent * 10.0).round() / 10.0,
            "memory_used_mb": view.state.memory_used_mb,
            "memory_allocated_mb": instance.resources.memory_mb,
            "players_online": view.state.players_online,
            "uptime_seconds": view.state.uptime_seconds,
        },
        "sources": sources,
        "spark": { "installed": spark.is_some(), "file": spark },
    });

    match (profile_for, spark.is_some()) {
        (Some(seconds), true) => out["profiler"] = profile(service, id, seconds, &mut notes).await?,
        (Some(_), false) => notes.push(
            "No profile was taken: `duration_seconds` needs spark, which is not installed on this server.".into(),
        ),
        (None, _) => {}
    }

    if spark.is_some() {
        notes.push("Which plugins or mods take the time, and the entities and chunks of each world, are in a spark profile: pass `duration_seconds` (e.g. 30) and open the `url` it returns.".into());
    } else {
        notes.push("Which plugins or mods take the time, and the entities and chunks of each world, are not available without a profiler. Install spark (install_addon with project_id `spark`, then restart_server) and call profile_server with `duration_seconds`.".into());
    }
    out["notes"] = json!(notes);
    Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
}

/// Run spark's profiler for `seconds` and return what it printed and the
/// address of the report it uploaded.
async fn profile(service: &Arc<InstanceService>, id: &str, seconds: u64, notes: &mut Vec<String>) -> AppResult<Value> {
    let mut capture = service
        .processes
        .capture(id, compile(SPARK_LINES))
        .map_err(|_| AppError::new(ErrorCode::NodeBusy).detail("the console is busy with another question: call profile_server again"))?;
    let command = format!("spark profiler start --timeout {seconds}");
    service.processes.send_internal_command(id, &command).await?;

    // The profile is uploaded when the time is up; that takes a moment more.
    let deadline = Instant::now() + Duration::from_secs(seconds + 20);
    let mut lines: Vec<String> = Vec::new();
    while Instant::now() < deadline && spark_url(&lines).is_none() && !capture.interrupted() {
        let more = capture.collect(Duration::from_secs(2), Duration::from_millis(500), Duration::from_secs(2)).await;
        if is_unknown_command(&more) {
            lines.extend(more);
            break;
        }
        lines.extend(more);
    }
    let interrupted = capture.interrupted();
    drop(capture);

    let url = spark_url(&lines);
    if url.is_none() {
        notes.push(if interrupted {
            "The profile was interrupted by a command typed in the console; its report, if any, is printed there (get_console).".into()
        } else {
            "spark did not print a report address in time. Look for it with get_console, or run `spark profiler stop` with send_command. Old versions of spark use `spark profiler --timeout <seconds>` instead.".into()
        });
    }
    Ok(json!({ "command": command, "seconds": seconds, "url": url, "output": tidy(lines) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands(distribution: &str, spark: bool) -> Vec<String> {
        probes(distribution, spark).into_iter().map(|probe| probe.command).collect()
    }

    #[test]
    fn each_server_type_is_asked_what_it_can_answer() {
        assert_eq!(commands("paper", false), ["tps", "mspt"]);
        assert_eq!(commands("purpur", false), ["tps", "mspt"]);
        assert_eq!(commands("spigot", false), ["tps"]);
        assert_eq!(commands("forge", false), ["forge tps", "forge entity list"]);
        assert_eq!(commands("neoforge", false), ["neoforge tps", "neoforge entity list"]);
        assert_eq!(commands("vanilla", false), ["tick query"]);
        assert_eq!(commands("fabric", false), ["tick query"]);
        assert_eq!(commands("paper", true), ["tps", "mspt", "spark tps", "spark health"]);
        assert_eq!(commands("fabric", true), ["tick query", "spark tps", "spark health"]);
    }

    #[test]
    fn every_pattern_compiles_and_catches_its_answer() {
        for distribution in ["paper", "spigot", "forge", "neoforge", "vanilla"] {
            for probe in probes(distribution, true) {
                assert_eq!(compile(&probe.patterns).len(), probe.patterns.len() + 1, "{}", probe.command);
            }
        }
        let caught = |command: &str, line: &str| {
            let probe = probes("paper", true)
                .into_iter()
                .chain(probes("forge", false))
                .chain(probes("vanilla", false))
                .find(|probe| probe.command == command)
                .unwrap();
            compile(&probe.patterns).iter().any(|pattern| pattern.is_match(line))
        };
        assert!(caught("tps", "[12:00:00 INFO]: TPS from last 1m, 5m, 15m: 20.0, 20.0, 20.0"));
        assert!(caught("mspt", "[12:00:00 INFO]: Server tick times (avg/min/max) from last 5s, 10s, 1m:"));
        assert!(caught("mspt", "[12:00:00 INFO]: ◴ 1.2/0.8/4.1, 1.3/0.8/5.0, 1.3/0.7/9.9"));
        assert!(caught("forge tps", "[Server thread/INFO]: Overall: Mean tick time: 1.234 ms. Mean TPS: 20.000"));
        assert!(caught("forge entity list", "[Server thread/INFO]:   12: minecraft:zombie"));
        assert!(caught("tick query", "[Server thread/INFO]: Average time per tick: 1.5ms (Target: 50.0ms)"));
        assert!(caught("spark tps", "[12:00:00 INFO]: [⚡] TPS from last 5s, 10s, 1m, 5m, 15m:"));
        assert!(caught("spark health", "[12:00:00 INFO]: Memory usage:"));
        // What has nothing to do with the question stays in the console.
        assert!(!caught("tps", "[12:00:00 INFO]: Steve joined the game"));
        assert!(!caught("spark tps", "[12:00:00 INFO]: <Steve> hello"));
        // A server without the command says so, and that is caught too.
        assert!(caught("mspt", "[12:00:00 INFO]: Unknown or incomplete command, see below for error"));
    }

    #[test]
    fn a_server_without_the_command_is_not_taken_for_an_answer() {
        assert!(is_unknown_command(&["Unknown or incomplete command, see below for error".into(), "tick query<--[HERE]".into()]));
        assert!(is_unknown_command(&["Unknown command. Type \"/help\" for help.".into()]));
        assert!(!is_unknown_command(&["TPS from last 1m, 5m, 15m: 20.0, 20.0, 20.0".into()]));
        assert!(!is_unknown_command(&[]));
    }

    #[test]
    fn spark_is_found_among_the_jars_when_it_is_on() {
        let names = |list: &[&str]| list.iter().map(|name| name.to_string()).collect::<Vec<_>>();
        assert_eq!(spark_file(names(&["LuckPerms.jar", "spark-1.10.119-bukkit.jar"])).as_deref(), Some("spark-1.10.119-bukkit.jar"));
        assert_eq!(spark_file(names(&["Spark.jar"])).as_deref(), Some("Spark.jar"));
        assert_eq!(spark_file(names(&["spark-1.10.119-bukkit.jar.disabled"])), None, "switched off");
        assert_eq!(spark_file(names(&["sparkle-config", "notspark.jar"])), None);
    }

    #[test]
    fn the_report_address_is_read_from_what_spark_printed() {
        let lines = vec![
            "[12:00:00 INFO]: [⚡] Profiler stopped & upload complete!".to_string(),
            "[12:00:00 INFO]: [⚡] https://spark.lucko.me/AbC123xyz (click to open)".to_string(),
        ];
        assert_eq!(spark_url(&lines).as_deref(), Some("https://spark.lucko.me/AbC123xyz"));
        assert_eq!(spark_url(&lines[..1]), None);
    }

    #[test]
    fn a_profile_runs_for_a_bounded_time() {
        assert_eq!(profile_seconds(&json!({})), None);
        assert_eq!(profile_seconds(&json!({ "duration_seconds": 30 })), Some(30));
        assert_eq!(profile_seconds(&json!({ "duration_seconds": 1 })), Some(MIN_PROFILE_SECONDS));
        assert_eq!(profile_seconds(&json!({ "duration_seconds": 3600 })), Some(MAX_PROFILE_SECONDS));
    }

    #[test]
    fn captured_lines_are_bounded() {
        let many: Vec<String> = (0..100).map(|n| format!("line {n}")).collect();
        assert_eq!(tidy(many).len(), MAX_LINES);
        let long = tidy(vec!["x".repeat(2000)]);
        assert_eq!(long[0].chars().count(), MAX_LINE + 1);
    }
}
