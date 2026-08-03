//! Adaptive status HUD: the extra rows stacked directly above the main status
//! line (per-turn tool tally, live/recent sub-agents, mode + infrastructure).
//!
//! Every row is optional. A row with nothing to say is not drawn and costs no
//! terminal height, so the status region grows and shrinks with the session.
//! The whole HUD is suppressed on short terminals, where the rows would eat
//! the conversation.

use std::collections::HashMap;

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use zode_core::{SubAgent, SubAgentStatus, ToolAccessMode};

use crate::theme::Theme;

/// Terminals shorter than this keep the classic 1–2 row status region.
pub const MIN_HEIGHT_FOR_HUD: u16 = 20;

/// Tool cells shown in the tally row before the `+N more` marker.
pub const MAX_TALLY_CELLS: usize = 4;

/// Sub-agent rows shown before the `+N more` marker.
pub const MAX_SUBAGENT_ROWS: usize = 3;

/// How long a finished sub-agent keeps its HUD row, in seconds.
pub const SUBAGENT_RECENT_SECS: u64 = 60;

/// Upper bound on HUD rows: 1 tally + 3 sub-agents + 1 overflow + 1 mode.
pub const MAX_HUD_ROWS: u16 = 6;

// ─── per-turn tool tally ────────────────────────────────────────────────────

/// One tool's contribution to the turn tally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TallyEntry {
    pub name: String,
    /// Calls started this turn. A call is counted the moment it STARTS, so an
    /// in-flight call is already visible — under the running glyph until its
    /// result arrives.
    pub calls: u32,
    /// Calls that have returned (successfully or not).
    pub finished: u32,
    /// Calls that returned an error.
    pub failed: u32,
}

impl TallyEntry {
    /// `✗` when any call failed, `◐` while any call is still in flight, else `✓`.
    pub fn glyph(&self) -> char {
        if self.failed > 0 {
            '✗'
        } else if self.finished < self.calls {
            '◐'
        } else {
            '✓'
        }
    }
}

/// Per-turn tool aggregation: uncapped, keyed by tool name, remembering
/// first-appearance order so equal counts sort deterministically.
#[derive(Debug, Clone, Default)]
pub struct ToolTally {
    entries: Vec<TallyEntry>,
    index: HashMap<String, usize>,
}

impl ToolTally {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
    }

    /// Count a starting call.
    pub fn record_start(&mut self, name: &str) {
        match self.index.get(name) {
            Some(&i) => self.entries[i].calls = self.entries[i].calls.saturating_add(1),
            None => {
                self.index.insert(name.to_string(), self.entries.len());
                self.entries.push(TallyEntry {
                    name: name.to_string(),
                    calls: 1,
                    finished: 0,
                    failed: 0,
                });
            }
        }
    }

    /// Close out a call. A result for a tool that was never started (a replayed
    /// or stale event) is ignored rather than inventing a cell.
    pub fn record_result(&mut self, name: &str, ok: bool) {
        let Some(&i) = self.index.get(name) else {
            return;
        };
        let entry = &mut self.entries[i];
        entry.finished = entry.finished.saturating_add(1).min(entry.calls);
        if !ok {
            entry.failed = entry.failed.saturating_add(1);
        }
    }

    /// The busiest `max` tools plus how many were left out. Ties keep
    /// first-appearance order.
    pub fn top(&self, max: usize) -> (Vec<&TallyEntry>, usize) {
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by(|&a, &b| {
            self.entries[b]
                .calls
                .cmp(&self.entries[a].calls)
                .then(a.cmp(&b))
        });
        let overflow = order.len().saturating_sub(max);
        let shown = order
            .into_iter()
            .take(max)
            .map(|i| &self.entries[i])
            .collect();
        (shown, overflow)
    }
}

/// One tally cell as plain text (`✓ Bash ×7`).
pub fn tally_cell_text(entry: &TallyEntry) -> String {
    format!("{} {} ×{}", entry.glyph(), entry.name, entry.calls)
}

// ─── sub-agent rows ─────────────────────────────────────────────────────────

