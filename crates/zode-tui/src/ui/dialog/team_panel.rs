//! `/team` status panel: roster + board summary, opened by bare `/team`.
//! Read-only and scrollable (mutations go through `/team dismiss` and the
//! team_* tools). Self-contained modal, mirroring `browser_panel`'s shape.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::theme::Theme;

pub const PANEL_BLURB: &str = "A team is a set of persistent teammates (external CLIs or internal \
     agents) coordinated through a shared board. Manage with team_* tools \
     and /team dismiss <name>.";

/// One roster row for display.
#[derive(Debug, Clone)]
pub struct TeamPanelRow {
    pub name: String,
    pub model_label: String,
    pub status_line: String,
    pub usage_in: u64,
    pub usage_out: u64,
}

pub struct TeamPanel {
    rows: Vec<TeamPanelRow>,
    board_summary: Vec<String>,
    scroll: u16,
}

impl TeamPanel {
    pub fn new(rows: Vec<TeamPanelRow>, board_summary: Vec<String>) -> Self {
        Self {
            rows,
            board_summary,
            scroll: 0,
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }
    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    fn lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut out: Vec<Line<'static>> = Vec::new();
        let subtle = Style::default().fg(theme.fg_subtle);
        let accent = Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD);
        out.push(Line::styled(PANEL_BLURB.to_string(), subtle));
        out.push(Line::from(""));
        if self.rows.is_empty() {
            out.push(Line::styled(
                "No teammates yet.".to_string(),
                Style::default().fg(theme.fg_text),
            ));
        } else {
            out.push(Line::styled(
                format!("Roster ({}):", self.rows.len()),
                accent,
            ));
            for r in &self.rows {
                out.push(Line::styled(
                    format!(
                        "  {}  [{}]  {}  ↑{} ↓{}",
                        r.name, r.model_label, r.status_line, r.usage_in, r.usage_out
                    ),
                    Style::default().fg(theme.fg_text),
                ));
            }
        }
        out.push(Line::from(""));
        out.push(Line::styled("Board:".to_string(), accent));
        if self.board_summary.is_empty() {
            out.push(Line::styled("  (empty)".to_string(), subtle));
        } else {
            for l in &self.board_summary {
                out.push(Line::styled(
                    format!("  {l}"),
                    Style::default().fg(theme.fg_text),
                ));
            }
        }
        out.push(Line::from(""));
        out.push(Line::styled("↑↓ scroll · esc close".to_string(), subtle));
        out
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let popup = modal_area(area);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_secondary)),
            popup,
        );
        let inner = Rect {
            x: popup.x.saturating_add(3),
            y: popup.y.saturating_add(1),
            width: popup.width.saturating_sub(6),
            height: popup.height.saturating_sub(2),
        };
        let para = Paragraph::new(self.lines(theme))
            .style(Style::default().bg(theme.bg_secondary).fg(theme.fg_text))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        f.render_widget(para, inner);
    }
}

fn modal_area(area: Rect) -> Rect {
    let max_w = area.width.saturating_sub(4);
    let max_h = area.height.saturating_sub(4);
    let width = max_w.min(72).max(max_w.min(44));
    let height = max_h.min(20).max(max_h.min(14));
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}
