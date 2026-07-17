use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use std::collections::BTreeMap;
use zode_app_model::{TranscriptItem, ZodeAppState};

use crate::widgets::transcript::{item_has_turn_divider, preview_available, turn_label};
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
    let empty = BTreeMap::new();
    let tool_expanded = state.tool_expanded.get(session).unwrap_or(&empty);
    for item_layout in ThreadTranscript::visible_item_layout_with_tools(
        layout.transcript,
        transcript,
        tool_expanded,
    ) {
        let item = &transcript.items[item_layout.index];
        match item {
            TranscriptItem::UserText(text) => {
                let label = if item_has_turn_divider(transcript, item_layout.index) {
                    format!("你：{text}；{}", turn_label(transcript, item_layout.index))
                } else {
                    format!("你：{text}")
                };
                nodes.push(node(
                    ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                    item_layout.visible_rect,
                    Role::Paragraph,
                    &label,
                    None,
                    Vec::new(),
                    None,
                    CursorHint::Default,
                ));
            }
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
            TranscriptItem::ActivityGroup(entries) => {
                let label = entries
                    .iter()
                    .map(|entry| match entry.detail.as_deref() {
                        Some(detail) => format!("{}：{detail}", entry.title),
                        None => entry.title.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join("；");
                nodes.push(node(
                    ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                    item_layout.visible_rect,
                    Role::Group,
                    &format!("活动：{label}"),
                    None,
                    Vec::new(),
                    None,
                    CursorHint::Default,
                ));
            }
            TranscriptItem::Tool(tool) => {
                let expanded = state
                    .tool_expanded
                    .get(session)
                    .and_then(|tools| tools.get(&tool.id))
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
            TranscriptItem::FileArtifact(file) => {
                let actionable = preview_available(state, session);
                nodes.push(node(
                    ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                    item_layout.visible_rect,
                    if actionable {
                        Role::Button
                    } else {
                        Role::Group
                    },
                    &format!(
                        "文件：{}，{}，{}",
                        file.summary,
                        file.path,
                        file.change_summary.as_deref().unwrap_or("无变更摘要")
                    ),
                    None,
                    if actionable {
                        vec![Action::Click, Action::Focus]
                    } else {
                        Vec::new()
                    },
                    if actionable {
                        next_order(focus_order)
                    } else {
                        None
                    },
                    if actionable {
                        CursorHint::Pointer
                    } else {
                        CursorHint::Default
                    },
                ));
            }
            TranscriptItem::Attachment(attachment) => {
                let actionable = attachment.path.is_some() && preview_available(state, session);
                let mut label = format!(
                    "附件：{}，{}",
                    attachment.display_name, attachment.media_type
                );
                if item_has_turn_divider(transcript, item_layout.index) {
                    label.push('；');
                    label.push_str(&turn_label(transcript, item_layout.index));
                }
                nodes.push(node(
                    ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                    item_layout.visible_rect,
                    if actionable {
                        Role::Button
                    } else {
                        Role::Image
                    },
                    &label,
                    None,
                    if actionable {
                        vec![Action::Click, Action::Focus]
                    } else {
                        Vec::new()
                    },
                    if actionable {
                        next_order(focus_order)
                    } else {
                        None
                    },
                    if actionable {
                        CursorHint::Pointer
                    } else {
                        CursorHint::Default
                    },
                ));
            }
            TranscriptItem::GoalProgress(goal) => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Status,
                &format!("目标：{}，{} / {}", goal.title, goal.completed, goal.total),
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            )),
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
