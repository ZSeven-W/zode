use jian_core::text_input::{prev_char_boundary, TextInputState};
use zode_app_model::ProviderDraft;
use zode_app_ui::{ImeEvent, Key, KeyEvent, Modifiers, WidgetId};
use zode_app_ui::{
    PROVIDER_API_KEY_INPUT_ID, PROVIDER_BASE_URL_INPUT_ID, PROVIDER_ID_INPUT_ID,
    PROVIDER_MODEL_IDS_INPUT_ID,
};

/// Result of editing one provider field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProviderInputOutcome {
    Ignored,
    Consumed,
    TextChanged,
}

/// Host-owned text state for the provider editor.
///
/// In particular, the API-key buffer lives only here. The immutable application
/// state, snapshots and accessibility tree receive only the boolean
/// `credential_configured` projection.
#[derive(Default)]
pub(super) struct ProviderInputController {
    provider_id: TextInputState,
    base_url: TextInputState,
    api_key: TextInputState,
    model_ids: TextInputState,
    now_ms: u64,
}

impl ProviderInputController {
    pub(super) fn begin(&mut self, draft: &ProviderDraft) {
        self.provider_id.set_text(draft.provider_id.clone());
        self.base_url
            .set_text(draft.base_url.clone().unwrap_or_default());
        self.api_key.set_text("");
        self.model_ids.set_text(draft.model_ids.join(", "));
    }

    pub(super) fn clear(&mut self) {
        self.provider_id.set_text("");
        self.base_url.set_text("");
        self.api_key.set_text("");
        self.model_ids.set_text("");
    }

    pub(super) fn text(&self, id: WidgetId) -> Option<&str> {
        Some(self.input(id)?.text())
    }

    pub(super) fn set_text(&mut self, id: WidgetId, value: String) -> bool {
        let Some(input) = self.input_mut(id) else {
            return false;
        };
        input.set_text(value);
        true
    }

    pub(super) fn key(&mut self, id: WidgetId, event: &KeyEvent) -> ProviderInputOutcome {
        if !event.pressed {
            return ProviderInputOutcome::Ignored;
        }
        self.now_ms = self.now_ms.saturating_add(1);
        let now_ms = self.now_ms;
        let Some(input) = self.input_mut(id) else {
            return ProviderInputOutcome::Ignored;
        };
        let select = event.modifiers.contains(Modifiers::SHIFT);
        match &event.key {
            Key::Enter if input.composition().is_some() => {
                input.commit_composition(now_ms);
                ProviderInputOutcome::TextChanged
            }
            Key::Backspace => {
                input.backspace(now_ms);
                ProviderInputOutcome::TextChanged
            }
            Key::Delete => {
                input.delete_forward(now_ms);
                ProviderInputOutcome::TextChanged
            }
            Key::ArrowLeft => {
                input.move_left(select, now_ms);
                ProviderInputOutcome::Consumed
            }
            Key::ArrowRight => {
                input.move_right(select, now_ms);
                ProviderInputOutcome::Consumed
            }
            Key::Home => {
                input.move_home(select, now_ms);
                ProviderInputOutcome::Consumed
            }
            Key::End => {
                input.move_end(select, now_ms);
                ProviderInputOutcome::Consumed
            }
            Key::Character(value)
                if event.modifiers.primary() && value.eq_ignore_ascii_case("a") =>
            {
                input.select_all();
                ProviderInputOutcome::Consumed
            }
            Key::Character(value)
                if !event.modifiers.primary() && !event.modifiers.contains(Modifiers::ALT) =>
            {
                input.insert_str(value, now_ms);
                ProviderInputOutcome::TextChanged
            }
            Key::Enter
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::PageUp
            | Key::PageDown
            | Key::Escape => ProviderInputOutcome::Consumed,
            Key::Tab | Key::Character(_) => ProviderInputOutcome::Ignored,
        }
    }

