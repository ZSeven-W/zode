use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    AppCommand, TranscriptItem, TranscriptState, TranscriptTurnStatus, TurnSummary, ZodeAppState,
};
use zode_node_protocol::SessionLocator;

use crate::{
    stable_widget_id, visible_range, ApprovalAction, ApprovalCard, MeasurementCache, RectExt,
    ToolCard, WidgetId, ZodeTheme,
};

mod activity;
mod attachment;
mod file_card;
mod goal;
mod markdown;
mod turn;

const ESTIMATED_ITEM_HEIGHT: f32 = 72.0;
const DEFAULT_ITEM_GAP: f32 = 12.0;

pub struct ThreadTranscript;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TranscriptItemLayout {
    pub index: usize,
    pub rect: Rect,
    pub visible_rect: Rect,
}

impl ThreadTranscript {
    /// Measures only visible transcript items. Paint, hit testing and accessibility
    /// all consume this exact list of rectangles.
    pub fn visible_item_layout(
        viewport: Rect,
        transcript: &TranscriptState,
    ) -> Vec<TranscriptItemLayout> {
        Self::visible_item_layout_with_tools(viewport, transcript, &BTreeMap::new())
    }

    pub fn visible_item_layout_with_tools(
        viewport: Rect,
        transcript: &TranscriptState,
        tool_expanded: &BTreeMap<String, bool>,
    ) -> Vec<TranscriptItemLayout> {
        let mut cache = MeasurementCache::with_estimate(
            transcript.items.len(),
            ESTIMATED_ITEM_HEIGHT + DEFAULT_ITEM_GAP,
        );
        for (index, item) in transcript.items.iter().enumerate() {
            let mut measured = transcript
                .item_heights
                .get(index)
                .copied()
                .filter(|height| height.is_finite() && *height > 0.0)
                .unwrap_or_else(|| {
                    Self::estimated_item_height(item, viewport.size.x, tool_expanded)
                });
            if item_has_turn_divider(transcript, index) {
                measured += turn::HEIGHT;
            }
            let _ = cache.update(
                index,
                measured + Self::item_gap_after(&transcript.items, index),
            );
        }
        let measurements = cache.items();
        let max_offset = (cache.total_height() - viewport.size.y).max(0.0);
        let offset = if transcript.follow_tail {
            max_offset
        } else {
            transcript.scroll_offset.clamp(0.0, max_offset)
        };
        visible_range(&measurements, offset, viewport.size.y)
            .filter_map(|index| {
                let measurement = measurements[index];
                let gap = Self::item_gap_after(&transcript.items, index);
                let item_rect = Rect::xywh(
                    viewport.origin.x,
                    viewport.origin.y + measurement.top - offset,
                    viewport.size.x,
                    (measurement.bottom - measurement.top - gap).max(1.0),
                );
                let visible_rect = Self::clip_to_viewport(item_rect, viewport)?;
                Some(TranscriptItemLayout {
                    index,
                    rect: item_rect,
                    visible_rect,
                })
            })
            .collect()
    }

