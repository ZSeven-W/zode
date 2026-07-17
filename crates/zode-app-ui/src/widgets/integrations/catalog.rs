use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{
    IntegrationCatalog, IntegrationCategory, IntegrationEntry, IntegrationMutationState,
    IntegrationScope, IntegrationsTab, LoadState, ShellRoute, ZodeAppState,
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
    let sections = visible_sections(catalog, tab, state);
    let columns = columns(page);
    let column_gap = if columns == 1 { 0.0 } else { COLUMN_GAP };
    let column_width = ((page.catalog.size.x - column_gap) / columns as f32).max(0.0);
    let mut cursor_y = page.catalog.origin.y - scroll_offset(page, state);
    sections
        .into_iter()
        .map(|section| {
            let height = section_height(section.rows.len(), columns);
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
                        state,
                    )
                })
                .collect::<Vec<_>>();
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

pub(super) fn max_scroll_offset(page: &IntegrationsPageLayout, state: &ZodeAppState) -> f32 {
    (content_height(page, state) - page.catalog.size.y).max(0.0)
}

fn scroll_offset(page: &IntegrationsPageLayout, state: &ZodeAppState) -> f32 {
    if !state.integration_scroll_offset.is_finite() {
        return 0.0;
    }
    state
        .integration_scroll_offset
        .clamp(0.0, max_scroll_offset(page, state))
}

fn content_height(page: &IntegrationsPageLayout, state: &ZodeAppState) -> f32 {
    let LoadState::Ready(catalog) = &state.presentation.integrations else {
        return 0.0;
    };
    let ShellRoute::Integrations(tab) = state.presentation.route else {
        return 0.0;
    };
    let columns = columns(page);
    visible_sections(catalog, tab, state)
        .iter()
        .enumerate()
        .map(|(index, section)| {
            section_height(section.rows.len(), columns) + if index == 0 { 0.0 } else { SECTION_GAP }
        })
        .sum()
}

fn columns(page: &IntegrationsPageLayout) -> usize {
    if page.catalog.size.x < 600.0 {
        1
    } else {
        2
    }
}

fn section_height(row_len: usize, columns: usize) -> f32 {
    let row_count = row_len.div_ceil(columns);
    let rows_height = row_count as f32 * ROW_HEIGHT + row_count.saturating_sub(1) as f32 * ROW_GAP;
    HEADER_HEIGHT + 8.0 + rows_height
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    page: &IntegrationsPageLayout,
    state: &ZodeAppState,
    catalog: &IntegrationCatalog,
    theme: &ZodeTheme,
) {
    let status = match &state.presentation.integration_mutation {
        IntegrationMutationState::Failed { message, .. } => {
            Some((message.as_str(), theme.tokens.destructive))
        }
        IntegrationMutationState::Updating { .. } => Some(("正在保存插件状态…", theme.warning)),
        IntegrationMutationState::Idle => catalog
            .directory_error
            .as_deref()
            .map(|message| (message, theme.warning)),
    };
    if let Some((message, color)) = status {
        paint_single_line(
            painter,
            message,
            page.directory_status,
            12.0,
            400,
            color,
            HorizontalAlign::Start,
        );
    }
    painter.save();
    painter.clip_rect(page.catalog);
    let sections = layout(page, state);
    if sections.is_empty() {
        paint_single_line(
            painter,
            match (
                state.presentation.integration_scope,
                state.presentation.route,
                state.presentation.integration_search.trim().is_empty(),
            ) {
                (IntegrationScope::Public, _, true) => {
                    "公开目录不可用；当前节点没有可验证的可安装项目"
                }
                (_, _, false) => "没有匹配的插件或技能",
                (_, ShellRoute::Integrations(IntegrationsTab::Skills), true) => "未发现已安装技能",
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
    } else {
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
    painter.restore();
}

struct VisibleSection<'a> {
    category: IntegrationCategory,
    rows: Vec<&'a IntegrationEntry>,
}

fn visible_sections<'a>(
    catalog: &'a IntegrationCatalog,
    tab: IntegrationsTab,
    state: &ZodeAppState,
) -> Vec<VisibleSection<'a>> {
    catalog
        .sections
        .iter()
        .filter_map(|section| {
            let tab_matches = match tab {
                IntegrationsTab::Plugins => section.category != IntegrationCategory::Skills,
                IntegrationsTab::Skills => section.category == IntegrationCategory::Skills,
            };
            if !tab_matches {
                return None;
            }
            let rows = section
                .rows
                .iter()
                .filter(|entry| super::entry_visible(state, entry))
                .collect::<Vec<_>>();
            (!rows.is_empty()).then_some(VisibleSection {
                category: section.category,
                rows,
            })
        })
        .collect()
}
