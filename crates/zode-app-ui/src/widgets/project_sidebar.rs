use jian_widgets::{centered_text_baseline_y, Painter, Point2D, Rect, TextLayout};
use std::collections::BTreeMap;

use zode_app_model::{
    AppCommand, ComingSoonFeature, IntegrationsTab, SettingsCategory, ShellRoute, ZodeAppState,
};
use zode_node_protocol::{SessionLocator, ThreadSummary, WorkspaceUri};

use crate::{stable_widget_id, RectExt, WidgetId, ZodeTheme};

const FONT: &str = "system-ui";
const HEADER_H: f32 = 46.0;
const ROW_H: f32 = 32.0;
const ROW_INSET: f32 = 12.0;
const FOOTER_BOTTOM: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarAction {
    NewSession,
    Navigate(ShellRoute),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarItem {
    pub label: &'static str,
    pub action: SidebarAction,
    pub implemented: bool,
}

const NAVIGATION: [SidebarItem; 6] = [
    SidebarItem {
        label: "新建任务",
        action: SidebarAction::NewSession,
        implemented: true,
    },
    SidebarItem {
        label: "已安排",
        action: SidebarAction::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::ScheduledTasks)),
        implemented: false,
    },
    SidebarItem {
        label: "插件",
        action: SidebarAction::Navigate(ShellRoute::Integrations(IntegrationsTab::Plugins)),
        implemented: true,
    },
    SidebarItem {
        label: "站点",
        action: SidebarAction::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::Sites)),
        implemented: false,
    },
    SidebarItem {
        label: "拉取请求",
        action: SidebarAction::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::PullRequests)),
        implemented: false,
    },
    SidebarItem {
        label: "聊天",
        action: SidebarAction::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::Chats)),
        implemented: false,
    },
];

const SETTINGS_FOOTER: SidebarItem = SidebarItem {
    label: "本地设置",
    action: SidebarAction::Navigate(ShellRoute::Settings(SettingsCategory::General)),
    implemented: true,
};

