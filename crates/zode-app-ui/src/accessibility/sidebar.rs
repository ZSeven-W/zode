use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use zode_app_model::ZodeAppState;

use super::{
    next_order, node, visible_rect, InteractionNode, CHATS_NAV_ID, HELP_ID, NEW_SESSION_ID,
    PLUGINS_NAV_ID, PULL_REQUESTS_NAV_ID, SCHEDULED_NAV_ID, SETTINGS_NAV_ID, SIDEBAR_ID,
    SITES_NAV_ID,
};
use crate::{
    ProjectSidebar, SidebarControlTarget, SidebarRowTarget, SidebarSection, ThreadTranscript,
    WidgetId, WorkspaceLayout,
};

pub(super) fn append_sidebar_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let sidebar = ProjectSidebar::layout(layout.sidebar, state);
    let mut scroll_actions = Vec::new();
    if sidebar.scroll_offset > 0.0 {
        scroll_actions.push(Action::ScrollUp);
    }
    if sidebar.scroll_offset < sidebar.max_scroll {
        scroll_actions.push(Action::ScrollDown);
    }
    nodes.push(node(
        SIDEBAR_ID,
        sidebar.scroll_viewport,
        Role::Navigation,
        "侧边栏",
        None,
        scroll_actions,
        None,
        CursorHint::Default,
    ));

    for row in &sidebar.navigation_rows {
        let Some(rect) = ThreadTranscript::clip_to_viewport(
            row.rect,
            if row.index == 0 {
                layout.sidebar
            } else {
                sidebar.scroll_viewport
            },
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
        let Some(rect) = ThreadTranscript::clip_to_viewport(section.rect, sidebar.scroll_viewport)
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
        let Some(rect) = ThreadTranscript::clip_to_viewport(row.rect, sidebar.scroll_viewport)
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
                    let Some(action_rect) =
                        ThreadTranscript::clip_to_viewport(action_rect, sidebar.scroll_viewport)
                    else {
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
                ThreadTranscript::clip_to_viewport(action_rect, sidebar.scroll_viewport)
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
                ThreadTranscript::clip_to_viewport(action_rect, sidebar.scroll_viewport)
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
        let Some(rect) = ThreadTranscript::clip_to_viewport(control.rect, sidebar.scroll_viewport)
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

    if visible_rect(sidebar.profile) {
        nodes.push(node(
            SETTINGS_NAV_ID,
            sidebar.profile,
            Role::Button,
            &format!("本地账户 {}", state.local_profile.display_name),
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
    if visible_rect(sidebar.help) {
        nodes.push(node(
            HELP_ID,
            sidebar.help,
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
    let Some(menu) = ProjectSidebar::menu_layout(layout.sidebar, state) else {
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
