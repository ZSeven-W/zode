use crate::{AppCommand, ZodeAppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCommandOutcome {
    Applied,
    Ignored,
}

pub fn reduce_settings_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> SettingsCommandOutcome {
    match command {
        AppCommand::SetProjectPermissions {
            workspace_uri,
            mut tools,
        } => {
            tools.sort();
            tools.dedup();
            state
                .project_permissions
                .insert(workspace_uri, crate::LoadState::Ready(tools));
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetThemePreference(theme) => {
            state.ui_preferences.theme = theme;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetReducedMotion(reduced_motion) => {
            state.ui_preferences.reduced_motion = reduced_motion;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetHighContrast(high_contrast) => {
            state.ui_preferences.high_contrast = high_contrast;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetTaskSuggestions(visible) => {
            state.ui_preferences.task_suggestions = visible;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetSidebarTasksExpanded(expanded) => {
            state.ui_preferences.sidebar_tasks_expanded = expanded;
            state.sidebar.tasks_expanded = expanded;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetSettingsSearch(search) => {
            state.settings_search = search;
            state.settings_scroll_offset = 0.0;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetArchivedTaskSearch(search) => {
            state.archived_tasks.search = search;
            state.settings_scroll_offset = 0.0;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetArchivedTaskWorkspaceFilter(workspace_filter) => {
            if workspace_filter.as_ref().is_some_and(|workspace| {
                !state.threads.iter().any(|thread| {
                    &thread.workspace_uri == workspace
                        && state.archived_sessions.contains(&thread.session)
                })
            }) {
                return SettingsCommandOutcome::Ignored;
            }
            state.archived_tasks.workspace_filter = workspace_filter;
            state.settings_scroll_offset = 0.0;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetSettingsScroll { offset } if offset.is_finite() => {
            state.settings_scroll_offset = offset.max(0.0);
            SettingsCommandOutcome::Applied
        }
        AppCommand::RevokeProjectPermission {
            workspace_uri,
            tool,
        } => {
            let Some(crate::LoadState::Ready(tools)) =
                state.project_permissions.get_mut(&workspace_uri)
            else {
                return SettingsCommandOutcome::Ignored;
            };
            let previous_len = tools.len();
            tools.retain(|candidate| candidate != &tool);
            if tools.len() == previous_len {
                return SettingsCommandOutcome::Ignored;
            }
            SettingsCommandOutcome::Applied
        }
        _ => SettingsCommandOutcome::Ignored,
    }
}
