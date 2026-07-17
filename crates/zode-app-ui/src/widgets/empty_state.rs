use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};

use crate::{RectExt, ZodeTheme};

const TITLE: &str = "我们在 Zode 中构建什么？";
const MARK_CHEVRON: &str = "M4 8L8 12L4 16";
const MARK_Z: &str = "M10 5H20L11 19H21";
const SUGGESTIONS: [(&str, &str, &str); 4] = [
    (
        "探索代码",
        "理解现有项目",
        "M12 3L14 9L20 11L14 13L12 19L10 13L4 11L10 9Z",
    ),
    (
        "构建功能",
        "实现应用或工具",
        "M4 18L11 11M9 5L19 15M13 3L21 11L17 15L9 7Z",
    ),
    ("审查变更", "检查代码并提出建议", "M4 12L9 17L20 6"),
    (
        "修复问题",
        "诊断失败并修复",
        "M8 8H16V18H8ZM9 5L12 8L15 5M4 10H8M16 10H20M4 15H8M16 15H20",
    ),
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
        let mark_size = 40.0;
        let mark_origin = Point2D::new(
            rect.origin.x + (rect.size.x - mark_size) / 2.0,
            title_y - 58.0,
        );
        painter.stroke_svg_path(
            MARK_CHEVRON,
            mark_origin,
            mark_size,
            Color::rgb_u8(56, 189, 248),
            2.0,
        );
        painter.stroke_svg_path(MARK_Z, mark_origin, mark_size, theme.zode_purple, 2.4);
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
        for (index, (title, detail, icon)) in SUGGESTIONS.iter().enumerate() {
            let card = Rect::xywh(
                rect.origin.x + index as f32 * (card_width + gap),
                card_y,
                card_width,
                104.0_f32.min((rect.max_y() - card_y).max(0.0)),
            );
            painter.fill_round_rect(card, 12.0, theme.tokens.card);
            painter.stroke_round_rect(card, 12.0, theme.tokens.border, 1.0);
            painter.stroke_svg_path(
                icon,
                Point2D::new(card.origin.x + 14.0, card.origin.y + 14.0),
                18.0,
                suggestion_color(index),
                1.7,
            );
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

fn suggestion_color(index: usize) -> Color {
    match index {
        0 => Color::rgb_u8(59, 130, 246),
        1 => Color::rgb_u8(139, 92, 246),
        2 => Color::rgb_u8(34, 197, 94),
        _ => Color::rgb_u8(249, 115, 22),
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
