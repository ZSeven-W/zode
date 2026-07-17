use zode_app_model::{AppCommand, ZodeAppState};

use super::DesktopApp;
use crate::services::RepositoryService;

impl DesktopApp {
    pub(super) fn apply_project_action_command(&self, command: &AppCommand) -> bool {
        consume_project_action_command(&self.app_state, self.repository_service.as_ref(), command)
    }
}

fn consume_project_action_command(
    state: &ZodeAppState,
    repository: &dyn RepositoryService,
    command: &AppCommand,
) -> bool {
    let AppCommand::OpenProjectInFinder { workspace_uri } = command else {
        return false;
    };
    if !state.available_workspace(workspace_uri) {
        return false;
    }
    if let Err(error) = repository.open_workspace(workspace_uri) {
        eprintln!("zode-app: opening the project directory failed: {error}");
    }
    true
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use zode_app_model::{AppCommand, ProjectState};
    use zode_node_protocol::WorkspaceUri;

    use super::consume_project_action_command;
    use crate::services::{RepositoryService, ServiceError};

    #[derive(Default)]
    struct RecordingRepository(Mutex<Vec<WorkspaceUri>>);

    impl RepositoryService for RecordingRepository {
        fn open_workspace(&self, workspace: &WorkspaceUri) -> Result<(), ServiceError> {
            self.0.lock().unwrap().push(workspace.clone());
            Ok(())
        }
    }

    #[test]
    fn finder_action_opens_only_an_available_project() {
        let mut state = zode_app_model::demo_state();
        let available = WorkspaceUri::new("file:///repo/available").unwrap();
        let unavailable = WorkspaceUri::new("file:///repo/unavailable").unwrap();
        for (workspace_uri, is_available) in
            [(available.clone(), true), (unavailable.clone(), false)]
        {
            state.projects.push(ProjectState {
                workspace_uri,
                expanded: true,
                available: is_available,
                last_opened_ms: 0,
            });
        }
        let repository = RecordingRepository::default();

        assert!(consume_project_action_command(
            &state,
            &repository,
            &AppCommand::OpenProjectInFinder {
                workspace_uri: available.clone(),
            },
        ));
        assert!(!consume_project_action_command(
            &state,
            &repository,
            &AppCommand::OpenProjectInFinder {
                workspace_uri: unavailable,
            },
        ));
        assert_eq!(*repository.0.lock().unwrap(), vec![available]);
    }
}
