use jian_widgets::{Painter, Point2D, Rect};
use zode_app_model::FileArtifact;

use crate::{RectExt, ZodeTheme};

use super::draw_text;

pub(super) const HEIGHT: f32 = 76.0;

pub(super) fn paint(painter: &mut dyn Painter, rect: Rect, file: &FileArtifact, theme: &ZodeTheme) {
    painter.fill_round_rect(rect, 10.0, theme.tokens.card);
    painter.stroke_round_rect(rect, 10.0, theme.tokens.border, 1.0);
    draw_text(
        painter,
        &file.summary,
        Point2D::new(rect.origin.x + 14.0, rect.origin.y + 27.0),
        13.0,
        600,
        theme.tokens.foreground,
    );
    draw_text(
        painter,
        &file.path,
        Point2D::new(rect.origin.x + 14.0, rect.origin.y + 51.0),
        11.0,
        400,
        theme.tokens.muted_foreground,
    );
    if let Some(change) = file.change_summary.as_deref() {
        let x = (rect.max_x() - painter.measure_text_weighted(change, 11.0, 500) - 14.0)
            .max(rect.origin.x + 180.0);
        draw_text(
            painter,
            change,
            Point2D::new(x, rect.origin.y + 27.0),
            11.0,
            500,
            theme.success,
        );
    }
}
