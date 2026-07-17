mod action;
mod row;
mod section;

use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{
    environment_actions, environment_sections, AppCommand, EnvironmentActionKind, EnvironmentEntry,
    EnvironmentSection, EnvironmentSectionKind, LoadState, ZodeAppState,
};
use zode_node_protocol::SessionLocator;

use crate::{
    paint_single_line, stable_widget_id, PinnedSummaryMode, RectExt, SemanticIcon, WidgetId,
    ZodeTheme,
};

pub use action::EnvironmentActionLayout;
pub use section::EnvironmentSectionLayout;

pub const ENVIRONMENT_PANEL_ID: WidgetId = WidgetId(203);
pub const ENVIRONMENT_CLOSE_ID: WidgetId = WidgetId(100);
pub const ENVIRONMENT_REVIEW_ID: WidgetId = WidgetId(101);
pub const ENVIRONMENT_REFRESH_ID: WidgetId = WidgetId(200);
pub const ENVIRONMENT_OPEN_WORKSPACE_ID: WidgetId = WidgetId(201);
pub const ENVIRONMENT_COMMIT_PUSH_ID: WidgetId = WidgetId(202);

const PANEL_WIDTH: f32 = 300.0;
const PANEL_MIN_HEIGHT: f32 = 320.0;
const PANEL_MAX_HEIGHT: f32 = 512.0;
const PANEL_RADIUS: f32 = 20.0;
const PANEL_INSET: f32 = 16.0;
const CLOSE_SIZE: f32 = 20.0;
const STATUS_HEIGHT: f32 = 31.0;
const MIN_ACTION_PANEL_HEIGHT: f32 = 240.0;

#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentStatusLayout {
    pub message: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentPanelLayout {
    pub card: Rect,
    pub header: Rect,
    pub content: Rect,
    pub close_button: Rect,
    pub sections: Vec<EnvironmentSectionLayout>,
    pub statuses: Vec<EnvironmentStatusLayout>,
    pub repository_action_header: Option<Rect>,
    pub repository_actions: Vec<EnvironmentActionLayout>,
    pub review_button: Option<Rect>,
    pub last_row: Option<Rect>,
    pub environment_group_end: f32,
}

pub struct EnvironmentPanel;

