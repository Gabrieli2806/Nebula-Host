//! Console autocomplete.
//!
//! When the server is online, the app asks it for its own command list —
//! `help`, paged when the manifest says so — through a hidden capture, so the
//! answer never reaches the user's console. Plugin and mod commands appear
//! there because the server registers them like any other. The result is
//! cached per instance until the server restarts or its addons change, and is
//! merged with the manifest's builtin list (the fallback when it is offline)
//! and the names of the players online right now.
//!
//! Limits, stated plainly:
//! - Only command *names* and a one-line usage/description are known. Deeper
//!   argument completion (item ids, coordinates, sub-sub-commands) is not.
//! - A server that hides `help` from the console, or a help plugin that
//!   rewrites its format, yields only the builtin list.
//! - The help answer is collected for a few seconds at most; a server under
//!   heavy load may answer partially. Asking again (`refresh`) retries.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::manifest::CompletionSpec;
use crate::model::InstanceStatus;
use crate::service::InstanceService;

const FIRST_LINE_WAIT: Duration = Duration::from_secs(3);
const IDLE_WAIT: Duration = Duration::from_millis(500);
const MAX_WAIT: Duration = Duration::from_secs(6);
/// How long an empty answer is trusted before the server is asked again.
const EMPTY_ANSWER_TTL: Duration = Duration::from_secs(120);

