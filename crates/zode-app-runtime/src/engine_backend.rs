use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agent::abort::AbortController;
use agent::error::AgentError;
use agent::stream::Event;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use zode_core::{SubAgent, SubAgentStatus};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEventKind, AgentQuery, AgentSnapshot,
    BackgroundProcessSnapshot, BackgroundProcessStatus, EndpointError, EndpointErrorKind, NodeId,
    SessionLocator, SubagentSnapshot, SubagentStatus as WireSubagentStatus, ToolCall, ToolStatus,
    TurnId, UsageSnapshot,
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
/// Cap for `SubagentSnapshot::result_summary` — the M2 panel row shows this
/// as a one-line muted preview, not a full transcript entry.
const RESULT_SUMMARY_MAX_CHARS: usize = 120;

/// The `agent-tools-code` `Task` tool's stable name — the only tool whose
/// lifecycle triggers a sub-agent registry diff.
const TASK_TOOL_NAME: &str = "Task";

/// The `agent-tools-code` background-shell tools' stable names — see
/// `vendor/agent/crates/agent-tools-code/src/bash_async.rs`. Their
/// lifecycle triggers a `BackgroundShellTracker` diff, mirroring how
/// `TASK_TOOL_NAME` triggers a sub-agent registry diff.
const BASH_RUN_TOOL_NAME: &str = "BashRun";
const BASH_OUTPUT_TOOL_NAME: &str = "BashOutput";
const KILL_SHELL_TOOL_NAME: &str = "KillShell";

/// The `zode-core` `computer` tool group's stable names — see
/// `zode_core::computer::tools`. Both get a structured summary (built from
/// `action` + target, not a generic `path=`/`url=` guess) and, on a
/// `permission_pending` result, an actionable detail hint.
const COMPUTER_READ_TOOL_NAME: &str = "computer_read";
const COMPUTER_ACT_TOOL_NAME: &str = "computer_act";

/// Surfaces the computer-use "permission_pending" retry hint (see
/// docs/proposals/computer-use.md §2) as tool-call detail text, so the
/// desktop transcript can render an actionable "需要授予权限" affordance
/// instead of a silently completed card. Every other tool's detail stays
/// `None`, matching prior behavior.
const PERMISSION_PENDING_DETAIL: &str =
    "需要授予权限：请在设置 → 电脑操控中打开系统设置完成授权，然后重试。";

fn permission_pending_detail(name: &str, output: &serde_json::Value) -> Option<String> {
    if name != COMPUTER_READ_TOOL_NAME && name != COMPUTER_ACT_TOOL_NAME {
        return None;
    }
    let is_pending =
        output.get("status").and_then(serde_json::Value::as_str) == Some("permission_pending");
    is_pending.then(|| PERMISSION_PENDING_DETAIL.to_owned())
}

/// Token-only changes to the same sub-agent are coalesced so a busy child
/// loop doesn't flood the event stream with per-delta updates.
const SUBAGENT_TOKEN_THROTTLE: Duration = Duration::from_secs(1);

/// Which model each `Task`-spawned sub-agent runs under, mirroring the
/// resolution `zode_core::ZodeTaskFactory::build` performs: an agent
/// definition that pins `model:` in its frontmatter wins, everything else
/// inherits the session's active model. Snapshotted per diff rather than
/// per agent so one registry pass costs one driver call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubagentModels {
    /// The session engine's active model. `None` for drivers that don't run
    /// one (test doubles), which makes every resolution `None` too.
    pub session_model: Option<String>,
    /// Agent type → the model its definition pins.
    pub overrides: HashMap<String, String>,
}

impl SubagentModels {
    pub fn resolve(&self, agent_type: &str) -> Option<String> {
        self.overrides
            .get(agent_type)
            .cloned()
            .or_else(|| self.session_model.clone())
    }
}

#[derive(Debug, Clone)]
struct CachedTool {
    name: String,
    summary: String,
}

/// Last-emitted state for one sub-agent, used to decide whether a fresh
/// registry snapshot warrants a new `SubagentUpdate` event.
#[derive(Debug, Clone, Copy)]
struct EmittedSubagent {
    status: SubAgentStatus,
    tokens: u64,
    emitted_at: Instant,
}

/// Converts agent-runtime stream events into the stable node protocol.
///
/// Tool arguments and results stay behind this boundary. Only a small,
/// display-safe summary is cached so a later `ToolResult` can reuse the tool's
/// identity without exposing its raw payload.
#[derive(Debug, Default)]
pub struct EventNormalizer {
    tools: HashMap<String, CachedTool>,
    subagents: HashMap<u64, EmittedSubagent>,
    /// Maps a `BashRun`-allocated `shell_id` back to the transcript
    /// `ToolCall.id` that launched it, captured from the `BashRun`
    /// `ToolResult` output. Powers the background-process row's "view
    /// output" jump — see `AgentEventKind::BackgroundProcessUpdate`.
    bash_shell_tool_calls: HashMap<String, String>,
    /// Last-emitted `killed` flag per shell id, so a shell already reported
    /// as `Stopped` is not re-emitted on every subsequent bash-tool event.
    background_processes: HashMap<String, bool>,
}

