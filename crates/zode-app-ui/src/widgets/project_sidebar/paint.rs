use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::ZodeAppState;
use zode_node_protocol::ThreadStatus;

use super::layout::{ICON_SIZE, ROW_H};
use super::{
    navigation_item_selected, ProjectSidebar, SidebarControlLayout, SidebarControlTarget,
    SidebarRowLayout, SidebarRowTarget, SidebarSection, SIDEBAR_TASKS_NEW_ID,
    SIDEBAR_TASKS_TOGGLE_ID,
};
use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme, HELP_ID};

pub(super) fn paint(
    painter: &mut dyn Painter,
    rect: Rect,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return;
    }
    let layout = ProjectSidebar::layout(rect, state);
    paint_brand(painter, layout.brand, layout.compact, theme);

    if let Some(new_task) = layout.navigation_rows.first() {
        paint_navigation_row(painter, new_task, layout.compact, state, theme);
    }

    painter.save();
    painter.clip_rect(layout.scroll_viewport);
    for row in layout.navigation_rows.iter().skip(1) {
        paint_navigation_row(painter, row, layout.compact, state, theme);
    }
    if !layout.compact {
        for section in &layout.sections {
            paint_section(painter, section, state, theme);
        }
        for label in &layout.labels {
            draw_label(
                painter,
                label.label,
                Rect::xywh(
                    label.rect.origin.x + if label.nested { 8.0 } else { 0.0 },
                    label.rect.origin.y,
                    label.rect.size.x,
                    label.rect.size.y,
                ),
                12.0,
                400,
                theme.tokens.muted_foreground,
            );
        }
        for row in &layout.rows {
            paint_dynamic_row(painter, row, focused, hovered, theme);
        }
        for control in &layout.controls {
            paint_control(painter, control, state, focused, hovered, theme);
        }
    }
    painter.restore();

    paint_footer(painter, &layout, state, focused, hovered, theme);
}

fn paint_brand(painter: &mut dyn Painter, rect: Rect, compact: bool, theme: &ZodeTheme) {
    draw_label(
        painter,
        if compact { "Z" } else { "Zode" },
        rect,
        if compact { 14.0 } else { 17.0 },
        600,
        theme.sidebar_foreground,
    );
}

fn paint_navigation_row(
    painter: &mut dyn Painter,
    row: &super::SidebarNavigationRowLayout,
    compact: bool,
    state: &ZodeAppState,
    theme: &ZodeTheme,
) {
    let selected = navigation_item_selected(state, row.item);
    if selected {
        painter.fill_round_rect(row.rect, 8.0, theme.tokens.row_selected);
    }
    let icon_x = if compact {
        row.rect.origin.x + (row.rect.size.x - ICON_SIZE) / 2.0
    } else {
        row.rect.origin.x + 8.0
    };
    painter.stroke_svg_path(
        row.item.icon.path(),
        Point2D::new(
            icon_x,
            row.rect.origin.y + (row.rect.size.y - ICON_SIZE) / 2.0,
        ),
        ICON_SIZE,
        theme.sidebar_foreground,
        row.item.icon.stroke_width(),
    );
    if compact {
        return;
    }
    draw_label(
        painter,
        row.item.label,
        Rect::xywh(
            row.rect.origin.x + 32.0,
            row.rect.origin.y,
            (row.rect.size.x - 40.0).max(0.0),
            row.rect.size.y,
        ),
        13.0,
        if selected { 500 } else { 400 },
        theme.sidebar_foreground,
    );
}

fn paint_section(
    painter: &mut dyn Painter,
    section: &super::SidebarSectionLayout,
    state: &ZodeAppState,
    theme: &ZodeTheme,
) {
    draw_label(
        painter,
        section.label,
        section.rect,
        11.0,
        500,
        theme.tokens.muted_foreground,
    );
    if section.section == SidebarSection::Tasks {
        let chevron = if state.sidebar.tasks_expanded {
            SemanticIcon::ChevronDown
        } else {
            SemanticIcon::ChevronRight
        };
        painter.stroke_svg_path(
            chevron.path(),
            Point2D::new(
                section.rect.origin.x + 32.0,
                section.rect.origin.y + (section.rect.size.y - 12.0) / 2.0,
            ),
            12.0,
            theme.tokens.muted_foreground,
            chevron.stroke_width(),
        );
    }
}

