use std::collections::BTreeMap;

use jian_widgets::{
    components::markdown::{parse_blocks, parse_inline, wrap_runs, MdBlock, MdRun},
    Color, Painter, Point2D, Rect, TextLayout,
};
use zode_app_model::{AppCommand, TranscriptItem, TranscriptState, ZodeAppState};
use zode_node_protocol::SessionLocator;

use crate::{
    stable_widget_id, visible_range, ApprovalAction, ApprovalCard, MeasurementCache, ToolCard,
    WidgetId, ZodeTheme,
};

const ESTIMATED_ITEM_HEIGHT: f32 = 72.0;
const ITEM_GAP: f32 = 12.0;
const MARKDOWN_LIMIT: usize = 50_000;

pub struct ThreadTranscript;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TranscriptItemLayout {
    pub index: usize,
    pub rect: Rect,
    pub visible_rect: Rect,
}

impl ThreadTranscript {
    /// Measures only the visible transcript items. Paint, hit testing and the
    /// accessibility tree all consume this exact list of rectangles.
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
            ESTIMATED_ITEM_HEIGHT + ITEM_GAP,
        );
        for (index, item) in transcript.items.iter().enumerate() {
            let measured = transcript
                .item_heights
                .get(index)
                .copied()
                .filter(|height| height.is_finite() && *height > 0.0)
                .unwrap_or_else(|| {
                    Self::estimated_item_height(item, viewport.size.x, tool_expanded)
                });
            let _ = cache.update(index, measured + ITEM_GAP);
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
                let item_rect = Rect::xywh(
                    viewport.origin.x,
                    viewport.origin.y + measurement.top - offset,
                    viewport.size.x,
                    (measurement.bottom - measurement.top - ITEM_GAP).max(1.0),
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
                transcript
                    .item_heights
                    .get(index)
                    .copied()
                    .filter(|height| height.is_finite() && *height > 0.0)
                    .unwrap_or_else(|| {
                        Self::estimated_item_height(item, viewport.size.x, tool_expanded)
                    })
                    + ITEM_GAP
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

    fn estimated_item_height(
        item: &TranscriptItem,
        width: f32,
        tool_expanded: &BTreeMap<String, bool>,
    ) -> f32 {
        match item {
            TranscriptItem::AssistantText(markdown) => markdown_height(markdown, width),
            TranscriptItem::Tool(tool) => {
                let expanded = tool_expanded
                    .get(&tool.id)
                    .copied()
                    .unwrap_or_else(|| ToolCard::default_expanded(tool));
                if expanded {
                    60.0
                } else {
                    42.0
                }
            }
            TranscriptItem::Approval { .. } => 66.0,
            TranscriptItem::UserText(_)
            | TranscriptItem::Thinking(_)
            | TranscriptItem::Status { .. }
            | TranscriptItem::Error { .. } => 54.0,
        }
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
                        .get(&tool.id)
                        .copied()
                        .unwrap_or_else(|| ToolCard::default_expanded(tool));
                    return Some(AppCommand::SetToolExpanded {
                        tool_id: tool.id.clone(),
                        expanded: !expanded,
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

        paint_items(painter, rect, transcript, &state.tool_expanded, theme);
        painter.restore();
    }
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
            tool_expanded,
            theme,
        );
    }
}

fn paint_item(
    painter: &mut dyn Painter,
    rect: Rect,
    item: &TranscriptItem,
    tool_expanded: &BTreeMap<String, bool>,
    theme: &ZodeTheme,
) {
    match item {
        TranscriptItem::UserText(text) => paint_user(painter, rect, text, theme),
        TranscriptItem::AssistantText(markdown) => paint_markdown(painter, rect, markdown, theme),
        TranscriptItem::Thinking(text) => draw_text(
            painter,
            text,
            Point2D::new(rect.origin.x, rect.origin.y + 20.0),
            12.0,
            400,
            theme.tokens.muted_foreground,
        ),
        TranscriptItem::Tool(tool) => ToolCard::paint(
            painter,
            rect,
            tool,
            tool_expanded
                .get(&tool.id)
                .copied()
                .unwrap_or_else(|| ToolCard::default_expanded(tool)),
            theme,
        ),
        TranscriptItem::Approval { tool, .. } => ApprovalCard::paint(painter, rect, tool, theme),
        TranscriptItem::Status { message, .. } => draw_text(
            painter,
            message,
            Point2D::new(rect.origin.x, rect.origin.y + 20.0),
            12.0,
            400,
            theme.tokens.muted_foreground,
        ),
        TranscriptItem::Error { message, .. } => paint_notice(
            painter,
            rect,
            message,
            theme.tokens.destructive.with_alpha(0.12),
            theme.tokens.destructive,
        ),
    }
}

fn paint_user(painter: &mut dyn Painter, rect: Rect, text: &str, theme: &ZodeTheme) {
    let max_width = rect.size.x * 0.72;
    let text_width = painter.measure_text_weighted(text, 13.0, 400);
    let bubble_width = (text_width + 28.0).clamp(56.0, max_width.max(56.0));
    let x = rect.origin.x + rect.size.x - bubble_width;
    let bubble = Rect::xywh(x, rect.origin.y + 4.0, bubble_width, 42.0);
    painter.fill_round_rect(bubble, 12.0, theme.user_bubble);
    draw_text(
        painter,
        text,
        Point2D::new(x + 14.0, rect.origin.y + 29.0),
        13.0,
        400,
        theme.tokens.foreground,
    );
}

fn paint_markdown(painter: &mut dyn Painter, rect: Rect, markdown: &str, theme: &ZodeTheme) {
    let mut y = rect.origin.y + 18.0;
    let max_chars = ((rect.size.x / 7.0).floor() as usize).max(8);
    for block in parse_blocks(markdown, MARKDOWN_LIMIT) {
        match block {
            MdBlock::Heading { level, text } => {
                let size = if level == 3 { 16.0 } else { 14.0 };
                draw_text(
                    painter,
                    &text,
                    Point2D::new(rect.origin.x, y),
                    size,
                    600,
                    theme.tokens.foreground,
                );
                y += size + 9.0;
            }
            MdBlock::Bullet(source) => {
                draw_text(
                    painter,
                    "•",
                    Point2D::new(rect.origin.x, y),
                    13.0,
                    600,
                    theme.zode_purple,
                );
                let lines = draw_inline(
                    painter,
                    &source,
                    Point2D::new(rect.origin.x + 16.0, y),
                    max_chars.saturating_sub(2),
                    theme,
                );
                y += lines as f32 * 20.0 + 4.0;
            }
            MdBlock::Paragraph(source) => {
                let lines = draw_inline(
                    painter,
                    &source,
                    Point2D::new(rect.origin.x, y),
                    max_chars,
                    theme,
                );
                y += lines as f32 * 20.0 + 4.0;
            }
        }
    }
}

fn markdown_height(markdown: &str, width: f32) -> f32 {
    let max_chars = ((width / 7.0).floor() as usize).max(8);
    let mut height = 18.0;
    for block in parse_blocks(markdown, MARKDOWN_LIMIT) {
        height += match block {
            MdBlock::Heading { level, .. } => {
                let size = if level == 3 { 16.0 } else { 14.0 };
                size + 9.0
            }
            MdBlock::Bullet(source) => {
                wrapped_line_count(&source, max_chars.saturating_sub(2)) as f32 * 20.0 + 4.0
            }
            MdBlock::Paragraph(source) => {
                wrapped_line_count(&source, max_chars) as f32 * 20.0 + 4.0
            }
        };
    }
    (height + 8.0).max(54.0)
}

fn wrapped_line_count(source: &str, max_chars: usize) -> usize {
    wrap_runs(&parse_inline(source), max_chars).len().max(1)
}

fn draw_inline(
    painter: &mut dyn Painter,
    source: &str,
    origin: Point2D,
    max_chars: usize,
    theme: &ZodeTheme,
) -> usize {
    let lines = wrap_runs(&parse_inline(source), max_chars);
    for (line_index, line) in lines.iter().enumerate() {
        let mut x = origin.x;
        let y = origin.y + line_index as f32 * 20.0;
        for run in line {
            let (weight, color) = match run {
                MdRun::Bold(_) => (600, theme.tokens.foreground),
                MdRun::Code(_) => (500, theme.zode_purple),
                MdRun::Color(_) => (500, theme.zode_purple),
                MdRun::Plain(_) => (400, theme.tokens.foreground),
            };
            let content = run.text();
            let visible = content.trim_start();
            let leading = &content[..content.len() - visible.len()];
            x += painter.measure_text_weighted(leading, 13.0, weight);
            if !visible.is_empty() {
                draw_text(painter, visible, Point2D::new(x, y), 13.0, weight, color);
                x += painter.measure_text_weighted(visible, 13.0, weight);
            }
        }
    }
    lines.len()
}

fn paint_notice(
    painter: &mut dyn Painter,
    rect: Rect,
    text: &str,
    background: Color,
    foreground: Color,
) {
    painter.fill_round_rect(
        Rect::xywh(rect.origin.x, rect.origin.y + 4.0, rect.size.x, 42.0),
        8.0,
        background,
    );
    draw_text(
        painter,
        text,
        Point2D::new(rect.origin.x + 12.0, rect.origin.y + 29.0),
        12.0,
        500,
        foreground,
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
