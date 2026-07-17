use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent::abort::AbortController;
use agent::error::AgentError;
use agent::message::{ContentBlock, Header, Message, MessageStore, ToolResultContent};
use agent::stream::{Event, ResultData};
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use tokio::sync::Mutex as AsyncMutex;
use zode_app_runtime::{
    DriverEventStream, EngineDriver, LoadedSession, LocalAgentEndpoint, LocalAppRuntime,
    LocalSessionRepository, NodeIdentityStore, SessionEngine, SessionEngineFactory,
    SessionEngineSnapshot, ZodeEngineDriver,
};
use zode_core::approval::{approval_queue, Approval, ApprovalQueue};
use zode_core::bootstrap::AppBootstrap;
use zode_core::config::ConfigManager;
use zode_core::engine::CarryState;
use zode_core::EngineTemplate;
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEndpoint, AgentEvent, AgentEventKind, AgentEventStream,
    AgentQuery, AgentSnapshot, ApprovalDecision, CapabilityManifest, DiffSnapshot, EndpointError,
    EndpointErrorKind, NodeCapability, NodeId, SessionLocator, ThreadHistory, TurnId,
    UsageSnapshot, WorkspaceUri, PROTOCOL_VERSION,
};

const EVENT_DEADLINE: Duration = Duration::from_secs(5);
const MODEL: &str = "fixture-model";
const TOOL_ID: &str = "fixture-file-edit";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

pub struct FixtureApp {
    root: Arc<FixtureRoot>,
    endpoint: Arc<LocalAgentEndpoint>,
    events: AsyncMutex<AgentEventStream>,
    received: AsyncMutex<Vec<AgentEvent>>,
    node_id: NodeId,
}

impl FixtureApp {
    pub async fn start(label: &str) -> Self {
        Self::build(Arc::new(FixtureRoot::new(label))).await
    }

    async fn build(root: Arc<FixtureRoot>) -> Self {
        let node_id = NodeIdentityStore::new(&root.config_dir)
            .load_or_create()
            .expect("fixture node identity should load");
        let (approvals, approval_rx) = approval_queue();
        let mut bootstrap = AppBootstrap::for_test(root.config_dir.clone())
            .resolve()
            .await
            .expect("production app bootstrap should resolve in test mode");
        let mut config = bootstrap.cfg.clone();
        config.provider.model = Some(MODEL.into());
        config.browser.enabled = Some(false);
        config.noema.enabled = Some(false);
        let template = EngineTemplate::new(
            config.clone(),
            root.workspace.clone(),
            Some(approvals.clone()),
            false,
            bootstrap.sandbox.clone(),
            "1970-01-01".into(),
        )
        .with_question_queue(Some(bootstrap.question_queue.clone()));
        bootstrap.needs_setup = false;
        bootstrap.cfg = config;
        bootstrap.template = template.clone();
        bootstrap.approval_rx = approval_rx;
        let capabilities = CapabilityManifest {
            node_id,
            capabilities: BTreeSet::from([
                NodeCapability::Agent,
                NodeCapability::Workspace,
                NodeCapability::FileSystem,
                NodeCapability::Approval,
            ]),
        };
        let repository = LocalSessionRepository::new(&root.config_dir, node_id);
        let driver: Arc<dyn EngineDriver> = Arc::new(ZodeEngineDriver::with_factory(
            node_id,
            template,
            repository,
            capabilities,
            Arc::new(FakeProviderFactory { approvals }),
        ));
        let runtime = LocalAppRuntime::with_driver(&root.config_dir, bootstrap, driver, 8)
            .expect("production local runtime should compose");
        let endpoint = runtime.endpoint();
        let events = endpoint
            .subscribe()
            .await
            .expect("fixture endpoint should accept its first subscriber");
        Self {
            root,
            endpoint,
            events: AsyncMutex::new(events),
            received: AsyncMutex::new(Vec::new()),
            node_id,
        }
    }

    pub async fn restart(self) -> Self {
        let root = self.root.clone();
        let node_id = self.node_id;
        drop(self);
        tokio::task::yield_now().await;
        let restarted = Self::build(root).await;
        assert_eq!(
            restarted.node_id, node_id,
            "node identity must come from disk"
        );
        restarted
    }

