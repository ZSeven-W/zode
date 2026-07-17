use jian_core::text_input::TextInputState;
use jian_widgets::{components::text_area::TextArea, HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::ComposerState;

use crate::{paint_single_line, RectExt, ZodeTheme};

pub(super) fn paint(
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
        12.0,
        18.0,
        theme.tokens.foreground.with_alpha(0.08),
    );
    painter.fill_round_rect(rect, 12.0, theme.tokens.card);
    painter.stroke_round_rect(rect, 12.0, theme.tokens.border, 1.0);

    TextArea {
        state: input,
        placeholder: "向 Zode 描述一个任务",
        focused: state.focused,
        font_size: 14.0,
        now_ms: 0,
        pad_x: 8.0,
        max_visible_lines: 3,
    }
    .paint(
        painter,
        Rect::xywh(
            rect.origin.x + 8.0,
            rect.origin.y + 6.0,
            (rect.size.x - 16.0).max(0.0),
            (rect.size.y - 48.0).max(0.0),
        ),
        &theme.tokens,
    );

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
        "M4 12H20M12 4V20",
        plus.origin,
        16.0,
        theme.tokens.muted_foreground,
        1.5,
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
        "M9 5V12A3 3 0 0 0 15 12V5M6 11A6 6 0 0 0 18 11M12 17V21",
        mic.origin,
        16.0,
        theme.tokens.muted_foreground,
        1.4,
    );
    let send = Rect::xywh(rect.max_x() - 42.0, controls.origin.y, 28.0, 28.0);
    painter.fill_round_rect(send, 14.0, theme.zode_purple);
    painter.stroke_svg_path(
        "M7 13L12 8L17 13M12 8V18",
        Point2D::new(send.origin.x + 6.0, send.origin.y + 6.0),
        16.0,
        jian_widgets::Color::WHITE,
        1.6,
    );
}
