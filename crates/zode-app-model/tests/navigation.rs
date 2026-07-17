use zode_app_model::{
    demo_state, reduce_navigation_command, AppCommand, NavigationOutcome, TranscriptState,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

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