    pub fn scroll_command(
        session: SessionLocator,
        viewport: Rect,
        transcript: &TranscriptState,
        tool_expanded: &BTreeMap<String, bool>,
        delta: f32,
    ) -> AppCommand {
        let content_height = transcript
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut measured = transcript
                    .item_heights
                    .get(index)
                    .copied()
                    .filter(|height| height.is_finite() && *height > 0.0)
                    .unwrap_or_else(|| {
                        Self::estimated_item_height(item, viewport.size.x, tool_expanded)
                    });
                if item_has_turn_divider(transcript, index) {
                    measured += turn::HEIGHT;
                }
                measured + Self::item_gap_after(&transcript.items, index)
            })
            .sum::<f32>();
        let max_offset = (content_height - viewport.size.y).max(0.0);
        let current = if transcript.follow_tail {
            max_offset
        } else {
            transcript.scroll_offset.clamp(0.0, max_offset)
        };
        let scroll_offset = (current + delta).clamp(0.0, max_offset);
        AppCommand::SetTranscriptViewport {
            session,
            scroll_offset,
            follow_tail: max_offset - scroll_offset <= 1.0,
        }
    }

    pub fn estimated_item_height(
        item: &TranscriptItem,
        width: f32,
        tool_expanded: &BTreeMap<String, bool>,
    ) -> f32 {
        match item {
            TranscriptItem::UserText(markdown) => markdown::user_height(markdown, width),
            TranscriptItem::AssistantText(markdown) => markdown::assistant_height(markdown, width),
            TranscriptItem::Thinking(_) => 34.0,
            TranscriptItem::ActivityGroup(entries) => activity::estimated_height(entries),
            TranscriptItem::Tool(tool) => {
                let expanded = tool_expanded
                    .get(&tool.id)
                    .copied()
                    .unwrap_or_else(|| ToolCard::default_expanded(tool));
                if expanded {
                    60.0
                } else {
                    35.0
                }
            }
            TranscriptItem::FileArtifact(_) => file_card::HEIGHT,
            TranscriptItem::Attachment(_) => attachment::HEIGHT,
            TranscriptItem::GoalProgress(_) => goal::HEIGHT,
            TranscriptItem::Approval { .. } => 66.0,
            TranscriptItem::Status { .. } => 34.0,
            TranscriptItem::Error { .. } => 50.0,
        }
    }

    fn item_gap_after(items: &[TranscriptItem], index: usize) -> f32 {
        let Some(current) = items.get(index) else {
            return 0.0;
        };
        let Some(next) = items.get(index + 1) else {
            return 0.0;
        };
        if matches!(current, TranscriptItem::UserText(_)) {
            return 20.0;
        }
        if matches!(next, TranscriptItem::UserText(_)) {
            return 40.0;
        }
        if is_lightweight_activity(current) && is_lightweight_activity(next) {
            return 4.0;
        }
        if matches!(current, TranscriptItem::AssistantText(_))
            && matches!(next, TranscriptItem::FileArtifact(_))
        {
            return 16.0;
        }
        DEFAULT_ITEM_GAP
    }

    pub fn clip_to_viewport(rect: Rect, viewport: Rect) -> Option<Rect> {
        let left = rect.origin.x.max(viewport.origin.x);
        let top = rect.origin.y.max(viewport.origin.y);
        let right = (rect.origin.x + rect.size.x).min(viewport.origin.x + viewport.size.x);
        let bottom = (rect.origin.y + rect.size.y).min(viewport.origin.y + viewport.size.y);
        (right > left && bottom > top).then(|| Rect::xywh(left, top, right - left, bottom - top))
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        let session = state.current_session.as_ref()?;
        let transcript = state.transcripts.get(session)?;
        for item in &transcript.items {
            match item {
                TranscriptItem::Tool(tool) if Self::tool_widget_id(session, &tool.id) == id => {
                    let expanded = state
                        .tool_expanded
                        .get(session)
                        .and_then(|tools| tools.get(&tool.id))
                        .copied()
                        .unwrap_or_else(|| ToolCard::default_expanded(tool));
                    return Some(AppCommand::SetToolExpanded {
                        session: session.clone(),
                        tool_id: tool.id.clone(),
                        expanded: !expanded,
                    });
                }
                TranscriptItem::FileArtifact(file)
                    if preview_available(state, session)
                        && Self::semantic_widget_id(session, 0, item) == id =>
                {
                    return Some(AppCommand::PreviewWorkspaceFile {
                        session: session.clone(),
                        relative_path: file.path.clone(),
                    });
                }
                TranscriptItem::Attachment(attachment)
                    if preview_available(state, session)
                        && attachment.path.is_some()
                        && Self::semantic_widget_id(session, 0, item) == id =>
                {
                    return Some(AppCommand::PreviewWorkspaceFile {
                        session: session.clone(),
                        relative_path: attachment.path.clone().expect("path was checked"),
                    });
                }
                TranscriptItem::Approval {
                    id: approval_id, ..
                } => {
                    for action in [
                        ApprovalAction::AllowOnce,
                        ApprovalAction::AllowAlways,
                        ApprovalAction::Deny,
                    ] {
                        if Self::approval_widget_id(session, approval_id, action) == id {
                            return Some(ApprovalCard::command(approval_id.clone(), action));
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub(crate) fn semantic_widget_id(
        session: &SessionLocator,
        index: usize,
        item: &TranscriptItem,
    ) -> WidgetId {
        match item {
            TranscriptItem::Tool(tool) => Self::tool_widget_id(session, &tool.id),
            TranscriptItem::Approval { id, .. } => stable_widget_id(0x32, &(session, id)),
            TranscriptItem::UserText(_) => stable_widget_id(0x10, &(session, index)),
            TranscriptItem::AssistantText(_) => stable_widget_id(0x11, &(session, index)),
            TranscriptItem::Thinking(_) => stable_widget_id(0x12, &(session, index)),
            TranscriptItem::Status { .. } => stable_widget_id(0x13, &(session, index)),
            TranscriptItem::Error { .. } => stable_widget_id(0x14, &(session, index)),
            TranscriptItem::ActivityGroup(entries) => match entries.first() {
                Some(entry) => stable_widget_id(0x15, &(session, &entry.id)),
                None => stable_widget_id(0x15, &(session, index)),
            },
            TranscriptItem::FileArtifact(file) => stable_widget_id(0x16, &(session, &file.id)),
            TranscriptItem::Attachment(attachment) => {
                stable_widget_id(0x17, &(session, &attachment.id))
            }
            TranscriptItem::GoalProgress(goal) => stable_widget_id(0x18, &(session, &goal.id)),
        }
    }

    pub(crate) fn tool_widget_id(session: &SessionLocator, tool_id: &str) -> WidgetId {
        stable_widget_id(0x20, &(session, tool_id))
    }

    pub(crate) fn approval_widget_id(
        session: &SessionLocator,
        approval_id: &str,
        action: ApprovalAction,
    ) -> WidgetId {
        let action_key = match action {
            ApprovalAction::AllowOnce => 0_u8,
            ApprovalAction::AllowAlways => 1,
            ApprovalAction::Deny => 2,
        };
        stable_widget_id(0x31, &(session, approval_id, action_key))
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        painter.save();
        painter.clip_rect(rect);

        let Some(session) = state.current_session.as_ref() else {
            paint_empty(
                painter,
                rect,
                "开始一项任务",
                "描述你想构建、修改或探索的内容。",
                theme,
            );
            painter.restore();
            return;
        };
        let Some(transcript) = state.transcripts.get(session) else {
            paint_empty(
                painter,
                rect,
                "任务已准备好",
                "消息与工具活动会显示在这里。",
                theme,
            );
            painter.restore();
            return;
        };
        if transcript.items.is_empty() {
            paint_empty(
                painter,
                rect,
                "任务已准备好",
                "消息与工具活动会显示在这里。",
                theme,
            );
            painter.restore();
            return;
        }

        let empty = BTreeMap::new();
        let tool_expanded = state.tool_expanded.get(session).unwrap_or(&empty);
        paint_items(painter, rect, transcript, tool_expanded, theme);
        painter.restore();
    }
}

pub(crate) fn preview_available(state: &ZodeAppState, session: &SessionLocator) -> bool {
    state
        .available_workspace_for_session(session)
        .is_some_and(|workspace| workspace.as_str().starts_with("file://"))
}

fn paint_items(
    painter: &mut dyn Painter,
    rect: Rect,
    transcript: &TranscriptState,
    tool_expanded: &BTreeMap<String, bool>,
    theme: &ZodeTheme,
) {
    for item_layout in
        ThreadTranscript::visible_item_layout_with_tools(rect, transcript, tool_expanded)
    {
        paint_item(
            painter,
            item_layout.rect,
            &transcript.items[item_layout.index],
            transcript,
            item_layout.index,
            tool_expanded,
            theme,
        );
    }
}

fn paint_item(
    painter: &mut dyn Painter,
    rect: Rect,
    item: &TranscriptItem,
    transcript: &TranscriptState,
    index: usize,
    tool_expanded: &BTreeMap<String, bool>,
    theme: &ZodeTheme,
) {
    let has_turn_divider = item_has_turn_divider(transcript, index);
    let content_height = if has_turn_divider {
        (rect.size.y - turn::HEIGHT).max(1.0)
    } else {
        rect.size.y
    };
    let content_rect = Rect::xywh(rect.origin.x, rect.origin.y, rect.size.x, content_height);
    match item {
        TranscriptItem::UserText(text) => {
            markdown::paint_user(painter, content_rect, text, theme);
        }
        TranscriptItem::AssistantText(text) => {
            markdown::paint_assistant(painter, content_rect, text, theme)
        }
        TranscriptItem::Thinking(text) => {
            activity::paint_thinking(painter, content_rect, text, theme)
        }
        TranscriptItem::ActivityGroup(entries) => {
            activity::paint_group(painter, content_rect, entries, theme)
        }
        TranscriptItem::Tool(tool) => ToolCard::paint(
            painter,
            content_rect,
            tool,
            tool_expanded
                .get(&tool.id)
                .copied()
                .unwrap_or_else(|| ToolCard::default_expanded(tool)),
            theme,
        ),
        TranscriptItem::FileArtifact(file) => file_card::paint(painter, content_rect, file, theme),
        TranscriptItem::Attachment(attachment) => {
            attachment::paint(painter, content_rect, attachment, theme)
        }
        TranscriptItem::GoalProgress(goal) => goal::paint(painter, content_rect, goal, theme),
        TranscriptItem::Approval { tool, .. } => {
            ApprovalCard::paint(painter, content_rect, tool, theme)
        }
        TranscriptItem::Status { message, .. } => {
            activity::paint_status(painter, content_rect, message, theme)
        }
        TranscriptItem::Error { message, retryable } => {
            activity::paint_error(painter, content_rect, message, *retryable, theme)
        }
    }
    if has_turn_divider {
        turn::paint(
            painter,
            Rect::xywh(
                rect.origin.x,
                content_rect.max_y(),
                rect.size.x,
                turn::HEIGHT,
            ),
            &turn_label(transcript, index),
            theme,
        );
    }
}

fn is_lightweight_activity(item: &TranscriptItem) -> bool {
    matches!(
        item,
        TranscriptItem::Thinking(_)
            | TranscriptItem::ActivityGroup(_)
            | TranscriptItem::Tool(_)
            | TranscriptItem::Status { .. }
    )
}

pub(crate) fn turn_label(transcript: &TranscriptState, item_index: usize) -> String {
    if let Some(turn) = turn_for_divider_after(transcript, item_index) {
        let status = match turn.status {
            TranscriptTurnStatus::Running => "正在处理",
            TranscriptTurnStatus::Completed | TranscriptTurnStatus::Restored => "已处理",
            TranscriptTurnStatus::Interrupted => "已中断",
            TranscriptTurnStatus::Failed => "未完成",
        };
        return match turn.elapsed_at(Instant::now()) {
            Some(elapsed) => format!("{status} {}", format_elapsed(elapsed)),
            None => status.to_string(),
        };
    }

    let is_latest_user = !transcript.items[item_index.saturating_add(1)..]
        .iter()
        .any(|item| matches!(item, TranscriptItem::UserText(_)));
    if transcript.busy && is_latest_user {
        "正在处理".to_string()
    } else {
        "已处理".to_string()
    }
}

pub(crate) fn item_has_turn_divider(transcript: &TranscriptState, item_index: usize) -> bool {
    if transcript.turns.is_empty() {
        return matches!(
            transcript.items.get(item_index),
            Some(TranscriptItem::UserText(_))
        );
    }
    turn_for_divider_after(transcript, item_index).is_some()
}

fn turn_for_divider_after(transcript: &TranscriptState, item_index: usize) -> Option<&TurnSummary> {
    let boundary = item_index.checked_add(1)?;
    let item_count = transcript.items.len();
    let turn_index = transcript
        .turns
        .partition_point(|turn| turn.response_item_index.min(item_count) < boundary);
    let turn = transcript.turns.get(turn_index)?;
    let start = turn.start_item_index.min(item_count);
    let response = turn.response_item_index.min(item_count);
    (start < response && response == boundary).then_some(turn)
}

fn format_elapsed(elapsed: Duration) -> String {
    let total_seconds = elapsed.as_secs();
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn paint_empty(
    painter: &mut dyn Painter,
    rect: Rect,
    headline: &str,
    detail: &str,
    theme: &ZodeTheme,
) {
    draw_text(
        painter,
        headline,
        Point2D::new(rect.origin.x, rect.origin.y + 30.0),
        17.0,
        600,
        theme.tokens.foreground,
    );
    draw_text(
        painter,
        detail,
        Point2D::new(rect.origin.x, rect.origin.y + 56.0),
        13.0,
        400,
        theme.tokens.muted_foreground,
    );
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    size: f32,
    weight: u16,
    color: Color,
) {
    let layout = TextLayout::single_run(text, "system-ui", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(&layout, origin);
}
