use zode_app_model::{
    demo_state, reduce_agent_event, reduce_tool_command, AppCommand, ReduceOutcome,
    ToolCommandOutcome, TranscriptItem, TranscriptState, ZodeAppState,
};
use zode_node_protocol::{
    AgentEvent, AgentEventKind, SessionLocator, ThreadStatus, ThreadSummary, ToolCall, ToolStatus,
    TurnId, UsageSnapshot, WorkspaceUri, PROTOCOL_VERSION,
};

fn active_state() -> (ZodeAppState, SessionLocator, TurnId) {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "s1");
    let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000002").unwrap();
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
        title: "为 zode 写 app".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Running,
    });
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            busy: true,
            ..TranscriptState::default()
        },
    );
    state.active_turns.insert(session.clone(), turn_id);
    (state, session, turn_id)
}

fn event(
    session: &SessionLocator,
    turn_id: TurnId,
    sequence: u64,
    kind: AgentEventKind,
) -> AgentEvent {
    AgentEvent {
        version: PROTOCOL_VERSION,
        session: session.clone(),
        turn_id,
        sequence,
        kind,
    }
}

fn tool(id: &str, status: ToolStatus, summary: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "shell".into(),
        status,
        summary: summary.into(),
        detail: None,
    }
}

#[test]
fn adjacent_text_deltas_extend_one_assistant_item() {
    let (mut state, session, turn_id) = active_state();

    assert_eq!(
        reduce_agent_event(
            &mut state,
            event(
                &session,
                turn_id,
                1,
                AgentEventKind::TextDelta {
                    delta: "你".into()
                },
            ),
        ),
        ReduceOutcome::Applied,
    );
    assert_eq!(
        reduce_agent_event(
            &mut state,
            event(
                &session,
                turn_id,
                2,
                AgentEventKind::TextDelta {
                    delta: "好".into()
                },
            ),
        ),
        ReduceOutcome::Applied,
    );

    assert_eq!(
        state.transcripts[&session].items,
        vec![TranscriptItem::AssistantText("你好".into())],
    );
}

#[test]
fn adjacent_thinking_deltas_extend_one_thinking_item() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::ThinkingDelta {
                delta: "先".into()
            },
        ),
    );
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::ThinkingDelta {
                delta: "想".into()
            },
        ),
    );

    assert_eq!(
        state.transcripts[&session].items,
        vec![TranscriptItem::Thinking("先想".into())],
    );
}

#[test]
fn streaming_update_invalidates_cached_item_height() {
    let (mut state, session, turn_id) = active_state();
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::TextDelta { delta: "a".into() },
        ),
    );
    state.transcripts.get_mut(&session).unwrap().item_heights = vec![88.0];
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::TextDelta {
                delta: " much longer response".into(),
            },
        ),
    );
    assert_eq!(state.transcripts[&session].item_heights, [0.0]);

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            3,
            AgentEventKind::ToolStarted {
                tool: tool("tool-1", ToolStatus::Running, "running"),
            },
        ),
    );
    state.transcripts.get_mut(&session).unwrap().item_heights = vec![0.0, 64.0];
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            4,
            AgentEventKind::ToolCompleted {
                tool: tool("tool-1", ToolStatus::Completed, "completed with detail"),
            },
        ),
    );
    assert_eq!(state.transcripts[&session].item_heights, [0.0, 0.0]);
}

#[test]
fn separated_text_deltas_remain_separate_items() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::TextDelta { delta: "a".into() },
        ),
    );
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::StatusNotice {
                code: "separator".into(),
                message: "separator".into(),
            },
        ),
    );
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            3,
            AgentEventKind::TextDelta { delta: "b".into() },
        ),
    );

    assert_eq!(
        state.transcripts[&session].items,
        vec![
            TranscriptItem::AssistantText("a".into()),
            TranscriptItem::Status {
                code: "separator".into(),
                message: "separator".into(),
            },
            TranscriptItem::AssistantText("b".into()),
        ],
    );
}

#[test]
fn separated_thinking_deltas_remain_separate_items() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::ThinkingDelta { delta: "a".into() },
        ),
    );
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::StatusNotice {
                code: "separator".into(),
                message: "separator".into(),
            },
        ),
    );
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            3,
            AgentEventKind::ThinkingDelta { delta: "b".into() },
        ),
    );

    assert_eq!(
        state.transcripts[&session].items,
        vec![
            TranscriptItem::Thinking("a".into()),
            TranscriptItem::Status {
                code: "separator".into(),
                message: "separator".into(),
            },
            TranscriptItem::Thinking("b".into()),
        ],
    );
}

#[test]
fn tool_started_inserts_then_replaces_the_same_tool() {
    let (mut state, session, turn_id) = active_state();
    let started = tool("tool-1", ToolStatus::Running, "started");
    let updated = tool("tool-1", ToolStatus::Running, "updated");

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::ToolStarted {
                tool: started.clone(),
            },
        ),
    );
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::ToolStarted {
                tool: updated.clone(),
            },
        ),
    );

    assert_eq!(
        state.transcripts[&session].items,
        vec![TranscriptItem::Tool(updated)],
    );
}

