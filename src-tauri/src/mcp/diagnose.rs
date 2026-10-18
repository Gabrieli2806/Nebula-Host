//! Crash and log diagnosis for assistants.
//!
//! The assistant is the one that reasons; this gathers the evidence it needs
//! in one call (recent errors, the newest crash report, the end of the log,
//! JVM crash files) and adds hints for the problems that cause most failed
//! starts, so even a small model lands on the right cause.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::AppResult;
use crate::model::{ConsoleLevel, ConsoleLine, InstanceView};
use crate::service::InstanceService;

/// A known problem: what to look for, and what it means.
struct Pattern {
    needles: &'static [&'static str],
    hint: &'static str,
}

const PATTERNS: &[Pattern] = &[
    Pattern {
        needles: &["UnsupportedClassVersionError", "has been compiled by a more recent version of the Java Runtime", "Unsupported class file major version"],
        hint: "Wrong Java version: the server or a mod needs a newer Java than the one used. Minecraft 1.20.5+ needs Java 21, 1.18–1.20.4 needs Java 17. Point the server at a newer Java in its settings.",
    },
    Pattern {
        needles: &["FAILED TO BIND TO PORT", "Address already in use", "BindException"],
        hint: "The port is already in use — another server or program is listening on it. Stop the other one or change this server's port.",
    },
    Pattern {
        needles: &["java.lang.OutOfMemoryError", "Java heap space", "GC overhead limit exceeded"],
        hint: "Out of memory: the server needs more RAM (increase its memory) or a mod/plugin is leaking. Modpacks usually need 6–8 GB.",
    },
    Pattern {
        needles: &["You need to agree to the EULA", "eula.txt"],
        hint: "The Minecraft EULA has not been accepted. Only the server's owner can accept it: ask them to start the server in the Nebula Host app, which shows the EULA and asks.",
    },
    Pattern {
        needles: &["Incompatible mods found", "Missing or unsupported mandatory dependencies", "requires version", "Unmet dependency listing", "depends on"],
        hint: "A mod is missing a required dependency or needs a different version of another mod/Minecraft. The lines above name the mod and what it needs; install or update that dependency.",
    },
    Pattern {
        needles: &["UnknownDependencyException", "Unknown/missing dependency plugins"],
        hint: "A plugin needs another plugin that is not installed. Install the plugin it names.",
    },
    Pattern {
        needles: &["Mixin apply failed", "MixinApplyError", "MixinTransformerError", "InvalidInjectionException"],
        hint: "Two mods conflict, or a mod is built for another Minecraft/loader version (a Mixin failed). The stack trace names the mod; remove or update it.",
    },
    Pattern {
        needles: &["Could not load 'plugins", "Invalid plugin.yml", "is not a valid plugin"],
        hint: "A plugin failed to load — usually built for a different server version or broken. Update or remove the plugin named.",
    },
    Pattern {
        needles: &["Exception in server tick loop", "Encountered an unexpected exception"],
        hint: "The server crashed while running. The crash report below has the stack trace; the first mod/plugin package named in it is usually the culprit.",
    },
    Pattern {
        needles: &["Failed to load level", "Exception reading", "level.dat", "Corrupted"],
        hint: "The world may be corrupted. Restore a backup, or check the world folder.",
    },
    Pattern {
        needles: &["Can't keep up!", "Running behind"],
        hint: "The server is overloaded (lag). Give it more RAM/CPU, pre-generate the world, lower view-distance, or find a heavy mod/plugin.",
    },
    Pattern {
        needles: &["Error: Could not find or load main class", "Unable to access jarfile", "Invalid or corrupt jarfile"],
        hint: "The server jar is missing or broken. Reinstall the server.",
    },
    Pattern {
        needles: &["A fatal error has been detected by the Java Runtime Environment", "EXCEPTION_ACCESS_VIOLATION"],
        hint: "The Java runtime itself crashed (native crash). Often a graphics/native mod or a broken Java install; an hs_err_pid file has details.",
    },
];

