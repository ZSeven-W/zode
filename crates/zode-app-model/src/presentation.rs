use std::collections::{BTreeMap, HashMap};

use zode_node_protocol::{
    BackgroundProcessSnapshot, BackgroundProcessStatus, DiffSnapshot, InstalledPluginSummary,
    RuntimeOptions, SessionLocator, SubagentSnapshot, SubagentStatus, ToolCall, ToolStatus,
    WorkspaceUri,
};

#[path = "presentation-integrations.rs"]
mod integrations;
pub use integrations::*;

#[path = "presentation-plugin-market.rs"]
mod plugin_market;
pub use plugin_market::*;

/// `EnvironmentEntry::id` prefix for the Subagents section's single compact
/// row (Codex's "avatar strip + N 完成" affordance). The real id also
/// encodes up to [`MAX_SUBAGENT_AVATARS`] agent ids after a `:` so the
/// environment widget can hash each into a stable per-agent color without a
/// UI-specific field on the shared `EnvironmentEntry` type - see
/// [`is_subagents_summary_entry`] / [`subagents_summary_avatar_ids`].
pub const SUBAGENTS_SUMMARY_ENTRY_ID: &str = "subagents-summary";

/// How many avatar dots the compact Subagents row shows. Codex's reference
/// shows the first ~4; the rest are folded into the count text.
pub const MAX_SUBAGENT_AVATARS: usize = 4;

/// True for the Subagents section's one compact row - an exact match on
/// [`SUBAGENTS_SUMMARY_ENTRY_ID`] or that constant plus an encoded
/// `:<id1>,<id2>,...` avatar suffix.
pub fn is_subagents_summary_entry(entry_id: &str) -> bool {
    entry_id == SUBAGENTS_SUMMARY_ENTRY_ID
        || entry_id.starts_with(&format!("{SUBAGENTS_SUMMARY_ENTRY_ID}:"))
}

/// Parses the avatar ids encoded into a Subagents summary row's entry id.
/// Empty for any other id, including the bare unsuffixed constant.
pub fn subagents_summary_avatar_ids(entry_id: &str) -> Vec<&str> {
    entry_id
        .strip_prefix(SUBAGENTS_SUMMARY_ENTRY_ID)
        .and_then(|rest| rest.strip_prefix(':'))
        .map(|ids| ids.split(',').filter(|id| !id.is_empty()).collect())
        .unwrap_or_default()
}

/// Typed destinations rendered by the desktop shell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ShellRoute {
    #[default]
    Conversation,
    Terminal,
    Settings(SettingsCategory),
    Integrations(IntegrationsTab),
    ComingSoon(ComingSoonFeature),
}

impl ShellRoute {
    /// Projects a typed route into the legacy page enum during migration.
    pub const fn legacy_page(self) -> crate::ShellPage {
        match self {
            Self::Conversation => crate::ShellPage::Conversation,
            Self::Terminal => crate::ShellPage::Terminal,
            Self::Settings(_) => crate::ShellPage::Settings,
            Self::Integrations(_) | Self::ComingSoon(_) => crate::ShellPage::ComingSoon,
        }
    }
}

/// Settings destinations backed by real local application state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SettingsCategory {
    #[default]
    General,
    Profile,
    Appearance,
    Voice,
    Configuration,
    ProviderModels,
    Personalization,
    Pets,
    Permissions,
    KeyboardShortcuts,
    Usage,
    Account,
    AppSnapshots,
    Browser,
    ComputerUse,
    Hooks,
    Connectors,
    Git,
    Environment,
    Worktree,
    ArchivedTasks,
}

/// Integration catalog views available to the desktop shell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IntegrationsTab {
    #[default]
    Plugins,
    Skills,
}

/// Catalog ownership filter. Public entries require a verified directory
/// source; personal entries are capabilities discovered on this machine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IntegrationScope {
    Public,
    #[default]
    Personal,
}

/// Explicit placeholders for shell destinations that have no implementation yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ComingSoonFeature {
    #[default]
    ScheduledTasks,
    Sites,
    PullRequests,
    Chats,
    Help,
}

/// A single optional pane presented alongside the primary route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondaryPane {
    Environment,
    Review,
    DocumentPreview,
    Terminal,
    Browser,
    Files,
    SideTask,
    /// The M2 dedicated sub-agent panel (see
    /// `docs/proposals/subagent-panel-m2.md`) - opened from the environment
    /// card's compact Subagents row, not from `PanelPicker`'s home grid.
    Subagents,
}

/// One workspace-owned file target. Callers bind only a session and relative
/// path; the controller derives the workspace URI from canonical thread state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewTarget {
    pub workspace_uri: WorkspaceUri,
    pub relative_path: String,
}

/// Text rendering mode selected from the real file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Markdown,
    PlainText,
}

