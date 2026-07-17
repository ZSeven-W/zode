use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use jian_widgets::Rect;
use zode_app_model::ZodeAppState;

use super::{
    next_order, node, InteractionNode, CHATS_NAV_ID, HELP_ID, NEW_SESSION_ID, PLUGINS_NAV_ID,
    PULL_REQUESTS_NAV_ID, SCHEDULED_NAV_ID, SETTINGS_NAV_ID, SIDEBAR_ID, SITES_NAV_ID,
};
use crate::{
    CollapsedSidebarChrome, ProjectSidebar, SidebarControlTarget, SidebarRowTarget, SidebarSection,
    ThreadTranscript, WidgetId, WorkspaceLayout, COLLAPSED_SIDEBAR_BACK_ID,
    COLLAPSED_SIDEBAR_FORWARD_ID, SIDEBAR_SEARCH_ID, SIDEBAR_TOGGLE_ID,
};

pub(super) fn append_collapsed_sidebar_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
) {
    let top_bar = Rect::xywh(
        layout.viewport.origin.x,
        layout.top_bar.origin.y,
        layout.viewport.size.x,
        layout.top_bar.size.y,
    );
    let chrome = CollapsedSidebarChrome::layout(top_bar);
    nodes.push(node(
        SIDEBAR_TOGGLE_ID,
        chrome.toggle,
        Role::Button,
        "显示侧边栏",
        None,
        vec![Action::Click, Action::Focus],
        next_order(focus_order),
        CursorHint::Pointer,
    ));

    let mut back = node(
        COLLAPSED_SIDEBAR_BACK_ID,
        chrome.back,
        Role::Button,
        "返回",
        None,
        Vec::new(),
        None,
        CursorHint::Default,
    );
    back.disabled = true;
    nodes.push(back);

    let mut forward = node(
        COLLAPSED_SIDEBAR_FORWARD_ID,
        chrome.forward,
        Role::Button,
        "前进",
        None,
        Vec::new(),
        None,
        CursorHint::Default,
    );
    forward.disabled = true;
    nodes.push(forward);

    nodes.push(node(
        NEW_SESSION_ID,
        chrome.new_task,
        Role::Button,
        "新建任务",
        None,
        vec![Action::Click, Action::Focus],
        next_order(focus_order),
        CursorHint::Pointer,
    ));
}

