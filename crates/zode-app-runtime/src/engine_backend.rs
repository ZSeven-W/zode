use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use agent::abort::AbortController;
use agent::error::AgentError;
use agent::stream::Event;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEventKind, AgentQuery, AgentSnapshot, EndpointError,
    EndpointErrorKind, NodeId, SessionLocator, ToolCall, ToolStatus, TurnId, UsageSnapshot,
};

use crate::{EventSink, NodeBackend};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistedApproval {
    AllowAlways,
    AllowOnceFallback { message: String },
}

/// Persist an allow-always rule, falling back explicitly instead of leaving
/// the waiting tool request unresolved when project state cannot be written.
pub fn persist_project_allow(cwd: &Path, tool: &str) -> PersistedApproval {
    match zode_core::persist_allow_always(cwd, tool) {
        Ok(()) => PersistedApproval::AllowAlways,
        Err(_) => PersistedApproval::AllowOnceFallback {
            message: "project permission could not be persisted; allowed once".into(),
        },
    }
}

const UNKNOWN_EVENT_CODE: &str = "agent.event.unknown";
const UNKNOWN_EVENT_MESSAGE: &str = "Ignored an unsupported agent runtime event";
const UNKNOWN_TOOL_NAME: &str = "unknown";
const UNKNOWN_TOOL_SUMMARY: &str = "Tool result";
const MAX_SUMMARY_CHARS: usize = 160;

#[derive(Debug, Clone)]
struct CachedTool {
    name: String,
    summary: String,
}

/// Converts agent-runtime stream events into the stable node protocol.
///
/// Tool arguments and results stay behind this boundary. Only a small,
/// display-safe summary is cached so a later `ToolResult` can reuse the tool's
/// identity without exposing its raw payload.
#[derive(Debug, Default)]
pub struct EventNormalizer {
    tools: HashMap<String, CachedTool>,
}

impl EventNormalizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn normalize(&mut self, event: Event) -> Option<AgentEventKind> {
        match event {
            Event::TextDelta { delta } => Some(AgentEventKind::TextDelta { delta }),
            Event::Thinking { delta } => Some(AgentEventKind::ThinkingDelta { delta }),
            Event::ToolUse { id, name, input } => {
                let summary = safe_tool_summary(&name, &input);
                self.tools.insert(
                    id.clone(),
                    CachedTool {
                        name: name.clone(),
                        summary: summary.clone(),
                    },
                );

                Some(AgentEventKind::ToolStarted {
                    tool: ToolCall {
                        id,
                        name,
                        status: ToolStatus::Running,
                        summary,
                        detail: None,
                    },
                })
            }
            Event::ToolResult { id, ok, .. } => {
                let cached = self.tools.remove(&id).unwrap_or_else(|| CachedTool {
                    name: UNKNOWN_TOOL_NAME.to_owned(),
                    summary: UNKNOWN_TOOL_SUMMARY.to_owned(),
                });

                Some(AgentEventKind::ToolCompleted {
                    tool: ToolCall {
                        id,
                        name: cached.name,
                        status: if ok {
                            ToolStatus::Completed
                        } else {
                            ToolStatus::Failed
                        },
                        summary: cached.summary,
                        detail: None,
                    },
                })
            }
            Event::Usage {
                input_tokens,
                output_tokens,
                ..
            } => Some(AgentEventKind::Usage {
                usage: UsageSnapshot {
                    input_tokens: u64::from(input_tokens),
                    output_tokens: u64::from(output_tokens),
                    context_used: None,
                    cost_usd: None,
                },
            }),
            Event::Notice { code, message } => Some(AgentEventKind::StatusNotice { code, message }),
            Event::Error { message, .. } => Some(AgentEventKind::Error {
                message: safe_message(&message, "agent runtime error"),
                retryable: true,
            }),
            Event::Result { .. } => None,
            Event::Unknown => Some(unknown_event_notice()),
            _ => Some(unknown_event_notice()),
        }
    }
}

fn safe_tool_summary(name: &str, input: &serde_json::Value) -> String {
    let Some(input) = input.as_object() else {
        return name.to_owned();
    };

    for key in ["path", "url", "query"] {
        if let Some(value) = input.get(key).and_then(serde_json::Value::as_str) {
            let value = sanitize_summary_value(value);
            if !value.is_empty() {
                return format!("{name} {key}={value}");
            }
        }
    }

    name.to_owned()
}

fn sanitize_summary_value(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    if normalized.chars().count() <= MAX_SUMMARY_CHARS {
        normalized
    } else {
        format!(
            "{}…",
            normalized
                .chars()
                .take(MAX_SUMMARY_CHARS)
                .collect::<String>()
        )
    }
}

fn unknown_event_notice() -> AgentEventKind {
    AgentEventKind::StatusNotice {
        code: UNKNOWN_EVENT_CODE.to_owned(),
        message: UNKNOWN_EVENT_MESSAGE.to_owned(),
    }
}

pub type DriverEventStream =
    Pin<Box<dyn Stream<Item = Result<Event, AgentError>> + Send + 'static>>;

