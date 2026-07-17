use jian_widgets::{components::tooltip::Tooltip, HorizontalAlign, Painter, Rect};
use zode_app_model::{AppCommand, QueuedMessage, QueuedMessageId, ZodeAppState};
use zode_node_protocol::SessionLocator;

use crate::{
    paint_single_line, stable_widget_id, RectExt, SemanticIcon, WidgetId, ZodeTheme,
    COMPOSER_QUEUE_INSET_X, COMPOSER_QUEUE_OVERLAP, COMPOSER_QUEUE_PAD_Y, COMPOSER_QUEUE_ROW_H,
};

const ACTION_ICON_SIZE: f32 = 14.0;
const MORE_VISUAL_SIZE: f32 = 24.0;
const MORE_HIT_W: f32 = 32.0;
const DELETE_HIT_W: f32 = 32.0;
const GUIDE_HIT_W: f32 = 58.0;
const ROW_INSET_X: f32 = 10.0;
const MIN_COMPACT_ACTION_W: f32 = 8.0;
const MIN_MORE_HIT_W: f32 = 24.0;
const MIN_ROW_W: f32 = MIN_COMPACT_ACTION_W * 2.0 + MIN_MORE_HIT_W;
const MENU_W: f32 = 112.0;
const MENU_H: f32 = 80.0;
const MENU_PAD: f32 = 4.0;
const MENU_ROW_H: f32 = 36.0;
const MIN_MENU_W: f32 = 72.0;
const MIN_MENU_ROW_H: f32 = 20.0;
const TOOLTIP_W: f32 = 164.0;
const TOOLTIP_H: f32 = 30.0;
const TOOLTIP_GAP: f32 = 5.0;
const PREVIEW_MAX_CHARS: usize = 512;
const PREVIEW_SCAN_MAX_CHARS: usize = PREVIEW_MAX_CHARS * 4;

