use crate::ProtocolError;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

/// The only protocol version accepted by this implementation.
pub const PROTOCOL_VERSION: u16 = 1;

macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Allocates a fresh random identity. `Default` delegates to this
            /// constructor and is intentionally non-deterministic.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Parses an identity from a UUID string.
            pub fn parse(value: &str) -> Result<Self, uuid::Error> {
                Uuid::parse_str(value).map(Self)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

uuid_id!(
    /// Stable identity for one installed Zode node.
    NodeId
);
uuid_id!(
    /// Identity for one agent turn.
    TurnId
);
uuid_id!(
    /// Identity for one terminal instance.
    TerminalId
);

/// Locates a session without assuming it is stored on the local machine.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLocator {
    pub node_id: NodeId,
    pub session_id: String,
}

impl SessionLocator {
    /// Creates a locator from an already allocated node and session identity.
    pub fn new(node_id: NodeId, session_id: impl Into<String>) -> Self {
        Self {
            node_id,
            session_id: session_id.into(),
        }
    }
}

/// A capability that a Zode node can advertise.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeCapability {
    Agent,
    Workspace,
    FileSystem,
    Terminal,
    Browser,
    Camera,
    Notifications,
    Approval,
}

/// The capabilities offered by a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityManifest {
    pub node_id: NodeId,
    pub capabilities: BTreeSet<NodeCapability>,
}

/// Canonical local source that contributed one integration registry row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IntegrationRegistryKind {
    ToolGroup,
    Skill,
    Mcp,
    Lsp,
    NodeCapability,
}

/// Truthful availability derived without starting an integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IntegrationRegistryState {
    Ready,
    Configured,
    Disabled,
}

/// One locally discovered integration. `source_id` is the stable registry key,
/// never a display label reconstructed by the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationRegistryEntry {
    pub source_id: String,
    pub name: String,
    pub description: String,
    pub kind: IntegrationRegistryKind,
    pub state: IntegrationRegistryState,
    pub installed: bool,
}

/// Workspace-scoped, disk-only integration discovery result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationRegistrySnapshot {
    pub workspace_uri: WorkspaceUri,
    pub entries: Vec<IntegrationRegistryEntry>,
    /// The optional online directory is independent from local discovery. A
    /// failure here must not erase `entries` or be replaced with fake offers.
    pub directory_error: Option<String>,
}

/// Kind of capability a plugin declares, mirroring
/// `zode_core::plugin_market::manifest::Capability` without this
/// backend-agnostic crate depending on `zode-core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginCapabilityKind {
    Skill,
    Mcp,
    Hook,
}

/// One capability declared by an installed plugin, projected for display on
/// its detail page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCapabilitySummary {
    /// Stable trust-review key (core's `Capability::trust_key`) - matched
    /// against `PluginTrustState`'s pending key lists to decide whether this
    /// row is gated.
    pub key: String,
    pub kind: PluginCapabilityKind,
    pub label: String,
    /// The `skill:<name>` / `mcp:<name>` id already understood by
    /// `SetIntegrationEnabled`. `None` for hooks, which have no independent
    /// enable/disable id today.
    pub toggle_source_id: Option<String>,
}

/// Trust-review status for one installed plugin's reviewable capabilities.
/// Carries only the pending capability keys - the exact command/script text
/// to review is fetched on demand via `AgentQuery::PluginTrustReview` so a
/// routine plugin-list refresh never has to re-read every hook script from
/// disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginTrustState {
    Trusted,
    NeedsReview(Vec<String>),
    Drifted(Vec<String>),
}

/// One installed plugin, listed on the Integrations page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPluginSummary {
    pub id: String,
    pub repo: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub installed_at_ms: u64,
    pub capabilities: Vec<PluginCapabilitySummary>,
    pub trust: PluginTrustState,
}

/// One reviewable capability's exact, unsummarized text - the command line
/// or script source a trust-review screen must render in full.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginTrustItem {
    pub key: String,
    pub content: String,
}

/// Full trust-review payload fetched on demand right before a review screen
/// opens - see `PluginTrustState`'s doc comment for why this is a separate
/// query rather than embedded in every plugin list refresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginTrustReview {
    pub plugin_id: String,
    pub items: Vec<PluginTrustItem>,
}

/// Result of fetching an installed plugin's pinned ref and comparing it with
/// the local checkout. A failed check is not modelled here - it surfaces as
/// the query's error, which the caller renders in the same notice slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateCheck {
    pub plugin_id: String,
    /// `None` when the checkout already sits at the ref's remote tip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<PluginUpdateAvailable>,
}

