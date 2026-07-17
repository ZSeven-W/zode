use jian_core::text_input::TextInputState;
use jian_widgets::{components::text_area::TextArea, HorizontalAlign, Painter, Rect};
use zode_app_model::ComposerState;

use crate::{paint_single_line, RectExt, SemanticIcon, ZodeTheme};

const TEXT_AREA_INSET_X: f32 = 8.0;
const TEXT_AREA_INSET_TOP: f32 = 6.0;
const TEXT_AREA_CONTROLS_H: f32 = 48.0;
const TEXT_AREA_FONT_SIZE: f32 = 14.0;
const TEXT_AREA_PAD_X: f32 = 8.0;
const TEXT_AREA_VISIBLE_LINES: usize = 3;
const INPUT_RADIUS: f32 = 16.0;

pub(super) fn paint(
    painter: &mut dyn Painter,
    rect: Rect,
    input: &TextInputState,
    state: &ComposerState,
    busy: bool,
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

    let controls = Rect::xywh(
        rect.origin.x + 14.0,
        rect.max_y() - 38.0,
        (rect.size.x - 28.0).max(0.0),
        28.0,
    );
    let plus = Rect::xywh(
        controls.origin.x,
        controls.origin.y + (controls.size.y - 16.0) / 2.0,
        16.0,
        16.0,
    );
    painter.stroke_svg_path(
        SemanticIcon::NewTask.path(),
        plus.origin,
        plus.size.x,
        theme.tokens.muted_foreground,
        SemanticIcon::NewTask.stroke_width(),
    );
    let model_x = (rect.max_x() - 190.0).max(rect.origin.x + 140.0);
    let effort_x = (rect.max_x() - 108.0).max(rect.origin.x + 220.0);
    let mic = Rect::xywh(
        rect.max_x() - 70.0,
        controls.origin.y + (controls.size.y - 16.0) / 2.0,
        16.0,
        16.0,
    );
    if !state.sandbox_label.trim().is_empty() {
        paint_single_line(
            painter,
            &state.sandbox_label,
            Rect::xywh(
                rect.origin.x + 44.0,
                controls.origin.y,
                (model_x - rect.origin.x - 52.0).max(0.0),
                controls.size.y,
            ),
            11.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    }
    paint_single_line(
        painter,
        state.model.as_deref().unwrap_or("选择模型"),
        Rect::xywh(
            model_x,
            controls.origin.y,
            (effort_x - model_x - 8.0).max(0.0),
            controls.size.y,
        ),
        11.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    if let Some(effort) = state
        .effort
        .as_deref()
        .filter(|effort| !effort.trim().is_empty())
    {
        paint_single_line(
            painter,
            effort,
            Rect::xywh(
                effort_x,
                controls.origin.y,
                (mic.origin.x - effort_x - 8.0).max(0.0),
                controls.size.y,
            ),
            11.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    }
    painter.stroke_svg_path(
        SemanticIcon::Microphone.path(),
        mic.origin,
        mic.size.x,
        theme.tokens.muted_foreground,
        SemanticIcon::Microphone.stroke_width(),
    );
    let send = Rect::xywh(rect.max_x() - 42.0, controls.origin.y, 28.0, 28.0);
    if busy {
        painter.fill_round_rect(send, 14.0, theme.tokens.foreground);
        let stop = Rect::xywh(
            send.origin.x + (send.size.x - 8.0) / 2.0,
            send.origin.y + (send.size.y - 8.0) / 2.0,
            8.0,
            8.0,
        );
        painter.fill_round_rect(stop, 1.5, theme.tokens.background);
    } else {
        painter.fill_round_rect(send, 14.0, theme.zode_purple);
        let send_icon = Rect::xywh(send.origin.x + 6.0, send.origin.y + 6.0, 16.0, 16.0);
        painter.stroke_svg_path(
            SemanticIcon::Send.path(),
            send_icon.origin,
            send_icon.size.x,
            jian_widgets::Color::WHITE,
            SemanticIcon::Send.stroke_width(),
        );
    }
}

pub(super) fn text_area<'a>(input: &'a TextInputState, focused: bool) -> TextArea<'a> {
    TextArea {
        state: input,
        placeholder: "向 Zode 描述一个任务",
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
