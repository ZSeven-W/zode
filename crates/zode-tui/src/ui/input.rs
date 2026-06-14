//! Multi-line input box (tui-textarea wrapper). The front-end intercepts
//! Enter (submit) and Shift/Alt+Enter (newline) before forwarding other
//! keys here.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use tui_textarea::TextArea;
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;
use crate::ui::status::Mode;

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

    pub fn render(
        &self,
        f: &mut Frame,
        area: Rect,
        theme: &Theme,
        mode: Mode,
        completion_placeholder: Option<&str>,
    ) {
        let border_color = match mode {
            Mode::Ready => theme.accent_secondary,
            Mode::Thinking => theme.system,
            Mode::Streaming => theme.accent,
            Mode::Error => Color::Red,
        };
        let title = Line::styled(
            " prompt ",
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        );
        let hint = Line::styled(
            " Enter send · Shift/Alt+Enter newline ",
            Style::default().fg(theme.fg_subtle),
        );
        let mut ta = self.area.clone();
        ta.set_block(
            Block::default()
                .title(title)
                .title_bottom(hint)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .style(Style::default().bg(theme.bg_input).fg(theme.fg_text)),
        );
        f.render_widget(&ta, area);
        self.render_completion_placeholder(f, area, theme, completion_placeholder);
    }

    fn render_completion_placeholder(
        &self,
        f: &mut Frame,
        area: Rect,
        theme: &Theme,
        completion_placeholder: Option<&str>,
    ) {
        let Some(placeholder) = completion_placeholder.filter(|p| !p.is_empty()) else {
            return;
        };
        let lines = self.area.lines();
        if lines.len() != 1 {
            return;
        }
        let (row, col) = self.area.cursor();
        if row != 0 || col != lines[0].chars().count() {
            return;
        }
        let inner = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let offset = UnicodeWidthStr::width(lines[0].as_str()).saturating_add(1);
        if offset >= inner.width as usize {
            return;
        }
        let x = inner.x.saturating_add(offset as u16);
        let width = inner.width.saturating_sub(offset as u16);
        let hint = Paragraph::new(Line::from(vec![Span::styled(
            placeholder.to_string(),
            Style::default().bg(theme.bg_input).fg(theme.fg_subtle),
        )]));
        f.render_widget(hint, Rect::new(x, inner.y, width, 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStore;
    use crate::ui::status::Mode;
    use ratatui::{backend::TestBackend, Terminal};

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

    #[test]
    fn renders_composer_title_hint_and_text() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut ib = InputBox::new();
        ib.insert_str("hello");
        let backend = TestBackend::new(80, 4);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| ib.render(f, f.area(), &theme, Mode::Ready, None))
            .unwrap();
        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("prompt"));
        assert!(content.contains("Enter send"));
        assert!(content.contains("hello"));
    }

    #[test]
    fn renders_completion_placeholder_without_changing_text() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut ib = InputBox::new();
        ib.insert_str("/sidebar ");
        let backend = TestBackend::new(80, 4);
        let mut term = Terminal::new(backend).unwrap();

        term.draw(|f| {
            ib.render(
                f,
                f.area(),
                &theme,
                Mode::Ready,
                Some("[on|off|toggle|auto]"),
            )
        })
        .unwrap();

        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert_eq!(ib.text(), "/sidebar ");
        assert!(content.contains("/sidebar "));
        assert!(content.contains("[on|off|toggle|auto]"));
    }
}