impl EventNormalizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Diffs a fresh sub-agent registry snapshot against what was last
    /// emitted, returning zero or more `SubagentUpdate` events. Call this
    /// after a `Task` tool `ToolStarted`/`ToolCompleted` event, and once more
    /// right before `TurnFinished` so a turn that ends (normally or via
    /// interruption) while a child is still `Running` gets a final,
    /// authoritative correction from the registry.
    ///
    /// Throttle: a sub-agent whose status is unchanged from the last emission
    /// is not re-emitted; a status change is always emitted immediately;
    /// a token-count-only change is coalesced to at most one emission per
    /// [`SUBAGENT_TOKEN_THROTTLE`] interval.
    pub fn diff_subagents(
        &mut self,
        snapshot: &[SubAgent],
        turn_id: TurnId,
        now: Instant,
        models: &SubagentModels,
    ) -> Vec<AgentEventKind> {
        let mut updates = Vec::new();
        for agent in snapshot {
            let tokens = u64::from(agent.input_tokens) + u64::from(agent.output_tokens);
            let should_emit = match self.subagents.get(&agent.id) {
                None => true,
                Some(last) => {
                    last.status != agent.status
                        || (last.tokens != tokens
                            && now.saturating_duration_since(last.emitted_at)
                                >= SUBAGENT_TOKEN_THROTTLE)
                }
            };
            if !should_emit {
                continue;
            }
            self.subagents.insert(
                agent.id,
                EmittedSubagent {
                    status: agent.status,
                    tokens,
                    emitted_at: now,
                },
            );
            updates.push(AgentEventKind::SubagentUpdate {
                subagent: to_wire_subagent(agent, tokens, turn_id, models),
            });
        }
        updates
    }

    /// Diffs a fresh `BackgroundShellTracker` snapshot against what was last
    /// emitted, returning zero or more `BackgroundProcessUpdate` events. Call
    /// this after a `BashRun`/`BashOutput`/`KillShell` tool event, mirroring
    /// `diff_subagents`.
    ///
    /// `zode_core::bg_shells::BgShell` only models two states — a shell is
    /// either tracked-and-alive or `killed` — so this can only ever produce
    /// `Running` (first sighting or still alive) and `Stopped` (killed).
    /// `Starting` and `Stopping` are real states in the wire type for Codex
    /// parity, but nothing in the desktop stack observes those transients
    /// today: `BgShellHook` only records a shell after `BashRun`'s
    /// `ToolResult` already carries a `shell_id`, and only marks it killed
    /// after `KillShell`'s `ToolResult` already succeeded. Producing them
    /// for real would need `BackgroundShellTracker` itself to gain an
    /// in-flight-launch and in-flight-kill state, which is out of this
    /// crate's scope — see `docs/proposals/right-panel-parity.md` section 1.2.
    pub fn diff_background_processes(
        &mut self,
        snapshot: &[zode_core::bg_shells::BgShell],
    ) -> Vec<AgentEventKind> {
        let mut updates = Vec::new();
        for shell in snapshot {
            let should_emit = self
                .background_processes
                .get(&shell.shell_id)
                .is_none_or(|&last_killed| last_killed != shell.killed);
            if !should_emit {
                continue;
            }
            self.background_processes
                .insert(shell.shell_id.clone(), shell.killed);
            updates.push(AgentEventKind::BackgroundProcessUpdate {
                process: BackgroundProcessSnapshot {
                    id: shell.shell_id.clone(),
                    command: shell.command.clone(),
                    status: if shell.killed {
                        BackgroundProcessStatus::Stopped
                    } else {
                        BackgroundProcessStatus::Running
                    },
                    started_at_ms: i64::try_from(shell.started_at)
                        .unwrap_or(i64::MAX)
                        .saturating_mul(1000),
                    tool_call_id: self.bash_shell_tool_calls.get(&shell.shell_id).cloned(),
                },
            });
        }
        updates
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
            Event::ToolResult { id, ok, output } => {
                let cached = self.tools.remove(&id).unwrap_or_else(|| CachedTool {
                    name: UNKNOWN_TOOL_NAME.to_owned(),
                    summary: UNKNOWN_TOOL_SUMMARY.to_owned(),
                });
                let detail = permission_pending_detail(&cached.name, &output);
                if ok && cached.name == BASH_RUN_TOOL_NAME {
                    if let Some(shell_id) = bash_shell_id(&output) {
                        self.bash_shell_tool_calls.insert(shell_id, id.clone());
                    }
                }

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
                        detail,
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

    if name == TASK_TOOL_NAME {
        return task_tool_summary(input).unwrap_or_else(|| name.to_owned());
    }

    if name == COMPUTER_READ_TOOL_NAME || name == COMPUTER_ACT_TOOL_NAME {
        if let Some(summary) = computer_tool_summary(input) {
            return summary;
        }
    }

    for key in ["path", "url", "query", "command", "shell_id"] {
        if let Some(value) = input.get(key).and_then(serde_json::Value::as_str) {
            let value = sanitize_summary_value(value);
            if !value.is_empty() {
                return format!("{name} {key}={value}");
            }
        }
    }

    name.to_owned()
}

/// Builds a `computer_read`/`computer_act` summary from the tool's own
/// `action` field plus whatever target it carries (`element`, `x`/`y`,
/// `text`, `key`), instead of the generic `path=`/`url=`/`query=` guess —
/// see `zode_core::computer::tools::{ComputerReadTool, ComputerActTool}`'s
/// input schemas. `tool_card::action_presentation` parses the first
/// whitespace-separated token back out as the action for its human label.
fn computer_tool_summary(input: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let action = input.get("action")?.as_str()?;
    let mut parts = vec![action.to_owned()];
    if let Some(app) = input
        .get("app")
        .and_then(serde_json::Value::as_str)
        .map(sanitize_summary_value)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("app={app}"));
    }
    if let Some(element) = input.get("element").and_then(serde_json::Value::as_u64) {
        parts.push(format!("element={element}"));
    } else if let (Some(x), Some(y)) = (
        input.get("x").and_then(serde_json::Value::as_f64),
        input.get("y").and_then(serde_json::Value::as_f64),
    ) {
        parts.push(format!("at=({x:.0},{y:.0})"));
    }
    if let Some(text) = input
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(sanitize_summary_value)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("text={text}"));
    }
    if let Some(key) = input
        .get("key")
        .and_then(serde_json::Value::as_str)
        .map(sanitize_summary_value)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("key={key}"));
    }
    Some(parts.join(" "))
}

