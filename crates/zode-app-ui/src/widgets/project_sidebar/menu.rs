use jian_widgets::{components::popover::Popover, HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::{
    AppCommand, ProjectDisplayMode, ProjectSortMode, SidebarSectionMenu, ZodeAppState,
};
use zode_node_protocol::WorkspaceUri;

use super::{ProjectSidebar, SidebarControlTarget, SidebarRowTarget};
use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme};

const MENU_W: f32 = 220.0;
const MENU_PAD: f32 = 5.0;
const ROW_H: f32 = 36.0;
const SEPARATOR_H: f32 = 1.0;
const HEADING_H: f32 = 28.0;

pub const SIDEBAR_PROJECT_MENU_ID: WidgetId = WidgetId(9_200);
pub const SIDEBAR_PROJECT_MENU_PIN_ID: WidgetId = WidgetId(9_201);
pub const SIDEBAR_PROJECT_MENU_FINDER_ID: WidgetId = WidgetId(9_202);
pub const SIDEBAR_PROJECT_MENU_TOGGLE_ID: WidgetId = WidgetId(9_203);
pub const SIDEBAR_PROJECT_MENU_ARCHIVE_ID: WidgetId = WidgetId(9_204);

pub const SIDEBAR_PROJECTS_MENU_ID: WidgetId = WidgetId(9_210);
pub const SIDEBAR_PROJECTS_MENU_GROUPED_ID: WidgetId = WidgetId(9_211);
pub const SIDEBAR_PROJECTS_MENU_FLAT_ID: WidgetId = WidgetId(9_212);
pub const SIDEBAR_PROJECTS_MENU_PRIORITY_ID: WidgetId = WidgetId(9_213);
pub const SIDEBAR_PROJECTS_MENU_RECENT_ID: WidgetId = WidgetId(9_214);
pub const SIDEBAR_PROJECTS_MENU_MANUAL_ID: WidgetId = WidgetId(9_215);

pub const SIDEBAR_TASKS_MENU_ID: WidgetId = WidgetId(9_220);
pub const SIDEBAR_TASKS_MENU_NEW_ID: WidgetId = WidgetId(9_221);
pub const SIDEBAR_TASKS_MENU_TOGGLE_ID: WidgetId = WidgetId(9_222);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMenuKind {
    Project,
    Projects,
    Tasks,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SidebarMenuItemLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub label: String,
    pub icon: Option<SemanticIcon>,
    pub enabled: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SidebarMenuLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub kind: SidebarMenuKind,
    pub workspace_uri: Option<WorkspaceUri>,
    pub items: Vec<SidebarMenuItemLayout>,
    pub separators: Vec<Rect>,
}

pub(super) fn layout(sidebar: Rect, state: &ZodeAppState) -> Option<SidebarMenuLayout> {
    if let Some(workspace) = state.sidebar.project_menu.as_ref() {
        return project_menu(sidebar, state, workspace);
    }
    match state.sidebar.section_menu {
        Some(SidebarSectionMenu::Projects) => projects_menu(sidebar, state),
        Some(SidebarSectionMenu::Tasks) => tasks_menu(sidebar, state),
        None => None,
    }
}

