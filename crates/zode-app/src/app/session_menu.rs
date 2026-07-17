use jian_widgets::Point2D;
use zode_app_model::{AppCommand, ZodeAppState};
use zode_app_ui::{
    ImeEvent, Key, KeyEvent, Modifiers, SessionRenameOutcome, ThreadHeader, WidgetId,
    WorkspaceSnapshot, HEADER_COPY_TITLE_ID, HEADER_MENU_COPY_ID, HEADER_MENU_PIN_ID,
    HEADER_MORE_ID, HEADER_RENAME_CANCEL_ID, HEADER_RENAME_INPUT_ID, HEADER_RENAME_SAVE_ID,
};

use super::DesktopApp;
use crate::{clipboard::ClipboardService, services::SessionWindowService};

pub(super) fn session_menu_escape_command(state: &ZodeAppState) -> Option<AppCommand> {
    if let Some(rename) = state.session_rename.as_ref() {
        return Some(AppCommand::CancelRenameSession {
            session: rename.session.clone(),
        });
    }
    if let Some(session) = state.session_copy_menu.as_ref() {
        return Some(AppCommand::ToggleSessionCopyMenu {
            session: session.clone(),
        });
    }
    state
        .session_menu
        .as_ref()
        .filter(|session| state.current_session.as_ref() == Some(*session))
        .map(|session| AppCommand::ToggleSessionMenu {
            session: session.clone(),
        })
}

pub(super) fn session_menu_outside_click_command(
    state: &ZodeAppState,
    snapshot: &WorkspaceSnapshot,
    position: Point2D,
) -> Option<AppCommand> {
    if let Some(rename) = state.session_rename.as_ref() {
        let inside = ThreadHeader::rename_layout(snapshot.layout.top_bar, state)
            .is_some_and(|layout| layout.rect.contains(position));
        return (!inside).then(|| AppCommand::CancelRenameSession {
            session: rename.session.clone(),
        });
    }
    let session = state
        .session_menu
        .as_ref()
        .filter(|session| state.current_session.as_ref() == Some(*session))?;
    let command = AppCommand::ToggleSessionMenu {
        session: session.clone(),
    };
    let inside = ThreadHeader::menu_layout(snapshot.layout.top_bar, state).is_some_and(|menu| {
        menu.rect.contains(position)
            || menu
                .copy_menu
                .is_some_and(|copy| copy.rect.contains(position))
    });
    (!inside).then_some(command)
}

pub(super) fn consume_session_window_command(
    state: &mut ZodeAppState,
    service: &dyn SessionWindowService,
    command: &AppCommand,
) -> bool {
    let AppCommand::OpenSessionInNewWindow { session } = command else {
        return false;
    };
    let valid = state.current_session.as_ref() == Some(session)
        && state
            .threads
            .iter()
            .any(|thread| &thread.session == session);
    if valid {
        if let Err(error) = service.open_session(session) {
            eprintln!("zode-app: opening task window failed: {error}");
        }
        state.close_session_action_surfaces();
    }
    true
}

