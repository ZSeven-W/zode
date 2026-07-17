use std::collections::BTreeMap;

use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{AppCommand, SettingsCategory, ShellRoute, ZodeAppState};
use zode_node_protocol::{SessionLocator, ThreadSummary, WorkspaceUri};

use super::row::{clip_to_viewport, paint_card, paint_divider, paint_heading, SECTION_TOP};
use crate::{paint_single_line, stable_widget_id, RectExt, SemanticIcon, WidgetId, ZodeTheme};

const GROUP_HEADING_HEIGHT: f32 = 24.0;
const GROUP_CARD_GAP: f32 = 10.0;
const GROUP_GAP: f32 = 32.0;
const TASK_ROW_HEIGHT: f32 = 64.0;
const ACTION_WIDTH: f32 = 94.0;
const ACTION_HEIGHT: f32 = 28.0;
const EMPTY_CARD_HEIGHT: f32 = 128.0;
const BOTTOM_GAP: f32 = 24.0;

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
    pub groups: Vec<ArchivedTaskGroupLayout>,
    pub empty_card: Option<Rect>,
    pub content_height: f32,
}

pub(super) fn layout(content: Rect, state: &ZodeAppState, offset: f32) -> ArchivedTasksLayout {
    let archived = archived_groups(state);
    if archived.is_empty() {
        return ArchivedTasksLayout {
            groups: Vec::new(),
            empty_card: Some(Rect::xywh(
                content.origin.x,
                content.origin.y + SECTION_TOP - offset,
                content.size.x,
                EMPTY_CARD_HEIGHT,
            )),
            content_height: content_height(state),
        };
    }

    let mut y = content.origin.y + SECTION_TOP - offset;
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
        groups,
        empty_card: None,
        content_height: content_height(state),
    }
}

pub(super) fn content_height(state: &ZodeAppState) -> f32 {
    let archived = archived_groups(state);
    if archived.is_empty() {
        return SECTION_TOP + EMPTY_CARD_HEIGHT + BOTTOM_GAP;
    }
    let groups_height = archived
        .iter()
        .map(|(_, threads)| {
            GROUP_HEADING_HEIGHT + GROUP_CARD_GAP + TASK_ROW_HEIGHT * threads.len() as f32
        })
        .sum::<f32>();
    SECTION_TOP + groups_height + GROUP_GAP * archived.len().saturating_sub(1) as f32 + BOTTOM_GAP
}

pub(super) fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if !matches!(
        state.presentation.route,
        ShellRoute::Settings(SettingsCategory::ArchivedTasks)
    ) {
        return None;
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
    offset: f32,
    theme: &ZodeTheme,
) {
    paint_heading(painter, content, "已归档任务", "本机任务", offset, theme);
    if let Some(card) = layout.empty_card {
        paint_card(painter, card, theme);
        paint_single_line(
            painter,
            "暂无已归档任务",
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
            "归档的任务会保留在本机，可在这里取消归档。",
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
    for thread in &state.threads {
        if state.archived_sessions.contains(&thread.session) {
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
