//! Chat-view tests for the collapsed tool-activity summaries and the
//! floating "jump to bottom" pill. Split out of `chat.rs`, which is already
//! far past the per-file size guideline.

use super::*;
use crate::theme::ThemeStore;
use ratatui::{backend::TestBackend, Terminal};

fn test_meta() -> ChatRenderMeta<'static> {
    ChatRenderMeta {
        theme_name: "minimal",
        model: "m",
        cwd: std::path::Path::new("."),
    }
}

fn joined_text(lines: &[Line<'static>]) -> String {
    lines
        .iter()
        .map(plain_line_text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// One finished tool call as the transcript records it: the call row, its
/// result row, and the token row the API reported alongside them.
fn push_finished_call(chat: &mut ChatView, tool: &str, detail: &str) {
    chat.push_tool_call(&format!("Tool {tool} {detail}"), tool);
    chat.push_tool_result(&format!("Tool {tool} ok\n    some output"));
    chat.push_usage("Usage ↑10 ↓5 · cache 15% (2)");
}

#[test]
fn a_finished_run_of_tool_rows_collapses_to_one_summary_line() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    chat.push_user("build it");
    for i in 0..9 {
        push_finished_call(&mut chat, "Bash", &format!("cargo step{i}"));
    }
    chat.end_turn();

    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    assert!(
        text.contains("Ran 9 shell commands"),
        "27 rows should read as one summary: {text}"
    );
    assert!(!text.contains("cargo step0"), "call rows must be folded");
    assert!(!text.contains("some output"), "result rows must be folded");
    assert!(!text.contains("Usage"), "usage row must never stand alone");
    assert_eq!(built.group_toggles.len(), 1, "the summary is clickable");
}

#[test]
fn small_runs_render_their_rows_instead_of_a_summary() {
    // A one-or-two call run carries its detail in the rows themselves —
    // "searched for 1 pattern" would say strictly less. Only the usage rows
    // fold away.
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    chat.push_user("look around");
    push_finished_call(&mut chat, "Grep", "pattern=swimlane");
    chat.push_delta("found it, editing");
    push_finished_call(&mut chat, "FileEdit", "step-source.tsx");
    push_finished_call(&mut chat, "FileEdit", "api.ts");
    chat.end_turn();

    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    assert!(
        text.contains("pattern=swimlane"),
        "single-call run keeps its call row: {text}"
    );
    assert!(text.contains("step-source.tsx"), "two-call run too: {text}");
    assert!(text.contains("api.ts"));
    assert!(
        !text.contains("Searched for") && !text.contains("Edited"),
        "no count summaries for small runs: {text}"
    );
    assert!(!text.contains("Usage"), "usage rows still fold: {text}");
    assert!(built.group_toggles.is_empty(), "nothing to toggle");
}

#[test]
fn mid_size_runs_keep_their_rows_visible() {
    // A typical explore-then-edit turn (up to SMALL_RUN_MAX_CALLS calls)
    // shows every call row — the path / command / pattern detail is the
    // point of the transcript, and a count line would erase it.
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    chat.push_user("look around");
    for i in 0..8 {
        push_finished_call(&mut chat, "Bash", &format!("cargo step{i}"));
    }
    chat.end_turn();

    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    for i in 0..8 {
        assert!(
            text.contains(&format!("cargo step{i}")),
            "call row {i} stays visible: {text}"
        );
    }
    assert!(
        !text.contains("Ran 8 shell commands"),
        "no count summary for a run at the threshold: {text}"
    );
    assert!(!text.contains("Usage"), "usage rows still fold: {text}");
}

#[test]
fn a_collapsed_result_row_leaks_its_first_output_line() {
    // The one-line fold of a result row carries the beginning of the output
    // beside its status, so a closed transcript still says what came back.
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    chat.push_user("run it");
    chat.push_tool_call("Tool Bash cargo build", "Bash");
    chat.push_tool_result("Tool Bash done\n    Compiling zode v0.1.0\n    Finished dev");
    chat.end_turn();

    let built = chat.build_lines(&theme, test_meta(), 100);
    let text = joined_text(&built.lines);
    assert!(
        text.contains("Tool Bash done · Compiling zode v0.1.0"),
        "folded result leaks its first content line: {text}"
    );
    assert!(text.contains("(+2)"), "fold count still shown: {text}");
    assert!(
        !text.contains("Finished dev"),
        "later output lines stay folded: {text}"
    );
}

#[test]
fn a_mixed_run_reads_as_one_sentence() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    for i in 0..4 {
        push_finished_call(&mut chat, "Grep", &format!("pattern=fn main{i}"));
    }
    for i in 0..5 {
        push_finished_call(&mut chat, "Bash", &format!("cargo step{i}"));
    }
    chat.end_turn();

    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    assert!(
        text.contains("Searched for 4 patterns, ran 5 shell commands"),
        "one line for the whole run: {text}"
    );
    assert_eq!(built.group_toggles.len(), 1, "adjacent rows are one group");
}

#[test]
fn assistant_prose_between_tools_splits_the_groups() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    for i in 0..9 {
        push_finished_call(&mut chat, "Bash", &format!("cargo step{i}"));
    }
    chat.push_delta("that failed, let me look");
    for i in 0..9 {
        push_finished_call(&mut chat, "Read", &format!("src/f{i}.rs"));
    }
    chat.end_turn();

    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    assert!(text.contains("Ran 9 shell commands"));
    assert!(text.contains("Read 9 files"));
    assert!(text.contains("that failed"), "prose stays between them");
    assert_eq!(built.group_toggles.len(), 2);
}

