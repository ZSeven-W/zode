use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::default_tool_expanded;
use zode_node_protocol::{ToolCall, ToolStatus};

use crate::{paint_single_line, ZodeTheme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTone {
    Running,
    Success,
    Failure,
}

pub struct ToolCard;

impl ToolCard {
    pub fn default_expanded(tool: &ToolCall) -> bool {
        default_tool_expanded(&tool.name)
    }

    pub const fn tone(tool: &ToolCall) -> ToolTone {
        match tool.status {
            ToolStatus::Running => ToolTone::Running,
            ToolStatus::Completed => ToolTone::Success,
            ToolStatus::Failed => ToolTone::Failure,
        }
    }

    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        tool: &ToolCall,
        expanded: bool,
        theme: &ZodeTheme,
    ) {
        let tone = Self::tone(tool);
        let accent = match tone {
            ToolTone::Running => theme.tokens.muted_foreground,
            ToolTone::Success => theme.success,
            ToolTone::Failure => theme.tokens.destructive,
        };
        painter.fill_round_rect(rect, 8.0, theme.tokens.muted);
        painter.stroke_round_rect(rect, 8.0, accent.with_alpha(0.55), 1.0);
        let header = Rect::xywh(rect.origin.x, rect.origin.y, rect.size.x, 35.0);
        painter.fill_oval(
            Rect::xywh(
                rect.origin.x + 10.0,
                header.origin.y + (header.size.y - 7.0) / 2.0,
                7.0,
                7.0,
            ),
            accent,
        );
        paint_single_line(
            painter,
            &tool.name,
            Rect::xywh(
                rect.origin.x + 26.0,
                header.origin.y,
                (rect.size.x - 38.0).max(0.0),
                header.size.y,
            ),
            12.0,
            600,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        if expanded {
            paint_single_line(
                painter,
                &tool.summary,
                Rect::xywh(
                    rect.origin.x + 12.0,
                    rect.origin.y + 35.0,
                    (rect.size.x - 24.0).max(0.0),
                    (rect.size.y - 35.0).max(0.0),
                ),
                11.0,
                400,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
        }
    }
}
