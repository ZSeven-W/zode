use jian_widgets::{
    components::markdown::{parse_blocks, parse_inline, wrap_runs, MdBlock, MdRun},
    Color, Painter, Point2D, Rect, TextLayout,
};
use zode_app_model::{TranscriptItem, TranscriptState, ZodeAppState};

use crate::{visible_range, MeasurementCache, ZodeTheme};

const ESTIMATED_ITEM_HEIGHT: f32 = 72.0;
const ITEM_GAP: f32 = 12.0;
const MARKDOWN_LIMIT: usize = 50_000;

pub struct ThreadTranscript;

impl ThreadTranscript {
    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        painter.save();
        painter.clip_rect(rect);

        let Some(session) = state.current_session.as_ref() else {
            paint_empty(
                painter,
                rect,
                "开始一项任务",
                "描述你想构建、修改或探索的内容。",
                theme,
            );
            painter.restore();
            return;
        };
        let Some(transcript) = state.transcripts.get(session) else {
            paint_empty(
                painter,
                rect,
                "任务已准备好",
                "消息与工具活动会显示在这里。",
                theme,
            );
            painter.restore();
            return;
        };
        if transcript.items.is_empty() {
            paint_empty(
                painter,
                rect,
                "任务已准备好",
                "消息与工具活动会显示在这里。",
                theme,
            );
            painter.restore();
            return;
        }

        paint_items(painter, rect, transcript, theme);
        painter.restore();
    }
}

fn paint_items(
    painter: &mut dyn Painter,
    rect: Rect,
    transcript: &TranscriptState,
    theme: &ZodeTheme,
) {
    let mut cache =
        MeasurementCache::with_estimate(transcript.items.len(), ESTIMATED_ITEM_HEIGHT + ITEM_GAP);
    for (index, height) in transcript.item_heights.iter().copied().enumerate() {
        if height > 0.0 {
            let _ = cache.update(index, height + ITEM_GAP);
        }
    }
    let measurements = cache.items();
    let max_offset = (cache.total_height() - rect.size.y).max(0.0);
    let offset = transcript.scroll_offset.clamp(0.0, max_offset);
    let range = visible_range(&measurements, offset, rect.size.y);

    for index in range {
        let measurement = measurements[index];
        let item_rect = Rect::xywh(
            rect.origin.x,
            rect.origin.y + measurement.top - offset,
            rect.size.x,
            (measurement.bottom - measurement.top - ITEM_GAP).max(1.0),
        );
        paint_item(painter, item_rect, &transcript.items[index], theme);
    }
}

fn paint_item(painter: &mut dyn Painter, rect: Rect, item: &TranscriptItem, theme: &ZodeTheme) {
    match item {
        TranscriptItem::UserText(text) => paint_user(painter, rect, text, theme),
        TranscriptItem::AssistantText(markdown) => paint_markdown(painter, rect, markdown, theme),
        TranscriptItem::Thinking(text) => draw_text(
            painter,
            text,
            Point2D::new(rect.origin.x, rect.origin.y + 20.0),
            12.0,
            400,
            theme.tokens.muted_foreground,
        ),
        TranscriptItem::Tool(tool) => paint_notice(
            painter,
            rect,
            &format!("{} · {}", tool.name, tool.summary),
            theme.tokens.muted,
            theme.tokens.foreground,
        ),
        TranscriptItem::Approval { tool, .. } => paint_notice(
            painter,
            rect,
            &format!("需要批准：{tool}"),
            theme.tokens.muted,
            theme.tokens.foreground,
        ),
        TranscriptItem::Status { message, .. } => draw_text(
            painter,
            message,
            Point2D::new(rect.origin.x, rect.origin.y + 20.0),
            12.0,
            400,
            theme.tokens.muted_foreground,
        ),
        TranscriptItem::Error { message, .. } => paint_notice(
            painter,
            rect,
            message,
            theme.tokens.destructive.with_alpha(0.12),
            theme.tokens.destructive,
        ),
    }
}

fn paint_user(painter: &mut dyn Painter, rect: Rect, text: &str, theme: &ZodeTheme) {
    let max_width = rect.size.x * 0.72;
    let text_width = painter.measure_text_weighted(text, 13.0, 400);
    let bubble_width = (text_width + 28.0).clamp(56.0, max_width.max(56.0));
    let x = rect.origin.x + rect.size.x - bubble_width;
    let bubble = Rect::xywh(x, rect.origin.y + 4.0, bubble_width, 42.0);
    painter.fill_round_rect(bubble, 12.0, theme.user_bubble);
    draw_text(
        painter,
        text,
        Point2D::new(x + 14.0, rect.origin.y + 29.0),
        13.0,
        400,
        theme.tokens.foreground,
    );
}

fn paint_markdown(painter: &mut dyn Painter, rect: Rect, markdown: &str, theme: &ZodeTheme) {
    let mut y = rect.origin.y + 18.0;
    let max_chars = ((rect.size.x / 7.0).floor() as usize).max(8);
    for block in parse_blocks(markdown, MARKDOWN_LIMIT) {
        match block {
            MdBlock::Heading { level, text } => {
                let size = if level == 3 { 16.0 } else { 14.0 };
                draw_text(
                    painter,
                    &text,
                    Point2D::new(rect.origin.x, y),
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
                    Point2D::new(rect.origin.x, y),
                    13.0,
                    600,
                    theme.zode_purple,
                );
                draw_inline(
                    painter,
                    &source,
                    Point2D::new(rect.origin.x + 16.0, y),
                    max_chars.saturating_sub(2),
                    theme,
                );
                y += 22.0;
            }
            MdBlock::Paragraph(source) => {
                let lines = draw_inline(
                    painter,
                    &source,
                    Point2D::new(rect.origin.x, y),
                    max_chars,
                    theme,
                );
                y += lines as f32 * 20.0 + 4.0;
            }
        }
    }
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
                MdRun::Code(_) => (500, theme.zode_purple),
                MdRun::Color(_) => (500, theme.zode_purple),
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
    lines.len()
}

fn paint_notice(
    painter: &mut dyn Painter,
    rect: Rect,
    text: &str,
    background: Color,
    foreground: Color,
) {
    painter.fill_round_rect(
        Rect::xywh(rect.origin.x, rect.origin.y + 4.0, rect.size.x, 42.0),
        8.0,
        background,
    );
    draw_text(
        painter,
        text,
        Point2D::new(rect.origin.x + 12.0, rect.origin.y + 29.0),
        12.0,
        500,
        foreground,
    );
}

fn paint_empty(
    painter: &mut dyn Painter,
    rect: Rect,
    headline: &str,
    detail: &str,
    theme: &ZodeTheme,
) {
    draw_text(
        painter,
        headline,
        Point2D::new(rect.origin.x, rect.origin.y + 30.0),
        17.0,
        600,
        theme.tokens.foreground,
    );
    draw_text(
        painter,
        detail,
        Point2D::new(rect.origin.x, rect.origin.y + 56.0),
        13.0,
        400,
        theme.tokens.muted_foreground,
    );
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    size: f32,
    weight: u16,
    color: Color,
) {
    let layout = TextLayout::single_run(text, "system-ui", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(&layout, origin);
}
