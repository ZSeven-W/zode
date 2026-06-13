//! Top tab bar: one segment per session tab. The active tab is highlighted;
//! a busy tab (turn in flight) is marked with a leading dot. Rendered only
//! when more than one tab exists, so single-tab use looks unchanged.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tab::SessionTab;
use crate::theme::Theme;

pub fn tab_label(index: usize, title: &str, busy: bool) -> String {
    if busy {
        format!(" {index} ● {title} ")
    } else {
        format!(" {index} {title} ")
    }
}

pub fn render_tabs(f: &mut Frame, area: Rect, tabs: &[SessionTab], active: usize, theme: &Theme) {
    let mut spans = Vec::new();
    for (i, tab) in tabs.iter().enumerate() {
        let label = tab_label(i + 1, &tab.title, tab.is_busy());
        let style = if i == active {
            Style::default()
                .bg(theme.accent)
                .fg(theme.bg_primary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(theme.bg_secondary).fg(theme.fg_text)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.bg_primary)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_label_marks_busy_tabs() {
        assert_eq!(tab_label(1, "main", false), " 1 main ");
        assert_eq!(tab_label(2, "work", true), " 2 ● work ");
    }
}
