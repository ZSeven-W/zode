use jian_widgets::{Painter, Point2D, Rect, TextLayout};
use zode_app_model::default_tool_expanded;
use zode_node_protocol::{ToolCall, ToolStatus};

use crate::ZodeTheme;

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
        painter.fill_oval(
            Rect::xywh(rect.origin.x + 10.0, rect.origin.y + 14.0, 7.0, 7.0),
            accent,
        );
        draw_text(
            painter,
            &tool.name,
            Point2D::new(rect.origin.x + 26.0, rect.origin.y + 22.0),
            12.0,
            600,
            theme.tokens.foreground,
        );
        if expanded {
            draw_text(
                painter,
                &tool.summary,
                Point2D::new(rect.origin.x + 12.0, rect.origin.y + 45.0),
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
