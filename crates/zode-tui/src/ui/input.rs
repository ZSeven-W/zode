//! Multi-line input box (tui-textarea wrapper). The front-end intercepts
//! Enter (submit) and Shift/Alt+Enter (newline) before forwarding other
//! keys here.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputSelectionPoint {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputSelection {
    pub anchor: InputSelectionPoint,
    pub focus: InputSelectionPoint,
}

impl InputSelection {
    pub fn new(anchor: InputSelectionPoint, focus: InputSelectionPoint) -> Self {
        Self { anchor, focus }
    }

    fn normalized(self) -> (InputSelectionPoint, InputSelectionPoint) {
        if (self.focus.row, self.focus.column) < (self.anchor.row, self.anchor.column) {
            (self.focus, self.anchor)
        } else {
            (self.anchor, self.focus)
        }
    }
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

    /// Enter fallback for terminals that cannot report Shift+Enter (only the
    /// kitty keyboard protocol distinguishes it from plain Enter — legacy
    /// terminals send the identical byte): a single trailing backslash under
    /// the cursor continues the line instead of submitting. Returns `true`
    /// when handled. A doubled `\\` stays literal, and a backslash anywhere
    /// but the very end of the text (Windows paths, regexes) never triggers.
    pub fn continue_backslash_line(&mut self) -> bool {
        let (row, col) = self.area.cursor();
        let lines = self.area.lines();
        let Some(last) = lines.last() else {
            return false;
        };
        let at_text_end = row + 1 == lines.len() && col == last.chars().count();
        if !at_text_end || !last.ends_with('\\') || last.ends_with("\\\\") {
            return false;
        }
        self.area.delete_char(); // drop the continuation backslash
        self.area.insert_newline();
        true
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
        self.render_with_selection(f, area, theme, mode, completion_placeholder, None);
    }

