//! Left session rail: one row per session tab. The active tab is highlighted;
//! a busy tab (turn in flight) is marked with a dot. Rendered only when more
//! than one tab exists, so single-tab use looks unchanged.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tab::SessionTab;
use crate::theme::Theme;

pub fn tab_label(index: usize, title: &str, busy: bool) -> String {
    if busy {
        format!("{index} ● {title}")
    } else {
        format!("{index} {title}")
    }
}

pub fn render_tabs(f: &mut Frame, area: Rect, tabs: &[SessionTab], active: usize, theme: &Theme) {
    let row_width = area.width.saturating_sub(1) as usize;
    let mut lines = vec![Line::styled(
        format!("{:<row_width$}", " sessions "),
        Style::default()
            .fg(theme.fg_subtle)
            .add_modifier(Modifier::BOLD),
    )];
    for (i, tab) in tabs
        .iter()
        .enumerate()
        .take(area.height.saturating_sub(1) as usize)
    {
        let label = tab_label(i + 1, &tab.title, tab.is_busy());
        let marker = if i == active { "▸ " } else { "  " };
        let row_style = if i == active {
            Style::default()
                .bg(theme.accent)
                .fg(theme.bg_primary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(theme.bg_secondary).fg(theme.fg_text)
        };
        let row = format!("{marker}{label}");
        lines.push(Line::from(Span::styled(
            format!("{row:<row_width$}"),
            row_style,
        )));
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::RIGHT)
                    .border_style(Style::default().fg(theme.separator)),
            )
            .style(Style::default().bg(theme.bg_secondary)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_label_marks_busy_tabs() {
        assert_eq!(tab_label(1, "main", false), "1 main");
        assert_eq!(tab_label(2, "work", true), "2 ● work");
    }
}
