use accesskit::{Action, Role};
use jian_core::CursorHint;
use zode_app_model::ZodeAppState;

use crate::widgets::{HEADER_MENU_ARCHIVE_ID, HEADER_MENU_ID, HEADER_MENU_PIN_ID, HEADER_MORE_ID};
use crate::{
    ThreadHeader, ThreadTranscript, WorkspaceLayout, HEADER_ENVIRONMENT_ID, HEADER_REVIEW_ID,
};

use super::{next_order, node, visible_rect, InteractionNode};

pub(super) fn append_header_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let header = ThreadHeader::layout(layout.top_bar, state);
    for (action, id, label) in [
        (header.more, HEADER_MORE_ID, "任务操作"),
        (header.environment, HEADER_ENVIRONMENT_ID, "环境信息"),
        (header.review, HEADER_REVIEW_ID, "审查变更"),
    ] {
        let Some(action) = action.filter(|action| visible_rect(action.rect)) else {
            continue;
        };
        debug_assert_eq!(action.id, id);
        nodes.push(node(
            id,
            action.rect,
            Role::Button,
            label,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
}

pub(super) fn append_header_menu_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let Some(menu) = ThreadHeader::menu_layout(layout.top_bar, state) else {
        return;
    };
    debug_assert_eq!(menu.id, HEADER_MENU_ID);
    if let Some(rect) = ThreadTranscript::clip_to_viewport(menu.rect, layout.viewport) {
        nodes.push(node(
            HEADER_MENU_ID,
            rect,
            Role::Menu,
            "任务操作",
            None,
            Vec::new(),
            None,
            CursorHint::Default,
        ));
    }

    let pinned = state
        .current_session
        .as_ref()
        .is_some_and(|session| state.pinned_sessions.contains(session));
    for (action, id, label) in [
        (
            menu.pin,
            HEADER_MENU_PIN_ID,
            if pinned {
                "取消置顶"
            } else {
                "置顶任务"
            },
        ),
        (menu.archive, HEADER_MENU_ARCHIVE_ID, "归档任务"),
    ] {
        debug_assert_eq!(action.id, id);
        let Some(rect) = ThreadTranscript::clip_to_viewport(action.rect, layout.viewport) else {
            continue;
        };
        nodes.push(node(
            id,
            rect,
            Role::MenuItem,
            label,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
}
