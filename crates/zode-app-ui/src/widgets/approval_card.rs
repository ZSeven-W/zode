use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::AppCommand;
use zode_node_protocol::ApprovalDecision;

use crate::{paint_single_line, ZodeTheme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalAction {
    AllowOnce,
    AllowAlways,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApprovalButtonLayout {
    pub action: ApprovalAction,
    pub label: &'static str,
    pub rect: Rect,
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

    /// Returns the exact button geometry consumed by paint, hit testing and
    /// accessibility. Keeping this calculation in one place prevents a
    /// semantic control from drifting away from its visible target.
    pub fn button_layout(rect: Rect) -> [ApprovalButtonLayout; 3] {
        let labels = ["允许一次", "始终允许", "拒绝"];
        let actions = [
            ApprovalAction::AllowOnce,
            ApprovalAction::AllowAlways,
            ApprovalAction::Deny,
        ];
        let button_width = 72.0;
        std::array::from_fn(|index| ApprovalButtonLayout {
            action: actions[index],
            label: labels[index],
            rect: Rect::xywh(
                rect.origin.x + 12.0 + index as f32 * (button_width + 8.0),
                rect.origin.y + 32.0,
                button_width,
                26.0,
            ),
        })
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, tool: &str, theme: &ZodeTheme) {
        painter.fill_round_rect(rect, 10.0, theme.tokens.muted);
        painter.stroke_round_rect(rect, 10.0, theme.tokens.border, 1.0);
        paint_single_line(
            painter,
            &format!("需要批准 · {tool}"),
            Rect::xywh(
                rect.origin.x + 12.0,
                rect.origin.y + 2.0,
                (rect.size.x - 24.0).max(0.0),
                30.0,
            ),
            12.0,
            600,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        for (index, button) in Self::button_layout(rect).into_iter().enumerate() {
            painter.fill_round_rect(
                button.rect,
                7.0,
                if index == 2 {
                    theme.tokens.destructive.with_alpha(0.12)
                } else {
                    theme.tokens.card
                },
            );
            paint_single_line(
                painter,
                button.label,
                button.rect,
                11.0,
                500,
                if index == 2 {
                    theme.tokens.destructive
                } else {
                    theme.tokens.foreground
                },
                HorizontalAlign::Center,
            );
        }
    }
}
