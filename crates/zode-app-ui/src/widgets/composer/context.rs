use jian_widgets::{components::tooltip::Tooltip, HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::GoalProgress;

use super::{COMPOSER_BRANCH_ID, COMPOSER_LOCATION_ID, COMPOSER_PROJECT_ID, PROJECT_DETACH_ID};
use crate::{
    paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme, COMPOSER_CONTEXT_OVERLAP,
};

const CONTEXT_ICON_SIZE: f32 = 14.0;
const CONTEXT_FONT_SIZE: f32 = 14.0;
const CONTEXT_ICON_GAP: f32 = 8.0;
const CONTEXT_ITEM_GAP: f32 = 24.0;
const CONTEXT_INSET_X: f32 = 18.0;
const CONTEXT_RADIUS: f32 = 18.0;
const CHIP_HEIGHT: f32 = 28.0;
const CHIP_RADIUS: f32 = CHIP_HEIGHT / 2.0;
const CHIP_GAP: f32 = 2.0;
const CHIP_INSET_X: f32 = 10.0;
const CHIP_PAD_X: f32 = 10.0;
const CHIP_MIN_WIDTH: f32 = 48.0;
const PROJECT_CHIP_MIN_WIDTH: f32 = 56.0;
const PROJECT_LABEL_GAP: f32 = 2.0;
const MAX_CHIP_LABEL_WIDTH: f32 = 180.0;
const CHIP_LABEL_WIDTH_SLOP: f32 = 1.0;
const DETACH_HIT_SIZE: f32 = 24.0;
const DETACH_VISUAL_SIZE: f32 = 20.0;
const TOOLTIP_H: f32 = 28.0;
const TOOLTIP_GAP: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposerContextChipLayout {
    pub id: WidgetId,
    /// Complete visual pill, including the project detach affordance.
    pub rect: Rect,
    /// Click target for the chip action. Project detach never overlaps this rectangle.
    pub action_rect: Rect,
    pub label_rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposerContextLayout {
    pub project: Option<ComposerContextChipLayout>,
    pub location: Option<ComposerContextChipLayout>,
    pub branch: Option<ComposerContextChipLayout>,
    pub detach: Option<Rect>,
}

impl ComposerContextLayout {
    pub fn hit_test(&self, point: Point2D) -> Option<WidgetId> {
        if self.detach.is_some_and(|rect| rect.contains(point)) {
            return Some(PROJECT_DETACH_ID);
        }
        [self.project, self.location, self.branch]
            .into_iter()
            .flatten()
            .find(|chip| chip.action_rect.contains(point))
            .map(|chip| chip.id)
    }

    fn occupied_right(self, fallback: f32) -> f32 {
        [self.project, self.location, self.branch]
            .into_iter()
            .flatten()
            .map(|chip| chip.rect.max_x())
            .fold(fallback, f32::max)
    }
}

pub fn layout(
    rect: Rect,
    workspace_label: Option<&str>,
    connection_label: Option<&str>,
    branch_label: Option<&str>,
    interactive: bool,
    detachable_project: bool,
) -> ComposerContextLayout {
    let mut result = ComposerContextLayout {
        project: None,
        location: None,
        branch: None,
        detach: None,
    };
    if !interactive || rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return result;
    }

    let top = rect.origin.y + (rect.size.y - CHIP_HEIGHT.min(rect.size.y)).max(0.0) / 2.0;
    let height = CHIP_HEIGHT.min(rect.size.y.max(0.0));
    let right = (rect.max_x() - CHIP_INSET_X).max(rect.origin.x);
    let mut x = (rect.origin.x + CHIP_INSET_X).min(right);

    if let Some(label) = non_empty(workspace_label) {
        let detach_width = if detachable_project {
            DETACH_HIT_SIZE + 4.0
        } else {
            CHIP_PAD_X + CONTEXT_ICON_SIZE + CONTEXT_ICON_GAP
        };
        let desired = detach_width
            + estimated_text_width(label).min(MAX_CHIP_LABEL_WIDTH)
            + CHIP_LABEL_WIDTH_SLOP
            + CHIP_PAD_X;
        if let Some(chip_rect) =
            take_chip(&mut x, right, top, height, desired, PROJECT_CHIP_MIN_WIDTH)
        {
            let detach = detachable_project.then(|| {
                Rect::xywh(
                    chip_rect.origin.x + 2.0,
                    chip_rect.origin.y + (chip_rect.size.y - DETACH_HIT_SIZE).max(0.0) / 2.0,
                    DETACH_HIT_SIZE.min((chip_rect.size.x - 2.0).max(0.0)),
                    DETACH_HIT_SIZE.min(chip_rect.size.y.max(0.0)),
                )
            });
            let leading_right = detach.map_or(
                chip_rect.origin.x + CHIP_PAD_X + CONTEXT_ICON_SIZE + CONTEXT_ICON_GAP,
                |detach| detach.max_x() + PROJECT_LABEL_GAP,
            );
            let label_rect = Rect::xywh(
                leading_right.min(chip_rect.max_x()),
                chip_rect.origin.y,
                (chip_rect.max_x() - CHIP_PAD_X - leading_right).max(0.0),
                chip_rect.size.y,
            );
            let action_left = detach.map_or(chip_rect.origin.x, |rect| rect.max_x());
            let action_rect = Rect::xywh(
                action_left.min(chip_rect.max_x()),
                chip_rect.origin.y,
                (chip_rect.max_x() - action_left).max(0.0),
                chip_rect.size.y,
            );
            result.detach = detach;
            result.project = Some(ComposerContextChipLayout {
                id: COMPOSER_PROJECT_ID,
                rect: chip_rect,
                action_rect,
                label_rect,
            });
        }
    }

    result.location = non_empty(connection_label).and_then(|label| {
        take_standard_chip(&mut x, right, top, height, COMPOSER_LOCATION_ID, label)
    });
    result.branch = non_empty(branch_label).and_then(|label| {
        take_standard_chip(&mut x, right, top, height, COMPOSER_BRANCH_ID, label)
    });
    result
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
        false,
        None,
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
    interactive: bool,
    detachable_project: bool,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    menu_active: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return;
    }
    let surface = Rect::xywh(
        rect.origin.x,
        rect.origin.y,
        rect.size.x,
        rect.size.y + COMPOSER_CONTEXT_OVERLAP,
    );
    painter.fill_round_rect(surface, CONTEXT_RADIUS, theme.tokens.muted);
    let square_tail_height = CONTEXT_RADIUS
        .min(surface.size.x / 2.0)
        .min(surface.size.y / 2.0);
    painter.fill_rect(
        Rect::xywh(
            surface.origin.x,
            surface.max_y() - square_tail_height,
            surface.size.x,
            square_tail_height,
        ),
        theme.tokens.muted,
    );
    painter.save();
    painter.clip_rect(rect);
    let context_layout = layout(
        rect,
        workspace_label,
        connection_label,
        branch,
        interactive,
        detachable_project && workspace_label.is_some(),
    );
    let occupied_right = if interactive {
        paint_chips(
            painter,
            context_layout,
            workspace_label,
            connection_label,
            branch,
            focused,
            hovered,
            menu_active,
            theme,
        );
        context_layout.occupied_right(rect.origin.x + CHIP_INSET_X)
    } else {
        paint_static_items(
            painter,
            rect,
            workspace_label,
            connection_label,
            branch,
            theme,
        )
    };
    if let Some(goal) = goal {
        let label = format!("{} · {} / {}", goal.title, goal.completed, goal.total);
        if let Some(slot) = goal_slot(rect, occupied_right) {
            paint_single_line(
                painter,
                &label,
                slot,
                12.0,
                500,
                theme.zode_purple,
                HorizontalAlign::End,
            );
        }
    }
    painter.restore();
    paint_active_tooltip(painter, rect, context_layout, focused, hovered, theme);
}