    pub async fn new_session(&self) -> SessionLocator {
        let session = SessionLocator::new(
            self.node_id,
            format!("desktop-first-release-{}", self.root.label),
        );
        self.endpoint
            .command(agent_command(
                &session,
                None,
                AgentCommandKind::CreateSession {
                    workspace_uri: self.workspace_uri(),
                    project_uri: None,
                    projectless: false,
                    model: Some(MODEL.into()),
                },
            ))
            .await
            .expect("production repository should create the fixture session");
        session
    }

    pub async fn send(&self, session: &SessionLocator, text: &str) -> TurnId {
        let turn_id = TurnId::new();
        self.endpoint
            .command(agent_command(
                session,
                Some(turn_id),
                AgentCommandKind::StartTurn {
                    input: vec![zode_node_protocol::UserContent::Text { text: text.into() }],
                },
            ))
            .await
            .expect("fixture turn should start through the production runtime");
        turn_id
    }

    pub async fn wait_for_approval(&self, tool_name: &str) -> String {
        loop {
            let event = self.next_event().await;
            let approval = match &event.kind {
                AgentEventKind::ApprovalRequested {
                    approval_id, tool, ..
                } if tool == tool_name => Some(approval_id.clone()),
                _ => None,
            };
            self.record(event).await;
            if let Some(approval) = approval {
                return approval;
            }
        }
    }

    pub async fn approve_always(&self, session: &SessionLocator, approval_id: &str) {
        self.endpoint
            .command(agent_command(
                session,
                None,
                AgentCommandKind::Approve {
                    approval_id: approval_id.into(),
                    decision: ApprovalDecision::AllowAlways,
                },
            ))
            .await
            .expect("production approval pump should resolve AllowAlways");
    }

    pub async fn interrupt(
        &self,
        session: &SessionLocator,
        turn_id: TurnId,
    ) -> Result<(), EndpointError> {
        self.endpoint
            .command(agent_command(
                session,
                Some(turn_id),
                AgentCommandKind::InterruptTurn,
            ))
            .await
    }

    pub async fn wait_finished(&self, turn_id: TurnId) -> Vec<AgentEvent> {
        loop {
            if self.received.lock().await.iter().any(|event| {
                event.turn_id == turn_id
                    && matches!(event.kind, AgentEventKind::TurnFinished { .. })
            }) {
                return self
                    .received
                    .lock()
                    .await
                    .iter()
                    .filter(|event| event.turn_id == turn_id)
                    .cloned()
                    .collect();
            }
            let event = self.next_event().await;
            self.record(event).await;
        }
    }

    pub async fn diff(&self, session: &SessionLocator) -> DiffSnapshot {
        match self
            .endpoint
            .query(AgentQuery::Diff {
                session: session.clone(),
            })
            .await
            .expect("production diff query should succeed")
        {
            AgentSnapshot::Diff(diff) => diff,
            other => panic!("expected diff snapshot, got {other:?}"),
        }
    }

    pub async fn resume(&self, session: &SessionLocator) -> ThreadHistory {
        match self
            .endpoint
            .query(AgentQuery::History {
                session: session.clone(),
            })
            .await
            .expect("production history query should succeed")
        {
            AgentSnapshot::History(history) => history,
            other => panic!("expected history snapshot, got {other:?}"),
        }
    }

    pub async fn project_permissions(&self) -> Vec<String> {
        match self
            .endpoint
            .query(AgentQuery::ProjectPermissions {
                workspace_uri: self.workspace_uri(),
            })
            .await
            .expect("production permission query should succeed")
        {
            AgentSnapshot::ProjectPermissions(tools) => tools,
            other => panic!("expected project permissions, got {other:?}"),
        }
    }

    pub fn production_session_is_persisted(&self, session: &SessionLocator) -> bool {
        let sessions = self.root.config_dir.join("sessions");
        [
            sessions.join("index.json"),
            sessions.join(format!("{}.jsonl", session.session_id)),
        ]
        .into_iter()
        .all(|path| path.metadata().is_ok_and(|metadata| metadata.len() > 0))
    }

    pub async fn expect_no_event(&self, duration: Duration) -> bool {
        let mut events = self.events.lock().await;
        tokio::time::timeout(duration, events.next()).await.is_err()
    }

    async fn next_event(&self) -> AgentEvent {
        let mut events = self.events.lock().await;
        tokio::time::timeout(EVENT_DEADLINE, events.next())
            .await
            .expect("timed out waiting for fixture event")
            .expect("fixture event stream closed")
            .expect("fixture event carried an endpoint error")
    }

