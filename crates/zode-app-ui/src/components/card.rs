//! Generic surface: fill + hairline border, optionally floating (the
//! shared [`crate::theme::Elevation::prominent`] shadow, painted through
//! [`crate::theme::paint_elevated_surface`]).
//!
//! This is deliberately thin - most existing card-like surfaces already
//! call `fill_round_rect` + `stroke_round_rect` directly and are not
//! migrated in this pass (see the crate-level module doc comment).

use jian_widgets::{Painter, Rect};

use crate::theme::paint_elevated_surface;
use crate::ZodeTheme;

pub struct Card;

impl Card {
    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        radius: f32,
        floating: bool,
        theme: &ZodeTheme,
    ) {
        if floating {
            paint_elevated_surface(painter, rect, radius, theme);
        }
        painter.fill_round_rect(rect, radius, theme.tokens.card);
        painter.stroke_round_rect(rect, radius, theme.tokens.border, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::test_support::RecordingPainter;

    #[test]
    fn floating_card_paints_the_shared_dual_layer_shadow_before_its_fill() {
        let theme = ZodeTheme::light();
        let mut painter = RecordingPainter::default();
        let rect = Rect::xywh(10.0, 10.0, 200.0, 100.0);

        Card::paint(&mut painter, rect, 12.0, true, &theme);

        assert_eq!(
            painter.shadows.len(),
            2,
            "expected the two elevation layers"
        );
        assert_eq!(
            painter.shadows[0].blur,
            theme.elevation_prominent.shadow.blur
        );
        assert_eq!(
            painter.shadows[0].color,
            theme.elevation_prominent.shadow.color
        );
        assert_eq!(painter.shadows[1].blur, theme.elevation_prominent.glow.blur);
        assert_eq!(
            painter.shadows[1].color,
            theme.elevation_prominent.glow.color
        );
        assert_eq!(painter.fills.last().unwrap().color, theme.tokens.card);
        assert_eq!(painter.strokes.last().unwrap().color, theme.tokens.border);
    }

    #[test]
    fn non_floating_card_paints_no_shadow() {
        let theme = ZodeTheme::light();
        let mut painter = RecordingPainter::default();
        Card::paint(
            &mut painter,
            Rect::xywh(0.0, 0.0, 100.0, 60.0),
            8.0,
            false,
            &theme,
        );
        assert!(painter.shadows.is_empty());
    }
}
