use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use jian_widgets::Rect;
use std::collections::BTreeMap;
use zode_app_model::{MessageFeedback, TranscriptItem, ZodeAppState};

use crate::widgets::transcript::{
    item_has_turn_divider, preview_available, subagent_chip_label, turn_label,
    TRANSCRIPT_BACK_TO_BOTTOM_ID,
};
use crate::{
    AnchorRail, ApprovalCard, ThreadTranscript, ToolCard, TranscriptFindBar, WorkspaceLayout,
    TRANSCRIPT_FIND_CLOSE_ID, TRANSCRIPT_FIND_INPUT_ID, TRANSCRIPT_FIND_NEXT_ID,
    TRANSCRIPT_FIND_PREVIOUS_ID, TRANSCRIPT_FIND_SURFACE_ID,
};

use super::{next_order, node, InteractionNode};

/// Copy/reaction/share buttons are always exposed in the accessibility tree
/// (an accesskit consumer can still reach them structurally without
/// sequential Tab order) but deliberately kept out of `focus_order`: they
/// only ever *paint* visibly on hover or (for the newest assistant message)
/// persistently, and giving one to every transcript message would multiply
/// the app's Tab stop count many times over for a cosmetic reveal action.
/// Contrast the 3 `ApprovalCard` buttons below, which do take `focus_order`
/// - those are rare (one pending approval at a time) and load-bearing.
fn action_button_node(id: crate::WidgetId, rect: Rect, label: String) -> InteractionNode {
    node(
        id,
        rect,
        Role::Button,
        &label,
        None,
        vec![Action::Click, Action::Focus],
        None,
        CursorHint::Pointer,
    )
}

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
            TranscriptItem::UserText { text, .. } => {
                let label = if item_has_turn_divider(transcript, item_layout.index) {
                    format!("你：{text}；{}", turn_label(transcript, item_layout.index))
                } else {
                    format!("你：{text}")
                };
                // `Action::Focus` (no `focus_order`) makes this whole bubble
                // a hover/hit-test target - see `ThreadTranscript::paint_with_hovered`
                // - without adding a Tab stop for every message in the
                // transcript. The copy button below is the same rect
                // `paint_item` paints it into.
                nodes.push(node(
                    ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                    item_layout.visible_rect,
                    Role::Paragraph,
                    &label,
                    None,
                    vec![Action::Focus],
                    None,
                    CursorHint::Default,
                ));
                let content_rect = ThreadTranscript::content_rect_for_item(
                    item_layout.rect,
                    transcript,
                    item_layout.index,
                );
                let button_rect = ThreadTranscript::user_copy_button_rect(content_rect, text);
                if let Some(visible) =
                    ThreadTranscript::clip_to_viewport(button_rect, layout.transcript)
                {
                    nodes.push(action_button_node(
                        ThreadTranscript::user_copy_button_id(session, item_layout.index),
                        visible,
                        "复制消息".to_string(),
                    ));
                }
            }
            TranscriptItem::AssistantText { text, feedback, .. } => {
                nodes.push(node(
                    ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                    item_layout.visible_rect,
                    Role::Paragraph,
                    text,
                    None,
                    vec![Action::Focus],
                    None,
                    CursorHint::Default,
                ));
                let content_rect = ThreadTranscript::content_rect_for_item(
                    item_layout.rect,
                    transcript,
                    item_layout.index,
                );
                let buttons = ThreadTranscript::assistant_button_rects(content_rect, text);
                for button in buttons {
                    let Some(visible) =
                        ThreadTranscript::clip_to_viewport(button.rect, layout.transcript)
                    else {
                        continue;
                    };
                    let id = ThreadTranscript::assistant_button_id(
                        session,
                        item_layout.index,
                        button.action,
                    );
                    let label = match button.action {
                        crate::widgets::transcript::message_actions::MessageAction::Copy => {
                            "复制消息".to_string()
                        }
                        crate::widgets::transcript::message_actions::MessageAction::ThumbsUp => {
                            if *feedback == MessageFeedback::Up {
                                "取消赞".to_string()
                            } else {
                                "赞".to_string()
                            }
                        }
                        crate::widgets::transcript::message_actions::MessageAction::ThumbsDown => {
                            if *feedback == MessageFeedback::Down {
                                "取消踩".to_string()
                            } else {
                                "踩".to_string()
                            }
                        }
                        crate::widgets::transcript::message_actions::MessageAction::Share => {
                            "分享消息".to_string()
                        }
                    };
                    let mut button_node = action_button_node(id, visible, label);
                    if matches!(
                        button.action,
                        crate::widgets::transcript::message_actions::MessageAction::ThumbsUp
                            | crate::widgets::transcript::message_actions::MessageAction::ThumbsDown
                    ) {
                        let active = matches!(
                            (button.action, feedback),
                            (
                                crate::widgets::transcript::message_actions::MessageAction::ThumbsUp,
                                MessageFeedback::Up
                            ) | (
                                crate::widgets::transcript::message_actions::MessageAction::ThumbsDown,
                                MessageFeedback::Down
                            )
                        );
                        button_node.toggled = Some(Toggled::from(active));
                    }
                    nodes.push(button_node);
                }
            }
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
            TranscriptItem::Image(image) => {
                let mut label = format!("图片：{}", image.path);
                if let (Some(width), Some(height)) = (image.width, image.height) {
                    label.push_str(&format!("，{width}×{height}"));
                }
                nodes.push(node(
                    ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                    item_layout.visible_rect,
                    Role::Button,
                    &label,
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
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
            TranscriptItem::SubagentChip(chip) => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Status,
                &subagent_chip_label(chip),
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

    // Unlike the per-message action-row buttons above, this is at most one
    // node total, so it takes a real `focus_order` like any other standalone
    // control (e.g. `ApprovalCard`'s buttons).
    if let Some(button_rect) =
        ThreadTranscript::back_to_bottom_layout(layout.transcript, transcript, tool_expanded)
    {
        nodes.push(node(
            TRANSCRIPT_BACK_TO_BOTTOM_ID,
            button_rect,
            Role::Button,
            "回到底部",
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }

    // Turn anchors, like the ticks/hover-card AnchorRail paints, come from
    // `AnchorRail::layout`, which already applies the same hide rule (not
    // enough gutter between `layout.primary_surface` and `layout.transcript`)
    // - hidden means an empty list, so nothing below registers. Each anchor
    // is a real jump target (not a hover-only reveal control like the
    // per-message action-row buttons above), so unlike those it takes a
    // real `focus_order`.
    for tick in AnchorRail::layout(
        layout.transcript,
        layout.primary_surface,
        transcript,
        session,
        tool_expanded,
    ) {
        nodes.push(node(
            tick.widget_id,
            tick.hit_rect,
            Role::Button,
            &format!("前往：{}", tick.anchor.user_excerpt),
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }

    append_find_bar_nodes(nodes, layout, focus_order, state);
}

/// The find bar floats over the transcript, so its nodes must come *after*
/// every item node above: `WorkspaceSnapshot::hit_test` scans in reverse and
/// takes the last match, which is what keeps a click on the bar from falling
/// through to the message underneath it.
fn append_find_bar_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let Some(bar) = TranscriptFindBar::layout(layout.transcript, state) else {
        return;
    };
    // Blocks blank-surface pointer hits from reaching the transcript behind
    // the bar, the same job `GLOBAL_SEARCH_SURFACE_ID` does for its overlay.
    // The match counter has no node of its own - it is not interactive, and
    // folding it into this group's name is how a screen reader hears it
    // without an extra stop.
    nodes.push(node(
        TRANSCRIPT_FIND_SURFACE_ID,
        bar.surface,
        Role::Group,
        &format!("在对话中查找，{}", bar.counter_label),
        None,
        vec![Action::Focus],
        None,
        CursorHint::Default,
    ));
    nodes.push(node(
        TRANSCRIPT_FIND_INPUT_ID,
        bar.input,
        Role::SearchInput,
        "在对话中查找",
        Some(
            state
                .current_session_presentation()
                .map(|presentation| presentation.find.query.clone())
                .unwrap_or_default(),
        ),
        vec![Action::Focus, Action::SetValue],
        next_order(focus_order),
        CursorHint::Text,
    ));
    for (id, rect, label) in [
        (TRANSCRIPT_FIND_PREVIOUS_ID, bar.previous, "上一个匹配"),
        (TRANSCRIPT_FIND_NEXT_ID, bar.next, "下一个匹配"),
        (TRANSCRIPT_FIND_CLOSE_ID, bar.close, "关闭查找"),
    ] {
        // A stepper with nothing to step through registers as a plain label:
        // no actions means `hit_test` skips it, so it cannot be clicked or
        // focused into while it paints disabled.
        let enabled = bar.navigable || id == TRANSCRIPT_FIND_CLOSE_ID;
        nodes.push(node(
            id,
            rect,
            if enabled { Role::Button } else { Role::Label },
            label,
            None,
            if enabled {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            enabled.then(|| next_order(focus_order)).flatten(),
            if enabled {
                CursorHint::Pointer
            } else {
                CursorHint::Default
            },
        ));
    }
}