/// Session-isolated document preview state. Failure retains the exact target
/// so retry and external-open actions never reconstruct a path from UI text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PreviewState {
    #[default]
    Idle,
    Loading {
        target: PreviewTarget,
    },
    Ready {
        target: PreviewTarget,
        title: String,
        content: String,
        kind: PreviewKind,
    },
    Failed {
        target: PreviewTarget,
        message: String,
    },
}

impl PreviewState {
    pub const fn target(&self) -> Option<&PreviewTarget> {
        match self {
            Self::Loading { target } | Self::Ready { target, .. } | Self::Failed { target, .. } => {
                Some(target)
            }
            Self::Idle => None,
        }
    }

    pub fn path(&self) -> Option<&str> {
        self.target().map(|target| target.relative_path.as_str())
    }
}

/// Explicit asynchronous state that never substitutes placeholder content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LoadState<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> LoadState<T> {
    pub const fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            Self::Idle | Self::Loading | Self::Failed(_) => None,
        }
    }
}

/// How a Sources row's file entered the transcript, driving its activity
/// chip (Codex parity: created/provided/read/updated). Attributed in
/// [`transcript_sources`] from the tool call (or attachment) that touched
/// the path, in transcript order - see that function's doc comment for the
/// exact per-path state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceActivity {
    Created,
    Provided,
    Read,
    Updated,
}

impl SourceActivity {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Created => "已创建",
            Self::Provided => "已提供",
            Self::Read => "已读取",
            Self::Updated => "已更新",
        }
    }
}

/// One real item reported by a host context source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentEntry {
    pub id: String,
    pub label: String,
    pub value: Option<String>,
    /// Sources-only: how this file entered the transcript. `None` for
    /// every other section, and for a Sources aggregate row (see `count`).
    pub activity: Option<SourceActivity>,
    /// Sources-only: an aggregate row's count (e.g. web pages fetched, web
    /// searches run) instead of one path's activity. `None` for a normal
    /// per-file Sources row and for every other section.
    pub count: Option<u32>,
}

impl EnvironmentEntry {
    /// Constructs a plain label/value row with no Sources-specific detail -
    /// the shape every non-Sources section entry, and most Sources rows,
    /// need.
    pub fn new(id: impl Into<String>, label: impl Into<String>, value: Option<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value,
            activity: None,
            count: None,
        }
    }
}

/// Stable semantic groups rendered by the environment inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EnvironmentSectionKind {
    Changes,
    Host,
    Branch,
    RepositoryActions,
    Comparisons,
    Subagents,
    ComputerUse,
    BackgroundProcesses,
    Sources,
}

impl EnvironmentSectionKind {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Changes => "变更",
            Self::Host => "环境信息",
            Self::Branch => "分支",
            Self::RepositoryActions => "仓库操作",
            Self::Comparisons => "比较分支",
            Self::Subagents => "子智能体",
            Self::ComputerUse => "电脑操控",
            Self::BackgroundProcesses => "后台进程",
            Self::Sources => "来源",
        }
    }
}

/// One non-empty environment group projected from current-session facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentSection {
    pub kind: EnvironmentSectionKind,
    pub entries: Vec<EnvironmentEntry>,
}

/// Environment facts loaded for one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentSnapshot {
    pub workspace_uri: WorkspaceUri,
    pub branch: Option<String>,
    /// Superseded by `SessionPresentationState::background_processes` (the
    /// live `BackgroundProcessUpdate` feed) - `environment_sections` no
    /// longer reads this field. Kept only so existing fixtures that build
    /// an `EnvironmentSnapshot` literal keep compiling; always empty in
    /// production (`presentation_bridge::load_environment`).
    pub background_processes: Vec<EnvironmentEntry>,
    /// Superseded by `transcript_sources`, which derives Sources rows from
    /// the transcript directly. Never read; kept for the same reason as
    /// `background_processes` above.
    pub sources: Vec<EnvironmentEntry>,
}

/// Safe local repository intents exposed by the environment inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EnvironmentActionKind {
    RefreshStatus,
    CompareWorkspaceToHead,
    OpenWorkspace,
    CommitOrPush,
    /// Offered only while [`PullRequestStatus::NoPr`] is the current status.
    /// Like `CommitOrPush`, there is no safe write contract for actually
    /// calling `gh pr create` yet, so this is always disabled - the row
    /// exists to show the affordance and its (truthful) unavailable reason.
    CreatePullRequest,
    /// Offered only while [`PullRequestStatus::MergeConflicts`] is the
    /// current status. Unlike the other repository actions this one *is*
    /// wired up: it seeds a fix prompt into the composer for the agent to
    /// act on, rather than mutating the repository itself.
    FixMergeConflicts,
}

