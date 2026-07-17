use std::time::{Duration, Instant};

use zode_app_model::{
    demo_state, reduce_agent_event, reduce_agent_event_at, reduce_settings_command,
    reduce_tool_command, AppCommand, LoadState, ReduceOutcome, SettingsCommandOutcome,
    ToolCommandOutcome, TranscriptItem, TranscriptState, TranscriptTurnStatus, ZodeAppState,
};
use zode_node_protocol::{
    AgentEvent, AgentEventKind, SessionLocator, SubagentSnapshot, SubagentStatus, ThreadStatus,
    ThreadSummary, ToolCall, ToolStatus, TurnId, UsageSnapshot, WorkspaceUri, PROTOCOL_VERSION,
};

fn active_state() -> (ZodeAppState, SessionLocator, TurnId) {
    active_state_at(Instant::now())
}

fn active_state_at(started_at: Instant) -> (ZodeAppState, SessionLocator, TurnId) {
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
    let mut transcript = TranscriptState::default();
    assert!(transcript.begin_turn_at(turn_id, 0, 0, started_at));
    state.transcripts.insert(session.clone(), transcript);
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

fn subagent(id: &str, status: SubagentStatus, tokens: u64, turn_id: TurnId) -> SubagentSnapshot {
    SubagentSnapshot {
        id: id.into(),
        agent_type: "researcher".into(),
        display_name: "researcher".into(),
        depth: 0,
        status,
        tokens,
        turn_id,
        completed_at_ms: None,
        result_summary: None,
    }
}

#[test]
fn project_permission_projection_preserves_a_known_empty_result() {
    let mut state = demo_state();
    let workspace = WorkspaceUri::new("file:///repo/settings").unwrap();

    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::SetProjectPermissions {
                workspace_uri: workspace.clone(),
                tools: Vec::new(),
            },
        ),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(
        state.project_permissions.get(&workspace),
        Some(&LoadState::Ready(Vec::new()))
    );
}

#[test]
fn settings_search_is_reduced_and_resets_content_scroll() {
    let mut state = demo_state();
    state.settings_scroll_offset = 240.0;

    assert_eq!(
        reduce_settings_command(&mut state, AppCommand::SetSettingsSearch("git".into())),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(state.settings_search, "git");
    assert_eq!(state.settings_scroll_offset, 0.0);
}

#[test]
fn local_general_preferences_update_real_ui_state() {
    let mut state = demo_state();

    assert_eq!(
        reduce_settings_command(&mut state, AppCommand::SetTaskSuggestions(false)),
        SettingsCommandOutcome::Applied,
    );
    assert!(!state.ui_preferences.task_suggestions);
    assert_eq!(
        reduce_settings_command(&mut state, AppCommand::SetSidebarTasksExpanded(false)),
        SettingsCommandOutcome::Applied,
    );
    assert!(!state.ui_preferences.sidebar_tasks_expanded);
    assert!(!state.sidebar.tasks_expanded);
}

#[test]
fn archived_task_filters_accept_only_real_archived_workspaces() {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "archived");
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace.clone(),
        title: "fix settings".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state.archived_sessions.insert(session);
    state.settings_scroll_offset = 50.0;

    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::SetArchivedTaskSearch("settings".into()),
        ),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(state.archived_tasks.search, "settings");
    assert_eq!(state.settings_scroll_offset, 0.0);
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::SetArchivedTaskWorkspaceFilter(Some(workspace.clone())),
        ),
        SettingsCommandOutcome::Applied,
    );
    assert_eq!(state.archived_tasks.workspace_filter, Some(workspace));
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::SetArchivedTaskWorkspaceFilter(Some(
                WorkspaceUri::new("file:///repo/missing").unwrap(),
            )),
        ),
        SettingsCommandOutcome::Ignored,
    );
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

    // Not `assert_eq!` against `TranscriptItem::assistant_text("你好")`: a
    // freshly-created assistant item is now stamped with a real wall-clock
    // `timestamp_ms` (see `reducer::now_epoch_ms`), which the no-timestamp
    // test constructor deliberately leaves `None`.
    assert!(matches!(
        state.transcripts[&session].items.as_slice(),
        [TranscriptItem::AssistantText { text, timestamp_ms: Some(_), feedback }]
            if text == "你好" && *feedback == zode_app_model::MessageFeedback::None
    ));
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

    // See the comment on `adjacent_text_deltas_extend_one_assistant_item`:
    // each new assistant item gets a real wall-clock `timestamp_ms`, so this
    // checks text/feedback per item instead of equality against the
    // no-timestamp test constructor.
    assert!(matches!(
        state.transcripts[&session].items.as_slice(),
        [
            TranscriptItem::AssistantText { text: a, timestamp_ms: Some(_), feedback: fa },
            TranscriptItem::Status { code, message },
            TranscriptItem::AssistantText { text: b, timestamp_ms: Some(_), feedback: fb },
        ] if a == "a"
            && b == "b"
            && code == "separator"
            && message == "separator"
            && *fa == zode_app_model::MessageFeedback::None
            && *fb == zode_app_model::MessageFeedback::None
    ));
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
        vec![TranscriptItem::assistant_text("partial")];

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
        vec![TranscriptItem::assistant_text("partial")],
    );
    assert_eq!(
        state.transcripts[&session].turns[0].status,
        TranscriptTurnStatus::Interrupted
    );
    assert_eq!(state.transcripts[&session].turns[0].end_item_index, Some(1));
    assert!(state.transcripts[&session].turns[0].elapsed.is_some());
}

