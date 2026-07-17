use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::GoalProgress;

use crate::{paint_single_line, RectExt, ZodeTheme};

pub(super) fn paint(
    painter: &mut dyn Painter,
    rect: Rect,
    workspace_label: Option<&str>,
    connection_label: Option<&str>,
    branch: Option<&str>,
    goal: Option<&GoalProgress>,
    theme: &ZodeTheme,
) {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return;
    }
    painter.fill_round_rect(rect, 12.0, theme.tokens.muted);
    painter.stroke_round_rect(rect, 12.0, theme.tokens.border, 1.0);
    painter.save();
    painter.clip_rect(rect);
    let mut x = rect.origin.x + 16.0;
    for label in [workspace_label, connection_label, branch]
        .into_iter()
        .flatten()
        .filter(|label| !label.trim().is_empty())
    {
        let label_width = painter.measure_text_weighted(label, 10.0, 400);
        paint_single_line(
            painter,
            label,
            Rect::xywh(x, rect.origin.y, label_width, rect.size.y),
            10.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
        x += label_width + 20.0;
        if x >= rect.max_x() - 16.0 {
            break;
        }
    }
    if let Some(goal) = goal {
        let label = format!("{} · {} / {}", goal.title, goal.completed, goal.total);
        if let Some(slot) = goal_slot(rect, x) {
            paint_single_line(
                painter,
                &label,
                slot,
                10.0,
                500,
                theme.zode_purple,
                HorizontalAlign::End,
            );
        }
    }
    painter.restore();
}

fn goal_slot(rect: Rect, occupied_right: f32) -> Option<Rect> {
    let right = (rect.max_x() - 16.0).max(rect.origin.x);
    let desired_width = (rect.size.x * 0.42).clamp(120.0, 300.0);
    let left = (right - desired_width)
        .max(occupied_right + 8.0)
        .max(rect.origin.x + 16.0);
    let width = (right - left).max(0.0);
    (width >= 48.0).then(|| Rect::xywh(left, rect.origin.y, width, rect.size.y))
}

#[cfg(test)]
mod tests {
    use super::goal_slot;
    use crate::RectExt;
    use jian_widgets::Rect;

    #[test]
    fn narrow_context_goal_slot_is_contained_or_omitted() {
        let rect = Rect::xywh(10.0, 20.0, 220.0, 44.0);
        let slot = goal_slot(rect, 80.0).expect("there is room for a compact goal");
        assert!(slot.min_x() >= rect.min_x());
        assert!(slot.max_x() <= rect.max_x());
        assert_eq!(slot.height(), rect.height());

        assert!(goal_slot(rect, 205.0).is_none());
    }
}
