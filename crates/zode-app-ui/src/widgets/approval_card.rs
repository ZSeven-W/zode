use jian_widgets::{Painter, Point2D, Rect, TextLayout};
use zode_app_model::AppCommand;
use zode_node_protocol::ApprovalDecision;

use crate::ZodeTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalAction {
    AllowOnce,
    AllowAlways,
    Deny,
}

pub struct ApprovalCard;

impl ApprovalCard {
    pub const fn decision(action: ApprovalAction) -> ApprovalDecision {
        match action {
            ApprovalAction::AllowOnce => ApprovalDecision::AllowOnce,
            ApprovalAction::AllowAlways => ApprovalDecision::AllowAlways,
            ApprovalAction::Deny => ApprovalDecision::Deny,
        }
    }

    pub fn command(id: impl Into<String>, action: ApprovalAction) -> AppCommand {
        AppCommand::Approve {
            id: id.into(),
            decision: Self::decision(action),
        }
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, tool: &str, theme: &ZodeTheme) {
        painter.fill_round_rect(rect, 10.0, theme.tokens.muted);
        painter.stroke_round_rect(rect, 10.0, theme.tokens.border, 1.0);
        draw_text(
            painter,
            &format!("需要批准 · {tool}"),
            Point2D::new(rect.origin.x + 12.0, rect.origin.y + 22.0),
            12.0,
            600,
            theme.tokens.foreground,
        );
        let labels = ["允许一次", "始终允许", "拒绝"];
        let button_width = 72.0;
        for (index, label) in labels.iter().enumerate() {
            let x = rect.origin.x + 12.0 + index as f32 * (button_width + 8.0);
            let button = Rect::xywh(x, rect.origin.y + 32.0, button_width, 26.0);
            painter.fill_round_rect(
                button,
                7.0,
                if index == 2 {
                    theme.tokens.destructive.with_alpha(0.12)
                } else {
                    theme.tokens.card
                },
            );
            draw_text(
                painter,
                label,
                Point2D::new(x + 8.0, rect.origin.y + 50.0),
                11.0,
                500,
                if index == 2 {
                    theme.tokens.destructive
                } else {
                    theme.tokens.foreground
                },
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
