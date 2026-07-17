//! Hover-revealed (or, for the newest assistant message, persistent) action
//! row painted below a transcript message: a muted timestamp for user
//! bubbles, and copy/thumbs-up/thumbs-down/share icon-buttons plus a
//! trailing timestamp for assistant messages.
//!
//! Every geometry helper here operates on the *reserved* strip
//! (`markdown::ACTION_ROW_RESERVED` / `ROW_HEIGHT` below) that
//! `estimated_item_height` already adds to every user and assistant item
//! unconditionally - painting into it is therefore always a no-op on layout,
//! never a resize. See the module-level notes on `TranscriptState::touch_layout`
//! and the doc comment on `ACTION_ROW_RESERVED` for why that matters.

use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::MessageFeedback;

use crate::{paint_single_line, RectExt, SemanticIcon, TypographyRole, ZodeTheme};

/// Height of the reserved strip below a message's own content. Kept small
/// enough to read as breathing room when nothing is revealed, tall enough to
/// host one row of 20px icon-buttons.
pub(super) const ROW_HEIGHT: f32 = 26.0;

const BUTTON_SIZE: f32 = 20.0;
const BUTTON_GAP: f32 = 2.0;
const ICON_SIZE: f32 = 13.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessageAction {
    Copy,
    ThumbsUp,
    ThumbsDown,
    Share,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ActionButtonLayout {
    pub action: MessageAction,
    pub rect: Rect,
}

/// The reveal row for a user bubble: right-aligned under the bubble, a
/// muted timestamp followed by a single copy button.
pub(super) fn user_row_rect(bubble: Rect) -> Rect {
    Rect::xywh(
        bubble.origin.x,
        bubble.max_y() + (ROW_HEIGHT - BUTTON_SIZE) / 2.0 - 2.0,
        bubble.size.x,
        BUTTON_SIZE,
    )
}

pub(super) fn user_copy_button_rect(row: Rect) -> Rect {
    Rect::xywh(
        row.max_x() - BUTTON_SIZE,
        row.origin.y,
        BUTTON_SIZE,
        BUTTON_SIZE,
    )
}

/// The reveal row for an assistant message: left-aligned icon-buttons
/// starting at the message's own left edge, with the timestamp trailing at
/// the row's end (spec: "timestamp at the row's end").
pub(super) fn assistant_row_rect(content_rect: Rect, text_bottom: f32) -> Rect {
    Rect::xywh(
        content_rect.origin.x,
        text_bottom + (ROW_HEIGHT - BUTTON_SIZE) / 2.0 - 2.0,
        content_rect.size.x,
        BUTTON_SIZE,
    )
}

pub(super) fn assistant_button_layout(row: Rect) -> [ActionButtonLayout; 4] {
    let actions = [
        MessageAction::Copy,
        MessageAction::ThumbsUp,
        MessageAction::ThumbsDown,
        MessageAction::Share,
    ];
    std::array::from_fn(|index| ActionButtonLayout {
        action: actions[index],
        rect: Rect::xywh(
            row.origin.x + index as f32 * (BUTTON_SIZE + BUTTON_GAP),
            row.origin.y,
            BUTTON_SIZE,
            BUTTON_SIZE,
        ),
    })
}

fn action_icon(action: MessageAction) -> SemanticIcon {
    match action {
        MessageAction::Copy => SemanticIcon::Copy,
        MessageAction::ThumbsUp => SemanticIcon::ThumbsUp,
        MessageAction::ThumbsDown => SemanticIcon::ThumbsDown,
        MessageAction::Share => SemanticIcon::Share,
    }
}

fn is_active(action: MessageAction, feedback: MessageFeedback) -> bool {
    matches!(
        (action, feedback),
        (MessageAction::ThumbsUp, MessageFeedback::Up)
            | (MessageAction::ThumbsDown, MessageFeedback::Down)
    )
}

/// Paints one icon-button, matching the highlight styling used for
/// hover-revealed icon buttons elsewhere in the shell (see
/// `project_sidebar::paint::paint_icon_button`): a rounded highlight fill
/// when hovered, plus a stronger tint when the button reflects the message's
/// active reaction (thumbs up/down).
fn paint_action_button(
    painter: &mut dyn Painter,
    rect: Rect,
    action: MessageAction,
    feedback: MessageFeedback,
    hovered: bool,
    theme: &ZodeTheme,
) {
    let active = is_active(action, feedback);
    if active {
        painter.fill_round_rect(rect, 6.0, theme.zode_purple.with_alpha(0.14));
    } else if hovered {
        painter.fill_round_rect(rect, 6.0, theme.tokens.muted);
    }
    let color = if active {
        theme.zode_purple
    } else {
        theme.tokens.muted_foreground
    };
    let icon = action_icon(action);
    painter.stroke_svg_path(
        icon.path(),
        Point2D::new(
            rect.origin.x + (rect.size.x - ICON_SIZE) / 2.0,
            rect.origin.y + (rect.size.y - ICON_SIZE) / 2.0,
        ),
        ICON_SIZE,
        color,
        icon.stroke_width(),
    );
}

fn paint_timestamp(painter: &mut dyn Painter, rect: Rect, label: &str, theme: &ZodeTheme) {
    let caption = TypographyRole::UiCaption.style();
    paint_single_line(
        painter,
        label,
        rect,
        caption.size,
        caption.weight,
        theme.tokens.muted_foreground,
        HorizontalAlign::End,
    );
}

/// Paints the user-bubble reveal row: timestamp (if the message has one),
/// then the copy button, both right-aligned under the bubble.
pub(super) fn paint_user_row(
    painter: &mut dyn Painter,
    bubble: Rect,
    timestamp: Option<&str>,
    copy_hovered: bool,
    theme: &ZodeTheme,
) {
    let row = user_row_rect(bubble);
    let copy_rect = user_copy_button_rect(row);
    if let Some(label) = timestamp {
        let timestamp_rect = Rect::xywh(
            row.origin.x,
            row.origin.y,
            (copy_rect.origin.x - row.origin.x - 6.0).max(0.0),
            row.size.y,
        );
        paint_timestamp(painter, timestamp_rect, label, theme);
    }
    paint_action_button(
        painter,
        copy_rect,
        MessageAction::Copy,
        MessageFeedback::None,
        copy_hovered,
        theme,
    );
}

/// Paints the assistant action row: four icon-buttons left-aligned, then a
/// trailing timestamp filling the remaining width on the right.
pub(super) fn paint_assistant_row(
    painter: &mut dyn Painter,
    content_rect: Rect,
    text_bottom: f32,
    feedback: MessageFeedback,
    timestamp: Option<&str>,
    hovered_action: Option<MessageAction>,
    theme: &ZodeTheme,
) {
    let row = assistant_row_rect(content_rect, text_bottom);
    let buttons = assistant_button_layout(row);
    for button in buttons {
        paint_action_button(
            painter,
            button.rect,
            button.action,
            feedback,
            hovered_action == Some(button.action),
            theme,
        );
    }
    if let Some(label) = timestamp {
        let buttons_end = buttons
            .last()
            .map(|button| button.rect.max_x())
            .unwrap_or(row.origin.x);
        let timestamp_rect = Rect::xywh(
            buttons_end + 8.0,
            row.origin.y,
            (row.max_x() - buttons_end - 8.0).max(0.0),
            row.size.y,
        );
        paint_timestamp(painter, timestamp_rect, label, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_buttons_are_evenly_spaced_and_left_aligned() {
        let row = Rect::xywh(10.0, 100.0, 400.0, BUTTON_SIZE);
        let buttons = assistant_button_layout(row);
        assert_eq!(buttons[0].rect.origin.x, 10.0);
        for pair in buttons.windows(2) {
            assert_eq!(
                pair[1].rect.origin.x - pair[0].rect.origin.x,
                BUTTON_SIZE + BUTTON_GAP
            );
        }
    }

    #[test]
    fn user_copy_button_hugs_the_bubbles_right_edge() {
        let bubble = Rect::xywh(50.0, 0.0, 200.0, 40.0);
        let row = user_row_rect(bubble);
        let copy = user_copy_button_rect(row);
        assert_eq!(copy.max_x(), bubble.max_x());
    }

    #[test]
    fn only_the_matching_reaction_reports_active() {
        assert!(is_active(MessageAction::ThumbsUp, MessageFeedback::Up));
        assert!(!is_active(MessageAction::ThumbsUp, MessageFeedback::Down));
        assert!(is_active(MessageAction::ThumbsDown, MessageFeedback::Down));
        assert!(!is_active(MessageAction::Copy, MessageFeedback::Up));
    }
}