#[test]
fn a_bare_usage_row_renders_nothing_at_all() {
    // The token row that trails a plain answer has no call to describe —
    // it must not leave a stray summary line behind either.
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    chat.push_delta("here is the answer");
    chat.push_usage("Usage ↑378 ↓46");
    chat.end_turn();

    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    assert!(text.contains("here is the answer"));
    assert!(
        !text.contains("Usage"),
        "usage leaked into the view: {text}"
    );
    assert!(built.group_toggles.is_empty(), "nothing to click");
}

#[test]
fn the_running_group_shows_only_what_is_still_in_flight() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    chat.push_user("build it");
    for i in 0..8 {
        push_finished_call(&mut chat, "Bash", &format!("cargo step{i}"));
    }
    // A ninth call starts and has not answered yet (nine calls total, so
    // the run is past the small-run threshold and summarizes).
    chat.push_tool_call("Tool Bash cargo fmt", "Bash");

    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    assert!(
        text.contains("▾ Ran 9 shell commands"),
        "live header counts every call: {text}"
    );
    assert!(text.contains("cargo fmt"), "the running call stays visible");
    assert!(
        !text.contains("cargo step0") && !text.contains("cargo step7"),
        "finished calls belong to the count, not the screen: {text}"
    );
    assert!(
        !text.contains("some output") && !text.contains("Usage"),
        "results and usage rows are manual-expand only: {text}"
    );

    // The turn ends: the same run folds itself away entirely.
    chat.push_tool_result("Tool Bash ok\n    some output");
    chat.end_turn();
    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    assert!(
        text.contains("▸ Ran 9 shell commands"),
        "folded header: {text}"
    );
    assert!(!text.contains("cargo step0") && !text.contains("cargo step7"));
}

#[test]
fn the_live_summary_count_ticks_up_as_calls_start() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    chat.push_tool_call("Tool Bash cargo build", "Bash");
    let built = chat.build_lines(&theme, test_meta(), 80);
    assert!(joined_text(&built.lines).contains("▾ Ran 1 shell command"));

    chat.push_tool_result("Tool Bash ok\n    some output");
    chat.push_tool_call("Tool Grep pattern=fn main", "Grep");
    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    assert!(
        text.contains("▾ Ran 1 shell command, searched for 1 pattern"),
        "the header tracks the run as it grows: {text}"
    );
}

