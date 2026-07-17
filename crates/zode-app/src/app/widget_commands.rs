use zode_app_model::{AppCommand, ThemePreference, ZodeAppState};
use zode_app_ui::{
    Composer, DocumentPreview, EnvironmentPanel, IntegrationsPage, ProjectPicker, ProjectSidebar,
    ReviewPanel, SettingsPanel, SidebarAction, ThreadHeader, ThreadTranscript, WidgetId,
    HIGH_CONTRAST_ID, PROJECT_PICKER_NEW_ID, PROJECT_PICKER_PROJECTLESS_ID,
    PROJECT_PICKER_TRIGGER_ID, REDUCED_MOTION_ID, THEME_DARK_ID, THEME_LIGHT_ID, THEME_SYSTEM_ID,
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
        .or_else(|| project_picker_command(state, id))
        .or_else(|| Composer::command_for_widget(state, id))
        .or_else(|| ThreadTranscript::command_for_widget(state, id))
}

fn project_picker_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if id == PROJECT_PICKER_TRIGGER_ID && state.current_session.is_none() {
        return Some(AppCommand::ToggleProjectPicker);
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
