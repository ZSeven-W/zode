//! Match banding for the in-conversation find bar.
//!
//! Painted underneath each visible transcript item, before the item itself,
//! so the band reads as a highlight behind the content rather than a wash
//! over it.

use jian_widgets::{Painter, Rect};
use zode_app_model::{TranscriptFindState, TranscriptState};

use crate::ZodeTheme;

/// Grown slightly past the item's own rect so the band reads as a highlight
/// around the content rather than a tight box clipping its glyphs.
const HIGHLIGHT_INSET: f32 = 4.0;

/// Bands a matched item: the current match gets the accent-tinted selection
/// token, every other match on screen gets the neutral one, so "where I am"
/// and "where else it appears" stay visually distinct without inventing a
/// color outside the theme. Items with no match paint nothing.
///
/// Highlighting is per item, not per matched run: wrapped markdown does not
/// expose the per-glyph geometry a run-level highlight would need. The match
/// records already carry byte ranges (`TranscriptFindMatch::start`/`end`), so
/// tightening this to the matched text later needs no state change.
pub(super) fn paint(
    painter: &mut dyn Painter,
    rect: Rect,
    transcript: &TranscriptState,
    find: &TranscriptFindState,
    index: usize,
    theme: &ZodeTheme,
) {
    let color = if find.is_active_item(transcript, index) {
        theme.tokens.row_selected_primary
    } else if find.is_matched_item(transcript, index) {
        theme.tokens.row_selected
    } else {
        return;
    };
    let band = Rect::xywh(
        rect.origin.x - HIGHLIGHT_INSET,
        rect.origin.y - HIGHLIGHT_INSET,
        rect.size.x + HIGHLIGHT_INSET * 2.0,
        rect.size.y + HIGHLIGHT_INSET * 2.0,
    );
    painter.fill_round_rect(band, theme.tokens.radius, color);
}

#[cfg(test)]
mod tests {
    use jian_widgets::{Color, Point2D, TextLayout};
    use zode_app_model::TranscriptItem;

    use super::*;

    #[derive(Default)]
    struct FillCapture(Vec<Color>);

    impl Painter for FillCapture {
        fn begin_frame(&mut self) {}
        fn end_frame(&mut self) {}
        fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
        fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
        fn draw_text(&mut self, _layout: &TextLayout, _origin: Point2D) {}
        fn clip_rect(&mut self, _rect: Rect) {}
        fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
        fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, color: Color) {
            self.0.push(color);
        }
        fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
        fn stroke_svg_path(
            &mut self,
            _d: &str,
            _top_left: Point2D,
            _size: f32,
            _color: Color,
            _width: f32,
        ) {
        }
        fn save(&mut self) {}
        fn restore(&mut self) {}
        fn translate(&mut self, _offset: Point2D) {}
        fn resize(&mut self, _width: u32, _height: u32) {}
        fn dpi_scale(&self) -> f32 {
            1.0
        }
    }

    fn fixture() -> (TranscriptState, TranscriptFindState) {
        let transcript = TranscriptState {
            items: vec![
                TranscriptItem::user_text("hit"),
                TranscriptItem::assistant_text("no match here"),
                TranscriptItem::assistant_text("hit again"),
            ],
            ..TranscriptState::default()
        };
        // Built by mutation rather than struct-update syntax: the memo field
        // is private to `zode-app-model`, so `..default()` is unavailable
        // outside that crate.
        let mut find = TranscriptFindState::default();
        find.open = true;
        find.query = "hit".into();
        (transcript, find)
    }

    fn band_color(index: usize) -> Option<Color> {
        let (transcript, find) = fixture();
        let theme = ZodeTheme::light();
        let mut painter = FillCapture::default();
        paint(
            &mut painter,
            Rect::xywh(0.0, 0.0, 100.0, 40.0),
            &transcript,
            &find,
            index,
            &theme,
        );
        painter.0.first().copied()
    }

    #[test]
    fn the_current_match_and_other_matches_use_distinct_tokens() {
        let theme = ZodeTheme::light();
        assert_eq!(band_color(0), Some(theme.tokens.row_selected_primary));
        assert_eq!(band_color(2), Some(theme.tokens.row_selected));
    }

    #[test]
    fn an_unmatched_item_paints_nothing() {
        assert_eq!(band_color(1), None);
    }
}
