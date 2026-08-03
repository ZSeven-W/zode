use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    AppCommand, MessageFeedback, TranscriptFindState, TranscriptItem, TranscriptState,
    TranscriptTurnStatus, TurnSummary, ZodeAppState,
};
use zode_node_protocol::SessionLocator;

use crate::{
    stable_widget_id, visible_range, ApprovalAction, ApprovalCard, MeasuredItem, RectExt,
    SemanticIcon, ToolCard, WidgetId, ZodeTheme,
};
use message_actions::MessageAction;

mod activity;
#[path = "anchor-rail.rs"]
mod anchor_rail;
mod attachment;
mod file_card;
#[path = "find-highlight.rs"]
mod find_highlight;
mod goal;
mod image;
mod markdown;
#[path = "message-actions.rs"]
pub(crate) mod message_actions;
#[path = "relative-time.rs"]
pub(crate) mod relative_time;
#[path = "subagent-chip.rs"]
mod subagent_chip;
mod timestamp;
mod turn;

pub use anchor_rail::{AnchorRail, AnchorTick};
pub use image::{
    corrected_card_height, image_source_id, TranscriptImageBytes, TranscriptImageSource,
};
pub(crate) use subagent_chip::accessibility_label as subagent_chip_label;

const DEFAULT_ITEM_GAP: f32 = 12.0;

/// Floating "back to bottom" button. A fixed small-integer id (like
/// `TERMINAL_ID`) rather than a `stable_widget_id` hash, since exactly one
/// instance exists regardless of session.
pub const TRANSCRIPT_BACK_TO_BOTTOM_ID: WidgetId = WidgetId(250);