impl EnvironmentActionKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::RefreshStatus => "刷新状态",
            Self::CompareWorkspaceToHead => "比较工作区与 HEAD",
            Self::OpenWorkspace => "打开工作目录",
            Self::CommitOrPush => "提交或推送",
            Self::CreatePullRequest => "创建拉取请求",
            Self::FixMergeConflicts => "修复",
        }
    }
}

/// One check-suite rollup's aggregate state for the current pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksState {
    Successful,
    Failing,
    Pending,
    /// No checks are configured for this pull request.
    None,
}

impl ChecksState {
    pub const fn label(self) -> Option<&'static str> {
        match self {
            Self::Successful => Some("检查通过"),
            Self::Failing => Some("检查未通过"),
            Self::Pending => Some("检查进行中"),
            Self::None => Option::None,
        }
    }
}

/// The full Codex `gh`-backed pull-request status taxonomy for the current
/// workspace/branch, produced by zode-app's `gh` provider and projected
/// into the RepositoryActions area. See
/// `docs/proposals/right-panel-parity.md` section 1.1 for the state machine this
/// mirrors and `zode_app_runtime::git_status` for the provider that fills
/// [`SessionPullRequestState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestStatus {
    /// No pull request exists yet for the current branch.
    NoPr,
    /// The workspace has no configured git remote, so no PR flow applies -
    /// falls back to the existing branch-comparison action.
    NoRemote,
    /// An open pull request exists.
    Pr { number: u64, checks: ChecksState },
    /// The pull request cannot be merged as-is.
    MergeConflicts { number: u64 },
    /// `gh` is installed but the user is not authenticated.
    GhCliSignedOut,
    /// The `gh` CLI is not installed / not on `PATH`.
    GhCliUnavailable,
    /// `gh` is available and authenticated, but the status fetch itself
    /// failed (network error, API error, unexpected response, etc).
    Unavailable,
}

impl PullRequestStatus {
    /// The label of this status's one associated row.
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoPr | Self::Pr { .. } | Self::MergeConflicts { .. } => "拉取请求",
            Self::NoRemote => "无关联仓库",
            Self::GhCliSignedOut => "GitHub 无权限",
            Self::GhCliUnavailable => "GitHub CLI 不可用",
            Self::Unavailable => "拉取请求不可用",
        }
    }

    /// The row's trailing value text, when distinct from its label (a PR
    /// number and/or check state).
    pub fn value(self) -> Option<String> {
        match self {
            Self::Pr { number, checks } => Some(match checks.label() {
                Some(checks) => format!("#{number} · {checks}"),
                None => format!("#{number}"),
            }),
            Self::MergeConflicts { number } => Some(format!("#{number} · 无法合并")),
            Self::NoPr
            | Self::NoRemote
            | Self::GhCliSignedOut
            | Self::GhCliUnavailable
            | Self::Unavailable => None,
        }
    }
}

/// A typed reason why a repository action cannot be run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentActionUnavailableReason {
    NoCurrentTask,
    TaskUnavailable,
    ProjectTaskRequired,
    LocalWorkspaceRequired,
    StatusNotReady,
    SafeMutationContractUnavailable,
}

impl EnvironmentActionUnavailableReason {
    pub const fn message(self) -> &'static str {
        match self {
            Self::NoCurrentTask => "请选择任务",
            Self::TaskUnavailable => "任务不可用",
            Self::ProjectTaskRequired => "需要项目任务",
            Self::LocalWorkspaceRequired => "仅支持本地工作区",
            Self::StatusNotReady => "等待分支和变更状态",
            Self::SafeMutationContractUnavailable => "没有安全写入契约",
        }
    }
}

/// One environment action together with its truthful current availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvironmentAction {
    pub kind: EnvironmentActionKind,
    pub unavailable_reason: Option<EnvironmentActionUnavailableReason>,
}

impl EnvironmentAction {
    pub const fn enabled(self) -> bool {
        self.unavailable_reason.is_none()
    }
}

/// The current session's loaded `gh`-backed pull-request status, or `None`
/// while it is idle/loading/failed/not-a-session. Shared by
/// `environment_actions` and the environment widget's PR row layout so both
/// agree on exactly which status is "current" without re-deriving it.
pub fn current_pull_request_status(state: &crate::ZodeAppState) -> Option<PullRequestStatus> {
    state
        .current_session_presentation()
        .and_then(|presentation| presentation.pull_request.load.ready())
        .copied()
}

