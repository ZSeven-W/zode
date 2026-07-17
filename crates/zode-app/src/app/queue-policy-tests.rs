use zode_app_model::{
    demo_state, reduce_agent_event, reduce_queue_command, AppCommand, ProjectState, ReduceOutcome,
    TranscriptState,
};
use zode_app_ui::{ComposerController, ComposerOutcome, ComposerSubmission, Key, Modifiers};
use zode_node_protocol::{
    AgentEvent, AgentEventKind, SessionLocator, ThreadStatus, ThreadSummary, TurnId, UserContent,
    WorkspaceUri, PROTOCOL_VERSION,
};

use super::{
    composer_submit_capability_changed, has_dispatchable_current_session, submission_queue_session,
};

fn state_with_session() -> (zode_app_model::ZodeAppState, SessionLocator) {
    let mut state = demo_state();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(state.host.node_id, "session");
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace.clone(),
        title: "session".into(),
        updated_at_ms: 0,
        status: ThreadStatus::Idle,
    });
    state
        .transcripts
        .insert(session.clone(), TranscriptState::default());
    state.current_session = Some(session.clone());
    state.active_workspace = Some(workspace);
    (state, session)
}

fn send(text: &str) -> ComposerOutcome {
    ComposerOutcome::Send(ComposerSubmission {
        content: vec![UserContent::Text { text: text.into() }],
        attachments: Vec::new(),
    })
}

#[test]
fn composer_submit_capability_edges_require_a_fresh_snapshot() {
    let mut state = demo_state();
    state.composer.draft.clear();

    assert!(!composer_submit_capability_changed(Some(false), &state));
    state.composer.draft = "first character".into();
    assert!(composer_submit_capability_changed(Some(false), &state));
    assert!(!composer_submit_capability_changed(Some(true), &state));
    state.composer.draft.clear();
    assert!(composer_submit_capability_changed(Some(true), &state));
    assert!(!composer_submit_capability_changed(None, &state));
}

#[test]
fn idle_send_joins_an_existing_session_queue_instead_of_bypassing_its_head() {
    let (mut state, session) = state_with_session();
    let head = state
        .message_queues
        .entry(session.clone())
        .or_default()
        .enqueue("existing head".into(), Vec::new())
        .unwrap();

    assert!(has_dispatchable_current_session(&state));
    assert_eq!(
        submission_queue_session(&state, &send("new tail")),
        Some(session.clone())
    );
    let _ = reduce_queue_command(
        &mut state,
        &AppCommand::EnqueueMessage {
            session,
            content: vec![UserContent::Text {
                text: "new tail".into(),
            }],
            attachments: Vec::new(),
            front: false,
        },
    );
    assert_eq!(
        state
            .message_queues
            .values()
            .next()
            .and_then(|queue| queue.items.first())
            .map(|message| message.id),
        Some(head)
    );
    assert_eq!(
        state.message_queues.values().next().map(|queue| queue
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>()),
        Some(vec!["existing head", "new tail"])
    );
}

#[test]
fn idle_send_without_pending_messages_keeps_the_direct_submit_path() {
    let (state, _) = state_with_session();

    assert_eq!(submission_queue_session(&state, &send("direct")), None);
}

#[test]
fn fatal_error_window_still_queues_composer_input_until_turn_finished() {
    let (mut state, session) = state_with_session();
    let turn_id = TurnId::new();
    state.transcripts.get_mut(&session).unwrap().busy = true;
    state.active_turns.insert(session.clone(), turn_id);

    assert_eq!(
        reduce_agent_event(
            &mut state,
            AgentEvent {
                version: PROTOCOL_VERSION,
                session: session.clone(),
                turn_id,
                sequence: 1,
                kind: AgentEventKind::Error {
                    message: "provider failed".into(),
                    retryable: false,
                },
            },
        ),
        ReduceOutcome::Applied,
    );

    let mut composer = ComposerController::new("follow up after failure");
    composer.set_busy(state.transcripts[&session].busy);
    let outcome = composer.key(Key::Enter, Modifiers::NONE);

    assert!(matches!(outcome, ComposerOutcome::Queue(_)));
    assert_eq!(submission_queue_session(&state, &outcome), Some(session));
}