    pub(super) fn ime(&mut self, id: WidgetId, event: &ImeEvent) -> ProviderInputOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        let now_ms = self.now_ms;
        let Some(input) = self.input_mut(id) else {
            return ProviderInputOutcome::Ignored;
        };
        match event {
            ImeEvent::Start => input.clear_composition(),
            ImeEvent::Update { text, cursor } => {
                let cursor = prev_char_boundary(text, cursor.unwrap_or(text.len()));
                input.set_composition(text.clone(), cursor, now_ms);
            }
            ImeEvent::Commit(text) => {
                input.set_composition(text.clone(), text.len(), now_ms);
                input.commit_composition(now_ms);
                return ProviderInputOutcome::TextChanged;
            }
            ImeEvent::End => input.clear_composition(),
        }
        ProviderInputOutcome::Consumed
    }

    pub(super) fn paste(&mut self, id: WidgetId, text: &str) -> ProviderInputOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        let now_ms = self.now_ms;
        let Some(input) = self.input_mut(id) else {
            return ProviderInputOutcome::Ignored;
        };
        input.insert_str(text, now_ms);
        ProviderInputOutcome::TextChanged
    }

    fn input(&self, id: WidgetId) -> Option<&TextInputState> {
        match id {
            PROVIDER_ID_INPUT_ID => Some(&self.provider_id),
            PROVIDER_BASE_URL_INPUT_ID => Some(&self.base_url),
            PROVIDER_API_KEY_INPUT_ID => Some(&self.api_key),
            PROVIDER_MODEL_IDS_INPUT_ID => Some(&self.model_ids),
            _ => None,
        }
    }

    fn input_mut(&mut self, id: WidgetId) -> Option<&mut TextInputState> {
        match id {
            PROVIDER_ID_INPUT_ID => Some(&mut self.provider_id),
            PROVIDER_BASE_URL_INPUT_ID => Some(&mut self.base_url),
            PROVIDER_API_KEY_INPUT_ID => Some(&mut self.api_key),
            PROVIDER_MODEL_IDS_INPUT_ID => Some(&mut self.model_ids),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            modifiers,
            pressed: true,
        }
    }

    #[test]
    fn caret_and_shift_selection_replace_text_in_the_middle() {
        let mut input = ProviderInputController::default();
        input.set_text(PROVIDER_ID_INPUT_ID, "abc".into());

        input.key(PROVIDER_ID_INPUT_ID, &key(Key::Home, Modifiers::NONE));
        input.key(PROVIDER_ID_INPUT_ID, &key(Key::ArrowRight, Modifiers::NONE));
        input.key(
            PROVIDER_ID_INPUT_ID,
            &key(Key::ArrowRight, Modifiers::SHIFT),
        );
        input.key(
            PROVIDER_ID_INPUT_ID,
            &key(Key::Character("X".into()), Modifiers::NONE),
        );

        assert_eq!(input.text(PROVIDER_ID_INPUT_ID), Some("aXc"));
    }

    #[test]
    fn select_all_then_paste_replaces_the_selection() {
        let mut input = ProviderInputController::default();
        input.set_text(PROVIDER_BASE_URL_INPUT_ID, "old".into());

        input.key(
            PROVIDER_BASE_URL_INPUT_ID,
            &key(Key::Character("a".into()), Modifiers::SUPER),
        );
        input.paste(PROVIDER_BASE_URL_INPUT_ID, "https://new.test/v1");

        assert_eq!(
            input.text(PROVIDER_BASE_URL_INPUT_ID),
            Some("https://new.test/v1")
        );
    }

    #[test]
    fn model_input_preserves_ime_text_and_trailing_delimiters() {
        let mut input = ProviderInputController::default();
        input.set_text(PROVIDER_MODEL_IDS_INPUT_ID, "model-a,".into());
        input.ime(
            PROVIDER_MODEL_IDS_INPUT_ID,
            &ImeEvent::Commit("模型".into()),
        );
        input.paste(PROVIDER_MODEL_IDS_INPUT_ID, ", ");

        assert_eq!(
            input.text(PROVIDER_MODEL_IDS_INPUT_ID),
            Some("model-a,模型, ")
        );
    }

    #[test]
    fn backspace_and_delete_respect_unicode_boundaries() {
        let mut input = ProviderInputController::default();
        input.set_text(PROVIDER_ID_INPUT_ID, "a中b".into());
        input.key(PROVIDER_ID_INPUT_ID, &key(Key::Home, Modifiers::NONE));
        input.key(PROVIDER_ID_INPUT_ID, &key(Key::ArrowRight, Modifiers::NONE));
        input.key(PROVIDER_ID_INPUT_ID, &key(Key::Delete, Modifiers::NONE));
        assert_eq!(input.text(PROVIDER_ID_INPUT_ID), Some("ab"));
        input.key(PROVIDER_ID_INPUT_ID, &key(Key::Backspace, Modifiers::NONE));
        assert_eq!(input.text(PROVIDER_ID_INPUT_ID), Some("b"));
    }
}
