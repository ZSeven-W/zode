//! Right session rail: one row per session tab. The active tab is highlighted;
//! a busy tab (turn in flight) is marked with a dot. Rendered only when more
//! than one tab exists, so single-tab use looks unchanged.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::tab::SessionTab;
use crate::theme::Theme;

pub fn tab_label(index: usize, title: &str, busy: bool) -> String {
    if busy {
        format!("{index} ● {title}")
    } else {
        format!("{index} {title}")
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TabRowParts {
    index: String,
    status: String,
    title: String,
    padding: String,
}

#[cfg(test)]
fn format_tab_row(index: usize, title: &str, busy: bool, width: usize) -> String {
    let parts = tab_row_parts(index, title, busy, width);
    format!(
        "{}{}{}{}",
        parts.index, parts.status, parts.title, parts.padding
    )
}

fn tab_row_parts(index: usize, title: &str, busy: bool, width: usize) -> TabRowParts {
    let index = format!("{index:>2}");
    let status = if busy {
        " ● ".to_string()
    } else {
        "   ".to_string()
    };
    let prefix_width =
        UnicodeWidthStr::width(index.as_str()) + UnicodeWidthStr::width(status.as_str());
    let title_width = width.saturating_sub(prefix_width);
    let title = truncate_to_width(title, title_width);
    let used = prefix_width + UnicodeWidthStr::width(title.as_str());
    let padding = " ".repeat(width.saturating_sub(used));

    TabRowParts {
        index,
        status,
        title,
        padding,
    }
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    let width = UnicodeWidthStr::width(text);
    if width <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let mut out = String::new();
    let mut used = 0;
    let body_width = max_width.saturating_sub(1);
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > body_width {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push('…');
    out
}

pub fn render_tabs(f: &mut Frame, area: Rect, tabs: &[SessionTab], active: usize, theme: &Theme) {
    let row_width = area.width.saturating_sub(1) as usize;
    let content_width = row_width.saturating_sub(2);
    let mut lines = vec![header_line(row_width, theme)];
    for (i, tab) in tabs
        .iter()
        .enumerate()
        .take(area.height.saturating_sub(1) as usize)
    {
        let row_active = i == active;
        let row_bg = if row_active {
            theme.bg_input
        } else {
            theme.bg_secondary
        };
        let marker_style = if row_active {
            Style::default()
                .bg(row_bg)
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(row_bg).fg(theme.bg_secondary)
        };
        let index_style = if row_active {
            Style::default()
                .bg(row_bg)
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(row_bg).fg(theme.fg_subtle)
        };
        let title_style = if row_active {
            Style::default()
                .bg(row_bg)
                .fg(theme.fg_white)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(row_bg).fg(theme.fg_text)
        };
        let parts = tab_row_parts(i + 1, &tab.title, tab.is_busy(), content_width);
        lines.push(Line::from(vec![
            Span::styled(if row_active { "▌" } else { " " }, marker_style),
            Span::styled(" ", Style::default().bg(row_bg)),
            Span::styled(parts.index, index_style),
            Span::styled(
                parts.status,
                Style::default().bg(row_bg).fg(theme.accent_secondary),
            ),
            Span::styled(parts.title, title_style),
            Span::styled(parts.padding, Style::default().bg(row_bg)),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::LEFT)
                    .border_style(Style::default().fg(theme.separator)),
            )
            .style(Style::default().bg(theme.bg_secondary)),
        area,
    );
}

fn header_line(width: usize, theme: &Theme) -> Line<'static> {
    let title = " sessions";
    let padding = " ".repeat(width.saturating_sub(UnicodeWidthStr::width(title)));
    Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(theme.fg_subtle)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(padding, Style::default().bg(theme.bg_secondary)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_label_marks_busy_tabs() {
        assert_eq!(tab_label(1, "main", false), "1 main");
        assert_eq!(tab_label(2, "work", true), "2 ● work");
    }

    #[test]
    fn format_tab_row_truncates_titles_to_inner_width() {
        assert_eq!(
            format_tab_row(12, "very-long-session-title", true, 14),
            "12 ● very-lon…"
        );
    }
}
