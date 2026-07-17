use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use async_trait::async_trait;
use zode_app_model::{
    demo_state, EnvironmentSnapshot, LoadState, PreviewKind, PreviewState, PreviewTarget,
    ProjectState, TranscriptState,
};
use zode_app_runtime::path_to_workspace_uri;
use zode_node_protocol::{
    AgentCommand, AgentEndpoint, AgentEventStream, AgentQuery, AgentSnapshot, DiffSnapshot,
    EndpointError, EndpointErrorKind, NodeId, SessionLocator, ThreadStatus, ThreadSummary,
};

use super::{PresentationQuery, PresentationQueryBridge};
use crate::services::{FileMetadata, FileService, LocalFileService, ServiceError};

struct FakeEndpoint {
    responses: Mutex<VecDeque<Result<AgentSnapshot, EndpointError>>>,
    queries: Mutex<Vec<AgentQuery>>,
}

impl FakeEndpoint {
    fn returning(response: Result<AgentSnapshot, EndpointError>) -> Arc<Self> {
        Self::returning_many([response])
    }

    fn returning_many(
        responses: impl IntoIterator<Item = Result<AgentSnapshot, EndpointError>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(responses.into_iter().collect()),
            queries: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl AgentEndpoint for FakeEndpoint {
    async fn command(&self, _command: AgentCommand) -> Result<(), EndpointError> {
        Err(unexpected())
    }

    async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
        self.queries.lock().unwrap().push(query);
        self.responses.lock().unwrap().pop_front().unwrap()
    }

    async fn subscribe(&self) -> Result<AgentEventStream, EndpointError> {
        Err(unexpected())
    }
}

struct BlockingDiffEndpoint {
    blocked_session: SessionLocator,
    block_once: AtomicBool,
    query_count: AtomicUsize,
    queries: Mutex<Vec<AgentQuery>>,
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

struct BlockingFileService {
    block_once: AtomicBool,
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl BlockingFileService {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            block_once: AtomicBool::new(true),
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        })
    }
}

#[async_trait]
impl FileService for BlockingFileService {
    async fn read(
        &self,
        _workspace: &zode_node_protocol::WorkspaceUri,
        _relative: &str,
    ) -> Result<Vec<u8>, ServiceError> {
        Err(ServiceError::Platform("unexpected unbounded read".into()))
    }

    async fn read_bounded(
        &self,
        _workspace: &zode_node_protocol::WorkspaceUri,
        relative: &str,
        _max_bytes: u64,
    ) -> Result<Vec<u8>, ServiceError> {
        if self.block_once.swap(false, Ordering::SeqCst) {
            self.started.notify_one();
            self.release.notified().await;
        }
        Ok(format!("content:{relative}").into_bytes())
    }

    async fn write(
        &self,
        _workspace: &zode_node_protocol::WorkspaceUri,
        _relative: &str,
        _bytes: Vec<u8>,
    ) -> Result<(), ServiceError> {
        Err(ServiceError::Platform("unexpected write".into()))
    }

    async fn metadata(
        &self,
        _workspace: &zode_node_protocol::WorkspaceUri,
        _relative: &str,
    ) -> Result<FileMetadata, ServiceError> {
        Err(ServiceError::Platform("unexpected metadata".into()))
    }
}

impl BlockingDiffEndpoint {
    fn new(blocked_session: SessionLocator) -> Arc<Self> {
        Arc::new(Self {
            blocked_session,
            block_once: AtomicBool::new(true),
            query_count: AtomicUsize::new(0),
            queries: Mutex::new(Vec::new()),
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        })
    }
}

#[async_trait]
impl AgentEndpoint for BlockingDiffEndpoint {
    async fn command(&self, _command: AgentCommand) -> Result<(), EndpointError> {
        Err(unexpected())
    }