fn project_menu(
    sidebar: Rect,
    state: &ZodeAppState,
    workspace: &WorkspaceUri,
) -> Option<SidebarMenuLayout> {
    let sidebar_layout = ProjectSidebar::layout(sidebar, state);
    let row = sidebar_layout.rows.iter().find(
        |row| matches!(&row.target, SidebarRowTarget::Project(value) if value == workspace),
    )?;
    let anchor = ProjectSidebar::project_more_rect(row)?;
    let rect = menu_rect(sidebar, anchor, MENU_PAD * 2.0 + ROW_H * 4.0 + SEPARATOR_H);
    let mut y = rect.origin.y + MENU_PAD;
    let row_x = rect.origin.x + MENU_PAD;
    let row_w = rect.size.x - MENU_PAD * 2.0;
    let pinned = state.sidebar.pinned_projects.contains(workspace);
    let expanded = state
        .projects
        .iter()
        .find(|project| &project.workspace_uri == workspace)
        .is_none_or(|project| project.expanded);
    let pin = item(
        row_x,
        row_w,
        &mut y,
        SIDEBAR_PROJECT_MENU_PIN_ID,
        if pinned {
            "取消置顶项目"
        } else {
            "置顶项目"
        },
        Some(SemanticIcon::Pin),
        true,
        false,
    );
    let finder = item(
        row_x,
        row_w,
        &mut y,
        SIDEBAR_PROJECT_MENU_FINDER_ID,
        "在 Finder 中显示",
        Some(SemanticIcon::Folder),
        state.available_workspace(workspace),
        false,
    );
    let toggle = item(
        row_x,
        row_w,
        &mut y,
        SIDEBAR_PROJECT_MENU_TOGGLE_ID,
        if expanded {
            "折叠项目"
        } else {
            "展开项目"
        },
        Some(if expanded {
            SemanticIcon::ChevronDown
        } else {
            SemanticIcon::ChevronRight
        }),
        true,
        false,
    );
    let separator = Rect::xywh(row_x, y, row_w, SEPARATOR_H);
    y += SEPARATOR_H;
    let archive = item(
        row_x,
        row_w,
        &mut y,
        SIDEBAR_PROJECT_MENU_ARCHIVE_ID,
        "归档项目任务",
        Some(SemanticIcon::Archive),
        true,
        false,
    );
    Some(SidebarMenuLayout {
        id: SIDEBAR_PROJECT_MENU_ID,
        rect,
        kind: SidebarMenuKind::Project,
        workspace_uri: Some(workspace.clone()),
        items: vec![pin, finder, toggle, archive],
        separators: vec![separator],
    })
}

fn projects_menu(sidebar: Rect, state: &ZodeAppState) -> Option<SidebarMenuLayout> {
    let sidebar_layout = ProjectSidebar::layout(sidebar, state);
    let anchor = sidebar_layout
        .controls
        .iter()
        .find(|control| control.target == SidebarControlTarget::ProjectsMenu)?
        .rect;
    let rect = menu_rect(
        sidebar,
        anchor,
        MENU_PAD * 2.0 + HEADING_H * 2.0 + ROW_H * 5.0 + SEPARATOR_H,
    );
    let row_x = rect.origin.x + MENU_PAD;
    let row_w = rect.size.x - MENU_PAD * 2.0;
    let mut y = rect.origin.y + MENU_PAD + HEADING_H;
    let grouped = item(
        row_x,
        row_w,
        &mut y,
        SIDEBAR_PROJECTS_MENU_GROUPED_ID,
        "按项目",
        Some(SemanticIcon::Folder),
        true,
        state.sidebar.project_display_mode == ProjectDisplayMode::Grouped,
    );
    let flat = item(
        row_x,
        row_w,
        &mut y,
        SIDEBAR_PROJECTS_MENU_FLAT_ID,
        "在一个列表中",
        Some(SemanticIcon::FileText),
        true,
        state.sidebar.project_display_mode == ProjectDisplayMode::Flat,
    );
    let separator = Rect::xywh(row_x, y, row_w, SEPARATOR_H);
    y += SEPARATOR_H + HEADING_H;
    let priority = item(
        row_x,
        row_w,
        &mut y,
        SIDEBAR_PROJECTS_MENU_PRIORITY_ID,
        "优先级",
        None,
        true,
        state.sidebar.project_sort_mode == ProjectSortMode::Priority,
    );
    let recent = item(
        row_x,
        row_w,
        &mut y,
        SIDEBAR_PROJECTS_MENU_RECENT_ID,
        "最近更新",
        None,
        true,
        state.sidebar.project_sort_mode == ProjectSortMode::RecentlyUpdated,
    );
    let manual = item(
        row_x,
        row_w,
        &mut y,
        SIDEBAR_PROJECTS_MENU_MANUAL_ID,
        "手动排序",
        None,
        true,
        state.sidebar.project_sort_mode == ProjectSortMode::Manual,
    );
    Some(SidebarMenuLayout {
        id: SIDEBAR_PROJECTS_MENU_ID,
        rect,
        kind: SidebarMenuKind::Projects,
        workspace_uri: None,
        items: vec![grouped, flat, priority, recent, manual],
        separators: vec![separator],
    })
}

