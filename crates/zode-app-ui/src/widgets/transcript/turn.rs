use jian_widgets::{Painter, Point2D, Rect};

use crate::{RectExt, SemanticIcon, TypographyRole, ZodeTheme};

pub(super) const TOP_GAP: f32 = 44.0;
pub(super) const ROW_HEIGHT: f32 = 34.0;
pub(super) const HEIGHT: f32 = TOP_GAP + ROW_HEIGHT;

pub(super) fn paint(painter: &mut dyn Painter, rect: Rect, label: &str, theme: &ZodeTheme) {
    let row = Rect::xywh(
        rect.origin.x,
        rect.origin.y + TOP_GAP,
        rect.size.x,
        ROW_HEIGHT,
    );
    let style = TypographyRole::UiLabel.style();
    let label_width = painter.measure_text_weighted(label, style.size, style.weight);
    super::draw_text(
        painter,
        label,
        Point2D::new(row.origin.x, row.origin.y + 3.0),
        style.size,
        style.weight,
        theme.tokens.muted_foreground,
    );

    let icon = SemanticIcon::ChevronRight;
    painter.stroke_svg_path(
        icon.path(),
        Point2D::new(row.origin.x + label_width + 8.0, row.origin.y + 4.0),
        13.0,
        theme.tokens.muted_foreground.with_alpha(0.72),
        icon.stroke_width(),
    );
    painter.stroke_line(
        Point2D::new(row.origin.x, row.max_y() - 1.0),
        Point2D::new(row.max_x(), row.max_y() - 1.0),
        theme.tokens.border.with_alpha(0.82),
        1.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_reserves_reference_spacing_between_bubble_and_answer() {
        assert_eq!(TOP_GAP, 44.0);
        assert_eq!(ROW_HEIGHT, 34.0);
        assert_eq!(HEIGHT, TOP_GAP + ROW_HEIGHT);
    }
}
