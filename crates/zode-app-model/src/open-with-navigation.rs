use crate::{AppCommand, LoadState, NavigationOutcome, ZodeAppState};

pub(crate) fn reduce_open_with_navigation(
    state: &mut ZodeAppState,
    command: &AppCommand,
) -> Option<NavigationOutcome> {
    let outcome = match command {
        AppCommand::ToggleOpenWithMenu => {
            if current_local_workspace(state).is_none() {
                return Some(NavigationOutcome::Ignored);
            }
            let opening = !state.open_with.menu_open;
            state.close_session_action_surfaces();
            state.open_with.menu_open = opening;
            if opening {
                if matches!(
                    state.open_with.applications,
                    LoadState::Idle | LoadState::Failed(_)
                ) {
                    state.open_with.applications = LoadState::Loading;
                }
            }
            NavigationOutcome::Applied
        }
        AppCommand::LoadExternalApplications => {
            if current_local_workspace(state).is_none() {
                return Some(NavigationOutcome::Ignored);
            }
            state.open_with.applications = LoadState::Loading;
            NavigationOutcome::NeedsEffect
        }
        AppCommand::ExternalApplicationsLoaded(applications) => {
            if !matches!(state.open_with.applications, LoadState::Loading) {
                return Some(NavigationOutcome::Ignored);
            }
            state.open_with.applications = LoadState::Ready(applications.clone());
            NavigationOutcome::Applied
        }
        AppCommand::ExternalApplicationsFailed(message) => {
            if !matches!(state.open_with.applications, LoadState::Loading) {
                return Some(NavigationOutcome::Ignored);
            }
            state.open_with.applications = LoadState::Failed(message.clone());
            NavigationOutcome::Applied
        }
        AppCommand::OpenWorkspaceExternally {
            workspace_uri,
            application,
        } => {
            if current_local_workspace(state) != Some(workspace_uri) {
                return Some(NavigationOutcome::Ignored);
            }
            let application_is_available = state
                .open_with
                .applications
                .ready()
                .is_none_or(|applications| applications.contains(application));
            if !application_is_available {
                return Some(NavigationOutcome::Ignored);
            }
            state.open_with.preferred = Some(*application);
            state.close_session_action_surfaces();
            NavigationOutcome::NeedsEffect
        }
        _ => return None,
    };
    Some(outcome)
}

fn current_local_workspace(state: &ZodeAppState) -> Option<&zode_node_protocol::WorkspaceUri> {
    state
        .current_session
        .as_ref()
        .and_then(|session| state.available_workspace_for_session(session))
        .or_else(|| state.active_available_workspace())
        .filter(|workspace| workspace.as_str().starts_with("file://"))
        .filter(|workspace| !state.is_projectless_workspace(workspace))
}

#[cfg(test)]
mod tests {
    use super::reduce_open_with_navigation;
    use crate::{
        demo_state, AppCommand, ExternalApplication, LoadState, NavigationOutcome, ProjectState,
    };
    use zode_node_protocol::WorkspaceUri;

    fn state_with_workspace() -> (crate::ZodeAppState, WorkspaceUri) {
        let mut state = demo_state();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        state.active_workspace = Some(workspace.clone());
        (state, workspace)
    }

    #[test]
    fn opening_the_picker_starts_a_catalog_load_without_a_fake_row() {
        let (mut state, _) = state_with_workspace();
        assert_eq!(
            reduce_open_with_navigation(&mut state, &AppCommand::ToggleOpenWithMenu),
            Some(NavigationOutcome::Applied)
        );
        assert!(state.open_with.menu_open);
        assert_eq!(state.open_with.applications, LoadState::Loading);
    }

    #[test]
    fn toggling_an_open_picker_closes_it() {
        let (mut state, _) = state_with_workspace();
        state.open_with.menu_open = true;
        state.open_with.applications = LoadState::Ready(vec![ExternalApplication::Finder]);

        assert_eq!(
            reduce_open_with_navigation(&mut state, &AppCommand::ToggleOpenWithMenu),
            Some(NavigationOutcome::Applied)
        );
        assert!(!state.open_with.menu_open);
        assert_eq!(
            state.open_with.applications,
            LoadState::Ready(vec![ExternalApplication::Finder])
        );
    }

    #[test]
    fn selection_is_rejected_when_the_scanned_application_is_absent() {
        let (mut state, workspace) = state_with_workspace();
        state.open_with.applications = LoadState::Ready(vec![ExternalApplication::Finder]);

        assert_eq!(
            reduce_open_with_navigation(
                &mut state,
                &AppCommand::OpenWorkspaceExternally {
                    workspace_uri: workspace,
                    application: ExternalApplication::Zed,
                },
            ),
            Some(NavigationOutcome::Ignored)
        );
        assert_eq!(state.open_with.preferred, None);
    }

    #[test]
    fn projectless_workspaces_cannot_open_the_application_picker() {
        let (mut state, workspace) = state_with_workspace();
        state.projectless_workspace_root = Some(workspace);
        assert_eq!(
            reduce_open_with_navigation(&mut state, &AppCommand::ToggleOpenWithMenu),
            Some(NavigationOutcome::Ignored)
        );
        assert!(!state.open_with.menu_open);
    }
}
