//! Collapsible "mcp" sidebar section: one row per configured MCP server with
//! its live connection state. Borderless, styled to match the other sidebar
//! sections; the ▼/▶ header row is a click target (fold toggle).

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;
use crate::ui::tabs::truncate_to_width;

/// Build the section's lines (empty when there are no servers): a blank
/// separator + a header, plus one row per server when expanded. Unlike the
/// sections below the tab list, no height fn is needed — this one flows
/// ABOVE the tab list, so the tab budget already sees it via `lines.len()`.
/// `width` is the full sidebar row width; `servers` is `(name, connected)`.
pub(crate) fn section_lines(
    servers: &[(String, bool)],
    collapsed: bool,
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if servers.is_empty() {
        return Vec::new();
    }
    let bg = Style::default().bg(theme.bg_secondary);
    let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled(" ".repeat(width), bg))];

    // Header: " ▼ mcp" (accent, bold) + right-aligned `connected/total`.
    let arrow = if collapsed { '▶' } else { '▼' };
    let connected = servers.iter().filter(|(_, on)| *on).count();
    let count = format!("{connected}/{}", servers.len());
    let label = format!(" {arrow} mcp");
    let pad = width.saturating_sub(
        UnicodeWidthStr::width(label.as_str()) + UnicodeWidthStr::width(count.as_str()) + 1,
    );
    lines.push(Line::from(vec![
        Span::styled(label, bg.fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" ".repeat(pad), bg),
        Span::styled(format!("{count} "), bg.fg(theme.fg_subtle)),
    ]));
    if collapsed {
        return lines;
    }

    // Item rows: " ● name" + right-aligned connection state.
    for (name, on) in servers {
        let state = if *on {
            crate::tr("connected")
        } else {
            crate::tr("disconnected")
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
        lines.push(Line::from(vec![
            Span::styled(" ● ", bg.fg(dot_fg)),
            Span::styled(name, bg.fg(theme.fg_text)),
            Span::styled(" ".repeat(pad), bg),
            Span::styled(format!("{state} "), bg.fg(state_fg)),
        ]));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"))
    }

    fn joined(lines: &[Line]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect()
    }

    #[test]
    fn empty_list_renders_nothing() {
        assert!(section_lines(&[], false, 30, &theme()).is_empty());
        assert!(section_lines(&[], true, 30, &theme()).is_empty());
    }

    #[test]
    fn header_and_rows_render_with_connection_state() {
        let servers = vec![
            ("chrome-devtools".to_string(), true),
            ("gateway".to_string(), false),
        ];
        let lines = section_lines(&servers, false, 34, &theme());
        assert_eq!(lines.len(), 4); // blank + header + 2 rows
        let j = joined(&lines);
        assert!(j.contains("▼ mcp"));
        assert!(j.contains("1/2")); // 1 connected / 2 total
        assert!(j.contains("chrome-devtools"));
        assert!(j.contains("connected"));
        assert!(j.contains("disconnected"));
    }

    #[test]
    fn collapsed_keeps_only_the_header() {
        let servers = vec![("chrome-devtools".to_string(), true)];
        let lines = section_lines(&servers, true, 34, &theme());
        assert_eq!(lines.len(), 2); // blank + header
        let j = joined(&lines);
        assert!(j.contains("▶ mcp"));
        assert!(!j.contains("chrome-devtools"));
    }
}