#[test]
fn tool_completed_replaces_an_existing_tool() {
    let (mut state, session, turn_id) = active_state();
    let completed = tool("tool-1", ToolStatus::Completed, "done");

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::ToolStarted {
                tool: tool("tool-1", ToolStatus::Running, "running"),
            },
        ),
    );
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::ToolCompleted {
                tool: completed.clone(),
            },
        ),
    );

    assert_eq!(
        state.transcripts[&session].items,
        vec![TranscriptItem::Tool(completed)],
    );
}

#[test]
fn tool_completed_does_not_insert_a_missing_tool() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::ToolCompleted {
                tool: tool("missing", ToolStatus::Completed, "done"),
            },
        ),
    );

    assert!(state.transcripts[&session].items.is_empty());
    assert_eq!(state.transcripts[&session].last_sequence, 1);
}

#[test]
fn tool_expansion_invalidates_cached_item_height() {
    let (mut state, session, _) = active_state();
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            items: vec![
                TranscriptItem::Tool(tool("target", ToolStatus::Completed, "target")),
                TranscriptItem::Tool(tool("other", ToolStatus::Completed, "other")),
            ],
            item_heights: vec![42.0, 64.0],
            ..TranscriptState::default()
        },
    );
    let second = SessionLocator::new(state.host.node_id, "s2");
    state.transcripts.insert(
        second.clone(),
        TranscriptState {
            items: vec![
                TranscriptItem::Tool(tool("other", ToolStatus::Completed, "other")),
                TranscriptItem::Tool(tool("target", ToolStatus::Completed, "target")),
            ],
            item_heights: vec![70.0, 80.0],
            ..TranscriptState::default()
        },
    );

    assert_eq!(
        reduce_tool_command(
            &mut state,
            AppCommand::SetToolExpanded {
                session: session.clone(),
                tool_id: "target".into(),
                expanded: true,
            },
        ),
        ToolCommandOutcome::Applied,
    );

    assert_eq!(state.transcripts[&session].item_heights, [0.0, 64.0]);
    assert_eq!(state.transcripts[&second].item_heights, [70.0, 80.0]);
    assert_eq!(state.tool_expanded[&session].get("target"), Some(&true));
    assert!(!state.tool_expanded.contains_key(&second));
}

#[test]
fn approval_requested_updates_transcript_and_lookup() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::ApprovalRequested {
                approval_id: "approval-1".into(),
                tool: "shell".into(),
                summary: "Run tests".into(),
            },
        ),
    );

    assert_eq!(
        state.transcripts[&session].items,
        vec![TranscriptItem::Approval {
            id: "approval-1".into(),
            tool: "shell".into(),
        }],
    );
    assert_eq!(state.approvals.get("approval-1"), Some(&session));
}

#[test]
fn usage_replaces_the_server_snapshot() {
    let (mut state, session, turn_id) = active_state();
    let first = UsageSnapshot {
        input_tokens: 10,
        output_tokens: 5,
        context_used: Some(0.1),
        cost_usd: Some(0.01),
    };
    let latest = UsageSnapshot {
        input_tokens: 20,
        output_tokens: 12,
        context_used: Some(0.2),
        cost_usd: Some(0.03),
    };

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::Usage {
                usage: first.clone(),
            },
        ),
    );
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::Usage {
                usage: latest.clone(),
            },
        ),
    );

    assert_eq!(state.usage.get(&session), Some(&latest));
}

#[test]
fn status_notice_appends_a_status_item() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::StatusNotice {
                code: "indexing".into(),
                message: "Indexing workspace".into(),
            },
        ),
    );

    assert_eq!(
        state.transcripts[&session].items,
        vec![TranscriptItem::Status {
            code: "indexing".into(),
            message: "Indexing workspace".into(),
        }],
    );
}

#[test]
fn diff_invalidated_only_marks_review_dirty() {
    let (mut state, session, turn_id) = active_state();
    state.current_session = Some(session.clone());

    reduce_agent_event(
        &mut state,
        event(&session, turn_id, 1, AgentEventKind::DiffInvalidated),
    );

    assert!(state.review.dirty);
    assert!(state.transcripts[&session].items.is_empty());
    assert!(state.transcripts[&session].busy);
    assert_eq!(state.active_turns.get(&session), Some(&turn_id));
}

#[test]
fn turn_finished_clears_busy_and_active_turn_but_keeps_items() {
    let (mut state, session, turn_id) = active_state();
    state.transcripts.get_mut(&session).unwrap().items =
        vec![TranscriptItem::AssistantText("partial".into())];

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::TurnFinished { interrupted: true },
        ),
    );

    assert!(!state.transcripts[&session].busy);
    assert!(!state.active_turns.contains_key(&session));
    assert_eq!(
        state.transcripts[&session].items,
        vec![TranscriptItem::AssistantText("partial".into())],
    );
}

#[test]
fn retryable_error_keeps_the_turn_busy() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::Error {
                message: "try again".into(),
                retryable: true,
            },
        ),
    );

    assert!(state.transcripts[&session].busy);
    assert_eq!(state.active_turns.get(&session), Some(&turn_id));
    assert_eq!(
        state.transcripts[&session].items,
        vec![TranscriptItem::Error {
            message: "try again".into(),
            retryable: true,
        }],
    );
}

#[test]
fn non_retryable_error_ends_busy_until_turn_finished_arrives() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::Error {
                message: "stopped".into(),
                retryable: false,
            },
        ),
    );

    assert!(!state.transcripts[&session].busy);
    assert_eq!(state.active_turns.get(&session), Some(&turn_id));
}

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
