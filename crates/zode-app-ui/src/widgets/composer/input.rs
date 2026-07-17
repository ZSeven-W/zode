use jian_core::text_input::TextInputState;
use jian_widgets::{components::text_area::TextArea, Painter, Rect};
use zode_app_model::ComposerState;

use crate::ZodeTheme;

const TEXT_AREA_INSET_X: f32 = 8.0;
const TEXT_AREA_INSET_TOP: f32 = 6.0;
const TEXT_AREA_CONTROLS_H: f32 = 48.0;
const TEXT_AREA_FONT_SIZE: f32 = 14.0;
const TEXT_AREA_PAD_X: f32 = 8.0;
const TEXT_AREA_VISIBLE_LINES: usize = 3;
const INPUT_RADIUS: f32 = 16.0;

pub(super) fn paint_surface(
    painter: &mut dyn Painter,
    rect: Rect,
    input: &TextInputState,
    state: &ComposerState,
    theme: &ZodeTheme,
) {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return;
    }
    painter.fill_drop_shadow(
        Rect::xywh(rect.origin.x, rect.origin.y + 2.0, rect.size.x, rect.size.y),
        INPUT_RADIUS,
        22.0,
        theme.tokens.foreground.with_alpha(0.06),
    );
    painter.fill_round_rect(rect, INPUT_RADIUS, theme.tokens.card);
    painter.stroke_round_rect(rect, INPUT_RADIUS, theme.tokens.border, 1.0);

    text_area(input, state.focused).paint(painter, text_area_rect(rect), &theme.tokens);
}

pub(super) fn text_area<'a>(input: &'a TextInputState, focused: bool) -> TextArea<'a> {
    TextArea {
        state: input,
        placeholder: "随心输入",
        focused,
        font_size: TEXT_AREA_FONT_SIZE,
        now_ms: 0,
        pad_x: TEXT_AREA_PAD_X,
        max_visible_lines: TEXT_AREA_VISIBLE_LINES,
    }
}

pub(super) fn text_area_rect(rect: Rect) -> Rect {
    Rect::xywh(
        rect.origin.x + TEXT_AREA_INSET_X,
        rect.origin.y + TEXT_AREA_INSET_TOP,
        (rect.size.x - TEXT_AREA_INSET_X * 2.0).max(0.0),
        (rect.size.y - TEXT_AREA_CONTROLS_H).max(0.0),
    )
}
