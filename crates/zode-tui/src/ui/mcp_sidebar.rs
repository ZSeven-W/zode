//! Collapsible status-list sidebar sections (MCP servers, LSP languages):
//! one row per item with its live state. Borderless, styled to match the
//! other sidebar sections; the ▼/▶ header row is a click target (fold
//! toggle).

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;
use crate::ui::tabs::{fit_line_to_width, truncate_to_width};

/// Build a section's lines (empty when there are no items): a blank
/// separator + a header, plus one row per item when expanded. Unlike the
/// sections below the tab list, no height fn is needed — these flow ABOVE
/// the tab list, so the tab budget already sees them via `lines.len()`.
/// `title` is the header label (e.g. "MCP", "LSP"); `items` is
/// `(name, active)`; `(on_label, off_label)` are the per-row state texts
/// (i18n keys, e.g. "connected"/"disconnected" or "running"/"idle");
/// `width` is the full sidebar row width.
pub(crate) fn section_lines(
    title: &str,
    items: &[(String, bool)],
    (on_label, off_label): (&'static str, &'static str),
    collapsed: bool,
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if items.is_empty() {
        return Vec::new();
    }
    let bg = Style::default().bg(theme.bg_secondary);
    let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled(" ".repeat(width), bg))];

    // Header: " ▼ <title>" (accent, bold) + right-aligned `active/total`.
    let arrow = if collapsed { '▶' } else { '▼' };
    let active = items.iter().filter(|(_, on)| *on).count();
    let count = format!("{active}/{}", items.len());
    let label = format!(" {arrow} {title}");
    let pad = width.saturating_sub(
        UnicodeWidthStr::width(label.as_str()) + UnicodeWidthStr::width(count.as_str()) + 1,
    );
    lines.push(fit_line_to_width(
        Line::from(vec![
            Span::styled(label, bg.fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" ".repeat(pad), bg),
            Span::styled(format!("{count} "), bg.fg(theme.fg_subtle)),
        ]),
        width,
        bg,
    ));
    if collapsed {
        return lines;
    }

    // Item rows: " ● name" + right-aligned state.
    for (name, on) in items {
        let state = if *on {
            crate::tr(on_label)
        } else {
            crate::tr(off_label)
        };
        let state_w = UnicodeWidthStr::width(state) + 1;
        let name_w = width.saturating_sub(3 + state_w + 1);
        let name = truncate_to_width(name, name_w);
        let pad = width.saturating_sub(3 + UnicodeWidthStr::width(name.as_str()) + state_w);
        let (dot_fg, state_fg) = if *on {
            (Color::Green, Color::Green)
        } else {
            (theme.fg_subtle, theme.fg_subtle)
        };
        lines.push(fit_line_to_width(
            Line::from(vec![
                Span::styled(" ● ", bg.fg(dot_fg)),
                Span::styled(name, bg.fg(theme.fg_text)),
                Span::styled(" ".repeat(pad), bg),
                Span::styled(format!("{state} "), bg.fg(state_fg)),
            ]),
            width,
            bg,
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const MCP_STATES: (&str, &str) = ("connected", "disconnected");

    fn theme() -> Theme {
        crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"))
    }

    fn joined(lines: &[Line]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect()
    }

    fn line_width(line: &Line) -> usize {
        line.spans
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum()
    }

    #[test]
    fn empty_list_renders_nothing() {
        assert!(section_lines("MCP", &[], MCP_STATES, false, 30, &theme()).is_empty());
        assert!(section_lines("MCP", &[], MCP_STATES, true, 30, &theme()).is_empty());
    }

    #[test]
    fn header_and_rows_render_with_connection_state() {
        let servers = vec![
            ("chrome-devtools".to_string(), true),
            ("gateway".to_string(), false),
        ];
        let lines = section_lines("MCP", &servers, MCP_STATES, false, 34, &theme());
        assert_eq!(lines.len(), 4); // blank + header + 2 rows
        let j = joined(&lines);
        assert!(j.contains("▼ MCP"));
        assert!(j.contains("1/2")); // 1 connected / 2 total
        assert!(j.contains("chrome-devtools"));
        assert!(j.contains("connected"));
        assert!(j.contains("disconnected"));
    }

    #[test]
    fn collapsed_keeps_only_the_header() {
        let servers = vec![("chrome-devtools".to_string(), true)];
        let lines = section_lines("MCP", &servers, MCP_STATES, true, 34, &theme());
        assert_eq!(lines.len(), 2); // blank + header
        let j = joined(&lines);
        assert!(j.contains("▶ MCP"));
        assert!(!j.contains("chrome-devtools"));
    }

    #[test]
    fn lsp_flavor_uses_running_idle_states() {
        let langs = vec![("rust".to_string(), true), ("python".to_string(), false)];
        let lines = section_lines("LSP", &langs, ("running", "idle"), false, 34, &theme());
        let j = joined(&lines);
        assert!(j.contains("▼ LSP"));
        assert!(j.contains("1/2"));
        assert!(j.contains("rust"));
        assert!(j.contains("running"));
        assert!(j.contains("idle"));
    }

    #[test]
    fn narrow_rows_never_exceed_the_section_width() {
        let servers = vec![("very-long-devtools-server".to_string(), true)];
        let lines = section_lines("MCP", &servers, MCP_STATES, false, 8, &theme());
        for line in &lines {
            assert!(
                line_width(line) <= 8,
                "line width {} exceeded 8: {line:?}",
                line_width(line)
            );
        }
    }
}