impl EnvironmentPanel {
    pub fn layout(rect: Rect, state: &ZodeAppState) -> EnvironmentPanelLayout {
        let available_width = finite_non_negative(rect.size.x);
        let available_height = finite_non_negative(rect.size.y);
        let mut projected = environment_sections(state);
        projected.sort_by_key(|section| section_order(section.kind));
        append_sources_view_all(&mut projected);
        let projected_actions = environment_actions(state);
        let status_messages = status_messages(state);
        let top_entry_count = projected
            .iter()
            .filter(|section| is_environment_group(section.kind))
            .map(|section| section.entries.len())
            .sum::<usize>()
            + usize::from(
                !projected
                    .iter()
                    .any(|section| section.kind == EnvironmentSectionKind::Changes),
            );
        let secondary_sections = projected
            .iter()
            .filter(|section| is_secondary_group(section.kind))
            .collect::<Vec<_>>();
        let secondary_height = secondary_sections
            .iter()
            .map(|section| section::height(section, true))
            .sum::<f32>()
            + section::SECTION_GAP * secondary_sections.len() as f32;
        let environment_height = section::SECTION_HEADER_HEIGHT
            + top_entry_count as f32 * row::ROW_HEIGHT
            + action::ACTION_ROW_HEIGHT * 2.0
            + section::SECTION_GAP;
        let status_height = status_messages.len() as f32 * STATUS_HEIGHT;
        let desired_height =
            PANEL_INSET + environment_height + secondary_height + status_height + PANEL_INSET;
        let card_width = PANEL_WIDTH.min(available_width);
        let card_height = if available_height > 0.0 {
            desired_height
                .clamp(PANEL_MIN_HEIGHT, PANEL_MAX_HEIGHT)
                .min(available_height)
        } else {
            0.0
        };
        let card = Rect::xywh(
            rect.origin.x + (available_width - card_width).max(0.0),
            rect.origin.y,
            card_width,
            card_height,
        );
        let close_size = CLOSE_SIZE.min(card.size.x).min(card.size.y);
        let close_button = Rect::xywh(
            (card.max_x() - PANEL_INSET + 4.0 - close_size).max(card.origin.x),
            (card.origin.y + 12.0).min((card.max_y() - close_size).max(card.origin.y)),
            close_size,
            close_size,
        );
        let content_rect = Rect::xywh(
            card.origin.x + PANEL_INSET,
            card.origin.y + PANEL_INSET,
            (card.size.x - PANEL_INSET * 2.0).max(0.0),
            (card.size.y - PANEL_INSET * 2.0).max(0.0),
        );
        let header = Rect::xywh(
            content_rect.origin.x,
            content_rect.origin.y,
            content_rect.size.x,
            section::SECTION_HEADER_HEIGHT,
        );
        let shows_actions = card.size.y >= MIN_ACTION_PANEL_HEIGHT;
        let mut y = header.max_y();
        let mut sections = Vec::new();
        let mut action_rects = Vec::new();
        for kind in [
            EnvironmentSectionKind::Changes,
            EnvironmentSectionKind::Host,
            EnvironmentSectionKind::Branch,
        ] {
            let Some(projected_section) = take_section(&mut projected, kind) else {
                if kind == EnvironmentSectionKind::Changes {
                    let rect = Rect::xywh(
                        content_rect.origin.x,
                        y,
                        content_rect.size.x,
                        row::ROW_HEIGHT,
                    );
                    y = rect.max_y();
                    if shows_actions {
                        action_rects.push((
                            EnvironmentActionKind::RefreshStatus,
                            rect,
                            "变更".into(),
                            None,
                        ));
                    }
                }
                continue;
            };
            let mut layout = section::layout(projected_section, content_rect, y, None, false);
            y = layout.rect.max_y();
            if shows_actions {
                let action_kind = match kind {
                    EnvironmentSectionKind::Changes => Some(EnvironmentActionKind::RefreshStatus),
                    EnvironmentSectionKind::Host => Some(EnvironmentActionKind::OpenWorkspace),
                    _ => None,
                };
                if let (Some(action_kind), Some(first_row)) = (action_kind, layout.rows.first_mut())
                {
                    first_row.painted_by_action = true;
                    let label = action_row_label(action_kind, &first_row.entry);
                    let value = action_row_value(action_kind, &first_row.entry);
                    action_rects.push((action_kind, first_row.rect, label, value));
                }
            }
            sections.push(layout);
        }
        let commit_rect = Rect::xywh(
            content_rect.origin.x,
            y,
            content_rect.size.x,
            action::ACTION_ROW_HEIGHT,
        );
        y = commit_rect.max_y();
        let compare_rect = Rect::xywh(
            content_rect.origin.x,
            y,
            content_rect.size.x,
            action::ACTION_ROW_HEIGHT,
        );
        y = compare_rect.max_y();
        if shows_actions {
            action_rects.push((
                EnvironmentActionKind::CommitOrPush,
                commit_rect,
                "提交或推送".into(),
                None,
            ));
            action_rects.push((
                EnvironmentActionKind::CompareWorkspaceToHead,
                compare_rect,
                "比较分支".into(),
                None,
            ));
        }
        for (kind, rect) in [
            (EnvironmentSectionKind::RepositoryActions, commit_rect),
            (EnvironmentSectionKind::Comparisons, compare_rect),
        ] {
            if let Some(projected_section) = take_section(&mut projected, kind) {
                sections.push(section::footer(projected_section, rect));
            }
        }
        let repository_actions = projected_actions
            .into_iter()
            .filter_map(|action| {
                let (_, rect, label, value) = action_rects
                    .iter()
                    .find(|(kind, _, _, _)| *kind == action.kind)?;
                Some(EnvironmentActionLayout {
                    id: action_widget_id(action.kind),
                    action,
                    rect: *rect,
                    label: label.clone(),
                    value: value.clone(),
                })
            })
            .collect::<Vec<_>>();
        let review_button = repository_actions
            .iter()
            .find(|layout| {
                layout.action.kind == EnvironmentActionKind::CompareWorkspaceToHead
                    && layout.action.enabled()
            })
            .map(|layout| layout.rect);
        let environment_group_end = y;
        y += section::SECTION_GAP;
        for projected_section in projected
            .into_iter()
            .filter(|section| is_secondary_group(section.kind))
        {
            let title = projected_section.kind.title();
            let layout = section::layout(projected_section, content_rect, y, Some(title), true);
            y = layout.rect.max_y() + section::SECTION_GAP;
            sections.push(layout);
        }
        let statuses = status_messages
            .into_iter()
            .map(|message| {
                let rect = Rect::xywh(content_rect.origin.x, y, content_rect.size.x, STATUS_HEIGHT);
                y += STATUS_HEIGHT;
                EnvironmentStatusLayout { message, rect }
            })
            .collect::<Vec<_>>();
        let last_row = statuses.last().map(|status| status.rect).or_else(|| {
            sections
                .iter()
                .rev()
                .find(|section| !section.footer)
                .and_then(|section| section.last_row())
                .or(Some(compare_rect))
        });

        EnvironmentPanelLayout {
            card,
            header,
            content: content_rect,
            close_button,
            sections,
            statuses,
            repository_action_header: None,
            repository_actions,
            review_button,
            last_row,
            environment_group_end,
        }
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        Self::paint_for_mode(painter, rect, state, PinnedSummaryMode::Overlay, theme);
    }