pub fn is_problem(line: &ConsoleLine) -> bool {
    matches!(line.level, ConsoleLevel::Warn | ConsoleLevel::Error)
        || line.message.contains("Exception")
        || line.message.contains("ERROR")
}

pub fn format_line(line: &ConsoleLine) -> String {
    let level = match line.level {
        ConsoleLevel::Info => "INFO",
        ConsoleLevel::Warn => "WARN",
        ConsoleLevel::Error => "ERROR",
        ConsoleLevel::System => "NEBULA",
    };
    format!("{} [{level}] {}", line.timestamp.format("%H:%M:%S"), line.message)
}

/// The last `max` bytes of `text`, cut at a line boundary.
pub fn tail_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let cut = text[start..].find('\n').map(|i| start + i + 1).unwrap_or(start);
    format!("[…truncated, showing the last {} KB]\n{}", max / 1024, &text[cut..])
}

/// The command list our own silent `help` (console autocomplete) leaves in
/// the log: `[..INFO]: /tp: A Mojang provided command.` — noise for a diagnosis.
fn is_help_listing(line: &str) -> bool {
    line.split_once("]: /").is_some_and(|(_, rest)| {
        rest.split_once(':').is_some_and(|(cmd, _)| !cmd.is_empty() && !cmd.contains(' '))
    }) || line.contains("--------- Help:")
}

fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !is_help_listing(l)).collect();
    let from = lines.len().saturating_sub(n);
    lines[from..].join("\n")
}

fn first_lines(text: &str, n: usize) -> String {
    text.lines().take(n).collect::<Vec<_>>().join("\n")
}

fn newest_file(dir: &Path, accept: impl Fn(&str) -> bool) -> Option<(PathBuf, SystemTime)> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| accept(&e.file_name().to_string_lossy()))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .max_by_key(|(_, modified)| *modified)
}

fn age(modified: SystemTime) -> String {
    let secs = SystemTime::now().duration_since(modified).map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..=119 => format!("{secs} s ago"),
        120..=7199 => format!("{} min ago", secs / 60),
        7200..=172_799 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// Hints whose needles appear in any of the evidence.
fn hints_for(evidence: &[&str]) -> Vec<&'static str> {
    PATTERNS
        .iter()
        .filter(|p| p.needles.iter().any(|needle| evidence.iter().any(|text| text.contains(needle))))
        .map(|p| p.hint)
        .collect()
}