/// Projects repository action availability from the selected local task.
pub fn environment_actions(state: &crate::ZodeAppState) -> Vec<EnvironmentAction> {
    let base_reason = environment_workspace_unavailable_reason(state);
    let compare_reason = base_reason.or_else(|| {
        let presentation = state.current_session_presentation()?;
        let branch_ready = presentation
            .context
            .ready()
            .and_then(|context| context.branch.as_deref())
            .is_some_and(|branch| !branch.trim().is_empty());
        let diff_ready = presentation.diff.load.ready().is_some();
        (!branch_ready || !diff_ready).then_some(EnvironmentActionUnavailableReason::StatusNotReady)
    });
    let mut actions = vec![
        EnvironmentAction {
            kind: EnvironmentActionKind::RefreshStatus,
            unavailable_reason: base_reason,
        },
        EnvironmentAction {
            kind: EnvironmentActionKind::CompareWorkspaceToHead,
            unavailable_reason: compare_reason,
        },
        EnvironmentAction {
            kind: EnvironmentActionKind::OpenWorkspace,
            unavailable_reason: base_reason,
        },
        EnvironmentAction {
            kind: EnvironmentActionKind::CommitOrPush,
            unavailable_reason: Some(
                EnvironmentActionUnavailableReason::SafeMutationContractUnavailable,
            ),
        },
    ];
    match current_pull_request_status(state) {
        Some(PullRequestStatus::NoPr) => actions.push(EnvironmentAction {
            kind: EnvironmentActionKind::CreatePullRequest,
            unavailable_reason: Some(
                EnvironmentActionUnavailableReason::SafeMutationContractUnavailable,
            ),
        }),
        Some(PullRequestStatus::MergeConflicts { .. }) => actions.push(EnvironmentAction {
            kind: EnvironmentActionKind::FixMergeConflicts,
            unavailable_reason: base_reason,
        }),
        _ => {}
    }
    actions
}

fn environment_workspace_unavailable_reason(
    state: &crate::ZodeAppState,
) -> Option<EnvironmentActionUnavailableReason> {
    let Some(session) = state.current_session.as_ref() else {
        return Some(EnvironmentActionUnavailableReason::NoCurrentTask);
    };
    if session.session_id.starts_with("local-error-") || !state.transcripts.contains_key(session) {
        return Some(EnvironmentActionUnavailableReason::TaskUnavailable);
    }
    let Some(workspace) = state.available_workspace_for_session(session) else {
        return Some(EnvironmentActionUnavailableReason::TaskUnavailable);
    };
    if state.is_projectless_workspace(workspace) {
        return Some(EnvironmentActionUnavailableReason::ProjectTaskRequired);
    }
    if !workspace.as_str().starts_with("file://") {
        return Some(EnvironmentActionUnavailableReason::LocalWorkspaceRequired);
    }
    None
}

/// Diff loading and invalidation state for one session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionDiffState {
    pub dirty: bool,
    pub load: LoadState<DiffSnapshot>,
}

impl SessionDiffState {
    pub fn invalidate(&mut self) {
        self.dirty = true;
        self.load = LoadState::Loading;
    }
}

/// Pull-request status loading state for one session, mirroring
/// `SessionDiffState`. Lazy by design (see
/// `docs/proposals/right-panel-parity.md` section 1.1's poll policy): a session
/// starts `Idle` and stays there until the RepositoryActions section is
/// actually visible, unlike `context`/`diff` which load eagerly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionPullRequestState {
    pub dirty: bool,
    pub load: LoadState<PullRequestStatus>,
}

impl SessionPullRequestState {
    pub fn invalidate(&mut self) {
        self.dirty = true;
        self.load = LoadState::Loading;
    }
}

/// Data shown by presentation surfaces for one session only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionPresentationState {
    pub diff: SessionDiffState,
    pub context: LoadState<EnvironmentSnapshot>,
    pub preview: PreviewState,
    pub runtime_options: LoadState<RuntimeOptions>,
    /// Live `Task`-spawned sub-agents for this session, fed by
    /// `AgentEventKind::SubagentUpdate` and ordered by first appearance
    /// (not by id, since registry ids are not lexicographically stable).
    pub subagents: Vec<SubagentSnapshot>,
    /// `gh`-backed pull-request status for the current branch. See
    /// [`SessionPullRequestState`].
    pub pull_request: SessionPullRequestState,
    /// Live background shells (`BashRun` sessions) for this session, fed by
    /// `AgentEventKind::BackgroundProcessUpdate` and upserted by id -
    /// mirrors `subagents` above.
    pub background_processes: Vec<BackgroundProcessSnapshot>,
    /// The background-process id currently armed for a stop confirmation
    /// (the row's "停止" action becomes a one-tap "确认停止" until this is
    /// cleared - see `EnvironmentActionKind`-adjacent
    /// `AppCommand::ArmBackgroundProcessStop`/`StopBackgroundProcess` in
    /// `zode-app`). `None` when nothing is armed.
    pub armed_stop_process_id: Option<String>,
    /// How many rows of the M2 sub-agent panel's "完成" (completed) section
    /// are currently shown - `0` means "not yet expanded", which
    /// [`subagents_visible_count`] resolves to [`SUBAGENTS_PAGE_SIZE`] rather
    /// than storing that default directly, so a fresh session never needs
    /// special-casing. Incremented by `AppCommand::ShowMoreCompletedSubagents`.
    /// Kept per-session so paging through one task's history never leaks
    /// into another's.
    pub subagents_shown: usize,
}

