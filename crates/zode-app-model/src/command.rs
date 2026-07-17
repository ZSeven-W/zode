use zode_node_protocol::{
    ApprovalDecision, SandboxMode, SessionLocator, UserContent, WorkspaceUri,
};

use crate::{
    IntegrationsTab, QueuedMessageId, SecondaryPane, SettingsCategory, ShellRoute, ThemePreference,
};

/// User intent emitted by widgets for the application controller to handle.
#[derive(Debug, Clone, PartialEq)]
pub enum AppCommand {
    BeginTask {
        workspace_uri: Option<WorkspaceUri>,
    },
    ToggleProjectPicker,
    CloseProjectPicker,
    SetProjectSearch(String),
    SetProjectPickerActive(usize),
    CreateProject,
    NewSession {
        workspace_uri: WorkspaceUri,
    },
    SelectSession(SessionLocator),
    ToggleSessionMenu {
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
    ToggleSidebar,
    Navigate(ShellRoute),
    OpenSecondary(SecondaryPane),
    CloseSecondary,
    SelectSettingsCategory(SettingsCategory),
    SelectIntegrationsTab(IntegrationsTab),
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
