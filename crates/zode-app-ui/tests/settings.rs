use zode_app_model::{
    demo_state, reduce_settings_command, AppCommand, SettingsCommandOutcome, SystemTheme,
    ThemePreference,
};
use zode_app_ui::{animation_duration_ms, resolve_theme, SettingsPanel, ThemeMode, ZodeTheme};

#[test]
fn appearance_controls_exist_without_an_active_project_and_emit_commands() {
    let state = demo_state();
    assert!(state.current_session.is_none());
    let controls = SettingsPanel::appearance_controls(&state);

    assert_eq!(
        controls
            .iter()
            .map(|control| control.label.as_str())
            .collect::<Vec<_>>(),
        vec!["跟随系统", "浅色", "深色", "减少动画", "高对比度"],
    );
    assert_eq!(
        controls[2].command,
        AppCommand::SetThemePreference(ThemePreference::Dark),
    );
    assert_eq!(controls[3].command, AppCommand::SetReducedMotion(true),);
    assert_eq!(controls[4].command, AppCommand::SetHighContrast(true),);
}

#[test]
fn settings_commands_update_preferences_without_mutating_the_observed_system_theme() {
    let mut state = demo_state();
    state.host.system_theme = SystemTheme::Light;

    for command in [
        AppCommand::SetThemePreference(ThemePreference::Dark),
        AppCommand::SetReducedMotion(true),
        AppCommand::SetHighContrast(true),
    ] {
        assert_eq!(
            reduce_settings_command(&mut state, command),
            SettingsCommandOutcome::Applied,
        );
    }

    assert_eq!(state.ui_preferences.theme, ThemePreference::Dark);
    assert!(state.ui_preferences.reduced_motion);
    assert!(state.ui_preferences.high_contrast);
    assert_eq!(state.host.system_theme, SystemTheme::Light);
}

#[test]
fn explicit_theme_and_reduced_motion_override_runtime_presentation_only() {
    let mut state = demo_state();
    state.host.system_theme = SystemTheme::Light;
    state.ui_preferences.theme = ThemePreference::Dark;
    state.ui_preferences.reduced_motion = true;

    assert_eq!(
        resolve_theme(state.host.system_theme, &state.ui_preferences),
        ThemeMode::Dark,
    );
    assert_eq!(animation_duration_ms(180, &state.ui_preferences), 0);
    assert_eq!(state.host.system_theme, SystemTheme::Light);
}

#[test]
fn high_contrast_uses_a_distinct_accessible_token_set() {
    let mut state = demo_state();
    state.ui_preferences.high_contrast = true;
    let regular = ZodeTheme::light();
    let high_contrast = ZodeTheme::for_preferences(SystemTheme::Light, &state.ui_preferences);

    assert_ne!(high_contrast, regular);
    for (foreground, background) in [
        (
            high_contrast.tokens.foreground,
            high_contrast.tokens.background,
        ),
        (
            high_contrast.tokens.card_foreground,
            high_contrast.tokens.card,
        ),
        (
            high_contrast.tokens.muted_foreground,
            high_contrast.tokens.muted,
        ),
    ] {
        assert!(contrast_ratio(foreground, background) >= 4.5);
    }
}

fn contrast_ratio(foreground: jian_widgets::Color, background: jian_widgets::Color) -> f32 {
    let lighter = luminance(foreground).max(luminance(background));
    let darker = luminance(foreground).min(luminance(background));
    (lighter + 0.05) / (darker + 0.05)
}

fn luminance(color: jian_widgets::Color) -> f32 {
    fn channel(value: f32) -> f32 {
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}
