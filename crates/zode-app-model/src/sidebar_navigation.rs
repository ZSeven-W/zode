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
        AppCommand::ToggleSidebarTasks => {
            state.sidebar.tasks_expanded = !state.sidebar.tasks_expanded;
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