    pub fn render_with_selection(
        &self,
        f: &mut Frame,
        area: Rect,
        theme: &Theme,
        mode: Mode,
        completion_placeholder: Option<&str>,
        selection: Option<InputSelection>,
    ) {
        let status_color = match mode {
            Mode::Ready => theme.accent_secondary,
            Mode::Thinking => theme.system,
            Mode::Streaming => theme.accent,
            Mode::Compacting => theme.system,
            Mode::Switching => theme.system,
            Mode::Error => Color::Red,
        };
        // `!cmd` shell-escape mode: a distinct (amber) rail + hint so it's clear
        // the line runs locally and is not sent to the model.
        let shell = self.is_shell_command();
        let rail_color = if shell { Color::Yellow } else { status_color };
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_input)),
            area,
        );
        if area.width > 0 && area.height > 0 {
            let rail_lines: Vec<Line<'static>> = (0..area.height)
                .map(|_| {
                    Line::from(Span::styled(
                        "▌",
                        Style::default().bg(theme.bg_input).fg(rail_color),
                    ))
                })
                .collect();
            f.render_widget(
                Paragraph::new(rail_lines).style(Style::default().bg(theme.bg_input)),
                Rect::new(area.x, area.y, 1, area.height),
            );
        }

        let body = input_body_area(area);
        self.render_wrapped_text(f, body, theme, selection);
        self.render_completion_placeholder(f, area, theme, completion_placeholder);
        if shell {
            self.render_shell_hint(f, area, theme);
        }
    }

    /// True when the input is a `!cmd` shell escape (first line begins with `!`).
    pub fn is_shell_command(&self) -> bool {
        self.area
            .lines()
            .first()
            .map(|l| l.trim_start().starts_with('!'))
            .unwrap_or(false)
    }

    /// Right-aligned, subtle reminder shown in shell-escape mode (when there's
    /// room on the first row that doesn't collide with the typed command).
    fn render_shell_hint(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let body = input_body_area(area);
        if body.width == 0 || body.height == 0 {
            return;
        }
        let hint = "shell · ↵ runs locally";
        // Width math in usize: the command line is unbounded, so a u16 add/cast
        // could overflow or truncate and defeat the collision guard.
        let hint_w = UnicodeWidthStr::width(hint);
        let used = self
            .area
            .lines()
            .first()
            .map(|l| UnicodeWidthStr::width(l.as_str()))
            .unwrap_or(0);
        let avail = body.width as usize;
        if hint_w.saturating_add(used).saturating_add(2) > avail {
            return; // not enough room beside the command
        }
        let x = body.x.saturating_add(avail.saturating_sub(hint_w) as u16);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                hint,
                Style::default()
                    .bg(theme.bg_input)
                    .fg(theme.fg_subtle)
                    .add_modifier(Modifier::ITALIC),
            ))),
            Rect::new(x, body.y, hint_w as u16, 1),
        );
    }

    pub fn selection_point_at(
        &self,
        area: Rect,
        column: u16,
        row: u16,
    ) -> Option<InputSelectionPoint> {
        let body = input_body_area(area);
        if !rect_contains(body, column, row) || body.width == 0 || body.height == 0 {
            return None;
        }
        let rows = wrap_input_lines(self.area.lines(), body.width);
        let start = visible_wrapped_start(&rows, self.area.cursor(), body.width, body.height);
        let row_idx = start.saturating_add(usize::from(row.saturating_sub(body.y)));
        let wrapped = rows.get(row_idx).or_else(|| rows.last())?;
        let x = usize::from(
            column
                .saturating_sub(body.x)
                .min(body.width.saturating_sub(1)),
        );
        let rel_col = char_col_for_display_x(&wrapped.text, x);
        Some(InputSelectionPoint {
            row: wrapped.source_row,
            column: wrapped
                .start_col
                .saturating_add(rel_col)
                .min(wrapped.end_col),
        })
    }

    pub fn selected_text(&self, selection: InputSelection) -> String {
        let lines = self.area.lines();
        if lines.is_empty() {
            return String::new();
        }
        let (start, end) = selection.normalized();
        if start == end {
            return String::new();
        }
        let last_row = lines.len().saturating_sub(1);
        let start_row = start.row.min(last_row);
        let end_row = end.row.min(last_row);
        let mut out = Vec::new();
        for (row_idx, line) in lines.iter().enumerate().take(end_row + 1).skip(start_row) {
            let line_len = line.chars().count();
            let text = if start_row == end_row {
                slice_char_cols(line, start.column.min(line_len), end.column.min(line_len))
            } else if row_idx == start_row {
                slice_char_cols(line, start.column.min(line_len), line_len)
            } else if row_idx == end_row {
                slice_char_cols(line, 0, end.column.min(line_len))
            } else {
                line.clone()
            };
            out.push(text);
        }
        out.join("\n")
    }

    fn render_wrapped_text(
        &self,
        f: &mut Frame,
        body: Rect,
        theme: &Theme,
        selection: Option<InputSelection>,
    ) {
        if body.width == 0 || body.height == 0 {
            return;
        }
        let style = Style::default().bg(theme.bg_input).fg(theme.fg_text);
        let rows = wrap_input_lines(self.area.lines(), body.width);
        let (cursor_row, cursor_col) =
            wrapped_cursor_position(&rows, self.area.cursor(), body.width).unwrap_or((0, 0));
        let visible_height = body.height as usize;
        let start = visible_wrapped_start(&rows, self.area.cursor(), body.width, body.height);
        let selected_style = Style::default()
            .bg(theme.accent)
            .fg(theme.bg_primary)
            .add_modifier(Modifier::BOLD);
        let visible_rows: Vec<Line<'static>> = rows
            .iter()
            .skip(start)
            .take(visible_height)
            .map(|row| render_input_row(row, style, selected_style, selection))
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

fn visible_wrapped_start(
    rows: &[WrappedInputLine],
    cursor: (usize, usize),
    width: u16,
    height: u16,
) -> usize {
    let (cursor_row, _) = wrapped_cursor_position(rows, cursor, width).unwrap_or((0, 0));
    cursor_row
        .saturating_add(1)
        .saturating_sub(usize::from(height))
}

fn render_input_row(
    row: &WrappedInputLine,
    style: Style,
    selected_style: Style,
    selection: Option<InputSelection>,
) -> Line<'static> {
    let Some((start_col, end_col)) =
        selection.and_then(|selection| selection_cols_for_wrapped_row(row, selection))
    else {
        return Line::from(Span::styled(row.text.clone(), style));
    };
    let mut spans = Vec::new();
    for (idx, ch) in row.text.chars().enumerate() {
        spans.push(Span::styled(
            ch.to_string(),
            if idx >= start_col && idx < end_col {
                selected_style
            } else {
                style
            },
        ));
    }
    Line::from(spans)
}

fn selection_cols_for_wrapped_row(
    row: &WrappedInputLine,
    selection: InputSelection,
) -> Option<(usize, usize)> {
    let (start, end) = selection.normalized();
    if start == end || row.source_row < start.row || row.source_row > end.row {
        return None;
    }
    let start_col = if row.source_row == start.row {
        start.column
    } else {
        0
    };
    let end_col = if row.source_row == end.row {
        end.column
    } else {
        usize::MAX
    };
    let start = start_col.max(row.start_col);
    let end = end_col.min(row.end_col);
    (start < end).then_some((start - row.start_col, end - row.start_col))
}

