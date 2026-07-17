use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use agent::abort::AbortController;
use agent::message::{ContentBlock, Header, ImageSource, Message, MessageStore};
use agent::stream::Event;
use async_trait::async_trait;
use futures::{stream, StreamExt};
use zode_app_runtime::{
    DriverEventStream, EngineDriver, LoadedSession, LocalSessionRepository, SessionEngine,
    SessionEngineFactory, SessionEngineSnapshot, ZodeEngineDriver,
};
use zode_core::config::{ConfigManager, ProviderConfig, ProviderKind, ZodeConfig};
use zode_core::engine::CarryState;
use zode_app_runtime::session_store::SessionWriteMode;
use zode_core::EngineTemplate;
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentQuery, AgentSnapshot, ApprovalMode, CapabilityManifest,
    DiffSnapshot, EndpointError, EndpointErrorKind, NodeCapability, NodeId, RuntimeOptions,
    SandboxMode, SessionLocator, TurnId, UsageSnapshot, UserContent, WorkspaceUri,
    PROTOCOL_VERSION,
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let suffix = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "zode-engine-driver-{label}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(value) = self.previous.take() {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssemblyRecord {
    session: SessionLocator,
    cwd: PathBuf,
    model: String,
    template_model: Option<String>,
    provider_names: Vec<String>,
    active_provider_name: Option<String>,
    sandbox_mode: SandboxMode,
    sandbox_network: bool,
    allowed_tools: Vec<String>,
    prior_messages: usize,
    carried: bool,
}

struct FakeSessionEngine {
    stream: Mutex<Option<DriverEventStream>>,
    store: Mutex<MessageStore>,
    model: Mutex<String>,
    cwd: Mutex<PathBuf>,
    started: Mutex<Vec<Vec<ContentBlock>>>,
    observed: Mutex<Vec<Event>>,
    steered: Mutex<Vec<Vec<ContentBlock>>>,
    cumulative_usage: UsageSnapshot,
    finish_usage_calls: AtomicUsize,
}

impl FakeSessionEngine {
    fn new(events: Vec<Result<Event, agent::error::AgentError>>) -> Self {
        Self {
            stream: Mutex::new(Some(Box::pin(stream::iter(events)))),
            store: Mutex::new(MessageStore::new()),
            model: Mutex::new(String::new()),
            cwd: Mutex::new(PathBuf::new()),
            started: Mutex::new(Vec::new()),
            observed: Mutex::new(Vec::new()),
            steered: Mutex::new(Vec::new()),
            cumulative_usage: UsageSnapshot {
                input_tokens: 110,
                output_tokens: 23,
                context_used: Some(0.5),
                cost_usd: Some(0.012),
            },
            finish_usage_calls: AtomicUsize::new(0),
        }
    }

    fn install(&self, store: MessageStore, model: String, cwd: PathBuf) {
        *self.store.lock().unwrap() = store;
        *self.model.lock().unwrap() = model;
        *self.cwd.lock().unwrap() = cwd;
    }
}

#[async_trait]
impl SessionEngine for FakeSessionEngine {
    async fn start_turn(
        &self,
        input: Vec<ContentBlock>,
        _abort: AbortController,
    ) -> DriverEventStream {
        self.started.lock().unwrap().push(input.clone());
        let mut store = self.store.lock().unwrap();
        store
            .push(Message::User {
                header: Header::new(),
                content: input,
            })
            .unwrap();
        store
            .push(Message::Assistant {
                header: Header::new(),
                content: vec![ContentBlock::Text {
                    text: "snapshot reply".into(),
                }],
            })
            .unwrap();
        drop(store);
        self.stream
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Box::pin(stream::empty()))
    }

    async fn snapshot(&self) -> Result<SessionEngineSnapshot, EndpointError> {
        Ok(SessionEngineSnapshot {
            store: self.store.lock().unwrap().clone(),
            model: self.model.lock().unwrap().clone(),
            cwd: self.cwd.lock().unwrap().clone(),
            carry: CarryState::default(),
        })
    }

    async fn observe_event(&self, event: &Event) -> Option<UsageSnapshot> {
        self.observed.lock().unwrap().push(event.clone());
        matches!(event, Event::Usage { .. }).then(|| self.cumulative_usage.clone())
    }

    fn finish_turn_usage(&self) {
        self.finish_usage_calls.fetch_add(1, Ordering::SeqCst);
    }

    fn steer(&self, input: Vec<ContentBlock>) -> Result<(), EndpointError> {
        self.steered.lock().unwrap().push(input);
        Ok(())
    }
}

