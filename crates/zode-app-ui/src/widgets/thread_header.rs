use jian_core::text_input::TextInputState;
use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::{AppCommand, SecondaryPane, ZodeAppState};
use zode_node_protocol::{SessionLocator, ThreadSummary};

use crate::{
    paint_single_line, PanelPicker, RectExt, SemanticIcon, UsageChip, WidgetId, ZodeTheme,
    HEADER_ENVIRONMENT_ID, HEADER_REVIEW_ID,
};

const ACTION_SIZE: f32 = 32.0;
const ACTION_GAP: f32 = 4.0;
const ACTION_RIGHT: f32 = 12.0;
const ENVIRONMENT_MIN_HEADER_WIDTH: f32 = 1_160.0;
const TITLE_FONT_SIZE: f32 = 13.0;
const MENU_WIDTH: f32 = 224.0;
const MENU_PADDING: f32 = 4.0;
const MENU_ROW_HEIGHT: f32 = 36.0;
const MENU_SEPARATOR_HEIGHT: f32 = 1.0;
const COPY_MENU_WIDTH: f32 = 190.0;
const RENAME_WIDTH: f32 = 380.0;
const RENAME_HEIGHT: f32 = 144.0;

pub const HEADER_MORE_ID: WidgetId = WidgetId(62);
pub const HEADER_MENU_ID: WidgetId = WidgetId(63);
pub const HEADER_MENU_PIN_ID: WidgetId = WidgetId(64);
pub const HEADER_MENU_ARCHIVE_ID: WidgetId = WidgetId(65);
pub const HEADER_MENU_RENAME_ID: WidgetId = WidgetId(150);
pub const HEADER_MENU_SIDE_TASK_ID: WidgetId = WidgetId(151);
pub const HEADER_MENU_COPY_ID: WidgetId = WidgetId(152);
pub const HEADER_MENU_CONTINUE_ID: WidgetId = WidgetId(153);
pub const HEADER_MENU_SCHEDULE_ID: WidgetId = WidgetId(154);
pub const HEADER_MENU_NEW_WINDOW_ID: WidgetId = WidgetId(155);
pub const HEADER_COPY_MENU_ID: WidgetId = WidgetId(156);
pub const HEADER_COPY_TITLE_ID: WidgetId = WidgetId(157);
pub const HEADER_COPY_DETAILS_ID: WidgetId = WidgetId(158);
pub const HEADER_COPY_SESSION_ID: WidgetId = WidgetId(159);
pub const HEADER_RENAME_DIALOG_ID: WidgetId = WidgetId(160);
pub const HEADER_RENAME_INPUT_ID: WidgetId = WidgetId(161);
pub const HEADER_RENAME_CANCEL_ID: WidgetId = WidgetId(162);
pub const HEADER_RENAME_SAVE_ID: WidgetId = WidgetId(163);

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
    pub panel_picker: Option<HeaderActionLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadMenuActionLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadCopyMenuLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub title: ThreadMenuActionLayout,
    pub details: ThreadMenuActionLayout,
    pub session_id: ThreadMenuActionLayout,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadMenuLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub pin: ThreadMenuActionLayout,
    pub rename: ThreadMenuActionLayout,
    pub archive: ThreadMenuActionLayout,
    pub side_task: ThreadMenuActionLayout,
    pub copy: ThreadMenuActionLayout,
    pub continue_in: ThreadMenuActionLayout,
    pub schedule: ThreadMenuActionLayout,
    pub new_window: ThreadMenuActionLayout,
    pub separator_one: Rect,
    pub separator_two: Rect,
    pub copy_menu: Option<ThreadCopyMenuLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadRenameLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub input: Rect,
    pub cancel: ThreadMenuActionLayout,
    pub save: ThreadMenuActionLayout,
}

pub struct ThreadHeader;

