//! Waiting for a line in a server's console, so an agent can run something
//! and be told when its effect shows instead of sleeping and reading again.
//!
//! A wait starts at a place in the console (a cursor: how many lines the
//! server has printed) and ends at the first line after it that matches, when
//! the server stops, or when the time is up. It only looks: nothing is sent to
//! the server, and other requests are served while it waits.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use regex::{Regex, RegexBuilder};
use serde_json::{json, Value};

use super::diagnose;
use super::tools::invalid;
use crate::error::AppResult;
use crate::model::{ConsoleLine, InstanceStatus};
use crate::service::InstanceService;

/// A pattern longer than this is a mistake, not a search.
const MAX_PATTERN: usize = 200;
/// The lines given around a match, and at the end of a wait that found none.
const CONTEXT: usize = 5;
const MAX_LINE: usize = 2000;
const DEFAULT_TIMEOUT: u64 = 60;
/// Under what MCP clients give one request.
const MAX_TIMEOUT: u64 = 100;
const POLL: Duration = Duration::from_millis(150);

/// What a console line is compared with.
pub enum Matcher {
    /// Anywhere in the line, whatever the case.
    Text(String),
    /// The `regex` crate: no backtracking, so no pattern can stall the app.
    Pattern(Regex),
}

impl Matcher {
    pub fn new(pattern: &str, regex: bool) -> AppResult<Self> {
        if pattern.trim().is_empty() {
            return Err(invalid("`pattern` is empty: give the text to wait for"));
        }
        if pattern.chars().count() > MAX_PATTERN {
            return Err(invalid(&format!("`pattern` is longer than {MAX_PATTERN} characters: wait for a shorter piece of the line")));
        }
        if !regex {
            return Ok(Self::Text(pattern.to_lowercase()));
        }
        RegexBuilder::new(pattern)
            .size_limit(1 << 20)
            .build()
            .map(Self::Pattern)
            .map_err(|error| invalid(&format!("`pattern` is not a valid regular expression ({error}). Leave `regex` out to match plain text")))
    }

    pub fn is_match(&self, line: &str) -> bool {
        match self {
            Self::Text(text) => line.to_lowercase().contains(text),
            Self::Pattern(pattern) => pattern.is_match(line),
        }
    }
}

/// What a wait was asked for.
pub struct Wait {
    pub matcher: Matcher,
    /// Where to start: a cursor from an earlier answer. `None` is now.
    pub since: Option<u64>,
    pub timeout: Duration,
}

impl Wait {
    /// From a tool's arguments: `pattern_key` names the pattern (`pattern`
    /// for wait_for_console, `wait_for` for send_command).
    pub fn from_args(args: &Value, pattern_key: &str) -> AppResult<Self> {
        let pattern = args
            .get(pattern_key)
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(&format!("missing argument `{pattern_key}`")))?;
        let regex = args.get("regex").and_then(Value::as_bool).unwrap_or(false);
        let seconds = args.get("timeout_seconds").and_then(Value::as_u64).unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT);
        Ok(Self {
            matcher: Matcher::new(pattern, regex)?,
            since: args.get("since").and_then(Value::as_u64),
            timeout: Duration::from_secs(seconds),
        })
    }
}

fn shown(line: &ConsoleLine) -> String {
    let text = diagnose::format_line(line);
    if text.chars().count() <= MAX_LINE {
        return text;
    }
    let cut: String = text.chars().take(MAX_LINE).collect();
    format!("{cut}…")
}

/// The first of `lines` that matches.
pub fn find(lines: &[ConsoleLine], matcher: &Matcher) -> Option<usize> {
    lines.iter().position(|line| matcher.is_match(&line.message))
}

/// The answer for a match at `index` of `lines`, which end at `cursor`.
/// `earlier` are the lines seen before these.
fn matched(lines: &[ConsoleLine], index: usize, cursor: u64, earlier: &VecDeque<String>) -> Value {
    let before: Vec<String> = earlier
        .iter()
        .cloned()
        .chain(lines[..index].iter().map(shown))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(CONTEXT)
        .rev()
        .collect();
    let after: Vec<String> = lines[index + 1..].iter().take(CONTEXT).map(shown).collect();
    // Right after the matched line, so a wait from here sees what followed.
    let after_match = cursor - (lines.len() - index - 1) as u64;
    json!({
        "matched": true,
        "reason": "matched",
        "line": shown(&lines[index]),
        "before": before,
        "after": after,
        "cursor": after_match,
    })
}

