use jian_widgets::{Painter, Point2D, Rect, TextLayout};

use crate::{RectExt, ZodeTheme};

const TITLE: &str = "我们在 Zode 中构建什么？";
const SUGGESTIONS: [(&str, &str); 4] = [
    ("探索代码", "理解现有项目"),
    ("构建功能", "实现应用或工具"),
    ("审查变更", "检查代码并提出建议"),
    ("修复问题", "诊断失败并修复"),
];

pub struct EmptyState;

impl EmptyState {
    pub fn paint(painter: &mut dyn Painter, rect: Rect, theme: &ZodeTheme) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        let title_size = 27.0;
        let title_width = painter.measure_text_weighted(TITLE, title_size, 500);
        let title_y = rect.origin.y + rect.size.y * 0.5 - 70.0;
        draw_text(
            painter,
            TITLE,
            Point2D::new(rect.origin.x + (rect.size.x - title_width) / 2.0, title_y),
            title_size,
            500,
            theme.tokens.foreground,
        );

        let gap = 12.0_f32.min(rect.size.x / 12.0);
        let card_width = ((rect.size.x - gap * 3.0) / 4.0).max(0.0);
        let card_y = title_y + 46.0;
        for (index, (title, detail)) in SUGGESTIONS.iter().enumerate() {
            let card = Rect::xywh(
                rect.origin.x + index as f32 * (card_width + gap),
                card_y,
                card_width,
                104.0_f32.min((rect.max_y() - card_y).max(0.0)),
            );
            painter.fill_round_rect(card, 12.0, theme.tokens.card);
            painter.stroke_round_rect(card, 12.0, theme.tokens.border, 1.0);
            draw_text(
                painter,
                title,
                Point2D::new(card.origin.x + 14.0, card.origin.y + 46.0),
                13.0,
                600,
                theme.tokens.foreground,
            );
            draw_text(
                painter,
                detail,
                Point2D::new(card.origin.x + 14.0, card.origin.y + 72.0),
                11.0,
                400,
                theme.tokens.muted_foreground,
            );
        }
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
