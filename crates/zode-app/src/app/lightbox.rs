use zode_app_model::{AppCommand, ZodeAppState};
use zode_app_ui::{Key, KeyEvent};

use super::DesktopApp;

/// Pure key-to-command mapping for the open lightbox, kept separate from
/// `DesktopApp` so it is testable without constructing a full app instance
/// (mirrors `session_menu::session_menu_escape_command`'s split between a
/// pure decision function and a thin dispatching method).
fn lightbox_key_command(state: &ZodeAppState, event: &KeyEvent) -> Option<AppCommand> {
    if state.lightbox.is_none() || !event.pressed {
        return None;
    }
    if event.key == Key::Escape {
        return Some(AppCommand::CloseLightbox);
    }
    match &event.key {
        Key::Character(value) if value == "+" || value == "=" => {
            Some(AppCommand::StepLightboxZoom { increase: true })
        }
        Key::Character(value) if value == "-" => {
            Some(AppCommand::StepLightboxZoom { increase: false })
        }
        _ => None,
    }
}

impl DesktopApp {
    /// Esc closes the open lightbox and `+`/`-` step its zoom, mirroring
    /// every other modal overlay's escape handling
    /// (`handle_global_search_key`, `handle_project_picker_key`, etc.) -
    /// checked first in `handle_key_event` since the lightbox paints above
    /// every other layer (see `WorkspaceShell::paint_snapshot_content`) and
    /// must swallow all other keys like the modal it visually is, not just
    /// the ones it recognizes.
    pub(super) fn handle_lightbox_key(&mut self, event: &KeyEvent) -> bool {
        if self.app_state.lightbox.is_none() {
            return false;
        }
        if !event.pressed {
            return true;
        }
        if let Some(command) = lightbox_key_command(&self.app_state, event) {
            self.enqueue_command(command);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use zode_app_model::{demo_state, AppCommand, ImageItem, LightboxState, TranscriptItem};
    use zode_app_ui::{Key, KeyEvent, Modifiers};
    use zode_node_protocol::SessionLocator;

    use super::lightbox_key_command;

    fn state_with_open_lightbox() -> zode_app_model::ZodeAppState {
        let mut state = demo_state();
        let session = SessionLocator::new(state.host.node_id, "lightbox-test");
        state
            .transcripts
            .entry(session.clone())
            .or_default()
            .items
            .push(TranscriptItem::Image(ImageItem {
                id: "image:1".into(),
                path: "/tmp/shot.png".into(),
                media_type: "image/png".into(),
                width: None,
                height: None,
            }));
        state.lightbox = Some(LightboxState::new(session, "image:1".into()));
        state
    }

    fn key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            modifiers: Modifiers::NONE,
            pressed: true,
        }
    }

    #[test]
    fn escape_closes_the_open_lightbox() {
        let state = state_with_open_lightbox();
        assert_eq!(
            lightbox_key_command(&state, &key(Key::Escape)),
            Some(AppCommand::CloseLightbox)
        );
    }

    #[test]
    fn plus_and_minus_step_zoom() {
        let state = state_with_open_lightbox();
        assert_eq!(
            lightbox_key_command(&state, &key(Key::Character("+".into()))),
            Some(AppCommand::StepLightboxZoom { increase: true })
        );
        assert_eq!(
            lightbox_key_command(&state, &key(Key::Character("-".into()))),
            Some(AppCommand::StepLightboxZoom { increase: false })
        );
    }

    #[test]
    fn no_command_when_no_lightbox_is_open() {
        let state = demo_state();
        assert_eq!(lightbox_key_command(&state, &key(Key::Escape)), None);
    }

    #[test]
    fn unrecognized_keys_produce_no_command_but_are_still_swallowed_by_the_caller() {
        let state = state_with_open_lightbox();
        assert_eq!(lightbox_key_command(&state, &key(Key::Tab)), None);
    }
}