/// A sub-agent line prepared for the HUD (model already resolved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAgentRow {
    pub glyph: char,
    pub agent_type: String,
    pub model: String,
    pub description: Option<String>,
    pub elapsed_secs: u64,
}

/// Second-granularity elapsed label. `format_duration_ms` renders a decimal
/// second ("12.0s") which is noise for sub-agents that only tick per second.
pub fn elapsed_label(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// One sub-agent row as plain text (`✓ general-purpose [opus]: audit (12s)`).
pub fn subagent_row_text(row: &SubAgentRow) -> String {
    let desc = row
        .description
        .as_deref()
        .map(|d| format!(": {d}"))
        .unwrap_or_default();
    format!(
        "{} {} [{}]{desc} ({})",
        row.glyph,
        row.agent_type,
        row.model,
        elapsed_label(row.elapsed_secs)
    )
}

fn status_glyph(status: SubAgentStatus) -> char {
    match status {
        SubAgentStatus::Running => '◐',
        SubAgentStatus::Done => '✓',
        SubAgentStatus::Failed => '✗',
    }
}

/// True when this sub-agent still deserves a HUD row: running, or finished
/// within the recency window.
pub fn is_hud_visible(agent: &SubAgent, now: u64) -> bool {
    match agent.finished_at {
        None => true,
        Some(end) => now.saturating_sub(end) <= SUBAGENT_RECENT_SECS,
    }
}

/// Build the HUD's sub-agent rows from a newest-first snapshot. `models` maps
/// an agent type to its declared model; anything unlisted falls back to the
/// session model.
pub fn subagent_rows(
    agents: &[SubAgent],
    now: u64,
    models: &HashMap<String, String>,
    fallback_model: &str,
) -> (Vec<SubAgentRow>, usize) {
    let visible: Vec<&SubAgent> = agents.iter().filter(|a| is_hud_visible(a, now)).collect();
    let overflow = visible.len().saturating_sub(MAX_SUBAGENT_ROWS);
    let rows = visible
        .into_iter()
        .take(MAX_SUBAGENT_ROWS)
        .map(|a| SubAgentRow {
            glyph: status_glyph(a.status),
            agent_type: a.agent_type.clone(),
            model: models
                .get(&a.agent_type)
                .map(String::as_str)
                .unwrap_or(fallback_model)
                .to_string(),
            description: a.description.clone(),
            elapsed_secs: a.finished_at.unwrap_or(now).saturating_sub(a.started_at),
        })
        .collect();
    (rows, overflow)
}

// ─── mode + infrastructure row ──────────────────────────────────────────────

/// Ambient counts for the mode row. A zero count drops its segment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InfraCounts {
    pub shells: usize,
    pub mcps: usize,
    pub instruction_files: usize,
}

fn plural(n: usize, one: &'static str, many: &'static str) -> String {
    let key = if n == 1 { one } else { many };
    crate::tr(key).replace("{n}", &n.to_string())
}

/// Segments of the mode row, in display order. The mode itself is always
/// present; each infrastructure count appears only when non-zero.
pub fn mode_segments(mode: ToolAccessMode, infra: InfraCounts) -> Vec<String> {
    let mut out = vec![crate::tr(match mode {
        ToolAccessMode::Auto => "auto mode on",
        ToolAccessMode::Prompt => "prompt mode on",
        ToolAccessMode::ReadOnly => "read-only mode on",
    })
    .to_string()];
    if infra.shells > 0 {
        out.push(plural(infra.shells, "{n} shell", "{n} shells"));
    }
    if infra.mcps > 0 {
        out.push(plural(infra.mcps, "{n} MCP", "{n} MCPs"));
    }
    if infra.instruction_files > 0 {
        // A filename has no English plural, so both forms share one key.
        out.push(plural(
            infra.instruction_files,
            "{n} CLAUDE.md",
            "{n} CLAUDE.md",
        ));
    }
    out
}

// ─── assembly ───────────────────────────────────────────────────────────────

/// Everything the HUD renders, computed once per frame by the app.
pub struct HudInput<'a> {
    /// Tally to display: the live turn's, or the last turn's between turns.
    pub tally: &'a ToolTally,
    pub subagents: &'a [SubAgentRow],
    pub subagent_overflow: usize,
    pub mode: ToolAccessMode,
    pub infra: InfraCounts,
}

