//! Tool-activity grouping for the chat transcript.
//!
//! A finished turn used to paint three rows per tool call — the invocation,
//! its result, and a `Usage ↑… ↓…` row — which buried the assistant's prose in
//! process noise. This module turns a run of those rows into ONE dim summary
//! line ("Searched for 1 pattern, ran 2 shell commands").
//!
//! The pass is purely positional: it classifies rows and groups adjacent ones,
//! it never rewrites or drops messages. Expanding a group therefore shows
//! exactly the rows the transcript already holds, and copy/history stay
//! lossless.

use std::ops::Range;

/// What a `Role::Tool` transcript row represents.
///
/// A message with no activity (`None`) is not groupable — thinking blocks,
/// notices, errors and local shell output all render as they always did, and
/// they break any run around them so nothing important is folded away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolActivity {
    /// A tool invocation. `tool` is the runtime tool name (`Bash`, `Grep`,
    /// `mcp__server__tool`, …); the only row kind that counts toward a summary.
    Call { tool: String },
    /// The result row belonging to a preceding call.
    Result,
    /// A token/cost row. Never renders outside an expanded group.
    Usage,
}

/// Maximal runs of consecutive groupable rows. `kinds[i]` describes the i-th
/// VISIBLE transcript row (after the `/thinking` and `/tool-details` filters),
/// so hiding thinking merges the runs it used to separate.
pub fn activity_runs(kinds: &[Option<ToolActivity>]) -> Vec<Range<usize>> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (i, kind) in kinds.iter().enumerate() {
        match (kind.is_some(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                runs.push(s..i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push(s..kinds.len());
    }
    runs
}

/// Whether a thinking block has anything worth painting. Some providers emit
/// thinking blocks whose visible text is empty or pure whitespace; painting
/// their bare `Thinking:` label adds a noise row AND — since thinking is not
/// activity — splits the surrounding tool run into a ladder of tiny groups.
/// Blank ones are treated as absent, so the runs around them merge.
pub fn thinking_has_content(body: &str) -> bool {
    !body.trim().is_empty()
}

/// Positions within a run (relative to its start) of the calls still waiting
/// on a result — the only rows a live, auto-opened group paints.
///
/// Results carry no identity on the transcript row, so the match is
/// positional: the first `results` calls count as finished. Calls that run in
/// parallel therefore leave exactly as many rows on screen as there are
/// outstanding calls, which is the number the user is waiting on.
pub fn in_flight_rows(kinds: &[Option<ToolActivity>]) -> Vec<usize> {
    let results = kinds
        .iter()
        .filter(|k| matches!(k, Some(ToolActivity::Result)))
        .count();
    let mut calls = 0usize;
    let mut out = Vec::new();
    for (i, kind) in kinds.iter().enumerate() {
        if matches!(kind, Some(ToolActivity::Call { .. })) {
            calls += 1;
            if calls > results {
                out.push(i);
            }
        }
    }
    out
}

/// A summary bucket. The four named kinds get purpose-written wording; every
/// other tool is counted under its own name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Bucket {
    Shell,
    Search,
    Read,
    Edit,
    Tool(String),
}

/// Once this many distinct one-off tools appear, naming each of them is
/// longer than the transcript row it replaces — collapse to a plain count.
const MAX_NAMED_TOOLS: usize = 2;

/// Runs with at most this many calls render their real rows (Claude-Code
/// style: the call and result lines carry the pattern / file / command the
/// user actually wants to see) instead of a count line that erases them —
/// "searched for 1 pattern" says strictly less than the row it replaced.
/// Longer runs keep the one-line summary that kills wall-of-noise turns.
pub const SMALL_RUN_MAX_CALLS: usize = 2;

fn bucket_for(tool: &str) -> Bucket {
    match tool {
        "Bash" | "Shell" | "BashOutput" => Bucket::Shell,
        "Grep" | "Glob" | "Search" | "Find" => Bucket::Search,
        "Read" | "FileRead" | "NotebookRead" | "View" => Bucket::Read,
        "Write" | "Edit" | "MultiEdit" | "FileWrite" | "FileEdit" | "NotebookEdit" => Bucket::Edit,
        other => Bucket::Tool(display_tool(other)),
    }
}

/// `mcp__server__tool` reads as `server.tool` in a summary line; everything
/// else is already the name the user sees on the per-call row.
fn display_tool(tool: &str) -> String {
    match tool.strip_prefix("mcp__") {
        Some(rest) => rest.replacen("__", ".", 1),
        None => tool.to_string(),
    }
}

/// The one-line, Claude-Code-style description of a run's tool calls, in the
/// order the kinds first appeared: "Searched for 1 pattern, ran 2 shell
/// commands". Empty when the run holds no calls at all (a bare token-usage
/// tail), which the caller renders as nothing.
pub fn summarize(tools: &[&str]) -> String {
    let mut counts: Vec<(Bucket, usize)> = Vec::new();
    for tool in tools {
        let bucket = bucket_for(tool);
        match counts.iter_mut().find(|(b, _)| *b == bucket) {
            Some((_, n)) => *n += 1,
            None => counts.push((bucket, 1)),
        }
    }
    let named = counts
        .iter()
        .filter(|(b, _)| matches!(b, Bucket::Tool(_)))
        .count();
    if named > MAX_NAMED_TOOLS {
        counts = fold_named_tools(counts);
    }
    let joined = counts
        .iter()
        .map(|(bucket, n)| phrase(bucket, *n))
        .collect::<Vec<_>>()
        .join(", ");
    capitalize_first(&joined)
}

/// Replace every named-tool bucket with a single count, kept at the position
/// of the first one so the sentence order still tracks what happened.
fn fold_named_tools(counts: Vec<(Bucket, usize)>) -> Vec<(Bucket, usize)> {
    let total: usize = counts
        .iter()
        .filter(|(b, _)| matches!(b, Bucket::Tool(_)))
        .map(|(_, n)| n)
        .sum();
    let mut folded = false;
    let mut out = Vec::new();
    for (bucket, n) in counts {
        if matches!(bucket, Bucket::Tool(_)) {
            if !folded {
                folded = true;
                out.push((Bucket::Tool(String::new()), total));
            }
            continue;
        }
        out.push((bucket, n));
    }
    out
}

fn phrase(bucket: &Bucket, n: usize) -> String {
    let plural = |one: &'static str, many: &'static str| {
        if n == 1 {
            crate::tr(one).to_string()
        } else {
            crate::tr(many).replace("{n}", &n.to_string())
        }
    };
    match bucket {
        Bucket::Shell => plural("ran 1 shell command", "ran {n} shell commands"),
        Bucket::Search => plural("searched for 1 pattern", "searched for {n} patterns"),
        Bucket::Read => plural("read 1 file", "read {n} files"),
        Bucket::Edit => plural("edited 1 file", "edited {n} files"),
        // The empty name is the folded "too many distinct tools" bucket.
        Bucket::Tool(name) if name.is_empty() => {
            crate::tr("{n} tool calls").replace("{n}", &n.to_string())
        }
        Bucket::Tool(name) => {
            if n == 1 {
                crate::tr("used {tool} once").replace("{tool}", name)
            } else {
                crate::tr("used {tool} {n} times")
                    .replace("{tool}", name)
                    .replace("{n}", &n.to_string())
            }
        }
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Whether the "jump to bottom" pill should float over the transcript. The
/// threshold is two full viewports so a small scroll-up — reading back over
/// the answer that just streamed in — never makes it flash.
pub fn jump_pill_visible(scroll_back: usize, viewport_rows: usize) -> bool {
    viewport_rows > 0 && scroll_back > viewport_rows.saturating_mul(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(tool: &str) -> Option<ToolActivity> {
        Some(ToolActivity::Call {
            tool: tool.to_string(),
        })
    }

    #[test]
    fn runs_span_consecutive_activity_and_break_on_prose() {
        let kinds = vec![
            None,                       // user
            call("Bash"),               // ┐
            Some(ToolActivity::Result), // │ one run
            Some(ToolActivity::Usage),  // ┘
            None,                       // assistant text
            call("Grep"),               // ┐ second run
            Some(ToolActivity::Result), // ┘
        ];
        assert_eq!(activity_runs(&kinds), vec![1..4, 5..7]);
    }

    #[test]
    fn a_trailing_run_reaches_the_end_of_the_transcript() {
        let kinds = vec![None, call("Bash")];
        assert_eq!(activity_runs(&kinds), vec![1..2]);
        assert!(activity_runs(&[None, None]).is_empty());
        assert!(activity_runs(&[]).is_empty());
    }

    #[test]
    fn only_thinking_with_visible_text_counts_as_present() {
        assert!(thinking_has_content("weighing the options"));
        assert!(!thinking_has_content(""));
        assert!(!thinking_has_content("   \n\t "));
    }

    #[test]
    fn a_live_run_shows_only_the_calls_still_waiting_on_a_result() {
        // Finished call, then a second one still running: only the second row.
        let kinds = vec![
            call("Bash"),
            Some(ToolActivity::Result),
            Some(ToolActivity::Usage),
            call("Grep"),
        ];
        assert_eq!(in_flight_rows(&kinds), vec![3]);

        // Everything answered: nothing is in flight.
        assert!(in_flight_rows(&kinds[0..3]).is_empty());

        // Two calls started before either answered, then one result lands.
        let parallel = vec![call("Bash"), call("Bash"), Some(ToolActivity::Result)];
        assert_eq!(in_flight_rows(&parallel), vec![1]);
        assert_eq!(in_flight_rows(&parallel[0..2]), vec![0, 1]);

        // A run of pure token accounting has nothing to show.
        assert!(in_flight_rows(&[Some(ToolActivity::Usage)]).is_empty());
    }

    #[test]
    fn mixed_runs_read_as_one_sentence_in_arrival_order() {
        assert_eq!(
            summarize(&["Grep", "Bash", "Bash"]),
            "Searched for 1 pattern, ran 2 shell commands"
        );
        assert_eq!(
            summarize(&["Bash", "Grep"]),
            "Ran 1 shell command, searched for 1 pattern"
        );
    }

    #[test]
    fn each_kind_has_its_own_singular_and_plural_wording() {
        assert_eq!(summarize(&["Bash"]), "Ran 1 shell command");
        assert_eq!(summarize(&["Glob", "Grep"]), "Searched for 2 patterns");
        assert_eq!(summarize(&["Read"]), "Read 1 file");
        assert_eq!(summarize(&["Write", "Edit", "FileEdit"]), "Edited 3 files");
    }

    #[test]
    fn unknown_tools_are_named_until_there_are_too_many() {
        assert_eq!(summarize(&["Todo"]), "Used Todo once");
        assert_eq!(summarize(&["Todo", "Todo"]), "Used Todo 2 times");
        assert_eq!(
            summarize(&["mcp__pencil__batch_get"]),
            "Used pencil.batch_get once"
        );
        // Three or more distinct one-off tools: a plain count instead of a
        // sentence longer than the rows it replaces. Known kinds keep their
        // wording, and the count lands where the first unknown tool did.
        assert_eq!(
            summarize(&["Bash", "Todo", "WebFetch", "Task", "Task"]),
            "Ran 1 shell command, 4 tool calls"
        );
    }

    #[test]
    fn a_run_without_calls_has_no_summary() {
        assert_eq!(summarize(&[]), "");
    }

    #[test]
    fn the_pill_appears_only_past_two_viewports() {
        assert!(!jump_pill_visible(0, 20));
        assert!(!jump_pill_visible(20, 20), "one viewport must not flash it");
        assert!(
            !jump_pill_visible(40, 20),
            "exactly two viewports is still quiet"
        );
        assert!(jump_pill_visible(41, 20));
        assert!(!jump_pill_visible(100, 0), "no viewport, no pill");
    }
}
