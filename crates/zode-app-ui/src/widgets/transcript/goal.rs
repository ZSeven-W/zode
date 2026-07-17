use jian_widgets::{Painter, Point2D, Rect};
use zode_app_model::GoalProgress;

use crate::{RectExt, ZodeTheme};

use super::draw_text;

pub(super) const HEIGHT: f32 = 74.0;

pub(super) fn paint(painter: &mut dyn Painter, rect: Rect, goal: &GoalProgress, theme: &ZodeTheme) {
    painter.fill_round_rect(rect, 10.0, theme.tokens.card);
    painter.stroke_round_rect(rect, 10.0, theme.tokens.border, 1.0);
    draw_text(
        painter,
        &goal.title,
        Point2D::new(rect.origin.x + 14.0, rect.origin.y + 26.0),
        12.0,
        600,
        theme.tokens.foreground,
    );
    let count = format!("{} / {}", goal.completed, goal.total);
    let count_width = painter.measure_text_weighted(&count, 11.0, 500);
    draw_text(
        painter,
        &count,
        Point2D::new(rect.max_x() - count_width - 14.0, rect.origin.y + 26.0),
        11.0,
        500,
        theme.tokens.muted_foreground,
    );
    let track = Rect::xywh(
        rect.origin.x + 14.0,
        rect.origin.y + 46.0,
        (rect.size.x - 28.0).max(0.0),
        6.0,
    );
    painter.fill_round_rect(track, 3.0, theme.tokens.muted);
    painter.fill_round_rect(
        Rect::xywh(
            track.origin.x,
            track.origin.y,
            track.size.x * goal.fraction(),
            6.0,
        ),
        3.0,
        theme.zode_purple,
    );
}