fn overflow_label(n: usize) -> String {
    crate::tr("+{n} more").replace("{n}", &n.to_string())
}

fn tally_line(input: &HudInput<'_>, theme: &Theme) -> Option<Line<'static>> {
    if input.tally.is_empty() {
        return None;
    }
    let (cells, overflow) = input.tally.top(MAX_TALLY_CELLS);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for entry in cells {
        if !spans.is_empty() {
            spans.push(Span::styled(" │ ", Style::default().fg(theme.separator)));
        }
        let color = match entry.glyph() {
            '✗' => Color::Red,
            '◐' => theme.accent,
            _ => theme.fg_subtle,
        };
        spans.push(Span::styled(
            tally_cell_text(entry),
            Style::default().fg(color),
        ));
    }
    if overflow > 0 {
        spans.push(Span::styled(" │ ", Style::default().fg(theme.separator)));
        spans.push(Span::styled(
            overflow_label(overflow),
            Style::default().fg(theme.fg_subtle),
        ));
    }
    Some(Line::from(spans))
}

fn subagent_lines(input: &HudInput<'_>, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = input
        .subagents
        .iter()
        .map(|row| {
            let color = match row.glyph {
                '✗' => Color::Red,
                '◐' => theme.accent,
                _ => theme.fg_subtle,
            };
            Line::from(Span::styled(
                subagent_row_text(row),
                Style::default().fg(color),
            ))
        })
        .collect();
    if input.subagent_overflow > 0 {
        lines.push(Line::from(Span::styled(
            overflow_label(input.subagent_overflow),
            Style::default().fg(theme.fg_subtle),
        )));
    }
    lines
}

fn mode_line(input: &HudInput<'_>, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, segment) in mode_segments(input.mode, input.infra)
        .into_iter()
        .enumerate()
    {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(theme.separator)));
        }
        let color = if i == 0 && matches!(input.mode, ToolAccessMode::Auto) {
            Color::Red
        } else {
            theme.fg_subtle
        };
        spans.push(Span::styled(segment, Style::default().fg(color)));
    }
    Line::from(spans)
}

/// The HUD's rows, top to bottom. The tally and sub-agent rows come and go;
/// the mode row anchors the block whenever the HUD is on at all.
pub fn hud_lines(input: &HudInput<'_>, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.extend(tally_line(input, theme));
    lines.extend(subagent_lines(input, theme));
    lines.push(mode_line(input, theme));
    lines
}

/// How many terminal rows the HUD wants. `0` on a terminal too short for it.
pub fn row_count(input: &HudInput<'_>, terminal_height: u16) -> u16 {
    if terminal_height < MIN_HEIGHT_FOR_HUD {
        return 0;
    }
    let tally = u16::from(!input.tally.is_empty());
    let subagents = u16::try_from(input.subagents.len()).unwrap_or(u16::MAX);
    let overflow = u16::from(input.subagent_overflow > 0);
    // The mode row is always present once the HUD is on.
    tally
        .saturating_add(subagents)
        .saturating_add(overflow)
        .saturating_add(1)
        .min(MAX_HUD_ROWS)
}

/// Render pre-computed HUD lines into `area` (normally exactly the rows
/// [`row_count`] asked for; extra rows are left blank).
pub fn render_lines(f: &mut Frame, area: Rect, theme: &Theme, lines: Vec<Line<'static>>) {
    if area.is_empty() || lines.is_empty() {
        return;
    }
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.bg_secondary)),
        area,
    );
}

