use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::AttachmentMetadata;

use crate::{paint_single_line, RectExt, ZodeTheme};

use super::ComposerAttachmentLayout;

pub(super) fn paint(
    painter: &mut dyn Painter,
    rect: Rect,
    layouts: &[ComposerAttachmentLayout],
    attachments: &[AttachmentMetadata],
    theme: &ZodeTheme,
) {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 || attachments.is_empty() {
        return;
    }
    painter.fill_rect(rect, theme.tokens.card);
    painter.stroke_line(
        jian_widgets::Point2D::new(rect.origin.x, rect.origin.y),
        jian_widgets::Point2D::new(rect.max_x(), rect.origin.y),
        theme.tokens.border,
        1.0,
    );
    for (layout, attachment) in layouts.iter().zip(attachments) {
        let details = match (attachment.width, attachment.height) {
            (Some(width), Some(height)) => {
                format!("{} · {width}×{height}", attachment.display_name)
            }
            _ => attachment.display_name.clone(),
        };
        let chip = layout.rect;
        painter.fill_round_rect(chip, 8.0, theme.tokens.muted);
        painter.stroke_round_rect(chip, 8.0, theme.tokens.border, 1.0);
        paint_single_line(
            painter,
            &details,
            Rect::xywh(
                chip.origin.x + 12.0,
                chip.origin.y,
                (chip.size.x - 24.0).max(0.0),
                chip.size.y,
            ),
            11.0,
            500,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
    }
}
