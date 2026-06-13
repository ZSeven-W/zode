//! Transient toast (auto-dismiss after N ticks). Info/Error coloring.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::theme::Theme;

#[derive(Debug, Clone, Copy)]
pub enum ToastKind {
    Info,
    Error,
}

pub struct Toast {
    text: String,
    kind: ToastKind,
    /// Remaining ticks before auto-dismiss (~100ms each → 30 = 3s).
    ttl: u8,
}

impl Toast {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: ToastKind::Info,
            ttl: 30,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: ToastKind::Error,
            ttl: 50,
        }
    }

    /// Decrement TTL on tick; returns true if expired.
    pub fn tick(&mut self) -> bool {
        self.ttl = self.ttl.saturating_sub(1);
        self.ttl == 0
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let color = match self.kind {
            ToastKind::Info => theme.accent,
            ToastKind::Error => Color::Red,
        };
        let w = (self.text.chars().count() as u16 + 4).min(area.width);
        let popup = Rect {
            x: area.x + area.width.saturating_sub(w),
            y: area.y,
            width: w,
            height: 3,
        };
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new(self.text.clone()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(color))
                    .style(Style::default().bg(theme.bg_secondary).fg(theme.fg_text)),
            ),
            popup,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_expires_after_ttl() {
        let mut t = Toast::info("hi");
        let mut expired = false;
        for _ in 0..30 {
            expired = t.tick();
        }
        assert!(expired);
    }
}