/// Where the lines come from: a server's console, or in the tests a script.
trait Console {
    /// The place in the console now.
    fn cursor(&self) -> impl std::future::Future<Output = u64> + Send;
    /// The server's status, the lines after `cursor`, and the cursor after them.
    fn read(&self, cursor: u64) -> impl std::future::Future<Output = (InstanceStatus, Vec<ConsoleLine>, u64)> + Send;
}

struct Live<'a> {
    service: &'a InstanceService,
    instance_id: &'a str,
}

impl Console for Live<'_> {
    async fn cursor(&self) -> u64 {
        self.service.console_cursor(self.instance_id).await
    }

    async fn read(&self, cursor: u64) -> (InstanceStatus, Vec<ConsoleLine>, u64) {
        // The status first: a server that stops prints its last lines before
        // it is seen as stopped, and those must still be looked at.
        let status = self.service.state_of(self.instance_id).await.status;
        let (lines, next) = self.service.console_since(self.instance_id, cursor).await;
        (status, lines, next)
    }
}

/// Wait on one server's console. Always answers with an object: `matched`,
/// `reason` (matched, timeout, crashed, not_running), a `cursor` to go on
/// from, and either the line with what surrounds it or the last lines seen.
pub async fn wait(service: &InstanceService, instance_id: &str, wait: Wait) -> Value {
    wait_on(&Live { service, instance_id }, wait).await
}

