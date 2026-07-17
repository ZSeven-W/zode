mod archived;
mod computer_use;
mod details;
mod general;
mod navigation;
mod permissions;
mod provider_models;
mod row;

use jian_widgets::{
    components::{radio::Radio, switch::Switch},
    HorizontalAlign, Painter, Rect,
};
use zode_app_model::{
    AppCommand, SettingsCategory, ShellPage, ShellRoute, ThemePreference, ZodeAppState,
};
use zode_node_protocol::WorkspaceUri;

use crate::{
    paint_single_line, RectExt, WidgetId, WorkspaceSnapshot, ZodeTheme, HIGH_CONTRAST_ID,
    REDUCED_MOTION_ID, THEME_DARK_ID, THEME_LIGHT_ID, THEME_SYSTEM_ID,
};

pub use archived::{
    ArchivedTaskGroupLayout, ArchivedTaskRowLayout, ArchivedTasksLayout, ARCHIVED_TASK_FILTER_ID,
    ARCHIVED_TASK_SEARCH_ID,
};
pub use computer_use::{
    AllowedAppRowLayout, ComputerUseLayout, PermissionStatusRowLayout, COMPUTER_ALLOWED_APP_ADD_ID,
    COMPUTER_ALLOWED_APP_INPUT_ID,
};
pub use general::GeneralSettingsLayout;
pub use navigation::{
    SettingsNavigationEntryLayout, SettingsNavigationGroupLayout, SettingsNavigationLayout,
    SETTINGS_BACK_ID, SETTINGS_SEARCH_ID,
};
pub use permissions::{PermissionPresetLayout, PermissionRow, PermissionRowLayout};
pub use provider_models::{
    provider_model_widget_id, provider_remove_widget_id, provider_widget_id, ProviderEditorLayout,
    ProviderFieldLayout, ProviderKindLayout, ProviderModelLayout, ProviderModelsLayout,
    ProviderRowLayout, PROVIDER_ADD_ID, PROVIDER_API_KEY_INPUT_ID, PROVIDER_BASE_URL_INPUT_ID,
    PROVIDER_CANCEL_ID, PROVIDER_DEFAULT_MODEL_INPUT_ID, PROVIDER_ID_INPUT_ID,
    PROVIDER_KIND_ANTHROPIC_ID, PROVIDER_KIND_OLLAMA_ID, PROVIDER_KIND_OPENAI_ID,
    PROVIDER_MODEL_IDS_INPUT_ID, PROVIDER_SAVE_ID,
};
pub use row::SettingRowLayout;

use row::{
    clip_to_viewport, content_rect, paint_card, paint_divider, paint_heading,
    APPEARANCE_ROW_HEIGHT, SECTION_TOP,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SettingControl {
    pub label: String,
    pub selected: bool,
    pub command: AppCommand,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingControlLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub visible_rect: Rect,
    pub indicator_rect: Rect,
    pub control: SettingControl,
}

/// Frozen settings geometry consumed by paint, hit testing, focus and a11y.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsPanelLayout {
    pub sidebar: Rect,
    pub content: Rect,
    pub scroll_offset: f32,
    pub navigation: SettingsNavigationLayout,
    pub archived: ArchivedTasksLayout,
    pub general: GeneralSettingsLayout,
    pub provider_models: ProviderModelsLayout,
    pub computer_use: ComputerUseLayout,
}

pub struct SettingsPanel;

impl SettingsPanel {
    pub fn layout(
        sidebar: Rect,
        primary_surface: Rect,
        state: &ZodeAppState,
    ) -> SettingsPanelLayout {
        let content = content_rect(primary_surface);
        let scroll_offset = scroll_offset(content, state);
        SettingsPanelLayout {
            sidebar,
            content,
            scroll_offset,
            navigation: navigation::layout(sidebar, state),
            archived: archived::layout(content, state, scroll_offset),
            general: general::layout(content, state, scroll_offset),
            provider_models: provider_models::layout(content, state, scroll_offset),
            computer_use: computer_use::layout(content, state, scroll_offset),
        }
    }

    pub fn page_layout(primary_surface: Rect) -> (Rect, Rect) {
        let content = content_rect(primary_surface);
        (
            content,
            Rect::xywh(
                content.origin.x,
                content.origin.y + SECTION_TOP,
                content.size.x,
                permissions::PERMISSION_PRESET_HEIGHT * 3.0,
            ),
        )
    }