/// What applying the update would move the checkout to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateAvailable {
    /// A short, already-localized one-liner for the detail overlay, e.g.
    /// `"abc1234 → def5678（2 个提交）"`.
    pub summary: String,
    /// The commit the update would land on - shown so the user can tell two
    /// consecutive checks apart, and logged with the applied update.
    pub target_commit: String,
}

/// A versioned command sent to a Zode node.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCommand {
    pub version: u16,

    /// The caller allocates this locator. For `CreateSession`, the runtime must
    /// adopt this exact identity instead of allocating a replacement session.
    pub session: SessionLocator,

    /// The caller-allocated turn identity. It is required for `StartTurn`,
    /// `SteerTurn`, and `InterruptTurn`; other commands may use it to preserve
    /// an optional association with the turn that prompted the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<TurnId>,

    #[serde(flatten)]
    pub kind: AgentCommandKind,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentCommandWire {
    version: u16,
    session: SessionLocator,
    turn_id: Option<TurnId>,
    #[serde(flatten)]
    kind: AgentCommandKind,
}

impl AgentCommand {
    /// Decodes and validates one command from its JSON wire representation.
    pub fn decode_json(input: &str) -> Result<Self, ProtocolError> {
        let wire: AgentCommandWire = serde_json::from_str(input)?;
        Self::from_wire(wire)
    }

    fn from_wire(wire: AgentCommandWire) -> Result<Self, ProtocolError> {
        let command = Self {
            version: wire.version,
            session: wire.session,
            turn_id: wire.turn_id,
            kind: wire.kind,
        };
        command.validate()?;
        Ok(command)
    }

    /// Validates protocol version and command identity requirements.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_version(self.version)?;

        let command = match &self.kind {
            AgentCommandKind::StartTurn { .. } => Some("startTurn"),
            AgentCommandKind::SteerTurn { .. } => Some("steerTurn"),
            AgentCommandKind::InterruptTurn => Some("interruptTurn"),
            _ => None,
        };
        if let (Some(command), None) = (command, self.turn_id) {
            return Err(ProtocolError::MissingTurnId { command });
        }

        Ok(())
    }
}

impl<'de> Deserialize<'de> for AgentCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = AgentCommandWire::deserialize(deserializer)?;
        Self::from_wire(wire).map_err(D::Error::custom)
    }
}

/// Commands understood by protocol version 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentCommandKind {
    /// Creates a session using the caller-allocated outer `SessionLocator`.
    CreateSession {
        /// Execution directory used by the agent runtime.
        workspace_uri: WorkspaceUri,
        /// Owning project shown in project-grouped navigation. This may differ
        /// from `workspace_uri` when a task runs in an isolated worktree.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project_uri: Option<WorkspaceUri>,
        /// Explicitly records that the task is not attached to any project.
        #[serde(default, skip_serializing_if = "is_false")]
        projectless: bool,
        model: Option<String>,
    },
    RenameSession {
        title: String,
    },
    DeleteSession,
    /// Starts a turn using the caller-allocated outer `TurnId`.
    StartTurn {
        input: Vec<UserContent>,
    },
    /// Adds input to the caller-selected outer `TurnId`.
    SteerTurn {
        input: Vec<UserContent>,
    },
    /// Interrupts the caller-selected outer `TurnId`.
    InterruptTurn,
    Approve {
        approval_id: String,
        decision: ApprovalDecision,
    },
    RevokeProjectPermission {
        workspace_uri: WorkspaceUri,
        tool: String,
    },
    /// Enables or disables one entry that the workspace integration registry
    /// has already discovered. This is not an install command and cannot
    /// create arbitrary plugin ids.
    SetIntegrationEnabled {
        workspace_uri: WorkspaceUri,
        source_id: String,
        enabled: bool,
    },
    SetModel {
        model: String,
    },
    /// Reloads provider credentials, model groups, and the active provider from
    /// the node's global configuration without resetting session state.
    ReloadProviderConfiguration,
    SetEffort {
        effort: String,
    },
    SetSandbox {
        mode: SandboxMode,
        network: bool,
    },
    SetPermissionPreset {
        approval_mode: ApprovalMode,
        sandbox_mode: SandboxMode,
        network: bool,
    },
    /// Clones a plugin from git and scans its capabilities. Slow (network
    /// I/O) - callers must run this off the UI thread; the desktop command
    /// bridge already dispatches every command on a background task.
    InstallPlugin {
        spec: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
    },
    UninstallPlugin {
        plugin_id: String,
    },
    /// Grants trust to the named capabilities (`Some`), or to every currently
    /// reviewable capability (`None`, "全部信任").
    GrantPluginTrust {
        plugin_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    /// Fast-forwards an installed plugin to its pinned ref's current remote
    /// tip and re-scans its capabilities. Slow (network I/O), like
    /// `InstallPlugin`. Trust is not carried over: capabilities whose
    /// executable content changed re-enter the trust-review gate.
    ApplyPluginUpdate {
        plugin_id: String,
    },
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// A decision for one tool approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
    AllowOnce,
    AllowAlways,
    Deny,
}

