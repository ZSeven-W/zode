use zode_app_model::{environment_actions, AppCommand, EnvironmentActionKind, ZodeAppState};

use super::{presentation::PresentationRefresh, DesktopApp};
use crate::services::RepositoryService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EnvironmentActionOutcome {
    Ignored,
    Consumed,
    Refresh,
    OpenReview,
}

pub(super) fn consume_environment_action_command(
    state: &ZodeAppState,
    repository: &dyn RepositoryService,
    command: &AppCommand,
) -> EnvironmentActionOutcome {
    let AppCommand::RunEnvironmentAction { session, action } = command else {
        return EnvironmentActionOutcome::Ignored;
    };
    if state.current_session.as_ref() != Some(session) {
        return EnvironmentActionOutcome::Consumed;
    }
    let enabled = environment_actions(state)
        .into_iter()
        .find(|candidate| candidate.kind == *action)
        .is_some_and(|candidate| candidate.enabled());
    if !enabled {
        return EnvironmentActionOutcome::Consumed;
    }
    match action {
        EnvironmentActionKind::RefreshStatus => EnvironmentActionOutcome::Refresh,
        EnvironmentActionKind::CompareWorkspaceToHead => EnvironmentActionOutcome::OpenReview,
        EnvironmentActionKind::OpenWorkspace => {
            let Some(workspace) = state.available_workspace_for_session(session) else {
                return EnvironmentActionOutcome::Consumed;
            };
            if let Err(error) = repository.open_workspace(workspace) {
                eprintln!("zode-app: opening the workspace directory failed: {error}");
            }
            EnvironmentActionOutcome::Consumed
        }
        EnvironmentActionKind::CommitOrPush => EnvironmentActionOutcome::Consumed,
    }
}

impl DesktopApp {
    pub(super) fn apply_environment_action_command(&mut self, command: &AppCommand) -> bool {
        match consume_environment_action_command(
            &self.app_state,
            self.repository_service.as_ref(),
            command,
        ) {
            EnvironmentActionOutcome::Ignored => false,
            EnvironmentActionOutcome::Consumed => {
                self.rebuild_frame_snapshot();
                self.request_redraw();
                true
            }
            EnvironmentActionOutcome::Refresh => {
                self.request_presentation_refresh(PresentationRefresh::CommandCompleted);
                self.rebuild_frame_snapshot();
                self.request_redraw();
                true
            }
            EnvironmentActionOutcome::OpenReview => {
                self.apply_presentation_command(AppCommand::OpenReview);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use zode_app_model::{
        AppCommand, EnvironmentActionKind, EnvironmentSnapshot, LoadState, ProjectState,
        SessionDiffState, SessionPresentationState, TranscriptState,
    };
    use zode_node_protocol::{
        DiffSnapshot, SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri,
    };

    use super::{consume_environment_action_command, EnvironmentActionOutcome};
    use crate::services::{RepositoryService, ServiceError};

    #[derive(Default)]
    struct RecordingRepository(Mutex<Vec<WorkspaceUri>>);

    impl RepositoryService for RecordingRepository {
        fn open_workspace(&self, workspace: &WorkspaceUri) -> Result<(), ServiceError> {
            self.0.lock().unwrap().push(workspace.clone());
            Ok(())
        }
    }

    fn ready_state() -> (zode_app_model::ZodeAppState, SessionLocator, WorkspaceUri) {
        let mut state = zode_app_model::demo_state();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        let session = SessionLocator::new(state.host.node_id, "environment-actions");
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 1,
        });
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: workspace.clone(),
            title: "environment actions".into(),
            updated_at_ms: 1,
            status: ThreadStatus::Idle,
        });
        state
            .transcripts
            .insert(session.clone(), TranscriptState::default());
        state.current_session = Some(session.clone());
        state.presentation.sessions.insert(
            session.clone(),
            SessionPresentationState {
                context: LoadState::Ready(EnvironmentSnapshot {
                    workspace_uri: workspace.clone(),
                    branch: Some("codex/environment-actions".into()),
                    subagents: Vec::new(),
                    background_processes: Vec::new(),
                    sources: Vec::new(),
                }),
                diff: SessionDiffState {
                    dirty: false,
                    load: LoadState::Ready(DiffSnapshot {
                        session: session.clone(),
                        files: Vec::new(),
                        unified: String::new(),
                    }),
                },
                ..SessionPresentationState::default()
            },
        );
        (state, session, workspace)
    }

    #[test]
    fn safe_actions_produce_local_effects_and_open_the_exact_workspace() {
        let (state, session, workspace) = ready_state();
        let repository = RecordingRepository::default();

        assert_eq!(
            consume_environment_action_command(
                &state,
                &repository,
                &AppCommand::RunEnvironmentAction {
                    session: session.clone(),
                    action: EnvironmentActionKind::RefreshStatus,
                },
            ),
            EnvironmentActionOutcome::Refresh
        );
        assert_eq!(
            consume_environment_action_command(
                &state,
                &repository,
                &AppCommand::RunEnvironmentAction {
                    session: session.clone(),
                    action: EnvironmentActionKind::CompareWorkspaceToHead,
                },
            ),
            EnvironmentActionOutcome::OpenReview
        );
        assert_eq!(
            consume_environment_action_command(
                &state,
                &repository,
                &AppCommand::RunEnvironmentAction {
                    session,
                    action: EnvironmentActionKind::OpenWorkspace,
                },
            ),
            EnvironmentActionOutcome::Consumed
        );
        assert_eq!(&*repository.0.lock().unwrap(), &[workspace]);
    }

    #[test]
    fn disabled_or_stale_mutation_intents_are_consumed_without_a_service_call() {
        let (state, session, _) = ready_state();
        let repository = RecordingRepository::default();

        for command in [
            AppCommand::RunEnvironmentAction {
                session: session.clone(),
                action: EnvironmentActionKind::CommitOrPush,
            },
            AppCommand::RunEnvironmentAction {
                session: SessionLocator::new(state.host.node_id, "stale"),
                action: EnvironmentActionKind::OpenWorkspace,
            },
        ] {
            assert_eq!(
                consume_environment_action_command(&state, &repository, &command),
                EnvironmentActionOutcome::Consumed
            );
        }
        assert!(repository.0.lock().unwrap().is_empty());
    }
}