/// Runtime operations needed by the local node lifecycle coordinator.
#[async_trait]
pub trait EngineDriver: Send + Sync + 'static {
    async fn command(&self, command: AgentCommand) -> Result<(), EndpointError>;

    async fn start_turn(&self, command: AgentCommand, abort: AbortController) -> DriverEventStream;

    async fn finish_turn(
        &self,
        session: &SessionLocator,
        turn_id: TurnId,
        model: Option<String>,
        interrupted: bool,
    ) -> Result<(), EndpointError>;

    /// Feed one raw runtime event into session-scoped accounting. Drivers may
    /// return a cumulative, display-safe usage snapshot for `Usage` events.
    async fn observe_event(
        &self,
        _session: &SessionLocator,
        _turn_id: TurnId,
        _event: &Event,
    ) -> Option<UsageSnapshot> {
        None
    }

    /// Clear per-turn cumulative-usage baselines after every terminal path.
    fn finish_turn_usage(&self, _session: &SessionLocator, _turn_id: TurnId) {}

    async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError>;
}

struct ActiveTurn {
    turn_id: TurnId,
    generation: u64,
    abort: AbortController,
}

/// Coordinates per-session engine turns behind the local `NodeBackend` seam.
pub struct EngineBackend {
    local_node_id: NodeId,
    driver: Arc<dyn EngineDriver>,
    active: Arc<Mutex<HashMap<SessionLocator, ActiveTurn>>>,
    next_generation: AtomicU64,
}

impl EngineBackend {
    pub fn new(local_node_id: NodeId, driver: Arc<dyn EngineDriver>) -> Self {
        Self {
            local_node_id,
            driver,
            active: Arc::new(Mutex::new(HashMap::new())),
            next_generation: AtomicU64::new(1),
        }
    }

    fn ensure_local(&self, session: &SessionLocator) -> Result<(), EndpointError> {
        if session.node_id == self.local_node_id {
            Ok(())
        } else {
            Err(endpoint_error(
                EndpointErrorKind::CapabilityDenied,
                "session is not owned by this node",
            ))
        }
    }

    fn allocate_generation(&self) -> Result<u64, EndpointError> {
        self.next_generation
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |generation| {
                generation.checked_add(1)
            })
            .map_err(|_| {
                endpoint_error(
                    EndpointErrorKind::Internal,
                    "turn generation counter is exhausted",
                )
            })
    }

    fn active_turn_matches(&self, session: &SessionLocator, turn_id: TurnId) -> bool {
        lock_active(&self.active)
            .get(session)
            .is_some_and(|active| active.turn_id == turn_id)
    }

    fn ensure_idle(&self, session: &SessionLocator) -> Result<(), EndpointError> {
        if lock_active(&self.active).contains_key(session) {
            Err(endpoint_error(
                EndpointErrorKind::Busy,
                "session has an active turn",
            ))
        } else {
            Ok(())
        }
    }

    fn start_turn(&self, command: AgentCommand, events: EventSink) -> Result<(), EndpointError> {
        let turn_id = command.turn_id.ok_or_else(|| {
            endpoint_error(
                EndpointErrorKind::InvalidRequest,
                "start turn requires a turn identity",
            )
        })?;
        let session = command.session.clone();
        let abort = AbortController::new();

        let generation = {
            let mut active = lock_active(&self.active);
            match active.entry(session.clone()) {
                std::collections::hash_map::Entry::Occupied(_) => {
                    return Err(endpoint_error(
                        EndpointErrorKind::Busy,
                        "session already has an active turn",
                    ));
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let generation = self.allocate_generation()?;
                    entry.insert(ActiveTurn {
                        turn_id,
                        generation,
                        abort: abort.clone(),
                    });
                    generation
                }
            }
        };

        let driver = self.driver.clone();
        let active = self.active.clone();
        tokio::spawn(async move {
            drive_turn(
                driver, active, command, session, turn_id, generation, abort, events,
            )
            .await;
        });
        Ok(())
    }

    fn interrupt_turn(
        &self,
        session: &SessionLocator,
        turn_id: TurnId,
    ) -> Result<(), EndpointError> {
        let abort = {
            let active = lock_active(&self.active);
            match active.get(session) {
                Some(active) if active.turn_id == turn_id => active.abort.clone(),
                _ => {
                    return Err(endpoint_error(
                        EndpointErrorKind::NotFound,
                        "matching active turn was not found",
                    ));
                }
            }
        };
        abort.abort();
        Ok(())
    }
}

