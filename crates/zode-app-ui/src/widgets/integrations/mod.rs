mod add_form;
mod catalog;
mod installed;
mod plugin_detail;
mod plugin_rows;
mod row;

use jian_core::text_input::TextInputState;
use jian_widgets::{components::input::Input, HorizontalAlign, Painter, Rect};
use zode_app_model::{
    AppCommand, Availability, IntegrationEntry, IntegrationInstallState, IntegrationScope,
    IntegrationsTab, LoadState, PluginDetailMode, ShellRoute, ZodeAppState,
};

use crate::{
    layout::CONTENT_W, paint_single_line, Button, ButtonVariant, SemanticIcon, WidgetId, ZodeTheme,
};

pub use add_form::{
    PluginAddFormLayout, PLUGIN_ADD_CANCEL_ID, PLUGIN_ADD_REFERENCE_INPUT_ID,
    PLUGIN_ADD_SPEC_INPUT_ID, PLUGIN_ADD_SUBMIT_ID,
};
pub use catalog::CatalogSectionLayout;
pub use installed::InstalledIconLayout;
pub use plugin_detail::{
    CapabilityRowLayout, PluginDetailBody, PluginDetailOverlayLayout, TrustItemRowLayout,
    PLUGIN_DETAIL_CHECK_UPDATE_ID, PLUGIN_DETAIL_CLOSE_ID, PLUGIN_DETAIL_TRUST_ALL_ID,
    PLUGIN_DETAIL_TRUST_CANCEL_ID, PLUGIN_DETAIL_TRUST_GRANT_SELECTED_ID,
    PLUGIN_DETAIL_UNINSTALL_CANCEL_ID, PLUGIN_DETAIL_UNINSTALL_CONFIRM_ID,
    PLUGIN_DETAIL_UNINSTALL_ID,
};
pub use plugin_rows::PluginRowLayout;
pub use row::IntegrationRowLayout;

