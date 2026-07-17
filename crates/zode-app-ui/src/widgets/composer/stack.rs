use jian_widgets::Rect;
use zode_app_model::ComposerState;

use crate::{
    composer_queue_reserved_height, RectExt, COMPOSER_ATTACHMENT_H, COMPOSER_CONTEXT_INSET_X,
    COMPOSER_CONTEXT_VISIBLE_H, COMPOSER_INPUT_H,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposerLayout {
    /// Visible content and hit-test area of the attached context rail.
    pub context: Rect,
    pub attachments: Option<Rect>,
    pub queue: Option<Rect>,
    pub input: Rect,
}

/// Lays out the composer from the bottom up so the editable surface never
/// moves when optional rows are added above it. The context rail background
/// may paint below its visible rectangle, but its hit area never overlaps the
/// following surface.
pub(super) fn layout(rect: Rect, state: &ComposerState, queue_count: usize) -> ComposerLayout {
    let available_height = rect.size.y.max(0.0);
    let input_height = COMPOSER_INPUT_H.min(available_height);
    let input = Rect::xywh(
        rect.origin.x,
        rect.max_y() - input_height,
        rect.size.x.max(0.0),
        input_height,
    );
    let mut cursor = input.origin.y;

    let queue_height =
        composer_queue_reserved_height(queue_count).min((cursor - rect.origin.y).max(0.0));
    let queue = (queue_height > 0.0).then(|| {
        cursor -= queue_height;
        Rect::xywh(rect.origin.x, cursor, rect.size.x.max(0.0), queue_height)
    });

    let attachment_height = if state.attachments.is_empty() {
        0.0
    } else {
        COMPOSER_ATTACHMENT_H.min((cursor - rect.origin.y).max(0.0))
    };
    let attachments = (attachment_height > 0.0).then(|| {
        cursor -= attachment_height;
        Rect::xywh(
            rect.origin.x,
            cursor,
            rect.size.x.max(0.0),
            attachment_height,
        )
    });

    let context_height = COMPOSER_CONTEXT_VISIBLE_H.min((cursor - rect.origin.y).max(0.0));
    cursor -= context_height;
    let context_inset = COMPOSER_CONTEXT_INSET_X.min((rect.size.x.max(0.0) / 2.0).max(0.0));
    let context = Rect::xywh(
        rect.origin.x + context_inset,
        cursor,
        (rect.size.x - context_inset * 2.0).max(0.0),
        context_height,
    );

    ComposerLayout {
        context,
        attachments,
        queue,
        input,
    }
}
