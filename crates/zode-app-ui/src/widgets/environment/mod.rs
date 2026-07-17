mod row;
mod section;

use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{
    environment_sections, AppCommand, EnvironmentSectionKind, LoadState, ZodeAppState,
};
use zode_node_protocol::SessionLocator;

use crate::{paint_single_line, stable_widget_id, RectExt, SemanticIcon, WidgetId, ZodeTheme};

pub use section::EnvironmentSectionLayout;

pub const ENVIRONMENT_CLOSE_ID: WidgetId = WidgetId(100);
pub const ENVIRONMENT_REVIEW_ID: WidgetId = WidgetId(101);

const PANEL_WIDTH: f32 = 300.0;
const PANEL_MIN_HEIGHT: f32 = 320.0;
const PANEL_MAX_HEIGHT: f32 = 512.0;
const PANEL_RADIUS: f32 = 16.0;
const PANEL_INSET: f32 = 16.0;
const HEADER_HEIGHT: f32 = 46.0;
const CLOSE_SIZE: f32 = 20.0;
const BODY_TOP: f32 = 10.0;
const REVIEW_HEIGHT: f32 = 34.0;
const REVIEW_GAP: f32 = 8.0;
const REVIEW_BOTTOM: f32 = 16.0;
const STATUS_HEIGHT: f32 = 24.0;
const MIN_REVIEW_PANEL_HEIGHT: f32 = 112.0;

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
    pub review_button: Option<Rect>,
    pub last_row: Option<Rect>,
}

pub struct EnvironmentPanel;

