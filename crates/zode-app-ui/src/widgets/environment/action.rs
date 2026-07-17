use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::EnvironmentAction;

use crate::{paint_single_line, WidgetId, ZodeTheme};

pub(super) const ACTION_HEADER_HEIGHT: f32 = 18.0;
pub(super) const ACTION_ROW_HEIGHT: f32 = 34.0;
pub(super) const ACTION_ROW_GAP: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvironmentActionLayout {
    pub id: WidgetId,
    pub action: EnvironmentAction,
    pub rect: Rect,
}

pub(super) fn section_height(action_count: usize) -> f32 {
    ACTION_HEADER_HEIGHT
        + ACTION_ROW_HEIGHT * action_count as f32
        + ACTION_ROW_GAP * action_count.saturating_sub(1) as f32
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    header: Rect,
    actions: &[EnvironmentActionLayout],
    theme: &ZodeTheme,
) {
    paint_single_line(
        painter,
        "仓库操作",
        header,
        11.0,
        600,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    for layout in actions {
        let enabled = layout.action.enabled();
        let foreground = if enabled {
            theme.tokens.foreground
        } else {
            theme.tokens.muted_foreground.with_alpha(0.72)
        };
        painter.fill_round_rect(
            layout.rect,
            8.0,
            if enabled {
                theme.tokens.muted
            } else {
                theme.tokens.muted.with_alpha(0.45)
            },
        );
        painter.stroke_round_rect(layout.rect, 8.0, theme.tokens.border, 1.0);
        paint_single_line(
            painter,
            layout.action.kind.label(),
            Rect::xywh(
                layout.rect.origin.x + 10.0,
                layout.rect.origin.y,
                132.0_f32.min((layout.rect.size.x - 20.0).max(0.0)),
                layout.rect.size.y,
            ),
            12.0,
            if enabled { 600 } else { 500 },
            foreground,
            HorizontalAlign::Start,
        );
        if let Some(reason) = layout.action.unavailable_reason {
            paint_single_line(
                painter,
                reason.message(),
                Rect::xywh(
                    layout.rect.origin.x + 142.0,
                    layout.rect.origin.y,
                    (layout.rect.size.x - 152.0).max(0.0),
                    layout.rect.size.y,
                ),
                10.0,
                400,
                foreground,
                HorizontalAlign::End,
            );
        }
    }
}
