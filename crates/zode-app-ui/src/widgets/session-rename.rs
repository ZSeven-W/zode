use jian_core::text_input::{prev_char_boundary, TextInputState};

use crate::{ImeEvent, Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRenameOutcome {
    Ignored,
    Edited,
}

/// Native editable buffer for the task-title dialog. The immutable app model
/// mirrors only `text()` so layout, hit testing, and accessibility stay pure.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRenameController {
    input: TextInputState,
    now_ms: u64,
}

impl SessionRenameController {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            input: TextInputState::with_text(text.into()),
            now_ms: 0,
        }
    }

    pub fn input_state(&self) -> &TextInputState {
        &self.input
    }

    pub fn text(&self) -> &str {
        self.input.text()
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.input.set_text(text.into());
        self.input.clear_composition();
    }

    pub fn key(&mut self, key: Key, modifiers: Modifiers) -> SessionRenameOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        match key {
            Key::Enter if self.input.composition().is_some() => {
                self.input.commit_composition(self.now_ms);
                SessionRenameOutcome::Edited
            }
            Key::Backspace => {
                self.input.backspace(self.now_ms);
                SessionRenameOutcome::Edited
            }
            Key::Delete => {
                self.input.delete_forward(self.now_ms);
                SessionRenameOutcome::Edited
            }
            Key::ArrowLeft => {
                self.input
                    .move_left(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                SessionRenameOutcome::Edited
            }
            Key::ArrowRight => {
                self.input
                    .move_right(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                SessionRenameOutcome::Edited
            }
            Key::Home => {
                self.input
                    .move_home(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                SessionRenameOutcome::Edited
            }
            Key::End => {
                self.input
                    .move_end(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                SessionRenameOutcome::Edited
            }
            Key::Character(character)
                if modifiers.primary() && character.eq_ignore_ascii_case("a") =>
            {
                self.input.select_all();
                SessionRenameOutcome::Edited
            }
            Key::Character(character)
                if !modifiers.primary() && !modifiers.contains(Modifiers::ALT) =>
            {
                self.input.insert_str(&character, self.now_ms);
                SessionRenameOutcome::Edited
            }
            Key::Enter
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::PageUp
            | Key::PageDown
            | Key::Tab
            | Key::Escape
            | Key::Character(_) => SessionRenameOutcome::Ignored,
        }
    }

    pub fn ime(&mut self, event: ImeEvent) -> SessionRenameOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        match event {
            ImeEvent::Start => self.input.clear_composition(),
            ImeEvent::Update { text, cursor } => {
                let cursor = prev_char_boundary(&text, cursor.unwrap_or(text.len()));
                self.input.set_composition(text, cursor, self.now_ms);
            }
            ImeEvent::Commit(text) => {
                let cursor = text.len();
                self.input.set_composition(text, cursor, self.now_ms);
                self.input.commit_composition(self.now_ms);
            }
            ImeEvent::End => self.input.clear_composition(),
        }
        SessionRenameOutcome::Edited
    }

    pub fn paste_text(&mut self, text: &str) -> SessionRenameOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        self.input.insert_str(text, self.now_ms);
        SessionRenameOutcome::Edited
    }
}

impl Default for SessionRenameController {
    fn default() -> Self {
        Self::new("")
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionRenameController, SessionRenameOutcome};
    use crate::{ImeEvent, Key, Modifiers};

    #[test]
    fn keyboard_ime_and_paste_share_one_editable_title_buffer() {
        let mut controller = SessionRenameController::new("Zode");
        assert_eq!(
            controller.key(Key::Character(" 桌面".into()), Modifiers::NONE),
            SessionRenameOutcome::Edited
        );
        let _ = controller.ime(ImeEvent::Commit("端".into()));
        let _ = controller.paste_text(" v1");
        assert_eq!(controller.text(), "Zode 桌面端 v1");
    }
}
