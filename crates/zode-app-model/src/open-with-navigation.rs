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
            if opening
                && matches!(
                    state.open_with.applications,
                    LoadState::Idle | LoadState::Failed(_)
                )
            {
                state.open_with.applications = LoadState::Loading;
                state.open_with.icons.clear();
            }
            NavigationOutcome::Applied
        }
        AppCommand::LoadExternalApplications => {
            if current_local_workspace(state).is_none() {
                return Some(NavigationOutcome::Ignored);
            }
            state.open_with.applications = LoadState::Loading;
            state.open_with.icons.clear();
            NavigationOutcome::NeedsEffect
        }
        AppCommand::ExternalApplicationsLoaded(catalog) => {
            if !matches!(state.open_with.applications, LoadState::Loading) {
                return Some(NavigationOutcome::Ignored);
            }
            state.open_with.applications = LoadState::Ready(catalog.applications.clone());
            state.open_with.icons = catalog
                .icons
                .iter()
                .filter(|icon| {
                    !icon.encoded_png().is_empty()
                        && catalog.applications.contains(&icon.application)
                })
                .cloned()
                .collect();
            NavigationOutcome::Applied
        }
        AppCommand::ExternalApplicationsFailed(message) => {
            if !matches!(state.open_with.applications, LoadState::Loading) {
                return Some(NavigationOutcome::Ignored);
            }
            state.open_with.applications = LoadState::Failed(message.clone());
            state.open_with.icons.clear();
            NavigationOutcome::Applied
        }
        AppCommand::SelectExternalApplication { application } => {
            if current_local_workspace(state).is_none() || !state.open_with.menu_open {
                return Some(NavigationOutcome::Ignored);
            }
            let application_is_available = state
                .open_with
                .applications
                .ready()
                .is_some_and(|applications| applications.contains(application));
            if !application_is_available {
                return Some(NavigationOutcome::Ignored);
            }
            state.open_with.preferred = Some(*application);
            state.close_session_action_surfaces();
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
    fn selecting_an_application_updates_the_primary_action_without_an_effect() {
        let (mut state, _) = state_with_workspace();
        state.open_with.menu_open = true;
        state.open_with.applications =
            LoadState::Ready(vec![ExternalApplication::Finder, ExternalApplication::Zed]);

        assert_eq!(
            reduce_open_with_navigation(
                &mut state,
                &AppCommand::SelectExternalApplication {
                    application: ExternalApplication::Zed,
                },
            ),
            Some(NavigationOutcome::Applied)
        );
        assert_eq!(state.open_with.preferred, Some(ExternalApplication::Zed));
        assert!(!state.open_with.menu_open);
    }

    #[test]
    fn selection_is_rejected_when_the_scanned_application_is_absent() {
        let (mut state, _) = state_with_workspace();
        state.open_with.menu_open = true;
        state.open_with.applications = LoadState::Ready(vec![ExternalApplication::Finder]);

        assert_eq!(
            reduce_open_with_navigation(
                &mut state,
                &AppCommand::SelectExternalApplication {
                    application: ExternalApplication::Zed,
                },
            ),
            Some(NavigationOutcome::Ignored)
        );
        assert_eq!(state.open_with.preferred, None);
        assert!(state.open_with.menu_open);
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
