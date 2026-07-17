use std::collections::BTreeMap;

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
}

/// Typed route, pane selection, and session-isolated presentation data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentationState {
    pub route: ShellRoute,
    pub secondary_pane: Option<SecondaryPane>,
    pub sessions: BTreeMap<SessionLocator, SessionPresentationState>,
}
