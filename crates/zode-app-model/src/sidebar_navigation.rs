use crate::{AppCommand, NavigationOutcome, ZodeAppState};

pub(crate) fn reduce_sidebar_navigation(
    state: &mut ZodeAppState,
    command: &AppCommand,
) -> Option<NavigationOutcome> {
    let outcome = match command {
        AppCommand::SetSessionPinned { session, pinned } => {
            if !state
                .threads
                .iter()
                .any(|thread| &thread.session == session)
            {
                return Some(NavigationOutcome::Ignored);
            }
            if *pinned {
                state.pinned_sessions.insert(session.clone());
            } else {
                state.pinned_sessions.remove(session);
            }
            if state.session_menu.as_ref() == Some(session) {
                state.close_session_action_surfaces();
            }
            NavigationOutcome::NeedsEffect
        }
        AppCommand::SetSessionArchived { session, archived } => {
            if !state
                .threads
                .iter()
                .any(|thread| &thread.session == session)
            {
                return Some(NavigationOutcome::Ignored);
            }
            if *archived {
                state.archived_sessions.insert(session.clone());
            } else {
                state.archived_sessions.remove(session);
            }
            if state.session_menu.as_ref() == Some(session) {
                state.close_session_action_surfaces();
            }
            if *archived && state.current_session.as_ref() == Some(session) {
                state.current_session = None;
                state.composer.queue_menu = None;
                state.composer.finish_queue_edit();
                state.review = crate::ReviewState::default();
                state.presentation.secondary_pane = None;
            }
            NavigationOutcome::NeedsEffect
        }
        AppCommand::SetSidebarScroll { offset } if offset.is_finite() => {
            state.sidebar.scroll_offset = offset.max(0.0);
            NavigationOutcome::Applied
        }
        AppCommand::ToggleProjectMenu { workspace_uri } => {
            if !project_exists(state, workspace_uri) {
                return Some(NavigationOutcome::Ignored);
            }
            let opening = state.sidebar.project_menu.as_ref() != Some(workspace_uri);
            state.close_session_action_surfaces();
            state.composer.queue_menu = None;
            if opening {
                state.sidebar.project_menu = Some(workspace_uri.clone());
            }
            NavigationOutcome::Applied
        }
        AppCommand::ToggleSidebarSectionMenu(section) => {
            let opening = state.sidebar.section_menu != Some(*section);
            state.close_session_action_surfaces();
            state.composer.queue_menu = None;
            if opening {
                state.sidebar.section_menu = Some(*section);
            }
            NavigationOutcome::Applied
        }
        AppCommand::SetProjectPinned {
            workspace_uri,
            pinned,
        } => {
            if !project_exists(state, workspace_uri) {
                return Some(NavigationOutcome::Ignored);
            }
            if *pinned {
                state.sidebar.pinned_projects.insert(workspace_uri.clone());
            } else {
                state.sidebar.pinned_projects.remove(workspace_uri);
            }
            state.close_session_action_surfaces();
            NavigationOutcome::Applied
        }
        AppCommand::OpenProjectInFinder { workspace_uri } => {
            if !state.available_workspace(workspace_uri) {
                return Some(NavigationOutcome::Ignored);
            }
            state.close_session_action_surfaces();
            NavigationOutcome::NeedsEffect
        }
        AppCommand::ArchiveProjectTasks { workspace_uri } => {
            if !project_exists(state, workspace_uri) {
                return Some(NavigationOutcome::Ignored);
            }
            let sessions = state
                .threads
                .iter()
                .filter(|thread| state.project_workspace_for_thread(thread) == Some(workspace_uri))
                .map(|thread| thread.session.clone())
                .collect::<Vec<_>>();
            if sessions.is_empty() {
                state.close_session_action_surfaces();
                return Some(NavigationOutcome::NeedsEffect);
            }
            state.archived_sessions.extend(sessions.iter().cloned());
            if state
                .current_session
                .as_ref()
                .is_some_and(|current| sessions.contains(current))
            {
                state.current_session = None;
                state.composer.queue_menu = None;
                state.composer.finish_queue_edit();
                state.review = crate::ReviewState::default();
                state.presentation.secondary_pane = None;
            }
            state.close_session_action_surfaces();
            NavigationOutcome::NeedsEffect
        }
        AppCommand::SetProjectDisplayMode(mode) => {
            state.sidebar.project_display_mode = *mode;
            state.close_session_action_surfaces();
            NavigationOutcome::Applied
        }
        AppCommand::SetProjectSortMode(mode) => {
            state.sidebar.project_sort_mode = *mode;
            state.close_session_action_surfaces();
            NavigationOutcome::Applied
        }
        AppCommand::ToggleSidebarTasks => {
            state.sidebar.tasks_expanded = !state.sidebar.tasks_expanded;
            state.ui_preferences.sidebar_tasks_expanded = state.sidebar.tasks_expanded;
            state.close_session_action_surfaces();
            NavigationOutcome::Applied
        }
        AppCommand::ShowAllProjects => {
            state.sidebar.show_all_projects = true;
            NavigationOutcome::Applied
        }
        AppCommand::ShowAllProjectSessions { workspace_uri } => {
            if !state
                .projects
                .iter()
                .any(|project| &project.workspace_uri == workspace_uri)
            {
                return Some(NavigationOutcome::Ignored);
            }
            state
                .sidebar
                .show_all_project_sessions
                .insert(workspace_uri.clone());
            NavigationOutcome::Applied
        }
        AppCommand::SetSidebarScroll { .. } => NavigationOutcome::Ignored,
        _ => return None,
    };
    Some(outcome)
}

fn project_exists(state: &ZodeAppState, workspace_uri: &zode_node_protocol::WorkspaceUri) -> bool {
    state
        .projects
        .iter()
        .any(|project| &project.workspace_uri == workspace_uri)
}
