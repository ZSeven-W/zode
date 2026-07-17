use jian_widgets::Rect;
use zode_app_model::{ProjectDisplayMode, ZodeAppState};
use zode_node_protocol::{SessionLocator, ThreadSummary, WorkspaceUri};

use super::{
    dynamic_projects, pinned_tasks, projectless_tasks, workspace_label, ProjectSidebar,
    SidebarItem, SidebarRowLayout, SidebarRowTarget, NAVIGATION, SIDEBAR_PROJECTS_MORE_ID,
    SIDEBAR_PROJECTS_NEW_ID, SIDEBAR_PROJECTS_SECTION_ID, SIDEBAR_SHOW_ALL_PROJECTS_ID,
    SIDEBAR_TASKS_MORE_ID, SIDEBAR_TASKS_NEW_ID, SIDEBAR_TASKS_SECTION_ID, SIDEBAR_TASKS_TOGGLE_ID,
};
use crate::{RectExt, WidgetId};

pub(super) const TITLEBAR_H: f32 = 38.0;
pub(super) const BRAND_H: f32 = 47.0;
pub(super) const ROW_SLOT_H: f32 = 31.0;
pub(super) const ROW_H: f32 = 30.0;
pub(super) const FOOTER_H: f32 = 46.0;
pub(super) const ICON_SIZE: f32 = 14.0;
const MAX_PROJECTS: usize = 5;
const MAX_PROJECT_SESSIONS: usize = 5;
const SECTION_H: f32 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarNavigationRowLayout {
    pub index: usize,
    pub rect: Rect,
    pub item: SidebarItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSection {
    Pinned,
    Projects,
    Tasks,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SidebarSectionLayout {
    pub id: WidgetId,
    pub section: SidebarSection,
    pub rect: Rect,
    pub label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarControlTarget {
    ShowAllProjects,
    ShowAllProjectSessions(WorkspaceUri),
    ToggleTasks,
    ProjectsMenu,
    NewProject,
    TasksMenu,
    NewProjectlessTask,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SidebarControlLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub label: String,
    pub target: SidebarControlTarget,
}

impl SidebarControlLayout {
    pub fn actionable(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SidebarLabelLayout {
    pub rect: Rect,
    pub label: &'static str,
    pub nested: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SidebarLayout {
    pub rect: Rect,
    pub compact: bool,
    pub brand: Rect,
    pub scroll_viewport: Rect,
    pub footer: Rect,
    pub profile: Rect,
    pub help: Rect,
    pub navigation_rows: Vec<SidebarNavigationRowLayout>,
    pub sections: Vec<SidebarSectionLayout>,
    pub rows: Vec<SidebarRowLayout>,
    pub controls: Vec<SidebarControlLayout>,
    pub labels: Vec<SidebarLabelLayout>,
    pub content_height: f32,
    pub max_scroll: f32,
    pub scroll_offset: f32,
}

#[derive(Default)]
struct RawContent {
    navigation: Vec<SidebarNavigationRowLayout>,
    sections: Vec<SidebarSectionLayout>,
    rows: Vec<SidebarRowLayout>,
    controls: Vec<SidebarControlLayout>,
    labels: Vec<SidebarLabelLayout>,
    y: f32,
    shortcut: usize,
}

pub(super) fn build(rect: Rect, state: &ZodeAppState) -> SidebarLayout {
    let compact = rect.size.x < 100.0;
    let footer = footer_rect(rect);
    let viewport = scroll_viewport(rect);
    let brand = Rect::xywh(
        rect.origin.x + 16.0,
        rect.origin.y + TITLEBAR_H,
        (rect.size.x - 32.0).max(0.0),
        BRAND_H,
    );
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return SidebarLayout {
            rect,
            compact,
            brand,
            scroll_viewport: viewport,
            footer,
            profile: profile_rect(footer),
            help: help_rect(footer),
            navigation_rows: Vec::new(),
            sections: Vec::new(),
            rows: Vec::new(),
            controls: Vec::new(),
            labels: Vec::new(),
            content_height: 0.0,
            max_scroll: 0.0,
            scroll_offset: 0.0,
        };
    }

    let mut raw = build_raw(rect, state, compact);
    let content_height = (raw.y + 8.0).max(0.0);
    let max_scroll = (content_height - viewport.size.y).max(0.0);
    let offset = state.sidebar.scroll_offset.clamp(0.0, max_scroll);

    translate_navigation(&mut raw.navigation, viewport, offset);
    translate_sections(&mut raw.sections, viewport, offset);
    translate_rows(&mut raw.rows, viewport, offset);
    translate_controls(&mut raw.controls, viewport, offset);
    translate_labels(&mut raw.labels, viewport, offset);

    let mut navigation_rows = vec![fixed_new_task_row(rect)];
    navigation_rows.extend(
        raw.navigation
            .into_iter()
            .filter(|row| intersects(row.rect, viewport)),
    );
    SidebarLayout {
        rect,
        compact,
        brand,
        scroll_viewport: viewport,
        footer,
        profile: profile_rect(footer),
        help: help_rect(footer),
        navigation_rows,
        sections: raw
            .sections
            .into_iter()
            .filter(|section| intersects(section.rect, viewport))
            .collect(),
        rows: raw
            .rows
            .into_iter()
            .filter(|row| intersects(row.rect, viewport))
            .collect(),
        controls: raw
            .controls
            .into_iter()
            .filter(|control| intersects(control.rect, viewport))
            .collect(),
        labels: raw
            .labels
            .into_iter()
            .filter(|label| intersects(label.rect, viewport))
            .collect(),
        content_height,
        max_scroll,
        scroll_offset: offset,
    }
}

fn build_raw(rect: Rect, state: &ZodeAppState, compact: bool) -> RawContent {
    let mut raw = RawContent::default();
    for (index, item) in NAVIGATION.iter().copied().enumerate().skip(1) {
        raw.navigation.push(SidebarNavigationRowLayout {
            index,
            rect: raw_row_rect(rect, raw.y),
            item,
        });
        raw.y += ROW_SLOT_H;
    }
    if compact {
        return raw;
    }
    raw.y += 12.0;

    let pinned = pinned_tasks(state);
    if !pinned.is_empty() {
        raw.push_section(rect, SidebarSection::Pinned, "置顶");
        for thread in pinned {
            raw.push_thread(rect, state, thread, true, false);
        }
        raw.y += 10.0;
    }

    let projects_header_y = raw.y;
    raw.push_section(rect, SidebarSection::Projects, "项目");
    raw.controls.extend([
        SidebarControlLayout {
            id: SIDEBAR_PROJECTS_MORE_ID,
            rect: Rect::xywh(rect.max_x() - 58.0, projects_header_y + 2.0, 24.0, 24.0),
            label: "项目菜单".into(),
            target: SidebarControlTarget::ProjectsMenu,
        },
        SidebarControlLayout {
            id: SIDEBAR_PROJECTS_NEW_ID,
            rect: Rect::xywh(rect.max_x() - 32.0, projects_header_y + 2.0, 24.0, 24.0),
            label: "新建项目".into(),
            target: SidebarControlTarget::NewProject,
        },
    ]);
    let projects = dynamic_projects(state);
    if state.sidebar.project_display_mode == ProjectDisplayMode::Flat {
        for project in &projects {
            for thread in project.sessions.iter().cloned() {
                raw.push_thread(rect, state, thread, false, false);
            }
        }
    } else {
        let project_limit = if state.sidebar.show_all_projects {
            projects.len()
        } else {
            MAX_PROJECTS
        };
        for project in projects.iter().take(project_limit) {
            raw.rows.push(SidebarRowLayout {
                id: ProjectSidebar::project_widget_id(&project.workspace_uri),
                rect: raw_row_rect(rect, raw.y),
                label: workspace_label(&project.workspace_uri, project.available),
                target: SidebarRowTarget::Project(project.workspace_uri.clone()),
                actionable: project.toggleable,
                selected: state.presentation.route == zode_app_model::ShellRoute::Conversation
                    && state.current_session.is_none()
                    && state.active_workspace.as_ref() == Some(&project.workspace_uri),
                status: None,
                pinned: false,
                shortcut: None,
                pin_id: None,
                archive_id: None,
                more_id: project
                    .toggleable
                    .then(|| ProjectSidebar::project_more_widget_id(&project.workspace_uri)),
                new_id: (project.toggleable && project.available)
                    .then(|| ProjectSidebar::project_new_widget_id(&project.workspace_uri)),
                workspace_uri: Some(project.workspace_uri.clone()),
            });
            raw.y += ROW_SLOT_H;
            if !project.expanded {
                continue;
            }
            if project.sessions.is_empty() {
                raw.labels.push(SidebarLabelLayout {
                    rect: raw_nested_row_rect(rect, raw.y),
                    label: "无任务",
                    nested: true,
                });
                raw.y += ROW_SLOT_H;
                continue;
            }
            let show_all = state
                .sidebar
                .show_all_project_sessions
                .contains(&project.workspace_uri);
            let session_limit = if show_all {
                project.sessions.len()
            } else {
                MAX_PROJECT_SESSIONS
            };
            for thread in project.sessions.iter().take(session_limit).cloned() {
                raw.push_thread(rect, state, thread, false, true);
            }
            if !show_all && project.sessions.len() > MAX_PROJECT_SESSIONS {
                raw.controls.push(SidebarControlLayout {
                    id: ProjectSidebar::show_all_project_sessions_widget_id(&project.workspace_uri),
                    rect: raw_nested_row_rect(rect, raw.y),
                    label: "展开显示".into(),
                    target: SidebarControlTarget::ShowAllProjectSessions(
                        project.workspace_uri.clone(),
                    ),
                });
                raw.y += ROW_SLOT_H;
            }
        }
        if !state.sidebar.show_all_projects && projects.len() > MAX_PROJECTS {
            raw.controls.push(SidebarControlLayout {
                id: SIDEBAR_SHOW_ALL_PROJECTS_ID,
                rect: raw_row_rect(rect, raw.y),
                label: "展开显示".into(),
                target: SidebarControlTarget::ShowAllProjects,
            });
            raw.y += ROW_SLOT_H;
        }
    }

    raw.y += 10.0;
    let header_y = raw.y;
    raw.push_section(rect, SidebarSection::Tasks, "任务");
    raw.controls.extend([
        SidebarControlLayout {
            id: SIDEBAR_TASKS_TOGGLE_ID,
            rect: Rect::xywh(rect.origin.x + 8.0, header_y, 72.0, SECTION_H),
            label: if state.sidebar.tasks_expanded {
                "折叠任务".into()
            } else {
                "展开任务".into()
            },
            target: SidebarControlTarget::ToggleTasks,
        },
        SidebarControlLayout {
            id: SIDEBAR_TASKS_MORE_ID,
            rect: Rect::xywh(rect.max_x() - 58.0, header_y + 2.0, 24.0, 24.0),
            label: "任务菜单".into(),
            target: SidebarControlTarget::TasksMenu,
        },
        SidebarControlLayout {
            id: SIDEBAR_TASKS_NEW_ID,
            rect: Rect::xywh(rect.max_x() - 32.0, header_y + 2.0, 24.0, 24.0),
            label: "新建无项目任务".into(),
            target: SidebarControlTarget::NewProjectlessTask,
        },
    ]);
    if state.sidebar.tasks_expanded {
        for thread in projectless_tasks(state) {
            raw.push_thread(rect, state, thread, false, false);
        }
    }
    raw
}

impl RawContent {
    fn push_section(&mut self, rect: Rect, section: SidebarSection, label: &'static str) {
        self.sections.push(SidebarSectionLayout {
            id: match section {
                SidebarSection::Pinned => WidgetId(9_099),
                SidebarSection::Projects => SIDEBAR_PROJECTS_SECTION_ID,
                SidebarSection::Tasks => SIDEBAR_TASKS_SECTION_ID,
            },
            section,
            rect: Rect::xywh(rect.origin.x + 16.0, self.y, rect.size.x - 32.0, SECTION_H),
            label,
        });
        self.y += SECTION_H;
    }

    fn push_thread(
        &mut self,
        rect: Rect,
        state: &ZodeAppState,
        thread: ThreadSummary,
        pinned: bool,
        nested: bool,
    ) {
        self.shortcut += 1;
        let shortcut = (self.shortcut <= 5).then_some(self.shortcut);
        let projectless = state.is_projectless_workspace(&thread.workspace_uri);
        let session = thread.session;
        self.rows.push(SidebarRowLayout {
            id: ProjectSidebar::session_widget_id(&session),
            rect: if nested && !pinned && !projectless {
                raw_nested_row_rect(rect, self.y)
            } else {
                raw_row_rect(rect, self.y)
            },
            label: thread.title,
            target: if projectless {
                SidebarRowTarget::Task(session.clone())
            } else {
                SidebarRowTarget::Session(session.clone())
            },
            actionable: true,
            selected: state.presentation.route == zode_app_model::ShellRoute::Conversation
                && state.current_session.as_ref() == Some(&session),
            status: Some(thread.status),
            pinned,
            shortcut,
            pin_id: Some(ProjectSidebar::session_pin_widget_id(&session)),
            archive_id: Some(ProjectSidebar::session_archive_widget_id(&session)),
            more_id: None,
            new_id: None,
            workspace_uri: Some(thread.workspace_uri),
        });
        self.y += ROW_SLOT_H;
    }
}

pub(super) fn ordered_sessions(state: &ZodeAppState) -> std::vec::IntoIter<SessionLocator> {
    let mut sessions = pinned_tasks(state)
        .into_iter()
        .map(|thread| thread.session)
        .collect::<Vec<_>>();
    let projects = dynamic_projects(state);
    if state.sidebar.project_display_mode == ProjectDisplayMode::Flat {
        sessions.extend(
            projects
                .into_iter()
                .flat_map(|project| project.sessions)
                .map(|thread| thread.session),
        );
    } else {
        let project_limit = if state.sidebar.show_all_projects {
            projects.len()
        } else {
            MAX_PROJECTS
        };
        for project in projects.into_iter().take(project_limit) {
            if !project.expanded {
                continue;
            }
            let session_limit = if state
                .sidebar
                .show_all_project_sessions
                .contains(&project.workspace_uri)
            {
                project.sessions.len()
            } else {
                MAX_PROJECT_SESSIONS
            };
            sessions.extend(
                project
                    .sessions
                    .into_iter()
                    .take(session_limit)
                    .map(|thread| thread.session),
            );
        }
    }
    if state.sidebar.tasks_expanded {
        sessions.extend(
            projectless_tasks(state)
                .into_iter()
                .map(|thread| thread.session),
        );
    }
    sessions.into_iter()
}

pub(super) fn footer_rect(rect: Rect) -> Rect {
    let height = FOOTER_H.min(rect.size.y.max(0.0));
    Rect::xywh(rect.origin.x, rect.max_y() - height, rect.size.x, height)
}

pub(super) fn profile_rect(footer: Rect) -> Rect {
    Rect::xywh(
        footer.origin.x,
        footer.origin.y,
        (footer.size.x - 42.0).max(0.0),
        footer.size.y,
    )
}

pub(super) fn help_rect(footer: Rect) -> Rect {
    Rect::xywh(
        footer.max_x() - 38.0,
        footer.origin.y + (footer.size.y - 30.0) / 2.0,
        30.0,
        30.0,
    )
}

pub(super) fn scroll_viewport(rect: Rect) -> Rect {
    let top = rect.origin.y + TITLEBAR_H + BRAND_H + ROW_SLOT_H;
    let bottom = footer_rect(rect).origin.y;
    Rect::xywh(
        rect.origin.x,
        top.min(bottom),
        rect.size.x,
        (bottom - top).max(0.0),
    )
}

pub(super) fn navigation_rows(rect: Rect, offset: f32) -> Vec<SidebarNavigationRowLayout> {
    let viewport = scroll_viewport(rect);
    let mut rows = vec![fixed_new_task_row(rect)];
    rows.extend(
        NAVIGATION
            .iter()
            .copied()
            .enumerate()
            .skip(1)
            .map(|(index, item)| SidebarNavigationRowLayout {
                index,
                rect: Rect::xywh(
                    rect.origin.x + 8.0,
                    viewport.origin.y + (index - 1) as f32 * ROW_SLOT_H - offset + 1.0,
                    (rect.size.x - 16.0).max(0.0),
                    ROW_H,
                ),
                item,
            }),
    );
    rows
}

fn fixed_new_task_row(rect: Rect) -> SidebarNavigationRowLayout {
    SidebarNavigationRowLayout {
        index: 0,
        rect: Rect::xywh(
            rect.origin.x + 8.0,
            rect.origin.y + TITLEBAR_H + BRAND_H + 1.0,
            (rect.size.x - 16.0).max(0.0),
            ROW_H,
        ),
        item: NAVIGATION[0],
    }
}

fn raw_row_rect(rect: Rect, y: f32) -> Rect {
    Rect::xywh(
        rect.origin.x + 8.0,
        y + 1.0,
        (rect.size.x - 16.0).max(0.0),
        ROW_H,
    )
}

fn raw_nested_row_rect(rect: Rect, y: f32) -> Rect {
    Rect::xywh(
        rect.origin.x + 32.0,
        y + 1.0,
        (rect.size.x - 40.0).max(0.0),
        ROW_H,
    )
}

fn translate_navigation(rows: &mut [SidebarNavigationRowLayout], viewport: Rect, offset: f32) {
    for row in rows {
        row.rect.origin.y += viewport.origin.y - offset;
    }
}

fn translate_sections(rows: &mut [SidebarSectionLayout], viewport: Rect, offset: f32) {
    for row in rows {
        row.rect.origin.y += viewport.origin.y - offset;
    }
}

fn translate_rows(rows: &mut [SidebarRowLayout], viewport: Rect, offset: f32) {
    for row in rows {
        row.rect.origin.y += viewport.origin.y - offset;
    }
}

fn translate_controls(rows: &mut [SidebarControlLayout], viewport: Rect, offset: f32) {
    for row in rows {
        row.rect.origin.y += viewport.origin.y - offset;
    }
}

fn translate_labels(rows: &mut [SidebarLabelLayout], viewport: Rect, offset: f32) {
    for row in rows {
        row.rect.origin.y += viewport.origin.y - offset;
    }
}

fn intersects(rect: Rect, viewport: Rect) -> bool {
    rect.size.x > 0.0
        && rect.size.y > 0.0
        && rect.max_x() > viewport.origin.x
        && rect.origin.x < viewport.max_x()
        && rect.max_y() > viewport.origin.y
        && rect.origin.y < viewport.max_y()
}
