use zode_node_protocol::{
    ApprovalDecision, SandboxMode, SessionLocator, UserContent, WorkspaceUri,
};

/// User intent emitted by widgets for the application controller to handle.
#[derive(Debug, Clone, PartialEq)]
pub enum AppCommand {
    NewSession {
        workspace_uri: WorkspaceUri,
    },
    SelectSession(SessionLocator),
    RenameSession {
        session: SessionLocator,
        title: String,
    },
    SetSessionPinned {
        session: SessionLocator,
        pinned: bool,
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
        tool_id: String,
        expanded: bool,
    },
    SetProjectPermissions {
        workspace_uri: WorkspaceUri,
        tools: Vec<String>,
    },
    RequestDeleteSession(SessionLocator),
    CancelDeleteSession,
    DeleteSession(SessionLocator),
    ToggleProject(WorkspaceUri),
    Submit(Vec<UserContent>),
    Steer(Vec<UserContent>),
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
    OpenWorkspaceFile {
        workspace_uri: WorkspaceUri,
        relative_path: String,
    },
    ToggleSidebar,
    OpenReview,
    OpenTerminal,
}