    pub fn active_category(state: &ZodeAppState) -> SettingsCategory {
        match state.presentation.route {
            ShellRoute::Settings(category) => category,
            _ if state.shell.page == ShellPage::Settings => SettingsCategory::Appearance,
            _ => SettingsCategory::General,
        }
    }

    pub const fn category_widget_id(category: SettingsCategory) -> WidgetId {
        navigation::category_widget_id(category)
    }

    pub fn navigation_entries(
        sidebar: Rect,
        state: &ZodeAppState,
    ) -> Vec<SettingsNavigationEntryLayout> {
        navigation::layout(sidebar, state).entries
    }

    pub fn active_workspace_uri(state: &ZodeAppState) -> Option<&WorkspaceUri> {
        state
            .active_available_workspace()
            .or_else(|| {
                state
                    .current_session
                    .as_ref()
                    .and_then(|session| state.available_workspace_for_session(session))
                    .filter(|workspace| !state.is_projectless_workspace(workspace))
            })
            .or_else(|| {
                if state.projectless_workspace_root.is_some() {
                    return None;
                }
                state
                    .projects
                    .iter()
                    .find(|project| project.available)
                    .map(|project| &project.workspace_uri)
            })
    }

    pub fn appearance_controls(state: &ZodeAppState) -> Vec<SettingControl> {
        vec![
            SettingControl {
                label: "跟随系统".into(),
                selected: state.ui_preferences.theme == ThemePreference::System,
                command: AppCommand::SetThemePreference(ThemePreference::System),
            },
            SettingControl {
                label: "浅色".into(),
                selected: state.ui_preferences.theme == ThemePreference::Light,
                command: AppCommand::SetThemePreference(ThemePreference::Light),
            },
            SettingControl {
                label: "深色".into(),
                selected: state.ui_preferences.theme == ThemePreference::Dark,
                command: AppCommand::SetThemePreference(ThemePreference::Dark),
            },
            SettingControl {
                label: "减少动画".into(),
                selected: state.ui_preferences.reduced_motion,
                command: AppCommand::SetReducedMotion(!state.ui_preferences.reduced_motion),
            },
            SettingControl {
                label: "高对比度".into(),
                selected: state.ui_preferences.high_contrast,
                command: AppCommand::SetHighContrast(!state.ui_preferences.high_contrast),
            },
        ]
    }

    pub fn appearance_control_layout(
        content: Rect,
        state: &ZodeAppState,
    ) -> Vec<SettingControlLayout> {
        if Self::active_category(state) != SettingsCategory::Appearance {
            return Vec::new();
        }
        let offset = scroll_offset(content, state);
        let card = appearance_card_rect(content, offset);
        [
            THEME_SYSTEM_ID,
            THEME_LIGHT_ID,
            THEME_DARK_ID,
            REDUCED_MOTION_ID,
            HIGH_CONTRAST_ID,
        ]
        .into_iter()
        .zip(Self::appearance_controls(state))
        .enumerate()
        .filter_map(|(index, (id, control))| {
            let rect = Rect::xywh(
                card.origin.x,
                card.origin.y + index as f32 * APPEARANCE_ROW_HEIGHT,
                card.size.x,
                APPEARANCE_ROW_HEIGHT,
            );
            let indicator_size = if index < 3 {
                (18.0, 18.0)
            } else {
                (32.0, 18.0)
            };
            Some(SettingControlLayout {
                id,
                rect,
                visible_rect: clip_to_viewport(rect, content)?,
                indicator_rect: Rect::xywh(
                    rect.max_x() - indicator_size.0 - 18.0,
                    rect.origin.y + (rect.size.y - indicator_size.1) / 2.0,
                    indicator_size.0,
                    indicator_size.1,
                ),
                control,
            })
        })
        .collect()
    }

    pub fn permission_rows(
        state: &ZodeAppState,
        workspace_uri: &WorkspaceUri,
    ) -> Vec<PermissionRow> {
        permissions::permission_rows(state, workspace_uri)
    }

    pub fn permission_widget_id(workspace_uri: &WorkspaceUri, tool: &str) -> WidgetId {
        permissions::permission_widget_id(workspace_uri, tool)
    }

    pub fn permission_row_layout(
        content: Rect,
        state: &ZodeAppState,
        workspace_uri: &WorkspaceUri,
    ) -> Vec<PermissionRowLayout> {
        if Self::active_category(state) != SettingsCategory::Permissions {
            return Vec::new();
        }
        permissions::permission_row_layout(content, state, workspace_uri)
    }

