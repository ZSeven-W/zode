//! Small rounded-full badge/tag. Radius is derived from the rect height so
//! it always reads as a pill regardless of the size a caller picks.

use jian_widgets::{HorizontalAlign, Painter, Rect, Tokens};

use crate::paint_single_line;

const PAD_X: f32 = 10.0;
const LABEL_FONT_SIZE: f32 = 11.0;
const LABEL_FONT_WEIGHT: u16 = 500;

pub struct Pill;

impl Pill {
    /// Minimum width so `label` (plus symmetric padding) is never clipped.
    pub fn min_width(painter: &mut dyn Painter, label: &str) -> f32 {
        painter.measure_text_weighted(label, LABEL_FONT_SIZE, LABEL_FONT_WEIGHT) + PAD_X * 2.0
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, label: &str, tokens: &Tokens) {
        let radius = rect.size.y / 2.0;
        painter.fill_round_rect(rect, radius, tokens.secondary);
        paint_single_line(
            painter,
            label,
            rect,
            LABEL_FONT_SIZE,
            LABEL_FONT_WEIGHT,
            tokens.secondary_foreground,
            HorizontalAlign::Center,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::test_support::RecordingPainter;

    #[test]
    fn radius_always_matches_a_full_pill_for_the_given_height() {
        let tokens = Tokens::light();
        let mut painter = RecordingPainter::default();
        for height in [16.0, 22.0, 28.0] {
            let rect = Rect::xywh(0.0, 0.0, 80.0, height);
            Pill::paint(&mut painter, rect, "beta", &tokens);
            assert_eq!(painter.fills.last().unwrap().radius, height / 2.0);
        }
    }

    #[test]
    fn painting_at_min_width_never_clips_the_label() {
        let tokens = Tokens::light();
        let mut painter = RecordingPainter::default();
        for label in ["新", "beta", "实验性功能", "a longer status label"] {
            let width = Pill::min_width(&mut painter, label);
            let rect = Rect::xywh(0.0, 0.0, width, 20.0);
            Pill::paint(&mut painter, rect, label, &tokens);
            let measured = painter.measure_text_weighted(label, LABEL_FONT_SIZE, LABEL_FONT_WEIGHT);
            assert!(width - PAD_X * 2.0 + 0.01 >= measured);
        }
    }
}
