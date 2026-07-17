use zode_app_model::{AppCommand, ThemePreference, ZodeAppState};
use zode_app_ui::{
    Composer, DocumentPreview, EnvironmentPanel, IntegrationsPage, ProjectSidebar, ReviewPanel,
    SettingsPanel, SidebarAction, ThreadHeader, ThreadTranscript, WidgetId, HIGH_CONTRAST_ID,
    REDUCED_MOTION_ID, THEME_DARK_ID, THEME_LIGHT_ID, THEME_SYSTEM_ID,
};

pub(super) fn widget_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    static_sidebar_command(state, id)
        .or_else(|| ProjectSidebar::command_for_widget(state, id))
        .or_else(|| ThreadHeader::command_for_widget(state, id))
        .or_else(|| IntegrationsPage::command_for_widget(id))
        .or_else(|| SettingsPanel::command_for_widget(state, id))
        .or_else(|| EnvironmentPanel::command_for_widget(state, id))
        .or_else(|| ReviewPanel::command_for_widget(state, id))
        .or_else(|| DocumentPreview::command_for_widget(state, id))
        .or_else(|| appearance_command(state, id))
        .or_else(|| Composer::command_for_widget(state, id))
        .or_else(|| ThreadTranscript::command_for_widget(state, id))
}

fn static_sidebar_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    let action = match id.0 {
        2..=7 => {
            ProjectSidebar::navigation_items()
                .get((id.0 - 2) as usize)?
                .action
        }
        9 => ProjectSidebar::footer_item().action,
        _ => return None,
    };
    match action {
        SidebarAction::NewSession => new_session_command(state),
        SidebarAction::Navigate(route) => Some(AppCommand::Navigate(route)),
    }
}

fn new_session_command(state: &ZodeAppState) -> Option<AppCommand> {
    state
        .active_available_workspace()
        .cloned()
        .or_else(|| {
            state
                .projects
                .iter()
                .find(|project| project.available)
                .map(|project| project.workspace_uri.clone())
        })
        .map(|workspace_uri| AppCommand::NewSession { workspace_uri })
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
