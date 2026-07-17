use zode_app_model::{
    reduce_presentation_command, AppCommand, PresentationCommandOutcome, ShellRoute,
};
use zode_app_ui::{Key, KeyEvent, Modifiers, INTEGRATIONS_SEARCH_ID};

use super::DesktopApp;

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
            | Key::End
            | Key::PageUp
            | Key::PageDown => return true,
            Key::Tab | Key::Character(_) => return false,
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
}
