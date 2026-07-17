use jian_widgets::{Painter, Point2D, Rect};
use zode_app_model::AttachmentMetadata;

use crate::{RectExt, ZodeTheme};

use super::draw_text;

pub(super) const HEIGHT: f32 = 68.0;

pub(super) fn paint(
    painter: &mut dyn Painter,
    rect: Rect,
    attachment: &AttachmentMetadata,
    theme: &ZodeTheme,
) {
    painter.fill_round_rect(rect, 10.0, theme.tokens.card);
    painter.stroke_round_rect(rect, 10.0, theme.tokens.border, 1.0);
    draw_text(
        painter,
        &attachment.display_name,
        Point2D::new(rect.origin.x + 14.0, rect.origin.y + 25.0),
        12.0,
        600,
        theme.tokens.foreground,
    );
    let details = match (attachment.width, attachment.height) {
        (Some(width), Some(height)) => format!(
            "{} · {}×{} · {}",
            attachment.media_type,
            width,
            height,
            human_bytes(attachment.byte_len)
        ),
        _ => format!(
            "{} · {}",
            attachment.media_type,
            human_bytes(attachment.byte_len)
        ),
    };
    draw_text(
        painter,
        &details,
        Point2D::new(rect.origin.x + 14.0, rect.origin.y + 47.0),
        11.0,
        400,
        theme.tokens.muted_foreground,
    );
    if attachment.path.is_some() {
        draw_text(
            painter,
            "文件",
            Point2D::new(rect.max_x() - 42.0, rect.origin.y + 25.0),
            10.0,
            600,
            theme.zode_purple,
        );
    }
}

pub(super) fn human_bytes(byte_len: u64) -> String {
    if byte_len >= 1_048_576 {
        format!("{:.1} MB", byte_len as f64 / 1_048_576.0)
    } else if byte_len >= 1_024 {
        format!("{:.1} KB", byte_len as f64 / 1_024.0)
    } else {
        format!("{byte_len} B")
    }
}