pub const INTEGRATIONS_PLUGINS_TAB_ID: WidgetId = WidgetId(70);
pub const INTEGRATIONS_SKILLS_TAB_ID: WidgetId = WidgetId(71);
pub const INTEGRATIONS_SEARCH_ID: WidgetId = WidgetId(190);
pub const INTEGRATIONS_PUBLIC_SCOPE_ID: WidgetId = WidgetId(191);
pub const INTEGRATIONS_PERSONAL_SCOPE_ID: WidgetId = WidgetId(192);
pub const INTEGRATIONS_ADD_PLUGIN_ID: WidgetId = WidgetId(300);

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
    /// Only non-empty on the Plugins tab - the git-install entry point.
    pub add_plugin_button: Rect,
    pub scopes: [IntegrationScopeLayout; 2],
    pub installed_title: Rect,
    pub installed_strip: Rect,
    pub directory_status: Rect,
    /// Reserved strip for [`plugin_rows`] above the catalog. Zero height
    /// (and thus no layout shift at all) unless the Plugins tab has at
    /// least one installed plugin bundle.
    pub plugins_section: Rect,
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
        paint_plugin_rows(painter, &layout, state, theme);
        painter.restore();

        if state.presentation.plugin_add.open {
            if let Some(form) = Self::plugin_add_form_layout(rect, state) {
                add_form::paint(painter, rect, &form, state, focused, theme);
            }
        }
        if let Some(detail) = Self::plugin_detail_layout(rect, state) {
            plugin_detail::paint(painter, rect, &detail, theme);
        }
    }

    pub fn layout(rect: Rect, state: &ZodeAppState) -> IntegrationsPageLayout {
        let content_width = rect.size.x.clamp(0.0, CONTENT_W);
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

        let show_add_button = selected == IntegrationsTab::Plugins;
        const ADD_BUTTON_W: f32 = 96.0;
        let search = if show_add_button {
            Rect::xywh(
                content.origin.x,
                content.origin.y + 100.0,
                (content.size.x - ADD_BUTTON_W - 8.0).max(0.0),
                34.0,
            )
        } else {
            Rect::xywh(
                content.origin.x,
                content.origin.y + 100.0,
                content.size.x,
                34.0,
            )
        };
        let add_plugin_button = if show_add_button {
            Rect::xywh(
                content.origin.x + content.size.x - ADD_BUTTON_W,
                content.origin.y + 100.0,
                ADD_BUTTON_W,
                34.0,
            )
        } else {
            Rect::xywh(content.origin.x, content.origin.y + 100.0, 0.0, 0.0)
        };

        let plugin_count = if show_add_button {
            state
                .presentation
                .installed_plugins
                .ready()
                .map(Vec::len)
                .unwrap_or(0)
        } else {
            0
        };
        let directory_status_bottom = content.origin.y + 278.0 + 22.0;
        let plugins_section_h = if plugin_count > 0 {
            22.0 + 8.0 + plugin_rows::strip_height(plugin_count) + 12.0
        } else {
            0.0
        };
        let plugins_section = Rect::xywh(
            content.origin.x,
            directory_status_bottom + 8.0,
            content.size.x,
            (plugins_section_h - 8.0).max(0.0),
        );
        let catalog_top = directory_status_bottom + 8.0 + plugins_section_h;

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
            search,
            add_plugin_button,
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
            plugins_section,
            catalog: Rect::xywh(
                content.origin.x,
                catalog_top,
                content.size.x,
                (content.size.y - (catalog_top - content.origin.y)).max(0.0),
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

    pub fn max_scroll_offset(rect: Rect, state: &ZodeAppState) -> f32 {
        if !catalog_matches_active_workspace(state) {
            return 0.0;
        }
        catalog::max_scroll_offset(&Self::layout(rect, state), state)
    }

    pub fn scroll_offset(rect: Rect, state: &ZodeAppState) -> f32 {
        let max_offset = Self::max_scroll_offset(rect, state);
        if !state.integration_scroll_offset.is_finite() {
            return 0.0;
        }
        state.integration_scroll_offset.clamp(0.0, max_offset)
    }

    pub fn scroll_command(rect: Rect, state: &ZodeAppState, delta: f32) -> Option<AppCommand> {
        if !delta.is_finite() {
            return None;
        }
        let max_offset = Self::max_scroll_offset(rect, state);
        let current = Self::scroll_offset(rect, state);
        let offset = (current + delta).clamp(0.0, max_offset);
        (offset != current).then_some(AppCommand::SetIntegrationsScroll { offset })
    }

    pub fn row_widget_id(source_id: &str) -> WidgetId {
        row::widget_id(source_id)
    }

    pub fn row_action_widget_id(source_id: &str) -> WidgetId {
        row::action_widget_id(source_id)
    }

    /// Rows for the compact "已安装插件" strip - empty unless the Plugins
    /// tab has at least one git-installed plugin bundle.
    pub fn plugin_row_layout(rect: Rect, state: &ZodeAppState) -> Vec<PluginRowLayout> {
        if !matches!(
            state.presentation.route,
            ShellRoute::Integrations(IntegrationsTab::Plugins)
        ) {
            return Vec::new();
        }
        let Some(plugins) = state.presentation.installed_plugins.ready() else {
            return Vec::new();
        };
        if plugins.is_empty() {
            return Vec::new();
        }
        let layout = Self::layout(rect, state);
        plugin_rows::layout(
            layout.plugins_section.origin.x,
            layout.plugins_section.origin.y + 22.0 + 8.0,
            layout.plugins_section.size.x,
            plugins,
        )
    }

    /// `Some` only while `presentation.plugin_add.open` is true.
    pub fn plugin_add_form_layout(rect: Rect, state: &ZodeAppState) -> Option<PluginAddFormLayout> {
        if !state.presentation.plugin_add.open {
            return None;
        }
        let content = Self::layout(rect, state).content;
        Some(PluginAddFormLayout::new(content, state))
    }

    /// `Some` only while `presentation.plugin_detail` names a plugin that is
    /// still present in the loaded installed-plugins catalog.
    pub fn plugin_detail_layout(
        rect: Rect,
        state: &ZodeAppState,
    ) -> Option<PluginDetailOverlayLayout> {
        let content = Self::layout(rect, state).content;
        PluginDetailOverlayLayout::new(content, state)
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if let Some(command) = plugin_market_command_for_widget(state, id) {
            return Some(command);
        }
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
            INTEGRATIONS_SEARCH_ID | PLUGIN_ADD_SPEC_INPUT_ID | PLUGIN_ADD_REFERENCE_INPUT_ID => {
                return None
            }
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

/// Dispatches every plugin-market-specific widget id: the add-plugin form,
/// the installed-plugin strip's rows, and the detail/trust-review overlay.
/// Kept as one function (rather than folded into `command_for_widget`'s own
/// body) so its many small `if id == ...` checks don't have to interleave
/// with the pre-existing catalog-row dispatch above.
fn plugin_market_command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if id == INTEGRATIONS_ADD_PLUGIN_ID {
        return Some(AppCommand::SetPluginAddOpen(true));
    }
    if state.presentation.plugin_add.open {
        if id == PLUGIN_ADD_CANCEL_ID {
            return Some(AppCommand::SetPluginAddOpen(false));
        }
        if id == PLUGIN_ADD_SUBMIT_ID {
            let can_submit = !state.presentation.plugin_add.spec.trim().is_empty()
                && state.presentation.plugin_add.status
                    != zode_app_model::PluginAddStatus::Installing;
            return can_submit.then_some(AppCommand::InstallPlugin);
        }
    }
    if matches!(
        state.presentation.route,
        ShellRoute::Integrations(IntegrationsTab::Plugins)
    ) {
        if let Some(plugins) = state.presentation.installed_plugins.ready() {
            if let Some(plugin) = plugins
                .iter()
                .find(|plugin| plugin_rows::plugin_row_widget_id(&plugin.id) == id)
            {
                return Some(AppCommand::OpenPluginDetail(plugin.id.clone()));
            }
        }
    }
    let detail = state.presentation.plugin_detail.as_ref()?;
    let plugins = state.presentation.installed_plugins.ready()?;
    let plugin = plugins
        .iter()
        .find(|plugin| plugin.id == detail.plugin_id)?;
    if id == PLUGIN_DETAIL_CLOSE_ID {
        return Some(AppCommand::ClosePluginDetail);
    }
    match &detail.mode {
        PluginDetailMode::Overview => {
            if id == PLUGIN_DETAIL_UNINSTALL_ID {
                return Some(AppCommand::RequestUninstallPlugin);
            }
            if id == PLUGIN_DETAIL_CHECK_UPDATE_ID {
                return Some(AppCommand::CheckPluginUpdate);
            }
            let pending_keys = match &plugin.trust {
                zode_node_protocol::PluginTrustState::Trusted => &[][..],
                zode_node_protocol::PluginTrustState::NeedsReview(keys)
                | zode_node_protocol::PluginTrustState::Drifted(keys) => keys.as_slice(),
            };
            plugin.capabilities.iter().find_map(|capability| {
                let source_id = capability.toggle_source_id.as_ref()?;
                if plugin_detail::capability_toggle_widget_id(&detail.plugin_id, source_id) != id {
                    return None;
                }
                if pending_keys.contains(&capability.key) {
                    return Some(AppCommand::RequestPluginTrustReview);
                }
                let catalog = state.presentation.integrations.ready()?;
                let entry = catalog
                    .all_entries()
                    .find(|entry| entry.source_id.as_deref() == Some(source_id.as_str()))?;
                Some(AppCommand::SetIntegrationEnabled {
                    workspace_uri: catalog.workspace_uri.clone(),
                    source_id: source_id.clone(),
                    enabled: entry.availability == Availability::Disabled,
                })
            })
        }
        PluginDetailMode::ConfirmUninstall => {
            if id == PLUGIN_DETAIL_UNINSTALL_CONFIRM_ID {
                return Some(AppCommand::ConfirmUninstallPlugin);
            }
            if id == PLUGIN_DETAIL_UNINSTALL_CANCEL_ID {
                return Some(AppCommand::CancelUninstallPlugin);
            }
            None
        }
        PluginDetailMode::Uninstalling => None,
        PluginDetailMode::TrustReview { review, selected } => {
            if id == PLUGIN_DETAIL_TRUST_CANCEL_ID {
                return Some(AppCommand::CancelPluginTrustReview);
            }
            if id == PLUGIN_DETAIL_TRUST_ALL_ID {
                return Some(AppCommand::GrantPluginTrust { keys: None });
            }
            if id == PLUGIN_DETAIL_TRUST_GRANT_SELECTED_ID && !selected.is_empty() {
                return Some(AppCommand::GrantPluginTrust {
                    keys: Some(selected.iter().cloned().collect()),
                });
            }
            let items = review.ready()?;
            items.items.iter().find_map(|item| {
                (plugin_detail::trust_item_checkbox_widget_id(&detail.plugin_id, &item.key) == id)
                    .then(|| AppCommand::ToggleTrustItemSelected(item.key.clone()))
            })
        }
    }
}

fn paint_plugin_rows(
    painter: &mut dyn Painter,
    layout: &IntegrationsPageLayout,
    state: &ZodeAppState,
    theme: &ZodeTheme,
) {
    if layout.plugins_section.size.y <= 0.0 {
        return;
    }
    let Some(plugins) = state.presentation.installed_plugins.ready() else {
        return;
    };
    paint_single_line(
        painter,
        "已安装插件",
        Rect::xywh(
            layout.plugins_section.origin.x,
            layout.plugins_section.origin.y,
            layout.plugins_section.size.x,
            20.0,
        ),
        13.0,
        600,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    let rows = plugin_rows::layout(
        layout.plugins_section.origin.x,
        layout.plugins_section.origin.y + 22.0 + 8.0,
        layout.plugins_section.size.x,
        plugins,
    );
    plugin_rows::paint(painter, &rows, plugins.len(), theme);
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
    if layout.add_plugin_button.size.x > 0.0 {
        Button::paint(
            painter,
            layout.add_plugin_button,
            8.0,
            "添加插件",
            None,
            ButtonVariant::Secondary,
            false,
            &theme.tokens,
        );
    }
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
