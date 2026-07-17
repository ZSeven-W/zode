use crate::{
    AttachmentMetadata, LayoutClass, LoadState, MessageQueueState, PresentationState,
    TranscriptState,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use zode_node_protocol::{
    ApprovalMode, CapabilityManifest, NodeId, RuntimeOptions, SandboxMode, SessionLocator,
    ThreadSummary, TurnId, UsageSnapshot, WorkspaceUri,
};

/// Menu anchored to one of the interactive composer context chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerContextMenu {
    Location,
    Branch,
}

/// Menu anchored to one of the controls in the composer footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerFooterMenu {
    Add,
    Permission,
    Model,
    ModelModels,
    ModelEffort,
    ModelSpeed,
}

/// Where a newly submitted task should run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TaskLaunchMode {
    #[default]
    Local,
    Worktree,
}

/// Truthful branch data loaded for exactly one workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchCatalog {
    pub workspace_uri: WorkspaceUri,
    pub current: String,
    pub branches: Vec<String>,
    pub dirty_files: usize,
}

/// Workspace-scoped asynchronous state for the branch picker.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum BranchCatalogState {
    #[default]
    Idle,
    Loading {
        workspace_uri: WorkspaceUri,
    },
    Switching {
        workspace_uri: WorkspaceUri,
        from: String,
        branch: String,
    },
    Ready(BranchCatalog),
    Failed {
        workspace_uri: WorkspaceUri,
        message: String,
    },
}

/// Search and catalog state owned by the branch context menu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchPickerState {
    pub query: String,
    pub catalog: BranchCatalogState,
}

/// Editable composer settings for the current session.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposerState {
    pub draft: String,
    /// Lightweight attachment projection only. Encoded payloads stay in the controller.
    pub attachments: Vec<AttachmentMetadata>,
    pub focused: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub sandbox_label: String,
    pub approval_mode: ApprovalMode,
    pub sandbox_mode: SandboxMode,
    pub sandbox_network: bool,
    pub available_models: Vec<String>,
    pub footer_menu: Option<ComposerFooterMenu>,
    pub queue_menu: Option<crate::QueuedMessageId>,
    pub editing_queued_message: Option<crate::QueuedMessageId>,
    pub draft_before_queue_edit: Option<String>,
    pub context_menu: Option<ComposerContextMenu>,
    pub launch_mode: TaskLaunchMode,
    pub branch_picker: BranchPickerState,
    /// Current branch confirmed by Git. The UI never changes this
    /// optimistically while a checkout is still in flight.
    pub selected_branch: Option<String>,
}

impl Default for ComposerState {
    fn default() -> Self {
        Self {
            draft: String::new(),
            attachments: Vec::new(),
            focused: false,
            model: None,
            effort: None,
            sandbox_label: String::new(),
            approval_mode: ApprovalMode::Request,
            sandbox_mode: SandboxMode::WorkspaceWrite,
            sandbox_network: false,
            available_models: Vec::new(),
            footer_menu: None,
            queue_menu: None,
            editing_queued_message: None,
            draft_before_queue_edit: None,
            context_menu: None,
            launch_mode: TaskLaunchMode::default(),
            branch_picker: BranchPickerState::default(),
            selected_branch: None,
        }
    }
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

/// Projected state for the task-title rename dialog. The native controller
/// owns cursor/selection/IME details while this value keeps paint, hit testing,
/// and accessibility on the same immutable snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRenameState {
    pub session: SessionLocator,
    pub draft: String,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPreferences {
    pub theme: ThemePreference,
    pub reduced_motion: bool,
    pub high_contrast: bool,
    #[serde(default = "default_enabled")]
    pub task_suggestions: bool,
    #[serde(default = "default_enabled")]
    pub sidebar_tasks_expanded: bool,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            theme: ThemePreference::default(),
            reduced_motion: false,
            high_contrast: false,
            task_suggestions: true,
            sidebar_tasks_expanded: true,
        }
    }
}

const fn default_enabled() -> bool {
    true
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProjectPickerAnchor {
    #[default]
    Welcome,
    Composer,
}

/// Transient state for the project switcher shown on a new-task surface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectPickerState {
    pub open: bool,
    pub anchor: ProjectPickerAnchor,
    pub search: String,
    pub active_index: usize,
}

/// Sidebar section whose overflow menu is currently visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSectionMenu {
    Projects,
    Tasks,
}

/// Whether project tasks are nested beneath their workspace or shown together.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectDisplayMode {
    #[default]
    Grouped,
    Flat,
}

/// Ordering applied to projects and their task projections in the sidebar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectSortMode {
    #[default]
    Priority,
    RecentlyUpdated,
    Manual,
}

/// Transient navigation state for the desktop sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarState {
    pub scroll_offset: f32,
    pub tasks_expanded: bool,
    pub show_all_projects: bool,
    pub show_all_project_sessions: BTreeSet<WorkspaceUri>,
    pub project_menu: Option<WorkspaceUri>,
    pub section_menu: Option<SidebarSectionMenu>,
    pub project_display_mode: ProjectDisplayMode,
    pub project_sort_mode: ProjectSortMode,
    pub pinned_projects: BTreeSet<WorkspaceUri>,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            scroll_offset: 0.0,
            tasks_expanded: true,
            show_all_projects: false,
            show_all_project_sessions: BTreeSet::new(),
            project_menu: None,
            section_menu: None,
            project_display_mode: ProjectDisplayMode::default(),
            project_sort_mode: ProjectSortMode::default(),
            pinned_projects: BTreeSet::new(),
        }
    }
}

