use jian_widgets::{Painter, Point2D, Rect};
use zode_app_model::FileArtifact;

use crate::{RectExt, SemanticIcon, ZodeTheme};

use super::draw_text;

pub(super) const HEIGHT: f32 = 64.0;

pub(super) fn paint(painter: &mut dyn Painter, rect: Rect, file: &FileArtifact, theme: &ZodeTheme) {
    painter.fill_round_rect(rect, 12.0, theme.tokens.card);
    painter.stroke_round_rect(rect, 12.0, theme.tokens.border, 1.0);
    paint_icon_tile(painter, rect, SemanticIcon::FileText, theme);
    draw_text(
        painter,
        &file.summary,
        Point2D::new(rect.origin.x + 64.0, rect.origin.y + 14.0),
        13.0,
        600,
        theme.tokens.foreground,
    );
    draw_text(
        painter,
        &file.path,
        Point2D::new(rect.origin.x + 64.0, rect.origin.y + 36.0),
        11.0,
        400,
        theme.tokens.muted_foreground,
    );
    if let Some(change) = file.change_summary.as_deref() {
        paint_change_summary(painter, rect, change, theme);
    }
}

pub(super) fn paint_icon_tile(
    painter: &mut dyn Painter,
    rect: Rect,
    icon: SemanticIcon,
    theme: &ZodeTheme,
) {
    let tile = Rect::xywh(rect.origin.x + 12.0, rect.origin.y + 12.0, 40.0, 40.0);
    painter.fill_round_rect(tile, 10.0, theme.tokens.muted.with_alpha(0.72));
    painter.stroke_svg_path(
        icon.path(),
        Point2D::new(tile.origin.x + 11.0, tile.origin.y + 11.0),
        18.0,
        theme.tokens.muted_foreground,
        icon.stroke_width(),
    );
}

fn paint_change_summary(painter: &mut dyn Painter, rect: Rect, change: &str, theme: &ZodeTheme) {
    let y = rect.origin.y + 14.0;
    let right = rect.max_x() - 14.0;
    if let Some((additions, deletions)) = split_change_summary(change) {
        let additions_width = painter.measure_text_weighted(additions, 11.0, 500);
        let deletions_width = painter.measure_text_weighted(deletions, 11.0, 500);
        let x = (right - additions_width - 5.0 - deletions_width).max(rect.origin.x + 180.0);
        draw_text(
            painter,
            additions,
            Point2D::new(x, y),
            11.0,
            500,
            theme.success,
        );
        draw_text(
            painter,
            deletions,
            Point2D::new(x + additions_width + 5.0, y),
            11.0,
            500,
            theme.tokens.destructive,
        );
        return;
    }

    let x = (right - painter.measure_text_weighted(change, 11.0, 500)).max(rect.origin.x + 180.0);
    draw_text(
        painter,
        change,
        Point2D::new(x, y),
        11.0,
        500,
        theme.success,
    );
}

fn split_change_summary(change: &str) -> Option<(&str, &str)> {
    let mut parts = change.split_whitespace();
    let additions = parts.next()?;
    let deletions = parts.next()?;
    if parts.next().is_some() || !valid_delta(additions, '+') || !valid_delta(deletions, '-') {
        return None;
    }
    Some((additions, deletions))
}

fn valid_delta(value: &str, sign: char) -> bool {
    value.strip_prefix(sign).is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}