const BACK_TO_BOTTOM_DIAMETER: f32 = 36.0;
const BACK_TO_BOTTOM_MARGIN: f32 = 16.0;

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
        let cache = transcript.layout_offsets(viewport.size.x, || {
            Self::compute_offsets(transcript, viewport.size.x, tool_expanded)
        });
        let measurements: Vec<MeasuredItem> = cache
            .offsets
            .iter()
            .map(|&(top, bottom)| MeasuredItem::new(top, bottom))
            .collect();
        let max_offset = (cache.total_height - viewport.size.y).max(0.0);
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
            .layout_offsets(viewport.size.x, || {
                Self::compute_offsets(transcript, viewport.size.x, tool_expanded)
            })
            .total_height;
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

    /// Recomputes cumulative (top, bottom) offsets for every item, plus the
    /// total content height. Only runs on a cache miss - see
    /// `TranscriptState::layout_offsets`, which both `visible_item_layout_with_tools`
    /// and `scroll_command` (and, transitively, both the paint path and the
    /// accessibility-tree builder) call to memoize this across calls and frames.
    fn compute_offsets(
        transcript: &TranscriptState,
        width: f32,
        tool_expanded: &BTreeMap<String, bool>,
    ) -> (Vec<(f32, f32)>, f32) {
        let mut offsets = Vec::with_capacity(transcript.items.len());
        let mut top = 0.0;
        for (index, item) in transcript.items.iter().enumerate() {
            let mut measured = transcript
                .item_heights
                .get(index)
                .copied()
                .filter(|height| height.is_finite() && *height > 0.0)
                .unwrap_or_else(|| Self::estimated_item_height(item, width, tool_expanded));
            if item_has_turn_divider(transcript, index) {
                measured += turn::HEIGHT;
            }
            let bottom = top + measured + Self::item_gap_after(&transcript.items, index);
            offsets.push((top, bottom));
            top = bottom;
        }
        (offsets, top)
    }

    pub fn estimated_item_height(
        item: &TranscriptItem,
        width: f32,
        tool_expanded: &BTreeMap<String, bool>,
    ) -> f32 {
        match item {
            TranscriptItem::UserText { text, .. } => markdown::user_height(text, width),
            TranscriptItem::AssistantText { text, .. } => markdown::assistant_height(text, width),
            TranscriptItem::Thinking(_) => 34.0,
            TranscriptItem::ActivityGroup(entries) => activity::estimated_height(entries),
            TranscriptItem::Tool(tool) => {
                let expanded = tool_expanded
                    .get(&tool.id)
                    .copied()
                    .unwrap_or_else(|| ToolCard::default_expanded(tool));
                // The permission-pending hint (computer-use TCC retry) is
                // always visible, not gated by `expanded` - it needs its own
                // reserved line even in the default collapsed state. Scoped
                // to the computer tools (see `ToolCard::shows_permission_hint`)
                // so ordinary tool `detail` does not inflate collapsed height.
                let has_pending_hint = ToolCard::shows_permission_hint(tool);
                if expanded || has_pending_hint {
                    60.0
                } else {
                    35.0
                }
            }
            TranscriptItem::FileArtifact(_) => file_card::HEIGHT,
            TranscriptItem::Attachment(_) => attachment::HEIGHT,
            TranscriptItem::Image(item) => image::estimated_height(item, width),
            TranscriptItem::GoalProgress(_) => goal::HEIGHT,
            TranscriptItem::SubagentChip(_) => subagent_chip::HEIGHT,
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
        if matches!(current, TranscriptItem::UserText { .. }) {
            return 20.0;
        }
        if matches!(next, TranscriptItem::UserText { .. }) {
            return 40.0;
        }
        if is_lightweight_activity(current) && is_lightweight_activity(next) {
            return 4.0;
        }
        if matches!(current, TranscriptItem::AssistantText { .. })
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
        if id == TRANSCRIPT_BACK_TO_BOTTOM_ID {
            return None; // needs viewport geometry; see `back_to_bottom_command`.
        }
        for (index, item) in transcript.items.iter().enumerate() {
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
                TranscriptItem::Image(image)
                    if Self::semantic_widget_id(session, 0, item) == id =>
                {
                    return Some(AppCommand::OpenLightbox {
                        session: session.clone(),
                        item_id: image.id.clone(),
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
                TranscriptItem::UserText { text, .. }
                    if Self::user_copy_button_id(session, index) == id =>
                {
                    return Some(AppCommand::CopyText(text.clone()));
                }
                TranscriptItem::AssistantText { text, feedback, .. } => {
                    if Self::assistant_copy_button_id(session, index) == id {
                        return Some(AppCommand::CopyText(text.clone()));
                    }
                    if Self::assistant_share_button_id(session, index) == id {
                        return Some(AppCommand::CopyText(share_payload(text)));
                    }
                    if Self::assistant_thumbs_up_id(session, index) == id {
                        let next = if *feedback == MessageFeedback::Up {
                            MessageFeedback::None
                        } else {
                            MessageFeedback::Up
                        };
                        return Some(AppCommand::SetMessageFeedback {
                            session: session.clone(),
                            index,
                            feedback: next,
                        });
                    }
                    if Self::assistant_thumbs_down_id(session, index) == id {
                        let next = if *feedback == MessageFeedback::Down {
                            MessageFeedback::None
                        } else {
                            MessageFeedback::Down
                        };
                        return Some(AppCommand::SetMessageFeedback {
                            session: session.clone(),
                            index,
                            feedback: next,
                        });
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Separate from `command_for_widget` because scrolling to the bottom
    /// needs the transcript's measured content height, which requires the
    /// viewport rect and tool-expanded overrides that `command_for_widget`
    /// (matched purely by widget id, like every other transcript control)
    /// does not have access to.
    pub fn back_to_bottom_command(
        state: &ZodeAppState,
        viewport: Rect,
        tool_expanded: &BTreeMap<String, bool>,
    ) -> Option<AppCommand> {
        let session = state.current_session.as_ref()?;
        let transcript = state.transcripts.get(session)?;
        Some(Self::scroll_to_bottom_command(
            session.clone(),
            viewport,
            transcript,
            tool_expanded,
        ))
    }

    /// Geometry for the floating back-to-bottom button, or `None` when it
    /// should not be shown. Shared by paint, hit testing (via
    /// `command_for_widget`'s caller) and the accessibility tree so all
    /// three agree on exactly when the button exists.
    pub fn back_to_bottom_layout(
        viewport: Rect,
        transcript: &TranscriptState,
        tool_expanded: &BTreeMap<String, bool>,
    ) -> Option<Rect> {
        if viewport.size.x <= 0.0 || viewport.size.y <= 0.0 {
            return None;
        }
        let cache = transcript.layout_offsets(viewport.size.x, || {
            Self::compute_offsets(transcript, viewport.size.x, tool_expanded)
        });
        let max_offset = (cache.total_height - viewport.size.y).max(0.0);
        let offset = if transcript.follow_tail {
            max_offset
        } else {
            transcript.scroll_offset.clamp(0.0, max_offset)
        };
        let scrolled_up = max_offset - offset;
        let shown = !transcript.follow_tail && scrolled_up > viewport.size.y;
        shown.then(|| {
            let center_x = viewport.origin.x + viewport.size.x / 2.0;
            let bottom = viewport.max_y() - BACK_TO_BOTTOM_MARGIN;
            Rect::xywh(
                center_x - BACK_TO_BOTTOM_DIAMETER / 2.0,
                bottom - BACK_TO_BOTTOM_DIAMETER,
                BACK_TO_BOTTOM_DIAMETER,
                BACK_TO_BOTTOM_DIAMETER,
            )
        })
    }

    pub(crate) fn semantic_widget_id(
        session: &SessionLocator,
        index: usize,
        item: &TranscriptItem,
    ) -> WidgetId {
        match item {
            TranscriptItem::Tool(tool) => Self::tool_widget_id(session, &tool.id),
            TranscriptItem::Approval { id, .. } => stable_widget_id(0x32, &(session, id)),
            TranscriptItem::UserText { .. } => stable_widget_id(0x10, &(session, index)),
            TranscriptItem::AssistantText { .. } => stable_widget_id(0x11, &(session, index)),
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
            TranscriptItem::SubagentChip(chip) => {
                stable_widget_id(0x1a, &(session, &chip.agent_id, chip.phase.key()))
            }
            TranscriptItem::Image(item) => stable_widget_id(0x19, &(session, &item.id)),
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

    /// Widget id for one user message's copy button. A distinct namespace
    /// from `semantic_widget_id`'s own `0x10` (the whole-bubble hover/focus
    /// target for that same message) so the two never collide.
    pub(crate) fn user_copy_button_id(session: &SessionLocator, index: usize) -> WidgetId {
        stable_widget_id(0x33, &(session, index))
    }

    pub(crate) fn assistant_copy_button_id(session: &SessionLocator, index: usize) -> WidgetId {
        stable_widget_id(0x34, &(session, index))
    }

    pub(crate) fn assistant_thumbs_up_id(session: &SessionLocator, index: usize) -> WidgetId {
        stable_widget_id(0x35, &(session, index))
    }

    pub(crate) fn assistant_thumbs_down_id(session: &SessionLocator, index: usize) -> WidgetId {
        stable_widget_id(0x36, &(session, index))
    }

    pub(crate) fn assistant_share_button_id(session: &SessionLocator, index: usize) -> WidgetId {
        stable_widget_id(0x37, &(session, index))
    }

    pub(crate) fn assistant_button_id(
        session: &SessionLocator,
        index: usize,
        action: MessageAction,
    ) -> WidgetId {
        match action {
            MessageAction::Copy => Self::assistant_copy_button_id(session, index),
            MessageAction::ThumbsUp => Self::assistant_thumbs_up_id(session, index),
            MessageAction::ThumbsDown => Self::assistant_thumbs_down_id(session, index),
            MessageAction::Share => Self::assistant_share_button_id(session, index),
        }
    }

    /// Jumps to the bottom of the transcript and re-engages follow-tail,
    /// mirroring `scroll_command`'s cache-friendly `layout_offsets` call so
    /// this shares the same memoized layout within the frame.
    pub fn scroll_to_bottom_command(
        session: SessionLocator,
        viewport: Rect,
        transcript: &TranscriptState,
        tool_expanded: &BTreeMap<String, bool>,
    ) -> AppCommand {
        let content_height = transcript
            .layout_offsets(viewport.size.x, || {
                Self::compute_offsets(transcript, viewport.size.x, tool_expanded)
            })
            .total_height;
        let max_offset = (content_height - viewport.size.y).max(0.0);
        AppCommand::SetTranscriptViewport {
            session,
            scroll_offset: max_offset,
            follow_tail: true,
        }
    }

    /// The item's content rect (its full rect minus a trailing turn-divider,
    /// when it has one). Shared by paint and the accessibility-tree builder
    /// so a message's action-row buttons are hit-tested at exactly the rect
    /// they were painted in.
    pub(crate) fn content_rect_for_item(
        rect: Rect,
        transcript: &TranscriptState,
        index: usize,
    ) -> Rect {
        let content_height = if item_has_turn_divider(transcript, index) {
            (rect.size.y - turn::HEIGHT).max(1.0)
        } else {
            rect.size.y
        };
        Rect::xywh(rect.origin.x, rect.origin.y, rect.size.x, content_height)
    }

    pub(crate) fn user_copy_button_rect(content_rect: Rect, text: &str) -> Rect {
        let bubble = markdown::user_bubble_rect(content_rect, text);
        message_actions::user_copy_button_rect(message_actions::user_row_rect(bubble))
    }

    pub(crate) fn assistant_button_rects(
        content_rect: Rect,
        text: &str,
    ) -> [message_actions::ActionButtonLayout; 4] {
        let text_bottom =
            content_rect.origin.y + markdown::assistant_content_height(text, content_rect.size.x);
        message_actions::assistant_button_layout(message_actions::assistant_row_rect(
            content_rect,
            text_bottom,
        ))
    }

    /// Convenience wrapper with no outer primary-surface rect available, so
    /// the anchor rail (which needs the gutter between that rect and `rect`
    /// to decide whether it fits) never paints here. Every real caller goes
    /// through `paint_with_hovered` with both rects instead.
    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        Self::paint_with_hovered(painter, rect, rect, state, None, theme)
    }

    pub fn paint_with_hovered(
        painter: &mut dyn Painter,
        rect: Rect,
        main_rect: Rect,
        state: &ZodeAppState,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        Self::paint_with_hovered_and_images(painter, rect, main_rect, state, hovered, None, theme)
    }

    /// Same as `paint_with_hovered`, plus an optional host-supplied lookup
    /// for already-decoded `TranscriptItem::Image` bytes (see
    /// `image::TranscriptImageSource`). Kept as a separate entry point,
    /// rather than adding the parameter to `paint_with_hovered` itself, so
    /// every existing caller (tests included) keeps compiling unchanged; a
    /// missing/`None` provider just paints the icon-tile placeholder for any
    /// `Image` item, exactly like `paint_with_hovered` did before this type
    /// existed.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_with_hovered_and_images(
        painter: &mut dyn Painter,
        rect: Rect,
        main_rect: Rect,
        state: &ZodeAppState,
        hovered: Option<WidgetId>,
        image_source: Option<&dyn image::TranscriptImageSource>,
        theme: &ZodeTheme,
    ) {
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
        // Highlights only exist while this session's find bar is open, so a
        // closed bar costs nothing - `TranscriptFindState::matches` is never
        // consulted and the band painting is skipped entirely.
        let find = state
            .presentation
            .sessions
            .get(session)
            .map(|presentation| &presentation.find)
            .filter(|find| find.open);
        paint_items(
            painter,
            rect,
            session,
            transcript,
            tool_expanded,
            hovered,
            image_source,
            find,
            theme,
        );
        if let Some(button) = Self::back_to_bottom_layout(rect, transcript, tool_expanded) {
            paint_back_to_bottom(
                painter,
                button,
                hovered == Some(TRANSCRIPT_BACK_TO_BOTTOM_ID),
                theme,
            );
        }
        painter.restore();

        // The rail paints in the gutter to the left of `rect`, so it needs
        // its own clip scope - `rect` above is clipped tightly to the
        // transcript viewport itself and would cut the rail off entirely.
        painter.save();
        painter.clip_rect(Rect::xywh(
            main_rect.min_x(),
            rect.min_y(),
            rect.max_x() - main_rect.min_x(),
            rect.size.y,
        ));
        AnchorRail::paint(
            painter,
            rect,
            main_rect,
            transcript,
            session,
            tool_expanded,
            hovered,
            theme,
        );
        painter.restore();
    }
}

pub(crate) fn preview_available(state: &ZodeAppState, session: &SessionLocator) -> bool {
    state
        .available_workspace_for_session(session)
        .is_some_and(|workspace| workspace.as_str().starts_with("file://"))
}

#[allow(clippy::too_many_arguments)]
fn paint_items(
    painter: &mut dyn Painter,
    rect: Rect,
    session: &SessionLocator,
    transcript: &TranscriptState,
    tool_expanded: &BTreeMap<String, bool>,
    hovered: Option<WidgetId>,
    image_source: Option<&dyn image::TranscriptImageSource>,
    find: Option<&TranscriptFindState>,
    theme: &ZodeTheme,
) {
    let now_ms = timestamp::now_ms();
    let last_assistant_index = last_assistant_index(transcript);
    for item_layout in
        ThreadTranscript::visible_item_layout_with_tools(rect, transcript, tool_expanded)
    {
        if let Some(find) = find {
            find_highlight::paint(
                painter,
                item_layout.rect,
                transcript,
                find,
                item_layout.index,
                theme,
            );
        }
        paint_item(
            painter,
            item_layout.rect,
            &transcript.items[item_layout.index],
            transcript,
            session,
            item_layout.index,
            last_assistant_index,
            tool_expanded,
            hovered,
            image_source,
            now_ms,
            theme,
        );
    }
}

pub(crate) fn last_assistant_index(transcript: &TranscriptState) -> Option<usize> {
    transcript
        .items
        .iter()
        .rposition(|item| matches!(item, TranscriptItem::AssistantText { .. }))
}

/// Share is intentionally identical to copy today (spec: "equivalent to
/// copy for now but via a distinct handler so it can later diverge") - kept
/// as its own function/command site rather than reusing the copy path so a
/// future markdown export or footer can change only this one spot.
fn share_payload(text: &str) -> String {
    text.to_string()
}

#[allow(clippy::too_many_arguments)]
fn paint_item(
    painter: &mut dyn Painter,
    rect: Rect,
    item: &TranscriptItem,
    transcript: &TranscriptState,
    session: &SessionLocator,
    index: usize,
    last_assistant_index: Option<usize>,
    tool_expanded: &BTreeMap<String, bool>,
    hovered: Option<WidgetId>,
    image_source: Option<&dyn image::TranscriptImageSource>,
    now_ms: i64,
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
        TranscriptItem::UserText { text, timestamp_ms } => {
            markdown::paint_user(painter, content_rect, text, theme);
            let bubble = markdown::user_bubble_rect(content_rect, text);
            let hover_id = ThreadTranscript::semantic_widget_id(session, index, item);
            if hovered == Some(hover_id) {
                let copy_id = ThreadTranscript::user_copy_button_id(session, index);
                let label = timestamp_ms.map(|ms| timestamp::format_timestamp(ms, now_ms));
                message_actions::paint_user_row(
                    painter,
                    bubble,
                    label.as_deref(),
                    hovered == Some(copy_id),
                    theme,
                );
            }
        }
        TranscriptItem::AssistantText {
            text,
            timestamp_ms,
            feedback,
        } => {
            markdown::paint_assistant(painter, content_rect, text, theme);
            let is_last = last_assistant_index == Some(index);
            let hover_id = ThreadTranscript::semantic_widget_id(session, index, item);
            if is_last || hovered == Some(hover_id) {
                let text_bottom = content_rect.origin.y
                    + markdown::assistant_content_height(text, content_rect.size.x);
                let label = timestamp_ms.map(|ms| timestamp::format_timestamp(ms, now_ms));
                let hovered_action = [
                    MessageAction::Copy,
                    MessageAction::ThumbsUp,
                    MessageAction::ThumbsDown,
                    MessageAction::Share,
                ]
                .into_iter()
                .find(|action| {
                    hovered
                        == Some(ThreadTranscript::assistant_button_id(
                            session, index, *action,
                        ))
                });
                message_actions::paint_assistant_row(
                    painter,
                    content_rect,
                    text_bottom,
                    *feedback,
                    label.as_deref(),
                    hovered_action,
                    theme,
                );
            }
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
        TranscriptItem::Image(item) => {
            image::paint(painter, content_rect, item, image_source, theme)
        }
        TranscriptItem::GoalProgress(goal) => goal::paint(painter, content_rect, goal, theme),
        TranscriptItem::SubagentChip(chip) => {
            subagent_chip::paint(painter, content_rect, chip, theme)
        }
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
            | TranscriptItem::SubagentChip(_)
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
        .any(|item| matches!(item, TranscriptItem::UserText { .. }));
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
            Some(TranscriptItem::UserText { .. })
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

/// Floating circular down-arrow button. Geometry always comes from
/// `ThreadTranscript::back_to_bottom_layout`, the single source of truth
/// paint, hit testing and the accessibility tree all share.
fn paint_back_to_bottom(painter: &mut dyn Painter, rect: Rect, hovered: bool, theme: &ZodeTheme) {
    let radius = rect.size.x / 2.0;
    painter.fill_round_rect(
        rect,
        radius,
        if hovered {
            theme.tokens.accent
        } else {
            theme.tokens.card
        },
    );
    painter.stroke_round_rect(rect, radius, theme.tokens.border, 1.0);
    let icon_size = 16.0;
    painter.stroke_svg_path(
        SemanticIcon::ChevronDown.path(),
        Point2D::new(
            rect.origin.x + (rect.size.x - icon_size) / 2.0,
            rect.origin.y + (rect.size.y - icon_size) / 2.0,
        ),
        icon_size,
        theme.tokens.foreground,
        SemanticIcon::ChevronDown.stroke_width(),
    );
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
