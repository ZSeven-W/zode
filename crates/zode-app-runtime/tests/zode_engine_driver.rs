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
use zode_core::config::ZodeConfig;
use zode_core::engine::CarryState;
use zode_core::session_store::SessionWriteMode;
use zode_core::EngineTemplate;
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentQuery, AgentSnapshot, CapabilityManifest, DiffSnapshot,
    EndpointError, NodeCapability, NodeId, SessionLocator, TurnId, UsageSnapshot, UserContent,
    WorkspaceUri, PROTOCOL_VERSION,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssemblyRecord {
    session: SessionLocator,
    cwd: PathBuf,
    model: String,
    template_model: Option<String>,
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

fn template(cwd: &Path, model: &str) -> EngineTemplate {
    let mut config = ZodeConfig::default();
    config.provider.model = Some(model.to_owned());
    EngineTemplate::new(
        config,
        cwd.to_path_buf(),
        None,
        true,
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

#[tokio::test]
async fn create_is_lazy_and_first_turn_persists_snapshot_model_and_title() {
    let dir = TestDir::new("turn-persist");
    let node_id = NodeId::new();
    let session = session(node_id, "caller-id");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let engine = Arc::new(FakeSessionEngine::new(vec![
        Ok(Event::TextDelta {
            delta: "reply".into(),
        }),
        Ok(Event::Usage {
            input_tokens: 10,
            output_tokens: 3,
            cache_read: 0,
            cache_create: 0,
        }),
        Ok(Event::Result {
            data: agent::stream::ResultData {
                stop_reason: Some("end_turn".into()),
                model: Some("resolved-model".into()),
                metadata: Default::default(),
            },
        }),
    ]));
    let factory = Arc::new(FakeFactory::new(vec![engine.clone()]));
    let driver = ZodeEngineDriver::with_factory(
        node_id,
        template(dir.path(), "initial-model"),
        repository.clone(),
        manifest(node_id),
        factory.clone(),
    );
    let project = dir.path().join("project");

    driver
        .command(create_command(
            session.clone(),
            workspace(&project),
            "initial-model",
        ))
        .await
        .unwrap();
    assert!(factory.assemblies.lock().unwrap().is_empty());
    assert_eq!(
        repository.load(&session).await.unwrap().meta.id,
        "caller-id"
    );

    let turn_id = TurnId::new();
    let events = driver
        .start_turn(
            start_command(session.clone(), turn_id, "Design the desktop shell"),
            AbortController::new(),
        )
        .await;
    assert_eq!(factory.assemblies.lock().unwrap().len(), 1);
    let started = engine.started.lock().unwrap();
    assert!(
        matches!(&started[0][0], ContentBlock::Text { text } if text == "Design the desktop shell")
    );
    assert!(matches!(
        &started[0][1],
        ContentBlock::Image { source: ImageSource::Base64 { media_type, data } }
            if media_type == "image/png" && data == "aGVsbG8="
    ));
    drop(started);

    let raw = collect_stream(events).await;
    let usage_event = raw
        .iter()
        .find(|event| matches!(event, Event::Usage { .. }))
        .unwrap();
    let cumulative = driver
        .observe_event(&session, turn_id, usage_event)
        .await
        .unwrap();
    assert_eq!(cumulative, engine.cumulative_usage);
    driver.finish_turn_usage(&session, turn_id);
    assert_eq!(engine.finish_usage_calls.load(Ordering::SeqCst), 1);

    driver
        .finish_turn(&session, turn_id, Some("resolved-model".into()), false)
        .await
        .unwrap();
    let loaded = repository.load(&session).await.unwrap();
    assert_eq!(loaded.meta.title, "Design the desktop shell");
    assert_eq!(loaded.meta.model, "resolved-model");
    assert_eq!(loaded.meta.cwd, project.to_string_lossy());
    assert_eq!(loaded.store.len(), 2);
}

#[tokio::test]
async fn restart_lazily_restores_the_persisted_transcript() {
    let dir = TestDir::new("restart");
    let node_id = NodeId::new();
    let session = session(node_id, "restart-id");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let loaded = repository
        .create(
            &session,
            &workspace(&dir.path().join("project")),
            "restored-model".into(),
        )
        .await
        .unwrap();
    repository
        .save(
            &session,
            loaded.meta,
            store_with_exchange("persisted prompt"),
            SessionWriteMode::Full,
        )
        .await
        .unwrap();

    let engine = Arc::new(FakeSessionEngine::new(Vec::new()));
    let factory = Arc::new(FakeFactory::new(vec![engine]));
    let restarted = ZodeEngineDriver::with_factory(
        node_id,
        template(dir.path(), "fallback-model"),
        repository,
        manifest(node_id),
        factory.clone(),
    );

    let _events = restarted
        .start_turn(
            start_command(session.clone(), TurnId::new(), "continue"),
            AbortController::new(),
        )
        .await;
    let assemblies = factory.assemblies.lock().unwrap();
    assert_eq!(assemblies.len(), 1);
    assert_eq!(assemblies[0].session, session);
    assert_eq!(assemblies[0].prior_messages, 2);
    assert_eq!(assemblies[0].model, "restored-model");
    assert_eq!(
        assemblies[0].template_model.as_deref(),
        Some("restored-model")
    );
    assert!(!assemblies[0].carried);
}

#[tokio::test]
async fn idle_model_switch_reassembles_with_carry_and_preserves_transcript() {
    let dir = TestDir::new("model-switch");
    let node_id = NodeId::new();
    let session = session(node_id, "model-id");
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let first = Arc::new(FakeSessionEngine::new(Vec::new()));
    let replacement = Arc::new(FakeSessionEngine::new(Vec::new()));
    let factory = Arc::new(FakeFactory::new(vec![first, replacement]));
    let driver = ZodeEngineDriver::with_factory(
        node_id,
        template(dir.path(), "old-model"),
        repository.clone(),
        manifest(node_id),
        factory.clone(),
    );
    driver
        .command(create_command(
            session.clone(),
            workspace(&dir.path().join("project")),
            "old-model",
        ))
        .await
        .unwrap();
    let turn_id = TurnId::new();
    let events = driver
        .start_turn(
            start_command(session.clone(), turn_id, "keep this transcript"),
            AbortController::new(),
        )
        .await;
    collect_stream(events).await;
    driver
        .finish_turn(&session, turn_id, None, false)
        .await
        .unwrap();

    driver
        .command(command(
            session.clone(),
            None,
            AgentCommandKind::SetModel {
                model: "new-model".into(),
            },
        ))
        .await
        .unwrap();

    let assemblies = factory.assemblies.lock().unwrap();
    assert_eq!(assemblies.len(), 2);
    assert_eq!(assemblies[1].model, "new-model");
    assert_eq!(assemblies[1].template_model.as_deref(), Some("new-model"));
    assert_eq!(assemblies[1].prior_messages, 2);
    assert!(assemblies[1].carried);
    drop(assemblies);
    let loaded = repository.load(&session).await.unwrap();
    assert_eq!(loaded.meta.model, "new-model");
    assert_eq!(loaded.store.len(), 2);
}

#[tokio::test]
async fn steer_and_all_query_shapes_delegate_to_stable_sources() {
    let dir = TestDir::new("queries");
    let node_id = NodeId::new();
    let session = session(node_id, "query-id");
    let capabilities = manifest(node_id);
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let engine = Arc::new(FakeSessionEngine::new(Vec::new()));
    let factory = Arc::new(FakeFactory::new(vec![engine.clone()]));
    let driver = ZodeEngineDriver::with_factory(
        node_id,
        template(dir.path(), "query-model"),
        repository,
        capabilities.clone(),
        factory,
    );
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let workspace_uri = workspace(&project);
    driver
        .command(create_command(
            session.clone(),
            workspace_uri.clone(),
            "query-model",
        ))
        .await
        .unwrap();
    let turn_id = TurnId::new();
    let _events = driver
        .start_turn(
            start_command(session.clone(), turn_id, "query state"),
            AbortController::new(),
        )
        .await;
    driver
        .command(command(
            session.clone(),
            Some(turn_id),
            AgentCommandKind::SteerTurn {
                input: vec![UserContent::Text {
                    text: "steer now".into(),
                }],
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        &engine.steered.lock().unwrap()[0][0],
        ContentBlock::Text { text } if text == "steer now"
    ));

    assert_eq!(
        driver.query(AgentQuery::Capabilities).await.unwrap(),
        AgentSnapshot::Capabilities(capabilities)
    );
    let AgentSnapshot::Threads(threads) = driver.query(AgentQuery::Threads).await.unwrap() else {
        panic!("expected thread snapshot");
    };
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].session, session);
    let AgentSnapshot::RuntimeOptions(options) =
        driver.query(AgentQuery::RuntimeOptions).await.unwrap()
    else {
        panic!("expected runtime options");
    };
    assert_eq!(options.active_model.as_deref(), Some("query-model"));
    assert!(options.models.contains(&"query-model".to_string()));
    let AgentSnapshot::Diff(DiffSnapshot {
        session: diff_session,
        files,
        unified,
    }) = driver
        .query(AgentQuery::Diff {
            session: session.clone(),
        })
        .await
        .unwrap()
    else {
        panic!("expected diff snapshot");
    };
    assert_eq!(diff_session, session);
    assert!(files.is_empty());
    assert!(unified.contains("working tree clean"));
    assert_eq!(
        driver
            .query(AgentQuery::ProjectPermissions { workspace_uri })
            .await
            .unwrap(),
        AgentSnapshot::ProjectPermissions(Vec::new())
    );
}