/// Display metadata for the local desktop profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalProfileState {
    pub display_name: String,
}

impl Default for LocalProfileState {
    fn default() -> Self {
        Self {
            display_name: "本地".into(),
        }
    }
}

/// One truthful label/value pair loaded from local desktop configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSettingFact {
    pub label: String,
    pub value: String,
}

/// Read-only local facts used by settings pages that do not yet expose an
/// editor. Each load state remains explicit so the UI never invents defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalSettingsState {
    pub configuration: crate::LoadState<Vec<LocalSettingFact>>,
    pub browser: crate::LoadState<Vec<LocalSettingFact>>,
    pub hooks: crate::LoadState<Vec<LocalSettingFact>>,
    pub git: crate::LoadState<Vec<LocalSettingFact>>,
    pub worktrees: crate::LoadState<Vec<LocalSettingFact>>,
}

/// Transient controls for the local archived-task browser.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchivedTasksState {
    pub search: String,
    pub workspace_filter: Option<WorkspaceUri>,
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
    pub local_profile: LocalProfileState,
    pub projects: Vec<ProjectState>,
    pub active_workspace: Option<WorkspaceUri>,
    pub project_picker: ProjectPickerState,
    /// Hidden local cwd used by sessions that are not attached to a user project.
    pub projectless_workspace_root: Option<WorkspaceUri>,
    pub current_session: Option<SessionLocator>,
    /// Session whose task-actions menu is currently visible in the thread header.
    pub session_menu: Option<SessionLocator>,
    /// Session whose copy-information submenu is open.
    pub session_copy_menu: Option<SessionLocator>,
    /// Active task-title rename dialog, if any.
    pub session_rename: Option<SessionRenameState>,
    pub pending_session_delete: Option<SessionLocator>,
    pub threads: Vec<ThreadSummary>,
    pub pinned_sessions: BTreeSet<SessionLocator>,
    pub archived_sessions: BTreeSet<SessionLocator>,
    pub sidebar: SidebarState,
    pub transcripts: BTreeMap<SessionLocator, TranscriptState>,
    pub message_queues: BTreeMap<SessionLocator, MessageQueueState>,
    pub active_turns: BTreeMap<SessionLocator, TurnId>,
    pub approvals: BTreeMap<String, SessionLocator>,
    pub tool_expanded: BTreeMap<SessionLocator, BTreeMap<String, bool>>,
    pub project_permissions: BTreeMap<WorkspaceUri, LoadState<Vec<String>>>,
    pub settings_search: String,
    pub settings_scroll_offset: f32,
    pub local_settings: LocalSettingsState,
    pub archived_tasks: ArchivedTasksState,
    pub composer: ComposerState,
    /// Global runtime defaults used by new tasks and the composer reset action.
    pub composer_defaults: Option<RuntimeOptions>,
    /// Interactive bootstrap detected that provider credentials are still absent.
    pub provider_setup_required: bool,
    pub usage: BTreeMap<SessionLocator, UsageSnapshot>,
    pub presentation: PresentationState,
    pub review: ReviewState,
    pub terminal: TerminalState,
    pub ui_preferences: UiPreferences,
    pub shell: ShellState,
}

impl ZodeAppState {
    pub fn close_session_action_surfaces(&mut self) {
        self.session_menu = None;
        self.session_copy_menu = None;
        self.session_rename = None;
        self.sidebar.project_menu = None;
        self.sidebar.section_menu = None;
        self.composer.context_menu = None;
        self.composer.footer_menu = None;
    }

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

    pub fn terminal_surface_visible(&self) -> bool {
        self.presentation.route == crate::ShellRoute::Terminal
            || (self.presentation.route == crate::ShellRoute::Conversation
                && self.presentation.secondary_pane == Some(crate::SecondaryPane::Terminal))
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
        local_profile: LocalProfileState::default(),
        projects: Vec::new(),
        active_workspace: None,
        project_picker: ProjectPickerState::default(),
        projectless_workspace_root: None,
        current_session: None,
        session_menu: None,
        session_copy_menu: None,
        session_rename: None,
        pending_session_delete: None,
        threads: Vec::new(),
        pinned_sessions: BTreeSet::new(),
        archived_sessions: BTreeSet::new(),
        sidebar: SidebarState::default(),
        transcripts: BTreeMap::new(),
        message_queues: BTreeMap::new(),
        active_turns: BTreeMap::new(),
        approvals: BTreeMap::new(),
        tool_expanded: BTreeMap::new(),
        project_permissions: BTreeMap::new(),
        settings_search: String::new(),
        settings_scroll_offset: 0.0,
        local_settings: LocalSettingsState::default(),
        archived_tasks: ArchivedTasksState::default(),
        composer: ComposerState::default(),
        composer_defaults: None,
        provider_setup_required: false,
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