    async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
        self.queries.lock().unwrap().push(query.clone());
        let AgentQuery::Diff { session } = query else {
            return Err(unexpected());
        };
        let sequence = self.query_count.fetch_add(1, Ordering::SeqCst) + 1;
        if session == self.blocked_session && self.block_once.swap(false, Ordering::SeqCst) {
            self.started.notify_one();
            self.release.notified().await;
        }
        Ok(AgentSnapshot::Diff(DiffSnapshot {
            session,
            files: Vec::new(),
            unified: format!("snapshot-{sequence}"),
        }))
    }

    async fn subscribe(&self) -> Result<AgentEventStream, EndpointError> {
        Err(unexpected())
    }
}

#[tokio::test]
async fn invalidation_during_flight_discards_old_diff_and_reruns_latest_generation() {
    let session = session("rerun");
    let endpoint = BlockingDiffEndpoint::new(session.clone());
    let (mut bridge, wake) = test_bridge(endpoint.clone());
    let mut state = demo_state();
    state.current_session = Some(session.clone());
    let query = PresentationQuery::Diff {
        session: session.clone(),
    };

    bridge.request(&mut state, query.clone()).unwrap();
    endpoint.started.notified().await;
    state
        .presentation
        .sessions
        .get_mut(&session)
        .unwrap()
        .diff
        .dirty = true;
    state.review.dirty = true;
    bridge.request(&mut state, query).unwrap();
    endpoint.release.notify_one();

    wake.notified().await;
    assert_eq!(bridge.drain_into(&mut state), 0);
    assert!(state.presentation.sessions[&session].diff.dirty);
    assert_eq!(
        state.presentation.sessions[&session].diff.load,
        LoadState::Loading
    );
    assert!(state.review.dirty);

    tokio::time::timeout(std::time::Duration::from_secs(1), wake.notified())
        .await
        .expect("the queued generation runs after the old result drains");
    assert_eq!(bridge.drain_into(&mut state), 1);
    assert_eq!(endpoint.queries.lock().unwrap().len(), 2);
    let ready = state.presentation.sessions[&session]
        .diff
        .load
        .ready()
        .expect("the latest diff is ready");
    assert_eq!(ready.unified, "snapshot-2");
    assert!(!state.presentation.sessions[&session].diff.dirty);
    assert!(!state.review.dirty);
}

#[tokio::test]
async fn slow_diff_does_not_block_another_sessions_query() {
    let slow = session("slow");
    let fast = session("fast");
    let endpoint = BlockingDiffEndpoint::new(slow.clone());
    let (mut bridge, wake) = test_bridge(endpoint.clone());
    let mut state = demo_state();

    bridge
        .request(
            &mut state,
            PresentationQuery::Diff {
                session: slow.clone(),
            },
        )
        .unwrap();
    endpoint.started.notified().await;
    bridge
        .request(
            &mut state,
            PresentationQuery::Diff {
                session: fast.clone(),
            },
        )
        .unwrap();

    tokio::time::timeout(std::time::Duration::from_secs(1), wake.notified())
        .await
        .expect("the fast session completes while the slow query is blocked");
    assert_eq!(bridge.drain_into(&mut state), 1);
    assert!(state.presentation.sessions[&fast]
        .diff
        .load
        .ready()
        .is_some());
    assert_eq!(
        state.presentation.sessions[&slow].diff.load,
        LoadState::Loading
    );

    endpoint.release.notify_one();
    wake.notified().await;
    assert_eq!(bridge.drain_into(&mut state), 1);
    assert!(state.presentation.sessions[&slow]
        .diff
        .load
        .ready()
        .is_some());
}

#[tokio::test]
async fn diff_error_becomes_failed_without_clearing_dirty() {
    let session = session("current");
    let endpoint = FakeEndpoint::returning(Err(EndpointError {
        kind: EndpointErrorKind::Unavailable,
        message: "offline".into(),
    }));
    let (mut bridge, wake) = test_bridge(endpoint);
    let mut state = demo_state();
    state.current_session = Some(session.clone());
    state.review.dirty = true;
    state
        .presentation
        .sessions
        .entry(session.clone())
        .or_default()
        .diff
        .dirty = true;

    bridge
        .request(
            &mut state,
            PresentationQuery::Diff {
                session: session.clone(),
            },
        )
        .unwrap();

    tokio::time::timeout(std::time::Duration::from_secs(1), wake.notified())
        .await
        .expect("a failed query still wakes the window loop");
    assert_eq!(bridge.drain_into(&mut state), 1);
    assert_eq!(
        state.presentation.sessions[&session].diff.load,
        LoadState::Failed("Unavailable: offline".into())
    );
    assert!(state.presentation.sessions[&session].diff.dirty);
    assert!(state.review.dirty);
}

