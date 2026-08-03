use accesskit::Action;
use jian_widgets::{Point2D, Rect};
use zode_app_model::ShellPage;
use zode_app_ui::{
    FocusDirection, Key, KeyEvent, Modifiers, TouchEvent, TouchPhase, WidgetId, WorkspaceSnapshot,
    ARCHIVED_TASK_SEARCH_ID, COMPOSER_BRANCH_SEARCH_ID, COMPOSER_ID, GLOBAL_SEARCH_INPUT_ID,
    HEADER_RENAME_INPUT_ID, INTEGRATIONS_SEARCH_ID, PLUGIN_ADD_REFERENCE_INPUT_ID,
    PLUGIN_ADD_SPEC_INPUT_ID, PROJECT_PICKER_SEARCH_ID, SETTINGS_SEARCH_ID, TERMINAL_ID,
    TRANSCRIPT_FIND_INPUT_ID,
};

use crate::event_map::{route_key_event, InputRoute};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDispatch {
    Widget,
    TerminalPty,
    FocusChanged(Option<WidgetId>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollTouchOutcome {
    Ignored,
    Captured,
    Scroll(f32),
    Tap(Point2D),
}

#[derive(Debug, Clone, Copy)]
struct ActiveScrollTouch {
    id: u64,
    start_y: f32,
    last_y: f32,
    moved: bool,
}

#[derive(Debug, Default)]
pub struct ScrollTouchTracker {
    active: Option<ActiveScrollTouch>,
}

pub type SettingsTouchOutcome = ScrollTouchOutcome;
pub type SettingsTouchTracker = ScrollTouchTracker;

impl ScrollTouchTracker {
    pub fn handle(&mut self, event: TouchEvent, content: Rect) -> ScrollTouchOutcome {
        match event.phase {
            TouchPhase::Started if self.active.is_none() && content.contains(event.position) => {
                self.active = Some(ActiveScrollTouch {
                    id: event.id,
                    start_y: event.position.y,
                    last_y: event.position.y,
                    moved: false,
                });
                ScrollTouchOutcome::Captured
            }
            TouchPhase::Moved => {
                let Some(active) = self.active.as_mut().filter(|active| active.id == event.id)
                else {
                    return ScrollTouchOutcome::Ignored;
                };
                let delta = active.last_y - event.position.y;
                active.last_y = event.position.y;
                active.moved |= (event.position.y - active.start_y).abs() >= 4.0;
                if active.moved && delta != 0.0 {
                    ScrollTouchOutcome::Scroll(delta)
                } else {
                    ScrollTouchOutcome::Captured
                }
            }
            TouchPhase::Ended => {
                let Some(active) = self.active.filter(|active| active.id == event.id) else {
                    return ScrollTouchOutcome::Ignored;
                };
                self.active = None;
                if !active.moved && content.contains(event.position) {
                    ScrollTouchOutcome::Tap(event.position)
                } else {
                    ScrollTouchOutcome::Captured
                }
            }
            TouchPhase::Cancelled => {
                if self.active.is_some_and(|active| active.id == event.id) {
                    self.active = None;
                    return ScrollTouchOutcome::Captured;
                }
                ScrollTouchOutcome::Ignored
            }
            TouchPhase::Started => ScrollTouchOutcome::Ignored,
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

/// Cmd+F on macOS, Ctrl+F elsewhere - `Modifiers::primary()` is how this
/// codebase already expresses that split (see `is_new_task_shortcut` and
/// `terminal_shortcut_command`), so the platform never has to be named here.
pub fn is_transcript_find_shortcut(event: &KeyEvent) -> bool {
    event.pressed
        && event.modifiers.primary()
        && !event.modifiers.contains(Modifiers::ALT)
        && matches!(&event.key, Key::Character(value) if value.eq_ignore_ascii_case("f"))
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
        ShellPage::Terminal => {
            matches!(focused, Some(TERMINAL_ID | GLOBAL_SEARCH_INPUT_ID))
        }
        ShellPage::Settings => {
            matches!(focused, Some(SETTINGS_SEARCH_ID | ARCHIVED_TASK_SEARCH_ID))
        }
        ShellPage::Conversation | ShellPage::Review | ShellPage::ComingSoon => {
            focused == Some(COMPOSER_ID)
                || focused == Some(COMPOSER_BRANCH_SEARCH_ID)
                || focused == Some(PROJECT_PICKER_SEARCH_ID)
                || focused == Some(HEADER_RENAME_INPUT_ID)
                || focused == Some(INTEGRATIONS_SEARCH_ID)
                || focused == Some(PLUGIN_ADD_SPEC_INPUT_ID)
                || focused == Some(PLUGIN_ADD_REFERENCE_INPUT_ID)
                || focused == Some(GLOBAL_SEARCH_INPUT_ID)
                || focused == Some(TRANSCRIPT_FIND_INPUT_ID)
        }
    }
}

#[cfg(test)]
mod tests {
    use zode_app_model::ShellPage;
    use zode_app_ui::{
        Key, KeyEvent, Modifiers, WidgetId, ARCHIVED_TASK_SEARCH_ID, COMPOSER_BRANCH_SEARCH_ID,
        GLOBAL_SEARCH_INPUT_ID, HEADER_RENAME_INPUT_ID, INTEGRATIONS_SEARCH_ID, SETTINGS_SEARCH_ID,
        TRANSCRIPT_FIND_INPUT_ID,
    };

    use super::{ime_allowed_for_focus, is_transcript_find_shortcut};

    fn key(character: &str, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key: Key::Character(character.into()),
            modifiers,
            pressed: true,
        }
    }

    #[test]
    fn find_is_bound_to_the_platform_primary_modifier_plus_f() {
        assert!(is_transcript_find_shortcut(&key("f", Modifiers::SUPER)));
        assert!(is_transcript_find_shortcut(&key("F", Modifiers::CONTROL)));
        assert!(!is_transcript_find_shortcut(&key("f", Modifiers::NONE)));
        assert!(!is_transcript_find_shortcut(&key("g", Modifiers::SUPER)));
        // Alt+Cmd+F belongs to whatever else may claim it, not the find bar.
        assert!(!is_transcript_find_shortcut(&key(
            "f",
            Modifiers::SUPER | Modifiers::ALT,
        )));
        let released = KeyEvent {
            pressed: false,
            ..key("f", Modifiers::SUPER)
        };
        assert!(!is_transcript_find_shortcut(&released));
    }

    #[test]
    fn the_find_field_enables_ime_on_conversation_surfaces_only() {
        assert!(ime_allowed_for_focus(
            ShellPage::Conversation,
            Some(TRANSCRIPT_FIND_INPUT_ID),
            true,
        ));
        assert!(!ime_allowed_for_focus(
            ShellPage::Conversation,
            Some(TRANSCRIPT_FIND_INPUT_ID),
            false,
        ));
        assert!(!ime_allowed_for_focus(
            ShellPage::Settings,
            Some(TRANSCRIPT_FIND_INPUT_ID),
            true,
        ));
    }

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

    #[test]
    fn global_search_enables_ime_on_every_searchable_surface() {
        for page in [
            ShellPage::Conversation,
            ShellPage::Review,
            ShellPage::Terminal,
            ShellPage::ComingSoon,
        ] {
            assert!(ime_allowed_for_focus(
                page,
                Some(GLOBAL_SEARCH_INPUT_ID),
                true,
            ));
        }
        assert!(!ime_allowed_for_focus(
            ShellPage::Settings,
            Some(GLOBAL_SEARCH_INPUT_ID),
            true,
        ));
        assert!(!ime_allowed_for_focus(
            ShellPage::Terminal,
            Some(COMPOSER_BRANCH_SEARCH_ID),
            true,
        ));
        assert!(!ime_allowed_for_focus(
            ShellPage::Conversation,
            Some(GLOBAL_SEARCH_INPUT_ID),
            false,
        ));
    }

    #[test]
    fn branch_search_enables_ime_only_on_conversation_surfaces() {
        assert!(ime_allowed_for_focus(
            ShellPage::Conversation,
            Some(COMPOSER_BRANCH_SEARCH_ID),
            true,
        ));
        assert!(!ime_allowed_for_focus(
            ShellPage::Settings,
            Some(COMPOSER_BRANCH_SEARCH_ID),
            true,
        ));
    }

    #[test]
    fn both_settings_search_fields_enable_ime() {
        assert!(ime_allowed_for_focus(
            ShellPage::Settings,
            Some(SETTINGS_SEARCH_ID),
            true,
        ));
        assert!(ime_allowed_for_focus(
            ShellPage::Settings,
            Some(ARCHIVED_TASK_SEARCH_ID),
            true,
        ));
        assert!(!ime_allowed_for_focus(
            ShellPage::Settings,
            Some(ARCHIVED_TASK_SEARCH_ID),
            false,
        ));
    }
}
