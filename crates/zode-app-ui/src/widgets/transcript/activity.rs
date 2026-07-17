use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::ActivityEntry;

use crate::{paint_single_line, RectExt, SemanticIcon, ZodeTheme};

use super::draw_text;

const ACTION_ROW_HEIGHT: f32 = 32.0;
const ICON_SIZE: f32 = 15.0;
const TEXT_GAP: f32 = 8.0;

pub(super) fn estimated_height(entries: &[ActivityEntry]) -> f32 {
    entries.len().max(1) as f32 * ACTION_ROW_HEIGHT
}

pub(super) fn paint_group(
    painter: &mut dyn Painter,
    rect: Rect,
    entries: &[ActivityEntry],
    theme: &ZodeTheme,
) {
    if entries.is_empty() {
        paint_action_row(
            painter,
            Rect::xywh(rect.origin.x, rect.origin.y, rect.size.x, ACTION_ROW_HEIGHT),
            "活动",
            None,
            SemanticIcon::Hook,
            theme,
        );
        return;
    }

    for (index, entry) in entries.iter().enumerate() {
        paint_action_row(
            painter,
            Rect::xywh(
                rect.origin.x,
                rect.origin.y + index as f32 * ACTION_ROW_HEIGHT,
                rect.size.x,
                ACTION_ROW_HEIGHT,
            ),
            &entry.title,
            entry.detail.as_deref(),
            activity_icon(&entry.title, entry.completed),
            theme,
        );
    }
}

pub(super) fn paint_thinking(painter: &mut dyn Painter, rect: Rect, text: &str, theme: &ZodeTheme) {
    paint_icon(
        painter,
        rect.origin.x,
        rect.origin.y + (rect.size.y - ICON_SIZE) / 2.0,
        SemanticIcon::Sparkles,
        theme,
    );
    paint_single_line(
        painter,
        if text.trim().is_empty() {
            "正在思考"
        } else {
            text
        },
        Rect::xywh(
            rect.origin.x + ICON_SIZE + TEXT_GAP,
            rect.origin.y,
            (rect.size.x - ICON_SIZE - TEXT_GAP).max(0.0),
            rect.size.y,
        ),
        14.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
}

pub(super) fn paint_status(
    painter: &mut dyn Painter,
    rect: Rect,
    message: &str,
    theme: &ZodeTheme,
) {
    paint_icon(
        painter,
        rect.origin.x,
        rect.origin.y + (rect.size.y - ICON_SIZE) / 2.0,
        SemanticIcon::Check,
        theme,
    );
    paint_single_line(
        painter,
        message,
        Rect::xywh(
            rect.origin.x + ICON_SIZE + TEXT_GAP,
            rect.origin.y,
            (rect.size.x - ICON_SIZE - TEXT_GAP).max(0.0),
            rect.size.y,
        ),
        14.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
}

fn paint_action_row(
    painter: &mut dyn Painter,
    rect: Rect,
    title: &str,
    detail: Option<&str>,
    icon: SemanticIcon,
    theme: &ZodeTheme,
) {
    paint_icon(
        painter,
        rect.origin.x,
        rect.origin.y + (rect.size.y - ICON_SIZE) / 2.0,
        icon,
        theme,
    );
    let text_x = rect.origin.x + ICON_SIZE + TEXT_GAP;
    let title_width = painter
        .measure_text_weighted(title, 14.0, 500)
        .min((rect.size.x * 0.68).max(0.0));
    paint_single_line(
        painter,
        title,
        Rect::xywh(text_x, rect.origin.y, title_width, rect.size.y),
        14.0,
        500,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    if let Some(detail) = detail.filter(|detail| !detail.trim().is_empty()) {
        let detail_x = text_x + title_width + 10.0;
        paint_single_line(
            painter,
            detail,
            Rect::xywh(
                detail_x,
                rect.origin.y,
                (rect.origin.x + rect.size.x - detail_x).max(0.0),
                rect.size.y,
            ),
            13.0,
            400,
            theme.tokens.muted_foreground.with_alpha(0.72),
            HorizontalAlign::Start,
        );
    }
}

fn paint_icon(painter: &mut dyn Painter, x: f32, y: f32, icon: SemanticIcon, theme: &ZodeTheme) {
    painter.stroke_svg_path(
        icon.path(),
        Point2D::new(x, y),
        ICON_SIZE,
        theme.tokens.muted_foreground,
        icon.stroke_width(),
    );
}

fn activity_icon(title: &str, completed: bool) -> SemanticIcon {
    let title = title.to_ascii_lowercase();
    if title.contains("读取") || title.contains("read") {
        SemanticIcon::FileText
    } else if title.contains("命令") || title.contains("运行") || title.contains("command") {
        SemanticIcon::Terminal
    } else if title.contains("编辑") || title.contains("更改") || title.contains("edit") {
        SemanticIcon::Edit
    } else if completed {
        SemanticIcon::Check
    } else {
        SemanticIcon::Sparkles
    }
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
