//! Concrete session driver backed by `zode_core::ZodeEngine`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use agent::abort::AbortController;
use agent::message::{ContentBlock, ImageSource, MessageStore};
use agent::stream::Event;
use async_trait::async_trait;
use futures::stream;
use zode_core::config::ConfigManager;
use zode_core::engine::CarryState;
use zode_core::sandbox::{SandboxConfig, SandboxMode as CoreSandboxMode};
use zode_core::session_meta::title_from_prompt;
use zode_core::session_store::{SessionSaveOutcome, SessionWriteMode};
use zode_core::{EngineTemplate, ZodeEngine};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentQuery, AgentSnapshot, CapabilityManifest, DiffFile,
    DiffFileStatus, DiffSnapshot, EndpointError, EndpointErrorKind, NodeId, RuntimeOptions,
    SandboxMode, SessionLocator, TurnId, UsageSnapshot, UserContent,
};

use crate::{
    runtime_policy, workspace_uri_to_path, DriverEventStream, EngineDriver, LoadedSession,
    LocalSessionRepository,
};

/// Serializable pieces needed to persist or rebuild one session engine.
pub struct SessionEngineSnapshot {
    pub store: MessageStore,
    pub model: String,
    pub cwd: PathBuf,
    pub carry: CarryState,
}

/// Small testable boundary around the concrete `ZodeEngine` surface.
#[async_trait]
pub trait SessionEngine: Send + Sync + 'static {
    async fn start_turn(
        &self,
        input: Vec<ContentBlock>,
        abort: AbortController,
    ) -> DriverEventStream;

    async fn snapshot(&self) -> Result<SessionEngineSnapshot, EndpointError>;

    async fn observe_event(&self, event: &Event) -> Option<UsageSnapshot>;

    fn finish_turn_usage(&self);

    async fn flush_turn_usage(&self) {}

    fn steer(&self, input: Vec<ContentBlock>) -> Result<(), EndpointError>;
}

/// Creates real or fake session engines while keeping driver lifecycle tests
/// independent of provider construction.
#[async_trait]
pub trait SessionEngineFactory: Send + Sync + 'static {
    async fn assemble(
        &self,
        template: &EngineTemplate,
        session: &SessionLocator,
        loaded: LoadedSession,
        carry: Option<CarryState>,
    ) -> Result<Arc<dyn SessionEngine>, EndpointError>;
}

#[derive(Default)]
pub struct ZodeSessionEngineFactory;

#[async_trait]
impl SessionEngineFactory for ZodeSessionEngineFactory {
    async fn assemble(
        &self,
        template: &EngineTemplate,
        session: &SessionLocator,
        loaded: LoadedSession,
        carry: Option<CarryState>,
    ) -> Result<Arc<dyn SessionEngine>, EndpointError> {
        let cwd = PathBuf::from(&loaded.meta.cwd);
        let template = template.with_model(loaded.meta.model);
        let engine = template
            .assemble_tab_with_carry(
                Some(cwd),
                Some(session.session_id.clone()),
                carry.unwrap_or_default(),
            )
            .await
            .map_err(map_internal)?
            .with_store(loaded.store);
        Ok(Arc::new(RealSessionEngine::new(engine)))
    }
}

struct RealSessionEngine {
    engine: Arc<ZodeEngine>,
    usage_boundary_pending: AtomicBool,
}

impl RealSessionEngine {
    fn new(engine: ZodeEngine) -> Self {
        Self {
            engine: Arc::new(engine),
            usage_boundary_pending: AtomicBool::new(false),
        }
    }

    async fn clear_pending_usage_boundary(&self) {
        if self.usage_boundary_pending.swap(false, Ordering::AcqRel) {
            self.engine.cost.finish_turn_usage().await;
        }
    }
}

#[async_trait]
impl SessionEngine for RealSessionEngine {
    async fn start_turn(
        &self,
        input: Vec<ContentBlock>,
        abort: AbortController,
    ) -> DriverEventStream {
        self.clear_pending_usage_boundary().await;
        match self.engine.turn_blocks(input, abort).await {
            Ok(events) => Box::pin(events),
            Err(error) => Box::pin(stream::once(async move { Err(error) })),
        }
    }

