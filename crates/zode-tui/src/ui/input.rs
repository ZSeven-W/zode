//! Multi-line input box (tui-textarea wrapper). The front-end intercepts
//! Enter (submit) and Shift/Alt+Enter (newline) before forwarding other
//! keys here.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use tui_textarea::TextArea;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

    /// True if the cursor sits on the first line — Up then recalls history
    /// instead of moving the cursor up within multi-line text.
    pub fn cursor_on_first_line(&self) -> bool {
        self.area.cursor().0 == 0
    }

    /// True if the cursor sits on the last line — Down then advances history.
    pub fn cursor_on_last_line(&self) -> bool {
        self.area.cursor().0 + 1 >= self.area.lines().len().max(1)
    }

    /// Replace the whole input with `s`, leaving the cursor at the end. Used to
    /// recall a prompt from history.
    pub fn set_text(&mut self, s: &str) {
        let mut area = TextArea::from(s.split('\n').map(str::to_string).collect::<Vec<_>>());
        area.move_cursor(tui_textarea::CursorMove::Bottom);
        area.move_cursor(tui_textarea::CursorMove::End);
        self.area = area;
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
        self.render_wrapped_text(f, body, theme);
        self.render_completion_placeholder(f, area, theme, completion_placeholder);
    }

    fn render_wrapped_text(&self, f: &mut Frame, body: Rect, theme: &Theme) {
        if body.width == 0 || body.height == 0 {
            return;
        }
        let style = Style::default().bg(theme.bg_input).fg(theme.fg_text);
        let rows = wrap_input_lines(self.area.lines(), body.width);
        let (cursor_row, cursor_col) =
            wrapped_cursor_position(&rows, self.area.cursor(), body.width).unwrap_or((0, 0));
        let visible_height = body.height as usize;
        let start = cursor_row.saturating_add(1).saturating_sub(visible_height);
        let visible_rows: Vec<Line<'static>> = rows
            .iter()
            .skip(start)
            .take(visible_height)
            .map(|row| Line::from(Span::styled(row.text.clone(), style)))
            .collect();

        f.render_widget(Paragraph::new(visible_rows).style(style), body);
        if cursor_row >= start && cursor_row < start.saturating_add(visible_height) {
            let max_x = body.width.saturating_sub(1) as usize;
            f.set_cursor_position((
                body.x.saturating_add(cursor_col.min(max_x) as u16),
                body.y.saturating_add((cursor_row - start) as u16),
            ));
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct WrappedInputLine {
    text: String,
    source_row: usize,
    start_col: usize,
    end_col: usize,
    display_width: usize,
}

fn wrap_input_lines(lines: &[String], width: u16) -> Vec<WrappedInputLine> {
    let max_width = usize::from(width.max(1));
    let mut rows = Vec::new();

    for (source_row, line) in lines.iter().enumerate() {
        if line.is_empty() {
            rows.push(WrappedInputLine {
                text: String::new(),
                source_row,
                start_col: 0,
                end_col: 0,
                display_width: 0,
            });
            continue;
        }

        let mut text = String::new();
        let mut display_width: usize = 0;
        let mut start_col = 0;
        let mut col = 0;

        for ch in line.chars() {
            let ch_width = char_width(ch);
            if !text.is_empty() && display_width.saturating_add(ch_width) > max_width {
                rows.push(WrappedInputLine {
                    text,
                    source_row,
                    start_col,
                    end_col: col,
                    display_width,
                });
                text = String::new();
                display_width = 0;
                start_col = col;
            }
            text.push(ch);
            display_width = display_width.saturating_add(ch_width);
            col += 1;
        }

        rows.push(WrappedInputLine {
            text,
            source_row,
            start_col,
            end_col: col,
            display_width,
        });
    }

    if rows.is_empty() {
        rows.push(WrappedInputLine {
            text: String::new(),
            source_row: 0,
            start_col: 0,
            end_col: 0,
            display_width: 0,
        });
    }

    rows
}

fn wrapped_cursor_position(
    rows: &[WrappedInputLine],
    cursor: (usize, usize),
    width: u16,
) -> Option<(usize, usize)> {
    let max_x = usize::from(width.saturating_sub(1));
    let mut fallback = None;
    for (idx, row) in rows.iter().enumerate() {
        if row.source_row != cursor.0 {
            continue;
        }

        let is_last_for_source = rows
            .get(idx + 1)
            .map_or(true, |next| next.source_row != row.source_row);
        if row.start_col == row.end_col && cursor.1 == row.start_col {
            return Some((idx, 0));
        }
        if cursor.1 >= row.start_col
            && (cursor.1 < row.end_col || (is_last_for_source && cursor.1 == row.end_col))
        {
            let rel_col = cursor.1.saturating_sub(row.start_col);
            let cursor_x = row
                .text
                .chars()
                .take(rel_col)
                .map(char_width)
                .sum::<usize>();
            return Some((idx, cursor_x.min(max_x)));
        }
        if cursor.1 >= row.end_col {
            fallback = Some((idx, row.display_width.min(max_x)));
        }
    }
    fallback
}

fn char_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
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
    fn set_text_replaces_content_and_edge_detection_works() {
        let mut ib = InputBox::new();
        // Empty box: cursor is on the one (empty) line — both first and last.
        assert!(ib.cursor_on_first_line());
        assert!(ib.cursor_on_last_line());

        // Recall a single-line prompt: cursor lands at the end, still the only
        // line, so Up/Down history nav stays available.
        ib.set_text("recalled prompt");
        assert_eq!(ib.text(), "recalled prompt");
        assert!(ib.cursor_on_first_line());
        assert!(ib.cursor_on_last_line());

        // Multi-line recall: cursor at the end (last line), not the first.
        ib.set_text("line one\nline two");
        assert_eq!(ib.text(), "line one\nline two");
        assert!(!ib.cursor_on_first_line());
        assert!(ib.cursor_on_last_line());
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
    fn long_prompt_soft_wraps_inside_composer_width() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut ib = InputBox::new();
        ib.insert_str("abcdefghijklmnop");
        let backend = TestBackend::new(12, 5);
        let mut term = Terminal::new(backend).unwrap();

        term.draw(|f| ib.render(f, f.area(), &theme, Mode::Ready, None))
            .unwrap();

        let buf = term.backend().buffer();
        let rows: Vec<String> = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(
            rows.iter().any(|row| row.contains("abcdefghij")),
            "first wrapped segment should remain visible: {rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("klmnop")),
            "overflow should wrap onto the next visual line: {rows:?}"
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