#[allow(clippy::too_many_arguments)]
fn paint_chips(
    painter: &mut dyn Painter,
    layout: ComposerContextLayout,
    workspace_label: Option<&str>,
    connection_label: Option<&str>,
    branch_label: Option<&str>,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    menu_active: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if let (Some(chip), Some(label)) = (layout.project, non_empty(workspace_label)) {
        let detach_active = is_active(PROJECT_DETACH_ID, focused, hovered, menu_active);
        let project_active = is_active(chip.id, focused, hovered, menu_active);
        let project_hovered = hovered == Some(chip.id) || hovered == Some(PROJECT_DETACH_ID);
        if project_active || detach_active {
            painter.fill_round_rect(
                chip.rect,
                CHIP_RADIUS.min(chip.rect.size.y / 2.0),
                theme.tokens.accent,
            );
        }
        if let Some(detach) = layout.detach {
            if project_hovered || focused == Some(PROJECT_DETACH_ID) {
                let visual = centered_square(detach, DETACH_VISUAL_SIZE);
                painter.fill_round_rect(
                    visual,
                    DETACH_VISUAL_SIZE / 2.0,
                    theme.tokens.muted_foreground.with_alpha(if detach_active {
                        0.2
                    } else {
                        0.12
                    }),
                );
                paint_icon(
                    painter,
                    SemanticIcon::Close,
                    centered_square(visual, 12.0),
                    if detach_active {
                        theme.tokens.foreground
                    } else {
                        theme.tokens.muted_foreground
                    },
                );
            } else {
                paint_icon(
                    painter,
                    SemanticIcon::Folder,
                    centered_square(detach, CONTEXT_ICON_SIZE),
                    theme.tokens.foreground,
                );
            }
        } else {
            let icon = Rect::xywh(
                chip.rect.origin.x + CHIP_PAD_X,
                chip.rect.origin.y + (chip.rect.size.y - CONTEXT_ICON_SIZE) / 2.0,
                CONTEXT_ICON_SIZE,
                CONTEXT_ICON_SIZE,
            );
            paint_icon(painter, SemanticIcon::Folder, icon, theme.tokens.foreground);
        }
        paint_chip_label(painter, label, chip.label_rect, theme);
    }
    for (chip, label, icon) in [
        (
            layout.location,
            non_empty(connection_label),
            SemanticIcon::Host,
        ),
        (layout.branch, non_empty(branch_label), SemanticIcon::Branch),
    ] {
        let (Some(chip), Some(label)) = (chip, label) else {
            continue;
        };
        if is_active(chip.id, focused, hovered, menu_active) {
            painter.fill_round_rect(
                chip.rect,
                CHIP_RADIUS.min(chip.rect.size.y / 2.0),
                theme.tokens.accent,
            );
        }
        let icon_rect = Rect::xywh(
            chip.rect.origin.x + CHIP_PAD_X,
            chip.rect.origin.y + (chip.rect.size.y - CONTEXT_ICON_SIZE) / 2.0,
            CONTEXT_ICON_SIZE,
            CONTEXT_ICON_SIZE,
        );
        paint_icon(painter, icon, icon_rect, theme.tokens.foreground);
        paint_chip_label(painter, label, chip.label_rect, theme);
    }
}

