use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::{AppCommand, SecondaryPane, ZodeAppState};

use crate::{
    paint_single_line, RectExt, SemanticIcon, UsageChip, WidgetId, ZodeTheme,
    HEADER_ENVIRONMENT_ID, HEADER_REVIEW_ID,
};

const ACTION_SIZE: f32 = 32.0;
const ACTION_GAP: f32 = 4.0;
const ACTION_RIGHT: f32 = 12.0;
const ENVIRONMENT_MIN_HEADER_WIDTH: f32 = 1_160.0;
const TITLE_FONT_SIZE: f32 = 13.0;
const MENU_WIDTH: f32 = 196.0;
const MENU_PADDING: f32 = 4.0;
const MENU_ROW_HEIGHT: f32 = 36.0;

pub(crate) const HEADER_MORE_ID: WidgetId = WidgetId(62);
pub(crate) const HEADER_MENU_ID: WidgetId = WidgetId(63);
pub(crate) const HEADER_MENU_PIN_ID: WidgetId = WidgetId(64);
pub(crate) const HEADER_MENU_ARCHIVE_ID: WidgetId = WidgetId(65);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderActionLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadHeaderLayout {
    pub title: Rect,
    pub more: Option<HeaderActionLayout>,
    pub environment: Option<HeaderActionLayout>,
    pub review: Option<HeaderActionLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadMenuActionLayout {
    pub id: WidgetId,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadMenuLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub pin: ThreadMenuActionLayout,
    pub archive: ThreadMenuActionLayout,
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

        let title_left = rect.origin.x + 20.0;
        let title = current_title(state);
        let more = title.map(|title| {
            let available = (title_right - title_left - ACTION_SIZE - 8.0).max(0.0);
            let title_width = estimated_title_width(title).min(available);
            HeaderActionLayout {
                id: HEADER_MORE_ID,
                rect: Rect::xywh(
                    title_left + title_width + 4.0,
                    rect.origin.y + (rect.size.y - ACTION_SIZE).max(0.0) / 2.0,
                    ACTION_SIZE.min(rect.size.y.max(0.0)),
                    ACTION_SIZE.min(rect.size.y.max(0.0)),
                ),
                selected: state
                    .current_session
                    .as_ref()
                    .is_some_and(|session| state.session_menu.as_ref() == Some(session)),
            }
        });
        let title_width = more
            .map(|action| action.rect.origin.x - title_left - 4.0)
            .unwrap_or(title_right - title_left)
            .max(0.0);

        ThreadHeaderLayout {
            title: Rect::xywh(title_left, rect.origin.y, title_width, rect.size.y.max(0.0)),
            more,
            environment,
            review,
        }
    }

    pub fn menu_layout(rect: Rect, state: &ZodeAppState) -> Option<ThreadMenuLayout> {
        let session = state.current_session.as_ref()?;
        if state.session_menu.as_ref() != Some(session) {
            return None;
        }
        let more = Self::layout(rect, state).more?;
        let width = MENU_WIDTH.min(rect.size.x.max(0.0));
        let height = MENU_PADDING * 2.0 + MENU_ROW_HEIGHT * 2.0;
        let min_x = rect.origin.x + 8.0;
        let max_x = (rect.max_x() - width - 8.0).max(min_x);
        let menu_rect = Rect::xywh(
            (more.rect.origin.x - MENU_PADDING).clamp(min_x, max_x),
            rect.max_y() + 6.0,
            width,
            height,
        );
        Some(ThreadMenuLayout {
            id: HEADER_MENU_ID,
            rect: menu_rect,
            pin: ThreadMenuActionLayout {
                id: HEADER_MENU_PIN_ID,
                rect: Rect::xywh(
                    menu_rect.origin.x + MENU_PADDING,
                    menu_rect.origin.y + MENU_PADDING,
                    (menu_rect.size.x - MENU_PADDING * 2.0).max(0.0),
                    MENU_ROW_HEIGHT,
                ),
            },
            archive: ThreadMenuActionLayout {
                id: HEADER_MENU_ARCHIVE_ID,
                rect: Rect::xywh(
                    menu_rect.origin.x + MENU_PADDING,
                    menu_rect.origin.y + MENU_PADDING + MENU_ROW_HEIGHT,
                    (menu_rect.size.x - MENU_PADDING * 2.0).max(0.0),
                    MENU_ROW_HEIGHT,
                ),
            },
        })
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        let session = state.current_session.as_ref()?;
        match id {
            HEADER_MORE_ID => Some(AppCommand::ToggleSessionMenu {
                session: session.clone(),
            }),
            HEADER_MENU_PIN_ID if state.session_menu.as_ref() == Some(session) => {
                Some(AppCommand::SetSessionPinned {
                    session: session.clone(),
                    pinned: !state.pinned_sessions.contains(session),
                })
            }
            HEADER_MENU_ARCHIVE_ID if state.session_menu.as_ref() == Some(session) => {
                Some(AppCommand::SetSessionArchived {
                    session: session.clone(),
                    archived: true,
                })
            }
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
            header.more = None;
            header.environment = None;
            header.review = None;
        }
        let title = current_title(state);
        if let Some(title) = title {
            paint_single_line(
                painter,
                title,
                header.title,
                TITLE_FONT_SIZE,
                600,
                theme.tokens.foreground,
                HorizontalAlign::Start,
            );
        }
        for (action, icon) in [
            (header.more, SemanticIcon::More),
            (header.environment, SemanticIcon::Environment),
            (header.review, SemanticIcon::Diff),
        ] {
            let Some(action) = action else {
                continue;
            };
            if action.selected {
                painter.fill_round_rect(action.rect, 9.0, theme.tokens.row_selected);
            }
            let icon_rect = Rect::xywh(
                action.rect.origin.x + (action.rect.size.x - 16.0).max(0.0) / 2.0,
                action.rect.origin.y + (action.rect.size.y - 16.0).max(0.0) / 2.0,
                16.0_f32.min(action.rect.size.x),
                16.0_f32.min(action.rect.size.y),
            );
            painter.stroke_svg_path(
                icon.path(),
                icon_rect.origin,
                icon_rect.size.x.min(icon_rect.size.y),
                theme.tokens.muted_foreground,
                icon.stroke_width(),
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

    pub fn paint_overlays(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &ZodeAppState,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        let Some(menu) = Self::menu_layout(rect, state) else {
            return;
        };
        painter.fill_drop_shadow(
            Rect::xywh(
                menu.rect.origin.x,
                menu.rect.origin.y + 2.0,
                menu.rect.size.x,
                menu.rect.size.y,
            ),
            10.0,
            16.0,
            theme.tokens.foreground.with_alpha(0.12),
        );
        painter.fill_round_rect(menu.rect, 10.0, theme.tokens.popover);
        painter.stroke_round_rect(menu.rect, 10.0, theme.tokens.border, 1.0);

        let pinned = state
            .current_session
            .as_ref()
            .is_some_and(|session| state.pinned_sessions.contains(session));
        for (action, icon, label) in [
            (
                menu.pin,
                SemanticIcon::Pin,
                if pinned {
                    "取消置顶"
                } else {
                    "置顶任务"
                },
            ),
            (menu.archive, SemanticIcon::Archive, "归档任务"),
        ] {
            if hovered == Some(action.id) {
                painter.fill_round_rect(action.rect, 7.0, theme.tokens.accent);
            }
            if focused == Some(action.id) {
                painter.stroke_round_rect(action.rect, 7.0, theme.tokens.ring, 1.5);
            }
            let icon_rect = Rect::xywh(
                action.rect.origin.x + 10.0,
                action.rect.origin.y + (action.rect.size.y - 16.0) / 2.0,
                16.0,
                16.0,
            );
            painter.stroke_svg_path(
                icon.path(),
                icon_rect.origin,
                icon_rect.size.x,
                theme.tokens.popover_foreground,
                icon.stroke_width(),
            );
            paint_single_line(
                painter,
                label,
                Rect::xywh(
                    icon_rect.max_x() + 9.0,
                    action.rect.origin.y,
                    (action.rect.max_x() - icon_rect.max_x() - 15.0).max(0.0),
                    action.rect.size.y,
                ),
                13.0,
                400,
                theme.tokens.popover_foreground,
                HorizontalAlign::Start,
            );
        }
    }
}

fn current_title(state: &ZodeAppState) -> Option<&str> {
    let session = state.current_session.as_ref()?;
    state
        .threads
        .iter()
        .find(|thread| &thread.session == session)
        .map(|thread| thread.title.as_str())
}

fn estimated_title_width(title: &str) -> f32 {
    title
        .chars()
        .map(|character| {
            if character.is_ascii() {
                TITLE_FONT_SIZE * 0.56
            } else {
                TITLE_FONT_SIZE
            }
        })
        .sum()
}
