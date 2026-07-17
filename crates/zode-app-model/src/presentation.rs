use std::collections::{BTreeMap, BTreeSet};

use zode_node_protocol::{
    DiffSnapshot, IntegrationRegistryEntry, IntegrationRegistryKind, IntegrationRegistrySnapshot,
    IntegrationRegistryState, RuntimeOptions, SessionLocator, WorkspaceUri,
};

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
    Help,
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

/// Stable catalog categories sourced from local registries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IntegrationCategory {
    BuiltInTools,
    Skills,
    Mcp,
    Lsp,
    Capabilities,
}

impl IntegrationCategory {
    pub const fn title(self) -> &'static str {
        match self {
            Self::BuiltInTools => "内置工具",
            Self::Skills => "技能",
            Self::Mcp => "MCP 服务",
            Self::Lsp => "语言服务",
            Self::Capabilities => "节点能力",
        }
    }
}

/// A status the runtime can prove without launching an integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Ready,
    Configured,
    Disabled,
}

impl Availability {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "可用",
            Self::Configured => "已配置",
            Self::Disabled => "已停用",
        }
    }
}

/// How an integration became present on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationInstallState {
    BuiltIn,
    Installed,
    Configured,
}

/// Auditable icon fallback. Repository-backed branded assets can be added as
/// another enum variant; missing assets always use a non-empty monogram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationIcon {
    Monogram(String),
}

impl IntegrationIcon {
    pub fn label(&self) -> &str {
        match self {
            Self::Monogram(label) => label,
        }
    }
}

/// One catalog row projected from exactly one runtime registry source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationEntry {
    pub source_id: Option<String>,
    pub name: String,
    pub description: String,
    pub category: IntegrationCategory,
    pub installed: bool,
    pub availability: Availability,
    pub install_state: IntegrationInstallState,
    pub icon: IntegrationIcon,
    /// Reserved for test scene builders. Production projection always writes
    /// false, which makes fixture leakage directly testable.
    pub fixture_only: bool,
}

/// Compact icon-band item derived from an installed catalog row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledIntegration {
    pub source_id: Option<String>,
    pub name: String,
    pub icon: IntegrationIcon,
    pub availability: Availability,
}

/// One non-empty catalog section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationSection {
    pub category: IntegrationCategory,
    pub rows: Vec<IntegrationEntry>,
}

/// Presentation-ready catalog that never depends on a network marketplace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationCatalog {
    pub workspace_uri: WorkspaceUri,
    pub installed: Vec<InstalledIntegration>,
    pub sections: Vec<IntegrationSection>,
    pub directory_error: Option<String>,
}

impl IntegrationCatalog {
    pub fn all_entries(&self) -> impl Iterator<Item = &IntegrationEntry> {
        self.sections.iter().flat_map(|section| section.rows.iter())
    }
}

/// Converts a typed runtime registry snapshot into production presentation
/// state. The mapper deliberately owns `fixture_only = false`.
pub fn integration_catalog(snapshot: IntegrationRegistrySnapshot) -> IntegrationCatalog {
    let mut grouped = BTreeMap::<IntegrationCategory, Vec<IntegrationEntry>>::new();
    for source in snapshot.entries {
        let entry = integration_entry(source);
        grouped.entry(entry.category).or_default().push(entry);
    }
    for rows in grouped.values_mut() {
        rows.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.source_id.cmp(&right.source_id))
        });
    }

    let sections = [
        IntegrationCategory::BuiltInTools,
        IntegrationCategory::Capabilities,
        IntegrationCategory::Skills,
        IntegrationCategory::Mcp,
        IntegrationCategory::Lsp,
    ]
    .into_iter()
    .filter_map(|category| {
        grouped
            .remove(&category)
            .filter(|rows| !rows.is_empty())
            .map(|rows| IntegrationSection { category, rows })
    })
    .collect::<Vec<_>>();
    let installed = sections
        .iter()
        .flat_map(|section| section.rows.iter())
        .filter(|entry| entry.installed)
        .map(|entry| InstalledIntegration {
            source_id: entry.source_id.clone(),
            name: entry.name.clone(),
            icon: entry.icon.clone(),
            availability: entry.availability,
        })
        .collect();

    IntegrationCatalog {
        workspace_uri: snapshot.workspace_uri,
        installed,
        sections,
        directory_error: snapshot.directory_error,
    }
}

fn integration_entry(source: IntegrationRegistryEntry) -> IntegrationEntry {
    let category = match source.kind {
        IntegrationRegistryKind::ToolGroup => IntegrationCategory::BuiltInTools,
        IntegrationRegistryKind::Skill => IntegrationCategory::Skills,
        IntegrationRegistryKind::Mcp => IntegrationCategory::Mcp,
        IntegrationRegistryKind::Lsp => IntegrationCategory::Lsp,
        IntegrationRegistryKind::NodeCapability => IntegrationCategory::Capabilities,
    };
    let availability = match source.state {
        IntegrationRegistryState::Ready => Availability::Ready,
        IntegrationRegistryState::Configured => Availability::Configured,
        IntegrationRegistryState::Disabled => Availability::Disabled,
    };
    let install_state = match category {
        IntegrationCategory::BuiltInTools | IntegrationCategory::Capabilities => {
            IntegrationInstallState::BuiltIn
        }
        IntegrationCategory::Skills => IntegrationInstallState::Installed,
        IntegrationCategory::Mcp | IntegrationCategory::Lsp => IntegrationInstallState::Configured,
    };
    let icon = IntegrationIcon::Monogram(stable_monogram(&source.name));
    IntegrationEntry {
        source_id: Some(source.source_id),
        name: source.name,
        description: source.description,
        category,
        installed: source.installed,
        availability,
        install_state,
        icon,
        fixture_only: false,
    }
}

fn stable_monogram(name: &str) -> String {
    let words = name
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let mut label = if words.len() >= 2 {
        words
            .iter()
            .take(2)
            .filter_map(|word| word.chars().next())
            .collect::<String>()
    } else {
        words
            .first()
            .copied()
            .unwrap_or(name)
            .chars()
            .take(2)
            .collect::<String>()
    };
    if label.is_empty() {
        label.push('Z');
    }
    label.to_uppercase()
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
    pub runtime_options: LoadState<RuntimeOptions>,
}

/// Typed route, pane selection, and session-isolated presentation data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentationState {
    pub route: ShellRoute,
    pub secondary_pane: Option<SecondaryPane>,
    pub sessions: BTreeMap<SessionLocator, SessionPresentationState>,
    pub integrations: LoadState<IntegrationCatalog>,
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
