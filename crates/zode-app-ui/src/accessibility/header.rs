use accesskit::{Action, Role};
use jian_core::CursorHint;
use zode_app_model::ZodeAppState;

use crate::widgets::{
    ThreadMenuActionLayout, HEADER_COPY_DETAILS_ID, HEADER_COPY_MENU_ID, HEADER_COPY_SESSION_ID,
    HEADER_COPY_TITLE_ID, HEADER_MENU_ARCHIVE_ID, HEADER_MENU_CONTINUE_ID, HEADER_MENU_COPY_ID,
    HEADER_MENU_ID, HEADER_MENU_NEW_WINDOW_ID, HEADER_MENU_PIN_ID, HEADER_MENU_RENAME_ID,
    HEADER_MENU_SCHEDULE_ID, HEADER_MENU_SIDE_TASK_ID, HEADER_MORE_ID, HEADER_RENAME_CANCEL_ID,
    HEADER_RENAME_DIALOG_ID, HEADER_RENAME_INPUT_ID, HEADER_RENAME_SAVE_ID,
};
use crate::{
    PanelPicker, ThreadHeader, ThreadTranscript, WorkspaceLayout, HEADER_ENVIRONMENT_ID,
    HEADER_REVIEW_ID, PANEL_PICKER_ID, PANEL_PICKER_MENU_ID,
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
        (header.panel_picker, PANEL_PICKER_ID, "选择侧边面板"),
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

pub(super) fn append_panel_picker_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let Some(anchor) = ThreadHeader::layout(layout.top_bar, state).panel_picker else {
        return;
    };
    let Some(menu) = PanelPicker::menu_layout(anchor.rect, layout.viewport, state) else {
        return;
    };
    debug_assert_eq!(menu.id, PANEL_PICKER_MENU_ID);
    nodes.push(node(
        PANEL_PICKER_MENU_ID,
        menu.rect,
        Role::Menu,
        "侧边面板",
        None,
        Vec::new(),
        None,
        CursorHint::Default,
    ));
    for item in menu.items {
        let mut interaction = node(
            item.id,
            item.rect,
            Role::MenuItem,
            item.label,
            item.unavailable_reason.map(str::to_owned),
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
        interaction.disabled = !item.enabled;
        nodes.push(interaction);
    }
}

pub(super) fn append_header_menu_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) -> Option<crate::WidgetId> {
    let mut overlay_focus = None;
    if let Some(menu) = ThreadHeader::menu_layout(layout.top_bar, state) {
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
            (menu.rename, HEADER_MENU_RENAME_ID, "重命名任务"),
            (menu.archive, HEADER_MENU_ARCHIVE_ID, "归档任务"),
            (
                menu.side_task,
                HEADER_MENU_SIDE_TASK_ID,
                "打开侧边任务，即将支持",
            ),
            (menu.copy, HEADER_MENU_COPY_ID, "复制"),
            (
                menu.continue_in,
                HEADER_MENU_CONTINUE_ID,
                "在其他应用中继续，即将支持",
            ),
            (
                menu.schedule,
                HEADER_MENU_SCHEDULE_ID,
                "添加计划任务，即将支持",
            ),
            (menu.new_window, HEADER_MENU_NEW_WINDOW_ID, "在新窗口中打开"),
        ] {
            append_menu_action(nodes, layout, focus_order, action, id, label);
        }
        overlay_focus = Some(HEADER_MENU_PIN_ID);
        if let Some(copy) = menu.copy_menu {
            if let Some(rect) = ThreadTranscript::clip_to_viewport(copy.rect, layout.viewport) {
                nodes.push(node(
                    HEADER_COPY_MENU_ID,
                    rect,
                    Role::Menu,
                    "复制任务信息",
                    None,
                    Vec::new(),
                    None,
                    CursorHint::Default,
                ));
            }
            for (action, id, label) in [
                (copy.title, HEADER_COPY_TITLE_ID, "复制任务标题"),
                (copy.details, HEADER_COPY_DETAILS_ID, "复制任务信息"),
                (copy.session_id, HEADER_COPY_SESSION_ID, "复制任务 ID"),
            ] {
                append_menu_action(nodes, layout, focus_order, action, id, label);
            }
            overlay_focus = Some(HEADER_COPY_TITLE_ID);
        }
    }
    if let Some(rename) = ThreadHeader::rename_layout(layout.top_bar, state) {
        if let Some(rect) = ThreadTranscript::clip_to_viewport(rename.rect, layout.viewport) {
            nodes.push(node(
                HEADER_RENAME_DIALOG_ID,
                rect,
                Role::Dialog,
                "重命名任务",
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            ));
        }
        let draft = state
            .session_rename
            .as_ref()
            .map(|rename| rename.draft.clone())
            .unwrap_or_default();
        nodes.push(node(
            HEADER_RENAME_INPUT_ID,
            rename.input,
            Role::TextInput,
            "任务名称",
            Some(draft),
            vec![Action::Focus, Action::SetValue],
            next_order(focus_order),
            CursorHint::Text,
        ));
        append_menu_action(
            nodes,
            layout,
            focus_order,
            rename.cancel,
            HEADER_RENAME_CANCEL_ID,
            "取消",
        );
        append_menu_action(
            nodes,
            layout,
            focus_order,
            rename.save,
            HEADER_RENAME_SAVE_ID,
            "保存",
        );
        overlay_focus = Some(HEADER_RENAME_INPUT_ID);
    }
    overlay_focus
}

fn append_menu_action(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    action: ThreadMenuActionLayout,
    id: crate::WidgetId,
    label: &str,
) {
    debug_assert_eq!(action.id, id);
    let Some(rect) = ThreadTranscript::clip_to_viewport(action.rect, layout.viewport) else {
        return;
    };
    let enabled_actions = if action.enabled {
        vec![Action::Click, Action::Focus]
    } else {
        Vec::new()
    };
    let mut item = node(
        id,
        rect,
        Role::MenuItem,
        label,
        None,
        enabled_actions,
        action.enabled.then(|| next_order(focus_order)).flatten(),
        if action.enabled {
            CursorHint::Pointer
        } else {
            CursorHint::Default
        },
    );
    item.disabled = !action.enabled;
    nodes.push(item);
}
