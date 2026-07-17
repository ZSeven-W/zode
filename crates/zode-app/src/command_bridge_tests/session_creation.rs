use zode_app_model::{
    reduce_agent_event, reduce_navigation_command, reduce_queue_command, AppCommand,
    BranchCatalogState, LoadState, NavigationOutcome, ProjectState, QueueCommandOutcome,
    ReduceOutcome, TranscriptItem, TranscriptState, TranscriptTurnStatus,
};
use zode_node_protocol::{
    AgentCommandKind, AgentEvent, AgentEventKind, SessionLocator, ThreadStatus, ThreadSummary,
    UserContent, WorkspaceUri, PROTOCOL_VERSION,
};

use super::{empty_fixture, first_submit, wait_for_commands, wait_for_result, FakeEndpoint};
use crate::command_bridge::{prepare_dispatch, CommandBridge};

struct TempTaskRoot(std::path::PathBuf);

impl TempTaskRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zode-projectless-submit-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        Self(path)
    }

    fn uri(&self) -> WorkspaceUri {
        zode_app_runtime::path_to_workspace_uri(&self.0).unwrap()
    }
}

#[test]
fn first_submit_waits_until_a_requested_branch_checkout_is_confirmed() {
    let mut state = zode_app_model::demo_state();
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects.push(ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.active_workspace = Some(workspace_uri.clone());
    state.composer.branch_picker.catalog = BranchCatalogState::Switching {
        workspace_uri,
        from: "main".into(),
        branch: "feature/menu".into(),
    };

    let error = prepare_dispatch(
        &mut state,
        AppCommand::Submit(vec![UserContent::Text {
            text: "hello".into(),
        }]),
    )
    .unwrap_err();

    assert!(error.contains("still being switched"));
    assert!(state.current_session.is_none());
}

impl Drop for TempTaskRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn selected_project_submit_uses_exact_active_workspace_before_starting_turn() {
    let endpoint = FakeEndpoint::success(Vec::new());
    let bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = zode_app_model::demo_state();
    let unselected = WorkspaceUri::new("file:///repo/first").unwrap();
    let selected = WorkspaceUri::new("file:///repo/zode").unwrap();
    for workspace_uri in [unselected, selected.clone()] {
        state.projects.push(ProjectState {
            workspace_uri,
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
    }
    state.active_workspace = Some(selected.clone());
    state.composer.model = Some("model-b".into());
    let dispatch = prepare_dispatch(
        &mut state,
        AppCommand::Submit(vec![UserContent::Text {
            text: "hello".into(),
        }]),
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        dispatch.commands[0].kind,
        AgentCommandKind::CreateSession {
            ref workspace_uri,
            model: Some(ref model),
            ..
        } if workspace_uri == &selected && model == "model-b"
    ));
    assert!(matches!(
        dispatch.commands[1].kind,
        AgentCommandKind::StartTurn { .. }
    ));
    assert_eq!(dispatch.commands[0].session, dispatch.commands[1].session);
    assert!(state.current_session.is_some());
    let session = state.current_session.as_ref().unwrap();
    let turn = &state.transcripts[session].turns[0];
    assert_eq!(turn.turn_id, dispatch.commands[1].turn_id);
    assert_eq!(turn.start_item_index, 0);
    assert_eq!(turn.response_item_index, 1);
    assert_eq!(turn.status, TranscriptTurnStatus::Running);
    assert!(turn.elapsed.is_none());
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 2).await;
    let commands = endpoint.commands.lock().unwrap();
    assert!(matches!(
        commands[0].kind,
        AgentCommandKind::CreateSession { .. }
    ));
    assert!(matches!(
        commands[1].kind,
        AgentCommandKind::StartTurn { .. }
    ));
}

#[test]
fn projectless_submit_uses_a_session_scratch_workspace_without_project_fallback() {
    let task_root = TempTaskRoot::new();
    let mut state = zode_app_model::demo_state();
    state.projects.push(ProjectState {
        workspace_uri: WorkspaceUri::new("file:///repo/must-not-be-selected").unwrap(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.projectless_workspace_root = Some(task_root.uri());

    let dispatch = first_submit(&mut state);
    let session = dispatch.commands[0].session.clone();
    let workspace_uri = match &dispatch.commands[0].kind {
        AgentCommandKind::CreateSession { workspace_uri, .. } => workspace_uri,
        other => panic!("expected CreateSession, got {other:?}"),
    };
    let workspace_path = zode_app_runtime::workspace_uri_to_path(workspace_uri).unwrap();

    assert_eq!(workspace_path, task_root.0.join(&session.session_id));
    assert!(workspace_path.is_dir());
    assert_eq!(state.active_workspace, None);
    assert_eq!(state.threads[0].workspace_uri, *workspace_uri);
    assert!(state.is_projectless_workspace(workspace_uri));
}

#[test]
fn submit_from_an_unavailable_historical_session_routes_to_the_active_workspace() {
    let mut state = zode_app_model::demo_state();
    let missing = WorkspaceUri::new("file:///repo/missing-history").unwrap();
    let startup = WorkspaceUri::new("file:///repo/startup").unwrap();
    let historical = SessionLocator::new(state.host.node_id, "missing-history");
    state.projects.extend([
        ProjectState {
            workspace_uri: missing.clone(),
            expanded: true,
            available: false,
            last_opened_ms: 10,
        },
        ProjectState {
            workspace_uri: startup.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        },
    ]);
    state.threads.push(ThreadSummary {
        session: historical.clone(),
        workspace_uri: missing,
        title: "missing".into(),
        updated_at_ms: 10,
        status: ThreadStatus::Idle,
    });
    state
        .transcripts
        .insert(historical.clone(), TranscriptState::default());
    state.current_session = Some(historical);
    state.active_workspace = Some(startup.clone());

    let dispatch = first_submit(&mut state);
    assert_eq!(dispatch.commands.len(), 2);
    assert!(matches!(
        &dispatch.commands[0].kind,
        AgentCommandKind::CreateSession { workspace_uri, .. } if workspace_uri == &startup
    ));
    assert_ne!(
        state.current_session.as_ref().unwrap().session_id,
        "missing-history"
    );
}

#[tokio::test]
async fn first_command_failure_rolls_back_uncreated_session_and_retry_recreates_it() {
    let endpoint = FakeEndpoint::failing_at(0);
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = empty_fixture();
    let dispatch = first_submit(&mut state);
    let uncreated = state.current_session.clone().unwrap();
    assert!(matches!(
        reduce_queue_command(
            &mut state,
            &AppCommand::EnqueueMessage {
                session: uncreated.clone(),
                content: vec![UserContent::Text {
                    text: "queued behind create".into(),
                }],
                attachments: Vec::new(),
                front: false,
            },
        ),
        QueueCommandOutcome::Enqueued(_)
    ));
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert!(!state.transcripts.contains_key(&uncreated));
    assert!(!state.message_queues.contains_key(&uncreated));
    assert!(state
        .current_session
        .as_ref()
        .unwrap()
        .session_id
        .starts_with("local-error-"));
    let retry = first_submit(&mut state);
    assert_eq!(retry.commands.len(), 2);
    assert!(matches!(
        retry.commands[0].kind,
        AgentCommandKind::CreateSession { .. }
    ));
}

#[tokio::test]
async fn projectless_create_failure_cleans_scratch_without_hijacking_newer_navigation() {
    let endpoint = FakeEndpoint::failing_at(0);
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let task_root = TempTaskRoot::new();
    std::fs::create_dir_all(&task_root.0).unwrap();
    let mut state = zode_app_model::demo_state();
    state.projectless_workspace_root = Some(task_root.uri());
    let dispatch = first_submit(&mut state);
    let failed_session = state.current_session.clone().unwrap();
    let scratch = zode_app_runtime::workspace_uri_to_path(&state.threads[0].workspace_uri).unwrap();
    assert!(scratch.is_dir());
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 1).await;

    let selected = WorkspaceUri::new("file:///repo/newer-selection").unwrap();
    state.projects.push(ProjectState {
        workspace_uri: selected.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::BeginTask {
                workspace_uri: Some(selected.clone()),
            },
        ),
        NavigationOutcome::Applied
    );
    wait_for_result(&mut bridge, &mut state).await;

    assert_eq!(state.current_session, None);
    assert_eq!(state.active_workspace, Some(selected));
    assert!(!scratch.exists());
    assert!(!state.transcripts.contains_key(&failed_session));
    assert!(state.threads.iter().any(|thread| {
        thread.session.session_id.starts_with("local-error-")
            && state.is_projectless_workspace(&thread.workspace_uri)
    }));
}

#[tokio::test]
async fn second_command_failure_keeps_created_session_retryable() {
    let endpoint = FakeEndpoint::failing_at(1);
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = empty_fixture();
    let dispatch = first_submit(&mut state);
    let created = state.current_session.clone().unwrap();
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 2).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert!(state.transcripts.contains_key(&created));
    assert!(!state.transcripts[&created].busy);
    assert_eq!(
        state.transcripts[&created].turns[0].status,
        TranscriptTurnStatus::Failed
    );
    assert!(state.transcripts[&created].turns[0].elapsed.is_some());
    let retry = prepare_dispatch(
        &mut state,
        AppCommand::Submit(vec![UserContent::Text {
            text: "retry".into(),
        }]),
    )
    .unwrap()
    .unwrap();
    assert_eq!(retry.commands.len(), 1);
    assert!(matches!(
        retry.commands[0].kind,
        AgentCommandKind::StartTurn { .. }
    ));
}

#[tokio::test]
async fn first_submit_runtime_readback_failure_keeps_the_started_turn_live() {
    let endpoint = FakeEndpoint::runtime_query_failure("runtime readback unavailable");
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = empty_fixture();
    let dispatch = first_submit(&mut state);
    let session = state.current_session.clone().unwrap();
    let turn_id = state.active_turns[&session];

    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 2).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert_eq!(state.active_turns.get(&session), Some(&turn_id));
    assert!(state.transcripts[&session].busy);
    assert!(matches!(
        state.threads.iter().find(|thread| thread.session == session),
        Some(thread) if thread.status == ThreadStatus::Running
    ));
    assert!(matches!(
        state.presentation.sessions[&session].runtime_options,
        LoadState::Failed(ref message) if message.contains("runtime readback unavailable")
    ));
    assert!(matches!(
        state.transcripts[&session].items.last(),
        Some(TranscriptItem::Error { message, retryable })
            if *retryable
                && message.contains("运行设置同步失败")
                && message.contains("runtime readback unavailable")
    ));

    assert_eq!(
        reduce_agent_event(
            &mut state,
            AgentEvent {
                version: PROTOCOL_VERSION,
                session: session.clone(),
                turn_id,
                sequence: 1,
                kind: AgentEventKind::TurnFinished { interrupted: false },
            },
        ),
        ReduceOutcome::Applied
    );
    assert!(!state.active_turns.contains_key(&session));
    assert!(!state.transcripts[&session].busy);
}
