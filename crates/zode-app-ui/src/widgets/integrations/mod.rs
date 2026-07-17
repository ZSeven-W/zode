mod catalog;
mod installed;
mod row;

use jian_core::text_input::TextInputState;
use jian_widgets::{components::input::Input, HorizontalAlign, Painter, Rect};
use zode_app_model::{
    AppCommand, Availability, IntegrationEntry, IntegrationInstallState, IntegrationScope,
    IntegrationsTab, LoadState, ShellRoute, ZodeAppState,
};

use crate::{paint_single_line, SemanticIcon, WidgetId, ZodeTheme};

pub use catalog::CatalogSectionLayout;
pub use installed::InstalledIconLayout;
pub use row::IntegrationRowLayout;

pub const INTEGRATIONS_PLUGINS_TAB_ID: WidgetId = WidgetId(70);
pub const INTEGRATIONS_SKILLS_TAB_ID: WidgetId = WidgetId(71);
pub const INTEGRATIONS_SEARCH_ID: WidgetId = WidgetId(190);
pub const INTEGRATIONS_PUBLIC_SCOPE_ID: WidgetId = WidgetId(191);
pub const INTEGRATIONS_PERSONAL_SCOPE_ID: WidgetId = WidgetId(192);

const CONTENT_WIDTH: f32 = 736.0;
const CONTENT_TOP: f32 = 46.0;
const TAB_TOP: f32 = 6.0;
const TAB_WIDTH: f32 = 48.0;
const TAB_HEIGHT: f32 = 32.0;
const TAB_GAP: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntegrationTabLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub tab: IntegrationsTab,
    pub label: &'static str,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntegrationScopeLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub scope: IntegrationScope,
    pub label: &'static str,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntegrationsPageLayout {
    pub content: Rect,
    pub title: Rect,
    pub subtitle: Rect,
    pub search: Rect,
    pub scopes: [IntegrationScopeLayout; 2],
    pub installed_title: Rect,
    pub installed_strip: Rect,
    pub directory_status: Rect,
    pub catalog: Rect,
    pub tabs: [IntegrationTabLayout; 2],
}

pub struct IntegrationsPage;