    async fn record(&self, event: AgentEvent) {
        println!(
            "zode-app-e2e session={} sequence={} kind={}",
            event.session.session_id,
            event.sequence,
            event_kind(&event.kind)
        );
        self.received.lock().await.push(event);
    }

    fn workspace_uri(&self) -> WorkspaceUri {
        zode_app_runtime::path_to_workspace_uri(&self.root.workspace)
            .expect("fixture workspace path should encode")
    }
}

struct FakeProviderFactory {
    approvals: ApprovalQueue,
}

#[async_trait]
impl SessionEngineFactory for FakeProviderFactory {
    async fn assemble(
        &self,
        _template: &EngineTemplate,
        session: &SessionLocator,
        loaded: LoadedSession,
        _carry: Option<CarryState>,
    ) -> Result<Arc<dyn SessionEngine>, EndpointError> {
        Ok(Arc::new(FakeProviderSession {
            approvals: self.approvals.clone(),
            session_id: session.session_id.clone(),
            store: Arc::new(Mutex::new(loaded.store)),
            model: loaded.meta.model,
            cwd: PathBuf::from(loaded.meta.cwd),
        }))
    }
}

struct FakeProviderSession {
    approvals: ApprovalQueue,
    session_id: String,
    store: Arc<Mutex<MessageStore>>,
    model: String,
    cwd: PathBuf,
}

#[async_trait]
impl SessionEngine for FakeProviderSession {
    async fn start_turn(
        &self,
        input: Vec<ContentBlock>,
        abort: AbortController,
    ) -> DriverEventStream {
        lock(&self.store)
            .push(Message::User {
                header: Header::new(),
                content: input,
            })
            .expect("fixture user message should be unique");
        let approvals = self.approvals.clone();
        let session_id = self.session_id.clone();
        let store = self.store.clone();
        let cwd = self.cwd.clone();
        let prelude = stream::iter([
            Ok(Event::TextDelta {
                delta: "edited".into(),
            }),
            Ok(Event::ToolUse {
                id: TOOL_ID.into(),
                name: "FileEdit".into(),
                input: serde_json::json!({"path": "a.txt"}),
            }),
        ]);
        let approval = stream::once(async move {
            let decision = approvals
                .request(
                    "FileEdit",
                    &serde_json::json!({"path": "a.txt"}),
                    Some(session_id),
                )
                .await;
            if abort.is_aborted() {
                return Err(AgentError::Aborted("fixture turn interrupted".into()));
            }
            let allowed = matches!(decision, Approval::AllowOnce | Approval::AllowAlways);
            if allowed {
                fs::write(cwd.join("a.txt"), b"edited\n")
                    .expect("fixture file edit should succeed");
                let mut store = lock(&store);
                store
                    .push(Message::Assistant {
                        header: Header::new(),
                        content: vec![
                            ContentBlock::Text {
                                text: "edited".into(),
                            },
                            ContentBlock::ToolUse {
                                id: TOOL_ID.into(),
                                name: "FileEdit".into(),
                                input: serde_json::json!({"path": "a.txt"}),
                            },
                        ],
                    })
                    .expect("fixture assistant message should be unique");
                store
                    .push(Message::User {
                        header: Header::new(),
                        content: vec![ContentBlock::ToolResult {
                            tool_use_id: TOOL_ID.into(),
                            content: ToolResultContent::Text("edited".into()),
                            is_error: false,
                        }],
                    })
                    .expect("fixture tool result should be unique");
            }
            Ok(Event::ToolResult {
                id: TOOL_ID.into(),
                ok: allowed,
                output: serde_json::json!({"edited": allowed}),
            })
        });
        let finish = stream::iter([
            Ok(Event::Usage {
                input_tokens: 12,
                output_tokens: 3,
                cache_read: 0,
                cache_create: 0,
            }),
            Ok(Event::Result {
                data: ResultData {
                    stop_reason: Some("end_turn".into()),
                    model: Some(MODEL.into()),
                    metadata: Default::default(),
                },
            }),
        ]);
        Box::pin(prelude.chain(approval).chain(finish))
    }

    async fn snapshot(&self) -> Result<SessionEngineSnapshot, EndpointError> {
        Ok(SessionEngineSnapshot {
            store: lock(&self.store).clone(),
            model: self.model.clone(),
            cwd: self.cwd.clone(),
            carry: CarryState::default(),
        })
    }