/// Builds a real (not name-guessed) summary for a `Task` tool call from its
/// own input, which always carries the model-chosen `agent_type` and an
/// optional one-line `description` — see `agent-tools-code::task::TaskInput`.
fn task_tool_summary(input: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let agent_type = input
        .get("agent_type")
        .and_then(serde_json::Value::as_str)
        .map(sanitize_summary_value)
        .filter(|value| !value.is_empty())?;
    let description = input
        .get("description")
        .and_then(serde_json::Value::as_str)
        .map(sanitize_summary_value)
        .filter(|value| !value.is_empty());
    Some(match description {
        Some(description) => format!("{agent_type}: {description}"),
        None => agent_type,
    })
}

/// Converts a core registry snapshot entry into the wire representation,
/// stamped with the turn during which the diff was observed.
fn to_wire_subagent(
    agent: &SubAgent,
    tokens: u64,
    turn_id: TurnId,
    models: &SubagentModels,
) -> SubagentSnapshot {
    let display_name = agent
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty())
        .map(sanitize_summary_value)
        .unwrap_or_else(|| agent.agent_type.clone());
    SubagentSnapshot {
        id: agent.id.to_string(),
        agent_type: agent.agent_type.clone(),
        display_name,
        depth: u8::try_from(agent.depth).unwrap_or(u8::MAX),
        status: match agent.status {
            SubAgentStatus::Running => WireSubagentStatus::Running,
            SubAgentStatus::Done => WireSubagentStatus::Completed,
            SubAgentStatus::Failed => WireSubagentStatus::Failed,
        },
        tokens,
        turn_id,
        // The registry's wall clock is second-precision (`now_secs()`); that
        // is plenty for the relative-time display this stamp drives ("1
        // 小时", "2 天"), so no registry change was needed for millisecond
        // precision here.
        completed_at_ms: agent.finished_at.map(|secs| {
            i64::try_from(secs)
                .unwrap_or(i64::MAX)
                .saturating_mul(1_000)
        }),
        result_summary: agent.final_output.as_deref().map(summarize_result),
        model: models.resolve(&agent.agent_type),
    }
}