fn paint_static_items(
    painter: &mut dyn Painter,
    rect: Rect,
    workspace_label: Option<&str>,
    connection_label: Option<&str>,
    branch_label: Option<&str>,
    theme: &ZodeTheme,
) -> f32 {
    let mut x = rect.origin.x + CONTEXT_INSET_X;
    for (icon, label) in [
        (SemanticIcon::Folder, workspace_label),
        (SemanticIcon::Host, connection_label),
        (SemanticIcon::Branch, branch_label),
    ]
    .into_iter()
    .filter_map(|(icon, label)| non_empty(label).map(|label| (icon, label)))
    {
        let icon_rect = Rect::xywh(
            x,
            rect.origin.y + (rect.size.y - CONTEXT_ICON_SIZE).max(0.0) / 2.0,
            CONTEXT_ICON_SIZE,
            CONTEXT_ICON_SIZE,
        );
        paint_icon(painter, icon, icon_rect, theme.tokens.foreground);
        x += CONTEXT_ICON_SIZE + CONTEXT_ICON_GAP;
        let label_width = painter.measure_text_weighted(label, CONTEXT_FONT_SIZE, 400);
        paint_single_line(
            painter,
            label,
            Rect::xywh(x, rect.origin.y, label_width, rect.size.y),
            CONTEXT_FONT_SIZE,
            400,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        x += label_width + CONTEXT_ITEM_GAP;
        if x >= rect.max_x() - CONTEXT_INSET_X {
            break;
        }
    }
    x
}

fn paint_active_tooltip(
    painter: &mut dyn Painter,
    rail: Rect,
    layout: ComposerContextLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    let active = hovered.or(focused);
    let tooltip = if active == Some(PROJECT_DETACH_ID) {
        layout.detach.map(|rect| (rect, "不在项目中工作"))
    } else {
        [
            (
                layout.project,
                if layout.detach.is_some() {
                    "更改此任务的项目"
                } else {
                    "选择项目"
                },
            ),
            (layout.location, "选择任务的运行位置"),
            (layout.branch, "切换任务分支"),
        ]
        .into_iter()
        .find_map(|(chip, label)| {
            chip.filter(|chip| active == Some(chip.id))
                .map(|chip| (chip.rect, label))
        })
    };
    let Some((anchor, label)) = tooltip else {
        return;
    };
    let width = (painter.measure_text_weighted(label, 12.0, 400) + 16.0)
        .clamp(80.0, 200.0)
        .min(rail.size.x);
    let x = (anchor.origin.x + anchor.size.x / 2.0 - width / 2.0)
        .clamp(rail.origin.x, (rail.max_x() - width).max(rail.origin.x));
    Tooltip { label }.paint(
        painter,
        Rect::xywh(
            x,
            anchor.origin.y - TOOLTIP_GAP - TOOLTIP_H,
            width,
            TOOLTIP_H,
        ),
        &theme.tokens,
    );
}

fn paint_chip_label(painter: &mut dyn Painter, label: &str, rect: Rect, theme: &ZodeTheme) {
    if rect.size.x <= 0.0 {
        return;
    }
    let visible = end_ellipsize(painter, label, rect.size.x, CONTEXT_FONT_SIZE, 400);
    painter.save();
    painter.clip_rect(rect);
    paint_single_line(
        painter,
        &visible,
        rect,
        CONTEXT_FONT_SIZE,
        400,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    painter.restore();
}

fn paint_icon(
    painter: &mut dyn Painter,
    icon: SemanticIcon,
    rect: Rect,
    color: jian_widgets::Color,
) {
    painter.stroke_svg_path(
        icon.path(),
        rect.origin,
        rect.size.x,
        color,
        icon.stroke_width(),
    );
}

fn centered_square(rect: Rect, side: f32) -> Rect {
    let side = side.min(rect.size.x).min(rect.size.y).max(0.0);
    Rect::xywh(
        rect.origin.x + (rect.size.x - side) / 2.0,
        rect.origin.y + (rect.size.y - side) / 2.0,
        side,
        side,
    )
}

fn is_active(
    id: WidgetId,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    menu_active: Option<WidgetId>,
) -> bool {
    focused == Some(id) || hovered == Some(id) || menu_active == Some(id)
}

fn take_standard_chip(
    x: &mut f32,
    right: f32,
    top: f32,
    height: f32,
    id: WidgetId,
    label: &str,
) -> Option<ComposerContextChipLayout> {
    let desired = CHIP_PAD_X * 2.0
        + CONTEXT_ICON_SIZE
        + CONTEXT_ICON_GAP
        + estimated_text_width(label).min(MAX_CHIP_LABEL_WIDTH)
        + CHIP_LABEL_WIDTH_SLOP;
    let rect = take_chip(x, right, top, height, desired, CHIP_MIN_WIDTH)?;
    let label_left = rect.origin.x + CHIP_PAD_X + CONTEXT_ICON_SIZE + CONTEXT_ICON_GAP;
    Some(ComposerContextChipLayout {
        id,
        rect,
        action_rect: rect,
        label_rect: Rect::xywh(
            label_left.min(rect.max_x()),
            rect.origin.y,
            (rect.max_x() - CHIP_PAD_X - label_left).max(0.0),
            rect.size.y,
        ),
    })
}

fn take_chip(
    x: &mut f32,
    right: f32,
    top: f32,
    height: f32,
    desired_width: f32,
    minimum_width: f32,
) -> Option<Rect> {
    let remaining = (right - *x).max(0.0);
    if remaining < minimum_width || height <= 0.0 {
        *x = right;
        return None;
    }
    let width = desired_width.max(minimum_width).min(remaining);
    let rect = Rect::xywh(*x, top, width, height);
    *x = if width + f32::EPSILON < desired_width {
        right
    } else {
        rect.max_x() + CHIP_GAP
    };
    Some(rect)
}

fn estimated_text_width(label: &str) -> f32 {
    label
        .chars()
        .map(|character| {
            if character.is_ascii() {
                CONTEXT_FONT_SIZE * 0.55
            } else {
                CONTEXT_FONT_SIZE
            }
        })
        .sum()
}

fn non_empty(label: Option<&str>) -> Option<&str> {
    label.filter(|label| !label.trim().is_empty())
}

fn end_ellipsize(
    painter: &mut dyn Painter,
    text: &str,
    max_width: f32,
    font_size: f32,
    weight: u16,
) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if painter.measure_text_weighted(text, font_size, weight) <= max_width {
        return text.to_owned();
    }
    let ellipsis = "…";
    let ellipsis_width = painter.measure_text_weighted(ellipsis, font_size, weight);
    if ellipsis_width > max_width {
        return String::new();
    }
    let mut end = text.len();
    while end > 0 {
        end = text[..end]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index);
        let candidate = format!("{}…", &text[..end]);
        if painter.measure_text_weighted(&candidate, font_size, weight) <= max_width {
            return candidate;
        }
    }
    ellipsis.to_owned()
}

fn goal_slot(rect: Rect, occupied_right: f32) -> Option<Rect> {
    let right = (rect.max_x() - CONTEXT_INSET_X).max(rect.origin.x);
    let desired_width = (rect.size.x * 0.42).clamp(120.0, 300.0);
    let left = (right - desired_width)
        .max(occupied_right + 8.0)
        .max(rect.origin.x + CONTEXT_INSET_X);
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
