use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{AppCommand, ConnectionState, EnvironmentEntry, LoadState, ZodeAppState};

use crate::{paint_single_line, RectExt, WidgetId, ZodeTheme};

pub const ENVIRONMENT_CLOSE_ID: WidgetId = WidgetId(100);
pub const ENVIRONMENT_REVIEW_ID: WidgetId = WidgetId(101);

const PANEL_WIDTH: f32 = 300.0;
const PANEL_HEIGHT: f32 = 512.0;
const PANEL_RADIUS: f32 = 16.0;
const PANEL_INSET: f32 = 16.0;
const HEADER_HEIGHT: f32 = 46.0;
const CLOSE_SIZE: f32 = 20.0;
const REVIEW_HEIGHT: f32 = 34.0;
const REVIEW_BOTTOM: f32 = 16.0;
const MIN_REVIEW_PANEL_HEIGHT: f32 = 112.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvironmentPanelLayout {
    pub card: Rect,
    pub header: Rect,
    pub close_button: Rect,
    pub review_button: Option<Rect>,
}

pub struct EnvironmentPanel;

impl EnvironmentPanel {
    pub fn layout(rect: Rect, state: &ZodeAppState) -> EnvironmentPanelLayout {
        let available_width = finite_non_negative(rect.size.x);
        let available_height = finite_non_negative(rect.size.y);
        let card_width = PANEL_WIDTH.min(available_width);
        let card_height = PANEL_HEIGHT.min(available_height);
        let card = Rect::xywh(
            rect.origin.x + (available_width - card_width).max(0.0),
            rect.origin.y,
            card_width,
            card_height,
        );
        let header = Rect::xywh(
            card.origin.x,
            card.origin.y,
            card.size.x,
            HEADER_HEIGHT.min(card.size.y),
        );
        let close_size = CLOSE_SIZE.min(card.size.x).min(card.size.y);
        let close_button = Rect::xywh(
            (card.max_x() - PANEL_INSET + 4.0 - close_size).max(card.origin.x),
            (card.origin.y + 12.0).min((card.max_y() - close_size).max(card.origin.y)),
            close_size,
            close_size,
        );
        let review_button = review_available(state)
            .then(|| review_button(card))
            .flatten();

        EnvironmentPanelLayout {
            card,
            header,
            close_button,
            review_button,
        }
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        let layout = Self::layout(rect, state);
        if layout.card.size.x <= 0.0 || layout.card.size.y <= 0.0 {
            return;
        }

        painter.fill_round_rect(layout.card, PANEL_RADIUS, theme.tokens.card);
        painter.stroke_round_rect(layout.card, PANEL_RADIUS, theme.tokens.border, 1.0);
        painter.save();
        painter.clip_rect(layout.card);
        paint_header(painter, &layout, theme);

        let body_bottom = layout
            .review_button
            .map(|button| button.origin.y - 8.0)
            .unwrap_or(layout.card.max_y() - PANEL_INSET);
        let body = Rect::xywh(
            layout.card.origin.x + PANEL_INSET,
            layout.header.max_y(),
            (layout.card.size.x - PANEL_INSET * 2.0).max(0.0),
            (body_bottom - layout.header.max_y()).max(0.0),
        );
        painter.save();
        painter.clip_rect(body);
        paint_body(painter, body, state, theme);
        painter.restore();

        if let Some(button) = layout.review_button {
            painter.fill_round_rect(button, 9.0, theme.tokens.muted);
            painter.stroke_round_rect(button, 9.0, theme.tokens.border, 1.0);
            paint_single_line(
                painter,
                "查看变更",
                Rect::xywh(
                    button.origin.x + 12.0,
                    button.origin.y,
                    (button.size.x - 24.0).max(0.0),
                    button.size.y,
                ),
                13.0,
                600,
                theme.tokens.foreground,
                HorizontalAlign::Start,
            );
        }
        painter.restore();
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        match id {
            ENVIRONMENT_CLOSE_ID => Some(AppCommand::CloseSecondary),
            ENVIRONMENT_REVIEW_ID if review_available(state) => Some(AppCommand::OpenReview),
            _ => None,
        }
    }
}

fn review_button(card: Rect) -> Option<Rect> {
    if card.size.x <= PANEL_INSET * 2.0 || card.size.y < MIN_REVIEW_PANEL_HEIGHT {
        return None;
    }
    Some(Rect::xywh(
        card.origin.x + PANEL_INSET,
        card.max_y() - REVIEW_BOTTOM - REVIEW_HEIGHT,
        card.size.x - PANEL_INSET * 2.0,
        REVIEW_HEIGHT,
    ))
}

fn review_available(state: &ZodeAppState) -> bool {
    state
        .current_session_presentation()
        .is_some_and(|presentation| matches!(presentation.diff.load, LoadState::Ready(_)))
}

fn paint_header(painter: &mut dyn Painter, layout: &EnvironmentPanelLayout, theme: &ZodeTheme) {
    draw_text(
        painter,
        "环境信息",
        Point2D::new(
            layout.header.origin.x + PANEL_INSET,
            layout.header.origin.y + 29.0,
        ),
        14.0,
        600,
        theme.tokens.foreground,
    );
    draw_text(
        painter,
        "×",
        Point2D::new(
            layout.close_button.origin.x + 4.0,
            layout.close_button.origin.y + 15.0,
        ),
        15.0,
        450,
        theme.tokens.muted_foreground,
    );
    painter.stroke_line(
        Point2D::new(layout.card.origin.x, layout.header.max_y()),
        Point2D::new(layout.card.max_x(), layout.header.max_y()),
        theme.tokens.border,
        1.0,
    );
}

