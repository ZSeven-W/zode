use crate::{AppCommand, LoadState, NavigationOutcome, ShellPage, ShellRoute, ZodeAppState};

/// Applies new-task and project-picker commands without touching live sessions.
pub(crate) fn reduce_task_navigation(
    state: &mut ZodeAppState,
    command: &AppCommand,
) -> Option<NavigationOutcome> {
    match command {
        AppCommand::BeginTask { workspace_uri } => {
            if workspace_uri
                .as_ref()
                .is_some_and(|workspace| !state.available_workspace(workspace))
            {
                return Some(NavigationOutcome::Ignored);
            }
            let context_changed = state.active_workspace.as_ref() != workspace_uri.as_ref();
            if let Some(workspace) = workspace_uri {
                let newest = state
                    .projects
                    .iter()
                    .map(|project| project.last_opened_ms)
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                if let Some(project) = state
                    .projects
                    .iter_mut()
                    .find(|project| &project.workspace_uri == workspace)
                {
                    project.last_opened_ms = newest;
                }
            }
            state.current_session = None;
            state.session_menu = None;
            state.active_workspace.clone_from(workspace_uri);
            state.composer.queue_menu = None;
            state.composer.finish_queue_edit();
            close_project_picker(state);
            state.presentation.route = ShellRoute::Conversation;
            state.presentation.secondary_pane = None;
            if context_changed {
                state.presentation.integrations = LoadState::Idle;
            }
            state.review.open = false;
            state.shell.page = ShellPage::Conversation;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::ToggleProjectPicker => {
            if state.project_picker.open {
                close_project_picker(state);
            } else {
                state.project_picker.open = true;
                state.project_picker.search.clear();
                state.project_picker.active_index = 0;
            }
            Some(NavigationOutcome::Applied)
        }
        AppCommand::CloseProjectPicker => {
            close_project_picker(state);
            Some(NavigationOutcome::Applied)
        }
        AppCommand::SetProjectSearch(search) => {
            state.project_picker.search.clone_from(search);
            state.project_picker.active_index = 0;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::SetProjectPickerActive(index) => {
            state.project_picker.active_index = *index;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::CreateProject => {
            close_project_picker(state);
            Some(NavigationOutcome::NeedsEffect)
        }
        _ => None,
    }
}

fn close_project_picker(state: &mut ZodeAppState) {
    state.project_picker.open = false;
    state.project_picker.search.clear();
    state.project_picker.active_index = 0;
}