/// First line of a sub-agent's final answer, truncated for display. Applied
/// once at the terminal transition, never to streamed deltas.
fn summarize_result(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or("").trim();
    let sanitized = sanitize_summary_value(first_line);
    if sanitized.chars().count() <= RESULT_SUMMARY_MAX_CHARS {
        sanitized
    } else {
        format!(
            "{}…",
            sanitized
                .chars()
                .take(RESULT_SUMMARY_MAX_CHARS)
                .collect::<String>()
        )
    }
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

    /// Clear per-turn cumulative-baselines after every terminal path.
    fn finish_turn_usage(&self, _session: &SessionLocator, _turn_id: TurnId) {}

    /// Snapshot the session's live `Task`-spawned sub-agent registry.
    /// Default empty — drivers with no session engine loaded (or none of
    /// their own) simply report no sub-agents.
    fn subagents_snapshot(&self, _session: &SessionLocator) -> Vec<SubAgent> {
        Vec::new()
    }

    /// Which model this session's sub-agents run under, by agent type. See
    /// [`SubagentModels`]. Default empty — a driver that reports no
    /// sub-agents has no models to disclose either.
    fn subagent_models(&self, _session: &SessionLocator) -> SubagentModels {
        SubagentModels::default()
    }

    /// Snapshot the session's tracked background shells (`BashRun`
    /// sessions). Default empty for drivers with no session engine loaded.
    async fn background_processes_snapshot(
        &self,
        _session: &SessionLocator,
    ) -> Vec<zode_core::bg_shells::BgShell> {
        Vec::new()
    }

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

    fn ensure_all_idle(&self) -> Result<(), EndpointError> {
        if lock_active(&self.active).is_empty() {
            Ok(())
        } else {
            Err(endpoint_error(
                EndpointErrorKind::Busy,
                "provider configuration cannot reload while a turn is active",
            ))
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
        if matches!(&command.kind, AgentCommandKind::ReloadProviderConfiguration) {
            self.ensure_all_idle()?;
        } else if matches!(
            &command.kind,
            AgentCommandKind::DeleteSession
                | AgentCommandKind::SetModel { .. }
                | AgentCommandKind::SetEffort { .. }
                | AgentCommandKind::SetSandbox { .. }
                | AgentCommandKind::SetPermissionPreset { .. }
                | AgentCommandKind::SetIntegrationEnabled { .. }
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

    'turn: while let Some(event) = stream.next().await {
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
                    let mut registry_updates = if is_task_tool_event(&kind) {
                        normalizer.diff_subagents(
                            &driver.subagents_snapshot(&session),
                            turn_id,
                            Instant::now(),
                            &driver.subagent_models(&session),
                        )
                    } else {
                        Vec::new()
                    };
                    if is_background_shell_tool_event(&kind) {
                        registry_updates.extend(normalizer.diff_background_processes(
                            &driver.background_processes_snapshot(&session).await,
                        ));
                    }
                    if events.send(session.clone(), turn_id, kind).await.is_err() {
                        abort.abort();
                        interrupted = true;
                        break;
                    }
                    for update in registry_updates {
                        if events.send(session.clone(), turn_id, update).await.is_err() {
                            abort.abort();
                            interrupted = true;
                            break 'turn;
                        }
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

    // One last authoritative diff so a turn that ends (normally or via
    // interruption) while a child was still `Running` gets corrected to the
    // registry's terminal state before `TurnFinished` reaches consumers.
    for update in normalizer.diff_subagents(
        &driver.subagents_snapshot(&session),
        turn_id,
        Instant::now(),
        &driver.subagent_models(&session),
    ) {
        let _ = events.send(session.clone(), turn_id, update).await;
    }
    for update in
        normalizer.diff_background_processes(&driver.background_processes_snapshot(&session).await)
    {
        let _ = events.send(session.clone(), turn_id, update).await;
    }
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

/// True when a normalized event is a `Task` tool's `ToolStarted`/
/// `ToolCompleted`, the only trigger for a sub-agent registry diff.
fn is_task_tool_event(kind: &AgentEventKind) -> bool {
    match kind {
        AgentEventKind::ToolStarted { tool } | AgentEventKind::ToolCompleted { tool } => {
            tool.name == TASK_TOOL_NAME
        }
        _ => false,
    }
}

/// True when a normalized event is a background-shell tool's
/// `ToolStarted`/`ToolCompleted`, the trigger for a `BackgroundShellTracker`
/// diff (see `EventNormalizer::diff_background_processes`).
fn is_background_shell_tool_event(kind: &AgentEventKind) -> bool {
    match kind {
        AgentEventKind::ToolStarted { tool } | AgentEventKind::ToolCompleted { tool } => matches!(
            tool.name.as_str(),
            BASH_RUN_TOOL_NAME | BASH_OUTPUT_TOOL_NAME | KILL_SHELL_TOOL_NAME
        ),
        _ => false,
    }
}

/// Pulls `shell_id` out of `BashRun`'s `ToolResult` output JSON.
fn bash_shell_id(output: &serde_json::Value) -> Option<String> {
    output
        .get("shell_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
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