const LIST_NAMESPACE: u8 = 0x50;
const ROW_NAMESPACE: u8 = 0x51;
const GUIDE_NAMESPACE: u8 = 0x52;
const DELETE_NAMESPACE: u8 = 0x53;
const MORE_NAMESPACE: u8 = 0x54;
const MENU_NAMESPACE: u8 = 0x55;
const MENU_EDIT_NAMESPACE: u8 = 0x56;
const MENU_CLOSE_NAMESPACE: u8 = 0x57;

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerQueueRowLayout {
    pub message_id: QueuedMessageId,
    pub row_id: WidgetId,
    pub rect: Rect,
    pub guide_id: WidgetId,
    pub guide: Rect,
    pub delete_id: WidgetId,
    pub delete: Rect,
    pub more_id: WidgetId,
    pub more: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerQueueMenuLayout {
    pub id: WidgetId,
    pub message_id: QueuedMessageId,
    pub rect: Rect,
    pub edit_id: WidgetId,
    pub edit: Rect,
    pub close_id: WidgetId,
    pub close: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerQueueLayout {
    pub list_id: WidgetId,
    pub surface: Rect,
    pub rows: Vec<ComposerQueueRowLayout>,
    pub menu: Option<ComposerQueueMenuLayout>,
    pub total_count: usize,
}

pub(super) fn current_queue(state: &ZodeAppState) -> Option<(&SessionLocator, &[QueuedMessage])> {
    let session = state.current_session.as_ref()?;
    let queue = state.message_queues.get(session)?;
    (!queue.items.is_empty()).then_some((session, queue.items.as_slice()))
}

pub(super) fn layout(
    slot: Rect,
    _viewport: Rect,
    state: &ZodeAppState,
) -> Option<ComposerQueueLayout> {
    let (session, messages) = current_queue(state)?;
    if slot.size.x <= 0.0 || slot.size.y <= 0.0 {
        return None;
    }
    let inset = COMPOSER_QUEUE_INSET_X
        .min((slot.size.x * 0.05).max(8.0))
        .min(slot.size.x / 2.0);
    let surface = Rect::xywh(
        slot.origin.x + inset,
        slot.origin.y,
        (slot.size.x - inset * 2.0).max(0.0),
        slot.size.y + COMPOSER_QUEUE_OVERLAP,
    );
    let menu_bounds = Rect::xywh(
        surface.origin.x,
        surface.origin.y,
        surface.size.x,
        slot.size.y + crate::COMPOSER_INPUT_H,
    );
    let vertical_capacity =
        ((slot.size.y - COMPOSER_QUEUE_PAD_Y).max(0.0) / COMPOSER_QUEUE_ROW_H).floor() as usize;
    let rows = messages
        .iter()
        .take(
            crate::COMPOSER_QUEUE_MAX_VISIBLE
                .min(vertical_capacity)
                .min(messages.len()),
        )
        .enumerate()
        .filter_map(|(index, message)| row_layout(session, message.id, surface, index))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return None;
    }
    let menu = state
        .composer
        .queue_menu
        .and_then(|id| rows.iter().find(|row| row.message_id == id))
        .and_then(|row| menu_layout(session, row, menu_bounds));
    Some(ComposerQueueLayout {
        list_id: scoped_id(LIST_NAMESPACE, session, None),
        surface,
        rows,
        menu,
        total_count: messages.len(),
    })
}

fn row_layout(
    session: &SessionLocator,
    message_id: QueuedMessageId,
    surface: Rect,
    index: usize,
) -> Option<ComposerQueueRowLayout> {
    let row_inset = ROW_INSET_X.min(((surface.size.x - MIN_ROW_W) / 2.0).max(0.0));
    let rect = Rect::xywh(
        surface.origin.x + row_inset,
        surface.origin.y + COMPOSER_QUEUE_PAD_Y + index as f32 * COMPOSER_QUEUE_ROW_H,
        (surface.size.x - row_inset * 2.0).max(0.0),
        COMPOSER_QUEUE_ROW_H,
    );
    if rect.size.x < MIN_ROW_W || rect.size.y <= 0.0 {
        return None;
    }
    let (guide_width, delete_width, more_width) =
        if rect.size.x >= GUIDE_HIT_W + DELETE_HIT_W + MORE_HIT_W {
            (GUIDE_HIT_W, DELETE_HIT_W, MORE_HIT_W)
        } else {
            let more_width =
                MORE_HIT_W.min((rect.size.x - MIN_COMPACT_ACTION_W * 2.0).max(MIN_MORE_HIT_W));
            let remaining = rect.size.x - more_width;
            let delete_width =
                (remaining - MIN_COMPACT_ACTION_W).clamp(MIN_COMPACT_ACTION_W, DELETE_HIT_W);
            let guide_width = remaining - delete_width;
            (guide_width, delete_width, more_width)
        };
    let more = Rect::xywh(
        rect.max_x() - more_width,
        rect.origin.y,
        more_width,
        rect.size.y,
    );
    let delete = Rect::xywh(
        more.origin.x - delete_width,
        rect.origin.y,
        delete_width,
        rect.size.y,
    );
    let guide = Rect::xywh(
        delete.origin.x - guide_width,
        rect.origin.y,
        guide_width,
        rect.size.y,
    );
    Some(ComposerQueueRowLayout {
        message_id,
        row_id: scoped_id(ROW_NAMESPACE, session, Some(message_id)),
        rect,
        guide_id: scoped_id(GUIDE_NAMESPACE, session, Some(message_id)),
        guide,
        delete_id: scoped_id(DELETE_NAMESPACE, session, Some(message_id)),
        delete,
        more_id: scoped_id(MORE_NAMESPACE, session, Some(message_id)),
        more,
    })
}

fn menu_layout(
    session: &SessionLocator,
    row: &ComposerQueueRowLayout,
    surface: Rect,
) -> Option<ComposerQueueMenuLayout> {
    let width = MENU_W
        .min(surface.size.x.max(0.0))
        .min(row.rect.size.x.max(0.0));
    let height = MENU_H.min(surface.size.y.max(0.0));
    if width < MIN_MENU_W || height < MENU_PAD * 2.0 + MIN_MENU_ROW_H * 2.0 {
        return None;
    }
    let right = surface.max_x();
    let bottom = surface.max_y();
    let x =
        (row.more.max_x() - width).clamp(surface.origin.x, (right - width).max(surface.origin.x));
    let desired_y = row.more.max_y() - MENU_PAD;
    let y = if desired_y + height > bottom {
        row.more.origin.y - height + MENU_PAD
    } else {
        desired_y
    }
    .clamp(surface.origin.y, (bottom - height).max(surface.origin.y));
    let rect = Rect::xywh(x, y, width, height);
    let action_height = MENU_ROW_H.min((rect.size.y - MENU_PAD * 2.0) / 2.0);
    let edit = Rect::xywh(
        rect.origin.x + MENU_PAD,
        rect.origin.y + MENU_PAD,
        (rect.size.x - MENU_PAD * 2.0).max(0.0),
        action_height,
    );
    let close = Rect::xywh(edit.origin.x, edit.max_y(), edit.size.x, action_height);
    Some(ComposerQueueMenuLayout {
        id: scoped_id(MENU_NAMESPACE, session, Some(row.message_id)),
        message_id: row.message_id,
        rect,
        edit_id: scoped_id(MENU_EDIT_NAMESPACE, session, Some(row.message_id)),
        edit,
        close_id: scoped_id(MENU_CLOSE_NAMESPACE, session, Some(row.message_id)),
        close,
    })
}

fn scoped_id(
    namespace: u8,
    session: &SessionLocator,
    message: Option<QueuedMessageId>,
) -> WidgetId {
    stable_widget_id(
        namespace,
        &(
            session.node_id,
            session.session_id.as_str(),
            message.map(QueuedMessageId::as_u64),
        ),
    )
}

pub(super) fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    let (session, messages) = current_queue(state)?;
    for message in messages.iter().take(crate::COMPOSER_QUEUE_MAX_VISIBLE) {
        if id == scoped_id(GUIDE_NAMESPACE, session, Some(message.id)) {
            return Some(AppCommand::GuideQueuedMessage {
                session: session.clone(),
                id: message.id,
            });
        }
        if id == scoped_id(DELETE_NAMESPACE, session, Some(message.id)) {
            return Some(AppCommand::RemoveQueuedMessage {
                session: session.clone(),
                id: message.id,
            });
        }
        if id == scoped_id(MORE_NAMESPACE, session, Some(message.id)) {
            return Some(AppCommand::ToggleQueuedMessageMenu {
                session: session.clone(),
                id: message.id,
            });
        }
    }
    let open = state.composer.queue_menu?;
    if id == scoped_id(MENU_EDIT_NAMESPACE, session, Some(open)) {
        return Some(AppCommand::BeginEditQueuedMessage {
            session: session.clone(),
            id: open,
        });
    }
    (id == scoped_id(MENU_CLOSE_NAMESPACE, session, Some(open))).then(|| {
        AppCommand::ClearMessageQueue {
            session: session.clone(),
        }
    })
}

