//! Static "display field" chrome: the focus-ring convention lifted from
//! `widgets/settings/provider_models/paint_helpers.rs::paint_input` (also
//! matched by the archived-tasks, global-search, and integrations search
//! boxes) - same surface for focused and unfocused states, only the border
//! swaps for the ring so a focused field never reads as disabled.
//!
//! This paints a plain value string, not an editable caret/selection.
//! Live-typed fields should use `jian_widgets::components::input::Input`
//! (backed by a real `TextInputState`) instead.

use jian_widgets::{HorizontalAlign, Painter, Rect, Tokens};

use crate::paint_single_line;

const PAD_X: f32 = 12.0;
const VALUE_FONT_SIZE: f32 = 12.0;
const VALUE_FONT_WEIGHT: u16 = 400;

pub struct Input;

impl Input {
    /// `radius` is caller-supplied rather than always `tokens.radius`, for
    /// the same reason as `Button::paint` - call sites already committed to
    /// a specific step of the radius scale should not have their corners
    /// silently resized by adopting this component.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        radius: f32,
        value: &str,
        muted: bool,
        focused: bool,
        tokens: &Tokens,
    ) {
        painter.fill_round_rect(rect, radius, tokens.card);
        let (border, border_width) = if focused {
            (tokens.ring, 1.5)
        } else {
            (tokens.input, 1.0)
        };
        painter.stroke_round_rect(rect, radius, border, border_width);
        paint_single_line(
            painter,
            value,
            Rect::xywh(
                rect.origin.x + PAD_X,
                rect.origin.y,
                (rect.size.x - PAD_X * 2.0).max(0.0),
                rect.size.y,
            ),
            VALUE_FONT_SIZE,
            VALUE_FONT_WEIGHT,
            if muted {
                tokens.muted_foreground
            } else {
                tokens.foreground
            },
            HorizontalAlign::Start,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::test_support::RecordingPainter;

    #[test]
    fn focus_swaps_the_border_for_the_ring_token() {
        let tokens = Tokens::light();
        let rect = Rect::xywh(0.0, 0.0, 200.0, 32.0);

        let mut focused_painter = RecordingPainter::default();
        Input::paint(
            &mut focused_painter,
            rect,
            8.0,
            "value",
            false,
            true,
            &tokens,
        );
        let focused_stroke = focused_painter.strokes.last().unwrap();
        assert_eq!(focused_stroke.color, tokens.ring);
        assert_eq!(focused_stroke.width, 1.5);

        let mut idle_painter = RecordingPainter::default();
        Input::paint(&mut idle_painter, rect, 8.0, "value", false, false, &tokens);
        let idle_stroke = idle_painter.strokes.last().unwrap();
        assert_eq!(idle_stroke.color, tokens.input);
        assert_eq!(idle_stroke.width, 1.0);
    }

    #[test]
    fn the_surface_fill_is_identical_whether_focused_or_not() {
        // A focused field must not look disabled: only the border changes.
        let tokens = Tokens::light();
        let rect = Rect::xywh(0.0, 0.0, 200.0, 32.0);

        let mut focused_painter = RecordingPainter::default();
        Input::paint(
            &mut focused_painter,
            rect,
            8.0,
            "value",
            false,
            true,
            &tokens,
        );

        let mut idle_painter = RecordingPainter::default();
        Input::paint(&mut idle_painter, rect, 8.0, "value", false, false, &tokens);

        assert_eq!(
            focused_painter.fills.last().unwrap().color,
            idle_painter.fills.last().unwrap().color
        );
    }

    #[test]
    fn muted_placeholder_uses_the_muted_foreground_token() {
        let tokens = Tokens::light();
        let rect = Rect::xywh(0.0, 0.0, 200.0, 32.0);
        let mut painter = RecordingPainter::default();
        Input::paint(&mut painter, rect, 8.0, "未设置", true, false, &tokens);
        let text = painter.texts.last().unwrap();
        assert_eq!(text.rect.origin.x, rect.origin.x + PAD_X);
        assert!(text.rect.size.x <= rect.size.x - PAD_X * 2.0 + 0.01);
        assert_eq!(text.color, tokens.muted_foreground.to_jian());
    }
}