impl IntegrationsPage {
    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        Self::paint_with_focus(painter, rect, state, None, theme);
    }

    pub fn paint_with_focus(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &ZodeAppState,
        focused: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        let ShellRoute::Integrations(tab) = state.presentation.route else {
            return;
        };
        let layout = Self::layout(rect, state);
        painter.save();
        painter.clip_rect(rect);
        paint_tabs(painter, &layout, theme);
        paint_header(painter, &layout, tab, state, focused, theme);
        match &state.presentation.integrations {
            LoadState::Ready(catalog)
                if state.active_available_workspace() == Some(&catalog.workspace_uri) =>
            {
                installed::paint(painter, &layout, state, theme);
                catalog::paint(painter, &layout, state, catalog, theme);
            }
            LoadState::Ready(_) => paint_load_state(painter, &layout, "当前任务未绑定项目", theme),
            LoadState::Idle => paint_load_state(painter, &layout, "尚未加载本机集成", theme),
            LoadState::Loading => paint_load_state(painter, &layout, "正在读取本机集成…", theme),
            LoadState::Failed(message) => paint_load_state(painter, &layout, message, theme),
        }
        painter.restore();
    }

    pub fn layout(rect: Rect, state: &ZodeAppState) -> IntegrationsPageLayout {
        let content_width = rect.size.x.clamp(0.0, CONTENT_WIDTH);
        let content = Rect::xywh(
            rect.origin.x + (rect.size.x - content_width).max(0.0) / 2.0,
            rect.origin.y + CONTENT_TOP,
            content_width,
            (rect.size.y - CONTENT_TOP).max(0.0),
        );
        let selected = match state.presentation.route {
            ShellRoute::Integrations(tab) => tab,
            _ => IntegrationsTab::Plugins,
        };
        let tab_x = rect.origin.x + 8.0;
        let tabs = [
            IntegrationTabLayout {
                id: INTEGRATIONS_PLUGINS_TAB_ID,
                rect: Rect::xywh(tab_x, rect.origin.y + TAB_TOP, TAB_WIDTH, TAB_HEIGHT),
                tab: IntegrationsTab::Plugins,
                label: "插件",
                selected: selected == IntegrationsTab::Plugins,
            },
            IntegrationTabLayout {
                id: INTEGRATIONS_SKILLS_TAB_ID,
                rect: Rect::xywh(
                    tab_x + TAB_WIDTH + TAB_GAP,
                    rect.origin.y + TAB_TOP,
                    TAB_WIDTH,
                    TAB_HEIGHT,
                ),
                tab: IntegrationsTab::Skills,
                label: "技能",
                selected: selected == IntegrationsTab::Skills,
            },
        ];
        let scope_y = content.origin.y + 144.0;
        let scopes = [
            IntegrationScopeLayout {
                id: INTEGRATIONS_PUBLIC_SCOPE_ID,
                rect: Rect::xywh(content.origin.x, scope_y, 52.0, 28.0),
                scope: IntegrationScope::Public,
                label: "公开",
                selected: state.presentation.integration_scope == IntegrationScope::Public,
            },
            IntegrationScopeLayout {
                id: INTEGRATIONS_PERSONAL_SCOPE_ID,
                rect: Rect::xywh(content.origin.x + 56.0, scope_y, 52.0, 28.0),
                scope: IntegrationScope::Personal,
                label: "个人",
                selected: state.presentation.integration_scope == IntegrationScope::Personal,
            },
        ];

        IntegrationsPageLayout {
            title: Rect::xywh(
                content.origin.x,
                content.origin.y + 12.0,
                content.size.x,
                36.0,
            ),
            subtitle: Rect::xywh(
                content.origin.x,
                content.origin.y + 48.0,
                content.size.x,
                24.0,
            ),
            search: Rect::xywh(
                content.origin.x,
                content.origin.y + 100.0,
                content.size.x,
                34.0,
            ),
            installed_title: Rect::xywh(
                content.origin.x,
                content.origin.y + 190.0,
                content.size.x,
                24.0,
            ),
            installed_strip: Rect::xywh(
                content.origin.x,
                content.origin.y + 222.0,
                content.size.x,
                44.0,
            ),
            directory_status: Rect::xywh(
                content.origin.x,
                content.origin.y + 278.0,
                content.size.x,
                22.0,
            ),
            catalog: Rect::xywh(
                content.origin.x,
                content.origin.y + 314.0,
                content.size.x,
                (content.size.y - 314.0).max(0.0),
            ),
            content,
            scopes,
            tabs,
        }
    }

    pub fn installed_icon_layout(rect: Rect, state: &ZodeAppState) -> Vec<InstalledIconLayout> {
        if !catalog_matches_active_workspace(state) {
            return Vec::new();
        }
        installed::layout(&Self::layout(rect, state), state)
    }

    pub fn catalog_section_layout(rect: Rect, state: &ZodeAppState) -> Vec<CatalogSectionLayout> {
        if !catalog_matches_active_workspace(state) {
            return Vec::new();
        }
        catalog::layout(&Self::layout(rect, state), state)
    }

    pub fn row_widget_id(source_id: &str) -> WidgetId {
        row::widget_id(source_id)
    }

    pub fn row_action_widget_id(source_id: &str) -> WidgetId {
        row::action_widget_id(source_id)
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        match id {
            INTEGRATIONS_PLUGINS_TAB_ID => {
                return Some(AppCommand::SelectIntegrationsTab(IntegrationsTab::Plugins));
            }
            INTEGRATIONS_SKILLS_TAB_ID => {
                return Some(AppCommand::SelectIntegrationsTab(IntegrationsTab::Skills));
            }
            INTEGRATIONS_PUBLIC_SCOPE_ID => {
                return Some(AppCommand::SetIntegrationScope(IntegrationScope::Public));
            }
            INTEGRATIONS_PERSONAL_SCOPE_ID => {
                return Some(AppCommand::SetIntegrationScope(IntegrationScope::Personal));
            }
            INTEGRATIONS_SEARCH_ID => return None,
            _ => {}
        }
        let LoadState::Ready(catalog) = &state.presentation.integrations else {
            return None;
        };
        let entry = catalog
            .all_entries()
            .filter(|entry| entry_visible(state, entry))
            .find(|entry| {
                entry
                    .source_id
                    .as_deref()
                    .is_some_and(|source_id| row::action_widget_id(source_id) == id)
            })?;
        let action = row::action_state(entry, state);
        if !action.enabled {
            return None;
        }
        Some(AppCommand::SetIntegrationEnabled {
            workspace_uri: catalog.workspace_uri.clone(),
            source_id: entry.source_id.clone()?,
            enabled: entry.availability == Availability::Disabled,
        })
    }
}