pub(super) fn append_sidebar_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let sidebar = ProjectSidebar::layout(layout.primary_sidebar_content, state);
    let visible_sidebar = layout.sidebar;
    let mut scroll_actions = Vec::new();
    if sidebar.scroll_offset > 0.0 {
        scroll_actions.push(Action::ScrollUp);
    }
    if sidebar.scroll_offset < sidebar.max_scroll {
        scroll_actions.push(Action::ScrollDown);
    }
    let Some(scroll_viewport) = visible_sidebar_rect(sidebar.scroll_viewport, visible_sidebar)
    else {
        return;
    };
    nodes.push(node(
        SIDEBAR_ID,
        scroll_viewport,
        Role::Navigation,
        "侧边栏",
        None,
        scroll_actions,
        None,
        CursorHint::Default,
    ));

    if !sidebar.compact {
        for (id, rect, name) in [
            (SIDEBAR_TOGGLE_ID, sidebar.titlebar_toggle, "隐藏侧边栏"),
            (SIDEBAR_SEARCH_ID, sidebar.brand_search, "搜索项目"),
        ] {
            if let Some(rect) = visible_sidebar_rect(rect, visible_sidebar) {
                nodes.push(node(
                    id,
                    rect,
                    Role::Button,
                    name,
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
        }
    }

    for row in &sidebar.navigation_rows {
        let Some(rect) = sidebar_interaction_rect(
            row.rect,
            if row.index == 0 {
                layout.sidebar
            } else {
                sidebar.scroll_viewport
            },
            visible_sidebar,
        ) else {
            continue;
        };
        nodes.push(node(
            navigation_id(row.index),
            rect,
            Role::Button,
            row.item.label,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }

    for section in &sidebar.sections {
        let Some(rect) =
            sidebar_interaction_rect(section.rect, sidebar.scroll_viewport, visible_sidebar)
        else {
            continue;
        };
        let interactive = section.section != SidebarSection::Pinned;
        let actions = if section.section == SidebarSection::Tasks {
            vec![Action::Click, Action::Focus]
        } else if interactive {
            vec![Action::Focus]
        } else {
            Vec::new()
        };
        nodes.push(node(
            section.id,
            rect,
            if section.section == SidebarSection::Tasks {
                Role::Button
            } else {
                Role::Group
            },
            section.label,
            None,
            actions,
            interactive.then(|| next_order(focus_order)).flatten(),
            if section.section == SidebarSection::Tasks {
                CursorHint::Pointer
            } else {
                CursorHint::Default
            },
        ));
    }

    for row in &sidebar.rows {
        let Some(rect) =
            sidebar_interaction_rect(row.rect, sidebar.scroll_viewport, visible_sidebar)
        else {
            continue;
        };
        let name = match &row.target {
            SidebarRowTarget::Project(_) => format!("项目 {}", row.label),
            SidebarRowTarget::Task(_) => format!("任务 {}", row.label),
            SidebarRowTarget::Session(_) if row.pinned => format!("置顶任务 {}", row.label),
            SidebarRowTarget::Session(_) => row.label.clone(),
        };
        let mut row_node = node(
            row.id,
            rect,
            Role::Button,
            &name,
            None,
            if row.actionable {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            row.actionable.then(|| next_order(focus_order)).flatten(),
            if row.actionable {
                CursorHint::Pointer
            } else {
                CursorHint::Default
            },
        );
        if row.selected {
            row_node.toggled = Some(Toggled::True);
        }
        nodes.push(row_node);

        if let SidebarRowTarget::Project(workspace) = &row.target {
            for (id, action_rect, label) in [
                (
                    row.more_id,
                    ProjectSidebar::project_more_rect(row),
                    "项目菜单",
                ),
                (
                    row.new_id,
                    ProjectSidebar::project_new_rect(row),
                    "在项目中新建任务",
                ),
            ] {
                if let (Some(id), Some(action_rect)) = (id, action_rect) {
                    let Some(action_rect) = sidebar_interaction_rect(
                        action_rect,
                        sidebar.scroll_viewport,
                        visible_sidebar,
                    ) else {
                        continue;
                    };
                    nodes.push(node(
                        id,
                        action_rect,
                        Role::Button,
                        label,
                        Some(workspace.as_str().to_owned()),
                        vec![Action::Click, Action::Focus],
                        next_order(focus_order),
                        CursorHint::Pointer,
                    ));
                }
            }
            continue;
        }
        let Some(session) = row.session() else {
            continue;
        };
        if let (Some(id), Some(action_rect)) = (row.pin_id, ProjectSidebar::session_pin_rect(row)) {
            if let Some(action_rect) =
                sidebar_interaction_rect(action_rect, sidebar.scroll_viewport, visible_sidebar)
            {
                nodes.push(node(
                    id,
                    action_rect,
                    Role::Button,
                    if row.pinned {
                        "取消置顶"
                    } else {
                        "置顶任务"
                    },
                    Some(session.session_id.clone()),
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
        }
        if let (Some(id), Some(action_rect)) =
            (row.archive_id, ProjectSidebar::session_archive_rect(row))
        {
            if let Some(action_rect) =
                sidebar_interaction_rect(action_rect, sidebar.scroll_viewport, visible_sidebar)
            {
                nodes.push(node(
                    id,
                    action_rect,
                    Role::Button,
                    "归档任务",
                    Some(session.session_id.clone()),
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
        }
    }

    for control in &sidebar.controls {
        if !control.actionable() {
            continue;
        }
        let Some(rect) =
            sidebar_interaction_rect(control.rect, sidebar.scroll_viewport, visible_sidebar)
        else {
            continue;
        };
        let name = match &control.target {
            SidebarControlTarget::ShowAllProjects => "显示全部项目",
            SidebarControlTarget::ShowAllProjectSessions(_) => "显示项目中的全部任务",
            SidebarControlTarget::ToggleTasks => control.label.as_str(),
            SidebarControlTarget::ProjectsMenu => "项目菜单",
            SidebarControlTarget::NewProject => "新建项目",
            SidebarControlTarget::TasksMenu => "任务菜单",
            SidebarControlTarget::NewProjectlessTask => "新建无项目任务",
        };
        nodes.push(node(
            control.id,
            rect,
            Role::Button,
            name,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }

    if let Some(profile) = visible_sidebar_rect(sidebar.profile, visible_sidebar) {
        nodes.push(node(
            SETTINGS_NAV_ID,
            profile,
            Role::Button,
            &format!("本地账户 {}", state.local_profile.display_name),
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
    if let Some(help) = visible_sidebar_rect(sidebar.help, visible_sidebar) {
        nodes.push(node(
            HELP_ID,
            help,
            Role::Button,
            "帮助",
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
}

pub(super) fn append_sidebar_menu_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    if layout.sidebar.size.x + f32::EPSILON < layout.primary_sidebar_content.size.x {
        return;
    }
    let Some(menu) = ProjectSidebar::menu_layout(layout.primary_sidebar_content, state) else {
        return;
    };
    nodes.push(node(
        menu.id,
        menu.rect,
        Role::Menu,
        "侧边栏菜单",
        None,
        Vec::new(),
        None,
        CursorHint::Default,
    ));
    for item in menu.items {
        let mut item_node = node(
            item.id,
            item.rect,
            Role::MenuItem,
            &item.label,
            None,
            if item.enabled {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            item.enabled.then(|| next_order(focus_order)).flatten(),
            if item.enabled {
                CursorHint::Pointer
            } else {
                CursorHint::Default
            },
        );
        item_node.toggled = item.selected.then_some(Toggled::True);
        item_node.disabled = !item.enabled;
        nodes.push(item_node);
    }
}

fn sidebar_interaction_rect(
    rect: jian_widgets::Rect,
    viewport: jian_widgets::Rect,
    visible: jian_widgets::Rect,
) -> Option<jian_widgets::Rect> {
    ThreadTranscript::clip_to_viewport(rect, viewport)
        .and_then(|rect| visible_sidebar_rect(rect, visible))
}

fn visible_sidebar_rect(
    rect: jian_widgets::Rect,
    visible: jian_widgets::Rect,
) -> Option<jian_widgets::Rect> {
    ThreadTranscript::clip_to_viewport(rect, visible)
}

const fn navigation_id(index: usize) -> WidgetId {
    match index {
        0 => NEW_SESSION_ID,
        1 => SCHEDULED_NAV_ID,
        2 => PLUGINS_NAV_ID,
        3 => SITES_NAV_ID,
        4 => PULL_REQUESTS_NAV_ID,
        5 => CHATS_NAV_ID,
        _ => SIDEBAR_ID,
    }
}
