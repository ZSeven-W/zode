mod layout;
mod paint;

use std::collections::BTreeMap;

use jian_widgets::{Painter, Rect};
use zode_app_model::{
    AppCommand, ComingSoonFeature, IntegrationsTab, SettingsCategory, ShellRoute, ZodeAppState,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

use crate::{stable_widget_id, RectExt, SemanticIcon, WidgetId, ZodeTheme};

pub use layout::{
    SidebarControlLayout, SidebarControlTarget, SidebarLabelLayout, SidebarLayout,
    SidebarNavigationRowLayout, SidebarSection, SidebarSectionLayout,
};

pub const SIDEBAR_TASKS_TOGGLE_ID: WidgetId = WidgetId(140);
pub const SIDEBAR_TASKS_MORE_ID: WidgetId = WidgetId(141);
pub const SIDEBAR_TASKS_NEW_ID: WidgetId = WidgetId(142);
pub const SIDEBAR_SHOW_ALL_PROJECTS_ID: WidgetId = WidgetId(143);

const PIN_NAMESPACE: u8 = 0x42;
const ARCHIVE_NAMESPACE: u8 = 0x43;
const SHOW_PROJECT_SESSIONS_NAMESPACE: u8 = 0x44;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarAction {
    NewSession,
    Navigate(ShellRoute),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarItem {
    pub label: &'static str,
    pub icon: SemanticIcon,
    pub action: SidebarAction,
    /// Whether the route has a complete product surface. Placeholder routes
    /// remain visually enabled so their selection state is never ambiguous.
    pub implemented: bool,
}

const NAVIGATION: [SidebarItem; 6] = [
    SidebarItem {
        label: "新建任务",
        icon: SemanticIcon::NewTask,
        action: SidebarAction::NewSession,
        implemented: true,
    },
    SidebarItem {
        label: "已安排",
        icon: SemanticIcon::Scheduled,
        action: SidebarAction::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::ScheduledTasks)),
        implemented: false,
    },
    SidebarItem {
        label: "插件",
        icon: SemanticIcon::Integrations,
        action: SidebarAction::Navigate(ShellRoute::Integrations(IntegrationsTab::Plugins)),
        implemented: true,
    },
    SidebarItem {
        label: "站点",
        icon: SemanticIcon::Sites,
        action: SidebarAction::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::Sites)),
        implemented: false,
    },
    SidebarItem {
        label: "拉取请求",
        icon: SemanticIcon::PullRequest,
        action: SidebarAction::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::PullRequests)),
        implemented: false,
    },
    SidebarItem {
        label: "聊天",
        icon: SemanticIcon::Chat,
        action: SidebarAction::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::Chats)),
        implemented: false,
    },
];

const SETTINGS_FOOTER: SidebarItem = SidebarItem {
    label: "本地设置",
    icon: SemanticIcon::Settings,
    action: SidebarAction::Navigate(ShellRoute::Settings(SettingsCategory::General)),
    implemented: true,
};

pub struct ProjectSidebar;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarRowTarget {
    Project(WorkspaceUri),
    Task(SessionLocator),
    Session(SessionLocator),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SidebarRowLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub label: String,
    pub target: SidebarRowTarget,
    pub actionable: bool,
    pub selected: bool,
    pub status: Option<ThreadStatus>,
    pub pinned: bool,
    pub shortcut: Option<usize>,
    pub pin_id: Option<WidgetId>,
    pub archive_id: Option<WidgetId>,
    pub workspace_uri: Option<WorkspaceUri>,
}

