use zode_node_protocol::{
    ApprovalDecision, ApprovalMode, SandboxMode, SessionLocator, UserContent, WorkspaceUri,
};

use crate::{
    BranchCatalog, ComposerContextMenu, ComposerFooterMenu, EnvironmentActionKind,
    ExternalApplication, IntegrationScope, IntegrationsTab, ProjectDisplayMode, ProjectSortMode,
    QueuedMessageId, SecondaryPane, SettingsCategory, ShellRoute, SidebarSectionMenu,
    TaskLaunchMode, ThemePreference,
};

/// User intent emitted by widgets for the application controller to handle.
#[derive(Debug, Clone, PartialEq)]
pub enum AppCommand {
    BeginTask {
        workspace_uri: Option<WorkspaceUri>,
    },
    ToggleProjectPicker,
    ToggleComposerProjectPicker,
    CloseProjectPicker,
    SetProjectSearch(String),
    SetProjectPickerActive(usize),
    ToggleComposerContextMenu(ComposerContextMenu),
    CloseComposerContextMenu,
    ToggleComposerFooterMenu(ComposerFooterMenu),
    CloseComposerFooterMenu,
    SelectTaskLaunchMode(TaskLaunchMode),
    SetBranchSearch(String),
    LoadBranches {
        workspace_uri: WorkspaceUri,
    },
    BranchesLoaded(BranchCatalog),
    BranchesFailed {
        workspace_uri: WorkspaceUri,
        message: String,
    },
    SelectBranch {
        workspace_uri: WorkspaceUri,
        branch: String,
    },
    CreateProject,
    NewSession {
        workspace_uri: WorkspaceUri,
    },
    SelectSession(SessionLocator),
    ToggleSessionMenu {
        session: SessionLocator,
    },
    ToggleSessionCopyMenu {
        session: SessionLocator,
    },
    BeginRenameSession {
        session: SessionLocator,
    },
    SetSessionRenameDraft {
        session: SessionLocator,
        draft: String,
    },
    CancelRenameSession {
        session: SessionLocator,
    },
    /// Contract reserved for the forthcoming side-task pane. The current UI
    /// exposes this action as disabled until the pane has a real host.
    OpenSessionInSidePane {
        session: SessionLocator,
    },
    OpenSessionInNewWindow {
        session: SessionLocator,
    },
    RenameSession {
        session: SessionLocator,
        title: String,
    },
    SetSessionPinned {
        session: SessionLocator,
        pinned: bool,
    },
    SetSessionArchived {
        session: SessionLocator,
        archived: bool,
    },
    SetSidebarScroll {
        offset: f32,
    },
    ToggleProjectMenu {
        workspace_uri: WorkspaceUri,
    },
    ToggleSidebarSectionMenu(SidebarSectionMenu),
    SetProjectPinned {
        workspace_uri: WorkspaceUri,
        pinned: bool,
    },
    OpenProjectInFinder {
        workspace_uri: WorkspaceUri,
    },
    ToggleOpenWithMenu,
    LoadExternalApplications,
    ExternalApplicationsLoaded(Vec<ExternalApplication>),
    ExternalApplicationsFailed(String),
    OpenWorkspaceExternally {
        workspace_uri: WorkspaceUri,
        application: ExternalApplication,
    },
    ArchiveProjectTasks {
        workspace_uri: WorkspaceUri,
    },
    SetProjectDisplayMode(ProjectDisplayMode),
    SetProjectSortMode(ProjectSortMode),
    ToggleSidebarTasks,
    ShowAllProjects,
    ShowAllProjectSessions {
        workspace_uri: WorkspaceUri,
    },
    SetTranscriptViewport {
        session: SessionLocator,
        scroll_offset: f32,
        follow_tail: bool,
    },
    SetTranscriptItemHeight {
        session: SessionLocator,
        index: usize,
        height: f32,
    },
    SetToolExpanded {
        session: SessionLocator,
        tool_id: String,
        expanded: bool,
    },
    SetProjectPermissions {
        workspace_uri: WorkspaceUri,
        tools: Vec<String>,
    },
    SetThemePreference(ThemePreference),
    SetReducedMotion(bool),
    SetHighContrast(bool),
    SetTaskSuggestions(bool),
    SetSidebarTasksExpanded(bool),
    SetSettingsSearch(String),
    SetArchivedTaskSearch(String),
    SetArchivedTaskWorkspaceFilter(Option<WorkspaceUri>),
    SetSettingsScroll {
        offset: f32,
    },
    RequestDeleteSession(SessionLocator),
    CancelDeleteSession,
    DeleteSession(SessionLocator),
    ToggleProject(WorkspaceUri),
    Submit(Vec<UserContent>),
    Steer(Vec<UserContent>),
    EnqueueMessage {
        session: SessionLocator,
        content: Vec<UserContent>,
        attachments: Vec<crate::AttachmentMetadata>,
    },
    EditQueuedMessageText {
        session: SessionLocator,
        id: QueuedMessageId,
        text: String,
    },
    RemoveQueuedMessage {
        session: SessionLocator,
        id: QueuedMessageId,
    },
    ClearMessageQueue {
        session: SessionLocator,
    },
    ToggleQueuedMessageMenu {
        session: SessionLocator,
        id: QueuedMessageId,
    },
    BeginEditQueuedMessage {
        session: SessionLocator,
        id: QueuedMessageId,
    },
    CancelQueuedMessageEdit {
        session: SessionLocator,
    },
    GuideQueuedMessage {
        session: SessionLocator,
        id: QueuedMessageId,
    },
    DispatchNextQueuedMessage {
        session: SessionLocator,
    },
    Interrupt,
    Approve {
        id: String,
        decision: ApprovalDecision,
    },
    SetModel(String),
    SetEffort(String),
    SetSandbox {
        mode: SandboxMode,
        network: bool,
    },
    SetPermissionPreset {
        approval_mode: ApprovalMode,
        sandbox_mode: SandboxMode,
        network: bool,
    },
    ResetComposerRuntime,
    RetryLastTurn,
    RevokeProjectPermission {
        workspace_uri: WorkspaceUri,
        tool: String,
    },
    CopyText(String),
    PreviewWorkspaceFile {
        session: SessionLocator,
        relative_path: String,
    },
    OpenPreviewExternally {
        session: SessionLocator,
        relative_path: String,
    },
    TogglePrimarySidebar,
    SetPrimarySidebarWidth(u16),
    ToggleSidebar,
    SetSecondarySidebarWidth(u16),
    Navigate(ShellRoute),
    OpenSecondary(SecondaryPane),
    CloseSecondary,
    SetPinnedSummaryAutoHidden(bool),
    SetPinnedSummaryOverlayOpen(bool),
    SelectSettingsCategory(SettingsCategory),
    SelectIntegrationsTab(IntegrationsTab),
    SetIntegrationSearch(String),
    SetIntegrationScope(IntegrationScope),
    SetIntegrationEnabled {
        workspace_uri: WorkspaceUri,
        source_id: String,
        enabled: bool,
    },
    RunEnvironmentAction {
        session: SessionLocator,
        action: EnvironmentActionKind,
    },
    OpenReview,
    OpenTerminal,
    SetTerminalFocus(bool),
    SetTerminalScroll {
        offset: f32,
        follow_tail: bool,
    },
    WriteTerminal {
        id: zode_node_protocol::TerminalId,
        bytes: Vec<u8>,
    },
    ResizeTerminal {
        id: zode_node_protocol::TerminalId,
        cols: u16,
        rows: u16,
    },
    CloseTerminal(zode_node_protocol::TerminalId),
}
