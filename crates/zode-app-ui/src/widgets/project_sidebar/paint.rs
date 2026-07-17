use std::time::{SystemTime, UNIX_EPOCH};

use jian_widgets::{components::tooltip::Tooltip, HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::{SidebarSectionMenu, ZodeAppState};
use zode_node_protocol::ThreadStatus;

use super::layout::{ICON_SIZE, ROW_H};
use super::{
    navigation_item_selected, ProjectSidebar, SidebarControlLayout, SidebarControlTarget,
    SidebarRowLayout, SidebarRowTarget, SidebarSection, SIDEBAR_PROJECTS_MORE_ID,
    SIDEBAR_PROJECTS_NEW_ID, SIDEBAR_PROJECTS_SECTION_ID, SIDEBAR_TASKS_MORE_ID,
    SIDEBAR_TASKS_NEW_ID, SIDEBAR_TASKS_SECTION_ID, SIDEBAR_TASKS_TOGGLE_ID,
};
use crate::{
    paint_elevated_surface, paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme,
};

pub(super) fn paint(
    painter: &mut dyn Painter,
    rect: Rect,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    show_shortcuts: bool,
    theme: &ZodeTheme,
) {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return;
    }
    let layout = ProjectSidebar::layout(rect, state);
    super::chrome::paint(painter, &layout, state, focused, hovered, theme);

    if let Some(new_task) = layout.navigation_rows.first() {
        paint_navigation_row(
            painter,
            new_task,
            layout.compact,
            state,
            focused,
            hovered,
            theme,
        );
    }

    painter.save();
    painter.clip_rect(layout.scroll_viewport);
    for row in layout.navigation_rows.iter().skip(1) {
        paint_navigation_row(painter, row, layout.compact, state, focused, hovered, theme);
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
                14.0,
                400,
                theme.sidebar_disabled_foreground,
            );
        }
        for row in &layout.rows {
            paint_dynamic_row(painter, row, state, focused, hovered, show_shortcuts, theme);
        }
        for control in &layout.controls {
            paint_control(painter, control, state, focused, hovered, theme);
        }
    }
    painter.restore();

    super::footer::paint_footer(painter, &layout, state, focused, hovered, theme);
}

fn paint_navigation_row(
    painter: &mut dyn Painter,
    row: &super::SidebarNavigationRowLayout,
    compact: bool,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    let selected = navigation_item_selected(state, row.item);
    let interactive = focused == Some(navigation_widget_id(row.index))
        || hovered == Some(navigation_widget_id(row.index));
    if selected {
        painter.fill_round_rect(row.rect, 8.0, theme.sidebar_row_selected);
    } else if interactive {
        painter.fill_round_rect(row.rect, 8.0, theme.sidebar_row_hover);
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
            (row.rect.size.x
                - if row.index == 0 && interactive {
                    72.0
                } else {
                    40.0
                })
            .max(0.0),
            row.rect.size.y,
        ),
        14.0,
        400,
        theme.sidebar_foreground,
    );
    if row.index == 0 && interactive {
        let keycap = Rect::xywh(
            row.rect.max_x() - 43.0,
            row.rect.origin.y + (row.rect.size.y - 20.0) / 2.0,
            35.0,
            20.0,
        );
        painter.fill_round_rect(keycap, 10.0, theme.tokens.muted.with_alpha(0.78));
        paint_single_line(
            painter,
            "⌘N",
            keycap,
            10.0,
            400,
            theme.sidebar_muted_foreground,
            HorizontalAlign::Center,
        );
    }
}

fn navigation_widget_id(index: usize) -> WidgetId {
    WidgetId(2 + index as u64)
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
        14.0,
        400,
        theme.sidebar_muted_foreground,
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
            theme.sidebar_muted_foreground,
            chevron.stroke_width(),
        );
    }
}

