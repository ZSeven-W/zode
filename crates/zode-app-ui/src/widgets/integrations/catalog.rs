use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{
    IntegrationCatalog, IntegrationCategory, IntegrationSection, IntegrationsTab, LoadState,
    ShellRoute, ZodeAppState,
};

use crate::{paint_single_line, ZodeTheme};

use super::{row, IntegrationRowLayout, IntegrationsPageLayout};

const SECTION_GAP: f32 = 18.0;
const HEADER_HEIGHT: f32 = 24.0;
const ROW_HEIGHT: f32 = 52.0;
const ROW_GAP: f32 = 8.0;
const COLUMN_GAP: f32 = 16.0;

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogSectionLayout {
    pub category: IntegrationCategory,
    pub title: &'static str,
    pub rect: Rect,
    pub header: Rect,
    pub rows: Vec<IntegrationRowLayout>,
}

pub(super) fn layout(
    page: &IntegrationsPageLayout,
    state: &ZodeAppState,
) -> Vec<CatalogSectionLayout> {
    let LoadState::Ready(catalog) = &state.presentation.integrations else {
        return Vec::new();
    };
    let tab = match state.presentation.route {
        ShellRoute::Integrations(tab) => tab,
        _ => return Vec::new(),
    };
    let sections = visible_sections(catalog, tab);
    let columns = if page.catalog.size.x < 600.0 { 1 } else { 2 };
    let column_gap = if columns == 1 { 0.0 } else { COLUMN_GAP };
    let column_width = ((page.catalog.size.x - column_gap) / columns as f32).max(0.0);
    let mut cursor_y = page.catalog.origin.y;
    sections
        .into_iter()
        .map(|section| {
            let row_count = section.rows.len().div_ceil(columns);
            let rows_height =
                row_count as f32 * ROW_HEIGHT + row_count.saturating_sub(1) as f32 * ROW_GAP;
            let header = Rect::xywh(
                page.catalog.origin.x,
                cursor_y,
                page.catalog.size.x,
                HEADER_HEIGHT,
            );
            let rows = section
                .rows
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    IntegrationRowLayout::new(
                        entry,
                        Rect::xywh(
                            page.catalog.origin.x
                                + (index % columns) as f32 * (column_width + column_gap),
                            cursor_y
                                + HEADER_HEIGHT
                                + 8.0
                                + (index / columns) as f32 * (ROW_HEIGHT + ROW_GAP),
                            column_width,
                            ROW_HEIGHT,
                        ),
                    )
                })
                .collect::<Vec<_>>();
            let height = HEADER_HEIGHT + 8.0 + rows_height;
            let result = CatalogSectionLayout {
                category: section.category,
                title: section.category.title(),
                rect: Rect::xywh(page.catalog.origin.x, cursor_y, page.catalog.size.x, height),
                header,
                rows,
            };
            cursor_y += height + SECTION_GAP;
            result
        })
        .collect()
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    page: &IntegrationsPageLayout,
    state: &ZodeAppState,
    catalog: &IntegrationCatalog,
    theme: &ZodeTheme,
) {
    if let Some(error) = catalog.directory_error.as_deref() {
        paint_single_line(
            painter,
            error,
            page.directory_status,
            12.0,
            400,
            theme.warning,
            HorizontalAlign::Start,
        );
    }
    let sections = layout(page, state);
    if sections.is_empty() {
        paint_single_line(
            painter,
            match state.presentation.route {
                ShellRoute::Integrations(IntegrationsTab::Skills) => "未发现已安装技能",
                _ => "当前工作区未发现可展示的集成",
            },
            Rect::xywh(
                page.catalog.origin.x,
                page.catalog.origin.y,
                page.catalog.size.x,
                72.0,
            ),
            14.0,
            500,
            theme.tokens.muted_foreground,
            HorizontalAlign::Center,
        );
        return;
    }
    for section in sections {
        paint_single_line(
            painter,
            section.title,
            section.header,
            16.0,
            600,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        for row_layout in &section.rows {
            row::paint(painter, row_layout, theme);
        }
    }
}

fn visible_sections(
    catalog: &IntegrationCatalog,
    tab: IntegrationsTab,
) -> Vec<&IntegrationSection> {
    catalog
        .sections
        .iter()
        .filter(|section| match tab {
            IntegrationsTab::Plugins => section.category != IntegrationCategory::Skills,
            IntegrationsTab::Skills => section.category == IntegrationCategory::Skills,
        })
        .collect()
}
