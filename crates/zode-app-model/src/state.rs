use crate::{
    AttachmentMetadata, LayoutClass, LoadState, MessageQueueState, PresentationState,
    TranscriptState,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use zode_node_protocol::{
    CapabilityManifest, NodeId, SessionLocator, ThreadSummary, TurnId, UsageSnapshot, WorkspaceUri,
};

/// Editable composer settings for the current session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComposerState {
    pub draft: String,
    /// Lightweight attachment projection only. Encoded payloads stay in the controller.
    pub attachments: Vec<AttachmentMetadata>,
    pub focused: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub sandbox_label: String,
    pub queue_menu: Option<crate::QueuedMessageId>,
    pub editing_queued_message: Option<crate::QueuedMessageId>,
    pub draft_before_queue_edit: Option<String>,
}

impl ComposerState {
    pub fn begin_queue_edit(&mut self, id: crate::QueuedMessageId, text: &str) {
        if self.editing_queued_message.is_none() {
            self.draft_before_queue_edit = Some(self.draft.clone());
        }
        self.draft = text.to_owned();
        self.editing_queued_message = Some(id);
        self.queue_menu = None;
    }

    pub fn finish_queue_edit(&mut self) {
        if self.editing_queued_message.take().is_some() {
            self.draft = self.draft_before_queue_edit.take().unwrap_or_default();
        } else {
            self.draft_before_queue_edit = None;
        }
    }
}

/// Change-review panel state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewState {
    pub open: bool,
    pub dirty: bool,
}

/// Terminal panel availability and visibility.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalState {
    pub open: bool,
    pub unavailable_reason: Option<String>,
    pub active_id: Option<zode_node_protocol::TerminalId>,
    pub focused: bool,
    pub scroll_offset: f32,
    pub follow_tail: bool,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            open: false,
            unavailable_reason: None,
            active_id: None,
            focused: false,
            scroll_offset: 0.0,
            follow_tail: true,
        }
    }
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

/// User-selected color-scheme behavior. The host-observed system value stays separate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

/// Durable appearance and motion preferences shared by every render surface.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPreferences {
    pub theme: ThemePreference,
    pub reduced_motion: bool,
    pub high_contrast: bool,
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

/// Transient state for the project switcher shown on a new-task surface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectPickerState {
    pub open: bool,
    pub search: String,
    pub active_index: usize,
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
    pub active_workspace: Option<WorkspaceUri>,
    pub project_picker: ProjectPickerState,
    /// Hidden local cwd used by sessions that are not attached to a user project.
    pub projectless_workspace_root: Option<WorkspaceUri>,
    pub current_session: Option<SessionLocator>,
    pub pending_session_delete: Option<SessionLocator>,
    pub threads: Vec<ThreadSummary>,
    pub transcripts: BTreeMap<SessionLocator, TranscriptState>,
    pub message_queues: BTreeMap<SessionLocator, MessageQueueState>,
    pub active_turns: BTreeMap<SessionLocator, TurnId>,
    pub approvals: BTreeMap<String, SessionLocator>,
    pub tool_expanded: BTreeMap<SessionLocator, BTreeMap<String, bool>>,
    pub project_permissions: BTreeMap<WorkspaceUri, LoadState<Vec<String>>>,
    pub settings_scroll_offset: f32,
    pub composer: ComposerState,
    pub usage: BTreeMap<SessionLocator, UsageSnapshot>,
    pub presentation: PresentationState,
    pub review: ReviewState,
    pub terminal: TerminalState,
    pub ui_preferences: UiPreferences,
    pub shell: ShellState,
}

impl ZodeAppState {
    pub fn available_workspace(&self, workspace_uri: &WorkspaceUri) -> bool {
        self.projects
            .iter()
            .any(|project| &project.workspace_uri == workspace_uri && project.available)
    }

    pub fn available_workspace_for_session(
        &self,
        session: &SessionLocator,
    ) -> Option<&WorkspaceUri> {
        self.threads
            .iter()
            .find(|thread| &thread.session == session)
            .map(|thread| &thread.workspace_uri)
            .filter(|workspace_uri| {
                self.available_workspace(workspace_uri)
                    || self.is_projectless_workspace(workspace_uri)
            })
    }

    pub fn is_projectless_workspace(&self, workspace_uri: &WorkspaceUri) -> bool {
        self.projectless_workspace_root
            .as_ref()
            .is_some_and(|root| uri_is_same_or_descendant(root, workspace_uri))
    }

    pub fn active_available_workspace(&self) -> Option<&WorkspaceUri> {
        self.active_workspace
            .as_ref()
            .filter(|workspace_uri| self.available_workspace(workspace_uri))
    }

    pub fn current_session_presentation(&self) -> Option<&crate::SessionPresentationState> {
        self.current_session
            .as_ref()
            .and_then(|session| self.presentation.sessions.get(session))
    }
}

fn uri_is_same_or_descendant(root: &WorkspaceUri, candidate: &WorkspaceUri) -> bool {
    let root = normalized_workspace_uri(root.as_str());
    let candidate = normalized_workspace_uri(candidate.as_str());
    if has_ambiguous_path_segment(root) || has_ambiguous_path_segment(candidate) {
        return false;
    }
    candidate == root
        || if root.ends_with('/') {
            candidate.starts_with(root)
        } else {
            candidate
                .strip_prefix(root)
                .is_some_and(|suffix| suffix.starts_with('/'))
        }
}

fn has_ambiguous_path_segment(uri: &str) -> bool {
    let lower = uri.to_ascii_lowercase();
    lower.contains("%2e")
        || lower.contains("%2f")
        || lower.contains("%5c")
        || uri.split('/').any(|segment| matches!(segment, "." | ".."))
}

fn normalized_workspace_uri(uri: &str) -> &str {
    if uri == "file:///" {
        uri
    } else {
        uri.trim_end_matches('/')
    }
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
        active_workspace: None,
        project_picker: ProjectPickerState::default(),
        projectless_workspace_root: None,
        current_session: None,
        pending_session_delete: None,
        threads: Vec::new(),
        transcripts: BTreeMap::new(),
        message_queues: BTreeMap::new(),
        active_turns: BTreeMap::new(),
        approvals: BTreeMap::new(),
        tool_expanded: BTreeMap::new(),
        project_permissions: BTreeMap::new(),
        settings_scroll_offset: 0.0,
        composer: ComposerState::default(),
        usage: BTreeMap::new(),
        presentation: PresentationState::default(),
        review: ReviewState::default(),
        terminal: TerminalState::default(),
        ui_preferences: UiPreferences::default(),
        shell: ShellState {
            layout: LayoutClass::Wide,
            sidebar_open: true,
            page: ShellPage::Conversation,
        },
    }
}