async fn wait_on(console: &(impl Console + Sync), wait: Wait) -> Value {
    let started = Instant::now();
    let mut cursor = match wait.since {
        Some(cursor) => cursor,
        None => console.cursor().await,
    };
    let mut earlier: VecDeque<String> = VecDeque::with_capacity(CONTEXT);

    loop {
        let (status, lines, next) = console.read(cursor).await;
        let waited = started.elapsed().as_secs();
        let status_word = serde_json::to_value(status).unwrap_or(Value::Null);

        if let Some(index) = find(&lines, &wait.matcher) {
            let mut answer = matched(&lines, index, next, &earlier);
            answer["status"] = status_word;
            answer["waited_seconds"] = json!(waited);
            return answer;
        }
        for line in &lines {
            if earlier.len() == CONTEXT {
                earlier.pop_front();
            }
            earlier.push_back(shown(line));
        }
        cursor = next;

        let ended = if !status.expects_process() {
            Some(if status == InstanceStatus::Crashed {
                ("crashed", "The server crashed before the line appeared. Call diagnose_server to see why.")
            } else {
                ("not_running", "The server is not running, so nothing more will be printed. Call start_server, or get_console for what it printed.")
            })
        } else if started.elapsed() >= wait.timeout {
            Some(("timeout", "The line has not appeared yet. Call wait_for_console again with `since` set to this `cursor` to keep waiting, or get_console to read what was printed."))
        } else {
            None
        };
        if let Some((reason, next_step)) = ended {
            return json!({
                "matched": false,
                "reason": reason,
                "status": status_word,
                "waited_seconds": waited,
                "cursor": cursor,
                "last_lines": earlier,
                "next": next_step,
            });
        }
        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ConsoleLevel;

    fn line(message: &str) -> ConsoleLine {
        ConsoleLine { timestamp: chrono::Utc::now(), level: ConsoleLevel::Info, message: message.to_string(), body: None, speaker: None }
    }

    #[test]
    fn plain_text_matches_anywhere_whatever_the_case() {
        let text = Matcher::new("done (", false).unwrap();
        assert!(text.is_match("[Server thread/INFO]: Done (3.2s)! For help, type \"help\""));
        assert!(!text.is_match("Preparing spawn area: 80%"));
        // What would be special in a regular expression is just text.
        let literal = Matcher::new("[MyPlugin] enabled.*", false).unwrap();
        assert!(literal.is_match("[MyPlugin] Enabled.* really"));
        assert!(!literal.is_match("[MyPlugin] enabled"));
    }

    #[test]
    fn a_regular_expression_is_opt_in_and_checked() {
        let pattern = Matcher::new(r"Loaded \d+ recipes", true).unwrap();
        assert!(pattern.is_match("Loaded 1290 recipes"));
        assert!(!pattern.is_match("loaded 1290 recipes"), "a regular expression is matched as written");
        assert!(Matcher::new("(?i)loaded", true).unwrap().is_match("LOADED"));

        let broken = Matcher::new("(unclosed", true).err().expect("refused");
        assert!(broken.detail.as_deref().unwrap_or("").contains("not a valid regular expression"));
        // Backreferences and look-around need backtracking: this engine has neither.
        assert!(Matcher::new(r"(a)\1", true).is_err());
        assert!(Matcher::new(r"(?=a)", true).is_err());
        // A pattern that compiles to something huge is refused, not run.
        assert!(Matcher::new(r"(\w{50}){50}{50}", true).is_err());
    }

    #[test]
    fn empty_and_overlong_patterns_are_refused() {
        assert!(Matcher::new("  ", false).is_err());
        assert!(Matcher::new(&"x".repeat(MAX_PATTERN), false).is_ok());
        assert!(Matcher::new(&"x".repeat(MAX_PATTERN + 1), false).is_err());
        assert!(Matcher::new(&"x".repeat(MAX_PATTERN + 1), true).is_err());
    }

    #[test]
    fn arguments_are_read_with_their_defaults_and_limits() {
        let wait = Wait::from_args(&json!({ "pattern": "Done" }), "pattern").unwrap();
        assert_eq!((wait.since, wait.timeout), (None, Duration::from_secs(DEFAULT_TIMEOUT)));
        assert!(matches!(wait.matcher, Matcher::Text(_)));

        let wait = Wait::from_args(&json!({ "wait_for": "D.ne", "regex": true, "since": 42, "timeout_seconds": 9999 }), "wait_for").unwrap();
        assert_eq!((wait.since, wait.timeout), (Some(42), Duration::from_secs(MAX_TIMEOUT)));
        assert!(matches!(wait.matcher, Matcher::Pattern(_)));
        assert_eq!(Wait::from_args(&json!({ "pattern": "x", "timeout_seconds": 0 }), "pattern").unwrap().timeout, Duration::from_secs(1));

        assert!(Wait::from_args(&json!({}), "pattern").is_err());
        assert!(Wait::from_args(&json!({ "pattern": 5 }), "pattern").is_err());
    }

    #[test]
    fn a_match_comes_with_what_surrounds_it_and_a_cursor_right_after_it() {
        let lines: Vec<ConsoleLine> = ["one", "two", "three", "Done (1.0s)!", "four", "five"].iter().map(|m| line(m)).collect();
        let matcher = Matcher::new("done", false).unwrap();
        assert_eq!(find(&lines, &matcher), Some(3));
        assert_eq!(find(&lines[4..], &matcher), None);

        // 100 lines were printed in all; these six are the last of them.
        let earlier: VecDeque<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let answer = matched(&lines, 3, 100, &earlier);
        assert_eq!(answer["matched"], true);
        assert!(answer["line"].as_str().unwrap().ends_with("[INFO] Done (1.0s)!"));
        let before: Vec<&str> = answer["before"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(before.len(), CONTEXT);
        assert_eq!(&before[..2], ["c", "d"]);
        assert!(before[4].ends_with("three"));
        assert_eq!(answer["after"].as_array().unwrap().len(), 2);
        assert_eq!(answer["cursor"], 98, "the two lines after the match are still ahead");
    }

    /// A console that prints what the script says, one step per look.
    struct Script {
        /// Lines printed so far (at first: before the wait began).
        printed: std::sync::Mutex<Vec<ConsoleLine>>,
        steps: std::sync::Mutex<VecDeque<(InstanceStatus, Vec<&'static str>)>>,
        /// What the server is doing once the script has run out.
        then: InstanceStatus,
    }

    impl Script {
        fn new(already: &[&str], steps: Vec<(InstanceStatus, Vec<&'static str>)>, then: InstanceStatus) -> Self {
            Self {
                printed: std::sync::Mutex::new(already.iter().map(|m| line(m)).collect()),
                steps: std::sync::Mutex::new(steps.into()),
                then,
            }
        }
    }

    impl Console for Script {
        async fn cursor(&self) -> u64 {
            self.printed.lock().unwrap().len() as u64
        }

        async fn read(&self, cursor: u64) -> (InstanceStatus, Vec<ConsoleLine>, u64) {
            let step = self.steps.lock().unwrap().pop_front();
            let mut printed = self.printed.lock().unwrap();
            let status = match step {
                Some((status, messages)) => {
                    printed.extend(messages.iter().map(|m| line(m)));
                    status
                }
                None => self.then,
            };
            (status, printed.iter().skip(cursor as usize).cloned().collect(), printed.len() as u64)
        }
    }

    fn asked(pattern: &str, since: Option<u64>, seconds: u64) -> Wait {
        Wait { matcher: Matcher::new(pattern, false).unwrap(), since, timeout: Duration::from_secs(seconds) }
    }

    use InstanceStatus::{Crashed, Offline, Online};

    #[tokio::test]
    async fn a_wait_ends_at_the_first_matching_line_printed_after_it_began() {
        // "Done" was printed before the wait: it does not count.
        let console = Script::new(
            &["Done (old)"],
            vec![(Online, vec!["Loading plugin"]), (Online, vec![]), (Online, vec!["[MyPlugin] Enabled", "Done (new)", "later"])],
            Online,
        );
        let answer = wait_on(&console, asked("done", None, 30)).await;
        assert_eq!(answer["matched"], true);
        assert_eq!(answer["reason"], "matched");
        assert!(answer["line"].as_str().unwrap().ends_with("Done (new)"));
        let before: Vec<&str> = answer["before"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(before.len(), 2, "what was printed since the wait began, not before");
        assert!(before[0].ends_with("Loading plugin") && before[1].ends_with("[MyPlugin] Enabled"));
        assert_eq!(answer["after"].as_array().unwrap().len(), 1);
        assert_eq!(answer["cursor"], 4, "after the matched line: `later` is still ahead");
        assert_eq!(answer["status"], "online");
    }

    #[tokio::test]
    async fn a_cursor_reaches_back_to_lines_already_printed() {
        let console = Script::new(&["one", "Reload complete", "three"], vec![], Online);
        let answer = wait_on(&console, asked("reload complete", Some(1), 30)).await;
        assert_eq!((answer["matched"].as_bool(), answer["cursor"].as_u64()), (Some(true), Some(2)));
        // From after that line there is nothing of the kind: time runs out.
        let answer = wait_on(&console, asked("reload complete", Some(2), 1)).await;
        assert_eq!(answer["reason"], "timeout");
        // 0 looks at everything the console holds.
        assert_eq!(wait_on(&console, asked("one", Some(0), 1)).await["matched"], true);
    }

    #[tokio::test]
    async fn a_wait_that_runs_out_says_how_to_go_on() {
        let console = Script::new(&[], vec![(Online, vec!["tick", "tock"])], Online);
        let started = Instant::now();
        let answer = wait_on(&console, asked("never printed", None, 1)).await;
        assert!(started.elapsed() >= Duration::from_secs(1) && started.elapsed() < Duration::from_secs(3));
        assert_eq!(answer["matched"], false);
        assert_eq!(answer["reason"], "timeout");
        assert_eq!(answer["cursor"], 2);
        assert_eq!(answer["last_lines"].as_array().unwrap().len(), 2);
        assert!(answer["next"].as_str().unwrap().contains("wait_for_console again with `since`"));
    }

    #[tokio::test]
    async fn a_server_that_stops_or_crashes_ends_the_wait_at_once() {
        // Its last lines are still looked at: here the match is among them.
        let console = Script::new(&[], vec![(Online, vec!["Stopping"]), (Offline, vec!["Saved the game"])], Offline);
        assert_eq!(wait_on(&console, asked("saved the game", None, 30)).await["matched"], true);

        let console = Script::new(&[], vec![(Online, vec!["Starting"]), (Crashed, vec!["Exception in server tick loop"])], Crashed);
        let started = Instant::now();
        let answer = wait_on(&console, asked("Done", None, 30)).await;
        assert!(started.elapsed() < Duration::from_secs(5), "it does not wait out the 30 seconds");
        assert_eq!((answer["matched"].as_bool(), answer["reason"].as_str()), (Some(false), Some("crashed")));
        assert_eq!(answer["status"], "crashed");
        assert!(answer["next"].as_str().unwrap().contains("diagnose_server"));
        assert!(answer["last_lines"][1].as_str().unwrap().ends_with("Exception in server tick loop"));

        let console = Script::new(&["old"], vec![], Offline);
        let answer = wait_on(&console, asked("Done", None, 30)).await;
        assert_eq!(answer["reason"], "not_running");
        assert!(answer["next"].as_str().unwrap().contains("start_server"));
    }

    #[tokio::test]
    async fn waiting_does_not_hold_up_anything_else() {
        let console = Script::new(&[], vec![], Online);
        let waiting = wait_on(&console, asked("never", None, 1));
        let other = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Instant::now()
        };
        let started = Instant::now();
        let (_, other_done) = tokio::join!(waiting, other);
        assert!(other_done - started < Duration::from_millis(500), "the other task ran while the wait was still on");
    }

    #[test]
    fn a_very_long_line_is_cut() {
        let long = shown(&line(&"x".repeat(5000)));
        assert_eq!(long.chars().count(), MAX_LINE + 1);
        assert!(long.ends_with('…'));
    }
}
