//! Session ledger — the mid-term "working state" layer between the live
//! transcript (L1) and durable noema memory (L3).
//!
//! Compaction is a single lossy LLM call made under pressure: whatever the
//! summary omits vanishes from working context, and noema recall is
//! retrieval-dependent. The ledger is the write-ahead answer: state is
//! recorded HERE as it happens — every real user request verbatim, every
//! shell command head with its latest outcome, every analysis bullet the
//! compaction pipeline produces — so the post-compaction restore message
//! can re-inject facts deterministically instead of hoping the summary
//! kept them.
//!
//! Session-scoped by design: carried across engine rebuilds via
//! `CarryState`, never persisted to disk (a resumed transcript carries its
//! own context; the ledger rebuilds as the session continues).

use std::sync::{Arc, Mutex};

use agent::compact::{SessionMemoryEntry, SessionMemoryKind};
use agent::hook::{HookEvent, HookOutcome, RustHookHandler};

/// User requests kept verbatim. The FIRST request usually states the goal,
/// so eviction keeps it and drops the second-oldest instead.
const MAX_REQUESTS: usize = 30;
/// Per-request excerpt cap — enough to restate intent, not a transcript.
const REQUEST_MAX_CHARS: usize = 400;
/// Distinct command heads tracked (oldest evicted beyond this).
const MAX_COMMANDS: usize = 32;
/// Accumulated analysis notes (oldest evicted beyond this).
const MAX_NOTES: usize = 120;
const NOTE_MAX_CHARS: usize = 300;
/// Rendering budget in characters (~2k tokens) — the ledger must inform
/// the post-compact context, not refill it.
const RENDER_MAX_CHARS: usize = 8_000;

#[derive(Debug, Clone, Copy)]
struct CommandStat {
    runs: u32,
    last_ok: bool,
}

#[derive(Debug, Default)]
struct Inner {
    /// Real user prompts, oldest first, deduped against the previous one
    /// (scheduler loops re-send identical text).
    requests: Vec<String>,
    /// Normalized shell-command heads in first-seen order.
    commands: Vec<(String, CommandStat)>,
    /// Analysis bullets teed from the compaction memory sink, deduped by
    /// exact text.
    notes: Vec<(SessionMemoryKind, String)>,
}

/// Host-maintained session working-state ledger. Cheap to clone (shared
/// interior), poisoning-tolerant (every operation is best-effort).
#[derive(Debug, Clone, Default)]
pub struct SessionLedger {
    inner: Arc<Mutex<Inner>>,
}

impl SessionLedger {
    /// Record one real user request (the raw prompt text, before reminder
    /// injection). Blank and consecutive-duplicate prompts are skipped.
    pub fn note_user_request(&self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let excerpt = truncate_chars(trimmed, REQUEST_MAX_CHARS);
        let Ok(mut g) = self.inner.lock() else { return };
        if g.requests.last().map(String::as_str) == Some(excerpt.as_str()) {
            return;
        }
        g.requests.push(excerpt);
        if g.requests.len() > MAX_REQUESTS {
            // Keep the goal-stating first request; drop the second-oldest.
            g.requests.remove(1);
        }
    }

    /// Record one executed shell command by its normalized head.
    pub fn note_command(&self, command: &str, ok: bool) {
        let head = command_head(command);
        if head.is_empty() {
            return;
        }
        let Ok(mut g) = self.inner.lock() else { return };
        match g.commands.iter_mut().find(|(h, _)| *h == head) {
            Some((_, stat)) => {
                stat.runs = stat.runs.saturating_add(1);
                stat.last_ok = ok;
            }
            None => {
                g.commands.push((
                    head,
                    CommandStat {
                        runs: 1,
                        last_ok: ok,
                    },
                ));
                if g.commands.len() > MAX_COMMANDS {
                    g.commands.remove(0);
                }
            }
        }
    }

    /// Tee one compaction analysis bullet into the ledger. All kinds are
    /// kept — an OPEN QUESTION is exactly the working state a fresh
    /// post-compact context needs, even though noema drops it as
    /// non-durable.
    pub fn note_entry(&self, entry: &SessionMemoryEntry) {
        let text = entry.text.trim();
        if text.is_empty() {
            return;
        }
        let text = truncate_chars(text, NOTE_MAX_CHARS);
        let Ok(mut g) = self.inner.lock() else { return };
        if g.notes.iter().any(|(_, t)| *t == text) {
            return;
        }
        g.notes.push((entry.kind, text));
        if g.notes.len() > MAX_NOTES {
            g.notes.remove(0);
        }
    }

