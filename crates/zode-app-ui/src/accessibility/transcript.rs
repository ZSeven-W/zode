use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use zode_app_model::{TranscriptItem, ZodeAppState};

use crate::{ApprovalCard, ThreadTranscript, ToolCard, WorkspaceLayout};

use super::{next_order, node, InteractionNode};

pub(super) fn append_transcript_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let Some(session) = state.current_session.as_ref() else {
        return;
    };
    let Some(transcript) = state.transcripts.get(session) else {
        return;
    };
    for item_layout in ThreadTranscript::visible_item_layout_with_tools(
        layout.transcript,
        transcript,
        &state.tool_expanded,
    ) {
        let item = &transcript.items[item_layout.index];
        match item {
            TranscriptItem::UserText(text) => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Paragraph,
                &format!("你：{text}"),
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            )),
            TranscriptItem::AssistantText(text) => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Paragraph,
                text,
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            )),
            TranscriptItem::Thinking(text) => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Status,
                &format!("思考：{text}"),
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            )),
            TranscriptItem::Tool(tool) => {
                let expanded = state
                    .tool_expanded
                    .get(&tool.id)
                    .copied()
                    .unwrap_or_else(|| ToolCard::default_expanded(tool));
                let mut control = node(
                    ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                    item_layout.visible_rect,
                    Role::Button,
                    &format!("{}：{}", tool.name, tool.summary),
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                );
                control.toggled = Some(Toggled::from(expanded));
                nodes.push(control);
            }
            TranscriptItem::Approval { id, tool } => {
                nodes.push(node(
                    ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                    item_layout.visible_rect,
                    Role::Group,
                    &format!("需要批准：{tool}"),
                    None,
                    Vec::new(),
                    None,
                    CursorHint::Default,
                ));
                for button in ApprovalCard::button_layout(item_layout.rect) {
                    let Some(visible_button) =
                        ThreadTranscript::clip_to_viewport(button.rect, layout.transcript)
                    else {
                        continue;
                    };
                    nodes.push(node(
                        ThreadTranscript::approval_widget_id(session, id, button.action),
                        visible_button,
                        Role::Button,
                        button.label,
                        None,
                        vec![Action::Click, Action::Focus],
                        next_order(focus_order),
                        CursorHint::Pointer,
                    ));
                }
            }
            TranscriptItem::Status { message, .. } => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Status,
                message,
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            )),
            TranscriptItem::Error { message, .. } => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Alert,
                message,
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            )),
        }
    }
}