#[test]
fn subagent_update_inserts_new_agents_in_first_seen_order() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::SubagentUpdate {
                subagent: subagent("2", SubagentStatus::Running, 0, turn_id),
            },
        ),
    );
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::SubagentUpdate {
                subagent: subagent("1", SubagentStatus::Running, 0, turn_id),
            },
        ),
    );

    let subagents = &state.presentation.sessions[&session].subagents;
    assert_eq!(
        subagents.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["2", "1"],
        "insertion order is preserved, not sorted by id",
    );
}

#[test]
fn subagent_update_replaces_the_same_id_in_place() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::SubagentUpdate {
                subagent: subagent("1", SubagentStatus::Running, 10, turn_id),
            },
        ),
    );
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::SubagentUpdate {
                subagent: subagent("1", SubagentStatus::Completed, 42, turn_id),
            },
        ),
    );

    let subagents = &state.presentation.sessions[&session].subagents;
    assert_eq!(subagents.len(), 1);
    assert_eq!(subagents[0].status, SubagentStatus::Completed);
    assert_eq!(subagents[0].tokens, 42);
}

#[test]
fn turn_finished_completes_still_running_subagents_on_a_clean_finish() {
    let (mut state, session, turn_id) = active_state();
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::SubagentUpdate {
                subagent: subagent("1", SubagentStatus::Running, 5, turn_id),
            },
        ),
    );

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::TurnFinished { interrupted: false },
        ),
    );

    assert_eq!(
        state.presentation.sessions[&session].subagents[0].status,
        SubagentStatus::Completed
    );
    assert!(
        state.presentation.sessions[&session].subagents[0]
            .completed_at_ms
            .is_some(),
        "the safety net stamps a completion time when the runtime's own diff never arrived",
    );
}

#[test]
fn turn_finished_fails_still_running_subagents_when_interrupted() {
    let (mut state, session, turn_id) = active_state();
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::SubagentUpdate {
                subagent: subagent("1", SubagentStatus::Running, 5, turn_id),
            },
        ),
    );

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::TurnFinished { interrupted: true },
        ),
    );

    assert_eq!(
        state.presentation.sessions[&session].subagents[0].status,
        SubagentStatus::Failed
    );
}

#[test]
fn turn_finished_does_not_touch_already_terminal_subagents() {
    let (mut state, session, turn_id) = active_state();
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::SubagentUpdate {
                subagent: subagent("1", SubagentStatus::Failed, 5, turn_id),
            },
        ),
    );

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::TurnFinished { interrupted: false },
        ),
    );

    assert_eq!(
        state.presentation.sessions[&session].subagents[0].status,
        SubagentStatus::Failed,
        "a clean finish must not resurrect an already-failed subagent as completed",
    );
}

#[test]
fn turn_finished_records_deterministic_monotonic_elapsed() {
    let started_at = Instant::now();
    let (mut state, session, turn_id) = active_state_at(started_at);
    state.transcripts.get_mut(&session).unwrap().items =
        vec![TranscriptItem::assistant_text("done")];

    reduce_agent_event_at(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::TurnFinished { interrupted: false },
        ),
        started_at + Duration::from_millis(42_125),
    );

    let turn = &state.transcripts[&session].turns[0];
    assert_eq!(turn.status, TranscriptTurnStatus::Completed);
    assert_eq!(turn.end_item_index, Some(1));
    assert_eq!(turn.elapsed, Some(Duration::from_millis(42_125)));
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
fn non_retryable_error_keeps_turn_busy_until_turn_finished_arrives() {
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

    assert!(state.transcripts[&session].busy);
    assert_eq!(state.active_turns.get(&session), Some(&turn_id));
}

#[test]
fn non_retryable_error_makes_the_eventual_turn_finish_failed() {
    let started_at = Instant::now();
    let (mut state, session, turn_id) = active_state_at(started_at);

    reduce_agent_event_at(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::Error {
                message: "provider stopped".into(),
                retryable: false,
            },
        ),
        started_at + Duration::from_secs(1),
    );
    reduce_agent_event_at(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::TurnFinished { interrupted: false },
        ),
        started_at + Duration::from_secs(2),
    );

    let transcript = &state.transcripts[&session];
    assert!(!transcript.busy);
    assert!(!state.active_turns.contains_key(&session));
    assert_eq!(transcript.turns[0].status, TranscriptTurnStatus::Failed);
    assert_eq!(transcript.turns[0].end_item_index, Some(1));
    assert_eq!(transcript.turns[0].elapsed, Some(Duration::from_secs(2)));
}

#[test]
fn terminal_error_takes_precedence_over_an_interrupted_finish_edge() {
    let (mut state, session, turn_id) = active_state();

    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            1,
            AgentEventKind::Error {
                message: "persistence failed after interrupt".into(),
                retryable: false,
            },
        ),
    );
    reduce_agent_event(
        &mut state,
        event(
            &session,
            turn_id,
            2,
            AgentEventKind::TurnFinished { interrupted: true },
        ),
    );

    assert_eq!(
        state.transcripts[&session].turns[0].status,
        TranscriptTurnStatus::Failed
    );
}

#[path = "reducer/stale-event-tests.rs"]
mod stale_event_tests;
