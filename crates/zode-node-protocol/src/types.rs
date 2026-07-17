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
        workspace_uri: WorkspaceUri,
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
    SetModel {
        model: String,
    },
    SetEffort {
        effort: String,
    },
    SetSandbox {
        mode: SandboxMode,
        network: bool,
    },
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

/// Runtime options exposed to an application client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOptions {
    pub models: Vec<String>,
    pub active_model: Option<String>,
    pub effort: Option<String>,
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

    /// A monotonically increasing sequence number within this turn.
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
