//! Selectable menu-row background + label, unifying the
//! `painter.fill_round_rect(row.rect, 7.0, tokens.accent)` hover tint
//! hand-rolled across composer-context-menu, global_search,
//! open-with-menu, project_sidebar::menu and thread-header-overlay.

use jian_widgets::{HorizontalAlign, Painter, Rect, Tokens};

use crate::paint_single_line;

pub struct MenuRow;

impl MenuRow {
    /// Radius shared by every hand-rolled row today, named instead of
    /// repeated as a bare literal at each call site.
    pub const RADIUS: f32 = 7.0;

    pub fn paint_background(painter: &mut dyn Painter, rect: Rect, hovered: bool, tokens: &Tokens) {
        if hovered {
            painter.fill_round_rect(rect, Self::RADIUS, tokens.accent);
        }
    }

    pub fn paint_label(
        painter: &mut dyn Painter,
        rect: Rect,
        label: &str,
        muted: bool,
        tokens: &Tokens,
    ) {
        paint_single_line(
            painter,
            label,
            rect,
            13.0,
            400,
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
    fn background_paints_only_when_hovered_at_the_shared_radius() {
        let tokens = Tokens::light();
        let rect = Rect::xywh(4.0, 4.0, 200.0, 30.0);

        let mut hovered_painter = RecordingPainter::default();
        MenuRow::paint_background(&mut hovered_painter, rect, true, &tokens);
        assert_eq!(hovered_painter.fills.len(), 1);
        assert_eq!(hovered_painter.fills[0].radius, MenuRow::RADIUS);
        assert_eq!(hovered_painter.fills[0].color, tokens.accent);

        let mut idle_painter = RecordingPainter::default();
        MenuRow::paint_background(&mut idle_painter, rect, false, &tokens);
        assert!(idle_painter.fills.is_empty());
    }

    #[test]
    fn muted_label_uses_the_muted_foreground_token() {
        let tokens = Tokens::light();
        let rect = Rect::xywh(0.0, 0.0, 100.0, 20.0);
        let mut painter = RecordingPainter::default();
        MenuRow::paint_label(&mut painter, rect, "disabled item", true, &tokens);
        let text = painter.texts.last().unwrap();
        assert_eq!(text.rect, rect);
        assert_eq!(text.color, tokens.muted_foreground.to_jian());
    }
}