    /// Hook handler feeding the command section from every finished shell
    /// call. Registered beside the reminder tracker's hook.
    pub fn hook(&self) -> RustHookHandler {
        let ledger = self.clone();
        RustHookHandler::new("session-ledger", move |event| {
            if let HookEvent::AfterToolUse {
                tool, input, ok, ..
            } = event
            {
                if matches!(tool.as_str(), "Bash" | "Shell" | "BashRun") {
                    if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                        ledger.note_command(cmd, *ok);
                    }
                }
            }
            HookOutcome::Ok
        })
    }

    /// Known (accepted) impurity: host-synthesized prompts (the goal-loop
    /// continue nudge, scheduler job prompts) arrive through the same turn
    /// path as human input and are recorded as "user requests". Consecutive
    /// dedup collapses the repeats, and the harness cannot reliably tell
    /// them apart at this layer without threading an origin flag through
    /// every turn API — not worth it for a display-only ledger line.
    ///
    /// Wipe everything — `/clear` starts a new conversation on the same
    /// engine, and a fresh session must not have last session's narrative
    /// restored after its first compaction.
    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            *g = Inner::default();
        }
    }

    /// Render the ledger for the post-compaction restore message. `None`
    /// when nothing has been recorded. Sections are ordered by how hard
    /// they are to rediscover: requests (irrecoverable) first, notes, then
    /// command stats. The whole render is capped at [`RENDER_MAX_CHARS`],
    /// trimming trailing sections rather than truncating mid-line.
    pub fn render(&self) -> Option<String> {
        let g = self.inner.lock().ok()?;
        if g.requests.is_empty() && g.commands.is_empty() && g.notes.is_empty() {
            return None;
        }
        let mut out = String::from(
            "[Session ledger — harness-maintained; recorded as it happened, \
             so it is authoritative over the compaction summary]\n",
        );
        // Budget in CHARS, matching the sibling `*_MAX_CHARS` constants and
        // the token-cap intent — `String::len()` counts BYTES and would
        // under-fill a CJK-heavy ledger ~3x.
        let mut used = out.chars().count();
        let push_line = |out: &mut String, used: &mut usize, line: &str| -> bool {
            let line_chars = line.chars().count() + 1;
            if *used + line_chars > RENDER_MAX_CHARS {
                return false;
            }
            out.push_str(line);
            out.push('\n');
            *used += line_chars;
            true
        };
        if !g.requests.is_empty() {
            let _ = push_line(
                &mut out,
                &mut used,
                "## User requests this session (oldest first)",
            );
            for (i, req) in g.requests.iter().enumerate() {
                let line = format!("{}. {}", i + 1, req.replace('\n', " "));
                if !push_line(&mut out, &mut used, &line) {
                    break;
                }
            }
        }
        if !g.notes.is_empty() {
            let _ = push_line(&mut out, &mut used, "## Working notes");
            for (kind, text) in &g.notes {
                let line = format!("- {}: {}", kind_label(*kind), text.replace('\n', " "));
                if !push_line(&mut out, &mut used, &line) {
                    break;
                }
            }
        }
        if !g.commands.is_empty() {
            let _ = push_line(
                &mut out,
                &mut used,
                "## Commands run (head · runs · last result)",
            );
            for (head, stat) in &g.commands {
                let line = format!(
                    "- `{head}` · {}× · last: {}",
                    stat.runs,
                    if stat.last_ok { "ok" } else { "FAILED" }
                );
                if !push_line(&mut out, &mut used, &line) {
                    break;
                }
            }
        }
        Some(out)
    }
}

fn kind_label(kind: SessionMemoryKind) -> &'static str {
    match kind {
        SessionMemoryKind::Decision => "DECISION",
        SessionMemoryKind::Constraint => "CONSTRAINT",
        SessionMemoryKind::Requirement => "REQUIREMENT",
        SessionMemoryKind::Observation => "OBSERVATION",
        SessionMemoryKind::OpenQuestion => "OPEN QUESTION",
    }
}

