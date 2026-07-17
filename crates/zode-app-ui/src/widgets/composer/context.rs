use jian_widgets::{components::tooltip::Tooltip, HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::GoalProgress;

use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme, PROJECT_DETACH_ID};

const CONTEXT_ICON_SIZE: f32 = 12.0;
const CONTEXT_ICON_GAP: f32 = 6.0;
const CONTEXT_ITEM_GAP: f32 = 20.0;
const DETACH_HIT_SIZE: f32 = 24.0;
const DETACH_VISUAL_SIZE: f32 = 20.0;
const TOOLTIP_W: f32 = 112.0;
const TOOLTIP_H: f32 = 28.0;
const TOOLTIP_GAP: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposerContextLayout {
    pub detach: Option<Rect>,
}

pub fn layout(rect: Rect, detachable_project: bool) -> ComposerContextLayout {
    let detach = (detachable_project && rect.size.x > 0.0 && rect.size.y > 0.0).then(|| {
        Rect::xywh(
            rect.origin.x + 10.0,
            rect.origin.y + (rect.size.y - DETACH_HIT_SIZE).max(0.0) / 2.0,
            DETACH_HIT_SIZE.min(rect.size.x.max(0.0)),
            DETACH_HIT_SIZE.min(rect.size.y.max(0.0)),
        )
    });
    ComposerContextLayout { detach }
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    rect: Rect,
    workspace_label: Option<&str>,
    connection_label: Option<&str>,
    branch: Option<&str>,
    goal: Option<&GoalProgress>,
    theme: &ZodeTheme,
) {
    paint_interactive(
        painter,
        rect,
        workspace_label,
        connection_label,
        branch,
        goal,
        false,
        None,
        None,
        theme,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_interactive(
    painter: &mut dyn Painter,
    rect: Rect,
    workspace_label: Option<&str>,
    connection_label: Option<&str>,
    branch: Option<&str>,
    goal: Option<&GoalProgress>,
    detachable_project: bool,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
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
    let context_layout = layout(rect, detachable_project && workspace_label.is_some());
    if let (Some(label), Some(detach)) = (workspace_label, context_layout.detach) {
        let detach_active =
            focused == Some(PROJECT_DETACH_ID) || hovered == Some(PROJECT_DETACH_ID);
        if detach_active {
            let visual = Rect::xywh(
                detach.origin.x + (detach.size.x - DETACH_VISUAL_SIZE) / 2.0,
                detach.origin.y + (detach.size.y - DETACH_VISUAL_SIZE) / 2.0,
                DETACH_VISUAL_SIZE,
                DETACH_VISUAL_SIZE,
            );
            painter.fill_round_rect(visual, DETACH_VISUAL_SIZE / 2.0, theme.tokens.accent);
            painter.stroke_svg_path(
                SemanticIcon::Close.path(),
                Point2D::new(visual.origin.x + 4.0, visual.origin.y + 4.0),
                12.0,
                theme.tokens.accent_foreground,
                SemanticIcon::Close.stroke_width(),
            );
            if focused == Some(PROJECT_DETACH_ID) {
                painter.stroke_round_rect(visual, DETACH_VISUAL_SIZE / 2.0, theme.tokens.ring, 1.5);
            }
        } else {
            painter.stroke_svg_path(
                SemanticIcon::Folder.path(),
                Point2D::new(
                    detach.origin.x + (detach.size.x - CONTEXT_ICON_SIZE) / 2.0,
                    detach.origin.y + (detach.size.y - CONTEXT_ICON_SIZE) / 2.0,
                ),
                CONTEXT_ICON_SIZE,
                theme.tokens.muted_foreground,
                SemanticIcon::Folder.stroke_width(),
            );
        }
        x = detach.max_x() + 4.0;
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
        x += label_width + CONTEXT_ITEM_GAP;
    }
    let standard_workspace = (!detachable_project).then_some(workspace_label).flatten();
    for (icon, label) in [
        (SemanticIcon::Folder, standard_workspace),
        (SemanticIcon::Host, connection_label),
        (SemanticIcon::Branch, branch),
    ]
    .into_iter()
    .filter_map(|(icon, label)| label.map(|label| (icon, label)))
    .filter(|(_, label)| !label.trim().is_empty())
    {
        let icon_origin = jian_widgets::Point2D::new(
            x,
            rect.origin.y + (rect.size.y - CONTEXT_ICON_SIZE).max(0.0) / 2.0,
        );
        painter.stroke_svg_path(
            icon.path(),
            icon_origin,
            CONTEXT_ICON_SIZE,
            theme.tokens.muted_foreground,
            icon.stroke_width(),
        );
        x += CONTEXT_ICON_SIZE + CONTEXT_ICON_GAP;
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
        x += label_width + CONTEXT_ITEM_GAP;
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
    if hovered == Some(PROJECT_DETACH_ID) || focused == Some(PROJECT_DETACH_ID) {
        if let Some(detach) = context_layout.detach {
            let tooltip_x = (detach.origin.x + detach.size.x / 2.0 - TOOLTIP_W / 2.0)
                .clamp(rect.origin.x, (rect.max_x() - TOOLTIP_W).max(rect.origin.x));
            Tooltip {
                label: "不在项目中工作",
            }
            .paint(
                painter,
                Rect::xywh(
                    tooltip_x,
                    detach.origin.y - TOOLTIP_GAP - TOOLTIP_H,
                    TOOLTIP_W.min(rect.size.x),
                    TOOLTIP_H,
                ),
                &theme.tokens,
            );
        }
    }
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