fn paint_dynamic_row(
    painter: &mut dyn Painter,
    row: &SidebarRowLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    let action_active = row_interaction_active(row, focused, hovered);
    if row.selected {
        painter.fill_round_rect(row.rect, 8.0, theme.tokens.row_selected);
    } else if hovered == Some(row.id) {
        painter.fill_round_rect(row.rect, 8.0, theme.tokens.muted.with_alpha(0.72));
    }

    let label_x = match row.target {
        SidebarRowTarget::Project(_) => {
            painter.stroke_svg_path(
                SemanticIcon::Folder.path(),
                Point2D::new(
                    row.rect.origin.x + 8.0,
                    row.rect.origin.y + (ROW_H - ICON_SIZE) / 2.0,
                ),
                ICON_SIZE,
                theme.sidebar_foreground,
                SemanticIcon::Folder.stroke_width(),
            );
            row.rect.origin.x + 32.0
        }
        SidebarRowTarget::Task(_) | SidebarRowTarget::Session(_) => row.rect.origin.x + 8.0,
    };
    let trailing = if action_active { 54.0 } else { 38.0 };
    draw_label(
        painter,
        &row.label,
        Rect::xywh(
            label_x,
            row.rect.origin.y,
            (row.rect.max_x() - label_x - trailing).max(0.0),
            row.rect.size.y,
        ),
        12.0,
        if row.selected { 500 } else { 400 },
        theme.sidebar_foreground,
    );

    if action_active {
        paint_session_actions(painter, row, focused, hovered, theme);
    } else {
        paint_row_trailing(painter, row, theme);
    }
}

fn paint_row_trailing(painter: &mut dyn Painter, row: &SidebarRowLayout, theme: &ZodeTheme) {
    match row.status {
        Some(ThreadStatus::Running) => painter.stroke_svg_path(
            SemanticIcon::Refresh.path(),
            Point2D::new(row.rect.max_x() - 19.0, row.rect.origin.y + 8.0),
            13.0,
            theme.tokens.muted_foreground,
            SemanticIcon::Refresh.stroke_width(),
        ),
        Some(ThreadStatus::Failed) => draw_label(
            painter,
            "!",
            Rect::xywh(
                row.rect.max_x() - 22.0,
                row.rect.origin.y,
                14.0,
                row.rect.size.y,
            ),
            12.0,
            700,
            theme.tokens.destructive,
        ),
        Some(ThreadStatus::Idle) | None => {
            if let Some(shortcut) = row.shortcut {
                draw_label(
                    painter,
                    &format!("⌘{shortcut}"),
                    Rect::xywh(
                        row.rect.max_x() - 30.0,
                        row.rect.origin.y,
                        24.0,
                        row.rect.size.y,
                    ),
                    10.0,
                    400,
                    theme.tokens.muted_foreground,
                );
            }
        }
    }
}

fn paint_session_actions(
    painter: &mut dyn Painter,
    row: &SidebarRowLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if let (Some(id), Some(rect)) = (row.pin_id, ProjectSidebar::session_pin_rect(row)) {
        paint_icon_button(
            painter,
            rect,
            SemanticIcon::Pin,
            focused == Some(id),
            hovered == Some(id),
            theme,
        );
    }
    if let (Some(id), Some(rect)) = (row.archive_id, ProjectSidebar::session_archive_rect(row)) {
        paint_icon_button(
            painter,
            rect,
            SemanticIcon::Archive,
            focused == Some(id),
            hovered == Some(id),
            theme,
        );
    }
}

fn paint_control(
    painter: &mut dyn Painter,
    control: &SidebarControlLayout,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    match control.target {
        SidebarControlTarget::MoreDecoration => paint_icon_button(
            painter,
            control.rect,
            SemanticIcon::More,
            false,
            false,
            theme,
        ),
        SidebarControlTarget::NewProjectlessTask => paint_icon_button(
            painter,
            control.rect,
            SemanticIcon::Edit,
            focused == Some(SIDEBAR_TASKS_NEW_ID),
            hovered == Some(SIDEBAR_TASKS_NEW_ID),
            theme,
        ),
        SidebarControlTarget::ToggleTasks => {
            let _ = state;
            if focused == Some(SIDEBAR_TASKS_TOGGLE_ID) {
                painter.stroke_round_rect(control.rect, 6.0, theme.tokens.ring, 1.5);
            }
        }
        SidebarControlTarget::ShowAllProjects | SidebarControlTarget::ShowAllProjectSessions(_) => {
            if focused == Some(control.id) || hovered == Some(control.id) {
                painter.fill_round_rect(control.rect, 6.0, theme.tokens.muted);
            }
            draw_label(
                painter,
                &control.label,
                Rect::xywh(
                    control.rect.origin.x + 8.0,
                    control.rect.origin.y,
                    control.rect.size.x - 8.0,
                    control.rect.size.y,
                ),
                12.0,
                400,
                theme.tokens.muted_foreground,
            );
        }
    }
}