#[tokio::test]
async fn wrong_session_diff_fails_only_the_requested_session() {
    let requested = session("requested");
    let returned = session("returned");
    let endpoint = FakeEndpoint::returning(Ok(AgentSnapshot::Diff(DiffSnapshot {
        session: returned.clone(),
        files: Vec::new(),
        unified: String::new(),
    })));
    let (mut bridge, wake) = test_bridge(endpoint);
    let mut state = demo_state();
    state.current_session = Some(requested.clone());
    state.review.dirty = true;
    state
        .presentation
        .sessions
        .entry(requested.clone())
        .or_default()
        .diff
        .dirty = true;

    bridge
        .request(
            &mut state,
            PresentationQuery::Diff {
                session: requested.clone(),
            },
        )
        .unwrap();

    wake.notified().await;
    assert_eq!(bridge.drain_into(&mut state), 1);
    assert_eq!(
        state.presentation.sessions[&requested].diff.load,
        LoadState::Failed("the endpoint returned a diff for the wrong session".into())
    );
    assert!(state.presentation.sessions[&requested].diff.dirty);
    assert!(!state.presentation.sessions.contains_key(&returned));
    assert!(state.review.dirty);
}

#[tokio::test]
async fn stale_session_result_does_not_change_the_current_review_projection() {
    let stale = session("stale");
    let current = session("current");
    let snapshot = DiffSnapshot {
        session: stale.clone(),
        files: Vec::new(),
        unified: String::new(),
    };
    let endpoint = FakeEndpoint::returning(Ok(AgentSnapshot::Diff(snapshot.clone())));
    let (mut bridge, wake) = test_bridge(endpoint);
    let mut state = demo_state();
    state.current_session = Some(current);
    state.review.dirty = true;
    state
        .presentation
        .sessions
        .entry(stale.clone())
        .or_default()
        .diff
        .dirty = true;

    bridge
        .request(
            &mut state,
            PresentationQuery::Diff {
                session: stale.clone(),
            },
        )
        .unwrap();
    wake.notified().await;
    assert_eq!(bridge.drain_into(&mut state), 1);

    assert_eq!(
        state.presentation.sessions[&stale].diff.load,
        LoadState::Ready(snapshot)
    );
    assert!(!state.presentation.sessions[&stale].diff.dirty);
    assert!(state.review.dirty);
}

#[tokio::test]
async fn local_environment_without_git_is_ready_with_empty_real_context() {
    let session = session("environment");
    let workspace = TempWorkspace::new();
    let workspace_uri = path_to_workspace_uri(&workspace.path).unwrap();
    let endpoint = FakeEndpoint::returning(Err(unexpected()));
    let (mut bridge, wake) = test_bridge(endpoint.clone());
    let mut state = demo_state();

    bridge
        .request(
            &mut state,
            PresentationQuery::Environment {
                session: session.clone(),
                workspace_uri: workspace_uri.clone(),
            },
        )
        .unwrap();
    assert_eq!(
        state.presentation.sessions[&session].context,
        LoadState::Loading
    );

    tokio::time::timeout(std::time::Duration::from_secs(2), wake.notified())
        .await
        .expect("environment completion wakes the window loop");
    assert_eq!(bridge.drain_into(&mut state), 1);
    assert_eq!(
        state.presentation.sessions[&session].context,
        LoadState::Ready(EnvironmentSnapshot {
            workspace_uri,
            branch: None,
            subagents: Vec::new(),
            background_processes: Vec::new(),
            sources: Vec::new(),
        })
    );
    assert!(endpoint.queries.lock().unwrap().is_empty());
}