#[async_trait]
impl NodeBackend for EngineBackend {
    async fn command(&self, command: AgentCommand, events: EventSink) -> Result<(), EndpointError> {
        command.validate().map_err(|error| {
            endpoint_error(EndpointErrorKind::InvalidRequest, error.to_string())
        })?;
        self.ensure_local(&command.session)?;

        if matches!(&command.kind, AgentCommandKind::StartTurn { .. }) {
            return self.start_turn(command, events);
        }
        if matches!(&command.kind, AgentCommandKind::InterruptTurn) {
            let turn_id = command.turn_id.ok_or_else(|| {
                endpoint_error(
                    EndpointErrorKind::InvalidRequest,
                    "interrupt turn requires a turn identity",
                )
            })?;
            return self.interrupt_turn(&command.session, turn_id);
        }
        if matches!(&command.kind, AgentCommandKind::SteerTurn { .. }) {
            let turn_id = command.turn_id.ok_or_else(|| {
                endpoint_error(
                    EndpointErrorKind::InvalidRequest,
                    "steer turn requires a turn identity",
                )
            })?;
            if !self.active_turn_matches(&command.session, turn_id) {
                return Err(endpoint_error(
                    EndpointErrorKind::NotFound,
                    "matching active turn was not found",
                ));
            }
        }
        if matches!(
            &command.kind,
            AgentCommandKind::DeleteSession
                | AgentCommandKind::SetModel { .. }
                | AgentCommandKind::SetEffort { .. }
                | AgentCommandKind::SetSandbox { .. }
        ) {
            self.ensure_idle(&command.session)?;
        }

        self.driver
            .command(command)
            .await
            .map_err(sanitize_endpoint_error)
    }

    async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
        match &query {
            AgentQuery::Diff { session }
            | AgentQuery::History { session }
            | AgentQuery::SessionRuntimeOptions { session } => {
                self.ensure_local(session)?;
            }
            _ => {}
        }
        self.driver
            .query(query)
            .await
            .map_err(sanitize_endpoint_error)
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive_turn(
    driver: Arc<dyn EngineDriver>,
    active: Arc<Mutex<HashMap<SessionLocator, ActiveTurn>>>,
    command: AgentCommand,
    session: SessionLocator,
    turn_id: TurnId,
    generation: u64,
    abort: AbortController,
    events: EventSink,
) {
    let mut stream = driver.start_turn(command, abort.clone()).await;
    let mut normalizer = EventNormalizer::new();
    let mut model = None;
    let mut interrupted = false;

    while let Some(event) = stream.next().await {
        if abort.is_aborted() {
            interrupted = true;
            break;
        }

        match event {
            Ok(event) => {
                let cumulative_usage = driver.observe_event(&session, turn_id, &event).await;
                if let Event::Result { data } = &event {
                    model = data.model.clone();
                }
                if let Some(mut kind) = normalizer.normalize(event) {
                    if let (AgentEventKind::Usage { usage }, Some(cumulative)) =
                        (&mut kind, cumulative_usage)
                    {
                        *usage = cumulative;
                    }
                    if events.send(session.clone(), turn_id, kind).await.is_err() {
                        abort.abort();
                        interrupted = true;
                        break;
                    }
                }
            }
            Err(AgentError::Aborted(_)) => {
                interrupted = true;
                break;
            }
            Err(error) => {
                let _ = events
                    .send(
                        session.clone(),
                        turn_id,
                        AgentEventKind::Error {
                            message: safe_message(&error.to_string(), "agent stream failed"),
                            retryable: false,
                        },
                    )
                    .await;
                break;
            }
        }
    }
    interrupted |= abort.is_aborted();
    driver.finish_turn_usage(&session, turn_id);

    if let Err(error) = driver
        .finish_turn(&session, turn_id, model, interrupted)
        .await
    {
        let _ = events
            .send(
                session.clone(),
                turn_id,
                AgentEventKind::Error {
                    message: safe_message(&error.message, "session persistence failed"),
                    retryable: false,
                },
            )
            .await;
    }
    let _ = events
        .send(session.clone(), turn_id, AgentEventKind::DiffInvalidated)
        .await;

    // `TurnFinished` is the public hand-off edge for the next turn. Release
    // this exact generation before publishing that edge so a consumer can
    // immediately start another turn for the same session without racing the
    // backend's stale busy slot.
    {
        let mut active = lock_active(&active);
        if active
            .get(&session)
            .is_some_and(|turn| turn.turn_id == turn_id && turn.generation == generation)
        {
            active.remove(&session);
        }
    }

    let _ = events
        .send(
            session.clone(),
            turn_id,
            AgentEventKind::TurnFinished { interrupted },
        )
        .await;
}

fn lock_active(
    active: &Mutex<HashMap<SessionLocator, ActiveTurn>>,
) -> std::sync::MutexGuard<'_, HashMap<SessionLocator, ActiveTurn>> {
    active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn sanitize_endpoint_error(mut error: EndpointError) -> EndpointError {
    error.message = safe_message(&error.message, "engine request failed");
    error
}

fn safe_message(message: &str, fallback: &str) -> String {
    let message = sanitize_summary_value(message);
    let lower = message.to_ascii_lowercase();
    let sensitive = [
        "api_key",
        "api-key",
        "apikey",
        "token",
        "secret",
        "password",
        "authorization",
        "bearer ",
        "sk-",
        "ghp_",
        "xoxb-",
    ]
    .iter()
    .any(|marker| lower.contains(marker));

    if message.is_empty() || sensitive {
        fallback.to_owned()
    } else {
        message
    }
}

fn endpoint_error(kind: EndpointErrorKind, message: impl Into<String>) -> EndpointError {
    let message = message.into();
    EndpointError {
        kind,
        message: safe_message(&message, "engine request failed"),
    }
}
