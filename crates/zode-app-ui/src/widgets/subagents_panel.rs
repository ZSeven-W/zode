//! M2 dedicated sub-agent panel (see `docs/proposals/subagent-panel-m2.md`).
//! Opened from the environment card's compact Subagents row rather than
//! `PanelPicker`'s home grid - `SecondaryPane::Subagents` is deliberately
//! absent from `panel_picker::descriptors`. Lists every `Task`-spawned
//! sub-agent for the current session in two sections ("已开启"/"完成 · N"),
//! paginating the completed section rather than rendering an unbounded
//! list. Per-row click-through to a per-agent transcript is out of scope
//! here (M3, see the design doc's "risk" section) - rows are informational
//! only.

use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{
    subagent_status_label, subagents_visible_count, AppCommand, SecondaryPane, ZodeAppState,
    SUBAGENTS_PAGE_SIZE,
};
use zode_node_protocol::{SubagentSnapshot, SubagentStatus};

use super::subagent_avatar::subagent_avatar_color;
use super::transcript::relative_time;
use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme};

pub const SUBAGENTS_PANEL_CLOSE_ID: WidgetId = WidgetId(260);
pub const SUBAGENTS_PANEL_SHOW_MORE_ID: WidgetId = WidgetId(261);

const HEADER_HEIGHT: f32 = 46.0;
const SECTION_HEADER_HEIGHT: f32 = 26.0;
const ROW_HEIGHT: f32 = 48.0;
const EMPTY_STATE_HEIGHT: f32 = 40.0;
const SHOW_MORE_HEIGHT: f32 = 32.0;
const ROW_PAD_X: f32 = 16.0;
const AVATAR_SIZE: f32 = 20.0;
const AVATAR_GAP: f32 = 10.0;
const TIME_COLUMN_WIDTH: f32 = 64.0;

/// One row's full snapshot, kept owned (rather than borrowed) so the layout
/// can outlive the presentation state it was built from - mirrors
/// `EnvironmentRowLayout` holding an owned `EnvironmentEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct SubagentRowLayout {
    pub subagent: SubagentSnapshot,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubagentsPanelLayout {
    pub rect: Rect,
    pub header: Rect,
    pub close_button: Rect,
    pub running_header: Rect,
    pub running_rows: Vec<SubagentRowLayout>,
    /// Present (and painted) only when there are no running sub-agents.
    pub running_empty: Option<Rect>,
    pub completed_header: Rect,
    pub completed_total: usize,
    pub completed_rows: Vec<SubagentRowLayout>,
    /// Present only while more completed rows exist beyond the current page.
    pub show_more: Option<Rect>,
    pub show_more_label: String,
}

pub struct SubagentsPanel;

