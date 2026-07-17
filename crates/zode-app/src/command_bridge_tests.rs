use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;
use zode_app_model::{
    apply_session_runtime_options, AppCommand, LoadState, ProjectState, TranscriptItem,
    TranscriptState,
};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEndpoint, AgentEventStream, AgentQuery, AgentSnapshot,
    ApprovalDecision, EndpointError, EndpointErrorKind, RuntimeOptions, SandboxMode,
    SessionLocator, ThreadStatus, ThreadSummary, UserContent, WorkspaceUri,
};

use super::{prepare_dispatch, reject_dispatch, CommandBridge};

#[path = "command_bridge_tests/queue.rs"]
mod queue;
#[path = "command_bridge_tests/session_creation.rs"]
mod session_creation;

struct FakeEndpoint {
    commands: Mutex<Vec<AgentCommand>>,
    fail_at: Option<usize>,
    failure_kind: EndpointErrorKind,
    failure_message: String,
    permissions: Vec<String>,
    runtime_options: RuntimeOptions,
    runtime_query_error: Option<String>,
    runtime_snapshot_session: Option<SessionLocator>,
}

impl FakeEndpoint {
    fn success(permissions: Vec<String>) -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            fail_at: None,
            failure_kind: EndpointErrorKind::Unavailable,
            failure_message: "offline".into(),
            permissions,
            runtime_options: default_runtime_options(),
            runtime_query_error: None,
            runtime_snapshot_session: None,
        })
    }

    fn success_with_runtime(runtime_options: RuntimeOptions) -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            fail_at: None,
            failure_kind: EndpointErrorKind::Unavailable,
            failure_message: "offline".into(),
            permissions: Vec::new(),
            runtime_options,
            runtime_query_error: None,
            runtime_snapshot_session: None,
        })
    }

    fn runtime_snapshot_for(
        runtime_options: RuntimeOptions,
        runtime_snapshot_session: SessionLocator,
    ) -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            fail_at: None,
            failure_kind: EndpointErrorKind::Unavailable,
            failure_message: "offline".into(),
            permissions: Vec::new(),
            runtime_options,
            runtime_query_error: None,
            runtime_snapshot_session: Some(runtime_snapshot_session),
        })
    }

    fn runtime_query_failure(message: &str) -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            fail_at: None,
            failure_kind: EndpointErrorKind::Unavailable,
            failure_message: "offline".into(),
            permissions: Vec::new(),
            runtime_options: default_runtime_options(),
            runtime_query_error: Some(message.into()),
            runtime_snapshot_session: None,
        })
    }

    fn failing_at(index: usize) -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            fail_at: Some(index),
            failure_kind: EndpointErrorKind::Unavailable,
            failure_message: "offline".into(),
            permissions: Vec::new(),
            runtime_options: default_runtime_options(),
            runtime_query_error: None,
            runtime_snapshot_session: None,
        })
    }

    fn approval_fallback() -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            fail_at: Some(0),
            failure_kind: EndpointErrorKind::PartialSuccess,
            failure_message: "project permission could not be persisted; allowed once".into(),
            permissions: Vec::new(),
            runtime_options: default_runtime_options(),
            runtime_query_error: None,
            runtime_snapshot_session: None,
        })
    }

    fn approval_expired() -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            fail_at: Some(0),
            failure_kind: EndpointErrorKind::RequestExpired,
            failure_message: "approval requester is no longer available".into(),
            permissions: Vec::new(),
            runtime_options: default_runtime_options(),
            runtime_query_error: None,
            runtime_snapshot_session: None,
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
            AgentQuery::SessionRuntimeOptions { session } => {
                if let Some(message) = self.runtime_query_error.clone() {
                    return Err(EndpointError {
                        kind: EndpointErrorKind::Unavailable,
                        message,
                    });
                }
                Ok(AgentSnapshot::SessionRuntimeOptions {
                    session: self.runtime_snapshot_session.clone().unwrap_or(session),
                    options: self.runtime_options.clone(),
                })
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

fn default_runtime_options() -> RuntimeOptions {
    RuntimeOptions {
        models: vec!["test-model".into()],
        active_model: Some("test-model".into()),
        effort: None,
        sandbox_mode: SandboxMode::Off,
        sandbox_network: false,
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
async fn runtime_setting_projects_only_after_canonical_session_readback() {
    let canonical = RuntimeOptions {
        models: vec!["old-model".into(), "new-model".into()],
        active_model: Some("new-model".into()),
        effort: Some("high".into()),
        sandbox_mode: SandboxMode::ReadOnly,
        sandbox_network: true,
    };
    let endpoint = FakeEndpoint::success_with_runtime(canonical.clone());
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = fixture();
    let session = state.current_session.clone().unwrap();
    let old = default_runtime_options();
    assert!(apply_session_runtime_options(
        &mut state,
        session.clone(),
        old.clone(),
    ));

    let dispatch = prepare_dispatch(&mut state, AppCommand::SetModel("new-model".into()))
        .unwrap()
        .unwrap();
    assert_eq!(
        state.presentation.sessions[&session].runtime_options,
        LoadState::Ready(old)
    );
    bridge.dispatch(dispatch).unwrap();
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert_eq!(
        state.presentation.sessions[&session].runtime_options,
        LoadState::Ready(canonical)
    );
    assert_eq!(state.composer.model.as_deref(), Some("new-model"));
    assert_eq!(state.composer.effort.as_deref(), Some("high"));
    assert_eq!(state.composer.sandbox_label, "只读");
}

#[tokio::test]
async fn delayed_runtime_readback_stays_bound_to_the_original_session() {
    let canonical_a = RuntimeOptions {
        models: vec!["model-a".into()],
        active_model: Some("model-a".into()),
        effort: Some("high".into()),
        sandbox_mode: SandboxMode::ReadOnly,
        sandbox_network: true,
    };
    let endpoint = FakeEndpoint::success_with_runtime(canonical_a.clone());
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
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
    let old_a = default_runtime_options();
    assert!(apply_session_runtime_options(
        &mut state,
        session_a.clone(),
        old_a,
    ));
    let dispatch = prepare_dispatch(
        &mut state,
        AppCommand::SetSandbox {
            mode: SandboxMode::ReadOnly,
            network: true,
        },
    )
    .unwrap()
    .unwrap();
    bridge.dispatch(dispatch).unwrap();

    state.current_session = Some(session_b.clone());
    let options_b = RuntimeOptions {
        models: vec!["model-b".into()],
        active_model: Some("model-b".into()),
        effort: Some("low".into()),
        sandbox_mode: SandboxMode::Off,
        sandbox_network: false,
    };
    assert!(apply_session_runtime_options(
        &mut state,
        session_b.clone(),
        options_b.clone(),
    ));
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert_eq!(
        state.presentation.sessions[&session_a].runtime_options,
        LoadState::Ready(canonical_a)
    );
    assert_eq!(
        state.presentation.sessions[&session_b].runtime_options,
        LoadState::Ready(options_b)
    );
    assert_eq!(state.composer.model.as_deref(), Some("model-b"));
    assert_eq!(state.composer.effort.as_deref(), Some("low"));
}

#[tokio::test]
async fn wrong_session_or_failed_runtime_readback_retains_the_last_canonical_state() {
    for endpoint in [
        FakeEndpoint::runtime_snapshot_for(
            default_runtime_options(),
            SessionLocator::new(zode_node_protocol::NodeId::new(), "wrong"),
        ),
        FakeEndpoint::runtime_query_failure("runtime readback unavailable"),
    ] {
        let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
        let mut state = fixture();
        let session = state.current_session.clone().unwrap();
        let old = RuntimeOptions {
            models: vec!["stable".into()],
            active_model: Some("stable".into()),
            effort: Some("medium".into()),
            sandbox_mode: SandboxMode::WorkspaceWrite,
            sandbox_network: false,
        };
        assert!(apply_session_runtime_options(
            &mut state,
            session.clone(),
            old.clone(),
        ));
        let dispatch = prepare_dispatch(&mut state, AppCommand::SetEffort("high".into()))
            .unwrap()
            .unwrap();
        bridge.dispatch(dispatch).unwrap();
        wait_for_commands(&endpoint, 1).await;
        wait_for_result(&mut bridge, &mut state).await;

        assert_eq!(
            state.presentation.sessions[&session].runtime_options,
            LoadState::Ready(old)
        );
        assert!(matches!(
            state.transcripts[&session].items.last(),
            Some(TranscriptItem::Error { .. })
        ));
    }
}

#[tokio::test]
async fn new_session_is_populated_from_its_canonical_runtime_snapshot() {
    let canonical = RuntimeOptions {
        models: vec!["new-session-model".into()],
        active_model: Some("new-session-model".into()),
        effort: Some("medium".into()),
        sandbox_mode: SandboxMode::WorkspaceWrite,
        sandbox_network: false,
    };
    let endpoint = FakeEndpoint::success_with_runtime(canonical.clone());
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = empty_fixture();
    let workspace_uri = state.projects[0].workspace_uri.clone();
    let dispatch = prepare_dispatch(&mut state, AppCommand::NewSession { workspace_uri })
        .unwrap()
        .unwrap();
    let created = dispatch.commands[0].session.clone();
    bridge.dispatch(dispatch).unwrap();
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert_eq!(state.current_session.as_ref(), Some(&created));
    assert_eq!(
        state.presentation.sessions[&created].runtime_options,
        LoadState::Ready(canonical)
    );
    assert_eq!(state.composer.model.as_deref(), Some("new-session-model"));
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
    assert_eq!(
        state.project_permissions[&workspace],
        LoadState::Ready(vec!["write_file".into()])
    );
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;
    assert_eq!(
        state.project_permissions[&workspace],
        LoadState::Ready(Vec::new())
    );
}

#[tokio::test]
async fn zero_thread_project_permission_uses_node_scoped_revoke_locator() {
    let endpoint = FakeEndpoint::success(Vec::new());
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = empty_fixture();
    let workspace = state.projects[0].workspace_uri.clone();
    state.project_permissions.insert(
        workspace.clone(),
        LoadState::Ready(vec!["write_file".into()]),
    );
    let dispatch = revoke_dispatch(&mut state, &workspace);

    assert_eq!(dispatch.commands[0].session.node_id, state.host.node_id);
    assert_eq!(
        dispatch.commands[0].session.session_id,
        "settings-permission-revoke"
    );
    assert!(bridge.dispatch(dispatch).is_ok());
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;
    assert_eq!(
        state.project_permissions[&workspace],
        LoadState::Ready(Vec::new())
    );
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

    assert_eq!(
        state.project_permissions[&workspace],
        LoadState::Ready(vec!["write_file".into()])
    );
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

#[test]
fn projectless_allow_always_is_scoped_to_one_use() {
    let mut state = fixture();
    let session = state.current_session.clone().unwrap();
    let root = WorkspaceUri::new("file:///tmp/zode-task-workspaces").unwrap();
    let scratch = WorkspaceUri::new("file:///tmp/zode-task-workspaces/session").unwrap();
    state.projectless_workspace_root = Some(root);
    state.active_workspace = None;
    state.threads[0].workspace_uri = scratch;
    add_pending_approval(&mut state, &session);

    let dispatch = approval_dispatch(&mut state, ApprovalDecision::AllowAlways);

    assert!(matches!(
        dispatch.commands[0].kind,
        AgentCommandKind::Approve {
            decision: ApprovalDecision::AllowOnce,
            ..
        }
    ));
    assert!(state.transcripts[&session]
        .items
        .iter()
        .any(|item| matches!(
            item,
            TranscriptItem::Status { code, message }
                if code == "approval.projectless_allow_once" && message.contains("仅允许一次")
        )));
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
        .insert(workspace, LoadState::Ready(vec!["write_file".into()]));
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
