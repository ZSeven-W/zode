//! Host side of the in-conversation find bar: the editable buffer plus the
//! key and IME routing that keeps it in sync with `TranscriptFindState`.
//!
//! Mirrors `global_search.rs`: the controller owns caret/selection/IME, the
//! reducer owns the query and match position, and every edit is projected as
//! an `AppCommand` so the model stays the single source of truth.

use zode_app_model::AppCommand;
use zode_app_ui::{
    ImeEvent, Key, KeyEvent, Modifiers, TranscriptFindBar, TranscriptFindOutcome, WidgetId,
    COMPOSER_ID, TRANSCRIPT_FIND_INPUT_ID,
};
use zode_node_protocol::SessionLocator;

use super::DesktopApp;

impl DesktopApp {
    /// The session whose find bar is currently open, if any.
    fn open_find_session(&self) -> Option<SessionLocator> {
        TranscriptFindBar::active(&self.app_state).map(|(session, _, _)| session.clone())
    }

    /// Handles Cmd+F / Ctrl+F. Only meaningful on a conversation route with a
    /// selected session; every other surface leaves the chord alone.
    pub(super) fn handle_transcript_find_shortcut(&mut self, event: &KeyEvent) -> bool {
        if !crate::input_dispatch::is_transcript_find_shortcut(event) {
            return false;
        }
        let Some(session) = self.app_state.current_session.clone() else {
            return false;
        };
        if !self.app_state.transcripts.contains_key(&session) {
            return false;
        }
        if self.open_find_session().is_some() {
            // Already open: return focus to the field rather than reopening,
            // so a second Cmd+F while reading the transcript gets the user
            // back to typing without discarding the query.
            self.set_focused_widget(Some(TRANSCRIPT_FIND_INPUT_ID));
            return true;
        }
        self.transcript_find_controller.set_text("");
        self.enqueue_command(AppCommand::OpenTranscriptFind { session });
        self.set_focused_widget(Some(TRANSCRIPT_FIND_INPUT_ID));
        true
    }

    /// Consumes keys aimed at an open find bar. Returns `false` when the bar
    /// is closed or the key belongs to some other surface, so the normal
    /// dispatch chain continues.
    pub(super) fn handle_transcript_find_key(&mut self, event: &KeyEvent) -> bool {
        let Some(session) = self.open_find_session().filter(|_| event.pressed) else {
            return false;
        };
        if event.key == Key::Escape {
            self.close_transcript_find(session);
            return true;
        }
        if self.focused_widget != Some(TRANSCRIPT_FIND_INPUT_ID) {
            return false;
        }
        if event.key == Key::Enter {
            // A pending IME composition commits on Enter instead of stepping,
            // so typing Chinese never skips a match while choosing candidates.
            if self
                .transcript_find_controller
                .input_state()
                .composition()
                .is_some()
            {
                let _ = self
                    .transcript_find_controller
                    .key(event.key.clone(), event.modifiers);
                self.sync_transcript_find_from_controller(session);
                return true;
            }
            self.enqueue_command(AppCommand::StepTranscriptFindMatch {
                session,
                forward: !event.modifiers.contains(Modifiers::SHIFT),
            });
            return true;
        }
        if self
            .transcript_find_controller
            .key(event.key.clone(), event.modifiers)
            == TranscriptFindOutcome::Edited
        {
            self.sync_transcript_find_from_controller(session);
            return true;
        }
        // Swallow everything else addressed to the focused field except the
        // paste chord, which the shared clipboard path handles.
        let is_paste = event.modifiers.primary()
            && matches!(&event.key, Key::Character(value) if value.eq_ignore_ascii_case("v"));
        !is_paste
    }

    pub(super) fn handle_transcript_find_ime(&mut self, event: ImeEvent) -> bool {
        let Some(session) = self
            .open_find_session()
            .filter(|_| self.focused_widget == Some(TRANSCRIPT_FIND_INPUT_ID))
        else {
            return false;
        };
        if self.transcript_find_controller.ime(event) == TranscriptFindOutcome::Edited {
            self.sync_transcript_find_from_controller(session);
        }
        true
    }

    pub(super) fn paste_transcript_find_text(&mut self, text: &str) -> bool {
        let Some(session) = self
            .open_find_session()
            .filter(|_| self.focused_widget == Some(TRANSCRIPT_FIND_INPUT_ID))
        else {
            return false;
        };
        if self.transcript_find_controller.paste_text(text) == TranscriptFindOutcome::Edited {
            self.sync_transcript_find_from_controller(session);
        }
        true
    }

    /// Closes the bar and hands focus back to the composer, the surface the
    /// user was in before opening it.
    pub(super) fn close_transcript_find(&mut self, session: SessionLocator) {
        self.transcript_find_controller.set_text("");
        self.enqueue_command(AppCommand::CloseTranscriptFind { session });
        self.set_focused_widget(Some(COMPOSER_ID));
    }

    /// Clicking a find-bar control. Separate from the generic widget-command
    /// path only for the input field, which focuses instead of acting.
    pub(super) fn activate_transcript_find_widget(&mut self, id: WidgetId) -> bool {
        if id == TRANSCRIPT_FIND_INPUT_ID {
            self.set_focused_widget(Some(TRANSCRIPT_FIND_INPUT_ID));
            return true;
        }
        let Some(command) = TranscriptFindBar::command_for_widget(&self.app_state, id) else {
            return false;
        };
        if matches!(command, AppCommand::CloseTranscriptFind { .. }) {
            self.transcript_find_controller.set_text("");
            self.set_focused_widget(Some(COMPOSER_ID));
        }
        self.enqueue_command(command);
        true
    }

    fn sync_transcript_find_from_controller(&mut self, session: SessionLocator) {
        self.enqueue_command(AppCommand::SetTranscriptFindQuery {
            session,
            query: self.transcript_find_controller.text().to_owned(),
        });
    }
}
