use jian_widgets::{Painter, Point2D, Rect, TextLayout};
use zode_app_model::ComposerState;

use crate::ZodeTheme;

pub struct Composer;

impl Composer {
    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ComposerState, theme: &ZodeTheme) {
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

        let prompt = if state.draft.is_empty() {
            "向 Zode 描述一个任务"
        } else {
            state.draft.as_str()
        };
        draw_text(
            painter,
            prompt,
            Point2D::new(rect.origin.x + 16.0, rect.origin.y + 30.0),
            14.0,
            if state.draft.is_empty() {
                theme.tokens.muted_foreground
            } else {
                theme.tokens.foreground
            },
        );
        let model = state.model.as_deref().unwrap_or("选择模型");
        draw_text(
            painter,
            model,
            Point2D::new(rect.origin.x + 16.0, rect.origin.y + rect.size.y - 16.0),
            11.0,
            theme.tokens.muted_foreground,
        );
        painter.fill_round_rect(
            Rect::xywh(
                rect.origin.x + rect.size.x - 42.0,
                rect.origin.y + rect.size.y - 38.0,
                28.0,
                28.0,
            ),
            14.0,
            theme.zode_purple,
        );
    }
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    size: f32,
    color: jian_widgets::Color,
) {
    let layout = TextLayout::single_run(text, "system-ui", size, color.to_jian(), Point2D::ZERO);
    painter.draw_text(&layout, origin);
}
