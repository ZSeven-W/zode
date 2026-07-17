use jian_widgets::{Painter, Point2D, Rect};
use zode_app_model::ActivityEntry;

use crate::{RectExt, ZodeTheme};

use super::draw_text;

pub(super) fn estimated_height(entries: &[ActivityEntry]) -> f32 {
    34.0 + entries.len().max(1) as f32 * 34.0
}

pub(super) fn paint_group(
    painter: &mut dyn Painter,
    rect: Rect,
    entries: &[ActivityEntry],
    theme: &ZodeTheme,
) {
    painter.fill_round_rect(rect, 10.0, theme.tokens.card);
    painter.stroke_round_rect(rect, 10.0, theme.tokens.border, 1.0);
    draw_text(
        painter,
        "活动",
        Point2D::new(rect.origin.x + 12.0, rect.origin.y + 21.0),
        11.0,
        600,
        theme.tokens.muted_foreground,
    );
    let mut y = rect.origin.y + 46.0;
    for entry in entries {
        let marker = if entry.completed { "✓" } else { "•" };
        draw_text(
            painter,
            marker,
            Point2D::new(rect.origin.x + 12.0, y),
            12.0,
            600,
            if entry.completed {
                theme.success
            } else {
                theme.zode_purple
            },
        );
        draw_text(
            painter,
            &entry.title,
            Point2D::new(rect.origin.x + 32.0, y),
            12.0,
            500,
            theme.tokens.foreground,
        );
        if let Some(detail) = entry.detail.as_deref() {
            let x = (rect.max_x() - painter.measure_text_weighted(detail, 11.0, 400) - 12.0)
                .max(rect.origin.x + 180.0);
            draw_text(
                painter,
                detail,
                Point2D::new(x, y),
                11.0,
                400,
                theme.tokens.muted_foreground,
            );
        }
        y += 34.0;
    }
}

pub(super) fn paint_thinking(painter: &mut dyn Painter, rect: Rect, text: &str, theme: &ZodeTheme) {
    draw_text(
        painter,
        "思考",
        Point2D::new(rect.origin.x, rect.origin.y + 20.0),
        11.0,
        600,
        theme.zode_purple,
    );
    draw_text(
        painter,
        text,
        Point2D::new(rect.origin.x + 38.0, rect.origin.y + 20.0),
        12.0,
        400,
        theme.tokens.muted_foreground,
    );
}

pub(super) fn paint_status(
    painter: &mut dyn Painter,
    rect: Rect,
    message: &str,
    theme: &ZodeTheme,
) {
    draw_text(
        painter,
        message,
        Point2D::new(rect.origin.x, rect.origin.y + 20.0),
        12.0,
        400,
        theme.tokens.muted_foreground,
    );
}

pub(super) fn paint_error(
    painter: &mut dyn Painter,
    rect: Rect,
    message: &str,
    retryable: bool,
    theme: &ZodeTheme,
) {
    painter.fill_round_rect(
        Rect::xywh(rect.origin.x, rect.origin.y + 4.0, rect.size.x, 42.0),
        8.0,
        theme.tokens.destructive.with_alpha(0.12),
    );
    draw_text(
        painter,
        message,
        Point2D::new(rect.origin.x + 12.0, rect.origin.y + 29.0),
        12.0,
        500,
        theme.tokens.destructive,
    );
    if retryable {
        draw_text(
            painter,
            "可重试",
            Point2D::new(rect.max_x() - 58.0, rect.origin.y + 29.0),
            11.0,
            600,
            theme.tokens.destructive,
        );
    }
}
