use std::collections::BTreeMap;

use jian_core::text_input::TextInputState;
use jian_widgets::{components::input::Input, HorizontalAlign, Painter, Rect};
use zode_app_model::{AppCommand, SettingsCategory, ShellRoute, ZodeAppState};
use zode_node_protocol::{SessionLocator, ThreadSummary, WorkspaceUri};

use super::row::{clip_to_viewport, paint_card, paint_divider, paint_heading, SECTION_TOP};
use crate::{paint_single_line, stable_widget_id, RectExt, SemanticIcon, WidgetId, ZodeTheme};

const GROUP_HEADING_HEIGHT: f32 = 24.0;
const TOOLBAR_HEIGHT: f32 = 36.0;
const TOOLBAR_GAP: f32 = 24.0;
const TOOLBAR_CONTROL_GAP: f32 = 10.0;
const FILTER_WIDTH: f32 = 176.0;
const GROUP_CARD_GAP: f32 = 10.0;
const GROUP_GAP: f32 = 32.0;
const TASK_ROW_HEIGHT: f32 = 64.0;
const ACTION_WIDTH: f32 = 94.0;
const ACTION_HEIGHT: f32 = 28.0;
const EMPTY_CARD_HEIGHT: f32 = 128.0;
const BOTTOM_GAP: f32 = 24.0;

pub const ARCHIVED_TASK_SEARCH_ID: WidgetId = WidgetId(8_400);
pub const ARCHIVED_TASK_FILTER_ID: WidgetId = WidgetId(8_401);