#[tokio::test]
async fn local_environment_reports_the_real_git_branch() {
    let session = session("git-environment");
    let workspace = TempWorkspace::new();
    run_git(&workspace.path, &["init", "--quiet"]);
    run_git(
        &workspace.path,
        &["checkout", "--quiet", "-b", "feature/presentation"],
    );
    std::fs::write(workspace.path.join("README.md"), "zode\n").unwrap();
    run_git(&workspace.path, &["add", "README.md"]);
    run_git(
        &workspace.path,
        &[
            "-c",
            "user.name=Zode Test",
            "-c",
            "user.email=zode@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "initial",
        ],
    );
    let workspace_uri = path_to_workspace_uri(&workspace.path).unwrap();
    let endpoint = FakeEndpoint::returning(Err(unexpected()));
    let (mut bridge, wake) = test_bridge(endpoint.clone());
    let mut state = demo_state();

    bridge
        .request(
            &mut state,
            PresentationQuery::Environment {
                session: session.clone(),
                workspace_uri,
            },
        )
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), wake.notified())
        .await
        .expect("git branch completion wakes the window loop");
    assert_eq!(bridge.drain_into(&mut state), 1);

    let snapshot = state.presentation.sessions[&session]
        .context
        .ready()
        .expect("environment is ready");
    assert_eq!(snapshot.branch.as_deref(), Some("feature/presentation"));
    assert!(endpoint.queries.lock().unwrap().is_empty());
}

#[tokio::test]
async fn environment_reruns_latest_workspace_and_rejects_stale_thread_context() {
    let session = session("moving-workspace");
    let first = TempWorkspace::new();
    let second = TempWorkspace::new();
    let current = TempWorkspace::new();
    let first_uri = path_to_workspace_uri(&first.path).unwrap();
    let second_uri = path_to_workspace_uri(&second.path).unwrap();
    let current_uri = path_to_workspace_uri(&current.path).unwrap();
    let endpoint = FakeEndpoint::returning(Err(unexpected()));
    let (mut bridge, wake) = test_bridge(endpoint);
    let mut state = demo_state();
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: first_uri.clone(),
        title: "moving".into(),
        updated_at_ms: 0,
        status: ThreadStatus::Idle,
    });

    bridge
        .request(
            &mut state,
            PresentationQuery::Environment {
                session: session.clone(),
                workspace_uri: first_uri,
            },
        )
        .unwrap();
    state.threads[0].workspace_uri = second_uri.clone();
    bridge
        .request(
            &mut state,
            PresentationQuery::Environment {
                session: session.clone(),
                workspace_uri: second_uri,
            },
        )
        .unwrap();

    wake.notified().await;
    assert_eq!(bridge.drain_into(&mut state), 0);
    assert_eq!(
        state.presentation.sessions[&session].context,
        LoadState::Loading
    );

    wake.notified().await;
    state.threads[0].workspace_uri = current_uri.clone();
    assert_eq!(bridge.drain_into(&mut state), 0);
    assert_eq!(
        state.presentation.sessions[&session].context,
        LoadState::Idle
    );

    bridge
        .request(
            &mut state,
            PresentationQuery::Environment {
                session: session.clone(),
                workspace_uri: current_uri.clone(),
            },
        )
        .unwrap();
    wake.notified().await;
    assert_eq!(bridge.drain_into(&mut state), 1);
    let ready = state.presentation.sessions[&session]
        .context
        .ready()
        .expect("the current workspace context is ready");
    assert_eq!(ready.workspace_uri, current_uri);
}