fn paint_dynamic_row(
    painter: &mut dyn Painter,
    row: &SidebarRowLayout,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    show_shortcuts: bool,
    theme: &ZodeTheme,
) {
    let action_active = row_interaction_active(row, focused, hovered)
        || matches!(&row.target, SidebarRowTarget::Project(workspace) if state.sidebar.project_menu.as_ref() == Some(workspace));
    if row.selected {
        painter.fill_round_rect(row.rect, 8.0, theme.sidebar_row_selected);
    } else if hovered == Some(row.id) || action_active {
        painter.fill_round_rect(row.rect, 8.0, theme.sidebar_row_hover);
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
        14.0,
        400,
        theme.sidebar_foreground,
    );

    if action_active {
        paint_row_actions(painter, row, focused, hovered, theme);
    } else {
        paint_row_trailing(painter, row, show_shortcuts, theme);
    }
}

fn paint_row_trailing(
    painter: &mut dyn Painter,
    row: &SidebarRowLayout,
    show_shortcuts: bool,
    theme: &ZodeTheme,
) {
    if show_shortcuts {
        if let Some(shortcut) = row.shortcut {
            paint_single_line(
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
                theme.sidebar_muted_foreground,
                HorizontalAlign::Center,
            );
            return;
        }
    }
    match row.status {
        Some(ThreadStatus::Running) => painter.stroke_svg_path(
            SemanticIcon::Refresh.path(),
            Point2D::new(row.rect.max_x() - 19.0, row.rect.origin.y + 8.0),
            13.0,
            theme.sidebar_muted_foreground,
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
        Some(ThreadStatus::Idle) | None => {}
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

fn paint_row_actions(
    painter: &mut dyn Painter,
    row: &SidebarRowLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if row.session().is_some() {
        paint_session_actions(painter, row, focused, hovered, theme);
        return;
    }
    if let (Some(id), Some(rect)) = (row.more_id, ProjectSidebar::project_more_rect(row)) {
        paint_icon_button(
            painter,
            rect,
            SemanticIcon::More,
            focused == Some(id),
            hovered == Some(id),
            theme,
        );
    }
    if let (Some(id), Some(rect)) = (row.new_id, ProjectSidebar::project_new_rect(row)) {
        paint_icon_button(
            painter,
            rect,
            SemanticIcon::NewTask,
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
        SidebarControlTarget::ProjectsMenu
        | SidebarControlTarget::NewProject
        | SidebarControlTarget::TasksMenu
        | SidebarControlTarget::NewProjectlessTask => {
            let projects = matches!(
                control.target,
                SidebarControlTarget::ProjectsMenu | SidebarControlTarget::NewProject
            );
            let active = if projects {
                state.sidebar.section_menu == Some(SidebarSectionMenu::Projects)
                    || [
                        SIDEBAR_PROJECTS_SECTION_ID,
                        SIDEBAR_PROJECTS_MORE_ID,
                        SIDEBAR_PROJECTS_NEW_ID,
                    ]
                    .into_iter()
                    .any(|id| focused == Some(id) || hovered == Some(id))
            } else {
                state.sidebar.section_menu == Some(SidebarSectionMenu::Tasks)
                    || [
                        SIDEBAR_TASKS_TOGGLE_ID,
                        SIDEBAR_TASKS_SECTION_ID,
                        SIDEBAR_TASKS_MORE_ID,
                        SIDEBAR_TASKS_NEW_ID,
                    ]
                    .into_iter()
                    .any(|id| focused == Some(id) || hovered == Some(id))
            };
            if !active {
                return;
            }
            paint_icon_button(
                painter,
                control.rect,
                if matches!(
                    control.target,
                    SidebarControlTarget::ProjectsMenu | SidebarControlTarget::TasksMenu
                ) {
                    SemanticIcon::More
                } else {
                    SemanticIcon::Plus
                },
                focused == Some(control.id),
                hovered == Some(control.id),
                theme,
            );
        }
        SidebarControlTarget::ToggleTasks => {}
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
                14.0,
                400,
                theme.sidebar_muted_foreground,
            );
        }
    }
}

pub(super) fn paint_icon_button(
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
    let size = 14.0;
    painter.stroke_svg_path(
        icon.path(),
        Point2D::new(
            rect.origin.x + (rect.size.x - size) / 2.0,
            rect.origin.y + (rect.size.y - size) / 2.0,
        ),
        size,
        theme.sidebar_muted_foreground,
        icon.stroke_width(),
    );
}

fn row_interaction_active(
    row: &SidebarRowLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
) -> bool {
    [
        Some(row.id),
        row.pin_id,
        row.archive_id,
        row.more_id,
        row.new_id,
    ]
    .into_iter()
    .flatten()
    .any(|id| focused == Some(id) || hovered == Some(id))
}

pub(super) fn paint_hover_overlay(
    painter: &mut dyn Painter,
    sidebar: Rect,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if let Some(menu) = super::menu::layout(sidebar, state) {
        super::menu::paint(painter, &menu, focused, hovered, theme);
        return;
    }
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
    let branch = verified_branch(state, row);
    let card_h = if branch.is_some() { 88.0 } else { 68.0 };
    let min_y = sidebar.origin.y + 8.0;
    let max_y = (sidebar.max_y() - card_h - 8.0).max(min_y);
    let card = Rect::xywh(
        sidebar.max_x() + 8.0,
        row.rect.origin.y.clamp(min_y, max_y),
        226.0,
        card_h,
    );
    paint_elevated_surface(painter, card, 12.0, theme);
    painter.fill_round_rect(card, 12.0, theme.tokens.popover);
    painter.stroke_round_rect(card, 12.0, theme.tokens.border, 1.0);

    let updated_at_ms = row
        .session()
        .and_then(|session| {
            state
                .threads
                .iter()
                .find(|thread| &thread.session == session)
        })
        .map_or(0, |thread| thread.updated_at_ms);
    let age = relative_age_label(updated_at_ms, current_time_ms());
    let age_width = painter
        .measure_text_weighted(&age, 12.0, 400)
        .clamp(32.0, 56.0);
    let age_rect = Rect::xywh(
        card.max_x() - 14.0 - age_width,
        card.origin.y + 5.0,
        age_width,
        26.0,
    );
    let title_rect = Rect::xywh(
        card.origin.x + 14.0,
        card.origin.y + 5.0,
        (age_rect.origin.x - card.origin.x - 22.0).max(0.0),
        26.0,
    );
    let title = end_ellipsize(painter, &row.label, title_rect.size.x, 14.0, 500);
    draw_label(
        painter,
        &title,
        title_rect,
        14.0,
        500,
        theme.tokens.popover_foreground,
    );
    paint_single_line(
        painter,
        &age,
        age_rect,
        12.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::End,
    );

    let workspace = if state.is_projectless_workspace(workspace) {
        "不在项目中工作".into()
    } else {
        super::workspace_label(workspace, true)
    };
    paint_metadata_row(
        painter,
        SemanticIcon::Folder,
        &workspace,
        Rect::xywh(card.origin.x + 14.0, card.origin.y + 32.0, 198.0, 22.0),
        theme,
    );
    if let Some(branch) = branch {
        paint_metadata_row(
            painter,
            SemanticIcon::Branch,
            &branch,
            Rect::xywh(card.origin.x + 14.0, card.origin.y + 54.0, 198.0, 22.0),
            theme,
        );
    }

    paint_session_action_tooltip(painter, sidebar, row, hovered, theme);
}

fn verified_branch(state: &ZodeAppState, row: &SidebarRowLayout) -> Option<String> {
    let session = row.session()?;
    let workspace = row.workspace_uri.as_ref()?;
    let context = state.presentation.sessions.get(session)?.context.ready()?;
    (&context.workspace_uri == workspace)
        .then_some(context.branch.as_deref())
        .flatten()
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(str::to_owned)
}

fn paint_metadata_row(
    painter: &mut dyn Painter,
    icon: SemanticIcon,
    label: &str,
    rect: Rect,
    theme: &ZodeTheme,
) {
    let icon_size = 13.0;
    painter.stroke_svg_path(
        icon.path(),
        Point2D::new(
            rect.origin.x,
            rect.origin.y + (rect.size.y - icon_size) / 2.0,
        ),
        icon_size,
        theme.tokens.muted_foreground,
        icon.stroke_width(),
    );
    let label_rect = Rect::xywh(
        rect.origin.x + 21.0,
        rect.origin.y,
        (rect.size.x - 21.0).max(0.0),
        rect.size.y,
    );
    let visible = end_ellipsize(painter, label, label_rect.size.x, 12.0, 400);
    draw_label(
        painter,
        &visible,
        label_rect,
        12.0,
        400,
        theme.tokens.popover_foreground,
    );
}

fn paint_session_action_tooltip(
    painter: &mut dyn Painter,
    sidebar: Rect,
    row: &SidebarRowLayout,
    hovered: WidgetId,
    theme: &ZodeTheme,
) {
    let (anchor, label) = if row.pin_id == Some(hovered) {
        let Some(anchor) = ProjectSidebar::session_pin_rect(row) else {
            return;
        };
        (
            anchor,
            if row.pinned {
                "取消置顶"
            } else {
                "置顶任务"
            },
        )
    } else if row.archive_id == Some(hovered) {
        let Some(anchor) = ProjectSidebar::session_archive_rect(row) else {
            return;
        };
        (anchor, "归档任务")
    } else {
        return;
    };
    let height = 28.0;
    let width = (painter.measure_text_weighted(label, 12.0, 400) + 16.0).clamp(60.0, 88.0);
    let min_x = sidebar.origin.x + 4.0;
    let max_x = (sidebar.max_x() - width - 4.0).max(min_x);
    let tooltip = Rect::xywh(
        (anchor.origin.x + anchor.size.x / 2.0 - width / 2.0).clamp(min_x, max_x),
        (anchor.origin.y - height - 6.0).max(sidebar.origin.y + 4.0),
        width,
        height,
    );
    paint_elevated_surface(painter, tooltip, 6.0, theme);
    Tooltip { label }.paint(painter, tooltip, &theme.tokens);
}

fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

fn relative_age_label(updated_at_ms: i64, now_ms: i64) -> String {
    if updated_at_ms <= 0 || updated_at_ms >= now_ms {
        return "现在".into();
    }
    let elapsed_ms = now_ms.saturating_sub(updated_at_ms);
    const HOUR_MS: i64 = 60 * 60 * 1_000;
    const DAY_MS: i64 = 24 * HOUR_MS;
    if elapsed_ms >= DAY_MS {
        format!("{} 天", elapsed_ms / DAY_MS)
    } else if elapsed_ms >= HOUR_MS {
        format!("{} 小时", elapsed_ms / HOUR_MS)
    } else {
        "现在".into()
    }
}

fn end_ellipsize(
    painter: &mut dyn Painter,
    value: &str,
    width: f32,
    font_size: f32,
    weight: u16,
) -> String {
    const ELLIPSIS: &str = "…";
    if width <= 0.0 {
        return String::new();
    }
    if painter.measure_text_weighted(value, font_size, weight) <= width {
        return value.to_owned();
    }
    if painter.measure_text_weighted(ELLIPSIS, font_size, weight) > width {
        return String::new();
    }
    let mut boundaries = value
        .char_indices()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    boundaries.push(value.len());
    let mut lower = 0usize;
    let mut upper = boundaries.len() - 1;
    while lower < upper {
        let middle = (lower + upper).div_ceil(2);
        let prefix = value[..boundaries[middle]].trim_end();
        let candidate = format!("{prefix}{ELLIPSIS}");
        if painter.measure_text_weighted(&candidate, font_size, weight) <= width {
            lower = middle;
        } else {
            upper = middle - 1;
        }
    }
    format!("{}{}", value[..boundaries[lower]].trim_end(), ELLIPSIS)
}

pub(super) fn draw_label(
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
