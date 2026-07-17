use std::ops::Range;

use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::TerminalState;

use crate::ZodeTheme;

use super::terminal_grid::{
    CellPosition, TerminalColor, TerminalGrid, TerminalLine, TerminalSelection, CELL_WIDTH,
    LINE_HEIGHT, TEXT_INSET,
};

pub struct TerminalPanel;

impl TerminalPanel {
    pub const MOBILE_UNAVAILABLE_MESSAGE: &'static str = "Terminal is unavailable on this device.";

    pub fn visible_line_range(
        line_count: usize,
        offset: f32,
        viewport_height: f32,
        line_height: f32,
    ) -> Range<usize> {
        let line_height = line_height.max(1.0);
        let offset = offset.max(0.0);
        let viewport_height = viewport_height.max(0.0);
        let lines_above = ((offset / line_height).floor() as usize).min(line_count);
        let start = lines_above.saturating_sub(1);
        let end = ((offset + viewport_height) / line_height).ceil() as usize;
        start..end.min(line_count).max(start)
    }

    pub fn tail_offset(line_count: usize, viewport_height: f32) -> f32 {
        (line_count as f32 * LINE_HEIGHT - viewport_height.max(0.0)).max(0.0)
    }

    pub fn copy_selection(grid: &TerminalGrid, selection: TerminalSelection) -> String {
        let (start, end) = ordered_selection(selection);
        if start.row >= grid.line_count() || start == end {
            return String::new();
        }

        let last_row = end.row.min(grid.line_count().saturating_sub(1));
        let mut selected = Vec::new();
        for row in start.row..=last_row {
            let line = grid
                .buffer_line(row)
                .expect("selected terminal row was validated");
            let from = if row == start.row { start.col } else { 0 }.min(line.cells.len());
            let to = if row == end.row {
                end.col
            } else {
                line.cells.len()
            }
            .min(line.cells.len());
            let text = line.cells[from..to]
                .iter()
                .map(|cell| cell.character)
                .collect::<String>()
                .trim_end_matches(' ')
                .to_owned();
            selected.push(text);
        }
        selected.join("\n")
    }

    pub fn unavailable_message(reason: Option<&str>) -> String {
        reason
            .filter(|reason| !reason.trim().is_empty())
            .unwrap_or(Self::MOBILE_UNAVAILABLE_MESSAGE)
            .to_owned()
    }

    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        grid: &TerminalGrid,
        state: &TerminalState,
        selection: Option<TerminalSelection>,
        theme: &ZodeTheme,
    ) {
        painter.fill_rect(rect, theme.tokens.background);
        if let Some(reason) = state.unavailable_reason.as_deref() {
            draw_text(
                painter,
                &Self::unavailable_message(Some(reason)),
                Point2D::new(rect.origin.x + TEXT_INSET, rect.origin.y + LINE_HEIGHT),
                theme.tokens.muted_foreground,
                false,
            );
            return;
        }

        let offset = state.scroll_offset.max(0.0);
        painter.save();
        painter.clip_rect(rect);
        for row in Self::visible_line_range(grid.line_count(), offset, rect.size.y, LINE_HEIGHT) {
            let Some(line) = grid.buffer_line(row) else {
                continue;
            };
            let y = rect.origin.y + row as f32 * LINE_HEIGHT - offset;
            paint_line_backgrounds(painter, rect.origin.x + TEXT_INSET, y, line, theme);
            paint_selection(painter, rect, row, y, line.cells.len(), selection, theme);
            paint_line(painter, rect.origin.x + TEXT_INSET, y, line, theme);
        }
        painter.restore();
        if state.focused {
            painter.stroke_rect(rect, theme.zode_purple, 1.0);
        }
    }
}

fn paint_selection(
    painter: &mut dyn Painter,
    rect: Rect,
    row: usize,
    y: f32,
    cols: usize,
    selection: Option<TerminalSelection>,
    theme: &ZodeTheme,
) {
    let Some(selection) = selection else {
        return;
    };
    let (start, end) = ordered_selection(selection);
    if row < start.row || row > end.row {
        return;
    }
    let from = if row == start.row { start.col } else { 0 }.min(cols);
    let to = if row == end.row { end.col } else { cols }.min(cols);
    if from >= to {
        return;
    }
    painter.fill_rect(
        Rect::xywh(
            rect.origin.x + TEXT_INSET + from as f32 * CELL_WIDTH,
            y,
            (to - from) as f32 * CELL_WIDTH,
            LINE_HEIGHT,
        ),
        theme.zode_purple.with_alpha(0.22),
    );
}

