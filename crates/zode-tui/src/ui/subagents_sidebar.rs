//! Compact "subagents" sidebar section: the active tab's Task-spawned
//! sub-agents under their own header, distinct from the sessions tab list.
//! Full step-by-step detail lives in the F2 / `/subagents` overlay; this is an
//! at-a-glance list. Borderless, styled to match the other sidebar sections.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;
use zode_core::{SubAgent, SubAgentStatus};

use crate::theme::Theme;
use crate::ui::tabs::truncate_to_width;

/// Max sub-agent rows shown before collapsing into a "…+k more" row.
const CAP: usize = 6;

fn glyph(s: SubAgentStatus) -> char {
    match s {
        SubAgentStatus::Running => '◐',
        SubAgentStatus::Done => '✓',
        SubAgentStatus::Failed => '✗',
    }
}

fn color(s: SubAgentStatus, theme: &Theme) -> Color {
    match s {
        SubAgentStatus::Running => theme.accent,
        SubAgentStatus::Done => theme.fg_subtle,
        SubAgentStatus::Failed => Color::Red,
    }
}

/// Rows this section occupies, or 0 when there are no sub-agents: a blank
/// separator + a header + up to `CAP` item rows (the last folds into a
/// "…+k more" row when the list overflows).
pub(crate) fn section_height(count: usize) -> usize {
    if count == 0 {
        0
    } else {
        2 + count.min(CAP)
    }
}

/// The compact label for one sub-agent row: `type` or `type · description`.
fn row_label(a: &SubAgent) -> String {
    match &a.description {
        Some(d) if !d.is_empty() => format!("{} · {}", a.agent_type, d),
        _ => a.agent_type.clone(),
    }
}

/// Build the section's lines (empty when there are no sub-agents). `width` is
/// the full sidebar row width. Newest-first ordering is the caller's.
pub(crate) fn section_lines(
    subagents: &[SubAgent],
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if subagents.is_empty() {
        return Vec::new();
    }
    let bg = Style::default().bg(theme.bg_secondary);
    let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled(" ".repeat(width), bg))];

    // Header: " subagents" (accent, bold) + right-aligned `running/total`.
    let running = subagents
        .iter()
        .filter(|a| a.status == SubAgentStatus::Running)
        .count();
    let count = format!("{running}/{}", subagents.len());
    let label = crate::tr("subagents");
    let left_w = 1 + UnicodeWidthStr::width(label);
    let right_w = UnicodeWidthStr::width(count.as_str()) + 1;
    let pad = width.saturating_sub(left_w + right_w);
    lines.push(Line::from(vec![
        Span::styled(
            format!(" {label}"),
            bg.fg(theme.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ".repeat(pad), bg),
        Span::styled(format!("{count} "), bg.fg(theme.fg_subtle)),
    ]));

    // Item rows: " " (leading pad) + glyph + " " + label (truncated).
    let subject_width = width.saturating_sub(3);
    let (shown, overflow) = if subagents.len() > CAP {
        (CAP - 1, subagents.len() - (CAP - 1))
    } else {
        (subagents.len(), 0)
    };
    for a in subagents.iter().take(shown) {
        let text = format!(
            " {} {}",
            glyph(a.status),
            truncate_to_width(&row_label(a), subject_width)
        );
        lines.push(Line::from(Span::styled(
            text,
            bg.fg(color(a.status, theme)),
        )));
    }
    if overflow > 0 {
        lines.push(Line::from(Span::styled(
            format!("   …+{overflow} {}", crate::tr("more")),
            bg.fg(theme.fg_subtle),
        )));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_core::{SubAgent, SubAgentStatus};

    fn sa(ty: &str, status: SubAgentStatus) -> SubAgent {
        SubAgent {
            id: 1,
            agent_type: ty.into(),
            description: Some("desc".into()),
            depth: 0,
            status,
            started_at: 0,
            finished_at: None,
            input_tokens: 0,
            output_tokens: 0,
            transcript: Vec::new(),
        }
    }

    fn joined(lines: &[Line]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect()
    }

    #[test]
    fn empty_list_renders_nothing() {
        assert_eq!(section_height(0), 0);
        assert!(section_lines(&[], 30, &theme()).is_empty());
    }

    fn theme() -> Theme {
        crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"))
    }

    #[test]
    fn header_and_rows_render() {
        let agents = vec![
            sa("researcher", SubAgentStatus::Running),
            sa("planner", SubAgentStatus::Done),
        ];
        assert_eq!(section_height(2), 4); // blank + header + 2 rows
        let j = joined(&section_lines(&agents, 30, &theme()));
        assert!(j.contains("subagents"));
        assert!(j.contains("1/2")); // 1 running / 2 total
        assert!(j.contains("researcher"));
        assert!(j.contains("planner"));
        assert!(j.contains('◐')); // running glyph
        assert!(j.contains('✓')); // done glyph
    }

    #[test]
    fn overflow_collapses_to_more_row() {
        let agents: Vec<SubAgent> = (0..10).map(|_| sa("a", SubAgentStatus::Running)).collect();
        assert_eq!(section_height(10), 2 + CAP); // capped
        let j = joined(&section_lines(&agents, 30, &theme()));
        // CAP-1 item rows + a "…+k more" row (k = 10 - (CAP-1)).
        assert!(j.contains(&format!("…+{} more", 10 - (CAP - 1))));
    }
}
