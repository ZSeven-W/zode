use zode_app_model::{reduce_presentation_command, AppCommand, PresentationCommandOutcome};
use zode_app_ui::{
    Key, KeyEvent, Modifiers, WidgetId, PLUGIN_ADD_REFERENCE_INPUT_ID, PLUGIN_ADD_SPEC_INPUT_ID,
};

use super::DesktopApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKeyOutcome {
    Changed,
    Consumed,
    Ignored,
}

impl DesktopApp {
    pub(super) fn handle_plugin_add_key(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed {
            return false;
        }
        let Some((id, mut value)) = self.focused_plugin_add_field() else {
            return false;
        };
        match edit_field(&mut value, event) {
            FieldKeyOutcome::Changed => self.set_plugin_add_field_value(id, value),
            FieldKeyOutcome::Consumed => return true,
            FieldKeyOutcome::Ignored => return false,
        }
        true
    }

    pub(super) fn handle_plugin_add_ime(&mut self, event: &zode_app_ui::ImeEvent) -> bool {
        let Some((id, mut value)) = self.focused_plugin_add_field() else {
            return false;
        };
        if let zode_app_ui::ImeEvent::Commit(text) = event {
            value.push_str(text);
            self.set_plugin_add_field_value(id, value);
        }
        true
    }

    pub(super) fn paste_plugin_add_text(&mut self, text: &str) -> bool {
        let Some((id, mut value)) = self.focused_plugin_add_field() else {
            return false;
        };
        value.push_str(text);
        self.set_plugin_add_field_value(id, value);
        true
    }

    fn focused_plugin_add_field(&self) -> Option<(WidgetId, String)> {
        if !self.app_state.presentation.plugin_add.open {
            return None;
        }
        match self.focused_widget {
            Some(PLUGIN_ADD_SPEC_INPUT_ID) => Some((
                PLUGIN_ADD_SPEC_INPUT_ID,
                self.app_state.presentation.plugin_add.spec.clone(),
            )),
            Some(PLUGIN_ADD_REFERENCE_INPUT_ID) => Some((
                PLUGIN_ADD_REFERENCE_INPUT_ID,
                self.app_state.presentation.plugin_add.reference.clone(),
            )),
            _ => None,
        }
    }

    pub(super) fn set_plugin_add_field_value(&mut self, id: WidgetId, value: String) {
        let command = match id {
            PLUGIN_ADD_SPEC_INPUT_ID => AppCommand::SetPluginAddSpec(value),
            PLUGIN_ADD_REFERENCE_INPUT_ID => AppCommand::SetPluginAddReference(value),
            _ => return,
        };
        if reduce_presentation_command(&mut self.app_state, command)
            == PresentationCommandOutcome::Applied
        {
            self.rebuild_frame_snapshot();
            self.request_redraw();
        }
    }
}

fn edit_field(value: &mut String, event: &KeyEvent) -> FieldKeyOutcome {
    match &event.key {
        Key::Escape if !value.is_empty() => value.clear(),
        Key::Escape => return FieldKeyOutcome::Ignored,
        Key::Backspace => {
            value.pop();
        }
        Key::Delete => value.clear(),
        Key::Character(character)
            if event.modifiers.primary() && character.eq_ignore_ascii_case("a") =>
        {
            value.clear();
        }
        Key::Character(character)
            if !event.modifiers.primary() && !event.modifiers.contains(Modifiers::ALT) =>
        {
            value.push_str(character);
        }
        Key::Enter
        | Key::ArrowLeft
        | Key::ArrowRight
        | Key::ArrowUp
        | Key::ArrowDown
        | Key::Home
        | Key::End
        | Key::PageUp
        | Key::PageDown => return FieldKeyOutcome::Consumed,
        Key::Tab | Key::Character(_) => return FieldKeyOutcome::Ignored,
    }
    FieldKeyOutcome::Changed
}