fn tasks_menu(sidebar: Rect, state: &ZodeAppState) -> Option<SidebarMenuLayout> {
    let sidebar_layout = ProjectSidebar::layout(sidebar, state);
    let anchor = sidebar_layout
        .controls
        .iter()
        .find(|control| control.target == SidebarControlTarget::TasksMenu)?
        .rect;
    let rect = menu_rect(sidebar, anchor, MENU_PAD * 2.0 + ROW_H * 2.0);
    let row_x = rect.origin.x + MENU_PAD;
    let row_w = rect.size.x - MENU_PAD * 2.0;
    let mut y = rect.origin.y + MENU_PAD;
    let new_task = item(
        row_x,
        row_w,
        &mut y,
        SIDEBAR_TASKS_MENU_NEW_ID,
        "新建无项目任务",
        Some(SemanticIcon::NewTask),
        true,
        false,
    );
    let toggle = item(
        row_x,
        row_w,
        &mut y,
        SIDEBAR_TASKS_MENU_TOGGLE_ID,
        if state.sidebar.tasks_expanded {
            "折叠任务"
        } else {
            "展开任务"
        },
        Some(if state.sidebar.tasks_expanded {
            SemanticIcon::ChevronDown
        } else {
            SemanticIcon::ChevronRight
        }),
        true,
        false,
    );
    Some(SidebarMenuLayout {
        id: SIDEBAR_TASKS_MENU_ID,
        rect,
        kind: SidebarMenuKind::Tasks,
        workspace_uri: None,
        items: vec![new_task, toggle],
        separators: Vec::new(),
    })
}

#[allow(clippy::too_many_arguments)]
fn item(
    x: f32,
    width: f32,
    y: &mut f32,
    id: WidgetId,
    label: &str,
    icon: Option<SemanticIcon>,
    enabled: bool,
    selected: bool,
) -> SidebarMenuItemLayout {
    let rect = Rect::xywh(x, *y, width, ROW_H);
    *y += ROW_H;
    SidebarMenuItemLayout {
        id,
        rect,
        label: label.into(),
        icon,
        enabled,
        selected,
    }
}

fn menu_rect(sidebar: Rect, anchor: Rect, height: f32) -> Rect {
    let min_y = sidebar.origin.y + 8.0;
    let max_y = (sidebar.max_y() - height - 8.0).max(min_y);
    Rect::xywh(
        sidebar.max_x() + 8.0,
        anchor.origin.y.clamp(min_y, max_y),
        MENU_W,
        height,
    )
}

