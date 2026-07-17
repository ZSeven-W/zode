use zode_app_model::{
    AppCommand, ComposerContextMenu, ComposerFooterMenu, ThemePreference, ZodeAppState,
};
use zode_app_ui::{
    Composer, ComposerContextMenu as ComposerContextMenuWidget, ComposerFooterMenuWidget,
    DocumentPreview, EnvironmentPanel, IntegrationsPage, PinnedSummaryMode, ProjectPicker,
    ProjectSidebar, ReviewPanel, SettingsPanel, SidebarAction, TerminalSecondaryPanel,
    ThreadHeader, ThreadTranscript, UnavailableSecondaryPanel, WidgetId, WorkspaceSnapshot,
    COMPOSER_ADD_ID, COMPOSER_BRANCH_ID, COMPOSER_LOCATION_ID, COMPOSER_MODEL_ID,
    COMPOSER_PERMISSION_ID, COMPOSER_PROJECT_ID, ENVIRONMENT_CLOSE_ID, HEADER_ENVIRONMENT_ID,
    HIGH_CONTRAST_ID, PROJECT_PICKER_NEW_ID, PROJECT_PICKER_PROJECTLESS_ID,
    PROJECT_PICKER_TRIGGER_ID, REDUCED_MOTION_ID, SECONDARY_PANE_BREAKPOINT, THEME_DARK_ID,
    THEME_LIGHT_ID, THEME_SYSTEM_ID,
};

pub(super) fn widget_command_for_snapshot(
    state: &ZodeAppState,
    snapshot: &WorkspaceSnapshot,
    id: WidgetId,
) -> Option<AppCommand> {
    if matches!(id, HEADER_ENVIRONMENT_ID | ENVIRONMENT_CLOSE_ID) && state.current_session.is_some()
    {
        return match snapshot.layout.pinned_summary {
            PinnedSummaryMode::Docked => Some(AppCommand::SetPinnedSummaryAutoHidden(true)),
            PinnedSummaryMode::Overlay => Some(AppCommand::SetPinnedSummaryOverlayOpen(false)),
            PinnedSummaryMode::Hidden if snapshot.layout.viewport.size.x > 0.0 => {
                if !state.presentation.secondary_sidebar_open
                    && snapshot.layout.viewport.size.x >= SECONDARY_PANE_BREAKPOINT
                    && state.presentation.pinned_summary_auto_hidden
                {
                    Some(AppCommand::SetPinnedSummaryAutoHidden(false))
                } else {
                    Some(AppCommand::SetPinnedSummaryOverlayOpen(true))
                }
            }
            PinnedSummaryMode::Hidden => None,
        };
    }
    widget_command(state, id)
}

pub(super) fn widget_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    static_sidebar_command(state, id)
        .or_else(|| ProjectSidebar::command_for_widget(state, id))
        .or_else(|| ThreadHeader::command_for_widget(state, id))
        .or_else(|| IntegrationsPage::command_for_widget(state, id))
        .or_else(|| SettingsPanel::command_for_widget(state, id))
        .or_else(|| EnvironmentPanel::command_for_widget(state, id))
        .or_else(|| ReviewPanel::command_for_widget(state, id))
        .or_else(|| DocumentPreview::command_for_widget(state, id))
        .or_else(|| TerminalSecondaryPanel::command_for_widget(state, id))
        .or_else(|| UnavailableSecondaryPanel::command_for_widget(state, id))
        .or_else(|| appearance_command(state, id))
        .or_else(|| project_picker_command(state, id))
        .or_else(|| composer_context_command(state, id))
        .or_else(|| ComposerContextMenuWidget::command_for_widget(state, id))
        .or_else(|| composer_footer_command(id))
        .or_else(|| ComposerFooterMenuWidget::command_for_widget(state, id))
        .or_else(|| Composer::command_for_widget(state, id))
        .or_else(|| ThreadTranscript::command_for_widget(state, id))
}

fn composer_footer_command(id: WidgetId) -> Option<AppCommand> {
    match id {
        COMPOSER_ADD_ID => Some(AppCommand::ToggleComposerFooterMenu(
            ComposerFooterMenu::Add,
        )),
        COMPOSER_PERMISSION_ID => Some(AppCommand::ToggleComposerFooterMenu(
            ComposerFooterMenu::Permission,
        )),
        COMPOSER_MODEL_ID => Some(AppCommand::ToggleComposerFooterMenu(
            ComposerFooterMenu::Model,
        )),
        _ => None,
    }
}

fn project_picker_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if id == PROJECT_PICKER_TRIGGER_ID && state.current_session.is_none() {
        return Some(AppCommand::ToggleProjectPicker);
    }
    if id == COMPOSER_PROJECT_ID && state.current_session.is_none() {
        return Some(AppCommand::ToggleComposerProjectPicker);
    }
    if id == PROJECT_PICKER_NEW_ID && state.project_picker.open {
        return Some(AppCommand::CreateProject);
    }
    if id == PROJECT_PICKER_PROJECTLESS_ID && state.project_picker.open {
        return Some(AppCommand::BeginTask {
            workspace_uri: None,
        });
    }
    if !state.project_picker.open {
        return None;
    }
    ProjectPicker::choices(state, &state.project_picker.search)
        .into_iter()
        .find(|choice| ProjectPicker::project_widget_id(&choice.workspace_uri) == id)
        .map(|choice| AppCommand::BeginTask {
            workspace_uri: Some(choice.workspace_uri),
        })
}

fn composer_context_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if state.current_session.is_some() {
        return None;
    }
    match id {
        COMPOSER_LOCATION_ID => Some(AppCommand::ToggleComposerContextMenu(
            ComposerContextMenu::Location,
        )),
        COMPOSER_BRANCH_ID if state.active_available_workspace().is_some() => Some(
            AppCommand::ToggleComposerContextMenu(ComposerContextMenu::Branch),
        ),
        _ => None,
    }
}

fn static_sidebar_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    let action = match id.0 {
        2..=7 => {
            ProjectSidebar::navigation_items()
                .get((id.0 - 2) as usize)?
                .action
        }
        9 => ProjectSidebar::footer_item().action,
        10 => SidebarAction::Navigate(zode_app_model::ShellRoute::ComingSoon(
            zode_app_model::ComingSoonFeature::Help,
        )),
        _ => return None,
    };
    match action {
        SidebarAction::NewSession => new_session_command(state),
        SidebarAction::Navigate(route) => Some(AppCommand::Navigate(route)),
    }
}

fn new_session_command(state: &ZodeAppState) -> Option<AppCommand> {
    Some(AppCommand::BeginTask {
        workspace_uri: state.active_available_workspace().cloned(),
    })
}

fn appearance_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    match id {
        THEME_SYSTEM_ID => Some(AppCommand::SetThemePreference(ThemePreference::System)),
        THEME_LIGHT_ID => Some(AppCommand::SetThemePreference(ThemePreference::Light)),
        THEME_DARK_ID => Some(AppCommand::SetThemePreference(ThemePreference::Dark)),
        REDUCED_MOTION_ID => Some(AppCommand::SetReducedMotion(
            !state.ui_preferences.reduced_motion,
        )),
        HIGH_CONTRAST_ID => Some(AppCommand::SetHighContrast(
            !state.ui_preferences.high_contrast,
        )),
        _ => None,
    }
}