pub(super) fn paint_rows(
    painter: &mut dyn Painter,
    layout: &ComposerQueueLayout,
    messages: &[QueuedMessage],
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    painter.fill_round_rect(layout.surface, 16.0, theme.tokens.card);
    painter.stroke_round_rect(layout.surface, 16.0, theme.tokens.border, 1.0);
    painter.save();
    painter.clip_rect(layout.surface);
    for (row, message) in layout.rows.iter().zip(messages) {
        let content_width = (row.guide.origin.x - row.rect.origin.x).max(0.0);
        if content_width >= ACTION_ICON_SIZE + 2.0 {
            let icon = Rect::xywh(
                row.rect.origin.x + 2.0,
                row.rect.origin.y + (row.rect.size.y - ACTION_ICON_SIZE) / 2.0,
                ACTION_ICON_SIZE,
                ACTION_ICON_SIZE,
            );
            painter.stroke_svg_path(
                SemanticIcon::Queue.path(),
                icon.origin,
                icon.size.x,
                theme.tokens.muted_foreground,
                SemanticIcon::Queue.stroke_width(),
            );
            let text_rect = Rect::xywh(
                icon.max_x() + 8.0,
                row.rect.origin.y,
                (row.guide.origin.x - icon.max_x() - 14.0).max(0.0),
                row.rect.size.y,
            );
            if text_rect.size.x > 0.0 {
                let preview = message_preview(message);
                let visible = end_ellipsize(painter, &preview, text_rect.size.x, 14.0, 400);
                paint_single_line(
                    painter,
                    &visible,
                    text_rect,
                    14.0,
                    400,
                    theme.tokens.foreground,
                    HorizontalAlign::Start,
                );
            }
        }
        paint_action_state(
            painter,
            row.guide,
            hovered == Some(row.guide_id),
            focused == Some(row.guide_id),
            7.0,
            theme,
        );
        paint_action_state(
            painter,
            row.delete,
            hovered == Some(row.delete_id),
            focused == Some(row.delete_id),
            7.0,
            theme,
        );
        let more_visual = centered_square(row.more, MORE_VISUAL_SIZE);
        if hovered == Some(row.more_id)
            || layout
                .menu
                .as_ref()
                .is_some_and(|menu| menu.message_id == row.message_id)
        {
            painter.fill_round_rect(more_visual, MORE_VISUAL_SIZE / 2.0, theme.tokens.muted);
        }
        paint_focus_ring(
            painter,
            more_visual,
            focused == Some(row.more_id),
            MORE_VISUAL_SIZE / 2.0,
            theme,
        );
        paint_guide(painter, row.guide, theme);
        paint_centered_icon(
            painter,
            SemanticIcon::Delete,
            row.delete,
            theme.tokens.muted_foreground,
        );
        paint_centered_icon(
            painter,
            SemanticIcon::More,
            row.more,
            theme.tokens.muted_foreground,
        );
    }
    painter.restore();
}

