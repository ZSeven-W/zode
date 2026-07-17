use super::*;

#[test]
fn unknown_event_records_diagnostics_without_finishing_the_turn() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(&session, turn_id, 1, AgentEventKind::Unknown),
    );

    assert_eq!(
        state.transcripts[&session].items,
        vec![TranscriptItem::Status {
            code: "agent.event.unknown".into(),
            message: "Ignored an unknown agent event".into(),
        }],
    );
    assert!(state.transcripts[&session].busy);
    assert_eq!(state.active_turns.get(&session), Some(&turn_id));
}

#[test]
fn event_for_unknown_session_is_ignored_without_mutation() {
    let (mut state, _, turn_id) = active_state();
    let missing = SessionLocator::new(state.host.node_id, "missing");
    state.transcripts.insert(
        missing.clone(),
        TranscriptState {
            busy: true,
            ..TranscriptState::default()
        },
    );
    state.active_turns.insert(missing.clone(), turn_id);
    let before = state.clone();

    let outcome = reduce_agent_event(
        &mut state,
        event(
            &missing,
            turn_id,
            1,
            AgentEventKind::TextDelta {
                delta: "late".into(),
            },
        ),
    );

    assert_eq!(outcome, ReduceOutcome::IgnoredStale);
    assert_eq!(state, before);
}

#[test]
fn event_for_wrong_turn_is_ignored_without_mutation() {
    let (mut state, session, _) = active_state();
    let wrong_turn = TurnId::parse("00000000-0000-0000-0000-000000000003").unwrap();
    let before = state.clone();

    let outcome = reduce_agent_event(
        &mut state,
        event(
            &session,
            wrong_turn,
            1,
            AgentEventKind::TextDelta {
                delta: "late".into(),
            },
        ),
    );

    assert_eq!(outcome, ReduceOutcome::IgnoredStale);
    assert_eq!(state, before);
}

#[test]
fn duplicate_or_out_of_order_sequence_is_ignored_without_mutation() {
    let (mut state, session, turn_id) = active_state();
    state.transcripts.get_mut(&session).unwrap().last_sequence = 2;
    let before = state.clone();

    for sequence in [2, 1] {
        let outcome = reduce_agent_event(
            &mut state,
            event(
                &session,
                turn_id,
                sequence,
                AgentEventKind::TextDelta {
                    delta: "late".into(),
                },
            ),
        );
        assert_eq!(outcome, ReduceOutcome::IgnoredStale);
        assert_eq!(state, before);
    }
}
