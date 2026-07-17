use accesskit::Action;
use zode_app_model::{
    reduce_presentation_command, AppCommand, PresentationCommandOutcome, ShellRoute,
};
use zode_app_ui::{
    IntegrationsPage, Key, KeyEvent, Modifiers, PointerButton, PointerEvent, PointerEventKind,
    TouchEvent, INTEGRATIONS_SEARCH_ID,
};

use super::DesktopApp;
use crate::input_dispatch::ScrollTouchOutcome;

impl DesktopApp {
    pub(super) fn handle_integration_search_key(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed
            || !matches!(
                self.app_state.presentation.route,
                ShellRoute::Integrations(_)
            )
            || self.focused_widget != Some(INTEGRATIONS_SEARCH_ID)
        {
            return false;
        }
        let mut query = self.app_state.presentation.integration_search.clone();
        match &event.key {
            Key::Escape if !query.is_empty() => query.clear(),
            Key::Escape => return false,
            Key::Backspace => {
                query.pop();
            }
            Key::Delete => query.clear(),
            Key::Character(value)
                if event.modifiers.primary() && value.eq_ignore_ascii_case("a") =>
            {
                query.clear();
            }
            Key::Character(value)
                if !event.modifiers.primary() && !event.modifiers.contains(Modifiers::ALT) =>
            {
                query.push_str(value);
            }
            Key::Enter
            | Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End => return true,
            Key::PageUp | Key::PageDown | Key::Tab | Key::Character(_) => return false,
        }
        self.set_integration_search_value(query);
        true
    }

    pub(super) fn handle_integration_search_ime(&mut self, event: &zode_app_ui::ImeEvent) -> bool {
        if !matches!(
            self.app_state.presentation.route,
            ShellRoute::Integrations(_)
        ) || self.focused_widget != Some(INTEGRATIONS_SEARCH_ID)
        {
            return false;
        }
        if let zode_app_ui::ImeEvent::Commit(text) = event {
            let mut query = self.app_state.presentation.integration_search.clone();
            query.push_str(text);
            self.set_integration_search_value(query);
        }
        true
    }

    pub(super) fn paste_integration_search_text(&mut self, text: &str) -> bool {
        if !matches!(
            self.app_state.presentation.route,
            ShellRoute::Integrations(_)
        ) || self.focused_widget != Some(INTEGRATIONS_SEARCH_ID)
        {
            return false;
        }
        let mut query = self.app_state.presentation.integration_search.clone();
        query.push_str(text);
        self.set_integration_search_value(query);
        true
    }

    pub(super) fn set_integration_search_value(&mut self, value: String) {
        if reduce_presentation_command(&mut self.app_state, AppCommand::SetIntegrationSearch(value))
            == PresentationCommandOutcome::Applied
        {
            self.rebuild_frame_snapshot();
            self.request_redraw();
        }
    }

    pub(super) fn apply_integrations_scroll_delta(&mut self, delta: f32) {
        let viewport = self.frame_snapshot.layout.primary_surface;
        let Some(command) = IntegrationsPage::scroll_command(viewport, &self.app_state, delta)
        else {
            return;
        };
        if reduce_presentation_command(&mut self.app_state, command)
            == PresentationCommandOutcome::Applied
        {
            self.rebuild_frame_snapshot();
            self.request_redraw();
        }
    }

    pub(super) fn handle_integrations_wheel(&mut self, delta: f32) -> bool {
        if !matches!(
            self.app_state.presentation.route,
            ShellRoute::Integrations(_)
        ) {
            return false;
        }
        let catalog =
            IntegrationsPage::layout(self.frame_snapshot.layout.primary_surface, &self.app_state)
                .catalog;
        if !catalog.contains(self.window_state.cursor_logical) {
            return false;
        }
        self.apply_integrations_scroll_delta(delta);
        true
    }

    pub(super) fn handle_integrations_touch(&mut self, event: TouchEvent) -> bool {
        if !matches!(
            self.app_state.presentation.route,
            ShellRoute::Integrations(_)
        ) {
            return false;
        }
        let catalog =
            IntegrationsPage::layout(self.frame_snapshot.layout.primary_surface, &self.app_state)
                .catalog;
        match self.scroll_touch.handle(event, catalog) {
            ScrollTouchOutcome::Captured => true,
            ScrollTouchOutcome::Scroll(delta) => {
                self.apply_integrations_scroll_delta(delta);
                true
            }
            ScrollTouchOutcome::Tap(position) => {
                self.handle_pointer_event(PointerEvent {
                    position,
                    kind: PointerEventKind::Press,
                    button: Some(PointerButton::Primary),
                });
                true
            }
            ScrollTouchOutcome::Ignored => false,
        }
    }

    pub(super) fn handle_integrations_page_key(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed
            || !matches!(
                self.app_state.presentation.route,
                ShellRoute::Integrations(_)
            )
        {
            return false;
        }
        let viewport_height =
            IntegrationsPage::layout(self.frame_snapshot.layout.primary_surface, &self.app_state)
                .catalog
                .size
                .y;
        let Some(delta) = integrations_scroll_delta_for_key(&event.key, viewport_height) else {
            return false;
        };
        self.apply_integrations_scroll_delta(delta);
        true
    }

    pub(super) fn handle_integrations_accessibility_scroll(&mut self, action: Action) -> bool {
        let viewport_height =
            IntegrationsPage::layout(self.frame_snapshot.layout.primary_surface, &self.app_state)
                .catalog
                .size
                .y;
        let Some(delta) = integrations_scroll_delta_for_action(action, viewport_height) else {
            return false;
        };
        self.apply_integrations_scroll_delta(delta);
        true
    }
}

fn integrations_scroll_delta_for_key(key: &Key, viewport_height: f32) -> Option<f32> {
    let page = (viewport_height * 0.85).max(1.0);
    match key {
        Key::PageUp => Some(-page),
        Key::PageDown => Some(page),
        _ => None,
    }
}

fn integrations_scroll_delta_for_action(action: Action, viewport_height: f32) -> Option<f32> {
    let page = (viewport_height * 0.85).max(1.0);
    match action {
        Action::ScrollUp => Some(-page),
        Action::ScrollDown => Some(page),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use accesskit::Action;

    use super::{integrations_scroll_delta_for_action, integrations_scroll_delta_for_key};
    use zode_app_ui::Key;

    #[test]
    fn page_keys_scroll_the_integrations_catalog_by_a_viewport_fraction() {
        assert_eq!(
            integrations_scroll_delta_for_key(&Key::PageUp, 400.0),
            Some(-340.0)
        );
        assert_eq!(
            integrations_scroll_delta_for_key(&Key::PageDown, 400.0),
            Some(340.0)
        );
        assert_eq!(
            integrations_scroll_delta_for_key(&Key::ArrowDown, 400.0),
            None
        );
        assert_eq!(
            integrations_scroll_delta_for_action(Action::ScrollUp, 400.0),
            Some(-340.0)
        );
        assert_eq!(
            integrations_scroll_delta_for_action(Action::ScrollDown, 400.0),
            Some(340.0)
        );
    }
}
