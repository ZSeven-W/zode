use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::IntegrationEntry;

use crate::{paint_single_line, stable_widget_id, WidgetId, ZodeTheme};

#[derive(Debug, Clone, PartialEq)]
pub struct IntegrationRowLayout {
    pub id: WidgetId,
    pub source_id: String,
    pub name: String,
    pub description: String,
    pub monogram: String,
    pub status: &'static str,
    pub rect: Rect,
}

impl IntegrationRowLayout {
    pub(super) fn new(entry: &IntegrationEntry, rect: Rect) -> Option<Self> {
        let source_id = entry.source_id.clone()?;
        Some(Self {
            id: widget_id(&source_id),
            source_id,
            name: entry.name.clone(),
            description: entry.description.clone(),
            monogram: entry.icon.label().to_owned(),
            status: entry.availability.label(),
            rect,
        })
    }
}

pub(super) fn widget_id(source_id: &str) -> WidgetId {
    stable_widget_id(0x72, source_id)
}

pub(super) fn paint(painter: &mut dyn Painter, row: &IntegrationRowLayout, theme: &ZodeTheme) {
    painter.save();
    painter.clip_round_rect(row.rect, 10.0);
    painter.fill_round_rect(row.rect, 10.0, theme.tokens.card);
    painter.stroke_round_rect(row.rect, 10.0, theme.tokens.border, 1.0);

    let icon_size = row.rect.size.y.clamp(0.0, 36.0);
    let icon = Rect::xywh(
        row.rect.origin.x + 10.0,
        row.rect.origin.y + (row.rect.size.y - icon_size) / 2.0,
        icon_size,
        icon_size,
    );
    painter.fill_round_rect(icon, 9.0, theme.tokens.muted);
    paint_single_line(
        painter,
        &row.monogram,
        icon,
        if row.monogram.chars().count() > 1 {
            11.0
        } else {
            14.0
        },
        650,
        theme.tokens.foreground,
        HorizontalAlign::Center,
    );

    let status_width = 62.0_f32.min((row.rect.size.x * 0.28).max(0.0));
    let status = Rect::xywh(
        row.rect.origin.x + (row.rect.size.x - status_width - 10.0).max(0.0),
        row.rect.origin.y + (row.rect.size.y - 24.0) / 2.0,
        status_width,
        24.0,
    );
    painter.fill_round_rect(status, 12.0, theme.tokens.muted);
    paint_single_line(
        painter,
        row.status,
        status,
        11.0,
        500,
        if row.status == "可用" {
            theme.success
        } else {
            theme.tokens.muted_foreground
        },
        HorizontalAlign::Center,
    );

    let text_x = icon.origin.x + icon.size.x + 10.0;
    let text_width = (status.origin.x - text_x - 8.0).max(0.0);
    paint_single_line(
        painter,
        &row.name,
        Rect::xywh(text_x, row.rect.origin.y + 8.0, text_width, 19.0),
        13.0,
        600,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    paint_single_line(
        painter,
        &row.description,
        Rect::xywh(text_x, row.rect.origin.y + 28.0, text_width, 16.0),
        11.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    painter.restore();
}