/// Typed route, pane selection, and session-isolated presentation data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentationState {
    pub route: ShellRoute,
    /// Visibility of the resizable auxiliary sidebar. Pane selection is kept
    /// separately so hiding and restoring the sidebar never loses context.
    pub secondary_sidebar_open: bool,
    pub secondary_pane: Option<SecondaryPane>,
    /// Explicitly suppresses the otherwise automatic wide-screen summary.
    pub pinned_summary_auto_hidden: bool,
    /// Explicit user-controlled overlay state for the pinned summary. This is
    /// independent from the auxiliary sidebar's selected pane.
    pub pinned_summary_overlay_open: bool,
    pub sessions: BTreeMap<SessionLocator, SessionPresentationState>,
    pub integrations: LoadState<IntegrationCatalog>,
    pub integration_search: String,
    pub integration_scope: IntegrationScope,
    pub integration_mutation: IntegrationMutationState,
    pub installed_plugins: LoadState<Vec<InstalledPluginSummary>>,
    pub plugin_add: PluginAddState,
    pub plugin_detail: Option<PluginDetailState>,
}

/// Builds the inspector vocabulary from the selected session's canonical state.
/// Diff data remains owned by `SessionDiffState`; host state and transcript
/// artifacts are projected without copying them into `EnvironmentSnapshot`.
pub fn environment_sections(state: &crate::ZodeAppState) -> Vec<EnvironmentSection> {
    let session = state.current_session.as_ref();
    let presentation = session.and_then(|session| state.presentation.sessions.get(session));
    let context = presentation.and_then(|presentation| presentation.context.ready());
    let diff = presentation
        .and_then(|presentation| presentation.diff.load.ready())
        .filter(|diff| session.is_some_and(|session| session == &diff.session))
        .filter(|diff| !diff.files.is_empty());

    let mut sections = Vec::new();
    if let Some(diff) = diff {
        let (additions, deletions) = diff_totals(diff);
        push_section(
            &mut sections,
            EnvironmentSectionKind::Changes,
            vec![EnvironmentEntry::new(
                "changes",
                "文件变更",
                Some(format!(
                    "{} 个文件 · +{additions} -{deletions}",
                    diff.files.len()
                )),
            )],
        );
    }

    let mut host_entries = vec![EnvironmentEntry::new(
        "connection",
        "主机连接",
        Some(connection_label(state.host.connection).into()),
    )];
    let workspace = context
        .map(|context| &context.workspace_uri)
        .or_else(|| {
            session.and_then(|session| {
                state
                    .threads
                    .iter()
                    .find(|thread| &thread.session == session)
                    .map(|thread| &thread.workspace_uri)
            })
        })
        .filter(|workspace| !state.is_projectless_workspace(workspace));
    if let Some(workspace) = workspace
        .map(WorkspaceUri::as_str)
        .filter(|workspace| !workspace.trim().is_empty())
    {
        host_entries.push(EnvironmentEntry::new(
            "workspace",
            "当前工作区",
            Some(workspace.into()),
        ));
    }
    push_section(&mut sections, EnvironmentSectionKind::Host, host_entries);

    if let Some(branch) = context
        .and_then(|context| context.branch.as_deref())
        .filter(|branch| !branch.trim().is_empty())
    {
        push_section(
            &mut sections,
            EnvironmentSectionKind::Branch,
            vec![EnvironmentEntry::new(
                "branch",
                "当前分支",
                Some(branch.into()),
            )],
        );
    }

    let mut repository_action_entries = Vec::new();
    if let Some(diff) = diff {
        repository_action_entries.push(EnvironmentEntry::new(
            "review",
            format!("审查 {} 项变更", diff.files.len()),
            None,
        ));
    }
    if let Some(status) =
        presentation.and_then(|presentation| presentation.pull_request.load.ready())
    {
        repository_action_entries.push(pull_request_entry(*status));
    }
    push_section(
        &mut sections,
        EnvironmentSectionKind::RepositoryActions,
        repository_action_entries,
    );
    if let Some(diff) = diff {
        let (additions, deletions) = diff_totals(diff);
        push_section(
            &mut sections,
            EnvironmentSectionKind::Comparisons,
            vec![EnvironmentEntry::new(
                "workspace-head",
                "工作区 ↔ HEAD",
                Some(format!("+{additions} -{deletions}")),
            )],
        );
    }

    if let Some(presentation) = presentation {
        push_section(
            &mut sections,
            EnvironmentSectionKind::Subagents,
            subagent_entries(&presentation.subagents),
        );
    }
    push_section(
        &mut sections,
        EnvironmentSectionKind::ComputerUse,
        computer_use_entries(state, session),
    );
    push_section(
        &mut sections,
        EnvironmentSectionKind::BackgroundProcesses,
        presentation
            .map(|presentation| background_process_entries(&presentation.background_processes))
            .unwrap_or_default(),
    );

    push_section(
        &mut sections,
        EnvironmentSectionKind::Sources,
        transcript_sources(state, session),
    );
    sections
}

