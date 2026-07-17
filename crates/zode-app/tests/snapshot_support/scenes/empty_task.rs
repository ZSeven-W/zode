use zode_app_model::{ShellRoute, ThemePreference};

use super::ReferenceScene;
use crate::snapshot_support::fixture::base_scene_state;

pub fn empty_task_scene(theme: ThemePreference, viewport_width: u32) -> ReferenceScene {
    let mut state = base_scene_state(theme, viewport_width);
    state.current_session = None;
    state.transcripts.clear();
    state.presentation.route = ShellRoute::Conversation;
    state.presentation.secondary_pane = None;
    state.shell.page = ShellRoute::Conversation.legacy_page();
    ReferenceScene {
        name: "empty-task",
        state,
    }
}