impl ThreadHeader {
    pub fn layout(rect: Rect, state: &ZodeAppState) -> ThreadHeaderLayout {
        let panel_picker = (rect.size.x > 0.0 && rect.size.y > 0.0).then(|| {
            let action_size = ACTION_SIZE
                .min(rect.size.x.max(0.0))
                .min(rect.size.y.max(0.0));
            let y = rect.origin.y + (rect.size.y - action_size).max(0.0) / 2.0;
            let x = (rect.origin.x + rect.size.x - ACTION_RIGHT - action_size).max(rect.origin.x);
            HeaderActionLayout {
                id: crate::PANEL_PICKER_ID,
                rect: Rect::xywh(x, y, action_size, action_size),
                selected: state.presentation.secondary_menu_open,
            }
        });
        let review = state.current_session.as_ref().and_then(|_| {
            panel_picker.map(|picker| HeaderActionLayout {
                id: HEADER_REVIEW_ID,
                rect: Rect::xywh(
                    (picker.rect.origin.x - ACTION_GAP - picker.rect.size.x).max(rect.origin.x),
                    picker.rect.origin.y,
                    picker.rect.size.x,
                    picker.rect.size.y,
                ),
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
            .or(panel_picker)
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
            panel_picker,
        }
    }

    pub fn menu_layout(rect: Rect, state: &ZodeAppState) -> Option<ThreadMenuLayout> {
        let session = state.current_session.as_ref()?;
        if state.session_menu.as_ref() != Some(session) {
            return None;
        }
        let more = Self::layout(rect, state).more?;
        let width = MENU_WIDTH.min(rect.size.x.max(0.0));
        let height = MENU_PADDING * 2.0 + MENU_ROW_HEIGHT * 8.0 + MENU_SEPARATOR_HEIGHT * 2.0;
        let min_x = rect.origin.x + 8.0;
        let max_x = (rect.max_x() - width - 8.0).max(min_x);
        let menu_rect = Rect::xywh(
            (more.rect.origin.x - MENU_PADDING).clamp(min_x, max_x),
            rect.max_y() + 6.0,
            width,
            height,
        );
        let row_x = menu_rect.origin.x + MENU_PADDING;
        let row_w = (menu_rect.size.x - MENU_PADDING * 2.0).max(0.0);
        let mut y = menu_rect.origin.y + MENU_PADDING;
        let pin = menu_row(row_x, row_w, &mut y, HEADER_MENU_PIN_ID, true);
        let rename = menu_row(row_x, row_w, &mut y, HEADER_MENU_RENAME_ID, true);
        let archive = menu_row(row_x, row_w, &mut y, HEADER_MENU_ARCHIVE_ID, true);
        let separator_one = Rect::xywh(row_x, y, row_w, MENU_SEPARATOR_HEIGHT);
        y += MENU_SEPARATOR_HEIGHT;
        let side_task = menu_row(row_x, row_w, &mut y, HEADER_MENU_SIDE_TASK_ID, false);
        let copy = menu_row(row_x, row_w, &mut y, HEADER_MENU_COPY_ID, true);
        let continue_in = menu_row(row_x, row_w, &mut y, HEADER_MENU_CONTINUE_ID, false);
        let schedule = menu_row(row_x, row_w, &mut y, HEADER_MENU_SCHEDULE_ID, false);
        let separator_two = Rect::xywh(row_x, y, row_w, MENU_SEPARATOR_HEIGHT);
        y += MENU_SEPARATOR_HEIGHT;
        let new_window = menu_row(row_x, row_w, &mut y, HEADER_MENU_NEW_WINDOW_ID, true);
        debug_assert!((y + MENU_PADDING - menu_rect.max_y()).abs() < 0.01);
        let copy_menu = (state.session_copy_menu.as_ref() == Some(session))
            .then(|| copy_menu_layout(rect, menu_rect, copy));
        Some(ThreadMenuLayout {
            id: HEADER_MENU_ID,
            rect: menu_rect,
            pin,
            rename,
            archive,
            side_task,
            copy,
            continue_in,
            schedule,
            new_window,
            separator_one,
            separator_two,
            copy_menu,
        })
    }

    pub fn rename_layout(rect: Rect, state: &ZodeAppState) -> Option<ThreadRenameLayout> {
        let rename = state.session_rename.as_ref()?;
        if state.current_session.as_ref() != Some(&rename.session) {
            return None;
        }
        let width = RENAME_WIDTH.min((rect.size.x - 16.0).max(0.0));
        if width < 220.0 {
            return None;
        }
        let surface = Rect::xywh(
            (rect.origin.x + 20.0).min((rect.max_x() - width - 8.0).max(rect.origin.x + 8.0)),
            rect.max_y() + 8.0,
            width,
            RENAME_HEIGHT,
        );
        let button_w = 72.0;
        let button_y = surface.max_y() - 44.0;
        Some(ThreadRenameLayout {
            id: HEADER_RENAME_DIALOG_ID,
            rect: surface,
            input: Rect::xywh(
                surface.origin.x + 16.0,
                surface.origin.y + 42.0,
                (surface.size.x - 32.0).max(0.0),
                34.0,
            ),
            cancel: ThreadMenuActionLayout {
                id: HEADER_RENAME_CANCEL_ID,
                rect: Rect::xywh(
                    surface.max_x() - 16.0 - button_w * 2.0 - 8.0,
                    button_y,
                    button_w,
                    30.0,
                ),
                enabled: true,
            },
            save: ThreadMenuActionLayout {
                id: HEADER_RENAME_SAVE_ID,
                rect: Rect::xywh(surface.max_x() - 16.0 - button_w, button_y, button_w, 30.0),
                enabled: !rename.draft.trim().is_empty(),
            },
        })
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if let Some(command) = PanelPicker::command_for_widget(state, id) {
            return Some(command);
        }
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
            HEADER_MENU_RENAME_ID if state.session_menu.as_ref() == Some(session) => {
                Some(AppCommand::BeginRenameSession {
                    session: session.clone(),
                })
            }
            HEADER_MENU_ARCHIVE_ID if state.session_menu.as_ref() == Some(session) => {
                Some(AppCommand::SetSessionArchived {
                    session: session.clone(),
                    archived: true,
                })
            }
            HEADER_MENU_COPY_ID if state.session_menu.as_ref() == Some(session) => {
                Some(AppCommand::ToggleSessionCopyMenu {
                    session: session.clone(),
                })
            }
            HEADER_COPY_TITLE_ID if state.session_copy_menu.as_ref() == Some(session) => {
                current_thread(state, session)
                    .map(|thread| AppCommand::CopyText(thread.title.clone()))
            }
            HEADER_COPY_DETAILS_ID if state.session_copy_menu.as_ref() == Some(session) => {
                current_thread(state, session).map(|thread| {
                    AppCommand::CopyText(format!(
                        "任务：{}\n项目：{}\n任务 ID：{}",
                        thread.title,
                        copy_workspace_label(state, session),
                        session.session_id
                    ))
                })
            }
            HEADER_COPY_SESSION_ID if state.session_copy_menu.as_ref() == Some(session) => {
                Some(AppCommand::CopyText(session.session_id.clone()))
            }
            HEADER_MENU_NEW_WINDOW_ID if state.session_menu.as_ref() == Some(session) => {
                Some(AppCommand::OpenSessionInNewWindow {
                    session: session.clone(),
                })
            }
            HEADER_RENAME_CANCEL_ID
                if state.session_rename.as_ref().map(|rename| &rename.session) == Some(session) =>
            {
                Some(AppCommand::CancelRenameSession {
                    session: session.clone(),
                })
            }
            HEADER_RENAME_SAVE_ID
                if state.session_rename.as_ref().is_some_and(|rename| {
                    &rename.session == session && !rename.draft.trim().is_empty()
                }) =>
            {
                let rename = state.session_rename.as_ref().expect("rename checked above");
                Some(AppCommand::RenameSession {
                    session: session.clone(),
                    title: rename.draft.trim().to_owned(),
                })
            }
            HEADER_ENVIRONMENT_ID => Some(AppCommand::OpenSecondary(SecondaryPane::Environment)),
            HEADER_REVIEW_ID => Some(AppCommand::OpenReview),
            _ => None,
        }
    }

    pub fn root_menu_focus_ids(state: &ZodeAppState) -> Vec<WidgetId> {
        let Some(_) = state
            .current_session
            .as_ref()
            .filter(|session| state.session_menu.as_ref() == Some(*session))
        else {
            return Vec::new();
        };
        vec![
            HEADER_MENU_PIN_ID,
            HEADER_MENU_RENAME_ID,
            HEADER_MENU_ARCHIVE_ID,
            HEADER_MENU_COPY_ID,
            HEADER_MENU_NEW_WINDOW_ID,
        ]
    }

    pub fn copy_menu_focus_ids(state: &ZodeAppState) -> Vec<WidgetId> {
        state
            .current_session
            .as_ref()
            .filter(|session| state.session_copy_menu.as_ref() == Some(*session))
            .map(|_| {
                vec![
                    HEADER_COPY_TITLE_ID,
                    HEADER_COPY_DETAILS_ID,
                    HEADER_COPY_SESSION_ID,
                ]
            })
            .unwrap_or_default()
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
            header.panel_picker = None;
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
            (header.panel_picker, SemanticIcon::Panel),
        ] {
            let Some(action) = action else {
                continue;
            };
            if action.selected {
                painter.fill_round_rect(action.rect, 9.0, theme.tokens.row_selected);
            }
            let (icon_size, icon_color, stroke_width) = if icon == SemanticIcon::More {
                (18.0, theme.tokens.foreground, 2.25)
            } else {
                (16.0, theme.tokens.muted_foreground, icon.stroke_width())
            };
            let icon_rect = Rect::xywh(
                action.rect.origin.x + (action.rect.size.x - icon_size).max(0.0) / 2.0,
                action.rect.origin.y + (action.rect.size.y - icon_size).max(0.0) / 2.0,
                icon_size.min(action.rect.size.x),
                icon_size.min(action.rect.size.y),
            );
            painter.stroke_svg_path(
                icon.path(),
                icon_rect.origin,
                icon_rect.size.x.min(icon_rect.size.y),
                icon_color,
                stroke_width,
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
                .or(header.panel_picker)
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
        viewport: Rect,
        state: &ZodeAppState,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        let rename_input = TextInputState::with_text(
            state
                .session_rename
                .as_ref()
                .map(|rename| rename.draft.clone())
                .unwrap_or_default(),
        );
        Self::paint_overlays_with_rename_input(
            painter,
            rect,
            viewport,
            state,
            &rename_input,
            focused,
            hovered,
            theme,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_overlays_with_rename_input(
        painter: &mut dyn Painter,
        rect: Rect,
        viewport: Rect,
        state: &ZodeAppState,
        rename_input: &TextInputState,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        if let Some(menu) = Self::menu_layout(rect, state) {
            super::thread_header_overlay::paint_task_menu(
                painter, &menu, state, focused, hovered, theme,
            );
        }
        if let Some(rename) = Self::rename_layout(rect, state) {
            super::thread_header_overlay::paint_rename_dialog(
                painter,
                &rename,
                rename_input,
                focused,
                hovered,
                theme,
            );
        }
        if let Some(anchor) = Self::layout(rect, state).panel_picker {
            if let Some(menu) = PanelPicker::menu_layout(anchor.rect, viewport, state) {
                PanelPicker::paint(painter, &menu, focused, hovered, theme);
            }
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

fn current_thread<'a>(
    state: &'a ZodeAppState,
    session: &SessionLocator,
) -> Option<&'a ThreadSummary> {
    state
        .threads
        .iter()
        .find(|thread| &thread.session == session)
}

fn copy_workspace_label(state: &ZodeAppState, session: &SessionLocator) -> String {
    let Some(workspace) = current_thread(state, session).map(|thread| &thread.workspace_uri) else {
        return "未知项目".into();
    };
    if state.is_projectless_workspace(workspace) {
        "不在项目中工作".into()
    } else {
        super::project_sidebar::workspace_label(workspace, state.available_workspace(workspace))
    }
}

fn copy_menu_layout(
    header: Rect,
    root_menu: Rect,
    copy: ThreadMenuActionLayout,
) -> ThreadCopyMenuLayout {
    let width = COPY_MENU_WIDTH.min(header.size.x.max(0.0));
    let height = MENU_PADDING * 2.0 + MENU_ROW_HEIGHT * 3.0;
    let right_x = root_menu.max_x() + 6.0;
    let left_x = (root_menu.origin.x - width - 6.0).max(header.origin.x + 8.0);
    let max_x = header.max_x() - width - 8.0;
    let x = if right_x <= max_x { right_x } else { left_x };
    let rect = Rect::xywh(x, copy.rect.origin.y, width, height);
    let row_x = rect.origin.x + MENU_PADDING;
    let row_w = (rect.size.x - MENU_PADDING * 2.0).max(0.0);
    let action = |id, index| ThreadMenuActionLayout {
        id,
        rect: Rect::xywh(
            row_x,
            rect.origin.y + MENU_PADDING + MENU_ROW_HEIGHT * index as f32,
            row_w,
            MENU_ROW_HEIGHT,
        ),
        enabled: true,
    };
    ThreadCopyMenuLayout {
        id: HEADER_COPY_MENU_ID,
        rect,
        title: action(HEADER_COPY_TITLE_ID, 0),
        details: action(HEADER_COPY_DETAILS_ID, 1),
        session_id: action(HEADER_COPY_SESSION_ID, 2),
    }
}

fn menu_row(
    x: f32,
    width: f32,
    y: &mut f32,
    id: WidgetId,
    enabled: bool,
) -> ThreadMenuActionLayout {
    let action = ThreadMenuActionLayout {
        id,
        rect: Rect::xywh(x, *y, width, MENU_ROW_HEIGHT),
        enabled,
    };
    *y += MENU_ROW_HEIGHT;
    action
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
