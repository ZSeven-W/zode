use accesskit::{Action, Role};
use jian_core::CursorHint;
use zode_app_model::ZodeAppState;

use super::{
    next_order, node, visible_rect, InteractionNode, CHATS_NAV_ID, NEW_SESSION_ID, PLUGINS_NAV_ID,
    PULL_REQUESTS_NAV_ID, SCHEDULED_NAV_ID, SETTINGS_NAV_ID, SIDEBAR_ID, SITES_NAV_ID,
};
use crate::{ProjectSidebar, SidebarRowTarget, ThreadTranscript, WorkspaceLayout};

pub(super) fn append_sidebar_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    nodes.push(node(
        SIDEBAR_ID,
        layout.sidebar,
        Role::Navigation,
        "项目",
        None,
        Vec::new(),
        None,
        CursorHint::Default,
    ));
    for (row, id) in ProjectSidebar::navigation_row_layout(layout.sidebar)
        .into_iter()
        .zip([
            NEW_SESSION_ID,
            SCHEDULED_NAV_ID,
            PLUGINS_NAV_ID,
            SITES_NAV_ID,
            PULL_REQUESTS_NAV_ID,
            CHATS_NAV_ID,
        ])
    {
        let Some(rect) = ThreadTranscript::clip_to_viewport(row.rect, layout.sidebar) else {
            continue;
        };
        nodes.push(node(
            id,
            rect,
            Role::Button,
            row.item.label,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
    for row in ProjectSidebar::dynamic_row_layout(layout.sidebar, state) {
        let name = match &row.target {
            SidebarRowTarget::Project(_) => format!("项目 {}", row.label),
            SidebarRowTarget::Task(_) => format!("任务 {}", row.label),
            SidebarRowTarget::Session(_) => row.label.clone(),
        };
        nodes.push(node(
            row.id,
            row.rect,
            Role::Button,
            &name,
            None,
            if row.actionable {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            if row.actionable {
                next_order(focus_order)
            } else {
                None
            },
            if row.actionable {
                CursorHint::Pointer
            } else {
                CursorHint::Default
            },
        ));
    }
    let footer = ProjectSidebar::footer_rect(layout.sidebar);
    if visible_rect(footer) {
        nodes.push(node(
            SETTINGS_NAV_ID,
            footer,
            Role::Button,
            ProjectSidebar::footer_item().label,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
}