#[test]
fn manually_expanding_a_live_group_still_reveals_every_row() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let meta = test_meta();
    let mut chat = ChatView::new();
    push_finished_call(&mut chat, "Bash", "cargo build");
    chat.push_tool_call("Tool Bash cargo test", "Bash");
    let area = Rect::new(0, 0, 80, 24);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| chat.render(f, area, &theme, meta))
        .unwrap();

    let built = chat.build_lines(&theme, meta, 80);
    let (&summary_line, _) = built
        .group_toggles
        .iter()
        .next()
        .expect("one activity group");
    assert!(chat.toggle_collapse_at(&theme, meta, area, summary_line as u16));

    let built = chat.build_lines(&theme, meta, 80);
    let text = joined_text(&built.lines);
    assert!(
        text.contains("cargo build"),
        "finished call revealed: {text}"
    );
    assert!(
        text.contains("cargo test"),
        "running call still shown: {text}"
    );
    assert!(text.contains("Usage"), "usage row revealed: {text}");
}

#[test]
fn a_thinking_block_with_no_visible_text_neither_renders_nor_splits_a_run() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    for i in 0..5 {
        push_finished_call(&mut chat, "Bash", &format!("cargo step{i}"));
    }
    // Some providers emit thinking blocks that carry nothing but whitespace.
    chat.push_thinking_delta("\n\n");
    chat.push_thinking_delta("   ");
    for i in 0..4 {
        push_finished_call(&mut chat, "Grep", &format!("pattern=fn main{i}"));
    }
    chat.end_turn();

    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    assert!(
        !text.contains("Thinking"),
        "an empty thinking label must not paint: {text}"
    );
    assert_eq!(
        built.group_toggles.len(),
        1,
        "the runs on either side merge past the small-run threshold: {text}"
    );
    assert!(text.contains("Ran 5 shell commands, searched for 4 patterns"));
}

#[test]
fn thinking_that_arrives_after_blank_deltas_still_splits_the_run() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    for i in 0..9 {
        push_finished_call(&mut chat, "Bash", &format!("cargo step{i}"));
    }
    // The block opens on whitespace, then real reasoning streams in.
    chat.push_thinking_delta("  ");
    chat.push_thinking_delta("now let me search");
    for i in 0..9 {
        push_finished_call(&mut chat, "Grep", &format!("pattern=fn main{i}"));
    }
    chat.end_turn();

    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    assert!(
        text.contains("now let me search"),
        "reasoning shows: {text}"
    );
    assert_eq!(
        built.group_toggles.len(),
        2,
        "content breaks the run into two summarized groups: {text}"
    );
}

#[test]
fn clicking_a_group_summary_reveals_the_rows_it_stands_for() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let meta = test_meta();
    let mut chat = ChatView::new();
    for i in 0..9 {
        push_finished_call(&mut chat, "Bash", &format!("cargo step{i}"));
    }
    chat.end_turn();
    let area = Rect::new(0, 0, 80, 24);

    // Paint once so last_render_start reflects a real frame.
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| chat.render(f, area, &theme, meta))
        .unwrap();

    let built = chat.build_lines(&theme, meta, 80);
    let (&summary_line, &anchor) = built
        .group_toggles
        .iter()
        .next()
        .expect("one activity group");
    assert!(!chat.messages[anchor].group_open);

    assert!(chat.toggle_collapse_at(&theme, meta, area, summary_line as u16));
    assert!(chat.messages[anchor].group_open, "click opens the group");

    let built = chat.build_lines(&theme, meta, 80);
    let text = joined_text(&built.lines);
    assert!(text.contains("cargo step0"), "call row revealed: {text}");
    assert!(text.contains("Usage"), "usage row revealed too: {text}");
    // The rows inside keep their own folds, one line each.
    assert!(text.contains("(+1)"), "result output still folded: {text}");

    // Clicking the header again folds the group back up.
    assert!(chat.toggle_collapse_at(&theme, meta, area, summary_line as u16));
    assert!(!chat.messages[anchor].group_open);
}

