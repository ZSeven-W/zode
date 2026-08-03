use zode_node_protocol::{
    ApprovalDecision, ApprovalMode, SandboxMode, SessionLocator, UserContent, WorkspaceUri,
};

use crate::{
    BranchCatalog, ComposerContextMenu, ComposerFooterMenu, ComputerPermissionKind,
    EnvironmentActionKind, ExternalApplication, ExternalApplicationCatalog, IntegrationScope,
    IntegrationsTab, MessageFeedback, ProjectDisplayMode, ProjectSortMode, ProviderDraft,
    ProviderKindChoice, ProviderSummary, QueuedMessageId, SecondaryPane, SettingsCategory,
    ShellRoute, SidebarSectionMenu, TaskLaunchMode, ThemePreference,
};

/// Which end of the queue a reorder action targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueReorderOp {
    /// Move to the head of the queue (置顶/插队).
    Front,
    /// Swap with the predecessor (上移).
    Up,
    /// Swap with the successor (下移).
    Down,
}

/// User intent emitted by widgets for the application controller to handle.
#[derive(Debug, Clone, PartialEq)]
pub enum AppCommand {
    BeginTask {
        workspace_uri: Option<WorkspaceUri>,
    },
    ToggleProjectPicker,
    ToggleComposerProjectPicker,
    ToggleSidebarProjectPicker,
    CloseProjectPicker,
    SetProjectSearch(String),
    SetProjectPickerActive(usize),
    ToggleGlobalSearch,
    CloseGlobalSearch,
    SetGlobalSearchQuery(String),
    SetGlobalSearchActive(usize),
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
    ExternalApplicationsLoaded(ExternalApplicationCatalog),
    ExternalApplicationsFailed(String),
    SelectExternalApplication {
        application: ExternalApplication,
    },
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
    /// Backfills an `TranscriptItem::Image`'s natural dimensions once the
    /// host-side decode learns them, so the lightbox can scale zoom steps
    /// against the real size. Addressed by `item_id` (not index) so a stale
    /// in-flight decode can never mutate a different item after edits.
    SetTranscriptImageDimensions {
        session: SessionLocator,
        item_id: String,
        width: u32,
        height: u32,
    },
    /// Opens the full-size lightbox overlay for one `TranscriptItem::Image`,
    /// addressed by its stable `id`. Ignored if that item is not present in
    /// `session`'s transcript.
    OpenLightbox {
        session: SessionLocator,
        item_id: String,
    },
    CloseLightbox,
    /// Steps the open lightbox's zoom to the next/previous
    /// `LIGHTBOX_ZOOM_STEPS` entry. Ignored when no lightbox is open.
    StepLightboxZoom {
        increase: bool,
    },
    /// Opens the in-conversation find bar above `session`'s transcript
    /// (Cmd+F on macOS, Ctrl+F elsewhere). Ignored when it is already open.
    OpenTranscriptFind {
        session: SessionLocator,
    },
    /// Closes the find bar and clears its query, dropping every highlight.
    CloseTranscriptFind {
        session: SessionLocator,
    },
    /// Replaces the find query. Resets the active match to the first hit and
    /// scrolls to it.
    SetTranscriptFindQuery {
        session: SessionLocator,
        query: String,
    },
    /// Steps to the next (`forward`) or previous match, wrapping at both
    /// ends, and scrolls the transcript to it.
    StepTranscriptFindMatch {
        session: SessionLocator,
        forward: bool,
    },
    SetToolExpanded {
        session: SessionLocator,
        tool_id: String,
        expanded: bool,
    },
    SetMessageFeedback {
        session: SessionLocator,
        index: usize,
        feedback: MessageFeedback,
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
    LoadProviderModels,
    ProviderModelsLoaded(Vec<ProviderSummary>),
    ProviderModelsLoadFailed(String),
    BeginProviderEdit {
        draft: ProviderDraft,
        credential_configured: bool,
    },
    CancelProviderEdit,
    UpdateProviderDraft(ProviderDraft),
    SetProviderDraftKind(ProviderKindChoice),
    SetProviderDraftBaseUrl(Option<String>),
    SetProviderDraftModelIds(Vec<String>),
    SelectProviderDefaultModel(String),
    RequestSaveProvider,
    ProviderSaveSucceeded(ProviderSummary),
    ProviderSaveFailed {
        provider_id: String,
        message: String,
    },
    RequestRemoveProvider {
        provider_id: String,
    },
    ProviderRemoveSucceeded {
        provider_id: String,
    },
    ProviderRemoveFailed {
        provider_id: String,
        message: String,
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
        /// `true` for priority send: enqueues at the head of the queue
        /// instead of the tail.
        front: bool,
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
    ReorderQueuedMessage {
        session: SessionLocator,
        id: QueuedMessageId,
        op: QueueReorderOp,
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
    /// Reload provider credentials and model groups after the desktop settings
    /// surface atomically persists the global configuration.
    ReloadProviderConfiguration,
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
    /// Opens the named System Settings privacy pane so the user can grant
    /// (or review) a computer-use TCC permission. Handled entirely as a
    /// platform side effect - see `consume_computer_use_command` in the
    /// desktop app crate.
    OpenComputerUsePermissionSettings(ComputerPermissionKind),
    /// Toggles the `computer` tool group's config-backed `enabled` flag.
    SetComputerToolEnabled(bool),
    /// Toggles the config-backed "Any App" blanket allowlist bypass. See
    /// `zode_core::config::ComputerConfig::any_app` for the current
    /// (not-yet-gate-enforced) scope of this setting.
    SetComputerAnyApp(bool),
    /// Live text buffer for the allowed-apps add row, distinct from the
    /// persisted `allowed_apps` list itself.
    SetComputerAllowedAppInput(String),
    AddComputerAllowedApp(String),
    RemoveComputerAllowedApp(String),
    TogglePrimarySidebar,
    SetPrimarySidebarWidth(u16),
    ToggleSidebar,
    SetSecondarySidebarWidth(u16),
    Navigate(ShellRoute),
    OpenSecondary(SecondaryPane),
    CloseSecondary,
    /// Expands the M2 sub-agent panel's "完成" (completed) section by one
    /// more page (`SUBAGENTS_PAGE_SIZE`) - the "再显示 N 个" button. Applied
    /// entirely locally by `reduce_presentation_command`, never reaching the
    /// endpoint command pump.
    ShowMoreCompletedSubagents {
        session: SessionLocator,
    },
    SetPinnedSummaryAutoHidden(bool),
    SetPinnedSummaryOverlayOpen(bool),
    SelectSettingsCategory(SettingsCategory),
    SelectIntegrationsTab(IntegrationsTab),
    SetIntegrationSearch(String),
    SetIntegrationScope(IntegrationScope),
    SetIntegrationsScroll {
        offset: f32,
    },
    SetIntegrationEnabled {
        workspace_uri: WorkspaceUri,
        source_id: String,
        enabled: bool,
    },
    SetPluginAddOpen(bool),
    SetPluginAddSpec(String),
    SetPluginAddReference(String),
    /// Reads `spec`/`reference` from `presentation.plugin_add` and dispatches
    /// `AgentCommandKind::InstallPlugin` on the endpoint command pump - the
    /// same background task every other endpoint command already runs on, so
    /// the (network-bound) git clone never blocks the UI thread.
    InstallPlugin,
    OpenPluginDetail(String),
    ClosePluginDetail,
    RequestUninstallPlugin,
    CancelUninstallPlugin,
    ConfirmUninstallPlugin,
    /// Opens the trust-review screen for the plugin currently shown in
    /// `presentation.plugin_detail` and fetches its pending items' verbatim
    /// content via `AgentQuery::PluginTrustReview`.
    RequestPluginTrustReview,
    CancelPluginTrustReview,
    ToggleTrustItemSelected(String),
    /// `keys: None` grants every pending item ("全部信任"); `Some` grants
    /// only the named ones ("信任" on a single row).
    GrantPluginTrust {
        keys: Option<Vec<String>>,
    },
    /// Starts an update check for the plugin in `presentation.plugin_detail`:
    /// the reducer parks the overlay in `PluginUpdateState::Checking` and the
    /// desktop crate fetches the answer via
    /// `PresentationQuery::PluginUpdateCheck`.
    CheckPluginUpdate,
    /// Applies the update reported by the last `CheckPluginUpdate`: pulls the
    /// plugin to its ref's remote tip, re-scans capabilities, and refreshes
    /// the installed list so changed hooks/MCP commands re-enter the trust
    /// gate before they can run again.
    ApplyPluginUpdate,
    RunEnvironmentAction {
        session: SessionLocator,
        action: EnvironmentActionKind,
    },
    /// Arms (or re-arms) a background-process row's stop confirmation - the
    /// row's action becomes a one-tap "确认停止" until either
    /// `StopBackgroundProcess` fires or a different row is armed/cleared.
    /// Never itself kills anything.
    ArmBackgroundProcessStop {
        session: SessionLocator,
        process_id: String,
    },
    /// Kills one background shell. Only meaningful immediately after that
    /// exact process was armed via `ArmBackgroundProcessStop` - the
    /// consumer re-checks the arm before dispatching the kill.
    StopBackgroundProcess {
        session: SessionLocator,
        process_id: String,
    },
    /// Jumps the transcript to the `BashRun` tool card that launched a
    /// background process ("查看输出"). A no-op when the process predates
    /// this session's normalizer and carries no `tool_call_id`.
    ViewBackgroundProcessOutput {
        session: SessionLocator,
        tool_call_id: String,
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