impl DesktopApp {
    pub(super) fn handle_session_action_key(&mut self, event: &KeyEvent) -> bool {
        if self.app_state.session_rename.is_some() {
            return self.handle_session_rename_key(event);
        }
        if self.app_state.session_menu.is_none() || !event.pressed {
            return false;
        }
        if event.key == Key::Escape {
            if let Some(command) = session_menu_escape_command(&self.app_state) {
                self.enqueue_command(command);
            }
            return true;
        }
        let ids = if self.app_state.session_copy_menu.is_some() {
            ThreadHeader::copy_menu_focus_ids(&self.app_state)
        } else {
            ThreadHeader::root_menu_focus_ids(&self.app_state)
        };
        if event.key == Key::Tab || matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
            let backwards = event.key == Key::ArrowUp || event.modifiers.contains(Modifiers::SHIFT);
            self.cycle_session_action_focus(&ids, backwards);
            return true;
        }
        !matches!(event.key, Key::Enter)
            && !matches!(&event.key, Key::Character(value) if value == " ")
    }

    pub(super) fn handle_session_rename_ime(&mut self, event: ImeEvent) -> bool {
        if self.app_state.session_rename.is_none()
            || self.focused_widget != Some(HEADER_RENAME_INPUT_ID)
        {
            return false;
        }
        if self.session_rename_controller.ime(event) == SessionRenameOutcome::Edited {
            self.sync_session_rename_from_controller();
        }
        true
    }

    pub(super) fn paste_session_rename_from_clipboard(
        &mut self,
        clipboard: &dyn ClipboardService,
    ) -> bool {
        if self.app_state.session_rename.is_none()
            || self.focused_widget != Some(HEADER_RENAME_INPUT_ID)
        {
            return false;
        }
        match clipboard.read_text() {
            Ok(Some(text)) if !text.is_empty() => {
                let _ = self.session_rename_controller.paste_text(&text);
                self.sync_session_rename_from_controller();
            }
            Ok(_) => {}
            Err(error) => eprintln!("zode-app: clipboard read failed: {error}"),
        }
        true
    }

    pub(super) fn set_session_rename_value(&mut self, value: String) {
        self.session_rename_controller.set_text(value);
        self.sync_session_rename_from_controller();
    }

    pub(super) fn sync_session_action_after_navigation(
        &mut self,
        command: &AppCommand,
    ) -> Option<WidgetId> {
        match command {
            AppCommand::ToggleSessionMenu { .. } => self
                .app_state
                .session_menu
                .is_some()
                .then_some(HEADER_MENU_PIN_ID)
                .or(Some(HEADER_MORE_ID)),
            AppCommand::ToggleSessionCopyMenu { .. } => self
                .app_state
                .session_copy_menu
                .is_some()
                .then_some(HEADER_COPY_TITLE_ID)
                .or(Some(HEADER_MENU_COPY_ID)),
            AppCommand::BeginRenameSession { .. } => {
                let draft = self
                    .app_state
                    .session_rename
                    .as_ref()
                    .map(|rename| rename.draft.clone())?;
                self.session_rename_controller.set_text(draft);
                Some(HEADER_RENAME_INPUT_ID)
            }
            AppCommand::CancelRenameSession { .. } => {
                self.session_rename_controller.set_text("");
                Some(HEADER_MORE_ID)
            }
            _ => None,
        }
    }

    fn handle_session_rename_key(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed {
            return true;
        }
        let session = self
            .app_state
            .session_rename
            .as_ref()
            .map(|rename| rename.session.clone())
            .expect("rename checked by caller");
        if event.key == Key::Escape {
            self.enqueue_command(AppCommand::CancelRenameSession { session });
            return true;
        }
        let save_enabled = !self.session_rename_controller.text().trim().is_empty();
        let mut ids = vec![HEADER_RENAME_INPUT_ID, HEADER_RENAME_CANCEL_ID];
        if save_enabled {
            ids.push(HEADER_RENAME_SAVE_ID);
        }
        if event.key == Key::Tab || matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
            let backwards = event.key == Key::ArrowUp || event.modifiers.contains(Modifiers::SHIFT);
            self.cycle_session_action_focus(&ids, backwards);
            return true;
        }
        if self.focused_widget != Some(HEADER_RENAME_INPUT_ID) {
            return !matches!(event.key, Key::Enter)
                && !matches!(&event.key, Key::Character(value) if value == " ");
        }
        let paste = event.modifiers.primary()
            && matches!(&event.key, Key::Character(value) if value.eq_ignore_ascii_case("v"));
        if paste {
            return false;
        }
        if event.key == Key::Enter
            && self
                .session_rename_controller
                .input_state()
                .composition()
                .is_none()
        {
            if save_enabled {
                self.enqueue_command(AppCommand::RenameSession {
                    session,
                    title: self.session_rename_controller.text().trim().to_owned(),
                });
            }
            return true;
        }
        if self
            .session_rename_controller
            .key(event.key.clone(), event.modifiers)
            == SessionRenameOutcome::Edited
        {
            self.sync_session_rename_from_controller();
        }
        true
    }

    fn sync_session_rename_from_controller(&mut self) {
        let Some(session) = self
            .app_state
            .session_rename
            .as_ref()
            .map(|rename| rename.session.clone())
        else {
            return;
        };
        self.enqueue_command(AppCommand::SetSessionRenameDraft {
            session,
            draft: self.session_rename_controller.text().to_owned(),
        });
    }

    fn cycle_session_action_focus(&mut self, ids: &[WidgetId], backwards: bool) {
        if ids.is_empty() {
            return;
        }
        let current = self
            .focused_widget
            .and_then(|focused| ids.iter().position(|id| *id == focused));
        let index = match (backwards, current) {
            (false, Some(index)) => (index + 1) % ids.len(),
            (true, Some(0) | None) => ids.len() - 1,
            (true, Some(index)) => index - 1,
            (false, None) => 0,
        };
        self.set_focused_widget(Some(ids[index]));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::{consume_session_window_command, session_menu_escape_command};
    use crate::services::{ServiceError, SessionWindowService};
    use zode_app_model::{AppCommand, ProjectState};
    use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

    #[derive(Default)]
    struct RecordingWindow(Mutex<Vec<SessionLocator>>);

    impl SessionWindowService for RecordingWindow {
        fn open_session(&self, session: &SessionLocator) -> Result<(), ServiceError> {
            self.0.lock().unwrap().push(session.clone());
            Ok(())
        }
    }

    fn state_with_session() -> (zode_app_model::ZodeAppState, SessionLocator) {
        let mut state = zode_app_model::demo_state();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        let session = SessionLocator::new(state.host.node_id, "session-menu");
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 1,
        });
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: workspace,
            title: "task".into(),
            updated_at_ms: 1,
            status: ThreadStatus::Idle,
        });
        state.current_session = Some(session.clone());
        (state, session)
    }

    #[test]
    fn escape_closes_the_deepest_task_surface_first() {
        let (mut state, session) = state_with_session();
        state.session_menu = Some(session.clone());
        state.session_copy_menu = Some(session.clone());
        assert_eq!(
            session_menu_escape_command(&state),
            Some(AppCommand::ToggleSessionCopyMenu {
                session: session.clone()
            })
        );
        state.session_rename = Some(zode_app_model::SessionRenameState {
            session: session.clone(),
            draft: "new title".into(),
        });
        assert_eq!(
            session_menu_escape_command(&state),
            Some(AppCommand::CancelRenameSession { session })
        );
    }

    #[test]
    fn new_window_effect_forwards_the_exact_task_and_closes_the_menu() {
        let (mut state, session) = state_with_session();
        state.session_menu = Some(session.clone());
        let service = RecordingWindow::default();

        assert!(consume_session_window_command(
            &mut state,
            &service,
            &AppCommand::OpenSessionInNewWindow {
                session: session.clone()
            }
        ));
        assert_eq!(*service.0.lock().unwrap(), vec![session]);
        assert!(state.session_menu.is_none());
    }
}