#[tokio::test]
async fn document_preview_loads_real_markdown_and_plain_text_from_a_temp_workspace() {
    let workspace = TempWorkspace::new();
    std::fs::write(workspace.path.join("README.md"), "# Real markdown\n").unwrap();
    std::fs::write(workspace.path.join("notes.txt"), "plain text\n").unwrap();
    let workspace_uri = path_to_workspace_uri(&workspace.path).unwrap();
    let session = session("documents");
    let mut state = preview_state(session.clone(), workspace_uri.clone());
    let endpoint = FakeEndpoint::returning(Err(unexpected()));
    let (mut bridge, wake) = test_bridge_with_file_service(endpoint, Arc::new(LocalFileService));

    for (path, expected_kind, expected_content) in [
        ("README.md", PreviewKind::Markdown, "# Real markdown\n"),
        ("notes.txt", PreviewKind::PlainText, "plain text\n"),
    ] {
        let target = PreviewTarget {
            workspace_uri: workspace_uri.clone(),
            relative_path: path.into(),
        };
        bridge
            .request(
                &mut state,
                PresentationQuery::DocumentPreview {
                    session: session.clone(),
                    target: target.clone(),
                },
            )
            .unwrap();
        wake.notified().await;
        assert_eq!(bridge.drain_into(&mut state), 1);
        assert!(matches!(
            &state.presentation.sessions[&session].preview,
            PreviewState::Ready { target: ready_target, content, kind, .. }
                if ready_target == &target && content == expected_content && kind == &expected_kind
        ));
    }
}

#[tokio::test]
async fn document_preview_rejects_non_utf8_and_nul_without_lossy_decode() {
    let workspace = TempWorkspace::new();
    std::fs::write(workspace.path.join("invalid.txt"), [0xff, 0xfe]).unwrap();
    std::fs::write(workspace.path.join("binary.txt"), b"hello\0secret").unwrap();
    let workspace_uri = path_to_workspace_uri(&workspace.path).unwrap();
    let session = session("invalid-text");
    let mut state = preview_state(session.clone(), workspace_uri.clone());
    let endpoint = FakeEndpoint::returning(Err(unexpected()));
    let (mut bridge, wake) = test_bridge_with_file_service(endpoint, Arc::new(LocalFileService));

    for (path, expected) in [("invalid.txt", "UTF-8"), ("binary.txt", "binary")] {
        let target = PreviewTarget {
            workspace_uri: workspace_uri.clone(),
            relative_path: path.into(),
        };
        bridge
            .request(
                &mut state,
                PresentationQuery::DocumentPreview {
                    session: session.clone(),
                    target: target.clone(),
                },
            )
            .unwrap();
        wake.notified().await;
        assert_eq!(bridge.drain_into(&mut state), 1);
        assert!(matches!(
            &state.presentation.sessions[&session].preview,
            PreviewState::Failed { target: failed_target, message }
                if failed_target == &target && message.contains(expected) && !message.contains('�')
        ));
    }
}

#[tokio::test]
async fn newer_document_path_discards_the_in_flight_result() {
    let workspace = TempWorkspace::new();
    let workspace_uri = path_to_workspace_uri(&workspace.path).unwrap();
    let session = session("latest-document");
    let mut state = preview_state(session.clone(), workspace_uri.clone());
    let endpoint = FakeEndpoint::returning(Err(unexpected()));
    let files = BlockingFileService::new();
    let (mut bridge, wake) = test_bridge_with_file_service(endpoint, files.clone());
    let old = PreviewTarget {
        workspace_uri: workspace_uri.clone(),
        relative_path: "old.md".into(),
    };
    let latest = PreviewTarget {
        workspace_uri,
        relative_path: "latest.md".into(),
    };

    bridge
        .request(
            &mut state,
            PresentationQuery::DocumentPreview {
                session: session.clone(),
                target: old,
            },
        )
        .unwrap();
    files.started.notified().await;
    bridge
        .request(
            &mut state,
            PresentationQuery::DocumentPreview {
                session: session.clone(),
                target: latest.clone(),
            },
        )
        .unwrap();
    files.release.notify_one();

    wake.notified().await;
    assert_eq!(bridge.drain_into(&mut state), 0);
    assert_eq!(
        state.presentation.sessions[&session].preview,
        PreviewState::Loading {
            target: latest.clone()
        }
    );
    wake.notified().await;
    assert_eq!(bridge.drain_into(&mut state), 1);
    assert!(matches!(
        &state.presentation.sessions[&session].preview,
        PreviewState::Ready { target, content, .. }
            if target == &latest && content == "content:latest.md"
    ));
}

