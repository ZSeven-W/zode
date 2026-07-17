use jian_widgets::{centered_text_baseline_y, Painter, Point2D, Rect, TextLayout};
use zode_app_model::ZodeAppState;

use crate::ZodeTheme;

pub struct ThreadHeader;

impl ThreadHeader {
    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        let title = state
            .current_session
            .as_ref()
            .and_then(|session| {
                state
                    .threads
                    .iter()
                    .find(|thread| &thread.session == session)
            })
            .map(|thread| thread.title.as_str())
            .unwrap_or("新任务");
        let label_rect = Rect::xywh(
            rect.origin.x + 20.0,
            rect.origin.y,
            (rect.size.x - 40.0).max(0.0),
            rect.size.y,
        );
        let layout = TextLayout::single_run(
            title,
            "system-ui",
            13.0,
            theme.tokens.foreground.to_jian(),
            Point2D::ZERO,
        )
        .with_font_weight(600);
        painter.draw_text(
            &layout,
            Point2D::new(
                label_rect.origin.x,
                centered_text_baseline_y(label_rect, 13.0),
            ),
        );
        painter.stroke_line(
            Point2D::new(rect.origin.x, rect.origin.y + rect.size.y),
            Point2D::new(rect.origin.x + rect.size.x, rect.origin.y + rect.size.y),
            theme.tokens.border,
            1.0,
        );
    }
}