    pub fn permission_preset_layout(
        content: Rect,
        state: &ZodeAppState,
    ) -> Vec<PermissionPresetLayout> {
        if Self::active_category(state) != SettingsCategory::General {
            return Vec::new();
        }
        let offset = scroll_offset(content, state);
        let card = Rect::xywh(
            content.origin.x,
            content.origin.y + SECTION_TOP - offset,
            content.size.x,
            permissions::PERMISSION_PRESET_HEIGHT * 3.0,
        );
        permissions::preset_layouts(card, content, state)
    }

    pub fn archived_task_layout(content: Rect, state: &ZodeAppState) -> ArchivedTasksLayout {
        archived::layout(content, state, scroll_offset(content, state))
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        navigation::command_for_widget(id)
            .or_else(|| permissions::command_for_preset(state, id))
            .or_else(|| general::command_for_widget(state, id))
            .or_else(|| archived::command_for_widget(state, id))
            .or_else(|| {
                (Self::active_category(state) == SettingsCategory::ProviderModels)
                    .then(|| provider_models::command_for_widget(state, id))
                    .flatten()
            })
            .or_else(|| computer_use::command_for_widget(state, id))
            .or_else(|| {
                (Self::active_category(state) == SettingsCategory::Appearance).then_some(())?;
                [
                    THEME_SYSTEM_ID,
                    THEME_LIGHT_ID,
                    THEME_DARK_ID,
                    REDUCED_MOTION_ID,
                    HIGH_CONTRAST_ID,
                ]
                .into_iter()
                .zip(Self::appearance_controls(state))
                .find(|(candidate, _)| *candidate == id)
                .map(|(_, control)| control.command)
            })
            .or_else(|| {
                state
                    .project_permissions
                    .iter()
                    .find_map(|(workspace, load)| {
                        load.ready().and_then(|tools| {
                            tools.iter().find_map(|tool| {
                                (permissions::permission_widget_id(workspace, tool) == id).then(
                                    || AppCommand::RevokeProjectPermission {
                                        workspace_uri: workspace.clone(),
                                        tool: tool.clone(),
                                    },
                                )
                            })
                        })
                    })
            })
    }

    pub fn max_scroll_offset(content: Rect, state: &ZodeAppState) -> f32 {
        (settings_content_height(state) - content.size.y).max(0.0)
    }

    pub fn scroll_command(content: Rect, state: &ZodeAppState, delta: f32) -> AppCommand {
        let max_offset = Self::max_scroll_offset(content, state);
        let current = state.settings_scroll_offset.clamp(0.0, max_offset);
        AppCommand::SetSettingsScroll {
            offset: (current + delta).clamp(0.0, max_offset),
        }
    }

    pub fn paint_page(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        workspace_uri: Option<&WorkspaceUri>,
        theme: &ZodeTheme,
    ) {
        Self::paint_page_with_hovered(painter, snapshot, state, workspace_uri, None, theme);
    }

    pub fn paint_page_with_hovered(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        workspace_uri: Option<&WorkspaceUri>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        let layout = Self::layout(
            snapshot.layout.sidebar,
            snapshot.layout.primary_surface,
            state,
        );
        navigation::paint(
            painter,
            layout.sidebar,
            &layout.navigation,
            state,
            snapshot.focused,
            hovered,
            theme,
        );
        painter.save();
        painter.clip_rect(layout.content);
        match Self::active_category(state) {
            SettingsCategory::General => general::paint(
                painter,
                layout.content,
                &layout.general,
                state,
                layout.scroll_offset,
                theme,
            ),
            SettingsCategory::Appearance => {
                paint_appearance(painter, layout.content, state, layout.scroll_offset, theme)
            }
            SettingsCategory::Permissions => permissions::paint_permission_page(
                painter,
                layout.content,
                state,
                workspace_uri,
                layout.scroll_offset,
                theme,
            ),
            SettingsCategory::ProviderModels => provider_models::paint(
                painter,
                layout.content,
                state,
                layout.scroll_offset,
                snapshot.focused,
                theme,
            ),
            SettingsCategory::ComputerUse => computer_use::paint(
                painter,
                layout.content,
                &layout.computer_use,
                state,
                layout.scroll_offset,
                snapshot.focused,
                theme,
            ),
            SettingsCategory::Profile
            | SettingsCategory::Voice
            | SettingsCategory::Configuration
            | SettingsCategory::Personalization
            | SettingsCategory::Pets
            | SettingsCategory::KeyboardShortcuts
            | SettingsCategory::Usage
            | SettingsCategory::Account
            | SettingsCategory::AppSnapshots
            | SettingsCategory::Browser
            | SettingsCategory::Hooks
            | SettingsCategory::Connectors
            | SettingsCategory::Git
            | SettingsCategory::Environment
            | SettingsCategory::Worktree => details::paint(
                painter,
                layout.content,
                state,
                Self::active_category(state),
                layout.scroll_offset,
                theme,
            ),
            SettingsCategory::ArchivedTasks => archived::paint(
                painter,
                layout.content,
                &layout.archived,
                state,
                layout.scroll_offset,
                snapshot.focused == Some(ARCHIVED_TASK_SEARCH_ID),
                theme,
            ),
        }
        painter.restore();
    }
}