fn char_col_for_display_x(text: &str, x: usize) -> usize {
    let mut display_col = 0usize;
    for (idx, ch) in text.chars().enumerate() {
        let next_col = display_col.saturating_add(char_width(ch).max(1));
        if x < next_col {
            return idx;
        }
        display_col = next_col;
    }
    text.chars().count()
}

fn slice_char_cols(text: &str, start_col: usize, end_col: usize) -> String {
    text.chars()
        .skip(start_col)
        .take(end_col.saturating_sub(start_col))
        .collect()
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
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
            .is_none_or(|next| next.source_row != row.source_row);
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
    fn detects_shell_command_prefix() {
        let mut ib = InputBox::new();
        assert!(!ib.is_shell_command());
        ib.insert_str("!ls -la");
        assert!(ib.is_shell_command());
        let mut chat = InputBox::new();
        chat.insert_str("hello");
        assert!(!chat.is_shell_command());
        let mut spaced = InputBox::new();
        spaced.insert_str("  !git status");
        assert!(spaced.is_shell_command());
    }

    #[test]
    fn shell_mode_colors_rail_amber_and_shows_hint() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut ib = InputBox::new();
        ib.insert_str("!ls");
        let backend = TestBackend::new(80, 3);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| ib.render(f, f.area(), &theme, Mode::Ready, None))
            .unwrap();
        let buf = term.backend().buffer();
        // The left rail is amber in shell mode (vs the mode color otherwise).
        assert_eq!(buf.cell((0, 0)).unwrap().fg, Color::Yellow);
        let content: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("shell"), "shell hint visible: {content}");
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
    fn backslash_continuation_breaks_the_line_only_at_a_trailing_backslash() {
        let mut ib = InputBox::new();
        ib.insert_str("first line\\");
        assert!(ib.continue_backslash_line());
        ib.insert_str("second");
        assert_eq!(ib.text(), "first line\nsecond");

        // Mid-text backslash (cursor not at end) never triggers.
        let mut ib = InputBox::new();
        ib.insert_str("C:\\path\\to");
        ib.area.move_cursor(tui_textarea::CursorMove::Head);
        assert!(!ib.continue_backslash_line());

        // Doubled backslash stays literal.
        let mut ib = InputBox::new();
        ib.insert_str("literal\\\\");
        assert!(!ib.continue_backslash_line());

        // No backslash → plain submit path.
        let mut ib = InputBox::new();
        ib.insert_str("plain");
        assert!(!ib.continue_backslash_line());
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
    fn selected_text_extracts_input_range() {
        let mut ib = InputBox::new();
        ib.set_text("queued follow-up");

        let selection = InputSelection::new(
            InputSelectionPoint { row: 0, column: 7 },
            InputSelectionPoint { row: 0, column: 13 },
        );

        assert_eq!(ib.selected_text(selection), "follow");
    }

    #[test]
    fn selection_point_maps_composer_coordinates_to_input_text() {
        let mut ib = InputBox::new();
        ib.set_text("queued follow-up");
        let area = Rect::new(0, 0, 32, 4);

        assert_eq!(
            ib.selection_point_at(area, 2, 1),
            Some(InputSelectionPoint { row: 0, column: 0 })
        );
        assert_eq!(
            ib.selection_point_at(area, 8, 1),
            Some(InputSelectionPoint { row: 0, column: 6 })
        );
    }

    #[test]
    fn input_selection_renders_with_theme_highlight() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut ib = InputBox::new();
        ib.set_text("queued follow-up");
        let selection = InputSelection::new(
            InputSelectionPoint { row: 0, column: 0 },
            InputSelectionPoint { row: 0, column: 6 },
        );
        let backend = TestBackend::new(32, 4);
        let mut term = Terminal::new(backend).unwrap();

        term.draw(|f| {
            ib.render_with_selection(f, f.area(), &theme, Mode::Ready, None, Some(selection))
        })
        .unwrap();

        let buf = term.backend().buffer();
        assert_eq!(buf[(2, 1)].symbol(), "q");
        assert_eq!(buf[(2, 1)].bg, theme.accent);
        assert_eq!(buf[(8, 1)].symbol(), " ");
        assert_ne!(buf[(8, 1)].bg, theme.accent);
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