pub(super) fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if let Some(workspace_uri) = state.sidebar.project_menu.as_ref() {
        return match id {
            SIDEBAR_PROJECT_MENU_PIN_ID => Some(AppCommand::SetProjectPinned {
                workspace_uri: workspace_uri.clone(),
                pinned: !state.sidebar.pinned_projects.contains(workspace_uri),
            }),
            SIDEBAR_PROJECT_MENU_FINDER_ID => Some(AppCommand::OpenProjectInFinder {
                workspace_uri: workspace_uri.clone(),
            }),
            SIDEBAR_PROJECT_MENU_TOGGLE_ID => {
                Some(AppCommand::ToggleProject(workspace_uri.clone()))
            }
            SIDEBAR_PROJECT_MENU_ARCHIVE_ID => Some(AppCommand::ArchiveProjectTasks {
                workspace_uri: workspace_uri.clone(),
            }),
            _ => None,
        };
    }
    match state.sidebar.section_menu {
        Some(SidebarSectionMenu::Projects) => match id {
            SIDEBAR_PROJECTS_MENU_GROUPED_ID => Some(AppCommand::SetProjectDisplayMode(
                ProjectDisplayMode::Grouped,
            )),
            SIDEBAR_PROJECTS_MENU_FLAT_ID => {
                Some(AppCommand::SetProjectDisplayMode(ProjectDisplayMode::Flat))
            }
            SIDEBAR_PROJECTS_MENU_PRIORITY_ID => {
                Some(AppCommand::SetProjectSortMode(ProjectSortMode::Priority))
            }
            SIDEBAR_PROJECTS_MENU_RECENT_ID => Some(AppCommand::SetProjectSortMode(
                ProjectSortMode::RecentlyUpdated,
            )),
            SIDEBAR_PROJECTS_MENU_MANUAL_ID => {
                Some(AppCommand::SetProjectSortMode(ProjectSortMode::Manual))
            }
            _ => None,
        },
        Some(SidebarSectionMenu::Tasks) => match id {
            SIDEBAR_TASKS_MENU_NEW_ID => Some(AppCommand::BeginTask {
                workspace_uri: None,
            }),
            SIDEBAR_TASKS_MENU_TOGGLE_ID => Some(AppCommand::ToggleSidebarTasks),
            _ => None,
        },
        None => None,
    }
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    menu: &SidebarMenuLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    painter.fill_drop_shadow(
        Rect::xywh(
            menu.rect.origin.x,
            menu.rect.origin.y + 2.0,
            menu.rect.size.x,
            menu.rect.size.y,
        ),
        10.0,
        16.0,
        theme.tokens.foreground.with_alpha(0.12),
    );
    Popover.paint(painter, menu.rect, &theme.tokens);
    if menu.kind == SidebarMenuKind::Projects {
        paint_heading(
            painter,
            menu.rect.origin.y + MENU_PAD,
            "整理",
            menu.rect,
            theme,
        );
        let second_y = menu.separators[0].origin.y + SEPARATOR_H;
        paint_heading(painter, second_y, "排序方式", menu.rect, theme);
    }
    for separator in &menu.separators {
        painter.fill_rect(*separator, theme.tokens.border);
    }
    for item in &menu.items {
        paint_item(painter, item, focused, hovered, theme);
    }
}

fn paint_heading(painter: &mut dyn Painter, y: f32, label: &str, menu: Rect, theme: &ZodeTheme) {
    paint_single_line(
        painter,
        label,
        Rect::xywh(menu.origin.x + 14.0, y, menu.size.x - 28.0, HEADING_H),
        11.0,
        500,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
}

fn paint_item(
    painter: &mut dyn Painter,
    item: &SidebarMenuItemLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if item.enabled && hovered == Some(item.id) {
        painter.fill_round_rect(item.rect, 7.0, theme.tokens.accent);
    }
    if item.enabled && focused == Some(item.id) {
        painter.stroke_round_rect(item.rect, 7.0, theme.tokens.ring, 1.5);
    }
    let foreground = if item.enabled {
        theme.tokens.popover_foreground
    } else {
        theme.tokens.muted_foreground
    };
    let mut label_x = item.rect.origin.x + 10.0;
    if item.selected {
        let icon = SemanticIcon::Check;
        painter.stroke_svg_path(
            icon.path(),
            Point2D::new(item.rect.origin.x + 9.0, item.rect.origin.y + 10.0),
            16.0,
            foreground,
            icon.stroke_width(),
        );
        label_x += 27.0;
    } else if let Some(icon) = item.icon {
        painter.stroke_svg_path(
            icon.path(),
            Point2D::new(item.rect.origin.x + 9.0, item.rect.origin.y + 10.0),
            16.0,
            foreground,
            icon.stroke_width(),
        );
        label_x += 27.0;
    }
    paint_single_line(
        painter,
        &item.label,
        Rect::xywh(
            label_x,
            item.rect.origin.y,
            (item.rect.max_x() - label_x - 10.0).max(0.0),
            item.rect.size.y,
        ),
        13.0,
        400,
        foreground,
        HorizontalAlign::Start,
    );
}