/// Projects one `gh`-backed pull-request status into its single
/// RepositoryActions row.
fn pull_request_entry(status: PullRequestStatus) -> EnvironmentEntry {
    EnvironmentEntry::new("pull-request", status.label(), status.value())
}

const fn background_process_status_label(status: BackgroundProcessStatus) -> &'static str {
    match status {
        BackgroundProcessStatus::Starting => "正在启动",
        BackgroundProcessStatus::Running => "运行中",
        BackgroundProcessStatus::Stopping => "正在停止",
        BackgroundProcessStatus::Stopped => "已停止",
        BackgroundProcessStatus::NotFound => "未找到",
    }
}

/// Projects live background shells into BackgroundProcesses rows. Entry id
/// carries the shell id (`bg:<id>`) so row actions (stop / view output) can
/// look the shell back up - see `zode_app_ui::EnvironmentPanel`'s
/// `background_process_stop_widget_id`/`..._view_output_widget_id`.
fn background_process_entries(processes: &[BackgroundProcessSnapshot]) -> Vec<EnvironmentEntry> {
    processes
        .iter()
        .map(|process| {
            EnvironmentEntry::new(
                format!("bg:{}", process.id),
                process.command.clone(),
                Some(background_process_status_label(process.status).to_owned()),
            )
        })
        .collect()
}

fn push_section(
    sections: &mut Vec<EnvironmentSection>,
    kind: EnvironmentSectionKind,
    entries: Vec<EnvironmentEntry>,
) {
    if !entries.is_empty() {
        sections.push(EnvironmentSection { kind, entries });
    }
}

/// Projects the session's live sub-agents into the environment card's
/// compact Codex-style row: up to [`MAX_SUBAGENT_AVATARS`] colored avatar
/// dots followed by running/completed counts ("N 完成", or "N 运行中 · M 完成"
/// while something is active). This is deliberately the *only* row - a full
/// per-agent list (name, status, history) belongs to the dedicated M2
/// sub-agent panel, not this compact card.
fn subagent_entries(subagents: &[SubagentSnapshot]) -> Vec<EnvironmentEntry> {
    if subagents.is_empty() {
        return Vec::new();
    }
    let running = subagents
        .iter()
        .filter(|subagent| subagent.status == SubagentStatus::Running)
        .count();
    let done = subagents.len() - running;
    let value = if running > 0 {
        format!("{running} 运行中 · {done} 完成")
    } else {
        format!("{done} 完成")
    };
    let avatar_ids = subagents
        .iter()
        .take(MAX_SUBAGENT_AVATARS)
        .map(|subagent| subagent.id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    // Label is deliberately distinct from `EnvironmentSectionKind::Subagents`'s
    // own title ("子智能体") so the accessibility name (which joins the
    // section title with this row's "label：value") doesn't read as a
    // stutter ("子智能体：子智能体：...").
    vec![EnvironmentEntry::new(
        format!("{SUBAGENTS_SUMMARY_ENTRY_ID}:{avatar_ids}"),
        "状态",
        Some(value),
    )]
}

/// `zode-core` `computer` tool group's stable names - see
/// `zode_core::computer::tools`. Duplicated here (rather than imported) since
/// this crate stays backend-agnostic (see the crate-level dependency
/// direction note) and these two names are effectively part of the wire
/// contract, like `TASK_TOOL_NAME` elsewhere.
const COMPUTER_READ_TOOL_NAME: &str = "computer_read";
const COMPUTER_ACT_TOOL_NAME: &str = "computer_act";

/// Gated on real activity, like [`subagent_entries`]: a compact one-row
/// summary only while a `computer_read`/`computer_act` tool call is actually
/// `Running` in the current session's transcript - empty (no stub row)
/// otherwise. Shows the target app when the most recent call carried one
/// (currently only `computer_read`'s `app_state` action does; see
/// `engine_backend::computer_tool_summary`).
fn computer_use_entries(
    state: &crate::ZodeAppState,
    session: Option<&SessionLocator>,
) -> Vec<EnvironmentEntry> {
    let Some(transcript) = session.and_then(|session| state.transcripts.get(session)) else {
        return Vec::new();
    };
    let running_computer_tool = transcript.items.iter().rev().find_map(|item| match item {
        crate::TranscriptItem::Tool(tool)
            if tool.status == ToolStatus::Running
                && (tool.name == COMPUTER_READ_TOOL_NAME
                    || tool.name == COMPUTER_ACT_TOOL_NAME) =>
        {
            Some(tool)
        }
        _ => None,
    });
    let Some(tool) = running_computer_tool else {
        return Vec::new();
    };
    let target_app = tool
        .summary
        .split_whitespace()
        .find_map(|part| part.strip_prefix("app="))
        .filter(|app| !app.is_empty())
        .map(str::to_owned);
    vec![EnvironmentEntry::new(
        "computer-use-active",
        "状态",
        Some(target_app.unwrap_or_else(|| "正在操作".into())),
    )]
}

/// Projects one sub-agent into a generic environment row (label = the
/// human-readable task name; value = agent type + status + token count,
/// agent_type demoted from label to a value detail now that `display_name`
/// carries the human-readable title). Used by the composer footer's agent
/// picker, which lists individual agents by name (unlike the environment
/// panel's compact avatar+count summary row).
pub fn subagent_entry(subagent: &SubagentSnapshot) -> EnvironmentEntry {
    EnvironmentEntry::new(
        format!("subagent:{}", subagent.id),
        subagent.display_name.clone(),
        Some(subagent_status_value(subagent)),
    )
}

/// Shared with the M2 sub-agent panel's per-row accessibility label (see
/// `zode_app_ui::SubagentsPanel`), so the status word never drifts between
/// the composer footer's agent picker and the dedicated panel.
pub const fn subagent_status_label(status: SubagentStatus) -> &'static str {
    match status {
        SubagentStatus::Running => "运行中",
        SubagentStatus::Completed => "已完成",
        SubagentStatus::Failed => "失败",
    }
}

