//! Reading meaning out of a server's stdout, driven entirely by the game's
//! [`ConsoleSpec`]. This module has never heard of Minecraft.

use std::io::{BufRead, BufReader, Read};

use regex::Regex;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::manifest::ConsoleSpec;
use crate::model::{ConsoleLevel, ConsoleSpeaker, SpeakerKind};

/// What one line of output turned out to mean.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineReading {
    /// Display text, already stripped of in-band formatting.
    pub text: String,
    /// `text` without the game's own log prefix, when it had one.
    pub body: Option<String>,
    pub level: ConsoleLevel,
    /// The server just finished starting.
    pub ready: bool,
    pub joined: Option<String>,
    pub left: Option<String>,
    pub tick_rate: Option<f32>,
    /// `(name, message)` when a player said something.
    pub chat: Option<(String, String)>,
    /// `(name, uuid)` when the server announced who a player is.
    pub identity: Option<(String, String)>,
    /// False when the line is the answer to a poll the user never made.
    pub visible: bool,
    /// The server announced a normal shutdown.
    pub stopping: bool,
    /// The line is one only a crash produces.
    pub crashed: bool,
}

impl LineReading {
    /// The player this line is about, for the console's chat rendering.
    pub fn speaker(&self) -> Option<ConsoleSpeaker> {
        if let Some((name, message)) = &self.chat {
            return Some(ConsoleSpeaker {
                name: name.clone(),
                kind: SpeakerKind::Chat,
                message: Some(message.clone()),
            });
        }
        if let Some(name) = &self.joined {
            return Some(ConsoleSpeaker { name: name.clone(), kind: SpeakerKind::Join, message: None });
        }
        self.left
            .as_ref()
            .map(|name| ConsoleSpeaker { name: name.clone(), kind: SpeakerKind::Leave, message: None })
    }
}

impl Default for ConsoleLevel {
    fn default() -> Self {
        Self::Info
    }
}

/// Compiled form of a game's [`ConsoleSpec`]. Built once per instance start —
/// compiling a regex per line would show up in a busy server's CPU profile.
pub struct ConsoleParser {
    ready: Regex,
    join: Option<Regex>,
    leave: Option<Regex>,
    chat: Option<Regex>,
    identity: Option<Regex>,
    tick_rate: Vec<Regex>,
    suppress: Vec<Regex>,
    log_prefix: Option<Regex>,
    stopping: Option<Regex>,
    crash: Option<Regex>,
    strip_colour_codes: bool,
}

impl ConsoleParser {
    pub fn new(spec: &ConsoleSpec) -> AppResult<Self> {
        Ok(Self {
            ready: compile(&spec.ready_pattern)?,
            join: spec.join_pattern.as_deref().map(compile).transpose()?,
            leave: spec.leave_pattern.as_deref().map(compile).transpose()?,
            chat: spec.chat_pattern.as_deref().map(compile).transpose()?,
            identity: spec.identity_pattern.as_deref().map(compile).transpose()?,
            tick_rate: spec.tick_rate_patterns.iter().map(|p| compile(p)).collect::<Result<_, _>>()?,
            suppress: spec.suppress_patterns.iter().map(|p| compile(p)).collect::<Result<_, _>>()?,
            log_prefix: spec.log_prefix_pattern.as_deref().map(compile).transpose()?,
            stopping: spec.stopping_pattern.as_deref().map(compile).transpose()?,
            crash: spec.crash_pattern.as_deref().map(compile).transpose()?,
            strip_colour_codes: spec.strip_colour_codes,
        })
    }