#[tokio::test]
async fn deleted_or_retargeted_session_discards_document_results() {
    let first = TempWorkspace::new();
    let second = TempWorkspace::new();
    std::fs::write(first.path.join("report.md"), "first").unwrap();
    let first_uri = path_to_workspace_uri(&first.path).unwrap();
    let second_uri = path_to_workspace_uri(&second.path).unwrap();
    let endpoint = FakeEndpoint::returning(Err(unexpected()));
    let (mut bridge, wake) = test_bridge_with_file_service(endpoint, Arc::new(LocalFileService));

    let deleted = session("deleted-preview");
    let mut state = preview_state(deleted.clone(), first_uri.clone());
    let deleted_target = PreviewTarget {
        workspace_uri: first_uri.clone(),
        relative_path: "report.md".into(),
    };
    bridge
        .request(
            &mut state,
            PresentationQuery::DocumentPreview {
                session: deleted.clone(),
                target: deleted_target,
            },
        )
        .unwrap();
    wake.notified().await;
    state.threads.clear();
    state.transcripts.remove(&deleted);
    state.presentation.sessions.remove(&deleted);
    assert_eq!(bridge.drain_into(&mut state), 0);

    let moved = session("moved-preview");
    let mut state = preview_state(moved.clone(), first_uri.clone());
    let moved_target = PreviewTarget {
        workspace_uri: first_uri,
        relative_path: "report.md".into(),
    };
    bridge
        .request(
            &mut state,
            PresentationQuery::DocumentPreview {
                session: moved.clone(),
                target: moved_target,
            },
        )
        .unwrap();
    wake.notified().await;
    state.threads[0].workspace_uri = second_uri;
    assert_eq!(bridge.drain_into(&mut state), 0);
    assert!(!matches!(
        state.presentation.sessions[&moved].preview,
        PreviewState::Ready { .. }
    ));
}

fn test_bridge(
    endpoint: Arc<dyn AgentEndpoint>,
) -> (PresentationQueryBridge, Arc<tokio::sync::Notify>) {
    let wake = Arc::new(tokio::sync::Notify::new());
    let wake_worker = Arc::clone(&wake);
    let bridge = PresentationQueryBridge::spawn_with_wake(endpoint, move || {
        wake_worker.notify_one();
    });
    (bridge, wake)
}

fn test_bridge_with_file_service(
    endpoint: Arc<dyn AgentEndpoint>,
    files: Arc<dyn FileService>,
) -> (PresentationQueryBridge, Arc<tokio::sync::Notify>) {
    let wake = Arc::new(tokio::sync::Notify::new());
    let wake_worker = Arc::clone(&wake);
    let bridge = PresentationQueryBridge::spawn_with_services(endpoint, files, move || {
        wake_worker.notify_one();
    });
    (bridge, wake)
}

fn preview_state(
    session: SessionLocator,
    workspace_uri: zode_node_protocol::WorkspaceUri,
) -> zode_app_model::ZodeAppState {
    let mut state = demo_state();
    state.projects.push(ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace_uri.clone(),
        title: "preview".into(),
        updated_at_ms: 0,
        status: ThreadStatus::Idle,
    });
    state
        .transcripts
        .insert(session.clone(), TranscriptState::default());
    state
        .presentation
        .sessions
        .entry(session.clone())
        .or_default();
    state.current_session = Some(session);
    state.active_workspace = Some(workspace_uri);
    state
}

fn session(id: &str) -> SessionLocator {
    SessionLocator::new(
        NodeId::parse("00000000-0000-0000-0000-000000000001").unwrap(),
        id,
    )
}

fn unexpected() -> EndpointError {
    EndpointError {
        kind: EndpointErrorKind::InvalidRequest,
        message: "unexpected endpoint call".into(),
    }
}

struct TempWorkspace {
    path: std::path::PathBuf,
}

impl TempWorkspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("zode-presentation-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn run_git(cwd: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
