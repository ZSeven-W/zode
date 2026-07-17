use zode_app_model::{
    reduce_queue_command, AppCommand, QueueCommandOutcome, TranscriptItem, TranscriptState,
    TranscriptTurnStatus,
};
use zode_node_protocol::{
    AgentCommandKind, SessionLocator, ThreadStatus, ThreadSummary, UserContent,
};

use super::fixture;
use crate::command_bridge::{prepare_dispatch, prepare_queued_start, prepare_queued_steer};

#[test]
fn queued_start_targets_its_owner_after_the_current_session_changes() {
    let mut state = fixture();
    let session_a = state.current_session.clone().unwrap();
    let workspace = state.threads[0].workspace_uri.clone();
    let session_b = SessionLocator::new(state.host.node_id, "session-b");
    state.threads.push(ThreadSummary {
        session: session_b.clone(),
        workspace_uri: workspace,
        title: "B".into(),
        updated_at_ms: 2,
        status: ThreadStatus::Idle,
    });
    state
        .transcripts
        .insert(session_b.clone(), TranscriptState::default());
    state.current_session = Some(session_b.clone());

    let dispatch = prepare_queued_start(
        &mut state,
        session_a.clone(),
        vec![UserContent::Text {
            text: "queued for A".into(),
        }],
    )
    .unwrap();

    assert_eq!(dispatch.commands[0].session, session_a);
    assert!(state.active_turns.contains_key(&session_a));
    assert!(!state.active_turns.contains_key(&session_b));
    assert!(matches!(
        state.transcripts[&session_a].items.last(),
        Some(TranscriptItem::UserText(text)) if text == "queued for A"
    ));
    let turn = &state.transcripts[&session_a].turns[0];
    assert_eq!(turn.turn_id, dispatch.commands[0].turn_id);
    assert_eq!(turn.start_item_index, 0);
    assert_eq!(turn.response_item_index, 1);
    assert_eq!(turn.status, TranscriptTurnStatus::Running);
    assert!(state.transcripts[&session_b].items.is_empty());
}

#[test]
fn active_turn_rejects_duplicate_queued_and_regular_submits() {
    let mut state = fixture();
    let session = state.current_session.clone().unwrap();
    let first = prepare_queued_start(
        &mut state,
        session.clone(),
        vec![UserContent::Text {
            text: "first".into(),
        }],
    )
    .unwrap();
    let first_turn = first.commands[0].turn_id;

    assert!(prepare_queued_start(
        &mut state,
        session.clone(),
        vec![UserContent::Text {
            text: "duplicate".into(),
        }],
    )
    .is_err());
    assert!(prepare_dispatch(
        &mut state,
        AppCommand::Submit(vec![UserContent::Text {
            text: "regular duplicate".into(),
        }]),
    )
    .is_err());
    assert_eq!(state.active_turns.get(&session).copied(), first_turn);
    assert_eq!(
        state.transcripts[&session]
            .items
            .iter()
            .filter(|item| matches!(item, TranscriptItem::UserText(_)))
            .count(),
        1,
    );
}

#[test]
fn accepted_queued_start_claims_the_session_before_a_repeated_dispatch_attempt() {
    let mut state = fixture();
    let session = state.current_session.clone().unwrap();
    let first = AppCommand::EnqueueMessage {
        session: session.clone(),
        content: vec![UserContent::Text {
            text: "first queued message".into(),
        }],
        attachments: Vec::new(),
    };
    let second = AppCommand::EnqueueMessage {
        session: session.clone(),
        content: vec![UserContent::Text {
            text: "second queued message".into(),
        }],
        attachments: Vec::new(),
    };
    let QueueCommandOutcome::Enqueued(first_id) = reduce_queue_command(&mut state, &first) else {
        panic!("first message should enter the queue");
    };
    let QueueCommandOutcome::Enqueued(second_id) = reduce_queue_command(&mut state, &second) else {
        panic!("second message should enter the queue");
    };

    let first_dispatch = prepare_queued_start(
        &mut state,
        session.clone(),
        vec![UserContent::Text {
            text: "first queued message".into(),
        }],
    )
    .expect("the first completion edge may start exactly one queued turn");
    assert!(matches!(
        first_dispatch.commands[0].kind,
        AgentCommandKind::StartTurn { .. }
    ));
    state
        .message_queues
        .get_mut(&session)
        .unwrap()
        .remove_exact(first_id)
        .expect("accepted dispatch removes only its exact queue item");

    let repeated = prepare_queued_start(
        &mut state,
        session.clone(),
        vec![UserContent::Text {
            text: "second queued message".into(),
        }],
    );
    assert!(
        repeated.is_err(),
        "a repeated redraw cannot dispatch another message while the claimed turn is active"
    );
    let queue = &state.message_queues[&session];
    assert_eq!(queue.items.len(), 1);
    assert_eq!(queue.items[0].id, second_id);
    assert_eq!(queue.items[0].text, "second queued message");
    assert_eq!(
        state.transcripts[&session]
            .items
            .iter()
            .filter(|item| matches!(item, TranscriptItem::UserText(_)))
            .count(),
        1,
    );
}

#[test]
fn queued_steer_uses_the_owner_active_turn_after_session_switch() {
    let mut state = fixture();
    let session_a = state.current_session.clone().unwrap();
    let workspace = state.threads[0].workspace_uri.clone();
    let started = prepare_queued_start(
        &mut state,
        session_a.clone(),
        vec![UserContent::Text {
            text: "running A".into(),
        }],
    )
    .unwrap();
    let turn_a = started.commands[0].turn_id;
    let session_b = SessionLocator::new(state.host.node_id, "session-b");
    state.threads.push(ThreadSummary {
        session: session_b.clone(),
        workspace_uri: workspace,
        title: "B".into(),
        updated_at_ms: 2,
        status: ThreadStatus::Idle,
    });
    state
        .transcripts
        .insert(session_b.clone(), TranscriptState::default());
    state.current_session = Some(session_b.clone());

    let steer = prepare_queued_steer(
        &mut state,
        session_a.clone(),
        vec![UserContent::Text {
            text: "guide A".into(),
        }],
    )
    .unwrap();

    assert_eq!(steer.commands[0].session, session_a);
    assert_eq!(steer.commands[0].turn_id, turn_a);
    assert!(matches!(
        steer.commands[0].kind,
        AgentCommandKind::SteerTurn { .. }
    ));
    assert!(matches!(
        state.transcripts[&session_a].items.last(),
        Some(TranscriptItem::UserText(text)) if text == "guide A"
    ));
    assert!(state.transcripts[&session_b].items.is_empty());
}