fn paint_body(painter: &mut dyn Painter, body: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
    let mut y = body.origin.y + 22.0;
    let Some(session) = state.current_session.as_ref() else {
        paint_value_row(
            painter,
            body,
            y,
            "主机连接",
            connection_label(state.host.connection),
            theme,
        );
        draw_text(
            painter,
            "选择任务以查看环境",
            Point2D::new(body.origin.x, y + 18.0),
            13.0,
            400,
            theme.tokens.muted_foreground,
        );
        return;
    };

    let presentation = state.current_session_presentation();
    let diff_state = presentation.map(|presentation| &presentation.diff.load);
    match diff_state {
        None | Some(LoadState::Idle) => {
            paint_section_value(painter, body, &mut y, "变更", "变更尚未加载", theme);
        }
        Some(LoadState::Loading) => {
            paint_section_value(painter, body, &mut y, "变更", "变更加载中", theme);
        }
        Some(LoadState::Failed(error)) => {
            let message = format!("变更加载失败：{error}");
            paint_section_value(painter, body, &mut y, "变更", &message, theme);
        }
        Some(LoadState::Ready(diff)) => {
            let (additions, deletions) = diff.files.iter().fold((0_u64, 0_u64), |totals, file| {
                (
                    totals.0 + u64::from(file.additions),
                    totals.1 + u64::from(file.deletions),
                )
            });
            let files = format!("{} 个文件", diff.files.len());
            let counts = format!("+{additions} -{deletions}");
            paint_section_value(painter, body, &mut y, "变更", &files, theme);
            draw_text(
                painter,
                &counts,
                Point2D::new(body.origin.x, y),
                12.0,
                550,
                theme.success,
            );
            y += 20.0;
        }
    }

    y += 8.0;
    paint_value_row(
        painter,
        body,
        y,
        "主机连接",
        connection_label(state.host.connection),
        theme,
    );
    y += 28.0;
    let workspace = presentation
        .and_then(|presentation| presentation.context.ready())
        .map(|context| context.workspace_uri.as_str())
        .or_else(|| {
            state
                .threads
                .iter()
                .find(|thread| &thread.session == session)
                .map(|thread| thread.workspace_uri.as_str())
        })
        .unwrap_or("未知");
    paint_stacked_value(painter, body, &mut y, "当前工作区", workspace, theme);

    match presentation.map(|presentation| &presentation.context) {
        None | Some(LoadState::Idle) => {
            paint_value_row(painter, body, y, "上下文", "尚未加载", theme);
        }
        Some(LoadState::Loading) => {
            paint_value_row(painter, body, y, "上下文", "加载中", theme);
        }
        Some(LoadState::Failed(error)) => {
            let message = format!("加载失败：{error}");
            paint_stacked_value(painter, body, &mut y, "上下文", &message, theme);
        }
        Some(LoadState::Ready(context)) => {
            paint_value_row(painter, body, y, "上下文", "已就绪", theme);
            y += 28.0;
            if let Some(branch) = context.branch.as_deref() {
                paint_stacked_value(painter, body, &mut y, "分支", branch, theme);
            }
            paint_entries(painter, body, &mut y, "子智能体", &context.subagents, theme);
            paint_entries(
                painter,
                body,
                &mut y,
                "后台进程",
                &context.background_processes,
                theme,
            );
            paint_entries(painter, body, &mut y, "来源", &context.sources, theme);
        }
    }
}

fn paint_entries(
    painter: &mut dyn Painter,
    body: Rect,
    y: &mut f32,
    heading: &str,
    entries: &[EnvironmentEntry],
    theme: &ZodeTheme,
) {
    if entries.is_empty() {
        return;
    }
    *y += 4.0;
    draw_text(
        painter,
        heading,
        Point2D::new(body.origin.x, *y + 14.0),
        12.0,
        600,
        theme.tokens.muted_foreground,
    );
    *y += 22.0;
    for entry in entries {
        draw_text(
            painter,
            &entry.label,
            Point2D::new(body.origin.x, *y + 14.0),
            12.0,
            450,
            theme.tokens.foreground,
        );
        *y += 20.0;
    }
}

fn paint_section_value(
    painter: &mut dyn Painter,
    body: Rect,
    y: &mut f32,
    heading: &str,
    value: &str,
    theme: &ZodeTheme,
) {
    *y += 4.0;
    paint_stacked_value(painter, body, y, heading, value, theme);
}

fn paint_stacked_value(
    painter: &mut dyn Painter,
    body: Rect,
    y: &mut f32,
    label: &str,
    value: &str,
    theme: &ZodeTheme,
) {
    draw_text(
        painter,
        label,
        Point2D::new(body.origin.x, *y + 14.0),
        12.0,
        500,
        theme.tokens.muted_foreground,
    );
    *y += 20.0;
    draw_text(
        painter,
        value,
        Point2D::new(body.origin.x, *y + 14.0),
        12.0,
        450,
        theme.tokens.foreground,
    );
    *y += 34.0;
}

fn paint_value_row(
    painter: &mut dyn Painter,
    body: Rect,
    y: f32,
    label: &str,
    value: &str,
    theme: &ZodeTheme,
) {
    draw_text(
        painter,
        label,
        Point2D::new(body.origin.x, y),
        12.0,
        500,
        theme.tokens.muted_foreground,
    );
    draw_text(
        painter,
        value,
        Point2D::new(body.origin.x + 104.0, y),
        12.0,
        500,
        theme.tokens.foreground,
    );
}

const fn connection_label(connection: ConnectionState) -> &'static str {
    match connection {
        ConnectionState::Local => "本地",
        ConnectionState::Connecting => "连接中",
        ConnectionState::Unavailable => "不可用",
    }
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    size: f32,
    weight: u16,
    color: jian_widgets::Color,
) {
    let layout = TextLayout::single_run(text, "system-ui", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(&layout, origin);
}

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}