    pub fn read(&self, raw: &str) -> LineReading {
        let text = if self.strip_colour_codes { strip_formatting(raw) } else { raw.to_string() };

        let tick_rate = self
            .tick_rate
            .iter()
            .find_map(|re| re.captures(&text))
            .and_then(|caps| caps.name("value"))
            .and_then(|m| m.as_str().parse::<f32>().ok());

        let chat = self.chat.as_ref().and_then(|re| {
            let caps = re.captures(&text)?;
            Some((
                caps.name("name")?.as_str().to_string(),
                caps.name("message").map(|m| m.as_str().to_string()).unwrap_or_default(),
            ))
        });
        let identity = self.identity.as_ref().and_then(|re| {
            let caps = re.captures(&text)?;
            Some((caps.name("name")?.as_str().to_string(), caps.name("uuid")?.as_str().to_string()))
        });
        // Whatever a player types is theirs: "<Bob> Steve joined the game" is
        // chat, and must not put a phantom Steve on the online list.
        let is_chat = chat.is_some();
        let body = self.log_prefix.as_ref().and_then(|re| {
            let found = re.find(&text)?;
            (found.start() == 0 && found.end() > 0 && found.end() < text.len())
                .then(|| text[found.end()..].to_string())
        });

        LineReading {
            level: classify_level(&text),
            ready: !is_chat && self.ready.is_match(&text),
            joined: if is_chat { None } else { capture_name(&self.join, &text) },
            left: if is_chat { None } else { capture_name(&self.leave, &text) },
            tick_rate: if is_chat { None } else { tick_rate },
            chat,
            identity: if is_chat { None } else { identity },
            visible: !self.suppress.iter().any(|re| re.is_match(&text)),
            stopping: !is_chat && self.stopping.as_ref().is_some_and(|re| re.is_match(&text)),
            crashed: !is_chat && self.crash.as_ref().is_some_and(|re| re.is_match(&text)),
            body,
            text,
        }
    }
}

fn compile(pattern: &str) -> AppResult<Regex> {
    Regex::new(pattern).map_err(|error| {
        AppError::new(ErrorCode::InvalidInput)
            .with("pattern", pattern)
            .detail(error)
    })
}

fn capture_name(re: &Option<Regex>, text: &str) -> Option<String> {
    re.as_ref()?
        .captures(text)?
        .name("name")
        .map(|m| m.as_str().to_string())
}