/// Normalized "head" of a shell command — the zode-side twin of the query
/// loop's repeat-guard normalizer: strip `cd <path> &&` hops, cut at the
/// first pipe/redirect, keep the first four tokens.
fn command_head(command: &str) -> String {
    let mut rest = command.trim();
    while let Some(stripped) = rest.strip_prefix("cd ") {
        let Some((_, after)) = stripped.split_once("&&") else {
            break;
        };
        rest = after.trim_start();
    }
    let action = rest.split(['|', '>']).next().unwrap_or(rest);
    let mut tokens: Vec<&str> = action.split_whitespace().take(4).collect();
    // An fd redirect (`2>&1`, `2>err`) splits at '>' and strands the fd
    // digits as a trailing token — `cargo build 2>&1` must bucket as
    // `cargo build`, not `cargo build 2`. Only when the cut really was a
    // '>' (a trailing number before a PIPE is genuine command text).
    if rest[action.len()..].starts_with('>')
        && tokens.len() > 1
        && tokens
            .last()
            .is_some_and(|t| t.chars().all(|c| c.is_ascii_digit()))
    {
        tokens.pop();
    }
    tokens.join(" ")
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    format!("{}…", value.chars().take(max).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::hook::HookHandler;

    #[test]
    fn command_head_drops_stranded_fd_redirect_digits() {
        // `2>&1` splits at '>' and would strand the fd digit in the bucket key.
        assert_eq!(command_head("cargo build 2>&1"), "cargo build");
        assert_eq!(
            command_head("cargo test --workspace 2>err.log"),
            "cargo test --workspace"
        );
        // A trailing number that is genuine command text stays.
        assert_eq!(command_head("sleep 2"), "sleep 2");
        assert_eq!(command_head("echo 2 | wc"), "echo 2");
        assert_eq!(command_head("cd /tmp && git log -1"), "git log -1");
    }

    #[test]
    fn render_budget_counts_chars_not_bytes() {
        // A CJK-heavy ledger (3 bytes/char) must fill toward the same CHAR
        // budget as ASCII — the old byte cap under-filled it ~3x.
        let ledger = SessionLedger::default();
        for i in 0..30 {
            ledger.note_user_request(&format!("请求 {i}：{}", "上下文压缩后恢复原文".repeat(40)));
        }
        let text = ledger.render().expect("non-empty");
        let chars = text.chars().count();
        assert!(chars <= RENDER_MAX_CHARS + 1, "over char budget: {chars}");
        assert!(
            chars > RENDER_MAX_CHARS / 2,
            "under-filled — byte cap regression: {chars}"
        );
    }

    #[test]
    fn requests_dedup_consecutive_and_keep_the_first_on_overflow() {
        let ledger = SessionLedger::default();
        ledger.note_user_request("the original goal");
        ledger.note_user_request("the original goal"); // consecutive dupe
        for i in 0..40 {
            ledger.note_user_request(&format!("follow-up {i}"));
        }
        let text = ledger.render().expect("non-empty");
        assert!(text.contains("1. the original goal"), "{text}");
        assert!(text.contains("follow-up 39"));
        assert!(
            !text.contains("follow-up 5"),
            "oldest follow-ups evicted: {text}"
        );
        ledger.note_user_request("   ");
        assert!(!ledger.render().unwrap().contains("2. \n"));
    }

    #[test]
    fn commands_aggregate_by_head_with_latest_outcome() {
        let ledger = SessionLedger::default();
        ledger.note_command("cd /w/app && npx vitest run a.tsx 2>&1 | grep x", true);
        ledger.note_command("npx vitest run a.tsx --reporter=verbose | head", false);
        ledger.note_command("cargo build", true);
        let text = ledger.render().unwrap();
        assert!(
            text.contains("- `npx vitest run a.tsx` · 2× · last: FAILED"),
            "{text}"
        );
        assert!(text.contains("- `cargo build` · 1× · last: ok"));
    }

    #[test]
    fn notes_dedup_and_keep_open_questions() {
        let ledger = SessionLedger::default();
        let entry = SessionMemoryEntry::new(SessionMemoryKind::Decision, "use tokio");
        ledger.note_entry(&entry);
        ledger.note_entry(&entry); // exact dupe
        ledger.note_entry(&SessionMemoryEntry::new(
            SessionMemoryKind::OpenQuestion,
            "split loop_.rs?",
        ));
        let text = ledger.render().unwrap();
        assert_eq!(text.matches("DECISION: use tokio").count(), 1);
        assert!(text.contains("OPEN QUESTION: split loop_.rs?"));
    }

    #[tokio::test]
    async fn hook_records_shell_tools_only() {
        let ledger = SessionLedger::default();
        let hook = ledger.hook();
        for (tool, cmd, ok) in [
            ("Bash", "cargo test", false),
            ("BashRun", "cargo test", true),
            ("FileRead", "ignored", true),
        ] {
            hook.handle(&HookEvent::AfterToolUse {
                tool: tool.into(),
                input: serde_json::json!({"command": cmd, "path": "x"}),
                output: serde_json::json!({}),
                ok,
            })
            .await;
        }
        let text = ledger.render().unwrap();
        assert!(text.contains("- `cargo test` · 2× · last: ok"), "{text}");
        assert!(!text.contains("ignored"));
    }

    #[test]
    fn clear_wipes_every_section() {
        let ledger = SessionLedger::default();
        ledger.note_user_request("goal");
        ledger.note_command("cargo test", true);
        ledger.note_entry(&SessionMemoryEntry::new(
            SessionMemoryKind::Decision,
            "use tokio",
        ));
        assert!(ledger.render().is_some());
        ledger.clear();
        assert!(ledger.render().is_none(), "cleared ledger renders nothing");
    }

    #[test]
    fn empty_ledger_renders_nothing_and_render_respects_the_cap() {
        assert!(SessionLedger::default().render().is_none());

        let ledger = SessionLedger::default();
        for i in 0..MAX_NOTES {
            ledger.note_entry(&SessionMemoryEntry::new(
                SessionMemoryKind::Observation,
                format!("note {i} {}", "x".repeat(250)),
            ));
        }
        let text = ledger.render().unwrap();
        assert!(
            text.len() <= RENDER_MAX_CHARS + 1,
            "cap held: {}",
            text.len()
        );
    }
}