pub(super) fn entry_visible(state: &ZodeAppState, entry: &IntegrationEntry) -> bool {
    let tab_matches = match state.presentation.route {
        ShellRoute::Integrations(IntegrationsTab::Plugins) => {
            entry.category != zode_app_model::IntegrationCategory::Skills
        }
        ShellRoute::Integrations(IntegrationsTab::Skills) => {
            entry.category == zode_app_model::IntegrationCategory::Skills
        }
        _ => false,
    };
    let scope_matches = match state.presentation.integration_scope {
        IntegrationScope::Public => entry.install_state == IntegrationInstallState::Available,
        IntegrationScope::Personal => entry.install_state != IntegrationInstallState::Available,
    };
    let query = state.presentation.integration_search.trim().to_lowercase();
    let query_matches = query.is_empty()
        || entry.name.to_lowercase().contains(&query)
        || entry.description.to_lowercase().contains(&query)
        || entry.category.title().to_lowercase().contains(&query)
        || entry
            .source_id
            .as_deref()
            .is_some_and(|source_id| source_id.to_lowercase().contains(&query));
    tab_matches && scope_matches && query_matches
}

fn catalog_matches_active_workspace(state: &ZodeAppState) -> bool {
    matches!(
        &state.presentation.integrations,
        LoadState::Ready(catalog)
            if state.active_available_workspace() == Some(&catalog.workspace_uri)
    )
}

fn paint_tabs(painter: &mut dyn Painter, layout: &IntegrationsPageLayout, theme: &ZodeTheme) {
    for tab in layout.tabs {
        if tab.selected {
            painter.fill_round_rect(tab.rect, 10.0, theme.tokens.row_selected);
        }
        paint_single_line(
            painter,
            tab.label,
            tab.rect,
            13.0,
            if tab.selected { 650 } else { 450 },
            if tab.selected {
                theme.tokens.foreground
            } else {
                theme.tokens.muted_foreground
            },
            HorizontalAlign::Center,
        );
    }
}

fn paint_header(
    painter: &mut dyn Painter,
    layout: &IntegrationsPageLayout,
    tab: IntegrationsTab,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    paint_single_line(
        painter,
        match tab {
            IntegrationsTab::Plugins => "插件",
            IntegrationsTab::Skills => "技能",
        },
        layout.title,
        28.0,
        650,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    paint_single_line(
        painter,
        match tab {
            IntegrationsTab::Plugins => "在常用工具与 Zode 协作",
            IntegrationsTab::Skills => "通过可复用指令扩展 Zode 工作流",
        },
        layout.subtitle,
        15.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );

    let search = TextInputState::with_text(state.presentation.integration_search.clone());
    Input {
        state: &search,
        placeholder: "搜索插件或技能",
        focused: focused == Some(INTEGRATIONS_SEARCH_ID),
        font_size: 13.0,
        now_ms: 0,
        icon_d: Some(SemanticIcon::Search.path()),
    }
    .paint(painter, layout.search, &theme.tokens);
    for scope in layout.scopes {
        if scope.selected {
            painter.fill_round_rect(scope.rect, 9.0, theme.tokens.row_selected);
        }
        paint_single_line(
            painter,
            scope.label,
            scope.rect,
            13.0,
            if scope.selected { 600 } else { 450 },
            if scope.selected {
                theme.tokens.foreground
            } else {
                theme.tokens.muted_foreground
            },
            HorizontalAlign::Center,
        );
    }
}

fn paint_load_state(
    painter: &mut dyn Painter,
    layout: &IntegrationsPageLayout,
    message: &str,
    theme: &ZodeTheme,
) {
    let panel = Rect::xywh(
        layout.content.origin.x,
        layout.content.origin.y + 190.0,
        layout.content.size.x,
        112.0,
    );
    painter.fill_round_rect(panel, 12.0, theme.tokens.card);
    painter.stroke_round_rect(panel, 12.0, theme.tokens.border, 1.0);
    paint_single_line(
        painter,
        message,
        Rect::xywh(
            panel.origin.x + 16.0,
            panel.origin.y,
            (panel.size.x - 32.0).max(0.0),
            panel.size.y,
        ),
        14.0,
        500,
        theme.tokens.muted_foreground,
        HorizontalAlign::Center,
    );
}