    pub fn paint_for_mode(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &ZodeAppState,
        mode: PinnedSummaryMode,
        theme: &ZodeTheme,
    ) {
        let layout = Self::layout(rect, state);
        if layout.card.size.x <= 0.0 || layout.card.size.y <= 0.0 {
            return;
        }
        painter.fill_drop_shadow(
            Rect::xywh(
                layout.card.origin.x,
                layout.card.origin.y + 8.0,
                layout.card.size.x,
                layout.card.size.y,
            ),
            PANEL_RADIUS,
            24.0,
            theme.tokens.foreground.with_alpha(0.08),
        );
        painter.fill_round_rect(layout.card, PANEL_RADIUS, theme.tokens.card);
        painter.stroke_round_rect(layout.card, PANEL_RADIUS, theme.tokens.border, 1.0);
        painter.save();
        painter.clip_rect(layout.card);
        painter.save();
        painter.clip_rect(layout.content);
        paint_single_line(
            painter,
            "环境信息",
            layout.header,
            14.0,
            500,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
        for section in &layout.sections {
            section::paint(painter, section, theme);
        }
        action::paint(painter, &layout.repository_actions, theme);
        let separator_y = layout.environment_group_end + section::SECTION_GAP / 2.0;
        painter.stroke_line(
            jian_widgets::Point2D::new(layout.content.origin.x, separator_y),
            jian_widgets::Point2D::new(layout.content.max_x(), separator_y),
            theme.tokens.border.with_alpha(0.72),
            1.0,
        );
        for status in &layout.statuses {
            paint_single_line(
                painter,
                &status.message,
                status.rect,
                14.0,
                400,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
        }
        painter.restore();
        if mode == PinnedSummaryMode::Overlay {
            paint_close_button(painter, layout.close_button, theme);
        }
        painter.restore();
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if id == ENVIRONMENT_CLOSE_ID {
            return Some(AppCommand::SetPinnedSummaryOverlayOpen(false));
        }
        let action = environment_actions(state)
            .into_iter()
            .find(|action| action_widget_id(action.kind) == id && action.enabled())?;
        let session = state.current_session.as_ref()?.clone();
        Some(AppCommand::RunEnvironmentAction {
            session,
            action: action.kind,
        })
    }

    pub fn section_widget_id(session: &SessionLocator, kind: EnvironmentSectionKind) -> WidgetId {
        stable_widget_id(0x62, &(session, kind))
    }

    pub fn section_accessibility_name(layout: &EnvironmentSectionLayout) -> String {
        section::accessibility_name(layout)
    }
}

fn append_sources_view_all(sections: &mut [EnvironmentSection]) {
    let Some(sources) = sections
        .iter_mut()
        .find(|section| section.kind == EnvironmentSectionKind::Sources)
    else {
        return;
    };
    sources.entries.push(EnvironmentEntry {
        id: row::SOURCES_VIEW_ALL_ID.into(),
        label: "查看全部".into(),
        value: None,
    });
}

fn take_section(
    sections: &mut Vec<EnvironmentSection>,
    kind: EnvironmentSectionKind,
) -> Option<EnvironmentSection> {
    sections
        .iter()
        .position(|section| section.kind == kind)
        .map(|index| sections.remove(index))
}

fn action_row_label(kind: EnvironmentActionKind, entry: &EnvironmentEntry) -> String {
    match kind {
        EnvironmentActionKind::RefreshStatus => "变更".into(),
        EnvironmentActionKind::OpenWorkspace => {
            entry.value.clone().unwrap_or_else(|| entry.label.clone())
        }
        EnvironmentActionKind::CompareWorkspaceToHead => "比较分支".into(),
        EnvironmentActionKind::CommitOrPush => "提交或推送".into(),
    }
}

fn action_row_value(kind: EnvironmentActionKind, entry: &EnvironmentEntry) -> Option<String> {
    (kind == EnvironmentActionKind::RefreshStatus)
        .then(|| entry.value.clone())
        .flatten()
}

pub const fn action_widget_id(kind: EnvironmentActionKind) -> WidgetId {
    match kind {
        EnvironmentActionKind::RefreshStatus => ENVIRONMENT_REFRESH_ID,
        EnvironmentActionKind::CompareWorkspaceToHead => ENVIRONMENT_REVIEW_ID,
        EnvironmentActionKind::OpenWorkspace => ENVIRONMENT_OPEN_WORKSPACE_ID,
        EnvironmentActionKind::CommitOrPush => ENVIRONMENT_COMMIT_PUSH_ID,
    }
}

fn status_messages(state: &ZodeAppState) -> Vec<String> {
    let Some(presentation) = state.current_session_presentation() else {
        return vec!["选择任务以查看环境".into()];
    };
    let mut messages = Vec::new();
    match &presentation.context {
        LoadState::Idle => messages.push("环境信息尚未加载".into()),
        LoadState::Loading => messages.push("环境信息加载中".into()),
        LoadState::Failed(error) => messages.push(format!("环境加载失败：{error}")),
        LoadState::Ready(_) => {}
    }
    match &presentation.diff.load {
        LoadState::Idle => messages.push("变更尚未加载".into()),
        LoadState::Loading => messages.push("变更加载中".into()),
        LoadState::Failed(error) => messages.push(format!("变更加载失败：{error}")),
        LoadState::Ready(_) => {}
    }
    messages
}

fn paint_close_button(painter: &mut dyn Painter, close_button: Rect, theme: &ZodeTheme) {
    let close_icon_size = 14.0_f32.min(close_button.size.x.min(close_button.size.y));
    painter.stroke_svg_path(
        SemanticIcon::Close.path(),
        jian_widgets::Point2D::new(
            close_button.origin.x + (close_button.size.x - close_icon_size) / 2.0,
            close_button.origin.y + (close_button.size.y - close_icon_size) / 2.0,
        ),
        close_icon_size,
        theme.tokens.muted_foreground,
        SemanticIcon::Close.stroke_width(),
    );
}

const fn section_order(kind: EnvironmentSectionKind) -> u8 {
    match kind {
        EnvironmentSectionKind::Changes => 0,
        EnvironmentSectionKind::Host => 1,
        EnvironmentSectionKind::Branch => 2,
        EnvironmentSectionKind::RepositoryActions => 3,
        EnvironmentSectionKind::Comparisons => 4,
        EnvironmentSectionKind::Subagents => 5,
        EnvironmentSectionKind::BackgroundProcesses => 6,
        EnvironmentSectionKind::Sources => 7,
    }
}

const fn is_environment_group(kind: EnvironmentSectionKind) -> bool {
    matches!(
        kind,
        EnvironmentSectionKind::Changes
            | EnvironmentSectionKind::Host
            | EnvironmentSectionKind::Branch
    )
}

const fn is_secondary_group(kind: EnvironmentSectionKind) -> bool {
    matches!(
        kind,
        EnvironmentSectionKind::Subagents
            | EnvironmentSectionKind::BackgroundProcesses
            | EnvironmentSectionKind::Sources
    )
}

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}
