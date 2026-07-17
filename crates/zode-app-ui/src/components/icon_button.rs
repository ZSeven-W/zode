//! Icon-only square button: hover background tint + a centered stroked
//! icon. Complements [`crate::components::Button`] for label-less actions
//! (close, more, chevrons).

use jian_widgets::{Painter, Point2D, Rect, Tokens};

use crate::SemanticIcon;

pub struct IconButton;

impl IconButton {
    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        icon: SemanticIcon,
        icon_size: f32,
        hovered: bool,
        tokens: &Tokens,
    ) {
        if hovered {
            painter.fill_round_rect(rect, tokens.radius, tokens.accent);
        }
        let inset_x = (rect.size.x - icon_size) / 2.0;
        let inset_y = (rect.size.y - icon_size) / 2.0;
        painter.stroke_svg_path(
            icon.path(),
            Point2D::new(rect.origin.x + inset_x, rect.origin.y + inset_y),
            icon_size,
            tokens.foreground,
            1.25,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::test_support::RecordingPainter;

    #[test]
    fn icon_is_centered_within_the_rect() {
        let tokens = Tokens::light();
        let mut painter = RecordingPainter::default();
        let rect = Rect::xywh(20.0, 30.0, 32.0, 32.0);
        IconButton::paint(
            &mut painter,
            rect,
            SemanticIcon::Delete,
            14.0,
            false,
            &tokens,
        );
        let svg = painter.svg_strokes.last().unwrap();
        assert_eq!(svg.top_left.x, rect.origin.x + (rect.size.x - 14.0) / 2.0);
        assert_eq!(svg.top_left.y, rect.origin.y + (rect.size.y - 14.0) / 2.0);
    }

    #[test]
    fn hover_paints_a_background_tint_only_when_hovered() {
        let tokens = Tokens::light();
        let rect = Rect::xywh(0.0, 0.0, 28.0, 28.0);

        let mut hovered_painter = RecordingPainter::default();
        IconButton::paint(
            &mut hovered_painter,
            rect,
            SemanticIcon::Edit,
            14.0,
            true,
            &tokens,
        );
        assert_eq!(hovered_painter.fills.len(), 1);
        assert_eq!(hovered_painter.fills[0].color, tokens.accent);

        let mut idle_painter = RecordingPainter::default();
        IconButton::paint(
            &mut idle_painter,
            rect,
            SemanticIcon::Edit,
            14.0,
            false,
            &tokens,
        );
        assert!(idle_painter.fills.is_empty());
    }
}
