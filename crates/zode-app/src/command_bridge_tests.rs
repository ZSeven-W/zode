use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;
use zode_app_model::{AppCommand, ProjectState, TranscriptItem, TranscriptState};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEndpoint, AgentEventStream, AgentQuery, AgentSnapshot,
    ApprovalDecision, EndpointError, EndpointErrorKind, SessionLocator, ThreadStatus,
    ThreadSummary, UserContent, WorkspaceUri,
};

use super::{prepare_dispatch, reject_dispatch, CommandBridge};

struct FakeEndpoint {
    commands: Mutex<Vec<AgentCommand>>,
    fail_at: Option<usize>,
    failure_kind: EndpointErrorKind,
    failure_message: String,
    permissions: Vec<String>,
}

impl FakeEndpoint {
    fn success(permissions: Vec<String>) -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            fail_at: None,
            failure_kind: EndpointErrorKind::Unavailable,
            failure_message: "offline".into(),
            permissions,
        })
    }

    fn failing_at(index: usize) -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            fail_at: Some(index),
            failure_kind: EndpointErrorKind::Unavailable,
            failure_message: "offline".into(),
            permissions: Vec::new(),
        })
    }

    fn approval_fallback() -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            fail_at: Some(0),
            failure_kind: EndpointErrorKind::PartialSuccess,
            failure_message: "project permission could not be persisted; allowed once".into(),
            permissions: Vec::new(),
        })
    }

    fn approval_expired() -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            fail_at: Some(0),
            failure_kind: EndpointErrorKind::RequestExpired,
            failure_message: "approval requester is no longer available".into(),
            permissions: Vec::new(),
        })
    }
}

#[async_trait]
impl AgentEndpoint for FakeEndpoint {
    async fn command(&self, command: AgentCommand) -> Result<(), EndpointError> {
        let mut commands = self.commands.lock().unwrap();
        let index = commands.len();
        commands.push(command);
        if self.fail_at == Some(index) {
            Err(EndpointError {
                kind: self.failure_kind,
                message: self.failure_message.clone(),
            })
        } else {
            Ok(())
        }
    }

    async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
        match query {
            AgentQuery::ProjectPermissions { .. } => {
                Ok(AgentSnapshot::ProjectPermissions(self.permissions.clone()))
            }
            _ => Err(EndpointError {
                kind: EndpointErrorKind::InvalidRequest,
                message: "unexpected query".into(),
            }),
        }
    }

    async fn subscribe(&self) -> Result<AgentEventStream, EndpointError> {
        Ok(Box::pin(futures_util::stream::empty()))
    }
}

#[tokio::test]
async fn command_worker_preserves_dispatch_order() {
    let endpoint = FakeEndpoint::success(Vec::new());
    let bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = fixture();
    assert!(bridge
        .dispatch(
            prepare_dispatch(&mut state, AppCommand::SetModel("model-a".into()))
                .unwrap()
                .unwrap(),
        )
        .is_ok());
    assert!(bridge
        .dispatch(
            prepare_dispatch(&mut state, AppCommand::SetEffort("high".into()))
                .unwrap()
                .unwrap(),
        )
        .is_ok());
    wait_for_commands(&endpoint, 2).await;

    let commands = endpoint.commands.lock().unwrap();
    assert!(matches!(
        commands[0].kind,
        AgentCommandKind::SetModel { .. }
    ));
    assert!(matches!(
        commands[1].kind,
        AgentCommandKind::SetEffort { .. }
    ));
}

