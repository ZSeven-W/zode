use zode_app_model::{
    demo_state, reduce_navigation_command, AppCommand, NavigationOutcome, TranscriptState,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[test]
fn project_expansion_does_not_change_active_workspace_and_session_selection_does() {
    let mut state = demo_state();
    let first = WorkspaceUri::new("file:///repo/first").unwrap();
    let second = WorkspaceUri::new("file:///repo/second").unwrap();
    let session = SessionLocator::new(state.host.node_id, "second-session");
    for workspace_uri in [first.clone(), second.clone()] {
        state.projects.push(zode_app_model::ProjectState {
            workspace_uri,
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
    }
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: second.clone(),
        title: "second".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::ToggleProject(first.clone())),
        NavigationOutcome::NeedsEffect
    );
    assert_eq!(state.active_workspace, None);
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SelectSession(session)),
        NavigationOutcome::Applied
    );
    assert_eq!(state.active_workspace, Some(second));
}

#[test]
fn delete_requires_confirmation_before_removing_session_state() {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "delete-me");
    state.current_session = Some(session.clone());
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
        title: "delete me".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state
        .transcripts
        .insert(session.clone(), TranscriptState::default());
    state
        .tool_expanded
        .entry(session.clone())
        .or_default()
        .insert("tool-1".into(), true);

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::RequestDeleteSession(session.clone()),
        ),
        NavigationOutcome::NeedsEffect,
    );
    assert_eq!(state.pending_session_delete, Some(session.clone()));
    assert_eq!(state.threads.len(), 1);

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::DeleteSession(session.clone())),
        NavigationOutcome::NeedsEffect,
    );
    assert!(state.threads.is_empty());
    assert!(!state.transcripts.contains_key(&session));
    assert!(!state.tool_expanded.contains_key(&session));
    assert_eq!(state.current_session, None);
    assert_eq!(state.pending_session_delete, None);
}

#[test]
fn cancel_delete_preserves_session() {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "keep-me");
    state.pending_session_delete = Some(session.clone());

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::CancelDeleteSession),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.pending_session_delete, None);
}
