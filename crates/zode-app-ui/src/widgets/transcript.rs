use jian_widgets::{Painter, Point2D, Rect, TextLayout};
use zode_app_model::ZodeAppState;

use crate::ZodeTheme;

pub struct ThreadTranscript;

impl ThreadTranscript {
    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        painter.save();
        painter.clip_rect(rect);
        let (headline, detail) = if state.current_session.is_none() {
            ("开始一项任务", "描述你想构建、修改或探索的内容。")
        } else {
            ("任务已准备好", "消息与工具活动会显示在这里。")
        };
        draw_text(
            painter,
            headline,
            Point2D::new(rect.origin.x, rect.origin.y + 30.0),
            17.0,
            600,
            theme.tokens.foreground,
        );
        draw_text(
            painter,
            detail,
            Point2D::new(rect.origin.x, rect.origin.y + 56.0),
            13.0,
            400,
            theme.tokens.muted_foreground,
        );
        painter.restore();
    }
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    size: f32,
    weight: u16,
    color: jian_widgets::Color,
) {
    let layout = TextLayout::single_run(text, "system-ui", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(&layout, origin);
}
