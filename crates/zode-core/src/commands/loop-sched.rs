//! `/loop` and `/schedule` slash-command parsers.
//!
//! `/loop` drives a session-local recurring prompt (not persisted); `/schedule`
//! manages persisted `ScheduleSpec` jobs (Task 6/7). Both parsers replicate the
//! exact-prefix + whitespace pattern from `team.rs` so `/loopx` / `/schedulex`
//! never parse.

use crate::scheduler::ScheduleSpec;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopCommand {
    Start {
        interval: Duration,
        prompt: String,
        max_runs: Option<u32>,
    },
    List,
    Stop(Option<u32>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleCommand {
    Add { spec: ScheduleSpec, prompt: String },
    List,
    Rm(String),
    Enable(String),
    Disable(String),
}

/// Strip the exact `/loop` prefix, matching `team.rs`'s exact-prefix +
/// whitespace pattern (`/loopx` must not parse).
fn strip_command<'a>(input: &'a str, name: &str) -> Option<&'a str> {
    let rest = input.strip_prefix(name)?;
    match rest {
        "" => Some(""),
        r if r.starts_with(char::is_whitespace) => Some(r.trim()),
        _ => None,
    }
}

/// Parse a hand-rolled numeric-prefix + unit-suffix (`s`/`m`/`h`) duration,
/// e.g. `30s`, `5m`, `1h`. Rejects anything under 30 seconds.
pub fn parse_interval(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (digits, unit) = match s.rfind(|c: char| c.is_ascii_digit()) {
        Some(idx) => s.split_at(idx + 1),
        None => return Err("invalid interval, expected e.g. 30s, 5m, 1h".to_string()),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err("invalid interval, expected e.g. 30s, 5m, 1h".to_string());
    }
    let n: u64 = digits
        .parse()
        .map_err(|_| "invalid interval, expected e.g. 30s, 5m, 1h".to_string())?;
    let secs = match unit {
        "s" => n,
        "m" => n
            .checked_mul(60)
            .ok_or_else(|| "invalid interval, expected e.g. 30s, 5m, 1h".to_string())?,
        "h" => n
            .checked_mul(3600)
            .ok_or_else(|| "invalid interval, expected e.g. 30s, 5m, 1h".to_string())?,
        _ => return Err("invalid interval, expected e.g. 30s, 5m, 1h".to_string()),
    };
    if secs < 30 {
        return Err("minimum interval is 30s".to_string());
    }
    Ok(Duration::from_secs(secs))
}

fn parse_weekday(s: &str) -> Option<chrono::Weekday> {
    match s {
        "mon" => Some(chrono::Weekday::Mon),
        "tue" => Some(chrono::Weekday::Tue),
        "wed" => Some(chrono::Weekday::Wed),
        "thu" => Some(chrono::Weekday::Thu),
        "fri" => Some(chrono::Weekday::Fri),
        "sat" => Some(chrono::Weekday::Sat),
        "sun" => Some(chrono::Weekday::Sun),
        _ => None,
    }
}

/// Parse `hh:mm`, validating `hour < 24 && minute < 60`.
fn parse_hh_mm(s: &str) -> Result<(u32, u32), String> {
    let (h, m) = s
        .split_once(':')
        .ok_or_else(|| "invalid time, expected hh:mm".to_string())?;
    let hour: u32 = h
        .parse()
        .map_err(|_| "invalid time, expected hh:mm".to_string())?;
    let minute: u32 = m
        .parse()
        .map_err(|_| "invalid time, expected hh:mm".to_string())?;
    if hour >= 24 || minute >= 60 {
        return Err("invalid time, expected hh:mm".to_string());
    }
    Ok((hour, minute))
}

/// Job prompts must be plain prompts, never slash commands.
///
/// The TUI's slash dispatch (`handle_slash`) is unconditionally scoped to the
/// *active* tab, so a `/loop` owned by a background tab whose prompt were a
/// slash command would run that command against whatever tab the user happens
/// to be looking at; and the slash paths that return without starting a turn
/// have nowhere to hand back the scheduler's pending-attribution entry, which
/// then leaks and misattributes a later, unrelated turn. Rejecting here is the
/// one place that makes the background and active injection paths agree, and
/// it fails at `/loop`/`/schedule add` time — loudly, while the user is
/// looking — rather than silently sending literal `"/compact"` to the model
/// every interval, or persisting a schedule that can never run correctly.
fn reject_slash_prompt(prompt: &str) -> Result<(), String> {
    if prompt.trim_start().starts_with('/') {
        return Err(
            "job prompts must be plain prompts, not slash commands — drop the leading '/'"
                .to_string(),
        );
    }
    Ok(())
}

