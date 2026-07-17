use zode_app_model::{SettingsCategory, ShellRoute, ThemePreference};

use super::ReferenceScene;
use crate::snapshot_support::fixture::base_scene_state;

pub fn settings_general_scene(theme: ThemePreference, viewport_width: u32) -> ReferenceScene {
    let mut state = base_scene_state(theme, viewport_width);
    let route = ShellRoute::Settings(SettingsCategory::General);
    state.presentation.route = route;
    state.presentation.secondary_pane = None;
    state.shell.page = route.legacy_page();
    ReferenceScene {
        name: "settings-general",
        state,
    }
}