/// User-provided content for starting or steering a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UserContent {
    Text {
        text: String,
    },
    Image {
        mime_type: String,
        data_base64: String,
        display_name: String,
    },
}

/// Sandbox policy applied to agent tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SandboxMode {
    WorkspaceWrite,
    ReadOnly,
    Off,
}

/// Session-scoped policy for approving mutating tool calls.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalMode {
    /// Ask before every mutating or destructive tool call.
    #[default]
    Request,
    /// Auto-approve ordinary mutations and ask only for sandbox escapes.
    Auto,
    /// Auto-approve every tool call, including sandbox escapes.
    Full,
}

/// Runtime options exposed to an application client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOptions {
    pub models: Vec<String>,
    pub active_model: Option<String>,
    pub effort: Option<String>,
    #[serde(default)]
    pub approval_mode: ApprovalMode,
    pub sandbox_mode: SandboxMode,
    pub sandbox_network: bool,
}

/// A workspace location that can be local or owned by another Zode node.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct WorkspaceUri(String);

impl WorkspaceUri {
    /// Constructs a URI after validating its supported scheme.
    pub fn new(value: impl Into<String>) -> Result<Self, ProtocolError> {
        Self::try_from(value.into())
    }

    /// Returns the validated URI string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for WorkspaceUri {
    type Error = ProtocolError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if is_valid_workspace_uri(&value) {
            Ok(Self(value))
        } else {
            Err(ProtocolError::InvalidWorkspaceUri(value))
        }
    }
}

fn is_valid_workspace_uri(value: &str) -> bool {
    if value.chars().any(char::is_whitespace) {
        return false;
    }

    if let Some(path) = value.strip_prefix("file://") {
        return path.starts_with('/');
    }

    value
        .strip_prefix("zode-node://")
        .and_then(|location| location.split_once('/'))
        .is_some_and(|(authority, workspace_path)| {
            !authority.is_empty() && !workspace_path.is_empty()
        })
}

impl<'de> Deserialize<'de> for WorkspaceUri {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(D::Error::custom)
    }
}

/// Current execution status for a session summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadStatus {
    Idle,
    Running,
    Failed,
}

/// Lightweight metadata for listing sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub session: SessionLocator,
    pub workspace_uri: WorkspaceUri,
    pub title: String,
    pub updated_at_ms: i64,
    pub status: ThreadStatus,
}

/// Display-safe persisted content for restoring one conversation transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadHistory {
    pub session: SessionLocator,
    pub items: Vec<HistoryItem>,
}

/// A renderable persisted transcript entry independent of any desktop UI model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HistoryItem {
    UserText { text: String },
    AssistantText { text: String },
    Thinking { text: String },
    Tool { tool: ToolCall },
    Status { code: String, message: String },
    Error { message: String, retryable: bool },
}

/// Status of one file in a diff snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
}

/// Summary counts for one changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    pub status: DiffFileStatus,
    pub additions: u32,
    pub deletions: u32,
}

/// Full diff state for a session at one point in time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffSnapshot {
    pub session: SessionLocator,
    pub files: Vec<DiffFile>,
    pub unified: String,
}

/// Lifecycle status for a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolStatus {
    Running,
    Completed,
    Failed,
}

/// Tool call data carried by lifecycle events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub status: ToolStatus,
    pub summary: String,
    pub detail: Option<String>,
}

/// Lifecycle status for one `Task`-spawned sub-agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubagentStatus {
    Running,
    Completed,
    Failed,
}

/// A point-in-time view of one `Task`-spawned sub-agent, carried by
/// `AgentEventKind::SubagentUpdate`. `id` is the host registry's stable
/// identity for this sub-agent (not the `Task` tool call's own id — the two
/// are allocated independently).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSnapshot {
    pub id: String,
    pub agent_type: String,
    /// Human-readable task label for the M2 sub-agent panel row title — the
    /// `Task` tool call's own `description` argument, falling back to
    /// `agent_type` when the model didn't supply one.
    pub display_name: String,
    pub depth: u8,
    pub status: SubagentStatus,
    pub tokens: u64,
    pub turn_id: TurnId,
    /// Wall-clock stamp of the terminal status transition (`Completed` or
    /// `Failed`), in epoch milliseconds. `None` while `status == Running`.
    pub completed_at_ms: Option<i64>,
    /// First line of the agent's final answer, truncated for display
    /// (~120 chars). Extracted once at the terminal transition, never
    /// streamed mid-run. `None` while running or after a failure.
    pub result_summary: Option<String>,
    /// Model this sub-agent runs under, driving the panel's per-agent
    /// disclosure. Resolved host-side from the agent definition's `model:`
    /// override, falling back to the session's active model; `None` when the
    /// producer cannot determine either. `#[serde(default)]` so snapshots
    /// written before this field existed still deserialize.
    #[serde(default)]
    pub model: Option<String>,
}

