use std::collections::{BTreeMap, BTreeSet};

use zode_node_protocol::{DiffSnapshot, SessionLocator, WorkspaceUri};

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
    Appearance,
    Permissions,
    KeyboardShortcuts,
    Environment,
}

/// Integration catalog views available to the desktop shell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IntegrationsTab {
    #[default]
    Plugins,
    Skills,
}

/// Explicit placeholders for shell destinations that have no implementation yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ComingSoonFeature {
    #[default]
    ScheduledTasks,
    Sites,
    PullRequests,
    Chats,
}

/// A single optional pane presented alongside the primary route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondaryPane {
    Environment,
    Review,
    DocumentPreview,
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
            Self::Host => "本地",
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
}

/// Typed route, pane selection, and session-isolated presentation data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentationState {
    pub route: ShellRoute,
    pub secondary_pane: Option<SecondaryPane>,
    pub sessions: BTreeMap<SessionLocator, SessionPresentationState>,
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
        .map(|context| context.workspace_uri.as_str())
        .or_else(|| {
            session.and_then(|session| {
                state
                    .threads
                    .iter()
                    .find(|thread| &thread.session == session)
                    .map(|thread| thread.workspace_uri.as_str())
            })
        });
    if let Some(workspace) = workspace.filter(|workspace| !workspace.trim().is_empty()) {
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