const LOOP_USAGE: &str = "usage: /loop <30s|5m|1h> [--max N] <prompt> | list | stop [id]";
const SCHEDULE_USAGE: &str =
    "usage: /schedule add <hh:mm|mon hh:mm|every Nh> <prompt> | list | rm <id> | enable|disable <id>";

/// Parse `/loop <interval> [--max N] <prompt>`, `/loop list`, or
/// `/loop stop [id]`.
pub fn parse_loop(input: &str) -> Result<LoopCommand, String> {
    let rest = strip_command(input, "/loop").ok_or_else(|| LOOP_USAGE.to_string())?;
    if rest == "list" {
        return Ok(LoopCommand::List);
    }
    if rest == "stop" {
        return Ok(LoopCommand::Stop(None));
    }
    if let Some(id) = rest.strip_prefix("stop") {
        let id = id.trim();
        if !id.is_empty() {
            let n: u32 = id
                .parse()
                .map_err(|_| "usage: /loop stop [id]".to_string())?;
            return Ok(LoopCommand::Stop(Some(n)));
        }
    }

    let (interval_str, tail) = rest
        .split_once(char::is_whitespace)
        .ok_or_else(|| LOOP_USAGE.to_string())?;
    let interval = parse_interval(interval_str)?;
    let tail = tail.trim();

    let (max_runs, prompt) = if let Some(after_max) = tail.strip_prefix("--max") {
        if !after_max.starts_with(char::is_whitespace) {
            return Err(LOOP_USAGE.to_string());
        }
        let after_max = after_max.trim_start();
        let (n_str, prompt) = after_max
            .split_once(char::is_whitespace)
            .ok_or_else(|| LOOP_USAGE.to_string())?;
        let n: u32 = n_str
            .trim()
            .parse()
            .map_err(|_| "invalid --max value".to_string())?;
        (Some(n), prompt.trim())
    } else {
        (None, tail)
    };

    if prompt.is_empty() {
        return Err(LOOP_USAGE.to_string());
    }
    reject_slash_prompt(prompt)?;

    Ok(LoopCommand::Start {
        interval,
        prompt: prompt.to_string(),
        max_runs,
    })
}

/// Parse `/schedule add ...`, `/schedule list`, `/schedule rm <id>`,
/// `/schedule enable <id>`, or `/schedule disable <id>`.
pub fn parse_schedule(input: &str) -> Result<ScheduleCommand, String> {
    let rest = strip_command(input, "/schedule").ok_or_else(|| SCHEDULE_USAGE.to_string())?;
    let (head, arg) = match rest.split_once(char::is_whitespace) {
        Some((h, a)) => (h, a.trim()),
        None => (rest, ""),
    };

    match head {
        "list" => Ok(ScheduleCommand::List),
        "rm" if !arg.is_empty() => Ok(ScheduleCommand::Rm(arg.to_string())),
        "enable" if !arg.is_empty() => Ok(ScheduleCommand::Enable(arg.to_string())),
        "disable" if !arg.is_empty() => Ok(ScheduleCommand::Disable(arg.to_string())),
        "add" => parse_schedule_add(arg),
        _ => Err(SCHEDULE_USAGE.to_string()),
    }
}