    async fn snapshot(&self) -> Result<SessionEngineSnapshot, EndpointError> {
        let store = self
            .engine
            .store
            .lock()
            .map_err(|_| internal("session transcript lock is unavailable"))?
            .clone();
        Ok(SessionEngineSnapshot {
            store,
            model: self.engine.model.clone(),
            cwd: self.engine.cwd.clone(),
            carry: self.engine.carry_state(),
        })
    }

    async fn observe_event(&self, event: &Event) -> Option<UsageSnapshot> {
        self.clear_pending_usage_boundary().await;
        self.engine.cost.observe(event).await;
        if !matches!(event, Event::Usage { .. }) {
            return None;
        }
        let totals = self.engine.cost.usage_totals().await;
        let context_used = self
            .engine
            .cost
            .last_prompt_tokens()
            .await
            .filter(|_| self.engine.model_max_tokens > 0)
            .map(|tokens| (tokens as f32 / self.engine.model_max_tokens as f32).clamp(0.0, 1.0));
        Some(UsageSnapshot {
            input_tokens: totals.input_tokens,
            output_tokens: totals.output_tokens,
            context_used,
            cost_usd: totals.cost_usd,
        })
    }

    fn finish_turn_usage(&self) {
        self.usage_boundary_pending.store(true, Ordering::Release);
    }

    async fn flush_turn_usage(&self) {
        self.clear_pending_usage_boundary().await;
    }

    fn steer(&self, input: Vec<ContentBlock>) -> Result<(), EndpointError> {
        self.engine
            .steer(input)
            .then_some(())
            .ok_or_else(|| unavailable("session steering channel is unavailable"))
    }
}

struct RuntimeSession {
    engine: Arc<dyn SessionEngine>,
    template: EngineTemplate,
    persisted_messages: usize,
}

#[derive(Default)]
struct SessionSlot {
    runtime: Mutex<Option<RuntimeSession>>,
    mutation: tokio::sync::Mutex<()>,
}

/// Owns lazy, isolated `ZodeEngine` instances for every local session.
pub struct ZodeEngineDriver {
    node_id: NodeId,
    template: EngineTemplate,
    config_dir: Option<PathBuf>,
    repository: LocalSessionRepository,
    capabilities: CapabilityManifest,
    factory: Arc<dyn SessionEngineFactory>,
    sessions: Mutex<HashMap<SessionLocator, Arc<SessionSlot>>>,
}

impl ZodeEngineDriver {
    pub fn new(
        node_id: NodeId,
        template: EngineTemplate,
        repository: LocalSessionRepository,
        capabilities: CapabilityManifest,
        config_dir: PathBuf,
    ) -> Self {
        Self::with_factory_and_config_dir(
            node_id,
            template,
            repository,
            capabilities,
            Arc::new(ZodeSessionEngineFactory),
            config_dir,
        )
    }