/// Lifecycle status for one host-tracked background process (a shell
/// launched by the `BashRun` tool). `Starting` and `Stopping` are transient
/// states this wire type can represent for Codex parity, but the current
/// desktop producer (`zode_core::bg_shells::BackgroundShellTracker`) only
/// ever observes a shell after it has started and after a kill has already
/// completed, so it never emits those two variants — see
/// `EventNormalizer::diff_background_processes` in zode-app-runtime for the
/// exact mapping and the seam left for a richer host-side tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundProcessStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    NotFound,
}

/// A point-in-time view of one background process, carried by
/// `AgentEventKind::BackgroundProcessUpdate`. `id` is the host's stable
/// shell identity (`BashRun`'s `shell_id`), independent from any tool
/// call's own id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundProcessSnapshot {
    pub id: String,
    pub command: String,
    pub status: BackgroundProcessStatus,
    pub started_at_ms: i64,
    /// The transcript `ToolCall.id` for the `BashRun` card that launched
    /// this shell, when the normalizer could associate the two. `None`
    /// when the launch predates this session's normalizer (e.g. restored
    /// history), so "view output" has nothing to jump to.
    pub tool_call_id: Option<String>,
}

/// Token, context, and optional cost measurements for a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,

    /// Fraction of the model context window consumed, in the range 0.0..=1.0.
    pub context_used: Option<f32>,
    pub cost_usd: Option<f64>,
}

/// A versioned, ordered event emitted by a Zode node.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub version: u16,
    pub session: SessionLocator,
    pub turn_id: TurnId,

    /// A monotonically increasing sequence number for this `SessionLocator`
    /// across all of its turns.
    pub sequence: u64,

    #[serde(flatten)]
    pub kind: AgentEventKind,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentEventWire {
    version: u16,
    session: SessionLocator,
    turn_id: TurnId,
    sequence: u64,
    #[serde(flatten)]
    kind: AgentEventKind,
}

impl AgentEvent {
    /// Decodes and validates one event from its JSON wire representation.
    pub fn decode_json(input: &str) -> Result<Self, ProtocolError> {
        let wire: AgentEventWire = serde_json::from_str(input)?;
        Self::from_wire(wire)
    }

    fn from_wire(wire: AgentEventWire) -> Result<Self, ProtocolError> {
        let event = Self {
            version: wire.version,
            session: wire.session,
            turn_id: wire.turn_id,
            sequence: wire.sequence,
            kind: wire.kind,
        };
        event.validate()?;
        Ok(event)
    }

    /// Validates the event's protocol version.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_version(self.version)
    }
}

impl<'de> Deserialize<'de> for AgentEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = AgentEventWire::deserialize(deserializer)?;
        Self::from_wire(wire).map_err(D::Error::custom)
    }
}

/// Events understood by protocol version 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentEventKind {
    TextDelta {
        delta: String,
    },
    ThinkingDelta {
        delta: String,
    },
    ToolStarted {
        tool: ToolCall,
    },
    ToolCompleted {
        tool: ToolCall,
    },
    ApprovalRequested {
        approval_id: String,
        tool: String,
        summary: String,
    },
    /// A `Task`-spawned sub-agent changed state (started, made progress, or
    /// reached a terminal status). Kept independent from `ToolStarted`/
    /// `ToolCompleted` so existing tool-card consumers are unaffected.
    SubagentUpdate {
        subagent: SubagentSnapshot,
    },
    /// A background shell (started by the `BashRun` tool) changed state.
    /// Kept independent from `ToolStarted`/`ToolCompleted` for the same
    /// reason as `SubagentUpdate`.
    BackgroundProcessUpdate {
        process: BackgroundProcessSnapshot,
    },
    DiffInvalidated,
    Usage {
        usage: UsageSnapshot,
    },
    StatusNotice {
        code: String,
        message: String,
    },
    TurnFinished {
        interrupted: bool,
    },
    Error {
        message: String,
        retryable: bool,
    },
    /// An event added by a future protocol version. Callers must diagnose and
    /// ignore it rather than interpreting it as a successful operation.
    #[serde(other)]
    Unknown,
}

fn validate_version(version: u16) -> Result<(), ProtocolError> {
    if version == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ProtocolError::UnsupportedVersion {
            expected: PROTOCOL_VERSION,
            actual: version,
        })
    }
}