pub async fn report(service: &InstanceService, view: &InstanceView) -> AppResult<String> {
    let instance = &view.instance;
    let root = instance.path.clone();
    let status = serde_json::to_value(view.state.status)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();

    let console = service.console_tail(&instance.id, 1000).await;
    let problems: Vec<String> = console.iter().filter(|l| is_problem(l)).map(format_line).collect();
    let recent_problems = problems[problems.len().saturating_sub(60)..].join("\n");
    let console_tail: Vec<String> = console.iter().rev().take(40).rev().map(format_line).collect();
    let console_tail = console_tail.join("\n");

    // Blocking disk reads, off the async runtime.
    let files = tokio::task::spawn_blocking(move || {
        let crash = newest_file(&root.join("crash-reports"), |n| n.ends_with(".txt"))
            .and_then(|(path, modified)| {
                let text = fs::read_to_string(&path).ok()?;
                Some((path.file_name()?.to_string_lossy().to_string(), age(modified), text))
            });
        let latest_log = fs::read_to_string(root.join("logs").join("latest.log")).ok();
        let jvm_crash = newest_file(&root, |n| n.starts_with("hs_err_pid") && n.ends_with(".log"))
            .and_then(|(path, modified)| {
                let text = fs::read_to_string(&path).ok()?;
                Some((path.file_name()?.to_string_lossy().to_string(), age(modified), text))
            });
        (crash, latest_log, jvm_crash)
    })
    .await
    .unwrap_or((None, None, None));
    let (crash, latest_log, jvm_crash) = files;

    let log_tail = latest_log.as_deref().map(|t| last_lines(t, 120)).unwrap_or_default();
    let crash_head = crash.as_ref().map(|(_, _, t)| first_lines(t, 140)).unwrap_or_default();
    let jvm_head = jvm_crash.as_ref().map(|(_, _, t)| first_lines(t, 40)).unwrap_or_default();

    let hints = hints_for(&[&recent_problems, &console_tail, &log_tail, &crash_head, &jvm_head]);

    let mut out = String::new();
    out.push_str(&format!("# Diagnosis: {}\n\n", instance.name));
    out.push_str(&format!(
        "- Status: {status}\n- Build: {} {}{}\n- Memory allocated: {} MB (in use: {} MB)\n- Java major (installed for): {}\n- Restart-after-crash: {} ({} attempts left)\n",
        instance.build.distribution,
        instance.build.version,
        instance.build.loader_version.as_ref().map(|l| format!(" (loader {l})")).unwrap_or_default(),
        instance.resources.memory_mb,
        view.state.memory_used_mb,
        instance.install.as_ref().and_then(|r| r.java_major).map(|j| j.to_string()).unwrap_or_else(|| "unknown".into()),
        instance.automation.restart_after_crash,
        instance.automation.restart_attempts_remaining,
    ));

    out.push_str("\n## Known-problem hints\n");
    if hints.is_empty() {
        out.push_str("None of the common failure patterns matched. Read the evidence below.\n");
    } else {
        for hint in &hints {
            out.push_str(&format!("- {hint}\n"));
        }
    }

    out.push_str("\n## Recent warnings and errors (console)\n");
    out.push_str(if recent_problems.is_empty() { "(none)" } else { &recent_problems });
    out.push_str("\n\n## Last console lines\n");
    out.push_str(if console_tail.is_empty() { "(empty — the server has not run since the app started)" } else { &console_tail });

    match &crash {
        Some((name, when, _)) => {
            out.push_str(&format!("\n\n## Newest crash report: crash-reports/{name} ({when})\n"));
            out.push_str(&crash_head);
        }
        None => out.push_str("\n\n## Crash reports\n(none)"),
    }
    if let Some((name, when, _)) = &jvm_crash {
        out.push_str(&format!("\n\n## JVM crash file: {name} ({when})\n"));
        out.push_str(&jvm_head);
    }
    if !log_tail.is_empty() {
        out.push_str("\n\n## End of logs/latest.log\n");
        out.push_str(&log_tail);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_match_common_failures() {
        let java = "java.lang.UnsupportedClassVersionError: net/minecraft/server/Main has been compiled by a more recent version of the Java Runtime";
        assert!(hints_for(&[java])[0].contains("Wrong Java"));
        assert!(hints_for(&["**** FAILED TO BIND TO PORT!"])[0].contains("port"));
        assert!(hints_for(&["java.lang.OutOfMemoryError: Java heap space"])[0].contains("memory"));
        assert!(hints_for(&["all good here"]).is_empty());
    }

    #[test]
    fn help_listing_is_dropped_from_the_log_tail() {
        let log = "[22:34:19] [Server thread/INFO]: /tp: A Mojang provided command.
[22:34:20] [Server thread/ERROR]: real problem
";
        assert_eq!(last_lines(log, 10), "[22:34:20] [Server thread/ERROR]: real problem");
    }

    #[test]
    fn tail_cuts_at_a_line_boundary() {
        let text = "aaaa\nbbbb\ncccc\n";
        let tail = tail_bytes(text, 7);
        assert!(tail.ends_with("cccc\n"));
        assert!(!tail.contains("bb\n") || tail.contains("bbbb"));
        assert_eq!(tail_bytes("short", 100), "short");
    }
}
