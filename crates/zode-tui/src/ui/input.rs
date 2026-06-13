//! Multi-line input box (tui-textarea wrapper). The front-end intercepts
//! Enter (submit) and Shift/Alt+Enter (newline) before forwarding other
//! keys here.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;
use tui_textarea::TextArea;

pub struct InputBox {
    area: TextArea<'static>,
}

impl Default for InputBox {
    fn default() -> Self {
        Self::new()
    }
}

impl InputBox {
    pub fn new() -> Self {
        Self {
            area: TextArea::default(),
        }
    }

    pub fn insert_str(&mut self, s: &str) {
        self.area.insert_str(s);
    }

    pub fn insert_newline(&mut self) {
        self.area.insert_newline();
    }

    /// Forward a key event to the textarea.
    pub fn input(&mut self, ev: crossterm::event::KeyEvent) {
        self.area.input(ev);
    }

    pub fn text(&self) -> String {
        self.area.lines().join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.area.lines().iter().all(|l| l.is_empty())
    }

    /// Return the current text and clear the box.
    pub fn take(&mut self) -> String {
        let text = self.text();
        self.area = TextArea::default();
        text
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
        let mut ta = self.area.clone();
        ta.set_block(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.bg_input).fg(theme.fg_text)),
        );
        f.render_widget(&ta, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_submits_and_clears() {
        let mut ib = InputBox::new();
        ib.insert_str("hello world");
        assert_eq!(ib.text(), "hello world");
        let taken = ib.take();
        assert_eq!(taken, "hello world");
        assert!(ib.is_empty());
    }

    #[test]
    fn empty_take_returns_empty() {
        let mut ib = InputBox::new();
        assert_eq!(ib.take(), "");
    }
}