impl SubagentsPanel {
    pub fn layout(rect: Rect, state: &ZodeAppState) -> SubagentsPanelLayout {
        let header_height = HEADER_HEIGHT.min(rect.size.y.max(0.0));
        let header = Rect::xywh(rect.origin.x, rect.origin.y, rect.size.x, header_height);
        let close_size = 32.0_f32.min(rect.size.x.max(0.0)).min(header_height);
        let close_button = Rect::xywh(
            (rect.max_x() - 40.0).max(rect.origin.x),
            rect.origin.y + (header_height - close_size).max(0.0) / 2.0,
            close_size,
            close_size,
        );

        let (running, completed, visible_count) = subagent_lists(state);
        let mut y = header.max_y();

        let running_header = Rect::xywh(rect.origin.x, y, rect.size.x, SECTION_HEADER_HEIGHT);
        y = running_header.max_y();
        let (running_rows, running_empty, next_y) = if running.is_empty() {
            (
                Vec::new(),
                Some(Rect::xywh(
                    rect.origin.x,
                    y,
                    rect.size.x,
                    EMPTY_STATE_HEIGHT,
                )),
                y + EMPTY_STATE_HEIGHT,
            )
        } else {
            let rows = layout_rows(running, rect, y);
            let next_y = y + rows.len() as f32 * ROW_HEIGHT;
            (rows, None, next_y)
        };
        y = next_y;

        let completed_header = Rect::xywh(rect.origin.x, y, rect.size.x, SECTION_HEADER_HEIGHT);
        y = completed_header.max_y();
        let shown = completed.len().min(visible_count);
        let completed_rows = layout_rows(completed[..shown].to_vec(), rect, y);
        y += completed_rows.len() as f32 * ROW_HEIGHT;
        let remaining = completed.len() - shown;
        let (show_more, show_more_label) = if remaining > 0 {
            let more = remaining.min(SUBAGENTS_PAGE_SIZE);
            (
                Some(Rect::xywh(rect.origin.x, y, rect.size.x, SHOW_MORE_HEIGHT)),
                format!("再显示 {more} 个"),
            )
        } else {
            (None, String::new())
        };

        SubagentsPanelLayout {
            rect,
            header,
            close_button,
            running_header,
            running_rows,
            running_empty,
            completed_header,
            completed_total: completed.len(),
            completed_rows,
            show_more,
            show_more_label,
        }
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        let layout = Self::layout(rect, state);
        if layout.rect.size.x <= 0.0 || layout.rect.size.y <= 0.0 {
            return;
        }
        painter.fill_rect(layout.rect, theme.tokens.background);
        Self::paint_header(painter, &layout, theme);
        Self::paint_section_header(painter, layout.running_header, "已开启", theme);
        if let Some(empty) = layout.running_empty {
            paint_single_line(
                painter,
                "没有已开启的子代理",
                Rect::xywh(
                    empty.origin.x + ROW_PAD_X,
                    empty.origin.y,
                    (empty.size.x - ROW_PAD_X * 2.0).max(0.0),
                    empty.size.y,
                ),
                13.0,
                400,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
        }
        for row in &layout.running_rows {
            Self::paint_row(painter, row, theme);
        }
        Self::paint_section_header(
            painter,
            layout.completed_header,
            &format!("完成 · {}", layout.completed_total),
            theme,
        );
        for row in &layout.completed_rows {
            Self::paint_row(painter, row, theme);
        }
        if let Some(show_more) = layout.show_more {
            paint_single_line(
                painter,
                &layout.show_more_label,
                Rect::xywh(
                    show_more.origin.x + ROW_PAD_X,
                    show_more.origin.y,
                    (show_more.size.x - ROW_PAD_X * 2.0).max(0.0),
                    show_more.size.y,
                ),
                13.0,
                500,
                theme.tokens.accent_foreground,
                HorizontalAlign::Start,
            );
        }
    }

    fn paint_header(painter: &mut dyn Painter, layout: &SubagentsPanelLayout, theme: &ZodeTheme) {
        paint_single_line(
            painter,
            "子智能体",
            Rect::xywh(
                layout.header.origin.x + ROW_PAD_X,
                layout.header.origin.y,
                (layout.close_button.origin.x - layout.header.origin.x - 24.0).max(0.0),
                layout.header.size.y,
            ),
            13.0,
            600,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        painter.stroke_svg_path(
            SemanticIcon::Close.path(),
            jian_widgets::Point2D::new(
                layout.close_button.origin.x + 8.0,
                layout.close_button.origin.y + 8.0,
            ),
            16.0,
            theme.tokens.muted_foreground,
            SemanticIcon::Close.stroke_width(),
        );
        painter.stroke_line(
            jian_widgets::Point2D::new(layout.header.origin.x, layout.header.max_y()),
            jian_widgets::Point2D::new(layout.header.max_x(), layout.header.max_y()),
            theme.tokens.border,
            1.0,
        );
    }

    fn paint_section_header(painter: &mut dyn Painter, rect: Rect, title: &str, theme: &ZodeTheme) {
        if rect.size.y <= 0.0 {
            return;
        }
        paint_single_line(
            painter,
            title,
            Rect::xywh(
                rect.origin.x + ROW_PAD_X,
                rect.origin.y,
                (rect.size.x - ROW_PAD_X * 2.0).max(0.0),
                rect.size.y,
            ),
            13.0,
            500,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    }

    fn paint_row(painter: &mut dyn Painter, row: &SubagentRowLayout, theme: &ZodeTheme) {
        let content_x = row.rect.origin.x + ROW_PAD_X;
        let content_width = (row.rect.size.x - ROW_PAD_X * 2.0).max(0.0);
        let avatar_rect = Rect::xywh(content_x, row.rect.origin.y + 6.0, AVATAR_SIZE, AVATAR_SIZE);
        painter.fill_round_rect(
            avatar_rect,
            AVATAR_SIZE / 2.0,
            subagent_avatar_color(&row.subagent.id),
        );
        let text_x = avatar_rect.max_x() + AVATAR_GAP;
        let text_width = (content_x + content_width - text_x).max(0.0);

        let trailing = row_trailing_label(&row.subagent);
        let time_width = TIME_COLUMN_WIDTH.min(text_width * 0.4);
        let title_width = (text_width - time_width - 8.0).max(0.0);
        let title = ellipsize_end(painter, &row.subagent.display_name, title_width, 14.0, 500);
        paint_single_line(
            painter,
            &title,
            Rect::xywh(text_x, row.rect.origin.y + 3.0, title_width, 18.0),
            14.0,
            500,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        paint_single_line(
            painter,
            &trailing,
            Rect::xywh(
                text_x + title_width + 8.0,
                row.rect.origin.y + 3.0,
                time_width,
                18.0,
            ),
            12.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::End,
        );
        if let Some(summary) = row.subagent.result_summary.as_deref() {
            if !summary.is_empty() {
                let visible = ellipsize_end(painter, summary, text_width, 12.0, 400);
                paint_single_line(
                    painter,
                    &visible,
                    Rect::xywh(text_x, row.rect.origin.y + 25.0, text_width, 16.0),
                    12.0,
                    400,
                    theme.tokens.muted_foreground,
                    HorizontalAlign::Start,
                );
            }
        }
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if id == SUBAGENTS_PANEL_CLOSE_ID
            && state.presentation.secondary_pane == Some(SecondaryPane::Subagents)
        {
            return Some(AppCommand::CloseSecondary);
        }
        if id == SUBAGENTS_PANEL_SHOW_MORE_ID
            && state.presentation.secondary_pane == Some(SecondaryPane::Subagents)
        {
            let session = state.current_session.clone()?;
            let (_, completed, visible_count) = subagent_lists(state);
            if completed.len() > visible_count {
                return Some(AppCommand::ShowMoreCompletedSubagents { session });
            }
        }
        None
    }

    /// Wall-clock "now", exposed so `accessibility.rs`'s node building can
    /// derive the same per-row relative-time label this widget paints,
    /// without reaching past this module into `transcript::relative_time`
    /// directly (that submodule is `pub(crate)` for the paint path's own
    /// use, not part of the widget's public surface).
    pub fn now_ms() -> i64 {
        relative_time::now_ms()
    }

    /// The per-row accessibility label ("<任务名>：<状态> <相对时长>", the
    /// timestamp omitted for a still-running agent) - shared between
    /// `accessibility.rs`'s node building and this module's own tests so the
    /// two can never drift.
    pub fn row_label(subagent: &SubagentSnapshot, now_ms: i64) -> String {
        let status = subagent_status_label(subagent.status);
        match subagent.completed_at_ms {
            Some(completed_at_ms) => format!(
                "{}：{status} {}",
                subagent.display_name,
                relative_time::relative_duration_label(completed_at_ms, now_ms)
            ),
            None => format!("{}：{status}", subagent.display_name),
        }
    }
}

/// Right-aligned trailing text painted on a row's first line: the relative
/// completion age for a finished agent, or a bare status word while still
/// running (there is no completion timestamp to derive an age from yet).
fn row_trailing_label(subagent: &SubagentSnapshot) -> String {
    match subagent.completed_at_ms {
        Some(completed_at_ms) => {
            relative_time::relative_duration_label(completed_at_ms, relative_time::now_ms())
        }
        None => subagent_status_label(subagent.status).to_owned(),
    }
}

/// Splits the current session's live sub-agents into (running, completed),
/// with completed sorted by `completed_at_ms` descending (most recent
/// first, per `docs/proposals/subagent-panel-m2.md`), plus how many
/// completed rows the panel currently shows. Shared by `layout` and
/// `command_for_widget` so the "show more" button's existence check can
/// never disagree with what was actually laid out.
fn subagent_lists(state: &ZodeAppState) -> (Vec<SubagentSnapshot>, Vec<SubagentSnapshot>, usize) {
    let Some(presentation) = state.current_session_presentation() else {
        return (Vec::new(), Vec::new(), SUBAGENTS_PAGE_SIZE);
    };
    let running = presentation
        .subagents
        .iter()
        .filter(|subagent| subagent.status == SubagentStatus::Running)
        .cloned()
        .collect();
    let mut completed = presentation
        .subagents
        .iter()
        .filter(|subagent| subagent.status != SubagentStatus::Running)
        .cloned()
        .collect::<Vec<_>>();
    completed
        .sort_by_key(|subagent| std::cmp::Reverse(subagent.completed_at_ms.unwrap_or(i64::MIN)));
    (running, completed, subagents_visible_count(presentation))
}

fn layout_rows(
    subagents: Vec<SubagentSnapshot>,
    rect: Rect,
    start_y: f32,
) -> Vec<SubagentRowLayout> {
    subagents
        .into_iter()
        .enumerate()
        .map(|(index, subagent)| SubagentRowLayout {
            subagent,
            rect: Rect::xywh(
                rect.origin.x,
                start_y + index as f32 * ROW_HEIGHT,
                rect.size.x,
                ROW_HEIGHT,
            ),
        })
        .collect()
}

/// End-anchored ellipsis truncation (unlike `environment/row.rs`'s
/// `middle_ellipsize`, which favors keeping both a path's ends visible) -
/// a task name or result summary reads better with its beginning intact.
fn ellipsize_end(
    painter: &mut dyn Painter,
    value: &str,
    max_width: f32,
    font_size: f32,
    weight: u16,
) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if painter.measure_text_weighted(value, font_size, weight) <= max_width {
        return value.to_owned();
    }
    const ELLIPSIS: &str = "…";
    if painter.measure_text_weighted(ELLIPSIS, font_size, weight) > max_width {
        return String::new();
    }
    let characters = value.chars().collect::<Vec<_>>();
    for kept in (0..characters.len()).rev() {
        let mut candidate = characters[..kept].iter().collect::<String>();
        candidate.push('…');
        if painter.measure_text_weighted(&candidate, font_size, weight) <= max_width {
            return candidate;
        }
    }
    ELLIPSIS.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_app_model::{demo_state, SessionPresentationState};
    use zode_node_protocol::{SessionLocator, TurnId};

    fn subagent(
        id: &str,
        status: SubagentStatus,
        completed_at_ms: Option<i64>,
    ) -> SubagentSnapshot {
        SubagentSnapshot {
            id: id.into(),
            agent_type: "general-purpose".into(),
            display_name: format!("task {id}"),
            depth: 0,
            status,
            tokens: 10,
            turn_id: TurnId::new(),
            completed_at_ms,
            result_summary: Some("done".into()),
        }
    }

    fn state_with_subagents(subagents: Vec<SubagentSnapshot>) -> ZodeAppState {
        let mut state = demo_state();
        let session = SessionLocator::new(state.host.node_id, "demo-session");
        state.current_session = Some(session.clone());
        state.presentation.sessions.insert(
            session,
            SessionPresentationState {
                subagents,
                ..Default::default()
            },
        );
        state
    }

    #[test]
    fn empty_running_section_reserves_the_empty_state_row() {
        let state = state_with_subagents(vec![subagent("a", SubagentStatus::Completed, Some(1))]);
        let layout = SubagentsPanel::layout(Rect::xywh(0.0, 0.0, 320.0, 800.0), &state);
        assert!(layout.running_rows.is_empty());
        assert!(layout.running_empty.is_some());
        assert_eq!(layout.completed_total, 1);
        assert_eq!(layout.completed_rows.len(), 1);
    }

    #[test]
    fn completed_rows_are_sorted_most_recent_first_and_paginated() {
        let subagents = (0..15)
            .map(|index| subagent(&index.to_string(), SubagentStatus::Completed, Some(index)))
            .collect();
        let state = state_with_subagents(subagents);
        let layout = SubagentsPanel::layout(Rect::xywh(0.0, 0.0, 320.0, 2000.0), &state);
        assert_eq!(layout.completed_total, 15);
        assert_eq!(layout.completed_rows.len(), SUBAGENTS_PAGE_SIZE);
        assert_eq!(layout.completed_rows[0].subagent.id, "14");
        assert_eq!(layout.show_more_label, "再显示 5 个");
    }

    #[test]
    fn show_more_command_only_fires_while_the_panel_is_open_and_more_remain() {
        let subagents = (0..15)
            .map(|index| subagent(&index.to_string(), SubagentStatus::Completed, Some(index)))
            .collect();
        let mut state = state_with_subagents(subagents);
        assert_eq!(
            SubagentsPanel::command_for_widget(&state, SUBAGENTS_PANEL_SHOW_MORE_ID),
            None
        );
        state.presentation.secondary_pane = Some(SecondaryPane::Subagents);
        let session = state.current_session.clone().unwrap();
        assert_eq!(
            SubagentsPanel::command_for_widget(&state, SUBAGENTS_PANEL_SHOW_MORE_ID),
            Some(AppCommand::ShowMoreCompletedSubagents { session })
        );
    }

    #[test]
    fn close_command_only_fires_while_the_subagents_pane_is_active() {
        let mut state = demo_state();
        assert_eq!(
            SubagentsPanel::command_for_widget(&state, SUBAGENTS_PANEL_CLOSE_ID),
            None
        );
        state.presentation.secondary_pane = Some(SecondaryPane::Subagents);
        assert_eq!(
            SubagentsPanel::command_for_widget(&state, SUBAGENTS_PANEL_CLOSE_ID),
            Some(AppCommand::CloseSecondary)
        );
    }

    #[test]
    fn running_row_label_omits_the_timestamp() {
        let running = subagent("a", SubagentStatus::Running, None);
        assert_eq!(SubagentsPanel::row_label(&running, 1_000), "task a：运行中");
    }

    #[test]
    fn completed_row_label_includes_the_relative_time() {
        let completed = subagent("a", SubagentStatus::Completed, Some(0));
        assert_eq!(
            SubagentsPanel::row_label(&completed, 5 * 60_000),
            "task a：已完成 5 分钟"
        );
    }
}