struct FakeFactory {
    engines: Mutex<VecDeque<Arc<FakeSessionEngine>>>,
    assemblies: Mutex<Vec<AssemblyRecord>>,
}

impl FakeFactory {
    fn new(engines: Vec<Arc<FakeSessionEngine>>) -> Self {
        Self {
            engines: Mutex::new(engines.into()),
            assemblies: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl SessionEngineFactory for FakeFactory {
    async fn assemble(
        &self,
        template: &EngineTemplate,
        session: &SessionLocator,
        loaded: LoadedSession,
        carry: Option<CarryState>,
    ) -> Result<Arc<dyn SessionEngine>, EndpointError> {
        let engine = self
            .engines
            .lock()
            .unwrap()
            .pop_front()
            .expect("a fake engine was prepared for every assembly");
        self.assemblies.lock().unwrap().push(AssemblyRecord {
            session: session.clone(),
            cwd: PathBuf::from(&loaded.meta.cwd),
            model: loaded.meta.model.clone(),
            template_model: template.model().map(str::to_owned),
            provider_names: template.provider_names(),
            active_provider_name: template.active_provider_name(),
            sandbox_mode: template
                .sandbox()
                .map(|sandbox| match sandbox.mode() {
                    zode_core::sandbox::SandboxMode::ReadOnly => SandboxMode::ReadOnly,
                    zode_core::sandbox::SandboxMode::WorkspaceWrite => SandboxMode::WorkspaceWrite,
                })
                .unwrap_or(SandboxMode::Off),
            sandbox_network: template
                .sandbox()
                .is_some_and(zode_core::sandbox::SandboxConfig::allow_network),
            allowed_tools: template.permissions().allow.clone(),
            prior_messages: loaded.store.len(),
            carried: carry.is_some(),
        });
        engine.install(
            loaded.store,
            loaded.meta.model,
            PathBuf::from(loaded.meta.cwd),
        );
        Ok(engine)
    }
}

struct FailOnSecondAssemblyFactory {
    first: Arc<FakeSessionEngine>,
    calls: AtomicUsize,
}

struct ValidateProviderOnReloadFactory {
    inner: FakeFactory,
    calls: AtomicUsize,
}

#[async_trait]
impl SessionEngineFactory for ValidateProviderOnReloadFactory {
    async fn assemble(
        &self,
        template: &EngineTemplate,
        session: &SessionLocator,
        loaded: LoadedSession,
        carry: Option<CarryState>,
    ) -> Result<Arc<dyn SessionEngine>, EndpointError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
            template
                .assemble_tab(
                    Some(PathBuf::from(&loaded.meta.cwd)),
                    Some(session.session_id.clone()),
                )
                .await
                .map_err(|error| EndpointError {
                    kind: EndpointErrorKind::Internal,
                    message: error.to_string(),
                })?;
        }
        self.inner.assemble(template, session, loaded, carry).await
    }
}

struct FailAtAssemblyFactory {
    engines: Mutex<VecDeque<Arc<FakeSessionEngine>>>,
    calls: AtomicUsize,
    fail_at: usize,
}

#[async_trait]
impl SessionEngineFactory for FailAtAssemblyFactory {
    async fn assemble(
        &self,
        _template: &EngineTemplate,
        _session: &SessionLocator,
        loaded: LoadedSession,
        _carry: Option<CarryState>,
    ) -> Result<Arc<dyn SessionEngine>, EndpointError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == self.fail_at {
            return Err(EndpointError {
                kind: EndpointErrorKind::Internal,
                message: "staged provider assembly failed".into(),
            });
        }
        let engine = self
            .engines
            .lock()
            .unwrap()
            .pop_front()
            .expect("a fake engine was prepared for every successful assembly");
        engine.install(
            loaded.store,
            loaded.meta.model,
            PathBuf::from(loaded.meta.cwd),
        );
        Ok(engine)
    }
}

