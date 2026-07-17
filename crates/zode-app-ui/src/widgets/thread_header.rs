use jian_widgets::{centered_text_baseline_y, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{AppCommand, SecondaryPane, ZodeAppState};

use crate::{UsageChip, WidgetId, ZodeTheme, HEADER_ENVIRONMENT_ID, HEADER_REVIEW_ID};

const ACTION_SIZE: f32 = 32.0;
const ACTION_GAP: f32 = 4.0;
const ACTION_RIGHT: f32 = 12.0;
const ENVIRONMENT_MIN_HEADER_WIDTH: f32 = 1_160.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderActionLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadHeaderLayout {
    pub title: Rect,
    pub environment: Option<HeaderActionLayout>,
    pub review: Option<HeaderActionLayout>,
}

pub struct ThreadHeader;

impl ThreadHeader {
    pub fn layout(rect: Rect, state: &ZodeAppState) -> ThreadHeaderLayout {
        let review = state.current_session.as_ref().and_then(|_| {
            if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
                return None;
            }
            let action_size = ACTION_SIZE
                .min(rect.size.x.max(0.0))
                .min(rect.size.y.max(0.0));
            let y = rect.origin.y + (rect.size.y - action_size).max(0.0) / 2.0;
            let review_x =
                (rect.origin.x + rect.size.x - ACTION_RIGHT - action_size).max(rect.origin.x);
            Some(HeaderActionLayout {
                id: HEADER_REVIEW_ID,
                rect: Rect::xywh(review_x, y, action_size, action_size),
                selected: state.presentation.secondary_pane == Some(SecondaryPane::Review),
            })
        });
        let environment = review
            .filter(|_| rect.size.x >= ENVIRONMENT_MIN_HEADER_WIDTH)
            .map(|review| HeaderActionLayout {
                id: HEADER_ENVIRONMENT_ID,
                rect: Rect::xywh(
                    (review.rect.origin.x - ACTION_GAP - review.rect.size.x).max(rect.origin.x),
                    review.rect.origin.y,
                    review.rect.size.x,
                    review.rect.size.y,
                ),
                selected: state.presentation.secondary_pane == Some(SecondaryPane::Environment),
            });
        let title_right = environment
            .or(review)
            .map(|action| action.rect.origin.x - 12.0)
            .unwrap_or(rect.origin.x + rect.size.x - 20.0)
            .max(rect.origin.x + 20.0);

        ThreadHeaderLayout {
            title: Rect::xywh(
                rect.origin.x + 20.0,
                rect.origin.y,
                (title_right - rect.origin.x - 20.0).max(0.0),
                rect.size.y.max(0.0),
            ),
            environment,
            review,
        }
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        state.current_session.as_ref()?;
        match id {
            HEADER_ENVIRONMENT_ID => Some(AppCommand::OpenSecondary(SecondaryPane::Environment)),
            HEADER_REVIEW_ID => Some(AppCommand::OpenReview),
            _ => None,
        }
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        Self::paint_internal(painter, rect, state, true, theme);
    }

    pub fn paint_title_only(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &ZodeAppState,
        theme: &ZodeTheme,
    ) {
        Self::paint_internal(painter, rect, state, false, theme);
    }

    fn paint_internal(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &ZodeAppState,
        show_actions: bool,
        theme: &ZodeTheme,
    ) {
        let mut header = Self::layout(rect, state);
        if !show_actions {
            header.title = Rect::xywh(
                rect.origin.x + 20.0,
                rect.origin.y,
                (rect.size.x - 40.0).max(0.0),
                rect.size.y.max(0.0),
            );
            header.environment = None;
            header.review = None;
        }
        let title = state
            .current_session
            .as_ref()
            .and_then(|session| {
                state
                    .threads
                    .iter()
                    .find(|thread| &thread.session == session)
            })
            .map(|thread| thread.title.as_str())
            .unwrap_or("新任务");
        let layout = TextLayout::single_run(
            title,
            "system-ui",
            13.0,
            theme.tokens.foreground.to_jian(),
            Point2D::ZERO,
        )
        .with_font_weight(600);
        painter.draw_text(
            &layout,
            Point2D::new(
                header.title.origin.x,
                centered_text_baseline_y(header.title, 13.0),
            ),
        );
        for (action, label) in [(header.environment, "环境"), (header.review, "审查")] {
            let Some(action) = action else {
                continue;
            };
            if action.selected {
                painter.fill_round_rect(action.rect, 9.0, theme.tokens.row_selected);
            }
            let action_text = TextLayout::single_run(
                label,
                "system-ui",
                11.0,
                theme.tokens.muted_foreground.to_jian(),
                Point2D::ZERO,
            )
            .with_font_weight(550);
            painter.draw_text(
                &action_text,
                Point2D::new(
                    action.rect.origin.x + 3.0,
                    centered_text_baseline_y(action.rect, 11.0),
                ),
            );
        }
        if let Some(usage) = state
            .current_session
            .as_ref()
            .and_then(|session| state.usage.get(session))
        {
            let right = header
                .environment
                .or(header.review)
                .map(|action| action.rect.origin.x - 12.0)
                .unwrap_or(rect.origin.x + rect.size.x - 20.0);
            let width = 260.0_f32.min((right - rect.origin.x - 180.0).max(0.0));
            UsageChip::paint(
                painter,
                Rect::xywh(
                    (right - width).max(rect.origin.x + 160.0),
                    rect.origin.y + 11.0,
                    width,
                    24.0,
                ),
                state.composer.model.as_deref(),
                usage,
                theme,
            );
        }
        painter.stroke_line(
            Point2D::new(rect.origin.x, rect.origin.y + rect.size.y),
            Point2D::new(rect.origin.x + rect.size.x, rect.origin.y + rect.size.y),
            theme.tokens.border,
            1.0,
        );
    }
}
