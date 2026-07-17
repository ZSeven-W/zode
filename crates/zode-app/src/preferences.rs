use zode_app_model::ZodeAppState;
use zode_app_ui::ZodeTheme;

pub fn theme_for_state(state: &ZodeAppState) -> ZodeTheme {
    ZodeTheme::for_preferences(state.host.system_theme, &state.ui_preferences)
}