#[async_trait]
impl SessionEngineFactory for FailOnSecondAssemblyFactory {
    async fn assemble(
        &self,
        _template: &EngineTemplate,
        _session: &SessionLocator,
        loaded: LoadedSession,
        _carry: Option<CarryState>,
    ) -> Result<Arc<dyn SessionEngine>, EndpointError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
            return Err(EndpointError {
                kind: EndpointErrorKind::Internal,
                message: "replacement assembly failed".into(),
            });
        }
        self.first.install(
            loaded.store,
            loaded.meta.model,
            PathBuf::from(loaded.meta.cwd),
        );
        Ok(self.first.clone())
    }
}

fn assert_runtime_options(
    snapshot: AgentSnapshot,
    expected_session: &SessionLocator,
) -> RuntimeOptions {
    let AgentSnapshot::SessionRuntimeOptions { session, options } = snapshot else {
        panic!("expected session runtime options");
    };
    assert_eq!(&session, expected_session);
    options
}

fn template(cwd: &Path, model: &str) -> EngineTemplate {
    let mut config = ZodeConfig::default();
    config.provider.model = Some(model.to_owned());
    EngineTemplate::new(
        config,
        cwd.to_path_buf(),
        None,
        false,
        None,
        "2026-07-11".into(),
    )
}

fn manifest(node_id: NodeId) -> CapabilityManifest {
    CapabilityManifest {
        node_id,
        capabilities: BTreeSet::from([
            NodeCapability::Agent,
            NodeCapability::Workspace,
            NodeCapability::FileSystem,
        ]),
    }
}

fn session(node_id: NodeId, id: &str) -> SessionLocator {
    SessionLocator::new(node_id, id)
}

fn workspace(path: &Path) -> WorkspaceUri {
    zode_app_runtime::path_to_workspace_uri(path).unwrap()
}

fn command(
    session: SessionLocator,
    turn_id: Option<TurnId>,
    kind: AgentCommandKind,
) -> AgentCommand {
    AgentCommand {
        version: PROTOCOL_VERSION,
        session,
        turn_id,
        kind,
    }
}

fn create_command(
    session: SessionLocator,
    workspace_uri: WorkspaceUri,
    model: &str,
) -> AgentCommand {
    command(
        session,
        None,
        AgentCommandKind::CreateSession {
            workspace_uri,
            project_uri: None,
            projectless: false,
            model: Some(model.into()),
        },
    )
}

fn start_command(session: SessionLocator, turn_id: TurnId, text: &str) -> AgentCommand {
    command(
        session,
        Some(turn_id),
        AgentCommandKind::StartTurn {
            input: vec![
                UserContent::Text { text: text.into() },
                UserContent::Image {
                    mime_type: "image/png".into(),
                    data_base64: "aGVsbG8=".into(),
                    display_name: "reference.png".into(),
                },
            ],
        },
    )
}

fn store_with_exchange(prompt: &str) -> MessageStore {
    let mut store = MessageStore::new();
    store
        .push(Message::User {
            header: Header::new(),
            content: vec![ContentBlock::Text {
                text: prompt.into(),
            }],
        })
        .unwrap();
    store
        .push(Message::Assistant {
            header: Header::new(),
            content: vec![ContentBlock::Text {
                text: "persisted reply".into(),
            }],
        })
        .unwrap();
    store
}

async fn collect_stream(mut events: DriverEventStream) -> Vec<Event> {
    let mut collected = Vec::new();
    while let Some(event) = events.next().await {
        collected.push(event.unwrap());
    }
    collected
}
