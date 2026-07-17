use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::ZodeAppState;

use super::{draw_text, SettingsPanel};
use crate::{paint_single_line, ZodeTheme};

pub(super) fn paint(
    painter: &mut dyn Painter,
    rect: Rect,
    state: &ZodeAppState,
    theme: &ZodeTheme,
) {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return;
    }
    painter.fill_rect(rect, theme.sidebar);
    draw_text(
        painter,
        "设置",
        Point2D::new(rect.origin.x + 16.0, rect.origin.y + 64.0),
        16.0,
        600,
        theme.sidebar_foreground,
    );
    let search = Rect::xywh(
        rect.origin.x + 8.0,
        rect.origin.y + 86.0,
        (rect.size.x - 16.0).max(0.0),
        28.0,
    );
    painter.fill_round_rect(search, 8.0, theme.tokens.card);
    painter.stroke_round_rect(search, 8.0, theme.tokens.border, 1.0);
    paint_single_line(
        painter,
        "搜索即将支持",
        Rect::xywh(
            search.origin.x + 12.0,
            search.origin.y,
            (search.size.x - 24.0).max(0.0),
            search.size.y,
        ),
        12.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    draw_text(
        painter,
        "个人",
        Point2D::new(rect.origin.x + 16.0, rect.origin.y + 143.0),
        12.0,
        500,
        theme.tokens.muted_foreground,
    );
    for (_, row, _, label, selected, available) in SettingsPanel::category_rows(rect, state) {
        if selected {
            painter.fill_round_rect(row, 10.0, theme.tokens.row_selected);
        }
        let label_rect = Rect::xywh(
            row.origin.x + 10.0,
            row.origin.y,
            (row.size.x - 20.0).max(0.0),
            row.size.y,
        );
        paint_single_line(
            painter,
            label,
            label_rect,
            13.0,
            if selected { 600 } else { 450 },
            if available || selected {
                theme.sidebar_foreground
            } else {
                theme.tokens.muted_foreground
            },
            HorizontalAlign::Start,
        );
        if !available {
            paint_single_line(
                painter,
                "即将支持",
                label_rect,
                10.0,
                450,
                theme.tokens.muted_foreground,
                HorizontalAlign::End,
            );
        }
    }
}
