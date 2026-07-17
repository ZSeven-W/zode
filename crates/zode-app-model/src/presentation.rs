use std::collections::{BTreeMap, BTreeSet};

use zode_node_protocol::{DiffSnapshot, RuntimeOptions, SessionLocator, WorkspaceUri};

#[path = "presentation-integrations.rs"]
mod integrations;
pub use integrations::*;

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

/// One real item reported by a host context source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentEntry {
    pub id: String,
    pub label: String,
    pub value: Option<String>,
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
    pub subagents: Vec<EnvironmentEntry>,
    pub background_processes: Vec<EnvironmentEntry>,
    pub sources: Vec<EnvironmentEntry>,
}

/// Safe local repository intents exposed by the environment inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EnvironmentActionKind {
    RefreshStatus,
    CompareWorkspaceToHead,
    OpenWorkspace,
    CommitOrPush,
}

impl EnvironmentActionKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::RefreshStatus => "刷新状态",
            Self::CompareWorkspaceToHead => "比较工作区与 HEAD",
            Self::OpenWorkspace => "打开工作目录",
            Self::CommitOrPush => "提交或推送",
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
    vec![
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
    ]
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

/// Data shown by presentation surfaces for one session only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionPresentationState {
    pub diff: SessionDiffState,
    pub context: LoadState<EnvironmentSnapshot>,
    pub preview: PreviewState,
    pub runtime_options: LoadState<RuntimeOptions>,
}

/// Typed route, pane selection, and session-isolated presentation data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentationState {
    pub route: ShellRoute,
    pub secondary_pane: Option<SecondaryPane>,
    /// Explicitly suppresses the otherwise automatic wide-screen summary.
    /// Narrow overlays continue to use `secondary_pane` so resizing alone
    /// never opens a floating panel.
    pub pinned_summary_auto_hidden: bool,
    pub secondary_menu_open: bool,
    pub sessions: BTreeMap<SessionLocator, SessionPresentationState>,
    pub integrations: LoadState<IntegrationCatalog>,
    pub integration_search: String,
    pub integration_scope: IntegrationScope,
    pub integration_mutation: IntegrationMutationState,
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
            vec![EnvironmentEntry {
                id: "changes".into(),
                label: "文件变更".into(),
                value: Some(format!(
                    "{} 个文件 · +{additions} -{deletions}",
                    diff.files.len()
                )),
            }],
        );
    }

    let mut host_entries = vec![EnvironmentEntry {
        id: "connection".into(),
        label: "主机连接".into(),
        value: Some(connection_label(state.host.connection).into()),
    }];
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
        host_entries.push(EnvironmentEntry {
            id: "workspace".into(),
            label: "当前工作区".into(),
            value: Some(workspace.into()),
        });
    }
    push_section(&mut sections, EnvironmentSectionKind::Host, host_entries);

    if let Some(branch) = context
        .and_then(|context| context.branch.as_deref())
        .filter(|branch| !branch.trim().is_empty())
    {
        push_section(
            &mut sections,
            EnvironmentSectionKind::Branch,
            vec![EnvironmentEntry {
                id: "branch".into(),
                label: "当前分支".into(),
                value: Some(branch.into()),
            }],
        );
    }

    if let Some(diff) = diff {
        let (additions, deletions) = diff_totals(diff);
        push_section(
            &mut sections,
            EnvironmentSectionKind::RepositoryActions,
            vec![EnvironmentEntry {
                id: "review".into(),
                label: format!("审查 {} 项变更", diff.files.len()),
                value: None,
            }],
        );
        push_section(
            &mut sections,
            EnvironmentSectionKind::Comparisons,
            vec![EnvironmentEntry {
                id: "workspace-head".into(),
                label: "工作区 ↔ HEAD".into(),
                value: Some(format!("+{additions} -{deletions}")),
            }],
        );
    }

    if let Some(context) = context {
        push_section(
            &mut sections,
            EnvironmentSectionKind::Subagents,
            context.subagents.clone(),
        );
        push_section(
            &mut sections,
            EnvironmentSectionKind::BackgroundProcesses,
            context.background_processes.clone(),
        );
    }

    push_section(
        &mut sections,
        EnvironmentSectionKind::Sources,
        transcript_sources(state, session),
    );
    sections
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

fn transcript_sources(
    state: &crate::ZodeAppState,
    session: Option<&SessionLocator>,
) -> Vec<EnvironmentEntry> {
    let Some(transcript) = session.and_then(|session| state.transcripts.get(session)) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    transcript
        .items
        .iter()
        .filter_map(|item| match item {
            crate::TranscriptItem::FileArtifact(file) => {
                source_entry(&mut seen, format!("file:{}", file.id), "文件", &file.path)
            }
            crate::TranscriptItem::Attachment(attachment) => {
                attachment.path.as_deref().and_then(|path| {
                    source_entry(
                        &mut seen,
                        format!("attachment:{}", attachment.id),
                        "附件",
                        path,
                    )
                })
            }
            _ => None,
        })
        .collect()
}

fn source_entry(
    seen: &mut BTreeSet<String>,
    id: String,
    label: &str,
    path: &str,
) -> Option<EnvironmentEntry> {
    let path = path.trim();
    (!path.is_empty() && seen.insert(path.to_owned())).then(|| EnvironmentEntry {
        id,
        label: label.into(),
        value: Some(path.into()),
    })
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