    pub fn with_factory(
        node_id: NodeId,
        template: EngineTemplate,
        repository: LocalSessionRepository,
        capabilities: CapabilityManifest,
        factory: Arc<dyn SessionEngineFactory>,
    ) -> Self {
        Self {
            node_id,
            template,
            config_dir: None,
            repository,
            capabilities,
            factory,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_factory_and_config_dir(
        node_id: NodeId,
        template: EngineTemplate,
        repository: LocalSessionRepository,
        capabilities: CapabilityManifest,
        factory: Arc<dyn SessionEngineFactory>,
        config_dir: PathBuf,
    ) -> Self {
        Self {
            node_id,
            template,
            config_dir: Some(config_dir),
            repository,
            capabilities,
            factory,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn ensure_local(&self, session: &SessionLocator) -> Result<(), EndpointError> {
        if session.node_id == self.node_id {
            Ok(())
        } else {
            Err(denied("session is not owned by the local node"))
        }
    }

    fn slot(&self, session: &SessionLocator) -> Arc<SessionSlot> {
        lock(&self.sessions)
            .entry(session.clone())
            .or_insert_with(|| Arc::new(SessionSlot::default()))
            .clone()
    }

    fn existing_slot(&self, session: &SessionLocator) -> Option<Arc<SessionSlot>> {
        lock(&self.sessions).get(session).cloned()
    }

    fn apply_workspace_policy(
        &self,
        template: &EngineTemplate,
        cwd: &Path,
    ) -> Result<EngineTemplate, EndpointError> {
        let Some(config_dir) = self.config_dir.as_deref() else {
            return Ok(template.clone());
        };
        runtime_policy::apply_workspace_policy(template, cwd, config_dir).map_err(map_internal)
    }

    fn session_template(&self, cwd: &Path, model: String) -> Result<EngineTemplate, EndpointError> {
        self.apply_workspace_policy(&self.template.with_model(model), cwd)
    }

    async fn ensure_engine(
        &self,
        session: &SessionLocator,
    ) -> Result<Arc<dyn SessionEngine>, EndpointError> {
        self.ensure_local(session)?;
        let slot = self.slot(session);
        if let Some(engine) = lock(&slot.runtime)
            .as_ref()
            .map(|runtime| runtime.engine.clone())
        {
            return Ok(engine);
        }

        let _mutation = slot.mutation.lock().await;
        if let Some(engine) = lock(&slot.runtime)
            .as_ref()
            .map(|runtime| runtime.engine.clone())
        {
            return Ok(engine);
        }
        let loaded = self.repository.load(session).await?;
        let persisted_messages = loaded.store.len();
        let template =
            self.session_template(Path::new(&loaded.meta.cwd), loaded.meta.model.clone())?;
        let engine = self
            .factory
            .assemble(&template, session, loaded, None)
            .await?;
        *lock(&slot.runtime) = Some(RuntimeSession {
            engine: engine.clone(),
            template,
            persisted_messages,
        });
        Ok(engine)
    }

    async fn reassemble(
        &self,
        session: &SessionLocator,
        update: impl FnOnce(&EngineTemplate, &Path) -> Result<EngineTemplate, EndpointError>,
    ) -> Result<(), EndpointError> {
        self.reassemble_with_commit(session, update, |_| Ok(()))
            .await
    }

    async fn reassemble_with_commit(
        &self,
        session: &SessionLocator,
        update: impl FnOnce(&EngineTemplate, &Path) -> Result<EngineTemplate, EndpointError>,
        commit: impl FnOnce(&Path) -> Result<(), EndpointError>,
    ) -> Result<(), EndpointError> {
        self.ensure_local(session)?;
        let Some(slot) = self.existing_slot(session) else {
            return Ok(());
        };
        let _mutation = slot.mutation.lock().await;
        let Some((old_engine, old_template, persisted_messages)) =
            lock(&slot.runtime).as_ref().map(|runtime| {
                (
                    runtime.engine.clone(),
                    runtime.template.clone(),
                    runtime.persisted_messages,
                )
            })
        else {
            return Ok(());
        };
        let snapshot = old_engine.snapshot().await?;
        let template = update(&old_template, &snapshot.cwd)?;
        let mut loaded = self.repository.load(session).await?;
        loaded.store = snapshot.store;
        loaded.meta.model = template.model().unwrap_or(&snapshot.model).to_string();
        let engine = self
            .factory
            .assemble(&template, session, loaded, Some(snapshot.carry))
            .await?;
        commit(&snapshot.cwd)?;
        *lock(&slot.runtime) = Some(RuntimeSession {
            engine,
            template,
            persisted_messages,
        });
        Ok(())
    }

    fn runtime_engine(&self, session: &SessionLocator) -> Option<Arc<dyn SessionEngine>> {
        self.existing_slot(session).and_then(|slot| {
            lock(&slot.runtime)
                .as_ref()
                .map(|runtime| runtime.engine.clone())
        })
    }

    async fn set_model(
        &self,
        session: &SessionLocator,
        model: String,
    ) -> Result<(), EndpointError> {
        self.repository.update_model(session, model.clone()).await?;
        self.reassemble(session, move |template, _| Ok(template.with_model(model)))
            .await
    }

    async fn set_effort(
        &self,
        session: &SessionLocator,
        effort: String,
    ) -> Result<(), EndpointError> {
        self.ensure_engine(session).await?;
        self.reassemble(session, move |template, _| {
            Ok(template.with_effort(Some(effort)))
        })
        .await
    }

    async fn set_sandbox(
        &self,
        session: &SessionLocator,
        mode: SandboxMode,
        network: bool,
    ) -> Result<(), EndpointError> {
        self.ensure_engine(session).await?;
        self.reassemble_with_commit(
            session,
            move |template, cwd| {
                let sandbox = match mode {
                    SandboxMode::Off => None,
                    SandboxMode::ReadOnly | SandboxMode::WorkspaceWrite => {
                        let core_mode = match mode {
                            SandboxMode::ReadOnly => CoreSandboxMode::ReadOnly,
                            _ => CoreSandboxMode::WorkspaceWrite,
                        };
                        Some(
                            template
                                .sandbox()
                                .cloned()
                                .map(|sandbox| sandbox.with_mode(core_mode).with_network(network))
                                .map(Ok)
                                .unwrap_or_else(|| SandboxConfig::new(cwd, core_mode, network, &[]))
                                .map_err(map_internal)?,
                        )
                    }
                };
                Ok(template.with_sandbox(sandbox))
            },
            move |cwd| {
                ConfigManager::update_project_state(cwd, |state| {
                    state.insert(
                        "sandbox".to_string(),
                        serde_json::json!({
                            "enabled": mode != SandboxMode::Off,
                            "mode": match mode {
                                SandboxMode::ReadOnly => Some("read-only"),
                                SandboxMode::WorkspaceWrite => Some("workspace-write"),
                                SandboxMode::Off => None,
                            },
                            "network": (mode != SandboxMode::Off).then_some(network),
                        }),
                    );
                })
                .map_err(map_internal)
            },
        )
        .await
    }

    async fn revoke_project_permission(
        &self,
        session: &SessionLocator,
        workspace_uri: &zode_node_protocol::WorkspaceUri,
        tool: &str,
    ) -> Result<(), EndpointError> {
        let cwd = workspace_uri_to_path(workspace_uri)?;
        ConfigManager::revoke_project_tool(&cwd, tool).map_err(map_internal)?;
        if self.runtime_engine(session).is_some() {
            self.reassemble(session, |template, cwd| {
                self.apply_workspace_policy(template, cwd)
            })
            .await?;
        }
        Ok(())
    }

    async fn diff(&self, session: SessionLocator) -> Result<DiffSnapshot, EndpointError> {
        self.ensure_local(&session)?;
        let cwd = self
            .runtime_engine(&session)
            .map(|engine| async move { engine.snapshot().await.map(|value| value.cwd) });
        let cwd = match cwd {
            Some(cwd) => cwd.await?,
            None => PathBuf::from(self.repository.load(&session).await?.meta.cwd),
        };
        let core = zode_core::diff::diff_snapshot(&cwd)
            .await
            .map_err(map_internal)?;
        let files = core
            .files
            .into_iter()
            .map(|file| DiffFile {
                status: match file.status {
                    zode_core::diff::CoreDiffFileStatus::Added => DiffFileStatus::Added,
                    zode_core::diff::CoreDiffFileStatus::Modified => DiffFileStatus::Modified,
                    zode_core::diff::CoreDiffFileStatus::Deleted => DiffFileStatus::Deleted,
                    zode_core::diff::CoreDiffFileStatus::Renamed => DiffFileStatus::Renamed,
                    zode_core::diff::CoreDiffFileStatus::Untracked => DiffFileStatus::Untracked,
                },
                path: file.path,
                additions: file.additions,
                deletions: file.deletions,
            })
            .collect();
        Ok(DiffSnapshot {
            session,
            files,
            unified: core.unified,
        })
    }

    fn runtime_options(&self) -> RuntimeOptions {
        runtime_policy::runtime_options(&self.template)
    }

    async fn session_runtime_options(
        &self,
        session: &SessionLocator,
    ) -> Result<RuntimeOptions, EndpointError> {
        self.ensure_local(session)?;
        if let Some(slot) = self.existing_slot(session) {
            if let Some(options) = lock(&slot.runtime)
                .as_ref()
                .map(|runtime| runtime_policy::runtime_options(&runtime.template))
            {
                return Ok(options);
            }
        }
        let loaded = self.repository.load(session).await?;
        let template = self.session_template(Path::new(&loaded.meta.cwd), loaded.meta.model)?;
        Ok(runtime_policy::runtime_options(&template))
    }
}

#[async_trait]
impl EngineDriver for ZodeEngineDriver {
    async fn command(&self, command: AgentCommand) -> Result<(), EndpointError> {
        self.ensure_local(&command.session)?;
        match command.kind {
            AgentCommandKind::CreateSession {
                workspace_uri,
                model,
            } => {
                let model = model
                    .or_else(|| self.template.model().map(str::to_string))
                    .unwrap_or_else(|| "unconfigured".to_string());
                self.repository
                    .create(&command.session, &workspace_uri, model)
                    .await?;
                Ok(())
            }
            AgentCommandKind::RenameSession { title } => {
                self.repository.rename(&command.session, title).await
            }
            AgentCommandKind::DeleteSession => {
                lock(&self.sessions).remove(&command.session);
                self.repository.delete(&command.session).await
            }
            AgentCommandKind::SteerTurn { input } => self
                .runtime_engine(&command.session)
                .ok_or_else(|| not_found("session engine is not loaded"))?
                .steer(convert_content(input)),
            AgentCommandKind::RevokeProjectPermission {
                workspace_uri,
                tool,
            } => {
                self.revoke_project_permission(&command.session, &workspace_uri, &tool)
                    .await
            }
            AgentCommandKind::SetModel { model } => self.set_model(&command.session, model).await,
            AgentCommandKind::SetEffort { effort } => {
                self.set_effort(&command.session, effort).await
            }
            AgentCommandKind::SetSandbox { mode, network } => {
                self.set_sandbox(&command.session, mode, network).await
            }
            AgentCommandKind::Approve { .. }
            | AgentCommandKind::StartTurn { .. }
            | AgentCommandKind::InterruptTurn => Err(invalid(
                "command must be coordinated by the local node runtime",
            )),
        }
    }

    async fn start_turn(&self, command: AgentCommand, abort: AbortController) -> DriverEventStream {
        let session = command.session.clone();
        let AgentCommandKind::StartTurn { input } = command.kind else {
            return endpoint_error_stream(invalid("start_turn requires StartTurn input"));
        };
        let first_text = input.iter().find_map(|content| match content {
            UserContent::Text { text } => Some(text.clone()),
            UserContent::Image { .. } => None,
        });
        let engine = match self.ensure_engine(&session).await {
            Ok(engine) => engine,
            Err(error) => return endpoint_error_stream(error),
        };
        if let (Some(prompt), Ok(loaded)) = (first_text, self.repository.load(&session).await) {
            if loaded.meta.title == "(untitled)" {
                if let Err(error) = self
                    .repository
                    .rename(&session, title_from_prompt(&prompt))
                    .await
                {
                    return endpoint_error_stream(error);
                }
            }
        }
        engine.start_turn(convert_content(input), abort).await
    }

    async fn finish_turn(
        &self,
        session: &SessionLocator,
        _turn_id: TurnId,
        model: Option<String>,
        _interrupted: bool,
    ) -> Result<(), EndpointError> {
        let slot = self
            .existing_slot(session)
            .ok_or_else(|| not_found("session engine is not loaded"))?;
        let (engine, persisted_messages) = lock(&slot.runtime)
            .as_ref()
            .map(|runtime| (runtime.engine.clone(), runtime.persisted_messages))
            .ok_or_else(|| not_found("session engine is not loaded"))?;
        engine.flush_turn_usage().await;
        let snapshot = engine.snapshot().await?;
        if let Some(model) = model {
            self.repository.update_model(session, model.clone()).await?;
            if let Some(runtime) = lock(&slot.runtime).as_mut() {
                runtime.template = runtime.template.with_model(model);
            }
        }
        let mut loaded = self.repository.load(session).await?;
        loaded.meta.model = snapshot.model;
        let outcome = self
            .repository
            .save(
                session,
                loaded.meta,
                snapshot.store,
                SessionWriteMode::Append {
                    expected_existing: persisted_messages,
                },
            )
            .await?;
        if let SessionSaveOutcome::Saved { persisted_messages } = outcome {
            if let Some(runtime) = lock(&slot.runtime).as_mut() {
                runtime.persisted_messages = persisted_messages;
            }
        }
        Ok(())
    }

    async fn observe_event(
        &self,
        session: &SessionLocator,
        _turn_id: TurnId,
        event: &Event,
    ) -> Option<UsageSnapshot> {
        let engine = self.runtime_engine(session)?;
        engine.observe_event(event).await
    }

    fn finish_turn_usage(&self, session: &SessionLocator, _turn_id: TurnId) {
        if let Some(engine) = self.runtime_engine(session) {
            engine.finish_turn_usage();
        }
    }

    async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
        match query {
            AgentQuery::Capabilities => Ok(AgentSnapshot::Capabilities(self.capabilities.clone())),
            AgentQuery::Threads => Ok(AgentSnapshot::Threads(self.repository.list()?)),
            AgentQuery::History { session } => Ok(AgentSnapshot::History(
                self.repository.history(&session).await?,
            )),
            AgentQuery::Diff { session } => Ok(AgentSnapshot::Diff(self.diff(session).await?)),
            AgentQuery::RuntimeOptions => Ok(AgentSnapshot::RuntimeOptions(self.runtime_options())),
            AgentQuery::SessionRuntimeOptions { session } => {
                let options = self.session_runtime_options(&session).await?;
                Ok(AgentSnapshot::SessionRuntimeOptions { session, options })
            }
            AgentQuery::ProjectPermissions { workspace_uri } => {
                let cwd = workspace_uri_to_path(&workspace_uri)?;
                let allowed = ConfigManager::project_allowed_tools(&cwd).map_err(map_internal)?;
                Ok(AgentSnapshot::ProjectPermissions(allowed))
            }
            AgentQuery::Integrations { workspace_uri } => {
                let snapshot = crate::integrations::discover_registry(
                    workspace_uri,
                    self.config_dir.as_deref(),
                    &self.capabilities,
                )
                .map_err(map_internal)?;
                Ok(AgentSnapshot::Integrations(snapshot))
            }
        }
    }
}

fn convert_content(input: Vec<UserContent>) -> Vec<ContentBlock> {
    input
        .into_iter()
        .map(|content| match content {
            UserContent::Text { text } => ContentBlock::Text { text },
            UserContent::Image {
                mime_type,
                data_base64,
                ..
            } => ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: mime_type,
                    data: data_base64,
                },
            },
        })
        .collect()
}

fn endpoint_error_stream(error: EndpointError) -> DriverEventStream {
    Box::pin(stream::once(async move {
        Err(agent::error::AgentError::other(error.message))
    }))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn map_internal(error: impl std::fmt::Display) -> EndpointError {
    internal(error.to_string())
}

fn invalid(message: impl Into<String>) -> EndpointError {
    endpoint_error(EndpointErrorKind::InvalidRequest, message)
}

fn not_found(message: impl Into<String>) -> EndpointError {
    endpoint_error(EndpointErrorKind::NotFound, message)
}

fn denied(message: impl Into<String>) -> EndpointError {
    endpoint_error(EndpointErrorKind::CapabilityDenied, message)
}

fn unavailable(message: impl Into<String>) -> EndpointError {
    endpoint_error(EndpointErrorKind::Unavailable, message)
}

fn internal(message: impl Into<String>) -> EndpointError {
    endpoint_error(EndpointErrorKind::Internal, message)
}

fn endpoint_error(kind: EndpointErrorKind, message: impl Into<String>) -> EndpointError {
    EndpointError {
        kind,
        message: message.into(),
    }
}
