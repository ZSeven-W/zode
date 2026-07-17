use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{LoadState, ZodeAppState};

use crate::{paint_single_line, stable_widget_id, WidgetId, ZodeTheme};

use super::IntegrationsPageLayout;

const ICON_SIZE: f32 = 38.0;
const ICON_GAP: f32 = 10.0;

#[derive(Debug, Clone, PartialEq)]
pub struct InstalledIconLayout {
    pub id: WidgetId,
    pub source_id: String,
    pub name: String,
    pub monogram: String,
    pub status: &'static str,
    pub rect: Rect,
}

pub(super) fn layout(
    page: &IntegrationsPageLayout,
    state: &ZodeAppState,
) -> Vec<InstalledIconLayout> {
    let LoadState::Ready(catalog) = &state.presentation.integrations else {
        return Vec::new();
    };
    let capacity = ((page.installed_strip.size.x + ICON_GAP) / (ICON_SIZE + ICON_GAP))
        .floor()
        .max(0.0) as usize;
    catalog
        .installed
        .iter()
        .filter_map(|item| {
            let source_id = item.source_id.as_ref()?;
            Some((source_id, item))
        })
        .take(capacity)
        .enumerate()
        .map(|(index, (source_id, item))| InstalledIconLayout {
            id: stable_widget_id(0x73, source_id),
            source_id: source_id.clone(),
            name: item.name.clone(),
            monogram: item.icon.label().to_owned(),
            status: item.availability.label(),
            rect: Rect::xywh(
                page.installed_strip.origin.x + index as f32 * (ICON_SIZE + ICON_GAP),
                page.installed_strip.origin.y + (page.installed_strip.size.y - ICON_SIZE) / 2.0,
                ICON_SIZE,
                ICON_SIZE,
            ),
        })
        .collect()
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    page: &IntegrationsPageLayout,
    state: &ZodeAppState,
    theme: &ZodeTheme,
) {
    paint_single_line(
        painter,
        "已安装",
        page.installed_title,
        16.0,
        600,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    for icon in layout(page, state) {
        painter.fill_round_rect(icon.rect, 10.0, theme.tokens.card);
        painter.stroke_round_rect(icon.rect, 10.0, theme.tokens.border, 1.0);
        paint_single_line(
            painter,
            &icon.monogram,
            icon.rect,
            if icon.monogram.chars().count() > 1 {
                12.0
            } else {
                15.0
            },
            650,
            theme.tokens.foreground,
            HorizontalAlign::Center,
        );
    }
}