fn subagent_status_value(subagent: &SubagentSnapshot) -> String {
    format!(
        "{} · {} · {} tokens",
        subagent.agent_type,
        subagent_status_label(subagent.status),
        subagent.tokens
    )
}

/// Page size for the M2 sub-agent panel's "完成" (completed) section -
/// see `docs/proposals/subagent-panel-m2.md`. Both the initial page and each
/// "再显示 N 个" expansion use this same increment.
pub const SUBAGENTS_PAGE_SIZE: usize = 10;

/// Resolves how many completed rows the panel currently shows. `0` (a fresh
/// session's default) means "not yet expanded" rather than "show nothing" -
/// see [`SessionPresentationState::subagents_shown`]'s doc comment.
pub fn subagents_visible_count(presentation: &SessionPresentationState) -> usize {
    if presentation.subagents_shown == 0 {
        SUBAGENTS_PAGE_SIZE
    } else {
        presentation.subagents_shown
    }
}

/// `agent-tools-code`'s registered tool names (see
/// `vendor/agent/crates/agent-tools-code/src/{fs,web,web_search}.rs`) whose
/// completed calls attribute a Sources activity. These are the tools'
/// actual wire names, not the bare "Read"/"Write"/"Edit" the model might
/// use in prose.
const FILE_READ_TOOL_NAME: &str = "FileRead";
const FILE_WRITE_TOOL_NAME: &str = "FileWrite";
const FILE_EDIT_TOOL_NAME: &str = "FileEdit";
const WEB_FETCH_TOOL_NAME: &str = "WebFetch";
const WEB_SEARCH_TOOL_NAME: &str = "WebSearch";

