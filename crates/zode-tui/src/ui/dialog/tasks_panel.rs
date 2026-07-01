//! Background tasks panel (Ctrl+B / /tasks): the active tab's background
//! shells (from BackgroundShellTracker) plus a per-tab running-turn summary.
//! `k` kills the selected shell via the engine's KillShell path. Rendering of
//! the shell rows is a pure function so it can be unit-tested without a Frame.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use zode_core::bg_shells::BgShell;

use crate::theme::Theme;
use crate::ui::centered;

/// One display line per background shell: status mark + id + age + command.
pub fn shell_rows(shells: &[BgShell], now: u64) -> Vec<Line<'static>> {
    shells
        .iter()
        .map(|s| {
            let (mark, color) = if s.killed {
                (format!("✗ {}", crate::tr("killed")), Color::Red)
            } else {
                (format!("● {}", crate::tr("running")), Color::Green)
            };
            let dur = now.saturating_sub(s.started_at);
            Line::from(vec![
                Span::styled(format!("{mark} "), Style::default().fg(color)),
                Span::raw(format!("{}  {}s  {}", s.shell_id, dur, s.command)),
            ])
        })
        .collect()
}

pub struct TasksPanel {
    state: ListState,
}

impl Default for TasksPanel {
    fn default() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self { state }
    }
}

impl TasksPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        let i = (self.state.selected().unwrap_or(0) + 1) % len;
        self.state.select(Some(i));
    }

    pub fn prev(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        let cur = self.state.selected().unwrap_or(0);
        let i = if cur == 0 { len - 1 } else { cur - 1 };
        self.state.select(Some(i));
    }

    pub fn selected(&self) -> usize {
        self.state.selected().unwrap_or(0)
    }

    pub fn render(
        &mut self,
        f: &mut Frame,
        area: Rect,
        shells: &[BgShell],
        turns: &[String],
        now: u64,
        theme: &Theme,
    ) {
        let popup = centered(area, 70, 60);
        f.render_widget(Clear, popup);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(popup);

        let items: Vec<ListItem> = shell_rows(shells, now)
            .into_iter()
            .map(ListItem::new)
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .title(format!(
                        " {}  [k] {}  [Esc] {} ",
                        crate::tr("Background shells"),
                        crate::tr("kill"),
                        crate::tr("close")
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent))
                    .style(Style::default().bg(theme.bg_secondary).fg(theme.fg_text)),
            )
            .highlight_style(
                Style::default()
                    .bg(theme.accent)
                    .fg(theme.bg_primary)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, rows[0], &mut self.state);

        let turn_lines: Vec<Line> = if turns.is_empty() {
            vec![Line::from(crate::tr("(no running turns)"))]
        } else {
            turns.iter().map(|t| Line::from(t.clone())).collect()
        };
        f.render_widget(
            Paragraph::new(turn_lines).block(
                Block::default()
                    .title(format!(" {} ", crate::tr("Running turns")))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent_secondary))
                    .style(Style::default().bg(theme.bg_secondary).fg(theme.fg_text)),
            ),
            rows[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shells() -> Vec<BgShell> {
        vec![
            BgShell {
                shell_id: "sh1".into(),
                command: "sleep 100".into(),
                started_at: 0,
                killed: false,
            },
            BgShell {
                shell_id: "sh2".into(),
                command: "tail -f x".into(),
                started_at: 0,
                killed: true,
            },
        ]
    }

    #[test]
    fn renders_shell_rows_with_status_and_command() {
        let lines = shell_rows(&shells(), 1000);
        assert_eq!(lines.len(), 2);
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(joined.contains("sleep 100"));
        assert!(joined.contains("running"));
        assert!(joined.contains("killed"));
        assert!(joined.contains("1000s")); // age = now - started_at
    }

    #[test]
    fn next_prev_wrap_within_bounds() {
        let mut p = TasksPanel::new();
        assert_eq!(p.selected(), 0);
        p.next(2);
        assert_eq!(p.selected(), 1);
        p.next(2); // wraps to 0
        assert_eq!(p.selected(), 0);
        p.prev(2); // wraps to last
        assert_eq!(p.selected(), 1);
    }
}