/// Compute and render the HUD in one step. Callers that must release their
/// borrow of the session before drawing use [`hud_lines`] + [`render_lines`].
pub fn render(f: &mut Frame, area: Rect, theme: &Theme, input: &HudInput<'_>) {
    render_lines(f, area, theme, hud_lines(input, theme));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStore;
    use ratatui::{backend::TestBackend, Terminal};

    fn tally_of(pairs: &[(&str, u32)]) -> ToolTally {
        let mut t = ToolTally::default();
        for (name, n) in pairs {
            for _ in 0..*n {
                t.record_start(name);
                t.record_result(name, true);
            }
        }
        t
    }

    #[test]
    fn tally_marks_running_success_and_failure() {
        let mut t = ToolTally::default();
        t.record_start("Bash");
        assert_eq!(
            t.top(4).0[0].glyph(),
            '◐',
            "in-flight call counts as running"
        );
        t.record_result("Bash", true);
        assert_eq!(t.top(4).0[0].glyph(), '✓');
        t.record_start("Bash");
        t.record_result("Bash", false);
        let (cells, _) = t.top(4);
        assert_eq!(cells[0].glyph(), '✗', "one failure marks the whole cell");
        assert_eq!(tally_cell_text(cells[0]), "✗ Bash ×2");
    }

    #[test]
    fn tally_result_without_a_start_is_ignored() {
        let mut t = ToolTally::default();
        t.record_result("Ghost", false);
        assert!(t.is_empty());
    }

    #[test]
    fn tally_top_orders_by_count_then_first_appearance() {
        let t = tally_of(&[
            ("Read", 2),
            ("Bash", 7),
            ("Edit", 2),
            ("Grep", 1),
            ("Web", 1),
        ]);
        let (cells, overflow) = t.top(MAX_TALLY_CELLS);
        let names: Vec<&str> = cells.iter().map(|c| c.name.as_str()).collect();
        // Bash first (7), then the 2s in first-appearance order, then Grep.
        assert_eq!(names, ["Bash", "Read", "Edit", "Grep"]);
        assert_eq!(overflow, 1);
    }

    #[test]
    fn tally_clear_empties_everything() {
        let mut t = tally_of(&[("Bash", 3)]);
        t.clear();
        assert!(t.is_empty());
        // A cleared tally still accepts fresh calls (reset, not poisoned).
        t.record_start("Read");
        assert_eq!(t.top(4).0[0].name, "Read");
    }

    fn agent(id: u64, kind: &str, started: u64, finished: Option<u64>) -> SubAgent {
        SubAgent {
            id,
            agent_type: kind.into(),
            description: Some("audit".into()),
            depth: 0,
            status: match finished {
                None => SubAgentStatus::Running,
                Some(_) => SubAgentStatus::Done,
            },
            started_at: started,
            finished_at: finished,
            input_tokens: 0,
            output_tokens: 0,
            transcript: Vec::new(),
            committed_input: 0,
            committed_output: 0,
            turn_input: 0,
            turn_output: 0,
        }
    }

    #[test]
    fn subagent_row_uses_agent_def_model_then_falls_back() {
        let mut models = HashMap::new();
        models.insert("general-purpose".to_string(), "opus".to_string());
        let agents = [
            agent(1, "general-purpose", 100, Some(112)),
            agent(2, "scout", 100, None),
        ];
        let (rows, overflow) = subagent_rows(&agents, 130, &models, "sonnet");
        assert_eq!(overflow, 0);
        assert_eq!(
            subagent_row_text(&rows[0]),
            "✓ general-purpose [opus]: audit (12s)"
        );
        assert_eq!(subagent_row_text(&rows[1]), "◐ scout [sonnet]: audit (30s)");
    }

    #[test]
    fn subagent_rows_drop_stale_finishers_and_cap_at_three() {
        let agents = [
            agent(1, "a", 0, Some(10)),
            agent(2, "b", 0, None),
            agent(3, "c", 0, None),
            agent(4, "d", 0, None),
            agent(5, "stale", 0, Some(5)),
        ];
        // now = 100: "a" finished 90s ago and "stale" 95s ago — both aged out.
        let (rows, overflow) = subagent_rows(&agents, 100, &HashMap::new(), "m");
        assert_eq!(rows.len(), MAX_SUBAGENT_ROWS);
        assert_eq!(overflow, 0);
        let kinds: Vec<&str> = rows.iter().map(|r| r.agent_type.as_str()).collect();
        assert_eq!(kinds, ["b", "c", "d"]);
        // Inside the window the finisher stays and pushes one row into overflow.
        let (rows, overflow) = subagent_rows(&agents, 40, &HashMap::new(), "m");
        assert_eq!(rows.len(), MAX_SUBAGENT_ROWS);
        assert_eq!(overflow, 2);
        assert_eq!(rows[0].agent_type, "a");
    }

    #[test]
    fn subagent_row_without_description_omits_the_colon() {
        let row = SubAgentRow {
            glyph: '◐',
            agent_type: "worker".into(),
            model: "opus".into(),
            description: None,
            elapsed_secs: 75,
        };
        assert_eq!(subagent_row_text(&row), "◐ worker [opus] (1m 15s)");
    }

    #[test]
    fn mode_row_omits_zero_segments() {
        assert_eq!(
            mode_segments(ToolAccessMode::Prompt, InfraCounts::default()),
            ["prompt mode on"]
        );
        assert_eq!(
            mode_segments(
                ToolAccessMode::Auto,
                InfraCounts {
                    shells: 3,
                    mcps: 0,
                    instruction_files: 2,
                }
            ),
            ["auto mode on", "3 shells", "2 CLAUDE.md"]
        );
        assert_eq!(
            mode_segments(
                ToolAccessMode::ReadOnly,
                InfraCounts {
                    shells: 1,
                    mcps: 1,
                    instruction_files: 0,
                }
            ),
            ["read-only mode on", "1 shell", "1 MCP"]
        );
    }

    fn input<'a>(tally: &'a ToolTally, rows: &'a [SubAgentRow], overflow: usize) -> HudInput<'a> {
        HudInput {
            tally,
            subagents: rows,
            subagent_overflow: overflow,
            mode: ToolAccessMode::Prompt,
            infra: InfraCounts::default(),
        }
    }

    #[test]
    fn row_count_is_adaptive_and_capped() {
        let empty = ToolTally::default();
        assert_eq!(row_count(&input(&empty, &[], 0), 40), 1, "mode row only");
        let busy = tally_of(&[("Bash", 1)]);
        assert_eq!(row_count(&input(&busy, &[], 0), 40), 2);
        let rows: Vec<SubAgentRow> = (0..3)
            .map(|i| SubAgentRow {
                glyph: '◐',
                agent_type: format!("a{i}"),
                model: "m".into(),
                description: None,
                elapsed_secs: 1,
            })
            .collect();
        assert_eq!(row_count(&input(&busy, &rows, 2), 40), MAX_HUD_ROWS);
    }

    #[test]
    fn row_count_is_zero_on_short_terminals() {
        let busy = tally_of(&[("Bash", 1)]);
        assert_eq!(row_count(&input(&busy, &[], 0), MIN_HEIGHT_FOR_HUD - 1), 0);
        assert!(row_count(&input(&busy, &[], 0), MIN_HEIGHT_FOR_HUD) > 0);
    }

    #[test]
    fn render_draws_tally_subagents_and_mode() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut tally = ToolTally::default();
        for _ in 0..7 {
            tally.record_start("Bash");
            tally.record_result("Bash", true);
        }
        tally.record_start("Edit");
        tally.record_result("Edit", false);
        let rows = [SubAgentRow {
            glyph: '◐',
            agent_type: "general-purpose".into(),
            model: "opus".into(),
            description: Some("scan".into()),
            elapsed_secs: 4,
        }];
        let hud = HudInput {
            tally: &tally,
            subagents: &rows,
            subagent_overflow: 0,
            mode: ToolAccessMode::Auto,
            infra: InfraCounts {
                shells: 3,
                mcps: 2,
                instruction_files: 2,
            },
        };
        let height = row_count(&hud, 40);
        assert_eq!(height, 3);
        let backend = TestBackend::new(90, height);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render(f, f.area(), &theme, &hud)).unwrap();
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("✓ Bash ×7"), "{text}");
        assert!(text.contains("✗ Edit ×1"), "{text}");
        assert!(
            text.contains("◐ general-purpose [opus]: scan (4s)"),
            "{text}"
        );
        assert!(text.contains("auto mode on"), "{text}");
        assert!(text.contains("3 shells"), "{text}");
        assert!(text.contains("2 MCPs"), "{text}");
        assert!(text.contains("2 CLAUDE.md"), "{text}");
    }
}