/// Projects the transcript's file-like artifacts - attachments the user
/// provided, plus files the agent created/read/updated via its filesystem
/// tools - into Sources rows, in encounter order and deduplicated by path
/// (see [`upsert_file_source`] for how one path's `activity` is decided
/// when it is touched more than once). Trailing aggregate rows summarize
/// web activity (pages fetched, searches run) instead of one row per URL,
/// since those don't have a stable per-item identity worth a row each.
fn transcript_sources(
    state: &crate::ZodeAppState,
    session: Option<&SessionLocator>,
) -> Vec<EnvironmentEntry> {
    let Some(transcript) = session.and_then(|session| state.transcripts.get(session)) else {
        return Vec::new();
    };
    let mut order = Vec::new();
    let mut by_path = HashMap::new();
    let mut web_pages = 0u32;
    let mut web_searches = 0u32;

    for item in &transcript.items {
        match item {
            crate::TranscriptItem::FileArtifact(file) => {
                let activity = if file.change_summary.is_some() {
                    SourceActivity::Updated
                } else {
                    SourceActivity::Created
                };
                upsert_file_source(&mut order, &mut by_path, &file.path, "文件", activity);
            }
            crate::TranscriptItem::Attachment(attachment) => {
                if let Some(path) = attachment.path.as_deref() {
                    upsert_file_source(
                        &mut order,
                        &mut by_path,
                        path,
                        "附件",
                        SourceActivity::Provided,
                    );
                }
            }
            crate::TranscriptItem::Tool(tool) if tool.status == ToolStatus::Completed => {
                match tool.name.as_str() {
                    FILE_READ_TOOL_NAME => {
                        if let Some(path) = tool_summary_value(tool) {
                            upsert_file_source(
                                &mut order,
                                &mut by_path,
                                path,
                                "文件",
                                SourceActivity::Read,
                            );
                        }
                    }
                    FILE_EDIT_TOOL_NAME => {
                        if let Some(path) = tool_summary_value(tool) {
                            upsert_file_source(
                                &mut order,
                                &mut by_path,
                                path,
                                "文件",
                                SourceActivity::Updated,
                            );
                        }
                    }
                    FILE_WRITE_TOOL_NAME => {
                        if let Some(path) = tool_summary_value(tool) {
                            // First sighting of a path via `FileWrite` reads
                            // as `Created`; a path already known to this
                            // transcript (provided, read, created or
                            // updated) reads as `Updated` instead - the
                            // best signal available since `FileWrite`'s own
                            // result never says whether it created or
                            // overwrote the file (see
                            // `zode_app_runtime::engine_backend`).
                            let activity = if by_path.contains_key(path) {
                                SourceActivity::Updated
                            } else {
                                SourceActivity::Created
                            };
                            upsert_file_source(&mut order, &mut by_path, path, "文件", activity);
                        }
                    }
                    WEB_FETCH_TOOL_NAME => web_pages += 1,
                    WEB_SEARCH_TOOL_NAME => web_searches += 1,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let mut entries = order
        .into_iter()
        .filter_map(|path: String| by_path.remove(&path))
        .collect::<Vec<_>>();
    if web_pages > 0 {
        entries.push(web_activity_entry("sources-web-pages", "网页", web_pages));
    }
    if web_searches > 0 {
        entries.push(web_activity_entry(
            "sources-web-searches",
            "网页搜索",
            web_searches,
        ));
    }
    entries
}

/// Inserts or updates one path's Sources row. `hint` is the activity this
/// particular sighting would imply in isolation; the stored activity only
/// ever moves toward `Updated` - a later `Read` never demotes a path
/// already known to be `Created`/`Updated`/`Provided`, while `Updated`
/// always wins once observed (an edit or an overwrite is the most concrete
/// thing this function can say happened). This is a monotonic lattice, not
/// last-write-wins: Sources answers "what happened to this file", not
/// "what was the last tool call that touched it".
fn upsert_file_source(
    order: &mut Vec<String>,
    by_path: &mut HashMap<String, EnvironmentEntry>,
    path: &str,
    label: &str,
    hint: SourceActivity,
) {
    let path = path.trim();
    if path.is_empty() {
        return;
    }
    if let Some(entry) = by_path.get_mut(path) {
        if matches!(hint, SourceActivity::Updated) {
            entry.activity = Some(SourceActivity::Updated);
        }
        return;
    }
    order.push(path.to_owned());
    by_path.insert(
        path.to_owned(),
        EnvironmentEntry {
            id: format!("source:{path}"),
            label: label.to_owned(),
            value: Some(path.to_owned()),
            activity: Some(hint),
            count: None,
        },
    );
}

fn web_activity_entry(id: &str, label: &str, count: u32) -> EnvironmentEntry {
    EnvironmentEntry {
        id: id.to_owned(),
        label: label.to_owned(),
        value: None,
        activity: None,
        count: Some(count),
    }
}

/// Re-parses the display-safe value out of a completed tool call's summary
/// (`"{name} {key}={value}"`, produced by
/// `zode_app_runtime::engine_backend::safe_tool_summary`) - the same
/// re-parsing trick `tool_card::action_presentation` uses to recover
/// `computer_read`/`computer_act`'s action from its summary. `ToolCall`
/// carries no structured path/url field, only this display string, so this
/// is the only way to recover it downstream of the wire boundary.
fn tool_summary_value(tool: &ToolCall) -> Option<&str> {
    tool.summary
        .strip_prefix(tool.name.as_str())
        .and_then(|rest| rest.strip_prefix(' '))
        .and_then(|rest| rest.split_once('='))
        .map(|(_, value)| value)
}

fn diff_totals(diff: &DiffSnapshot) -> (u64, u64) {
    diff.files
        .iter()
        .fold((0, 0), |(additions, deletions), file| {
            (
                additions + u64::from(file.additions),
                deletions + u64::from(file.deletions),
            )
        })
}

const fn connection_label(connection: crate::ConnectionState) -> &'static str {
    match connection {
        crate::ConnectionState::Local => "本地",
        crate::ConnectionState::Connecting => "连接中",
        crate::ConnectionState::Unavailable => "不可用",
    }
}
