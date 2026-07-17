use zode_app_model::{
    reduce_presentation_command, AppCommand, LoadState, PresentationCommandOutcome, PreviewState,
    SecondaryPane, ZodeAppState,
};
use zode_node_protocol::SessionLocator;

use super::DesktopApp;
use crate::presentation_bridge::PresentationQuery;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PresentationRefresh {
    PaneOpened,
    SessionChanged,
    CommandCompleted,
    DiffInvalidated(SessionLocator),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalPresentationOutcome {
    pub refresh: Option<PresentationRefresh>,
}

pub(super) fn reduce_local_presentation_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> Option<LocalPresentationOutcome> {
    let refresh = matches!(
        command,
        AppCommand::OpenSecondary(_)
            | AppCommand::OpenReview
            | AppCommand::PreviewWorkspaceFile { .. }
    )
    .then_some(PresentationRefresh::PaneOpened);
    if reduce_presentation_command(state, command) == PresentationCommandOutcome::Applied {
        Some(LocalPresentationOutcome { refresh })
    } else {
        None
    }
}

pub(super) fn presentation_queries_for_refresh(
    state: &ZodeAppState,
    refresh: PresentationRefresh,
) -> Vec<PresentationQuery> {
    let Some(session) = state.current_session.as_ref().cloned() else {
        return Vec::new();
    };
    if session.session_id.starts_with("local-error-") || !state.transcripts.contains_key(&session) {
        return Vec::new();
    }
    let Some(workspace_uri) = state.available_workspace_for_session(&session).cloned() else {
        return Vec::new();
    };
    let Some(pane) = state.presentation.secondary_pane else {
        return Vec::new();
    };
    match (pane, refresh) {
        (
            SecondaryPane::Environment,
            PresentationRefresh::PaneOpened
            | PresentationRefresh::SessionChanged
            | PresentationRefresh::CommandCompleted,
        ) => vec![
            PresentationQuery::Environment {
                session: session.clone(),
                workspace_uri,
            },
            PresentationQuery::Diff { session },
        ],
        (SecondaryPane::Environment, PresentationRefresh::DiffInvalidated(invalidated))
            if invalidated == session =>
        {
            vec![
                PresentationQuery::Environment {
                    session: session.clone(),
                    workspace_uri,
                },
                PresentationQuery::Diff { session },
            ]
        }
        (
            SecondaryPane::Review,
            PresentationRefresh::PaneOpened
            | PresentationRefresh::SessionChanged
            | PresentationRefresh::CommandCompleted,
        ) => vec![PresentationQuery::Diff { session }],
        (SecondaryPane::Review, PresentationRefresh::DiffInvalidated(invalidated))
            if invalidated == session =>
        {
            vec![PresentationQuery::Diff { session }]
        }
        (
            SecondaryPane::DocumentPreview,
            PresentationRefresh::PaneOpened | PresentationRefresh::SessionChanged,
        ) => state
            .presentation
            .sessions
            .get(&session)
            .and_then(|presentation| presentation.preview.target())
            .filter(|target| target.workspace_uri == workspace_uri)
            .cloned()
            .map(|target| PresentationQuery::DocumentPreview { session, target })
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) fn mark_presentation_query_failed(
    state: &mut ZodeAppState,
    query: PresentationQuery,
    message: String,
) {
    match query {
        PresentationQuery::Environment { session, .. } => {
            state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .context = LoadState::Failed(message);
        }
        PresentationQuery::Diff { session } => {
            state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .diff
                .load = LoadState::Failed(message);
        }
        PresentationQuery::DocumentPreview { session, target } => {
            let preview = &mut state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .preview;
            if preview.target() == Some(&target) {
                *preview = PreviewState::Failed { target, message };
            }
        }
    }
}

impl DesktopApp {
    pub(super) fn apply_presentation_command(&mut self, command: AppCommand) -> bool {
        let Some(outcome) = reduce_local_presentation_command(&mut self.app_state, command) else {
            return false;
        };
        if let Some(refresh) = outcome.refresh {
            self.request_presentation_refresh(refresh);
        }
        self.rebuild_frame_snapshot();
        self.request_redraw();
        true
    }

    pub(super) fn request_presentation_refresh(&mut self, refresh: PresentationRefresh) {
        let queries = presentation_queries_for_refresh(&self.app_state, refresh);
        if queries.is_empty() {
            return;
        }
        let Some(bridge) = self.presentation_queries.as_mut() else {
            for query in queries {
                mark_presentation_query_failed(
                    &mut self.app_state,
                    query,
                    "the presentation query service is unavailable".into(),
                );
            }
            return;
        };
        for query in queries {
            if let Err(query) = bridge.request(&mut self.app_state, query) {
                mark_presentation_query_failed(
                    &mut self.app_state,
                    query,
                    "the presentation query pump is unavailable".into(),
                );
            }
        }
    }

    pub(super) fn drain_presentation_queries(&mut self) -> usize {
        self.presentation_queries
            .as_mut()
            .map_or(0, |queries| queries.drain_into(&mut self.app_state))
    }

    pub(super) fn refresh_if_session_changed(&mut self, previous: Option<SessionLocator>) {
        if previous != self.app_state.current_session {
            self.request_presentation_refresh(PresentationRefresh::SessionChanged);
        }
    }
}

#[cfg(test)]
mod tests {
    use zode_app_model::{
        demo_state, AppCommand, IntegrationsTab, LoadState, PreviewState, PreviewTarget,
        ProjectState, SecondaryPane, SettingsCategory, ShellRoute, TranscriptState,
    };
    use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

    use super::{
        mark_presentation_query_failed, presentation_queries_for_refresh,
        reduce_local_presentation_command, PresentationRefresh,
    };
    use crate::presentation_bridge::PresentationQuery;

    fn state_with_session(available: bool) -> (zode_app_model::ZodeAppState, SessionLocator) {
        let mut state = demo_state();
        let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
        let session = SessionLocator::new(state.host.node_id, "session-1");
        state.projects.push(ProjectState {
            workspace_uri: workspace_uri.clone(),
            expanded: true,
            available,
            last_opened_ms: 0,
        });
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: workspace_uri.clone(),
            title: "session".into(),
            updated_at_ms: 0,
            status: ThreadStatus::Idle,
        });
        state
            .transcripts
            .insert(session.clone(), TranscriptState::default());
        state.current_session = Some(session.clone());
        state.active_workspace = Some(workspace_uri);
        (state, session)
    }

    #[test]
    fn presentation_commands_are_reduced_locally() {
        let commands = [
            AppCommand::Navigate(ShellRoute::Conversation),
            AppCommand::OpenSecondary(SecondaryPane::Environment),
            AppCommand::CloseSecondary,
            AppCommand::SelectSettingsCategory(SettingsCategory::Appearance),
            AppCommand::SelectIntegrationsTab(IntegrationsTab::Skills),
            AppCommand::OpenReview,
        ];

        for command in commands {
            let mut state = demo_state();
            assert!(reduce_local_presentation_command(&mut state, command).is_some());
        }
        assert!(reduce_local_presentation_command(
            &mut demo_state(),
            AppCommand::SetModel("model".into()),
        )
        .is_none());
    }

    #[test]
    fn opening_environment_queries_environment_and_canonical_diff() {
        let (mut state, session) = state_with_session(true);
        reduce_local_presentation_command(
            &mut state,
            AppCommand::OpenSecondary(SecondaryPane::Environment),
        );

        assert_eq!(
            presentation_queries_for_refresh(&state, PresentationRefresh::PaneOpened),
            vec![
                PresentationQuery::Environment {
                    session: session.clone(),
                    workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                },
                PresentationQuery::Diff { session },
            ]
        );
    }

    #[test]
    fn opening_review_queries_the_current_real_session() {
        let (mut state, session) = state_with_session(true);
        reduce_local_presentation_command(&mut state, AppCommand::OpenReview);

        assert_eq!(
            presentation_queries_for_refresh(&state, PresentationRefresh::PaneOpened),
            vec![PresentationQuery::Diff { session }]
        );
    }

    #[test]
    fn unavailable_or_synthetic_sessions_never_produce_queries() {
        let (mut unavailable, _) = state_with_session(false);
        reduce_local_presentation_command(&mut unavailable, AppCommand::OpenReview);
        assert_eq!(
            presentation_queries_for_refresh(&unavailable, PresentationRefresh::PaneOpened),
            Vec::<PresentationQuery>::new()
        );

        let (mut synthetic, _) = state_with_session(true);
        synthetic.current_session = Some(SessionLocator::new(
            synthetic.host.node_id,
            "local-error-example",
        ));
        assert_eq!(
            presentation_queries_for_refresh(&synthetic, PresentationRefresh::PaneOpened),
            Vec::<PresentationQuery>::new()
        );

        synthetic.current_session = None;
        assert_eq!(
            presentation_queries_for_refresh(&synthetic, PresentationRefresh::PaneOpened),
            Vec::<PresentationQuery>::new()
        );

        let (mut incomplete, session) = state_with_session(true);
        incomplete.transcripts.remove(&session);
        reduce_local_presentation_command(&mut incomplete, AppCommand::OpenReview);
        assert_eq!(
            presentation_queries_for_refresh(&incomplete, PresentationRefresh::PaneOpened),
            Vec::<PresentationQuery>::new()
        );
    }

    #[test]
    fn diff_invalidation_refreshes_only_the_matching_open_review() {
        let (mut state, session) = state_with_session(true);
        reduce_local_presentation_command(&mut state, AppCommand::OpenReview);
        let other = SessionLocator::new(state.host.node_id, "other");

        assert_eq!(
            presentation_queries_for_refresh(&state, PresentationRefresh::DiffInvalidated(other),),
            Vec::<PresentationQuery>::new()
        );
        assert_eq!(
            presentation_queries_for_refresh(
                &state,
                PresentationRefresh::DiffInvalidated(session.clone()),
            ),
            vec![PresentationQuery::Diff { session }]
        );
    }

    #[test]
    fn matching_diff_invalidation_refreshes_open_environment_and_diff() {
        let (mut state, session) = state_with_session(true);
        reduce_local_presentation_command(
            &mut state,
            AppCommand::OpenSecondary(SecondaryPane::Environment),
        );
        let other = SessionLocator::new(state.host.node_id, "other");

        assert!(presentation_queries_for_refresh(
            &state,
            PresentationRefresh::DiffInvalidated(other),
        )
        .is_empty());
        assert_eq!(
            presentation_queries_for_refresh(
                &state,
                PresentationRefresh::DiffInvalidated(session.clone()),
            ),
            vec![
                PresentationQuery::Environment {
                    session: session.clone(),
                    workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                },
                PresentationQuery::Diff { session },
            ]
        );
    }

    #[test]
    fn query_failure_is_projected_and_not_replanned_without_a_trigger() {
        let (mut state, session) = state_with_session(true);
        reduce_local_presentation_command(&mut state, AppCommand::OpenReview);
        let query = PresentationQuery::Diff {
            session: session.clone(),
        };

        mark_presentation_query_failed(&mut state, query, "query pump stopped".into());

        assert_eq!(
            state.presentation.sessions[&session].diff.load,
            LoadState::Failed("query pump stopped".into())
        );
    }

    #[test]
    fn retargeted_workspace_never_reloads_the_previous_document() {
        let (mut state, session) = state_with_session(true);
        state.presentation.secondary_pane = Some(SecondaryPane::DocumentPreview);
        state
            .presentation
            .sessions
            .entry(session.clone())
            .or_default()
            .preview = PreviewState::Ready {
            target: PreviewTarget {
                workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                relative_path: "docs/old.md".into(),
            },
            title: "old.md".into(),
            content: "old workspace content".into(),
            kind: zode_app_model::PreviewKind::Markdown,
        };
        let new_workspace = WorkspaceUri::new("file:///repo/new").unwrap();
        state.projects.push(ProjectState {
            workspace_uri: new_workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        state
            .threads
            .iter_mut()
            .find(|thread| thread.session == session)
            .unwrap()
            .workspace_uri = new_workspace.clone();
        state.active_workspace = Some(new_workspace);

        assert!(
            presentation_queries_for_refresh(&state, PresentationRefresh::SessionChanged,)
                .is_empty()
        );
    }
}
