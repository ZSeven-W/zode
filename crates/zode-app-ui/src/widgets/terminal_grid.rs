use std::collections::VecDeque;

use vte::{Params, Parser, Perform};

pub(crate) const CELL_WIDTH: f32 = 8.0;
pub(crate) const LINE_HEIGHT: f32 = 20.0;
pub(crate) const TEXT_INSET: f32 = 8.0;
const MAX_SCROLLBACK_LINES: usize = 10_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TerminalColor {
    #[default]
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCell {
    pub character: char,
    pub foreground: TerminalColor,
    pub background: TerminalColor,
    pub bold: bool,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            character: ' ',
            foreground: TerminalColor::Default,
            background: TerminalColor::Default,
            bold: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalLine {
    pub(crate) cells: Vec<TerminalCell>,
}

impl TerminalLine {
    fn blank(cols: usize) -> Self {
        Self {
            cells: vec![TerminalCell::default(); cols],
        }
    }

    pub fn plain_text(&self) -> String {
        self.cells
            .iter()
            .map(|cell| cell.character)
            .collect::<String>()
            .trim_end_matches(' ')
            .to_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPosition {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSelection {
    pub start: CellPosition,
    pub end: CellPosition,
}

#[derive(Debug, Clone, Copy, Default)]
struct CellStyle {
    foreground: TerminalColor,
    background: TerminalColor,
    bold: bool,
}

pub struct TerminalGrid {
    cols: usize,
    rows: usize,
    screen: Vec<TerminalLine>,
    scrollback: VecDeque<TerminalLine>,
    cursor_row: usize,
    cursor_col: usize,
    scroll_top: usize,
    scroll_bottom: usize,
    wrap_pending: bool,
    style: CellStyle,
    parser: Parser,
}

impl TerminalGrid {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            cols,
            rows,
            screen: (0..rows).map(|_| TerminalLine::blank(cols)).collect(),
            scrollback: VecDeque::new(),
            cursor_row: 0,
            cursor_col: 0,
            scroll_top: 0,
            scroll_bottom: rows - 1,
            wrap_pending: false,
            style: CellStyle::default(),
            parser: Parser::new(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let mut parser = std::mem::take(&mut self.parser);
        for byte in bytes {
            parser.advance(self, *byte);
        }
        self.parser = parser;
    }

    pub fn line_count(&self) -> usize {
        self.scrollback.len() + self.screen.len()
    }

    pub fn size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    pub const fn scrollback_limit() -> usize {
        MAX_SCROLLBACK_LINES
    }

    /// Resizes the visible grid without attempting xterm-style content
    /// reflow. Existing rows are truncated or padded in place.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        for line in self.scrollback.iter_mut().chain(&mut self.screen) {
            line.cells.resize(cols, TerminalCell::default());
        }
        if rows < self.rows {
            let last_content = self
                .screen
                .iter()
                .rposition(|line| {
                    line.cells
                        .iter()
                        .any(|cell| *cell != TerminalCell::default())
                })
                .unwrap_or(self.cursor_row);
            let keep_end = (last_content + 1)
                .max(self.cursor_row + 1)
                .max(rows)
                .min(self.rows);
            let keep_start = keep_end - rows;
            self.scrollback.extend(self.screen.drain(..keep_start));
            self.trim_scrollback();
            self.screen.truncate(rows);
            self.cursor_row = self.cursor_row.saturating_sub(keep_start).min(rows - 1);
        } else {
            self.screen.resize_with(rows, || TerminalLine::blank(cols));
            self.cursor_row = self.cursor_row.min(rows - 1);
        }
        self.cols = cols;
        self.rows = rows;
        self.cursor_col = self.cursor_col.min(cols - 1);
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        self.wrap_pending = false;
    }

    pub fn line(&self, row: usize) -> &TerminalLine {
        self.screen
            .get(row)
            .expect("terminal row must be inside the grid")
    }

    pub fn cell(&self, row: usize, col: usize) -> &TerminalCell {
        self.line(row)
            .cells
            .get(col)
            .expect("terminal column must be inside the grid")
    }

    pub(crate) fn buffer_line(&self, row: usize) -> Option<&TerminalLine> {
        if row < self.scrollback.len() {
            self.scrollback.get(row)
        } else {
            self.screen.get(row - self.scrollback.len())
        }
    }

    fn print_character(&mut self, character: char) {
        if self.wrap_pending {
            self.cursor_col = 0;
            self.line_feed();
            self.wrap_pending = false;
        }
        self.screen[self.cursor_row].cells[self.cursor_col] = TerminalCell {
            character,
            foreground: self.style.foreground,
            background: self.style.background,
            bold: self.style.bold,
        };
        if self.cursor_col + 1 == self.cols {
            self.wrap_pending = true;
        } else {
            self.cursor_col += 1;
        }
    }

    fn line_feed(&mut self) {
        if self.cursor_row == self.scroll_bottom {
            self.scroll_up();
        } else {
            self.cursor_row = (self.cursor_row + 1).min(self.rows - 1);
        }
    }

    fn scroll_up(&mut self) {
        let removed = self.screen.remove(self.scroll_top);
        self.screen
            .insert(self.scroll_bottom, TerminalLine::blank(self.cols));
        if self.scroll_top == 0 && self.scroll_bottom == self.rows - 1 {
            self.scrollback.push_back(removed);
            self.trim_scrollback();
        }
    }

    fn trim_scrollback(&mut self) {
        while self.scrollback.len() > MAX_SCROLLBACK_LINES {
            self.scrollback.pop_front();
        }
    }

    fn scroll_down(&mut self) {
        self.screen.remove(self.scroll_bottom);
        self.screen
            .insert(self.scroll_top, TerminalLine::blank(self.cols));
    }

    fn clear_screen(&mut self, mode: u16) {
        match mode {
            0 => {
                for col in self.cursor_col.min(self.cols)..self.cols {
                    self.screen[self.cursor_row].cells[col] = TerminalCell::default();
                }
                for row in self.cursor_row.saturating_add(1)..self.rows {
                    self.screen[row] = TerminalLine::blank(self.cols);
                }
            }
            1 => {
                for row in 0..self.cursor_row {
                    self.screen[row] = TerminalLine::blank(self.cols);
                }
                let end = self.cursor_col.min(self.cols - 1);
                for col in 0..=end {
                    self.screen[self.cursor_row].cells[col] = TerminalCell::default();
                }
            }
            2 => {
                self.screen = (0..self.rows)
                    .map(|_| TerminalLine::blank(self.cols))
                    .collect();
                self.cursor_row = 0;
                self.cursor_col = 0;
                self.wrap_pending = false;
            }
            3 => self.scrollback.clear(),
            _ => {}
        }
    }

    fn set_scroll_region(&mut self, params: &Params) {
        let top = csi_param(params, 0, 1).saturating_sub(1) as usize;
        let bottom = csi_param(params, 1, self.rows as u16).saturating_sub(1) as usize;
        if top < bottom && bottom < self.rows {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
            self.cursor_row = 0;
            self.cursor_col = 0;
            self.wrap_pending = false;
        }
    }

    fn set_graphics(&mut self, params: &Params) {
        let values = params
            .iter()
            .map(|param| param.first().copied().unwrap_or(0))
            .collect::<Vec<_>>();
        let values = if values.is_empty() { vec![0] } else { values };
        let mut index = 0;
        while index < values.len() {
            match values[index] {
                0 => self.style = CellStyle::default(),
                1 => self.style.bold = true,
                22 => self.style.bold = false,
                30..=37 => self.style.foreground = basic_color(values[index] - 30, false),
                39 => self.style.foreground = TerminalColor::Default,
                40..=47 => self.style.background = basic_color(values[index] - 40, false),
                49 => self.style.background = TerminalColor::Default,
                90..=97 => self.style.foreground = basic_color(values[index] - 90, true),
                100..=107 => self.style.background = basic_color(values[index] - 100, true),
                38 | 48 => {
                    let foreground = values[index] == 38;
                    if values.get(index + 1) == Some(&5) {
                        if let Some(value) = values
                            .get(index + 2)
                            .and_then(|value| u8::try_from(*value).ok())
                        {
                            set_color(&mut self.style, foreground, TerminalColor::Indexed(value));
                            index += 2;
                        }
                    } else if values.get(index + 1) == Some(&2) && index + 4 < values.len() {
                        let rgb = (
                            u8::try_from(values[index + 2]),
                            u8::try_from(values[index + 3]),
                            u8::try_from(values[index + 4]),
                        );
                        if let (Ok(red), Ok(green), Ok(blue)) = rgb {
                            set_color(
                                &mut self.style,
                                foreground,
                                TerminalColor::Rgb(red, green, blue),
                            );
                            index += 4;
                        }
                    }
                }
                _ => {}
            }
            index += 1;
        }
    }

    fn position_cursor(&mut self, row: u16, col: u16) {
        self.cursor_row = usize::from(row.saturating_sub(1)).min(self.rows - 1);
        self.cursor_col = usize::from(col.saturating_sub(1)).min(self.cols - 1);
        self.wrap_pending = false;
    }
}

impl Perform for TerminalGrid {
    fn print(&mut self, character: char) {
        self.print_character(character);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\x08' => {
                self.wrap_pending = false;
                self.cursor_col = self.cursor_col.saturating_sub(1);
            }
            b'\t' => {
                self.wrap_pending = false;
                self.cursor_col = ((self.cursor_col / 8 + 1) * 8).min(self.cols - 1);
            }
            b'\n' | b'\x0b' | b'\x0c' => {
                self.wrap_pending = false;
                self.line_feed();
            }
            b'\r' => {
                self.wrap_pending = false;
                self.cursor_col = 0;
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore || !intermediates.is_empty() {
            return;
        }
        match action {
            'A' => {
                self.wrap_pending = false;
                let distance = usize::from(csi_param(params, 0, 1));
                self.cursor_row = self.cursor_row.saturating_sub(distance);
            }
            'B' => {
                self.wrap_pending = false;
                let distance = usize::from(csi_param(params, 0, 1));
                self.cursor_row = (self.cursor_row + distance).min(self.rows - 1);
            }
            'C' => {
                self.wrap_pending = false;
                let distance = usize::from(csi_param(params, 0, 1));
                self.cursor_col = (self.cursor_col + distance).min(self.cols - 1);
            }
            'D' => {
                self.wrap_pending = false;
                let distance = usize::from(csi_param(params, 0, 1));
                self.cursor_col = self.cursor_col.saturating_sub(distance);
            }
            'G' => self.position_cursor((self.cursor_row + 1) as u16, csi_param(params, 0, 1)),
            'H' | 'f' => self.position_cursor(csi_param(params, 0, 1), csi_param(params, 1, 1)),
            'J' => self.clear_screen(csi_param(params, 0, 0)),
            'S' => {
                for _ in 0..csi_param(params, 0, 1) {
                    self.scroll_up();
                }
            }
            'T' => {
                for _ in 0..csi_param(params, 0, 1) {
                    self.scroll_down();
                }
            }
            'd' => self.position_cursor(csi_param(params, 0, 1), (self.cursor_col + 1) as u16),
            'm' => self.set_graphics(params),
            'r' => self.set_scroll_region(params),
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if ignore || !intermediates.is_empty() {
            return;
        }
        match byte {
            b'D' => {
                self.wrap_pending = false;
                self.line_feed();
            }
            b'E' => {
                self.wrap_pending = false;
                self.cursor_col = 0;
                self.line_feed();
            }
            b'M' if self.cursor_row == self.scroll_top => {
                self.wrap_pending = false;
                self.scroll_down();
            }
            b'M' => {
                self.wrap_pending = false;
                self.cursor_row = self.cursor_row.saturating_sub(1);
            }
            _ => {}
        }
    }
}

fn csi_param(params: &Params, index: usize, default: u16) -> u16 {
    match params
        .iter()
        .nth(index)
        .and_then(|param| param.first())
        .copied()
    {
        Some(0) | None => default,
        Some(value) => value,
    }
}

fn basic_color(index: u16, bright: bool) -> TerminalColor {
    match (index, bright) {
        (0, false) => TerminalColor::Black,
        (1, false) => TerminalColor::Red,
        (2, false) => TerminalColor::Green,
        (3, false) => TerminalColor::Yellow,
        (4, false) => TerminalColor::Blue,
        (5, false) => TerminalColor::Magenta,
        (6, false) => TerminalColor::Cyan,
        (7, false) => TerminalColor::White,
        (0, true) => TerminalColor::BrightBlack,
        (1, true) => TerminalColor::BrightRed,
        (2, true) => TerminalColor::BrightGreen,
        (3, true) => TerminalColor::BrightYellow,
        (4, true) => TerminalColor::BrightBlue,
        (5, true) => TerminalColor::BrightMagenta,
        (6, true) => TerminalColor::BrightCyan,
        (7, true) => TerminalColor::BrightWhite,
        _ => TerminalColor::Default,
    }
}

fn set_color(style: &mut CellStyle, foreground: bool, color: TerminalColor) {
    if foreground {
        style.foreground = color;
    } else {
        style.background = color;
    }
}
