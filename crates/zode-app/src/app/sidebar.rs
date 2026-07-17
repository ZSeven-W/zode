use accesskit::Action;
use jian_widgets::{Point2D, Rect};
use zode_app_model::{reduce_navigation_command, AppCommand, NavigationOutcome, ShellRoute};
use zode_app_ui::{Key, KeyEvent, ProjectSidebar};

use super::DesktopApp;

impl DesktopApp {
    /// Scrolls only the sidebar content beneath the fixed new-task row. A
    /// wheel gesture over the sidebar never leaks through to the transcript.
    pub(super) fn handle_sidebar_scroll_delta(&mut self, delta: f32) -> bool {
        let sidebar = self.frame_snapshot.layout.sidebar;
        if !sidebar_accepts_scroll(
            self.app_state.presentation.route,
            self.app_state.shell.sidebar_open,
            ProjectSidebar::scroll_viewport(sidebar),
            self.window_state.cursor_logical,
        ) {
            return false;
        }
        if let Some(command) = ProjectSidebar::scroll_command(sidebar, &self.app_state, delta) {
            if reduce_navigation_command(&mut self.app_state, command) == NavigationOutcome::Applied
            {
                self.rebuild_frame_snapshot();
                self.request_redraw();
            }
        }
        true
    }

    pub(super) fn handle_sidebar_shortcut(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed
            || !event.modifiers.primary()
            || self.app_state.project_picker.open
            || matches!(self.app_state.presentation.route, ShellRoute::Settings(_))
        {
            return false;
        }
        let Some(number) = shortcut_number(event) else {
            return false;
        };
        let Some(session) = ProjectSidebar::shortcut_session(&self.app_state, number) else {
            return false;
        };
        self.enqueue_command(AppCommand::SelectSession(session));
        true
    }

    pub(super) fn handle_sidebar_accessibility_scroll(&mut self, action: Action) -> bool {
        let viewport = ProjectSidebar::scroll_viewport(self.frame_snapshot.layout.sidebar);
        let delta = match action {
            Action::ScrollUp => -viewport.size.y * 0.8,
            Action::ScrollDown => viewport.size.y * 0.8,
            _ => return false,
        };
        if let Some(command) = ProjectSidebar::scroll_command(
            self.frame_snapshot.layout.sidebar,
            &self.app_state,
            delta,
        ) {
            if reduce_navigation_command(&mut self.app_state, command) == NavigationOutcome::Applied
            {
                self.rebuild_frame_snapshot();
                self.request_redraw();
            }
        }
        true
    }
}

fn sidebar_accepts_scroll(
    route: ShellRoute,
    sidebar_open: bool,
    viewport: Rect,
    cursor: Point2D,
) -> bool {
    sidebar_open && !matches!(route, ShellRoute::Settings(_)) && viewport.contains(cursor)
}

fn shortcut_number(event: &KeyEvent) -> Option<usize> {
    let Key::Character(value) = &event.key else {
        return None;
    };
    value
        .chars()
        .next()
        .filter(|_| value.chars().count() == 1)
        .and_then(|value| value.to_digit(10))
        .map(|value| value as usize)
        .filter(|value| (1..=5).contains(value))
}

#[cfg(test)]
mod tests {
    use jian_widgets::{Point2D, Rect};
    use zode_app_model::{SettingsCategory, ShellRoute};
    use zode_app_ui::{Key, KeyEvent, Modifiers};

    use super::{shortcut_number, sidebar_accepts_scroll};

    #[test]
    fn sidebar_scroll_capture_is_limited_to_its_visible_middle() {
        let viewport = Rect::xywh(0.0, 120.0, 240.0, 600.0);
        assert!(sidebar_accepts_scroll(
            ShellRoute::Conversation,
            true,
            viewport,
            Point2D::new(120.0, 300.0),
        ));
        assert!(!sidebar_accepts_scroll(
            ShellRoute::Conversation,
            true,
            viewport,
            Point2D::new(400.0, 300.0),
        ));
        assert!(!sidebar_accepts_scroll(
            ShellRoute::Settings(SettingsCategory::General),
            true,
            viewport,
            Point2D::new(120.0, 300.0),
        ));
    }

    #[test]
    fn only_single_digit_sidebar_shortcuts_one_through_five_are_resolved() {
        let event = |value: &str| KeyEvent {
            key: Key::Character(value.into()),
            modifiers: Modifiers::SUPER,
            pressed: true,
        };
        assert_eq!(shortcut_number(&event("1")), Some(1));
        assert_eq!(shortcut_number(&event("5")), Some(5));
        assert_eq!(shortcut_number(&event("0")), None);
        assert_eq!(shortcut_number(&event("12")), None);
    }
}