pub(super) fn paint_overlays(
    painter: &mut dyn Painter,
    layout: &ComposerQueueLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if let Some(row) = layout.rows.iter().find(|row| hovered == Some(row.guide_id)) {
        let width = TOOLTIP_W.min(layout.surface.size.x.max(0.0));
        let desired_x = row.guide.origin.x + row.guide.size.x / 2.0 - width / 2.0;
        let x = desired_x.clamp(
            layout.surface.origin.x,
            (layout.surface.max_x() - width).max(layout.surface.origin.x),
        );
        let tooltip = Rect::xywh(
            x,
            row.guide.origin.y - TOOLTIP_H - TOOLTIP_GAP,
            width,
            TOOLTIP_H,
        );
        Tooltip {
            label: "提交，但不中断模型运行",
        }
        .paint(painter, tooltip, &theme.tokens);
    }
    let Some(menu) = layout.menu.as_ref() else {
        return;
    };
    painter.fill_drop_shadow(
        Rect::xywh(
            menu.rect.origin.x,
            menu.rect.origin.y + 2.0,
            menu.rect.size.x,
            menu.rect.size.y,
        ),
        10.0,
        16.0,
        theme.tokens.foreground.with_alpha(0.12),
    );
    painter.fill_round_rect(menu.rect, 10.0, theme.tokens.popover);
    painter.stroke_round_rect(menu.rect, 10.0, theme.tokens.border, 1.0);
    for (rect, id, icon, label) in [
        (menu.edit, menu.edit_id, SemanticIcon::Edit, "编辑消息"),
        (
            menu.close,
            menu.close_id,
            SemanticIcon::CloseQueue,
            "关闭排队",
        ),
    ] {
        paint_action_state(
            painter,
            rect,
            hovered == Some(id),
            focused == Some(id),
            7.0,
            theme,
        );
        let icon_rect = Rect::xywh(
            rect.origin.x + 8.0,
            rect.origin.y + (rect.size.y - 16.0) / 2.0,
            16.0,
            16.0,
        );
        painter.stroke_svg_path(
            icon.path(),
            icon_rect.origin,
            icon_rect.size.x,
            theme.tokens.popover_foreground,
            icon.stroke_width(),
        );
        paint_single_line(
            painter,
            label,
            Rect::xywh(
                icon_rect.max_x() + 7.0,
                rect.origin.y,
                (rect.max_x() - icon_rect.max_x() - 13.0).max(0.0),
                rect.size.y,
            ),
            13.0,
            400,
            theme.tokens.popover_foreground,
            HorizontalAlign::Start,
        );
    }
}

pub(super) fn message_preview(message: &QueuedMessage) -> String {
    let text = bounded_normalized_preview(&message.text);
    if !text.is_empty() {
        text
    } else if message.attachments.len() == 1 {
        bounded_normalized_preview(&format!("附件：{}", message.attachments[0].display_name))
    } else {
        format!("{} 个附件", message.attachments.len())
    }
}