fn paint_appearance(
    painter: &mut dyn Painter,
    content: Rect,
    state: &ZodeAppState,
    offset: f32,
    theme: &ZodeTheme,
) {
    paint_heading(painter, content, "外观", "主题与动态效果", offset, theme);
    let card = appearance_card_rect(content, offset);
    paint_card(painter, card, theme);
    for (index, layout) in SettingsPanel::appearance_control_layout(content, state)
        .into_iter()
        .enumerate()
    {
        if index > 0 {
            paint_divider(painter, card, index as f32 * APPEARANCE_ROW_HEIGHT, theme);
        }
        paint_single_line(
            painter,
            &layout.control.label,
            Rect::xywh(
                layout.rect.origin.x + 18.0,
                layout.rect.origin.y,
                (layout.indicator_rect.origin.x - layout.rect.origin.x - 30.0).max(0.0),
                layout.rect.size.y,
            ),
            13.0,
            500,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        if index < 3 {
            Radio {
                selected: layout.control.selected,
                enabled: true,
            }
            .paint(painter, layout.indicator_rect, &theme.tokens);
        } else {
            Switch {
                on: layout.control.selected,
                enabled: true,
                hovered: false,
                pressed: false,
            }
            .paint(painter, layout.indicator_rect, &theme.tokens);
        }
    }
}

fn appearance_card_rect(content: Rect, offset: f32) -> Rect {
    Rect::xywh(
        content.origin.x,
        content.origin.y + SECTION_TOP - offset,
        content.size.x,
        APPEARANCE_ROW_HEIGHT * 5.0,
    )
}

fn settings_content_height(state: &ZodeAppState) -> f32 {
    match SettingsPanel::active_category(state) {
        SettingsCategory::General => general::content_height(),
        SettingsCategory::Appearance => SECTION_TOP + APPEARANCE_ROW_HEIGHT * 5.0 + 24.0,
        SettingsCategory::Permissions => {
            let count = SettingsPanel::active_workspace_uri(state)
                .map(|workspace| permissions::permission_rows(state, workspace).len())
                .unwrap_or_default();
            SECTION_TOP
                + (permissions::PERMISSION_WORKSPACE_HEIGHT
                    + count as f32 * permissions::PERMISSION_ROW_HEIGHT)
                    .max(104.0)
                + 24.0
        }
        SettingsCategory::ProviderModels => provider_models::content_height(state),
        SettingsCategory::ComputerUse => computer_use::content_height(state),
        SettingsCategory::Profile
        | SettingsCategory::Voice
        | SettingsCategory::Configuration
        | SettingsCategory::Personalization
        | SettingsCategory::Pets
        | SettingsCategory::KeyboardShortcuts
        | SettingsCategory::Usage
        | SettingsCategory::Account
        | SettingsCategory::AppSnapshots
        | SettingsCategory::Browser
        | SettingsCategory::Hooks
        | SettingsCategory::Connectors
        | SettingsCategory::Git
        | SettingsCategory::Environment
        | SettingsCategory::Worktree => {
            details::content_height(state, SettingsPanel::active_category(state))
        }
        SettingsCategory::ArchivedTasks => archived::content_height(state),
    }
}

pub(super) fn scroll_offset(content: Rect, state: &ZodeAppState) -> f32 {
    state
        .settings_scroll_offset
        .clamp(0.0, SettingsPanel::max_scroll_offset(content, state))
}