#[tokio::test]
async fn zero_history_submit_creates_session_before_starting_turn() {
    let endpoint = FakeEndpoint::success(Vec::new());
    let bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = zode_app_model::demo_state();
    state.projects.push(ProjectState {
        workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
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
        AgentCommandKind::CreateSession { .. }
    ));
    assert!(matches!(
        dispatch.commands[1].kind,
        AgentCommandKind::StartTurn { .. }
    ));
    assert_eq!(dispatch.commands[0].session, dispatch.commands[1].session);
    assert!(state.current_session.is_some());
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

#[test]
fn new_session_rejects_an_unavailable_workspace() {
    let mut state = zode_app_model::demo_state();
    let missing = WorkspaceUri::new("file:///repo/missing").unwrap();
    state.projects.push(ProjectState {
        workspace_uri: missing.clone(),
        expanded: true,
        available: false,
        last_opened_ms: 0,
    });

    assert!(prepare_dispatch(
        &mut state,
        AppCommand::NewSession {
            workspace_uri: missing,
        },
    )
    .is_err());
}

#[tokio::test]
async fn revoke_updates_only_after_successful_refresh() {
    let endpoint = FakeEndpoint::success(Vec::new());
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = fixture_with_permission();
    let workspace = state.threads[0].workspace_uri.clone();
    let dispatch = revoke_dispatch(&mut state, &workspace);
    assert_eq!(state.project_permissions[&workspace], ["write_file"]);
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;
    assert!(!state.project_permissions.contains_key(&workspace));
}

#[tokio::test]
async fn zero_thread_project_permission_uses_node_scoped_revoke_locator() {
    let endpoint = FakeEndpoint::success(Vec::new());
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = empty_fixture();
    let workspace = state.projects[0].workspace_uri.clone();
    state
        .project_permissions
        .insert(workspace.clone(), vec!["write_file".into()]);
    let dispatch = revoke_dispatch(&mut state, &workspace);

    assert_eq!(dispatch.commands[0].session.node_id, state.host.node_id);
    assert_eq!(
        dispatch.commands[0].session.session_id,
        "settings-permission-revoke"
    );
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;
    assert!(!state.project_permissions.contains_key(&workspace));
}

#[tokio::test]
async fn revoke_failure_keeps_permission_and_projects_visible_error() {
    let endpoint = FakeEndpoint::failing_at(0);
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = fixture_with_permission();
    let workspace = state.threads[0].workspace_uri.clone();
    let dispatch = revoke_dispatch(&mut state, &workspace);
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert_eq!(state.project_permissions[&workspace], ["write_file"]);
    let session = state.current_session.as_ref().unwrap();
    assert!(matches!(
        state.transcripts[session].items.last(),
        Some(TranscriptItem::Error { message, .. }) if message.contains("offline")
    ));
}

#[tokio::test]
async fn allow_always_persistence_fallback_removes_stale_card_and_reports_allow_once() {
    let endpoint = FakeEndpoint::approval_fallback();
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = fixture();
    let session = state.current_session.clone().unwrap();
    state.approvals.insert("approval-1".into(), session.clone());
    state
        .transcripts
        .get_mut(&session)
        .unwrap()
        .items
        .push(TranscriptItem::Approval {
            id: "approval-1".into(),
            tool: "write_file".into(),
        });
    let dispatch = prepare_dispatch(
        &mut state,
        AppCommand::Approve {
            id: "approval-1".into(),
            decision: ApprovalDecision::AllowAlways,
        },
    )
    .unwrap()
    .unwrap();
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert!(!state.approvals.contains_key("approval-1"));
    assert!(state.transcripts[&session]
        .items
        .iter()
        .all(|item| !matches!(item, TranscriptItem::Approval { id, .. } if id == "approval-1")));
    assert!(matches!(
        state.transcripts[&session].items.last(),
        Some(TranscriptItem::Status { code, message })
            if code == "approval.allow_always_fallback" && message.contains("已仅允许一次")
    ));
}

#[tokio::test]
async fn approval_preflight_failure_keeps_the_card_retryable() {
    let endpoint = FakeEndpoint::failing_at(0);
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = fixture();
    let session = state.current_session.clone().unwrap();
    add_pending_approval(&mut state, &session);
    let dispatch = approval_dispatch(&mut state, ApprovalDecision::AllowAlways);
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert!(state.approvals.contains_key("approval-1"));
    assert!(state.transcripts[&session]
        .items
        .iter()
        .any(|item| matches!(item, TranscriptItem::Approval { id, .. } if id == "approval-1")));
    assert!(matches!(
        state.transcripts[&session].items.last(),
        Some(TranscriptItem::Error { message, retryable })
            if *retryable && message.contains("offline")
    ));
}

#[tokio::test]
async fn expired_approval_request_removes_the_stale_card_with_structured_status() {
    let endpoint = FakeEndpoint::approval_expired();
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = fixture();
    let session = state.current_session.clone().unwrap();
    add_pending_approval(&mut state, &session);
    let dispatch = approval_dispatch(&mut state, ApprovalDecision::AllowOnce);
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert!(!state.approvals.contains_key("approval-1"));
    assert!(state.transcripts[&session]
        .items
        .iter()
        .all(|item| !matches!(item, TranscriptItem::Approval { id, .. } if id == "approval-1")));
    assert!(matches!(
        state.transcripts[&session].items.last(),
        Some(TranscriptItem::Status { code, message })
            if code == "approval.request_expired" && message.contains("已失效")
    ));
}

#[tokio::test]
async fn first_command_failure_rolls_back_uncreated_session_and_retry_recreates_it() {
    let endpoint = FakeEndpoint::failing_at(0);
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = empty_fixture();
    let dispatch = first_submit(&mut state);
    let uncreated = state.current_session.clone().unwrap();
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert!(!state.transcripts.contains_key(&uncreated));
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

#[test]
fn closed_command_channel_rolls_back_optimistic_turn_and_shows_error() {
    let (sender, receiver) = mpsc::unbounded_channel();
    drop(receiver);
    let (_result_sender, results) = mpsc::unbounded_channel();
    let bridge = CommandBridge { sender, results };
    let mut state = fixture();
    let session = state.current_session.clone().unwrap();
    let dispatch = prepare_dispatch(
        &mut state,
        AppCommand::Submit(vec![UserContent::Text {
            text: "hello".into(),
        }]),
    )
    .unwrap()
    .unwrap();
    assert!(state.active_turns.contains_key(&session));

    let rejected = bridge.dispatch(dispatch).unwrap_err();
    reject_dispatch(&mut state, rejected, "command pump closed".into());
    assert!(!state.active_turns.contains_key(&session));
    assert!(!state.transcripts[&session].busy);
    assert!(matches!(
        state.transcripts[&session].items.last(),
        Some(TranscriptItem::Error { message, .. }) if message.contains("command pump closed")
    ));
}

fn revoke_dispatch(
    state: &mut zode_app_model::ZodeAppState,
    workspace: &WorkspaceUri,
) -> super::CommandDispatch {
    prepare_dispatch(
        state,
        AppCommand::RevokeProjectPermission {
            workspace_uri: workspace.clone(),
            tool: "write_file".into(),
        },
    )
    .unwrap()
    .unwrap()
}

fn add_pending_approval(state: &mut zode_app_model::ZodeAppState, session: &SessionLocator) {
    state.approvals.insert("approval-1".into(), session.clone());
    state
        .transcripts
        .get_mut(session)
        .unwrap()
        .items
        .push(TranscriptItem::Approval {
            id: "approval-1".into(),
            tool: "write_file".into(),
        });
}

fn approval_dispatch(
    state: &mut zode_app_model::ZodeAppState,
    decision: ApprovalDecision,
) -> super::CommandDispatch {
    prepare_dispatch(
        state,
        AppCommand::Approve {
            id: "approval-1".into(),
            decision,
        },
    )
    .unwrap()
    .unwrap()
}

fn first_submit(state: &mut zode_app_model::ZodeAppState) -> super::CommandDispatch {
    prepare_dispatch(
        state,
        AppCommand::Submit(vec![UserContent::Text {
            text: "hello".into(),
        }]),
    )
    .unwrap()
    .unwrap()
}

fn empty_fixture() -> zode_app_model::ZodeAppState {
    let mut state = zode_app_model::demo_state();
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects.push(ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.active_workspace = Some(workspace_uri);
    state
}

async fn wait_for_commands(endpoint: &FakeEndpoint, expected: usize) {
    for _ in 0..100 {
        if endpoint.commands.lock().unwrap().len() >= expected {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("command worker did not receive {expected} commands");
}

async fn wait_for_result(bridge: &mut CommandBridge, state: &mut zode_app_model::ZodeAppState) {
    for _ in 0..100 {
        if bridge.drain_into(state) > 0 {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("command worker did not return a completion");
}

fn fixture_with_permission() -> zode_app_model::ZodeAppState {
    let mut state = fixture();
    let workspace = state.threads[0].workspace_uri.clone();
    state
        .project_permissions
        .insert(workspace, vec!["write_file".into()]);
    state
}

fn fixture() -> zode_app_model::ZodeAppState {
    let mut state = zode_app_model::demo_state();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(state.host.node_id, "session");
    state.current_session = Some(session.clone());
    state.active_workspace = Some(workspace.clone());
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace,
        title: "Task".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state
        .transcripts
        .insert(session, TranscriptState::default());
    state
}