impl EnvironmentPanel {
    pub fn layout(rect: Rect, state: &ZodeAppState) -> EnvironmentPanelLayout {
        let available_width = finite_non_negative(rect.size.x);
        let available_height = finite_non_negative(rect.size.y);
        let projected = environment_sections(state);
        let status_messages = status_messages(state);
        let body_sections = projected
            .iter()
            .filter(|section| !section::is_repository_action(section.kind))
            .collect::<Vec<_>>();
        let sections_height = body_sections
            .iter()
            .map(|section| section::height(section))
            .sum::<f32>()
            + section::SECTION_GAP * body_sections.len().saturating_sub(1) as f32;
        let has_review = projected
            .iter()
            .any(|section| section::is_repository_action(section.kind));
        let status_height = status_messages.len() as f32 * STATUS_HEIGHT;
        let desired_height = HEADER_HEIGHT
            + BODY_TOP
            + sections_height
            + status_height
            + if has_review {
                REVIEW_GAP + REVIEW_HEIGHT + REVIEW_BOTTOM
            } else {
                REVIEW_BOTTOM
            };
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
        let review_button = (has_review && card.size.y >= MIN_REVIEW_PANEL_HEIGHT).then(|| {
            Rect::xywh(
                card.origin.x + PANEL_INSET,
                card.max_y() - REVIEW_BOTTOM - REVIEW_HEIGHT,
                (card.size.x - PANEL_INSET * 2.0).max(0.0),
                REVIEW_HEIGHT,
            )
        });
        let content_rect = Rect::xywh(
            card.origin.x + PANEL_INSET,
            header.max_y() + BODY_TOP,
            (card.size.x - PANEL_INSET * 2.0).max(0.0),
            (review_button
                .map(|button| button.origin.y - REVIEW_GAP)
                .unwrap_or(card.max_y() - REVIEW_BOTTOM)
                - header.max_y()
                - BODY_TOP)
                .max(0.0),
        );

        let repository_index = projected
            .iter()
            .position(|section| section::is_repository_action(section.kind));
        let repository = repository_index.map(|index| projected[index].clone());
        let occupied_content_height = sections_height + status_height;
        let mut y = content_rect.origin.y
            + if review_button.is_none() {
                (content_rect.size.y - occupied_content_height).max(0.0)
            } else {
                0.0
            };
        let mut sections = Vec::new();
        for projected_section in projected
            .into_iter()
            .filter(|section| !section::is_repository_action(section.kind))
        {
            let layout = section::layout(projected_section, content_rect, y);
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
        if let (Some(index), Some(repository), Some(button)) =
            (repository_index, repository, review_button)
        {
            sections.insert(
                index.min(sections.len()),
                section::footer(repository, button),
            );
        }
        let last_row = review_button.or_else(|| {
            statuses
                .last()
                .map(|status| status.rect)
                .or_else(|| sections.iter().rev().find_map(|section| section.last_row()))
        });

        EnvironmentPanelLayout {
            card,
            header,
            content: content_rect,
            close_button,
            sections,
            statuses,
            review_button,
            last_row,
        }
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        let layout = Self::layout(rect, state);
        if layout.card.size.x <= 0.0 || layout.card.size.y <= 0.0 {
            return;
        }
        painter.fill_drop_shadow(
            Rect::xywh(
                layout.card.origin.x,
                layout.card.origin.y + 4.0,
                layout.card.size.x,
                layout.card.size.y,
            ),
            PANEL_RADIUS,
            24.0,
            theme.tokens.foreground.with_alpha(0.12),
        );
        painter.fill_round_rect(layout.card, PANEL_RADIUS, theme.tokens.card);
        painter.stroke_round_rect(layout.card, PANEL_RADIUS, theme.tokens.border, 1.0);
        painter.save();
        painter.clip_rect(layout.card);
        paint_header(painter, &layout, theme);
        painter.save();
        painter.clip_rect(layout.content);
        for section in &layout.sections {
            section::paint(painter, section, theme);
        }
        for status in &layout.statuses {
            paint_single_line(
                painter,
                &status.message,
                status.rect,
                11.0,
                400,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
        }
        painter.restore();
        if let Some(button) = layout.review_button {
            painter.fill_round_rect(button, 9.0, theme.tokens.muted);
            painter.stroke_round_rect(button, 9.0, theme.tokens.border, 1.0);
            let label = layout
                .sections
                .iter()
                .find(|section| section.footer)
                .and_then(|section| section.section.entries.first())
                .map_or("查看变更", |entry| entry.label.as_str());
            paint_single_line(
                painter,
                label,
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

    pub fn section_widget_id(session: &SessionLocator, kind: EnvironmentSectionKind) -> WidgetId {
        stable_widget_id(0x62, &(session, kind))
    }

    pub fn section_accessibility_name(layout: &EnvironmentSectionLayout) -> String {
        section::accessibility_name(layout)
    }
}

fn review_available(state: &ZodeAppState) -> bool {
    environment_sections(state)
        .iter()
        .any(|section| section.kind == EnvironmentSectionKind::RepositoryActions)
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

fn paint_header(painter: &mut dyn Painter, layout: &EnvironmentPanelLayout, theme: &ZodeTheme) {
    paint_single_line(
        painter,
        "环境信息",
        Rect::xywh(
            layout.header.origin.x + PANEL_INSET,
            layout.header.origin.y,
            (layout.header.size.x - PANEL_INSET * 2.0 - CLOSE_SIZE).max(0.0),
            layout.header.size.y,
        ),
        14.0,
        600,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    let close_icon_size = 14.0_f32.min(layout.close_button.size.x.min(layout.close_button.size.y));
    painter.stroke_svg_path(
        SemanticIcon::Close.path(),
        jian_widgets::Point2D::new(
            layout.close_button.origin.x + (layout.close_button.size.x - close_icon_size) / 2.0,
            layout.close_button.origin.y + (layout.close_button.size.y - close_icon_size) / 2.0,
        ),
        close_icon_size,
        theme.tokens.muted_foreground,
        SemanticIcon::Close.stroke_width(),
    );
    painter.stroke_line(
        jian_widgets::Point2D::new(layout.card.origin.x, layout.header.max_y()),
        jian_widgets::Point2D::new(layout.card.max_x(), layout.header.max_y()),
        theme.tokens.border,
        1.0,
    );
}

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}
