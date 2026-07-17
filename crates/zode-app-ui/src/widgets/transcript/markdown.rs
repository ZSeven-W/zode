use jian_widgets::{
    components::markdown::{parse_blocks, parse_inline, wrap_runs, MdBlock, MdRun},
    Painter, Point2D, Rect,
};

use crate::{RectExt, ZodeTheme};

use super::draw_text;

const MARKDOWN_LIMIT: usize = 50_000;

pub(super) fn paint_assistant(
    painter: &mut dyn Painter,
    rect: Rect,
    markdown: &str,
    theme: &ZodeTheme,
) {
    paint_markdown_at(
        painter,
        Point2D::new(rect.origin.x, rect.origin.y + 18.0),
        rect.size.x,
        markdown,
        theme,
    );
}

pub(super) fn paint_user(painter: &mut dyn Painter, rect: Rect, markdown: &str, theme: &ZodeTheme) {
    let max_width = (rect.size.x * 0.72).max(56.0);
    let measured = markdown
        .lines()
        .map(|line| painter.measure_text_weighted(line, 13.0, 400))
        .fold(0.0_f32, f32::max);
    let bubble_width = (measured + 28.0).clamp(56.0, max_width);
    let x = rect.max_x() - bubble_width;
    let height = markdown_height(markdown, bubble_width - 28.0).max(54.0);
    let bubble = Rect::xywh(x, rect.origin.y, bubble_width, height.min(rect.size.y));
    painter.fill_round_rect(bubble, 12.0, theme.user_bubble);
    painter.save();
    painter.clip_rect(bubble);
    paint_markdown_at(
        painter,
        Point2D::new(x + 14.0, bubble.origin.y + 16.0),
        (bubble_width - 28.0).max(1.0),
        markdown,
        theme,
    );
    painter.restore();
}

fn paint_markdown_at(
    painter: &mut dyn Painter,
    origin: Point2D,
    width: f32,
    markdown: &str,
    theme: &ZodeTheme,
) {
    let mut y = origin.y;
    let max_chars = ((width / 7.0).floor() as usize).max(8);
    for block in parse_blocks(markdown, MARKDOWN_LIMIT) {
        match block {
            MdBlock::Heading { level, text } => {
                let size = if level == 3 { 16.0 } else { 14.0 };
                draw_text(
                    painter,
                    &text,
                    Point2D::new(origin.x, y),
                    size,
                    600,
                    theme.tokens.foreground,
                );
                y += size + 9.0;
            }
            MdBlock::Bullet(source) => {
                draw_text(
                    painter,
                    "•",
                    Point2D::new(origin.x, y),
                    13.0,
                    600,
                    theme.zode_purple,
                );
                let lines = draw_inline(
                    painter,
                    &source,
                    Point2D::new(origin.x + 16.0, y),
                    max_chars.saturating_sub(2),
                    theme,
                );
                y += lines as f32 * 20.0 + 4.0;
            }
            MdBlock::Paragraph(source) => {
                let lines = draw_inline(
                    painter,
                    &source,
                    Point2D::new(origin.x, y),
                    max_chars,
                    theme,
                );
                y += lines as f32 * 20.0 + 4.0;
            }
        }
    }
}

pub(super) fn markdown_height(markdown: &str, width: f32) -> f32 {
    let max_chars = ((width / 7.0).floor() as usize).max(8);
    let mut height = 18.0;
    for block in parse_blocks(markdown, MARKDOWN_LIMIT) {
        height += match block {
            MdBlock::Heading { level, .. } => {
                let size = if level == 3 { 16.0 } else { 14.0 };
                size + 9.0
            }
            MdBlock::Bullet(source) => {
                wrapped_line_count(&source, max_chars.saturating_sub(2)) as f32 * 20.0 + 4.0
            }
            MdBlock::Paragraph(source) => {
                wrapped_line_count(&source, max_chars) as f32 * 20.0 + 4.0
            }
        };
    }
    (height + 8.0).max(54.0)
}

fn wrapped_line_count(source: &str, max_chars: usize) -> usize {
    wrap_runs(&parse_inline(source), max_chars).len().max(1)
}

fn draw_inline(
    painter: &mut dyn Painter,
    source: &str,
    origin: Point2D,
    max_chars: usize,
    theme: &ZodeTheme,
) -> usize {
    let lines = wrap_runs(&parse_inline(source), max_chars);
    for (line_index, line) in lines.iter().enumerate() {
        let mut x = origin.x;
        let y = origin.y + line_index as f32 * 20.0;
        for run in line {
            let (weight, color) = match run {
                MdRun::Bold(_) => (600, theme.tokens.foreground),
                MdRun::Code(_) | MdRun::Color(_) => (500, theme.zode_purple),
                MdRun::Plain(_) => (400, theme.tokens.foreground),
            };
            let content = run.text();
            let visible = content.trim_start();
            let leading = &content[..content.len() - visible.len()];
            x += painter.measure_text_weighted(leading, 13.0, weight);
            if !visible.is_empty() {
                draw_text(painter, visible, Point2D::new(x, y), 13.0, weight, color);
                x += painter.measure_text_weighted(visible, 13.0, weight);
            }
        }
    }
    lines.len().max(1)
}
