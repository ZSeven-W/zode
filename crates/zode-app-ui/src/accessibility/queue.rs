use accesskit::{Action, Role};
use jian_core::CursorHint;
use zode_app_model::{QueuedMessage, ZodeAppState};

use crate::{Composer, WorkspaceLayout};

use super::{next_order, node, InteractionNode};

pub(super) fn append_queue_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let Some(queue) = Composer::queue_layout_in_viewport(layout.composer, layout.viewport, state)
    else {
        return;
    };
    let Some(session) = state.current_session.as_ref() else {
        return;
    };
    let Some(messages) = state.message_queues.get(session) else {
        return;
    };
    nodes.push(node(
        queue.list_id,
        queue.surface,
        Role::List,
        &format!("排队消息，共 {} 条", queue.total_count),
        (queue.total_count > queue.rows.len())
            .then(|| format!("当前显示前 {} 条", queue.rows.len())),
        Vec::new(),
        None,
        CursorHint::Default,
    ));
    for (row, message) in queue.rows.iter().zip(&messages.items) {
        let preview = accessibility_preview(message);
        nodes.push(node(
            row.row_id,
            row.rect,
            Role::ListItem,
            &preview,
            (!message.attachments.is_empty())
                .then(|| format!("包含 {} 个附件", message.attachments.len())),
            Vec::new(),
            None,
            CursorHint::Default,
        ));
        for (id, rect, name, value) in [
            (
                row.guide_id,
                row.guide,
                format!("引导排队消息：{preview}"),
                Some("提交，但不中断模型运行".into()),
            ),
            (
                row.delete_id,
                row.delete,
                format!("删除排队消息：{preview}"),
                None,
            ),
            (
                row.more_id,
                row.more,
                format!("更多排队操作：{preview}"),
                None,
            ),
        ] {
            nodes.push(node(
                id,
                rect,
                Role::Button,
                &name,
                value,
                vec![Action::Click, Action::Focus],
                next_order(focus_order),
                CursorHint::Pointer,
            ));
        }
    }
}

pub(super) fn append_queue_menu_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let Some(queue) = Composer::queue_layout_in_viewport(layout.composer, layout.viewport, state)
    else {
        return;
    };
    let Some(menu) = queue.menu else {
        return;
    };
    nodes.push(node(
        menu.id,
        menu.rect,
        Role::Menu,
        "排队消息操作",
        None,
        Vec::new(),
        None,
        CursorHint::Default,
    ));
    for (id, rect, name) in [
        (menu.edit_id, menu.edit, "编辑消息"),
        (menu.close_id, menu.close, "关闭排队并清空全部消息"),
    ] {
        nodes.push(node(
            id,
            rect,
            Role::MenuItem,
            name,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
}

fn accessibility_preview(message: &QueuedMessage) -> String {
    if !message.text.trim().is_empty() {
        message.text.clone()
    } else if message.attachments.len() == 1 {
        format!("附件：{}", message.attachments[0].display_name)
    } else {
        format!("{} 个附件", message.attachments.len())
    }
}