impl SidebarRowLayout {
    pub fn session(&self) -> Option<&SessionLocator> {
        match &self.target {
            SidebarRowTarget::Task(session) | SidebarRowTarget::Session(session) => Some(session),
            SidebarRowTarget::Project(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSessionGroup {
    pub workspace_uri: WorkspaceUri,
    pub sessions: Vec<ThreadSummary>,
}

#[derive(Debug)]
pub(super) struct DynamicProject {
    pub workspace_uri: WorkspaceUri,
    pub sessions: Vec<ThreadSummary>,
    pub expanded: bool,
    pub available: bool,
    pub sort_key_ms: i64,
    pub toggleable: bool,
}

pub fn group_sessions(sessions: Vec<ThreadSummary>) -> Vec<ProjectSessionGroup> {
    let mut by_workspace: BTreeMap<WorkspaceUri, Vec<ThreadSummary>> = BTreeMap::new();
    for session in sessions {
        by_workspace
            .entry(session.workspace_uri.clone())
            .or_default()
            .push(session);
    }
    let mut groups: Vec<_> = by_workspace
        .into_iter()
        .map(|(workspace_uri, mut sessions)| {
            sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
            ProjectSessionGroup {
                workspace_uri,
                sessions,
            }
        })
        .collect();
    groups.sort_by(|left, right| {
        let left_newest = left
            .sessions
            .first()
            .map_or(i64::MIN, |session| session.updated_at_ms);
        let right_newest = right
            .sessions
            .first()
            .map_or(i64::MIN, |session| session.updated_at_ms);
        right_newest
            .cmp(&left_newest)
            .then_with(|| left.workspace_uri.cmp(&right.workspace_uri))
    });
    groups
}

impl ProjectSidebar {
    pub const fn navigation_items() -> &'static [SidebarItem] {
        &NAVIGATION
    }

    pub const fn footer_item() -> SidebarItem {
        SETTINGS_FOOTER
    }

    pub fn layout(rect: Rect, state: &ZodeAppState) -> SidebarLayout {
        layout::build(rect, state)
    }

    pub fn footer_rect(rect: Rect) -> Rect {
        layout::footer_rect(rect)
    }

    pub fn help_rect(rect: Rect) -> Rect {
        layout::help_rect(Self::footer_rect(rect))
    }

    pub fn profile_rect(rect: Rect) -> Rect {
        layout::profile_rect(Self::footer_rect(rect))
    }

    pub fn footer_selected(state: &ZodeAppState) -> bool {
        matches!(state.presentation.route, ShellRoute::Settings(_))
    }

    /// Compatibility geometry for a non-scrolled sidebar. Interactive paint
    /// and accessibility use [`Self::layout`] so state scroll is respected.
    pub fn navigation_row_layout(rect: Rect) -> Vec<SidebarNavigationRowLayout> {
        layout::navigation_rows(rect, 0.0)
    }

    pub fn dynamic_row_layout(rect: Rect, state: &ZodeAppState) -> Vec<SidebarRowLayout> {
        Self::layout(rect, state).rows
    }

    pub fn scroll_viewport(rect: Rect) -> Rect {
        layout::scroll_viewport(rect)
    }

    pub fn content_height(rect: Rect, state: &ZodeAppState) -> f32 {
        Self::layout(rect, state).content_height
    }

    pub fn max_scroll(rect: Rect, state: &ZodeAppState) -> f32 {
        Self::layout(rect, state).max_scroll
    }

    /// `delta > 0` moves the content down the list. The host converts wheel
    /// direction and line/pixel units before calling this helper.
    pub fn scroll_command(rect: Rect, state: &ZodeAppState, delta: f32) -> Option<AppCommand> {
        let sidebar = Self::layout(rect, state);
        let offset = (sidebar.scroll_offset + delta).clamp(0.0, sidebar.max_scroll);
        ((offset - sidebar.scroll_offset).abs() > f32::EPSILON)
            .then_some(AppCommand::SetSidebarScroll { offset })
    }

    pub fn project_widget_id(workspace: &WorkspaceUri) -> WidgetId {
        stable_widget_id(0x40, workspace)
    }

    pub fn session_widget_id(session: &SessionLocator) -> WidgetId {
        stable_widget_id(0x41, session)
    }

    pub fn session_pin_widget_id(session: &SessionLocator) -> WidgetId {
        stable_widget_id(PIN_NAMESPACE, session)
    }

    pub fn session_archive_widget_id(session: &SessionLocator) -> WidgetId {
        stable_widget_id(ARCHIVE_NAMESPACE, session)
    }

    pub fn session_pin_rect(row: &SidebarRowLayout) -> Option<Rect> {
        row.pin_id
            .map(|_| Rect::xywh(row.rect.max_x() - 46.0, row.rect.origin.y + 4.0, 22.0, 22.0))
    }

    pub fn session_archive_rect(row: &SidebarRowLayout) -> Option<Rect> {
        row.archive_id
            .map(|_| Rect::xywh(row.rect.max_x() - 23.0, row.rect.origin.y + 4.0, 22.0, 22.0))
    }

    pub fn show_all_project_sessions_widget_id(workspace: &WorkspaceUri) -> WidgetId {
        stable_widget_id(SHOW_PROJECT_SESSIONS_NAMESPACE, workspace)
    }

    /// Resolves the first five rendered session rows, including pinned rows,
    /// into the stable Cmd+1...Cmd+5 shortcut order.
    pub fn shortcut_session(state: &ZodeAppState, number: usize) -> Option<SessionLocator> {
        if !(1..=5).contains(&number) {
            return None;
        }
        layout::ordered_sessions(state).nth(number - 1)
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if id == SIDEBAR_TASKS_TOGGLE_ID {
            return Some(AppCommand::ToggleSidebarTasks);
        }
        if id == SIDEBAR_TASKS_NEW_ID {
            return Some(AppCommand::BeginTask {
                workspace_uri: None,
            });
        }
        if id == SIDEBAR_SHOW_ALL_PROJECTS_ID {
            return Some(AppCommand::ShowAllProjects);
        }

        for thread in state
            .threads
            .iter()
            .filter(|thread| !state.archived_sessions.contains(&thread.session))
        {
            if Self::session_pin_widget_id(&thread.session) == id {
                return Some(AppCommand::SetSessionPinned {
                    session: thread.session.clone(),
                    pinned: !state.pinned_sessions.contains(&thread.session),
                });
            }
            if Self::session_archive_widget_id(&thread.session) == id {
                return Some(AppCommand::SetSessionArchived {
                    session: thread.session.clone(),
                    archived: true,
                });
            }
            if Self::session_widget_id(&thread.session) == id {
                return Some(AppCommand::SelectSession(thread.session.clone()));
            }
        }
        for project in dynamic_projects(state) {
            if Self::show_all_project_sessions_widget_id(&project.workspace_uri) == id {
                return Some(AppCommand::ShowAllProjectSessions {
                    workspace_uri: project.workspace_uri,
                });
            }
            if project.toggleable && Self::project_widget_id(&project.workspace_uri) == id {
                return Some(AppCommand::ToggleProject(project.workspace_uri));
            }
        }
        None
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        Self::paint_with_interaction(painter, rect, state, None, None, theme);
    }

    pub fn paint_with_interaction(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &ZodeAppState,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        paint::paint(painter, rect, state, focused, hovered, theme);
    }

    /// Drawn after the primary surface so the hover card is not occluded by
    /// the content pane that begins at the sidebar's trailing edge.
    pub fn paint_hover_overlay(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &ZodeAppState,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        paint::paint_hover_overlay(painter, rect, state, hovered, theme);
    }
}

pub(super) fn dynamic_projects(state: &ZodeAppState) -> Vec<DynamicProject> {
    let mut sessions = group_sessions(
        state
            .threads
            .iter()
            .filter(|thread| {
                !state.archived_sessions.contains(&thread.session)
                    && !state.pinned_sessions.contains(&thread.session)
            })
            .cloned()
            .collect(),
    )
    .into_iter()
    .filter(|group| !state.is_projectless_workspace(&group.workspace_uri))
    .map(|group| (group.workspace_uri, group.sessions))
    .collect::<BTreeMap<_, _>>();
    let known_projects = state
        .projects
        .iter()
        .filter(|project| !state.is_projectless_workspace(&project.workspace_uri))
        .map(|project| (project.workspace_uri.clone(), project))
        .collect::<BTreeMap<_, _>>();
    let mut projects = known_projects
        .into_values()
        .map(|project| {
            let project_sessions = sessions.remove(&project.workspace_uri).unwrap_or_default();
            let newest_session = project_sessions
                .first()
                .map_or(i64::MIN, |thread| thread.updated_at_ms);
            DynamicProject {
                workspace_uri: project.workspace_uri.clone(),
                sessions: project_sessions,
                expanded: project.expanded,
                available: project.available,
                sort_key_ms: project.last_opened_ms.max(newest_session),
                toggleable: true,
            }
        })
        .collect::<Vec<_>>();
    projects.extend(
        sessions
            .into_iter()
            .map(|(workspace_uri, project_sessions)| {
                let newest_session = project_sessions
                    .first()
                    .map_or(i64::MIN, |thread| thread.updated_at_ms);
                DynamicProject {
                    workspace_uri,
                    sessions: project_sessions,
                    expanded: true,
                    available: true,
                    sort_key_ms: newest_session,
                    toggleable: false,
                }
            }),
    );
    projects.sort_by(|left, right| {
        right
            .sort_key_ms
            .cmp(&left.sort_key_ms)
            .then_with(|| left.workspace_uri.cmp(&right.workspace_uri))
    });
    projects
}

pub(super) fn pinned_tasks(state: &ZodeAppState) -> Vec<ThreadSummary> {
    sorted_tasks(state, |thread| {
        state.pinned_sessions.contains(&thread.session)
            && !state.archived_sessions.contains(&thread.session)
    })
}

pub(super) fn projectless_tasks(state: &ZodeAppState) -> Vec<ThreadSummary> {
    sorted_tasks(state, |thread| {
        state.is_projectless_workspace(&thread.workspace_uri)
            && !state.pinned_sessions.contains(&thread.session)
            && !state.archived_sessions.contains(&thread.session)
    })
}

fn sorted_tasks(
    state: &ZodeAppState,
    include: impl Fn(&ThreadSummary) -> bool,
) -> Vec<ThreadSummary> {
    let mut tasks = state
        .threads
        .iter()
        .filter(|thread| include(thread))
        .cloned()
        .collect::<Vec<_>>();
    tasks.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| left.session.cmp(&right.session))
    });
    tasks
}

pub(crate) fn workspace_label(workspace: &WorkspaceUri, available: bool) -> String {
    let value = workspace.as_str().trim_end_matches('/');
    let name = value.rsplit('/').next().unwrap_or(value);
    if available {
        name.to_owned()
    } else {
        format!("{name} · unavailable")
    }
}

pub(super) fn navigation_item_selected(state: &ZodeAppState, item: SidebarItem) -> bool {
    match item.action {
        SidebarAction::NewSession => {
            state.presentation.route == ShellRoute::Conversation
                && state.current_session.is_none()
                && state.active_workspace.is_none()
        }
        SidebarAction::Navigate(ShellRoute::Integrations(_)) => {
            matches!(state.presentation.route, ShellRoute::Integrations(_))
        }
        SidebarAction::Navigate(route) => state.presentation.route == route,
    }
}
