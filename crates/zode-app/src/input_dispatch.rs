use accesskit::Action;
use jian_widgets::{Point2D, Rect};
use zode_app_model::ShellPage;
use zode_app_ui::{
    FocusDirection, Key, KeyEvent, TouchEvent, TouchPhase, WidgetId, WorkspaceSnapshot,
    COMPOSER_ID, HEADER_RENAME_INPUT_ID, INTEGRATIONS_SEARCH_ID, PROJECT_PICKER_SEARCH_ID,
    TERMINAL_ID,
};

use crate::event_map::{route_key_event, InputRoute};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDispatch {
    Widget,
    TerminalPty,
    FocusChanged(Option<WidgetId>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsTouchOutcome {
    Ignored,
    Captured,
    Scroll(f32),
    Tap(Point2D),
}

#[derive(Debug, Clone, Copy)]
struct ActiveSettingsTouch {
    id: u64,
    start_y: f32,
    last_y: f32,
    moved: bool,
}

#[derive(Debug, Default)]
pub struct SettingsTouchTracker {
    active: Option<ActiveSettingsTouch>,
}

impl SettingsTouchTracker {
    pub fn handle(&mut self, event: TouchEvent, content: Rect) -> SettingsTouchOutcome {
        match event.phase {
            TouchPhase::Started if self.active.is_none() && content.contains(event.position) => {
                self.active = Some(ActiveSettingsTouch {
                    id: event.id,
                    start_y: event.position.y,
                    last_y: event.position.y,
                    moved: false,
                });
                SettingsTouchOutcome::Captured
            }
            TouchPhase::Moved => {
                let Some(active) = self.active.as_mut().filter(|active| active.id == event.id)
                else {
                    return SettingsTouchOutcome::Ignored;
                };
                let delta = active.last_y - event.position.y;
                active.last_y = event.position.y;
                active.moved |= (event.position.y - active.start_y).abs() >= 4.0;
                if active.moved && delta != 0.0 {
                    SettingsTouchOutcome::Scroll(delta)
                } else {
                    SettingsTouchOutcome::Captured
                }
            }
            TouchPhase::Ended => {
                let Some(active) = self.active.filter(|active| active.id == event.id) else {
                    return SettingsTouchOutcome::Ignored;
                };
                self.active = None;
                if !active.moved && content.contains(event.position) {
                    SettingsTouchOutcome::Tap(event.position)
                } else {
                    SettingsTouchOutcome::Captured
                }
            }
            TouchPhase::Cancelled => {
                if self.active.is_some_and(|active| active.id == event.id) {
                    self.active = None;
                    return SettingsTouchOutcome::Captured;
                }
                SettingsTouchOutcome::Ignored
            }
            TouchPhase::Started => SettingsTouchOutcome::Ignored,
        }
    }
}

pub fn settings_scroll_delta_for_key(key: &Key, viewport_height: f32) -> Option<f32> {
    let page = (viewport_height * 0.85).max(1.0);
    match key {
        Key::PageUp => Some(-page),
        Key::PageDown => Some(page),
        Key::Home => Some(-f32::MAX),
        Key::End => Some(f32::MAX),
        _ => None,
    }
}

pub fn settings_scroll_delta_for_action(action: Action, viewport_height: f32) -> Option<f32> {
    let page = (viewport_height * 0.85).max(1.0);
    match action {
        Action::ScrollUp => Some(-page),
        Action::ScrollDown => Some(page),
        _ => None,
    }
}

pub fn dispatch_key(
    snapshot: &WorkspaceSnapshot,
    focused: Option<WidgetId>,
    event: &KeyEvent,
    terminal_focused: bool,
) -> KeyDispatch {
    match route_key_event(event, terminal_focused) {
        InputRoute::Widget => KeyDispatch::Widget,
        InputRoute::TerminalPty => KeyDispatch::TerminalPty,
        InputRoute::MoveFocus(direction) => {
            KeyDispatch::FocusChanged(snapshot.move_focus(focused, direction))
        }
    }
}

pub fn direction_for_shift(shift: bool) -> FocusDirection {
    if shift {
        FocusDirection::Backward
    } else {
        FocusDirection::Forward
    }
}

pub fn ime_allowed_for_focus(
    page: ShellPage,
    focused: Option<WidgetId>,
    window_focused: bool,
) -> bool {
    if !window_focused {
        return false;
    }
    match page {
        ShellPage::Terminal => focused == Some(TERMINAL_ID),
        ShellPage::Settings => focused == Some(zode_app_ui::SETTINGS_SEARCH_ID),
        ShellPage::Conversation | ShellPage::Review | ShellPage::ComingSoon => {
            focused == Some(COMPOSER_ID)
                || focused == Some(PROJECT_PICKER_SEARCH_ID)
                || focused == Some(HEADER_RENAME_INPUT_ID)
                || focused == Some(INTEGRATIONS_SEARCH_ID)
        }
    }
}

#[cfg(test)]
mod tests {
    use zode_app_model::ShellPage;
    use zode_app_ui::{WidgetId, HEADER_RENAME_INPUT_ID, INTEGRATIONS_SEARCH_ID};

    use super::ime_allowed_for_focus;

    #[test]
    fn task_rename_input_enables_ime_only_while_the_window_is_focused() {
        assert!(ime_allowed_for_focus(
            ShellPage::Conversation,
            Some(HEADER_RENAME_INPUT_ID),
            true,
        ));
        assert!(!ime_allowed_for_focus(
            ShellPage::Conversation,
            Some(HEADER_RENAME_INPUT_ID),
            false,
        ));
        assert!(!ime_allowed_for_focus(
            ShellPage::Settings,
            Some(WidgetId(HEADER_RENAME_INPUT_ID.0)),
            true,
        ));
    }

    #[test]
    fn integration_search_enables_ime_on_the_typed_catalog_route() {
        assert!(ime_allowed_for_focus(
            ShellPage::ComingSoon,
            Some(INTEGRATIONS_SEARCH_ID),
            true,
        ));
        assert!(!ime_allowed_for_focus(
            ShellPage::ComingSoon,
            Some(INTEGRATIONS_SEARCH_ID),
            false,
        ));
    }
}