fn bounded_normalized_preview(value: &str) -> String {
    let mut preview = String::with_capacity(value.len().min(PREVIEW_MAX_CHARS * 3));
    let mut count = 0usize;
    let mut pending_space = false;
    for character in value.chars().take(PREVIEW_SCAN_MAX_CHARS) {
        if character.is_whitespace() {
            pending_space = !preview.is_empty();
            continue;
        }
        if pending_space {
            if count + 1 >= PREVIEW_MAX_CHARS {
                break;
            }
            preview.push(' ');
            count += 1;
            pending_space = false;
        }
        if count >= PREVIEW_MAX_CHARS {
            break;
        }
        preview.push(character);
        count += 1;
    }
    preview
}

fn paint_guide(painter: &mut dyn Painter, rect: Rect, theme: &ZodeTheme) {
    if rect.size.x >= 48.0 {
        let icon = Rect::xywh(
            rect.origin.x + 4.0,
            rect.origin.y + (rect.size.y - ACTION_ICON_SIZE) / 2.0,
            ACTION_ICON_SIZE,
            ACTION_ICON_SIZE,
        );
        painter.stroke_svg_path(
            SemanticIcon::Guide.path(),
            icon.origin,
            icon.size.x,
            theme.tokens.muted_foreground,
            SemanticIcon::Guide.stroke_width(),
        );
        paint_single_line(
            painter,
            "引导",
            Rect::xywh(
                icon.max_x() + 5.0,
                rect.origin.y,
                (rect.max_x() - icon.max_x() - 5.0).max(0.0),
                rect.size.y,
            ),
            13.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    } else {
        paint_centered_icon(
            painter,
            SemanticIcon::Guide,
            rect,
            theme.tokens.muted_foreground,
        );
    }
}

fn paint_centered_icon(
    painter: &mut dyn Painter,
    icon: SemanticIcon,
    rect: Rect,
    color: jian_widgets::Color,
) {
    let visual = centered_square(rect, ACTION_ICON_SIZE);
    painter.stroke_svg_path(
        icon.path(),
        visual.origin,
        visual.size.x,
        color,
        icon.stroke_width(),
    );
}

fn paint_action_state(
    painter: &mut dyn Painter,
    rect: Rect,
    hovered: bool,
    focused: bool,
    radius: f32,
    theme: &ZodeTheme,
) {
    if hovered {
        painter.fill_round_rect(rect, radius, theme.tokens.muted);
    }
    paint_focus_ring(painter, rect, focused, radius, theme);
}

fn paint_focus_ring(
    painter: &mut dyn Painter,
    rect: Rect,
    focused: bool,
    radius: f32,
    theme: &ZodeTheme,
) {
    if focused {
        painter.stroke_round_rect(rect, radius, theme.tokens.ring, 1.5);
    }
}

fn centered_square(rect: Rect, size: f32) -> Rect {
    Rect::xywh(
        rect.origin.x + (rect.size.x - size).max(0.0) / 2.0,
        rect.origin.y + (rect.size.y - size).max(0.0) / 2.0,
        size.min(rect.size.x),
        size.min(rect.size.y),
    )
}

fn end_ellipsize(
    painter: &mut dyn Painter,
    value: &str,
    width: f32,
    font_size: f32,
    weight: u16,
) -> String {
    const ELLIPSIS: &str = "…";
    if width <= 0.0 {
        return String::new();
    }
    if painter.measure_text_weighted(value, font_size, weight) <= width {
        return value.to_owned();
    }
    if painter.measure_text_weighted(ELLIPSIS, font_size, weight) > width {
        return String::new();
    }
    let mut boundaries = value
        .char_indices()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    boundaries.push(value.len());
    let mut lower = 0usize;
    let mut upper = boundaries.len() - 1;
    while lower < upper {
        let middle = (lower + upper).div_ceil(2);
        let prefix = value[..boundaries[middle]].trim_end();
        let mut candidate = String::with_capacity(prefix.len() + ELLIPSIS.len());
        candidate.push_str(prefix);
        candidate.push_str(ELLIPSIS);
        if painter.measure_text_weighted(&candidate, font_size, weight) <= width {
            lower = middle;
        } else {
            upper = middle - 1;
        }
    }
    let prefix = value[..boundaries[lower]].trim_end();
    let mut visible = String::with_capacity(prefix.len() + ELLIPSIS.len());
    visible.push_str(prefix);
    visible.push_str(ELLIPSIS);
    visible
}