/// Strip Minecraft-style `§X` codes and ANSI escape sequences.
pub fn strip_formatting(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '§' {
            chars.next(); // the colour digit/letter
        } else if ch == '\x1b' {
            // ANSI escape: ESC [ <digits;digits…> <letter>
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Severity, read from the common `[…/LEVEL]` shape most server logs use.
fn classify_level(line: &str) -> ConsoleLevel {
    let head = line.get(..120).unwrap_or(line).to_ascii_uppercase();
    let has = |token: &str| {
        head.contains(&format!("/{token}]"))
            || head.contains(&format!(" {token}]"))
            || head.contains(&format!("/{token}):"))
            || head.contains(&format!(" {token}):"))
    };
    if has("ERROR") || has("FATAL") || has("SEVERE") {
        ConsoleLevel::Error
    } else if has("WARN") {
        ConsoleLevel::Warn
    } else {
        ConsoleLevel::Info
    }
}

/// Read one line from a child's stdout, repairing the encoding servers on
/// Windows produce.
///
/// Minecraft on a CP1252 console emits a lone `0xA7` for `§`, which is invalid
/// UTF-8 and would otherwise turn the whole line into replacement characters.
pub fn read_line_lossy<R: Read>(reader: &mut BufReader<R>, buf: &mut Vec<u8>) -> Option<String> {
    buf.clear();
    match reader.read_until(b'\n', buf) {
        Ok(0) => None, // EOF
        Ok(_) => {
            while matches!(buf.last(), Some(b'\n') | Some(b'\r')) {
                buf.pop();
            }
            Some(decode_line(buf))
        }
        Err(_) => None,
    }
}

fn decode_line(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => {
            let mut fixed = Vec::with_capacity(bytes.len() + 32);
            for &b in bytes {
                match b {
                    0xA7 => fixed.extend_from_slice(&[0xC2, 0xA7]), // § in UTF-8
                    0x80.. => fixed.push(b'?'),
                    _ => fixed.push(b),
                }
            }
            String::from_utf8_lossy(&fixed).into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::minecraft;

    fn minecraft_parser() -> ConsoleParser {
        ConsoleParser::new(&minecraft::manifest().console).expect("manifest patterns compile")
    }

    #[test]
    fn detects_a_normal_shutdown_however_it_was_asked_for() {
        let parser = minecraft_parser();
        // `stop` typed in the console.
        assert!(parser.read("[22:41:54] [Server thread/INFO]: Stopping the server").stopping);
        // A player's `/stop`, echoed to the log as an admin command.
        assert!(parser.read("[22:41:54] [Server thread/INFO]: [Steve: Stopping the server]").stopping);
        // The shutdown itself.
        assert!(parser.read("[22:41:54] [Server thread/INFO]: Stopping server").stopping);
        assert!(!parser.read("[22:41:54] [Server thread/INFO]: Done (1.0s)!").stopping);
    }

    #[test]
    fn chat_cannot_fake_a_shutdown_or_a_crash() {
        let parser = minecraft_parser();
        let said = parser.read("[22:41:54] [Server thread/INFO]: <Bob> Stopping the server");
        assert!(!said.stopping);
        let said = parser.read("[22:41:54] [Server thread/INFO]: <Bob> This crash report has been saved to");
        assert!(!said.crashed);
    }

    #[test]
    fn detects_crash_only_lines() {
        let parser = minecraft_parser();
        assert!(parser.read("[22:41:54] [Server thread/ERROR]: Encountered an unexpected exception").crashed);
        assert!(parser
            .read("[22:41:54] [Server thread/ERROR]: This crash report has been saved to: ./crash-reports/x.txt")
            .crashed);
        // Paper / Spigot watchdog: the server froze. It then shuts down
        // through the normal path and may exit 0.
        assert!(parser
            .read("[22:41:54 ERROR]: The server has stopped responding! This is (probably) not a Paper bug.")
            .crashed);
        assert!(!parser.read("[22:41:54] [Server thread/INFO]: Stopping server").crashed);
    }

    #[test]
    fn detects_the_ready_line() {
        let parser = minecraft_parser();
        let line = "[22:41:54] [Server thread/INFO]: Done (12.345s)! For help, type \"help\"";
        assert!(parser.read(line).ready);
        assert!(!parser.read("[22:41:54] [Server thread/INFO]: Preparing spawn area").ready);
    }

    #[test]
    fn reads_joins_and_leaves() {
        let parser = minecraft_parser();
        let joined = parser.read("[12:00:00] [Server thread/INFO]: Gabrieli2806 joined the game");
        assert_eq!(joined.joined.as_deref(), Some("Gabrieli2806"));
        let left = parser.read("[12:00:00] [Server thread/INFO]: Gabrieli2806 left the game");
        assert_eq!(left.left.as_deref(), Some("Gabrieli2806"));
    }

    #[test]
    fn parses_paper_single_line_tick_rate() {
        let parser = minecraft_parser();
        let line = "[12:00:00 INFO]: TPS from last 1m, 5m, 15m: *20.0, 19.98, 20.0";
        assert_eq!(parser.read(line).tick_rate, Some(20.0));
    }

    #[test]
    fn parses_modern_neoforge_overall_row() {
        let parser = minecraft_parser();
        let line = "[22:41:54] [Server thread/INFO] [minecraft/MinecraftServer]: Overall: 20.000 TPS (7.014 ms/tick)";
        assert_eq!(parser.read(line).tick_rate, Some(20.0));
    }

    #[test]
    fn per_dimension_rows_do_not_overwrite_the_server_figure_but_stay_hidden() {
        let parser = minecraft_parser();
        for line in [
            "[22:41:54] [Server thread/INFO] [minecraft/MinecraftServer]: Overworld: 20.000 TPS (6.758 ms/tick)",
            "[22:41:54] [Server thread/INFO] [minecraft/MinecraftServer]: The End: 19.500 TPS (0.041 ms/tick)",
            "[22:41:54] [Server thread/INFO] [minecraft/MinecraftServer]: The Nether: 20.000 TPS (0.011 ms/tick)",
        ] {
            let reading = parser.read(line);
            assert_eq!(reading.tick_rate, None, "{line}");
            assert!(!reading.visible, "{line}");
        }
    }

    #[test]
    fn parses_legacy_forge_mean_line() {
        let parser = minecraft_parser();
        let line = "Overall : Mean tick time: 2.1 ms. Mean TPS: 20.00";
        assert_eq!(parser.read(line).tick_rate, Some(20.0));
    }

    #[test]
    fn ordinary_lines_are_visible_and_carry_no_tick_rate() {
        let parser = minecraft_parser();
        for line in [
            "[22:41:54] [Server thread/INFO]: Done (12.345s)! For help, type \"help\"",
            "[22:41:54] [Server thread/INFO]: Gabrieli2806 joined the game",
        ] {
            let reading = parser.read(line);
            assert_eq!(reading.tick_rate, None, "{line}");
            assert!(reading.visible, "{line}");
        }
    }

    #[test]
    fn reads_vanilla_and_paper_chat_lines() {
        let parser = minecraft_parser();
        let vanilla = parser.read("[12:00:00] [Server thread/INFO]: <Steve> hola a todos");
        assert_eq!(vanilla.chat, Some(("Steve".into(), "hola a todos".into())));
        let paper = parser.read("[12:00:00 INFO]: [Async Chat Thread - #0/INFO]: <Alex_2> hi");
        assert_eq!(paper.chat.as_ref().map(|c| c.0.as_str()), Some("Alex_2"));
        let unsigned = parser.read("[12:00:00] [Server thread/INFO]: [Not Secure] <Steve> yo");
        assert_eq!(unsigned.chat, Some(("Steve".into(), "yo".into())));
        let speaker = vanilla.speaker().expect("a speaker");
        assert_eq!(speaker.kind, SpeakerKind::Chat);
        assert_eq!(speaker.message.as_deref(), Some("hola a todos"));
    }

    #[test]
    fn display_body_drops_the_game_log_prefix() {
        let parser = minecraft_parser();
        let body = |line: &str| parser.read(line).body;
        assert_eq!(body("[12:00:01] [Server thread/INFO]: Saved the game").as_deref(), Some("Saved the game"));
        assert_eq!(body("[12:00:01 INFO]: Saved the game").as_deref(), Some("Saved the game"));
        assert_eq!(
            body("[12:00:01 WARN]: [EssentialsX] Old config").as_deref(),
            Some("[EssentialsX] Old config"),
            "a plugin tag is the message, not the prefix"
        );
        assert_eq!(
            body("[22:41:54] [Server thread/INFO] [minecraft/MinecraftServer]: Overworld ok").as_deref(),
            Some("Overworld ok")
        );
        assert_eq!(body("at java.base/java.lang.Thread.run(Thread.java:1583)"), None);
        // The level still comes from the raw line.
        assert_eq!(parser.read("[12:00:01 WARN]: Can't keep up!").level, ConsoleLevel::Warn);
    }

    #[test]
    fn chat_that_looks_like_a_join_is_still_chat() {
        let parser = minecraft_parser();
        let reading = parser.read("[12:00:00] [Server thread/INFO]: <Bob> Steve joined the game");
        assert_eq!(reading.joined, None, "a player typed it; nobody joined");
        assert_eq!(reading.chat.as_ref().map(|c| c.0.as_str()), Some("Bob"));
        let done = parser.read("[12:00:00] [Server thread/INFO]: <Bob> Done (1.0s)!");
        assert!(!done.ready, "chat must not mark the server ready");
    }

    #[test]
    fn join_and_leave_lines_carry_a_speaker() {
        let parser = minecraft_parser();
        let joined = parser.read("[12:00:00] [Server thread/INFO]: Steve joined the game");
        assert_eq!(joined.speaker().map(|s| s.kind), Some(SpeakerKind::Join));
        let left = parser.read("[12:00:00] [Server thread/INFO]: Steve left the game");
        assert_eq!(left.speaker().map(|s| s.kind), Some(SpeakerKind::Leave));
        assert_eq!(parser.read("[12:00:00] [Server thread/INFO]: Saved the game").speaker(), None);
    }

    #[test]
    fn reads_the_uuid_announcement() {
        let parser = minecraft_parser();
        let line = "[12:00:00] [User Authenticator #1/INFO]: UUID of player Steve is 8667ba71-b85a-4004-af54-457a9734eed7";
        assert_eq!(
            parser.read(line).identity,
            Some(("Steve".into(), "8667ba71-b85a-4004-af54-457a9734eed7".into()))
        );
    }

    #[test]
    fn classifies_severity() {
        let parser = minecraft_parser();
        assert_eq!(parser.read("[12:00:00] [main/WARN]: careful").level, ConsoleLevel::Warn);
        assert_eq!(parser.read("[12:00:00] [main/ERROR]: broken").level, ConsoleLevel::Error);
        assert_eq!(parser.read("[12:00:00] [main/INFO]: fine").level, ConsoleLevel::Info);
    }

    #[test]
    fn strips_colour_codes_and_ansi() {
        assert_eq!(strip_formatting("§aGreen §ltext"), "Green text");
        assert_eq!(strip_formatting("\x1b[33myellow\x1b[0m"), "yellow");
    }

    #[test]
    fn repairs_cp1252_section_sign() {
        assert_eq!(decode_line(&[b'a', 0xA7, b'b']), "a§b");
    }

    #[test]
    fn reads_lines_without_trailing_newlines() {
        let data = b"first\r\nsecond\n".to_vec();
        let mut reader = BufReader::new(std::io::Cursor::new(data));
        let mut buf = Vec::new();
        assert_eq!(read_line_lossy(&mut reader, &mut buf).as_deref(), Some("first"));
        assert_eq!(read_line_lossy(&mut reader, &mut buf).as_deref(), Some("second"));
        assert_eq!(read_line_lossy(&mut reader, &mut buf), None);
    }
}