/// A cached list worth using: any real answer (until restart or an addon
/// change clears it), or an empty one that is still recent.
fn usable(cached: &CachedCommands) -> bool {
    !cached.commands.is_empty()
        || (Utc::now() - cached.fetched_at).to_std().is_ok_and(|age| age < EMPTY_ANSWER_TTL)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    /// From the manifest's builtin list.
    Builtin,
    /// Reported by the running server (vanilla, plugin or mod command).
    Server,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandSuggestion {
    /// Without the leading slash (`gamemode`, `worldedit:wand`).
    pub name: String,
    /// Argument syntax after the name (`<targets> [<reason>]`), when known.
    pub usage: Option<String>,
    /// The server's one-line description (Bukkit-style help), when known.
    pub description: Option<String>,
    pub source: CommandSource,
    /// Zero-based index of the first argument that takes a player name, so
    /// the UI can offer `players` there.
    pub player_arg: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionSet {
    /// Sorted by name, de-duplicated (server entries win over builtin ones).
    pub commands: Vec<CommandSuggestion>,
    /// Players online now.
    pub players: Vec<String>,
    /// True when the list came (at least partly) from the running server.
    pub from_server: bool,
    /// When the server was last asked; absent when it never was.
    pub fetched_at: Option<DateTime<Utc>>,
}

/// What is cached per instance.
#[derive(Debug, Clone)]
pub struct CachedCommands {
    pub commands: Vec<CommandSuggestion>,
    pub fetched_at: DateTime<Utc>,
}

/// Compiled form of a [`CompletionSpec`].
pub struct HelpParser {
    page: Option<Regex>,
    entry: Regex,
    noise: Vec<Regex>,
    player_placeholders: Vec<String>,
}

impl HelpParser {
    pub fn new(spec: &CompletionSpec) -> AppResult<Self> {
        let compile = |p: &str| {
            Regex::new(p).map_err(|e| AppError::new(ErrorCode::InvalidInput).with("pattern", p).detail(e))
        };
        Ok(Self {
            page: spec.page_pattern.as_deref().map(compile).transpose()?,
            entry: compile(&spec.entry_pattern)?,
            noise: spec.noise_patterns.iter().map(|p| compile(p)).collect::<Result<_, _>>()?,
            player_placeholders: spec.player_placeholders.clone(),
        })
    }

    /// Everything that belongs to a help answer, for the hidden capture.
    pub fn capture_patterns(&self) -> Vec<Regex> {
        let mut out = vec![self.entry.clone()];
        out.extend(self.page.iter().cloned());
        out.extend(self.noise.iter().cloned());
        out
    }

    /// `(page, pages)` if this line is a page header.
    pub fn page_of(&self, line: &str) -> Option<(u32, u32)> {
        let caps = self.page.as_ref()?.captures(line)?;
        Some((caps.name("page")?.as_str().parse().ok()?, caps.name("pages")?.as_str().parse().ok()?))
    }

    /// Read the commands out of a help answer.
    pub fn parse(&self, lines: &[String]) -> Vec<CommandSuggestion> {
        let mut found: BTreeMap<String, CommandSuggestion> = BTreeMap::new();
        for line in lines {
            if self.page_of(line).is_some() || self.noise.iter().any(|re| re.is_match(line)) {
                continue;
            }
            let Some(caps) = self.entry.captures(line) else { continue };
            let Some(name) = caps.name("command").map(|m| m.as_str().to_string()) else { continue };
            let usage = caps.name("usage").map(|m| m.as_str().trim().to_string()).filter(|s| !s.is_empty());
            let description =
                caps.name("description").map(|m| m.as_str().trim().to_string()).filter(|s| !s.is_empty());
            let player_arg = usage.as_deref().and_then(|u| player_arg(u, &self.player_placeholders));
            found.entry(name.to_lowercase()).or_insert(CommandSuggestion {
                name,
                usage,
                description,
                source: CommandSource::Server,
                player_arg,
            });
        }
        found.into_values().collect()
    }
}

/// Index of the first usage token that is a player placeholder.
pub fn player_arg(usage: &str, placeholders: &[String]) -> Option<usize> {
    usage
        .split_whitespace()
        .position(|token| placeholders.iter().any(|p| token.eq_ignore_ascii_case(p) || token.trim_matches(['[', ']']).eq_ignore_ascii_case(p)))
}

/// The builtin list as suggestions.
pub fn builtin(spec: &CompletionSpec) -> Vec<CommandSuggestion> {
    spec.builtin
        .iter()
        .map(|command| CommandSuggestion {
            name: command.name.clone(),
            usage: command.usage.clone(),
            description: None,
            source: CommandSource::Builtin,
            player_arg: command.usage.as_deref().and_then(|u| player_arg(u, &spec.player_placeholders)),
        })
        .collect()
}

/// Merge server-reported commands over the builtin list. A namespaced
/// duplicate (`minecraft:ban` beside `ban`) is dropped; a namespaced command
/// with no plain twin (`worldedit:wand`) is kept.
pub fn merge(builtin: Vec<CommandSuggestion>, server: Vec<CommandSuggestion>) -> Vec<CommandSuggestion> {
    let mut by_name: BTreeMap<String, CommandSuggestion> = BTreeMap::new();
    for command in builtin {
        by_name.insert(command.name.to_lowercase(), command);
    }
    for mut command in server {
        let key = command.name.to_lowercase();
        if let Some(existing) = by_name.get(&key) {
            // The server's usage can be terser than ours; keep the richer one.
            if command.usage.is_none() {
                command.usage = existing.usage.clone();
                command.player_arg = existing.player_arg;
            }
        }
        by_name.insert(key, command);
    }
    let plain: std::collections::HashSet<String> =
        by_name.keys().filter(|k| !k.contains(':')).cloned().collect();
    by_name
        .into_iter()
        .filter(|(key, _)| match key.split_once(':') {
            Some((_, bare)) => !plain.contains(bare),
            None => true,
        })
        .map(|(_, command)| command)
        .collect()
}

/// Ask a running server for its commands, hidden from the console.
async fn ask_server(service: &InstanceService, instance_id: &str, spec: &CompletionSpec) -> AppResult<Vec<CommandSuggestion>> {
    let parser = HelpParser::new(spec)?;
    let mut capture = service.processes.capture(instance_id, parser.capture_patterns())?;

    service.processes.send_internal_command(instance_id, &spec.help_command).await?;
    let mut lines = capture.collect(FIRST_LINE_WAIT, IDLE_WAIT, MAX_WAIT).await;
    if capture.interrupted() {
        return Err(AppError::new(ErrorCode::NodeBusy).with("reason", "user_command"));
    }

    // Paged answers: fetch the rest. Bukkit-family servers answer the console
    // in one page, but some forks and help plugins page it anyway.
    let pages = lines.iter().find_map(|line| parser.page_of(line)).map(|(_, pages)| pages).unwrap_or(1);
    if let Some(page_command) = &spec.page_command {
        for page in 2..=pages.min(spec.max_pages) {
            let command = page_command.replace("{page}", &page.to_string());
            service.processes.send_internal_command(instance_id, &command).await?;
            lines.extend(capture.collect(FIRST_LINE_WAIT, IDLE_WAIT, MAX_WAIT).await);
            if capture.interrupted() {
                return Err(AppError::new(ErrorCode::NodeBusy).with("reason", "user_command"));
            }
        }
    }
    drop(capture);

    Ok(parser.parse(&lines))
}

impl InstanceService {
    /// Suggestions for the console input. See the module docs for limits.
    pub async fn completions(&self, instance_id: &str, refresh: bool) -> AppResult<CompletionSet> {
        let instance = self.store.get(instance_id).await?;
        let manifest = self.registry.manifest(&instance.game)?;
        let players = self.online_players(instance_id).await;

        let Some(spec) = manifest.console.completion.clone() else {
            return Ok(CompletionSet { commands: Vec::new(), players, from_server: false, fetched_at: None });
        };
        let builtin_list = builtin(&spec);

        let online = self.state_of(instance_id).await.status == InstanceStatus::Online;
        let cached = if refresh { None } else { self.cached_completions(instance_id).await.filter(usable) };

        let cached = match cached {
            Some(cached) => Some(cached),
            None if online => {
                // One fetch at a time per server: two panels asking at once
                // must not interleave two `help`s on the same console. Other
                // servers are not held up by a slow or paged one.
                let lock = self.completion_lock(instance_id).await;
                let _guard = lock.lock().await;
                // Someone else may have fetched while we waited for the lock.
                let fresh = if refresh { None } else { self.cached_completions(instance_id).await.filter(usable) };
                match fresh {
                    Some(cached) => Some(cached),
                    None => match ask_server(self, instance_id, &spec).await {
                        Ok(commands) if !commands.is_empty() => {
                            let cached = CachedCommands { commands, fetched_at: Utc::now() };
                            self.store_completions(instance_id, cached.clone()).await;
                            Some(cached)
                        }
                        // An empty answer (a help plugin, a hidden `help`, a
                        // server too busy to answer in time) is remembered
                        // for a while, so every call does not re-ask and wait.
                        Ok(_) => {
                            self.store_completions(
                                instance_id,
                                CachedCommands { commands: Vec::new(), fetched_at: Utc::now() },
                            )
                            .await;
                            None
                        }
                        Err(error) => {
                            tracing::debug!(instance_id, %error, "help capture failed; using builtin list");
                            None
                        }
                    },
                }
            }
            None => None,
        };

        Ok(match cached.filter(|cached| !cached.commands.is_empty()) {
            Some(cached) => CompletionSet {
                commands: merge(builtin_list, cached.commands),
                players,
                from_server: true,
                fetched_at: Some(cached.fetched_at),
            },
            None => CompletionSet { commands: merge(builtin_list, Vec::new()), players, from_server: false, fetched_at: None },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::minecraft;

    fn spec() -> CompletionSpec {
        minecraft::manifest().console.completion.expect("minecraft has completion")
    }

    fn lines(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_vanilla_brigadier_help() {
        let parser = HelpParser::new(&spec()).expect("compiles");
        let out = parser.parse(&lines(&[
            "[12:00:00] [Server thread/INFO]: /advancement (grant|revoke)",
            "[12:00:00] [Server thread/INFO]: /ban <targets> [<reason>]",
            "[12:00:00] [Server thread/INFO]: /list",
            "[12:00:00] [Server thread/INFO] [minecraft/DedicatedServer]: /kick <targets> [<reason>]",
            "[12:00:00] [Server thread/INFO]: Steve joined the game",
        ]));
        let names: Vec<&str> = out.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["advancement", "ban", "kick", "list"]);
        let ban = out.iter().find(|c| c.name == "ban").expect("ban");
        assert_eq!(ban.usage.as_deref(), Some("<targets> [<reason>]"));
        assert_eq!(ban.player_arg, Some(0));
        assert_eq!(ban.source, CommandSource::Server);
    }

    #[test]
    fn parses_bukkit_index_with_descriptions_and_pages() {
        let parser = HelpParser::new(&spec()).expect("compiles");
        let header = "[12:00:00 INFO]: --------- Help: Index (1/3) --------------------";
        assert_eq!(parser.page_of(header), Some((1, 3)));
        let out = parser.parse(&lines(&[
            header,
            "[12:00:00 INFO]: Use /help [n] to get page n of help.",
            "[12:00:00 INFO]: Aliases: Lists command aliases",
            "[12:00:00 INFO]: WorldEdit: All commands for WorldEdit",
            "[12:00:00 INFO]: /ban: Prevents the specified player from using this server",
            "[12:00:00 INFO]: /essentials:home: Teleport to your home.",
            "[12:00:00 INFO]: //wand: Get the wand object",
        ]));
        let names: Vec<&str> = out.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"ban"), "{names:?}");
        assert!(names.contains(&"essentials:home"), "{names:?}");
        assert!(names.contains(&"/wand"), "WorldEdit's double-slash commands survive: {names:?}");
        assert!(!names.iter().any(|n| n.contains("Aliases") || n.contains("WorldEdit")), "{names:?}");
        let ban = out.iter().find(|c| c.name == "ban").expect("ban");
        assert_eq!(ban.description.as_deref(), Some("Prevents the specified player from using this server"));
        assert_eq!(ban.usage, None);
    }

    #[test]
    fn capture_patterns_cover_every_help_line_but_not_ordinary_output() {
        let parser = HelpParser::new(&spec()).expect("compiles");
        let patterns = parser.capture_patterns();
        let hidden = |line: &str| patterns.iter().any(|re| re.is_match(line));
        assert!(hidden("[12:00:00 INFO]: --------- Help: Index (1/3) --------------------"));
        assert!(hidden("[12:00:00 INFO]: Use /help [n] to get page n of help."));
        assert!(hidden("[12:00:00 INFO]: /ban: Prevents the specified player"));
        assert!(hidden("[12:00:00] [Server thread/INFO]: /list"));
        assert!(!hidden("[12:00:00] [Server thread/INFO]: Steve joined the game"));
        assert!(!hidden("[12:00:00] [Server thread/INFO]: <Steve> /help me"));
    }

    #[test]
    fn merge_prefers_server_entries_and_drops_namespaced_twins() {
        let s = spec();
        let server = vec![
            CommandSuggestion { name: "ban".into(), usage: None, description: Some("desc".into()), source: CommandSource::Server, player_arg: None },
            CommandSuggestion { name: "minecraft:ban".into(), usage: None, description: None, source: CommandSource::Server, player_arg: None },
            CommandSuggestion { name: "worldedit:wand".into(), usage: None, description: None, source: CommandSource::Server, player_arg: None },
        ];
        let merged = merge(builtin(&s), server);
        let ban = merged.iter().find(|c| c.name == "ban").expect("ban");
        assert_eq!(ban.source, CommandSource::Server);
        assert_eq!(ban.description.as_deref(), Some("desc"));
        assert_eq!(ban.usage.as_deref(), Some("<targets> [<reason>]"), "builtin usage fills the gap");
        assert_eq!(ban.player_arg, Some(0));
        assert!(!merged.iter().any(|c| c.name == "minecraft:ban"));
        assert!(merged.iter().any(|c| c.name == "worldedit:wand"));
        assert!(merged.iter().any(|c| c.name == "gamemode"), "builtin entries remain");
    }

    #[test]
    fn builtin_player_arguments_are_found_by_placeholder() {
        let s = spec();
        let list = builtin(&s);
        let find = |n: &str| list.iter().find(|c| c.name == n).expect(n).player_arg;
        assert_eq!(find("op"), Some(0));
        assert_eq!(find("gamemode"), Some(1));
        assert_eq!(find("stop"), None);
        assert_eq!(player_arg("[<targets>] [<item>]", &s.player_placeholders), Some(0));
    }
}
