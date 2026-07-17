use crate::{LayoutClass, TranscriptState};
use std::collections::BTreeMap;
use zode_node_protocol::{
    CapabilityManifest, NodeId, SessionLocator, ThreadSummary, TurnId, UsageSnapshot, UserContent,
    WorkspaceUri,
};

/// Editable composer settings for the current session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComposerState {
    pub draft: String,
    pub attachments: Vec<UserContent>,
    pub focused: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub sandbox_label: String,
}

/// Change-review panel state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewState {
    pub open: bool,
    pub dirty: bool,
}

/// Terminal panel availability and visibility.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerminalState {
    pub open: bool,
    pub unavailable_reason: Option<String>,
}

/// Top-level shell destinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellPage {
    #[default]
    Conversation,
    Review,
    Terminal,
    Settings,
    ComingSoon,
}

/// Connection status for the endpoint backing this application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Local,
    Connecting,
    Unavailable,
}

/// System theme observed by the host platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTheme {
    Light,
    Dark,
}

/// State owned by the node hosting the current application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostState {
    pub node_id: NodeId,
    pub capabilities: CapabilityManifest,
    pub connection: ConnectionState,
    pub system_theme: SystemTheme,
}

/// Navigation metadata for one known workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectState {
    pub workspace_uri: WorkspaceUri,
    pub expanded: bool,
    pub available: bool,
    pub last_opened_ms: i64,
}

/// Responsive shell state shared by all pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellState {
    pub layout: LayoutClass,
    pub sidebar_open: bool,
    pub page: ShellPage,
}

/// The single source of truth read by desktop widgets.
#[derive(Debug, Clone, PartialEq)]
pub struct ZodeAppState {
    pub host: HostState,
    pub projects: Vec<ProjectState>,
    pub current_session: Option<SessionLocator>,
    pub pending_session_delete: Option<SessionLocator>,
    pub threads: Vec<ThreadSummary>,
    pub transcripts: BTreeMap<SessionLocator, TranscriptState>,
    pub active_turns: BTreeMap<SessionLocator, TurnId>,
    pub approvals: BTreeMap<String, SessionLocator>,
    pub tool_expanded: BTreeMap<String, bool>,
    pub project_permissions: BTreeMap<WorkspaceUri, Vec<String>>,
    pub composer: ComposerState,
    pub usage: BTreeMap<SessionLocator, UsageSnapshot>,
    pub review: ReviewState,
    pub terminal: TerminalState,
    pub shell: ShellState,
}

/// Creates a deterministic empty state for previews and reducer tests.
pub fn demo_state() -> ZodeAppState {
    let node_id = NodeId::parse("00000000-0000-0000-0000-000000000001")
        .expect("the demo node id is a valid UUID");

    ZodeAppState {
        host: HostState {
            node_id,
            capabilities: CapabilityManifest {
                node_id,
                capabilities: Default::default(),
            },
            connection: ConnectionState::Local,
            system_theme: SystemTheme::Light,
        },
        projects: Vec::new(),
        current_session: None,
        pending_session_delete: None,
        threads: Vec::new(),
        transcripts: BTreeMap::new(),
        active_turns: BTreeMap::new(),
        approvals: BTreeMap::new(),
        tool_expanded: BTreeMap::new(),
        project_permissions: BTreeMap::new(),
        composer: ComposerState::default(),
        usage: BTreeMap::new(),
        review: ReviewState::default(),
        terminal: TerminalState::default(),
        shell: ShellState {
            layout: LayoutClass::Wide,
            sidebar_open: true,
            page: ShellPage::Conversation,
        },
    }
}