fn paint_footer(
    painter: &mut dyn Painter,
    layout: &super::SidebarLayout,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if layout.footer.size.y <= 0.0 {
        return;
    }
    painter.fill_rect(
        Rect::xywh(
            layout.footer.origin.x,
            layout.footer.origin.y,
            layout.footer.size.x,
            1.0,
        ),
        theme.tokens.border.with_alpha(0.72),
    );
    if ProjectSidebar::footer_selected(state) {
        painter.fill_round_rect(layout.profile, 0.0, theme.tokens.row_selected);
    }
    if layout.compact {
        paint_avatar(
            painter,
            Rect::xywh(
                layout.profile.origin.x + (layout.profile.size.x - 20.0) / 2.0,
                layout.profile.origin.y + (layout.profile.size.y - 20.0) / 2.0,
                20.0,
                20.0,
            ),
            &state.local_profile.display_name,
            theme,
        );
        paint_icon_button(
            painter,
            layout.help,
            SemanticIcon::Help,
            focused == Some(HELP_ID),
            hovered == Some(HELP_ID),
            theme,
        );
        return;
    }
    let avatar = Rect::xywh(
        layout.profile.origin.x + 16.0,
        layout.profile.origin.y + (layout.profile.size.y - 20.0) / 2.0,
        20.0,
        20.0,
    );
    paint_avatar(painter, avatar, &state.local_profile.display_name, theme);
    draw_label(
        painter,
        state.local_profile.display_name.as_str(),
        Rect::xywh(
            layout.profile.origin.x + 44.0,
            layout.profile.origin.y,
            (layout.profile.size.x - 52.0).max(0.0),
            layout.profile.size.y,
        ),
        12.0,
        500,
        theme.sidebar_foreground,
    );
    paint_icon_button(
        painter,
        layout.help,
        SemanticIcon::Help,
        focused == Some(HELP_ID),
        hovered == Some(HELP_ID),
        theme,
    );
}

fn paint_avatar(painter: &mut dyn Painter, rect: Rect, display_name: &str, theme: &ZodeTheme) {
    painter.fill_round_rect(rect, rect.size.x / 2.0, theme.zode_purple);
    let initial = display_name
        .chars()
        .next()
        .map(|value| value.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "Z".into());
    paint_single_line(
        painter,
        &initial,
        rect,
        10.0,
        600,
        theme.tokens.primary_foreground,
        HorizontalAlign::Center,
    );
}

fn paint_icon_button(
    painter: &mut dyn Painter,
    rect: Rect,
    icon: SemanticIcon,
    focused: bool,
    hovered: bool,
    theme: &ZodeTheme,
) {
    if hovered || focused {
        painter.fill_round_rect(rect, 6.0, theme.tokens.muted);
    }
    if focused {
        painter.stroke_round_rect(rect, 6.0, theme.tokens.ring, 1.25);
    }
    let size = 14.0;
    painter.stroke_svg_path(
        icon.path(),
        Point2D::new(
            rect.origin.x + (rect.size.x - size) / 2.0,
            rect.origin.y + (rect.size.y - size) / 2.0,
        ),
        size,
        theme.tokens.muted_foreground,
        icon.stroke_width(),
    );
}

fn row_interaction_active(
    row: &SidebarRowLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
) -> bool {
    row.session().is_some()
        && [Some(row.id), row.pin_id, row.archive_id]
            .into_iter()
            .flatten()
            .any(|id| focused == Some(id) || hovered == Some(id))
}

pub(super) fn paint_hover_overlay(
    painter: &mut dyn Painter,
    sidebar: Rect,
    state: &ZodeAppState,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    let Some(hovered) = hovered else {
        return;
    };
    let layout = ProjectSidebar::layout(sidebar, state);
    let Some(row) = layout.rows.iter().find(|row| {
        row.session().is_some()
            && (row.id == hovered || row.pin_id == Some(hovered) || row.archive_id == Some(hovered))
    }) else {
        return;
    };
    let Some(workspace) = row.workspace_uri.as_ref() else {
        return;
    };
    let card_h = 82.0;
    let card = Rect::xywh(
        sidebar.max_x() + 8.0,
        row.rect
            .origin
            .y
            .clamp(sidebar.origin.y + 8.0, sidebar.max_y() - card_h - 8.0),
        260.0,
        card_h,
    );
    painter.fill_round_rect(card, 10.0, theme.tokens.popover);
    painter.stroke_round_rect(card, 10.0, theme.tokens.border, 1.0);
    draw_label(
        painter,
        &row.label,
        Rect::xywh(card.origin.x + 14.0, card.origin.y + 8.0, 232.0, 28.0),
        13.0,
        600,
        theme.tokens.popover_foreground,
    );
    let workspace = if state.is_projectless_workspace(workspace) {
        "不在项目中工作".into()
    } else {
        super::workspace_label(workspace, true)
    };
    draw_label(
        painter,
        &workspace,
        Rect::xywh(card.origin.x + 14.0, card.origin.y + 34.0, 232.0, 20.0),
        11.0,
        400,
        theme.tokens.muted_foreground,
    );
    let status = match row.status {
        Some(ThreadStatus::Running) => "运行中",
        Some(ThreadStatus::Failed) => "需要处理",
        Some(ThreadStatus::Idle) | None => "已就绪",
    };
    draw_label(
        painter,
        status,
        Rect::xywh(card.origin.x + 14.0, card.origin.y + 54.0, 232.0, 20.0),
        11.0,
        400,
        if row.status == Some(ThreadStatus::Failed) {
            theme.tokens.destructive
        } else {
            theme.tokens.muted_foreground
        },
    );
}

fn draw_label(
    painter: &mut dyn Painter,
    text: &str,
    rect: Rect,
    size: f32,
    weight: u16,
    color: jian_widgets::Color,
) {
    paint_single_line(
        painter,
        text,
        rect,
        size,
        weight,
        color,
        HorizontalAlign::Start,
    );
}