fn parse_schedule_add(arg: &str) -> Result<ScheduleCommand, String> {
    if arg.is_empty() {
        return Err(SCHEDULE_USAGE.to_string());
    }

    let (head, tail) = arg
        .split_once(char::is_whitespace)
        .ok_or_else(|| SCHEDULE_USAGE.to_string())?;
    let tail = tail.trim();

    if head == "every" {
        let (interval_str, prompt) = tail
            .split_once(char::is_whitespace)
            .ok_or_else(|| SCHEDULE_USAGE.to_string())?;
        let interval = parse_interval(interval_str)?;
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(SCHEDULE_USAGE.to_string());
        }
        reject_slash_prompt(prompt)?;
        return Ok(ScheduleCommand::Add {
            spec: ScheduleSpec::Interval {
                secs: interval.as_secs(),
            },
            prompt: prompt.to_string(),
        });
    }

    if let Some(weekday) = parse_weekday(head) {
        let (hh_mm, prompt) = tail
            .split_once(char::is_whitespace)
            .ok_or_else(|| SCHEDULE_USAGE.to_string())?;
        let (hour, minute) = parse_hh_mm(hh_mm)?;
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(SCHEDULE_USAGE.to_string());
        }
        reject_slash_prompt(prompt)?;
        return Ok(ScheduleCommand::Add {
            spec: ScheduleSpec::Weekly {
                weekday,
                hour,
                minute,
            },
            prompt: prompt.to_string(),
        });
    }

    // Otherwise `head` must be an `hh:mm` daily time.
    let (hour, minute) = parse_hh_mm(head)?;
    let prompt = tail.trim();
    if prompt.is_empty() {
        return Err(SCHEDULE_USAGE.to_string());
    }
    reject_slash_prompt(prompt)?;
    Ok(ScheduleCommand::Add {
        spec: ScheduleSpec::Daily { hour, minute },
        prompt: prompt.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_parsing() {
        assert_eq!(
            parse_loop("/loop 5m check the deploy"),
            Ok(LoopCommand::Start {
                interval: Duration::from_secs(300),
                prompt: "check the deploy".into(),
                max_runs: None,
            })
        );
        assert_eq!(
            parse_loop("/loop 1h --max 3 rotate logs"),
            Ok(LoopCommand::Start {
                interval: Duration::from_secs(3600),
                prompt: "rotate logs".into(),
                max_runs: Some(3),
            })
        );
        assert_eq!(parse_loop("/loop list"), Ok(LoopCommand::List));
        assert_eq!(parse_loop("/loop stop"), Ok(LoopCommand::Stop(None)));
        assert_eq!(parse_loop("/loop stop 2"), Ok(LoopCommand::Stop(Some(2))));
        assert!(
            parse_loop("/loop 10s too fast").is_err(),
            "sub-30s rejected"
        );
        assert!(parse_loop("/loop 5m").is_err(), "prompt required");
        assert!(parse_loop("/loopx 5m x").is_err(), "exact command only");
    }

    #[test]
    fn schedule_parsing() {
        assert_eq!(
            parse_schedule("/schedule add 09:00 standup notes"),
            Ok(ScheduleCommand::Add {
                spec: ScheduleSpec::Daily { hour: 9, minute: 0 },
                prompt: "standup notes".into(),
            })
        );
        assert_eq!(
            parse_schedule("/schedule add mon 09:30 weekly report"),
            Ok(ScheduleCommand::Add {
                spec: ScheduleSpec::Weekly {
                    weekday: chrono::Weekday::Mon,
                    hour: 9,
                    minute: 30,
                },
                prompt: "weekly report".into(),
            })
        );
        assert_eq!(
            parse_schedule("/schedule add every 2h sync upstream"),
            Ok(ScheduleCommand::Add {
                spec: ScheduleSpec::Interval { secs: 7200 },
                prompt: "sync upstream".into(),
            })
        );
        assert_eq!(parse_schedule("/schedule list"), Ok(ScheduleCommand::List));
        assert_eq!(
            parse_schedule("/schedule rm ab12"),
            Ok(ScheduleCommand::Rm("ab12".into()))
        );
        assert_eq!(
            parse_schedule("/schedule enable ab12"),
            Ok(ScheduleCommand::Enable("ab12".into()))
        );
        assert!(
            parse_schedule("/schedule add 25:00 x").is_err(),
            "invalid time"
        );
        assert!(
            parse_schedule("/schedule add 09:00").is_err(),
            "prompt required"
        );
    }

    /// A slash-command job prompt is rejected at parse time — the one place
    /// that keeps the active-tab and background injection paths in agreement.
    #[test]
    fn slash_command_prompts_are_rejected() {
        for input in [
            "/loop 5m /compact",
            "/loop 1h --max 3 /cost",
            "/schedule add 09:00 /compact",
            "/schedule add mon 09:30 /compact",
            "/schedule add every 2h /compact",
        ] {
            let err = match (parse_loop(input), parse_schedule(input)) {
                (Err(e), _) if input.starts_with("/loop") => e,
                (_, Err(e)) => e,
                other => panic!("{input} should be rejected, got {other:?}"),
            };
            assert!(
                err.contains("slash commands"),
                "{input}: error should explain why, got {err:?}"
            );
        }
        // Plain prompts that merely CONTAIN a slash are still fine.
        assert!(parse_loop("/loop 5m check src/main.rs").is_ok());
        assert!(parse_schedule("/schedule add 09:00 read docs/plan.md").is_ok());
    }

    #[test]
    fn interval_overflow_is_rejected() {
        assert!(parse_interval("18446744073709551615h").is_err());
        assert!(parse_loop("/loop 18446744073709551615h x").is_err());
    }
}