pub struct ProjectSidebar;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarNavigationRowLayout {
    pub index: usize,
    pub rect: Rect,
    pub item: SidebarItem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarRowTarget {
    Project(WorkspaceUri),
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSessionGroup {
    pub workspace_uri: WorkspaceUri,
    pub sessions: Vec<ThreadSummary>,
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

    pub fn footer_rect(rect: Rect) -> Rect {
        let footer_height = if rect.size.y >= FOOTER_BOTTOM + ROW_H {
            ROW_H
        } else {
            0.0
        };
        let footer_y = if footer_height > 0.0 {
            rect.max_y() - FOOTER_BOTTOM - footer_height
        } else {
            rect.max_y()
        };
        Rect::xywh(
            rect.origin.x + 8.0,
            footer_y,
            (rect.size.x - 16.0).max(0.0),
            footer_height,
        )
    }

    pub fn footer_selected(state: &ZodeAppState) -> bool {
        matches!(state.presentation.route, ShellRoute::Settings(_))
    }

    pub fn navigation_row_layout(rect: Rect) -> Vec<SidebarNavigationRowLayout> {
        NAVIGATION
            .iter()
            .copied()
            .enumerate()
            .map(|(index, item)| SidebarNavigationRowLayout {
                index,
                rect: Rect::xywh(
                    rect.origin.x + 8.0,
                    rect.origin.y + HEADER_H + index as f32 * ROW_H,
                    (rect.size.x - 16.0).max(0.0),
                    ROW_H,
                ),
                item,
            })
            .collect()
    }

    pub fn project_widget_id(workspace: &WorkspaceUri) -> WidgetId {
        stable_widget_id(0x40, workspace)
    }

    pub fn session_widget_id(session: &SessionLocator) -> WidgetId {
        stable_widget_id(0x41, session)
    }

    /// Returns every painted dynamic row in visual order. The snapshot uses
    /// this same list for pointer targets and accessibility bounds.
    pub fn dynamic_row_layout(rect: Rect, state: &ZodeAppState) -> Vec<SidebarRowLayout> {
        if rect.size.x < 100.0 || rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return Vec::new();
        }
        let project_y = rect.origin.y + HEADER_H + NAVIGATION.len() as f32 * ROW_H + 18.0;
        let first_row_y = project_y + 28.0;
        let rows_bottom = Self::footer_rect(rect).origin.y;
        let row_capacity = ((rows_bottom - first_row_y) / ROW_H).floor().max(0.0) as usize;
        if row_capacity == 0 {
            return Vec::new();
        }
        let mut rows = Vec::new();
        for project in dynamic_projects(state) {
            let workspace = project.workspace_uri;
            rows.push(SidebarRowLayout {
                id: Self::project_widget_id(&workspace),
                rect: dynamic_row_rect(rect, project_y, rows.len()),
                label: workspace_label(&workspace, project.available),
                target: SidebarRowTarget::Project(workspace.clone()),
                actionable: project.toggleable,
                selected: state.presentation.route == ShellRoute::Conversation
                    && state.current_session.is_none()
                    && state.active_workspace.as_ref() == Some(&workspace),
            });
            if rows.len() >= row_capacity {
                break;
            }
            if project.expanded {
                for thread in project.sessions {
                    let session = thread.session;
                    rows.push(SidebarRowLayout {
                        id: Self::session_widget_id(&session),
                        rect: dynamic_row_rect(rect, project_y, rows.len()),
                        label: thread.title,
                        target: SidebarRowTarget::Session(session.clone()),
                        actionable: true,
                        selected: state.presentation.route == ShellRoute::Conversation
                            && state.current_session.as_ref() == Some(&session),
                    });
                    if rows.len() >= row_capacity {
                        break;
                    }
                }
            }
            if rows.len() >= row_capacity {
                break;
            }
        }
        rows
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        for project in dynamic_projects(state) {
            if project.toggleable && Self::project_widget_id(&project.workspace_uri) == id {
                return Some(AppCommand::ToggleProject(project.workspace_uri));
            }
            for thread in project.sessions {
                if Self::session_widget_id(&thread.session) == id {
                    return Some(AppCommand::SelectSession(thread.session));
                }
            }
        }
        None
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        draw_label(
            painter,
            "Zode",
            Rect::xywh(
                rect.origin.x + ROW_INSET,
                rect.origin.y,
                rect.size.x - ROW_INSET,
                HEADER_H,
            ),
            13.0,
            700,
            theme.zode_purple,
        );

        let compact = rect.size.x < 100.0;
        for row in Self::navigation_row_layout(rect) {
            let selected = navigation_item_selected(state, row.item);
            if selected {
                painter.fill_round_rect(row.rect, theme.tokens.radius, theme.tokens.row_selected);
            }
            let label = if compact {
                row.item.label.chars().next().unwrap_or(' ').to_string()
            } else {
                row.item.label.to_string()
            };
            draw_label(
                painter,
                &label,
                Rect::xywh(
                    row.rect.origin.x + 8.0,
                    row.rect.origin.y,
                    (row.rect.size.x - 16.0).max(0.0),
                    row.rect.size.y,
                ),
                13.0,
                if selected { 600 } else { 400 },
                if row.item.implemented {
                    theme.sidebar_foreground
                } else {
                    theme.tokens.muted_foreground
                },
            );
        }

        if !compact {
            let project_y = rect.origin.y + HEADER_H + NAVIGATION.len() as f32 * ROW_H + 18.0;
            draw_label(
                painter,
                if state.projects.is_empty() {
                    "项目"
                } else {
                    "最近项目"
                },
                Rect::xywh(rect.origin.x + 16.0, project_y, rect.size.x - 32.0, 24.0),
                11.0,
                600,
                theme.tokens.muted_foreground,
            );
            for row in Self::dynamic_row_layout(rect, state) {
                if row.selected {
                    painter.fill_round_rect(
                        row.rect,
                        theme.tokens.radius,
                        theme.tokens.row_selected,
                    );
                }
                let (left_inset, weight) = match row.target {
                    SidebarRowTarget::Project(_) => (8.0, 600),
                    SidebarRowTarget::Session(_) => (20.0, 400),
                };
                draw_label(
                    painter,
                    &row.label,
                    Rect::xywh(
                        row.rect.origin.x + left_inset,
                        row.rect.origin.y,
                        (row.rect.size.x - left_inset - 8.0).max(0.0),
                        row.rect.size.y,
                    ),
                    12.0,
                    weight,
                    theme.sidebar_foreground,
                );
            }
        }

        let footer = Self::footer_rect(rect);
        if footer.size.y <= 0.0 {
            return;
        }
        let footer_selected = Self::footer_selected(state);
        if footer_selected {
            painter.fill_round_rect(footer, theme.tokens.radius, theme.tokens.row_selected);
        }
        draw_label(
            painter,
            if compact {
                "设"
            } else {
                SETTINGS_FOOTER.label
            },
            Rect::xywh(
                footer.origin.x + 8.0,
                footer.origin.y,
                (footer.size.x - 16.0).max(0.0),
                footer.size.y,
            ),
            12.0,
            if footer_selected { 600 } else { 500 },
            theme.sidebar_foreground,
        );
    }
}

fn navigation_item_selected(state: &ZodeAppState, item: SidebarItem) -> bool {
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

#[derive(Debug)]
struct DynamicProject {
    workspace_uri: WorkspaceUri,
    sessions: Vec<ThreadSummary>,
    expanded: bool,
    available: bool,
    sort_key_ms: i64,
    toggleable: bool,
}

fn dynamic_projects(state: &ZodeAppState) -> Vec<DynamicProject> {
    let mut sessions = group_sessions(state.threads.clone())
        .into_iter()
        .map(|group| (group.workspace_uri, group.sessions))
        .collect::<BTreeMap<_, _>>();
    let known_projects = state
        .projects
        .iter()
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

fn dynamic_row_rect(rect: Rect, project_y: f32, index: usize) -> Rect {
    Rect::xywh(
        rect.origin.x + 8.0,
        project_y + 28.0 + index as f32 * ROW_H,
        (rect.size.x - 16.0).max(0.0),
        ROW_H,
    )
}

fn workspace_label(workspace: &WorkspaceUri, available: bool) -> String {
    let value = workspace.as_str().trim_end_matches('/');
    let name = value.rsplit('/').next().unwrap_or(value);
    if available {
        name.to_owned()
    } else {
        format!("{name} · unavailable")
    }
}

fn draw_label(
    painter: &mut dyn Painter,
    text: &str,
    rect: Rect,
    size: f32,
    weight: u16,
    color: jian_widgets::Color,
) {
    let layout = TextLayout::single_run(text, FONT, size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(
        &layout,
        Point2D::new(rect.origin.x, centered_text_baseline_y(rect, size)),
    );
}