#[test]
fn expand_all_reveals_grouped_rows_too() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    for i in 0..9 {
        push_finished_call(&mut chat, "Bash", &format!("cargo step{i}"));
    }
    chat.end_turn();

    assert!(chat.toggle_all_collapsed(), "first press expands all");
    let built = chat.build_lines(&theme, test_meta(), 80);
    let text = joined_text(&built.lines);
    assert!(text.contains("cargo step0"), "group opened: {text}");
    assert!(
        text.contains("some output"),
        "inner folds opened too: {text}"
    );

    assert!(!chat.toggle_all_collapsed(), "second press folds all");
    let built = chat.build_lines(&theme, test_meta(), 80);
    assert!(!joined_text(&built.lines).contains("cargo step0"));
}

#[test]
fn hiding_thinking_merges_the_runs_it_separated() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let mut chat = ChatView::new();
    for i in 0..5 {
        push_finished_call(&mut chat, "Bash", &format!("cargo step{i}"));
    }
    chat.push_thinking_delta("now let me search");
    for i in 0..4 {
        push_finished_call(&mut chat, "Grep", &format!("pattern=fn main{i}"));
    }
    chat.end_turn();

    // Thinking shown: it breaks the run in two SMALL runs, which render
    // their real rows (no summaries, nothing to toggle).
    let built = chat.build_lines(&theme, test_meta(), 80);
    assert!(built.group_toggles.is_empty());
    assert!(joined_text(&built.lines).contains("cargo step0"));

    // Thinking hidden: the runs merge past the small-run threshold and
    // read as one summary line.
    chat.set_display_prefs(false, true);
    let built = chat.build_lines(&theme, test_meta(), 80);
    assert_eq!(built.group_toggles.len(), 1);
    assert!(joined_text(&built.lines).contains("Ran 5 shell commands, searched for 4 patterns"));
}

#[test]
fn the_jump_pill_floats_over_the_bottom_row_only_when_far_from_the_tail() {
    let theme = ThemeStore::with_builtins().resolve(None);
    let meta = test_meta();
    let mut view = ChatView::new();
    for i in 0..80 {
        view.push_user(&format!("question number {i}"));
    }
    let backend = TestBackend::new(40, 10);
    let mut term = Terminal::new(backend).unwrap();
    let painted = |term: &Terminal<TestBackend>| -> String {
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    };

    // Following the tail, and a small scroll-up: no pill.
    term.draw(|f| view.render(f, f.area(), &theme, meta))
        .unwrap();
    assert!(!painted(&term).contains("Jump to bottom"));
    assert!(!view.jump_pill_hit(20, 9));

    view.scroll_up(15);
    term.draw(|f| view.render(f, f.area(), &theme, meta))
        .unwrap();
    assert!(
        !painted(&term).contains("Jump to bottom"),
        "a one-and-a-half viewport scroll must not flash the pill"
    );

    // Well past two viewports: the pill appears on the bottom row and is
    // clickable there.
    view.scroll_up(40);
    term.draw(|f| view.render(f, f.area(), &theme, meta))
        .unwrap();
    let content = painted(&term);
    assert!(content.contains("Jump to bottom"), "no pill: {content}");
    let pill = view.last_pill.expect("pill rect recorded");
    assert_eq!(pill.y, 9, "pill sits on the bottom row of the viewport");
    assert!(view.jump_pill_hit(pill.x, pill.y));
    assert!(!view.jump_pill_hit(pill.x, pill.y - 1));

    // Jumping back to the tail retires it.
    view.scroll_to_bottom();
    term.draw(|f| view.render(f, f.area(), &theme, meta))
        .unwrap();
    assert!(!painted(&term).contains("Jump to bottom"));
    assert!(view.last_pill.is_none());
}