#[derive(Debug, Clone, PartialEq)]
pub struct ArchivedTaskRowLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub title_rect: Rect,
    pub detail_rect: Rect,
    pub action_rect: Rect,
    pub visible_action_rect: Option<Rect>,
    pub session: SessionLocator,
    pub workspace_uri: WorkspaceUri,
    pub title: String,
    pub command: AppCommand,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArchivedTaskGroupLayout {
    pub workspace_uri: WorkspaceUri,
    pub label: String,
    pub heading_rect: Rect,
    pub count_rect: Rect,
    pub card: Rect,
    pub rows: Vec<ArchivedTaskRowLayout>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArchivedTasksLayout {
    pub search_rect: Rect,
    pub filter_rect: Rect,
    pub filter_label: String,
    pub filter_enabled: bool,
    pub groups: Vec<ArchivedTaskGroupLayout>,
    pub empty_card: Option<Rect>,
    pub empty_title: Option<&'static str>,
    pub content_height: f32,
}

pub(super) fn layout(content: Rect, state: &ZodeAppState, offset: f32) -> ArchivedTasksLayout {
    let archived = archived_groups(state);
    let toolbar_y = content.origin.y + SECTION_TOP - offset;
    let filter_width = FILTER_WIDTH.min(content.size.x * 0.38);
    let filter_rect = Rect::xywh(
        content.max_x() - filter_width,
        toolbar_y,
        filter_width,
        TOOLBAR_HEIGHT,
    );
    let search_rect = Rect::xywh(
        content.origin.x,
        toolbar_y,
        (filter_rect.origin.x - content.origin.x - TOOLBAR_CONTROL_GAP).max(0.0),
        TOOLBAR_HEIGHT,
    );
    let filter_label = state
        .archived_tasks
        .workspace_filter
        .as_ref()
        .map(workspace_label)
        .unwrap_or_else(|| "所有项目".into());
    let filter_enabled = !archived_workspace_options(state).is_empty();
    let group_top = toolbar_y + TOOLBAR_HEIGHT + TOOLBAR_GAP;
    if archived.is_empty() {
        let has_archived = has_archived_tasks(state);
        return ArchivedTasksLayout {
            search_rect,
            filter_rect,
            filter_label,
            filter_enabled,
            groups: Vec::new(),
            empty_card: Some(Rect::xywh(
                content.origin.x,
                group_top,
                content.size.x,
                EMPTY_CARD_HEIGHT,
            )),
            empty_title: Some(if has_archived {
                "没有符合筛选条件的任务"
            } else {
                "暂无已归档任务"
            }),
            content_height: content_height(state),
        };
    }

    let mut y = group_top;
    let mut groups = Vec::with_capacity(archived.len());
    for (workspace_uri, threads) in archived {
        let heading_rect = Rect::xywh(content.origin.x, y, content.size.x, GROUP_HEADING_HEIGHT);
        let count_width = 92.0_f32.min(content.size.x);
        let count_rect = Rect::xywh(
            heading_rect.max_x() - count_width,
            heading_rect.origin.y,
            count_width,
            heading_rect.size.y,
        );
        let card = Rect::xywh(
            content.origin.x,
            heading_rect.max_y() + GROUP_CARD_GAP,
            content.size.x,
            TASK_ROW_HEIGHT * threads.len() as f32,
        );
        let rows = threads
            .into_iter()
            .enumerate()
            .map(|(index, thread)| task_row(content, card, index, thread))
            .collect();
        groups.push(ArchivedTaskGroupLayout {
            label: workspace_label(&workspace_uri),
            workspace_uri,
            heading_rect,
            count_rect,
            card,
            rows,
        });
        y = card.max_y() + GROUP_GAP;
    }

    ArchivedTasksLayout {
        search_rect,
        filter_rect,
        filter_label,
        filter_enabled,
        groups,
        empty_card: None,
        empty_title: None,
        content_height: content_height(state),
    }
}

pub(super) fn content_height(state: &ZodeAppState) -> f32 {
    let archived = archived_groups(state);
    if archived.is_empty() {
        return SECTION_TOP + TOOLBAR_HEIGHT + TOOLBAR_GAP + EMPTY_CARD_HEIGHT + BOTTOM_GAP;
    }
    let groups_height = archived
        .iter()
        .map(|(_, threads)| {
            GROUP_HEADING_HEIGHT + GROUP_CARD_GAP + TASK_ROW_HEIGHT * threads.len() as f32
        })
        .sum::<f32>();
    SECTION_TOP
        + TOOLBAR_HEIGHT
        + TOOLBAR_GAP
        + groups_height
        + GROUP_GAP * archived.len().saturating_sub(1) as f32
        + BOTTOM_GAP
}

pub(super) fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if !matches!(
        state.presentation.route,
        ShellRoute::Settings(SettingsCategory::ArchivedTasks)
    ) {
        return None;
    }
    if id == ARCHIVED_TASK_FILTER_ID {
        let options = archived_workspace_options(state);
        if options.is_empty() {
            return None;
        }
        let next = state
            .archived_tasks
            .workspace_filter
            .as_ref()
            .and_then(|current| options.iter().position(|workspace| workspace == current))
            .and_then(|index| options.get(index + 1).cloned())
            .or_else(|| {
                state
                    .archived_tasks
                    .workspace_filter
                    .is_none()
                    .then(|| options[0].clone())
            });
        return Some(AppCommand::SetArchivedTaskWorkspaceFilter(next));
    }
    state
        .threads
        .iter()
        .find(|thread| {
            state.archived_sessions.contains(&thread.session)
                && archived_task_widget_id(&thread.session) == id
        })
        .map(|thread| AppCommand::SetSessionArchived {
            session: thread.session.clone(),
            archived: false,
        })
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    content: Rect,
    layout: &ArchivedTasksLayout,
    state: &ZodeAppState,
    offset: f32,
    search_focused: bool,
    theme: &ZodeTheme,
) {
    paint_heading(painter, content, "已归档任务", "本机任务", offset, theme);
    let search_input = TextInputState::with_text(state.archived_tasks.search.clone());
    Input {
        state: &search_input,
        placeholder: "搜索已归档任务",
        focused: search_focused,
        font_size: 12.0,
        now_ms: 0,
        icon_d: Some(SemanticIcon::Search.path()),
    }
    .paint(painter, layout.search_rect, &theme.tokens);
    painter.fill_round_rect(layout.filter_rect, 9.0, theme.tokens.card);
    painter.stroke_round_rect(layout.filter_rect, 9.0, theme.tokens.border, 1.0);
    painter.stroke_svg_path(
        SemanticIcon::Folder.path(),
        jian_widgets::Point2D::new(
            layout.filter_rect.origin.x + 12.0,
            layout.filter_rect.origin.y + 10.0,
        ),
        16.0,
        if layout.filter_enabled {
            theme.tokens.foreground
        } else {
            theme.tokens.muted_foreground
        },
        SemanticIcon::Folder.stroke_width(),
    );
    paint_single_line(
        painter,
        &layout.filter_label,
        Rect::xywh(
            layout.filter_rect.origin.x + 36.0,
            layout.filter_rect.origin.y,
            (layout.filter_rect.size.x - 48.0).max(0.0),
            layout.filter_rect.size.y,
        ),
        12.0,
        500,
        if layout.filter_enabled {
            theme.tokens.foreground
        } else {
            theme.tokens.muted_foreground
        },
        HorizontalAlign::Start,
    );
    if let Some(card) = layout.empty_card {
        paint_card(painter, card, theme);
        paint_single_line(
            painter,
            layout.empty_title.unwrap_or("暂无已归档任务"),
            Rect::xywh(
                card.origin.x + 18.0,
                card.origin.y + 24.0,
                (card.size.x - 36.0).max(0.0),
                28.0,
            ),
            14.0,
            600,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        paint_single_line(
            painter,
            if !has_archived_tasks(state) {
                "归档的任务会保留在本机，可在这里取消归档。"
            } else {
                "调整搜索词或项目筛选后重试。"
            },
            Rect::xywh(
                card.origin.x + 18.0,
                card.origin.y + 58.0,
                (card.size.x - 36.0).max(0.0),
                28.0,
            ),
            12.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
        return;
    }

    for group in &layout.groups {
        let icon_rect = Rect::xywh(
            group.heading_rect.origin.x,
            group.heading_rect.origin.y + 4.0,
            16.0,
            16.0,
        );
        painter.stroke_svg_path(
            SemanticIcon::Folder.path(),
            icon_rect.origin,
            icon_rect.size.x,
            theme.tokens.muted_foreground,
            SemanticIcon::Folder.stroke_width(),
        );
        paint_single_line(
            painter,
            &group.label,
            Rect::xywh(
                icon_rect.max_x() + 8.0,
                group.heading_rect.origin.y,
                (group.count_rect.origin.x - icon_rect.max_x() - 16.0).max(0.0),
                group.heading_rect.size.y,
            ),
            13.0,
            600,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        paint_single_line(
            painter,
            &format!("{} 个任务", group.rows.len()),
            group.count_rect,
            11.0,
            450,
            theme.tokens.muted_foreground,
            HorizontalAlign::End,
        );
        paint_card(painter, group.card, theme);
        for (index, row) in group.rows.iter().enumerate() {
            if index > 0 {
                paint_divider(painter, group.card, index as f32 * TASK_ROW_HEIGHT, theme);
            }
            paint_single_line(
                painter,
                &row.title,
                row.title_rect,
                13.0,
                550,
                theme.tokens.foreground,
                HorizontalAlign::Start,
            );
            paint_single_line(
                painter,
                row.workspace_uri.as_str(),
                row.detail_rect,
                11.0,
                400,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
            painter.fill_round_rect(row.action_rect, 8.0, theme.tokens.row_selected);
            paint_single_line(
                painter,
                "取消归档",
                row.action_rect,
                12.0,
                550,
                theme.tokens.foreground,
                HorizontalAlign::Center,
            );
        }
    }
}

fn archived_groups(state: &ZodeAppState) -> Vec<(WorkspaceUri, Vec<&ThreadSummary>)> {
    let mut grouped = BTreeMap::<String, Vec<&ThreadSummary>>::new();
    let search = state.archived_tasks.search.trim().to_lowercase();
    for thread in &state.threads {
        let workspace_matches = state
            .archived_tasks
            .workspace_filter
            .as_ref()
            .is_none_or(|workspace| workspace == &thread.workspace_uri);
        let search_matches = search.is_empty()
            || thread.title.to_lowercase().contains(&search)
            || thread.session.session_id.to_lowercase().contains(&search)
            || thread
                .workspace_uri
                .as_str()
                .to_lowercase()
                .contains(&search)
            || workspace_label(&thread.workspace_uri)
                .to_lowercase()
                .contains(&search);
        if state.archived_sessions.contains(&thread.session) && workspace_matches && search_matches
        {
            grouped
                .entry(thread.workspace_uri.as_str().to_owned())
                .or_default()
                .push(thread);
        }
    }
    grouped
        .into_values()
        .map(|mut threads| {
            threads.sort_by(|left, right| {
                right
                    .updated_at_ms
                    .cmp(&left.updated_at_ms)
                    .then_with(|| left.session.session_id.cmp(&right.session.session_id))
            });
            (threads[0].workspace_uri.clone(), threads)
        })
        .collect()
}

fn archived_workspace_options(state: &ZodeAppState) -> Vec<WorkspaceUri> {
    let mut workspaces = BTreeMap::<String, WorkspaceUri>::new();
    for thread in &state.threads {
        if state.archived_sessions.contains(&thread.session) {
            workspaces
                .entry(thread.workspace_uri.as_str().to_owned())
                .or_insert_with(|| thread.workspace_uri.clone());
        }
    }
    workspaces.into_values().collect()
}

fn has_archived_tasks(state: &ZodeAppState) -> bool {
    state
        .threads
        .iter()
        .any(|thread| state.archived_sessions.contains(&thread.session))
}

fn task_row(
    viewport: Rect,
    card: Rect,
    index: usize,
    thread: &ThreadSummary,
) -> ArchivedTaskRowLayout {
    let rect = Rect::xywh(
        card.origin.x,
        card.origin.y + index as f32 * TASK_ROW_HEIGHT,
        card.size.x,
        TASK_ROW_HEIGHT,
    );
    let action_rect = Rect::xywh(
        rect.max_x() - ACTION_WIDTH - 18.0,
        rect.origin.y + (rect.size.y - ACTION_HEIGHT) / 2.0,
        ACTION_WIDTH,
        ACTION_HEIGHT,
    );
    let text_width = (action_rect.origin.x - rect.origin.x - 36.0).max(0.0);
    let title = if thread.title.trim().is_empty() {
        "无标题任务".to_owned()
    } else {
        thread.title.trim().to_owned()
    };
    ArchivedTaskRowLayout {
        id: archived_task_widget_id(&thread.session),
        rect,
        title_rect: Rect::xywh(rect.origin.x + 18.0, rect.origin.y + 8.0, text_width, 24.0),
        detail_rect: Rect::xywh(rect.origin.x + 18.0, rect.origin.y + 31.0, text_width, 22.0),
        action_rect,
        visible_action_rect: clip_to_viewport(action_rect, viewport),
        session: thread.session.clone(),
        workspace_uri: thread.workspace_uri.clone(),
        title,
        command: AppCommand::SetSessionArchived {
            session: thread.session.clone(),
            archived: false,
        },
    }
}

fn archived_task_widget_id(session: &SessionLocator) -> WidgetId {
    stable_widget_id(0xA7, session)
}

fn workspace_label(workspace_uri: &WorkspaceUri) -> String {
    workspace_uri
        .as_str()
        .trim_end_matches('/')
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(workspace_uri.as_str())
        .to_owned()
}