fn paint_line(painter: &mut dyn Painter, x: f32, y: f32, line: &TerminalLine, theme: &ZodeTheme) {
    let used = line
        .cells
        .iter()
        .rposition(|cell| {
            cell.character != ' '
                || cell.foreground != TerminalColor::Default
                || cell.background != TerminalColor::Default
                || cell.bold
        })
        .map_or(0, |index| index + 1);
    let mut start = 0;
    while start < used {
        let style = line.cells[start];
        let mut end = start + 1;
        while end < used
            && line.cells[end].foreground == style.foreground
            && line.cells[end].background == style.background
            && line.cells[end].bold == style.bold
        {
            end += 1;
        }
        let text = line.cells[start..end]
            .iter()
            .map(|cell| cell.character)
            .collect::<String>();
        draw_text(
            painter,
            &text,
            Point2D::new(x + start as f32 * CELL_WIDTH, y + 15.0),
            terminal_color(style.foreground, theme),
            style.bold,
        );
        start = end;
    }
}

fn paint_line_backgrounds(
    painter: &mut dyn Painter,
    x: f32,
    y: f32,
    line: &TerminalLine,
    theme: &ZodeTheme,
) {
    let mut start = 0;
    while start < line.cells.len() {
        let background = line.cells[start].background;
        let mut end = start + 1;
        while end < line.cells.len() && line.cells[end].background == background {
            end += 1;
        }
        if background != TerminalColor::Default {
            painter.fill_rect(
                Rect::xywh(
                    x + start as f32 * CELL_WIDTH,
                    y,
                    (end - start) as f32 * CELL_WIDTH,
                    LINE_HEIGHT,
                ),
                terminal_color(background, theme),
            );
        }
        start = end;
    }
}

fn draw_text(painter: &mut dyn Painter, text: &str, origin: Point2D, color: Color, bold: bool) {
    let weight = if bold { 700 } else { 400 };
    let layout = TextLayout::single_run(text, "ui-monospace", 12.0, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(&layout, origin);
}

fn terminal_color(color: TerminalColor, theme: &ZodeTheme) -> Color {
    match color {
        TerminalColor::Default => theme.tokens.foreground,
        TerminalColor::Black => Color::rgb_u8(0, 0, 0),
        TerminalColor::Red => Color::rgb_u8(205, 49, 49),
        TerminalColor::Green => Color::rgb_u8(13, 188, 121),
        TerminalColor::Yellow => Color::rgb_u8(229, 229, 16),
        TerminalColor::Blue => Color::rgb_u8(36, 114, 200),
        TerminalColor::Magenta => Color::rgb_u8(188, 63, 188),
        TerminalColor::Cyan => Color::rgb_u8(17, 168, 205),
        TerminalColor::White => Color::rgb_u8(229, 229, 229),
        TerminalColor::BrightBlack => Color::rgb_u8(102, 102, 102),
        TerminalColor::BrightRed => Color::rgb_u8(241, 76, 76),
        TerminalColor::BrightGreen => Color::rgb_u8(35, 209, 139),
        TerminalColor::BrightYellow => Color::rgb_u8(245, 245, 67),
        TerminalColor::BrightBlue => Color::rgb_u8(59, 142, 234),
        TerminalColor::BrightMagenta => Color::rgb_u8(214, 112, 214),
        TerminalColor::BrightCyan => Color::rgb_u8(41, 184, 219),
        TerminalColor::BrightWhite => Color::rgb_u8(255, 255, 255),
        TerminalColor::Indexed(_) | TerminalColor::Rgb(_, _, _) => indexed_or_rgb(color),
    }
}

fn indexed_or_rgb(color: TerminalColor) -> Color {
    match color {
        TerminalColor::Rgb(red, green, blue) => Color::rgb_u8(red, green, blue),
        TerminalColor::Indexed(index) => indexed_color(index),
        _ => Color::WHITE,
    }
}

fn indexed_color(index: u8) -> Color {
    const ANSI: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    let (red, green, blue) = match index {
        0..=15 => ANSI[usize::from(index)],
        16..=231 => {
            let value = index - 16;
            let component = |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
            (
                component(value / 36),
                component((value % 36) / 6),
                component(value % 6),
            )
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            (value, value, value)
        }
    };
    Color::rgb_u8(red, green, blue)
}

fn ordered_selection(selection: TerminalSelection) -> (CellPosition, CellPosition) {
    if (selection.start.row, selection.start.col) <= (selection.end.row, selection.end.col) {
        (selection.start, selection.end)
    } else {
        (selection.end, selection.start)
    }
}