    async fn observe_event(&self, event: &Event) -> Option<UsageSnapshot> {
        matches!(event, Event::Usage { .. }).then_some(UsageSnapshot {
            input_tokens: 12,
            output_tokens: 3,
            context_used: Some(0.01),
            cost_usd: Some(0.0),
        })
    }

    fn finish_turn_usage(&self) {}

    fn steer(&self, _input: Vec<ContentBlock>) -> Result<(), EndpointError> {
        Err(endpoint_error(
            EndpointErrorKind::Unavailable,
            "fixture steering is unavailable",
        ))
    }
}

struct FixtureRoot {
    path: PathBuf,
    config_dir: PathBuf,
    workspace: PathBuf,
    label: String,
}

impl FixtureRoot {
    fn new(label: &str) -> Self {
        assert!(label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'));
        let unique = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "zode-app-end-to-end-{label}-{}-{unique}",
            std::process::id()
        ));
        let config_dir = path.join("config");
        let workspace = path.join("workspace");
        for directory in [
            config_dir.clone(),
            workspace.clone(),
            path.join("home"),
            path.join("xdg"),
        ] {
            fs::create_dir_all(directory).expect("fixture directory should be created");
        }
        ConfigManager::ensure_default_global_in(&config_dir)
            .expect("fixture ZODE_CONFIG_DIR should be initialized");
        initialize_git_repository(&workspace);
        Self {
            path,
            config_dir,
            workspace,
            label: label.into(),
        }
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn initialize_git_repository(workspace: &Path) {
    run_git(workspace, &["init", "--quiet"]);
    fs::write(workspace.join("a.txt"), b"original\n").expect("fixture seed file should be written");
    run_git(workspace, &["add", "a.txt"]);
    run_git(
        workspace,
        &[
            "-c",
            "user.name=Zode Fixture",
            "-c",
            "user.email=fixture@zode.local",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "--no-verify",
            "-m",
            "fixture baseline",
        ],
    );
}

fn run_git(workspace: &Path, arguments: &[&str]) {
    let root = workspace.parent().expect("fixture workspace has a parent");
    let global = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let output = Command::new("git")
        .arg("-c")
        .arg(format!(
            "core.hooksPath={}",
            root.join("disabled-hooks").display()
        ))
        .arg("-C")
        .arg(workspace)
        .args(arguments)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", global)
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env_remove("GIT_CONFIG_COUNT")
        .output()
        .expect("git should be available for the desktop end-to-end fixture");
    assert!(
        output.status.success(),
        "git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn agent_command(
    session: &SessionLocator,
    turn_id: Option<TurnId>,
    kind: AgentCommandKind,
) -> AgentCommand {
    AgentCommand {
        version: PROTOCOL_VERSION,
        session: session.clone(),
        turn_id,
        kind,
    }
}

fn event_kind(kind: &AgentEventKind) -> String {
    match kind {
        AgentEventKind::TextDelta { .. } => "text_delta".into(),
        AgentEventKind::ThinkingDelta { .. } => "thinking_delta".into(),
        AgentEventKind::ToolStarted { tool } => format!("tool_started:{}:{}", tool.name, tool.id),
        AgentEventKind::ToolCompleted { tool } => {
            format!("tool_completed:{}:{}:{:?}", tool.name, tool.id, tool.status)
        }
        AgentEventKind::ApprovalRequested { tool, .. } => format!("approval_requested:{tool}"),
        AgentEventKind::SubagentUpdate { subagent } => {
            format!(
                "subagent_update:{}:{:?}",
                subagent.agent_type, subagent.status
            )
        }
        AgentEventKind::BackgroundProcessUpdate { process } => {
            format!(
                "background_process_update:{}:{:?}",
                process.id, process.status
            )
        }
        AgentEventKind::DiffInvalidated => "diff_invalidated".into(),
        AgentEventKind::Usage { .. } => "usage".into(),
        AgentEventKind::StatusNotice { code, .. } => format!("status_notice:{code}"),
        AgentEventKind::TurnFinished { interrupted } => {
            format!("turn_finished:interrupted={interrupted}")
        }
        AgentEventKind::Error { retryable, .. } => format!("error:retryable={retryable}"),
        AgentEventKind::Unknown => "unknown".into(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn endpoint_error(kind: EndpointErrorKind, message: impl Into<String>) -> EndpointError {
    EndpointError {
        kind,
        message: message.into(),
    }
}
