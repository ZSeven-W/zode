//! Multi-line input box (tui-textarea wrapper). The front-end intercepts
//! Enter (submit) and Shift/Alt+Enter (newline) before forwarding other
//! keys here.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
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
        let status_color = match mode {
            Mode::Ready => theme.accent_secondary,
            Mode::Thinking => theme.system,
            Mode::Streaming => theme.accent,
            Mode::Error => Color::Red,
        };
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_input)),
            area,
        );
        if area.width > 0 && area.height > 0 {
            let rail_lines: Vec<Line<'static>> = (0..area.height)
                .map(|_| {
                    Line::from(Span::styled(
                        "▌",
                        Style::default().bg(theme.bg_input).fg(status_color),
                    ))
                })
                .collect();
            f.render_widget(
                Paragraph::new(rail_lines).style(Style::default().bg(theme.bg_input)),
                Rect::new(area.x, area.y, 1, area.height),
            );
        }

        let body = input_body_area(area);
        let mut ta = self.area.clone();
        ta.set_cursor_line_style(Style::default().bg(theme.bg_input).fg(theme.fg_text));
        ta.set_block(
            Block::default()
                .borders(Borders::NONE)
                .border_style(Style::default().fg(status_color))
                .style(Style::default().bg(theme.bg_input).fg(theme.fg_text)),
        );
        f.render_widget(&ta, body);
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
        let inner = input_body_area(area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let offset = UnicodeWidthStr::width(lines[0].as_str());
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

fn input_body_area(area: Rect) -> Rect {
    let top_padding = u16::from(area.height > 1);
    let bottom_padding = u16::from(area.height > 2);
    Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(top_padding),
        width: area.width.saturating_sub(2),
        height: area
            .height
            .saturating_sub(top_padding)
            .saturating_sub(bottom_padding),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStore;
    use crate::ui::status::Mode;
    use ratatui::style::Modifier;
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
    fn renders_composer_text_without_inline_metadata() {
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
        assert!(!content.contains("prompt"));
        assert!(!content.contains("Enter send"));
        assert!(content.contains("hello"));
        assert!(!content.contains("zode"));
        assert!(!content.contains("deepseek-v4-pro"));
        assert!(!content.contains("─"));
        assert!(!content.contains("│"));
        let buf = term.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), "▌");
        assert_eq!(buf[(0, 0)].fg, theme.accent_secondary);
    }

    #[test]
    fn input_text_does_not_use_cursor_line_underline() {
        let theme = ThemeStore::with_builtins().resolve(Some("cyberpunk"));
        let mut ib = InputBox::new();
        ib.insert_str("hello");
        let backend = TestBackend::new(40, 4);
        let mut term = Terminal::new(backend).unwrap();

        term.draw(|f| ib.render(f, f.area(), &theme, Mode::Ready, None))
            .unwrap();

        let buf = term.backend().buffer();
        assert_eq!(buf[(2, 0)].symbol(), " ");
        assert_eq!(buf[(2, 1)].symbol(), "h");
        assert_eq!(buf[(2, 3)].symbol(), " ");
        assert!(
            !buf[(2, 1)].modifier.contains(Modifier::UNDERLINED),
            "input text should not inherit tui-textarea's default cursor-line underline"
        );
    }

    #[test]
    fn composer_keeps_bottom_padding_for_multiline_input() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut ib = InputBox::new();
        ib.insert_str("one");
        ib.insert_newline();
        ib.insert_str("two");
        ib.insert_newline();
        ib.insert_str("three");
        let backend = TestBackend::new(40, 4);
        let mut term = Terminal::new(backend).unwrap();

        term.draw(|f| ib.render(f, f.area(), &theme, Mode::Ready, None))
            .unwrap();

        let buf = term.backend().buffer();
        let bottom_row: String = (0..buf.area.width).map(|x| buf[(x, 3)].symbol()).collect();
        assert!(
            bottom_row.starts_with("▌") && !bottom_row.contains("three"),
            "composer bottom row should stay as rail-only padding: {bottom_row:?}"
        );
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
