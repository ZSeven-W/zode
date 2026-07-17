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
    for theme in [high_contrast, ZodeTheme::high_contrast(ThemeMode::Dark)] {
        let destructive_notice = composite(
            theme.tokens.destructive.with_alpha(0.12),
            theme.tokens.background,
        );
        let destructive_card =
            composite(theme.tokens.destructive.with_alpha(0.12), theme.tokens.card);
        let destructive_muted = composite(
            theme.tokens.destructive.with_alpha(0.12),
            theme.tokens.muted,
        );
        for (name, foreground, background) in [
            ("canvas", theme.tokens.foreground, theme.tokens.background),
            ("card", theme.tokens.card_foreground, theme.tokens.card),
            (
                "popover",
                theme.tokens.popover_foreground,
                theme.tokens.popover,
            ),
            ("muted", theme.tokens.muted_foreground, theme.tokens.muted),
            ("muted body", theme.tokens.foreground, theme.tokens.muted),
            (
                "accent",
                theme.tokens.accent_foreground,
                theme.tokens.accent,
            ),
            (
                "secondary",
                theme.tokens.secondary_foreground,
                theme.tokens.secondary,
            ),
            (
                "primary",
                theme.tokens.primary_foreground,
                theme.tokens.primary,
            ),
            (
                "destructive surface",
                theme.tokens.destructive_foreground,
                theme.tokens.destructive,
            ),
            (
                "destructive notice text",
                theme.tokens.destructive,
                destructive_notice,
            ),
            (
                "destructive card text",
                theme.tokens.destructive,
                destructive_card,
            ),
            (
                "destructive approval text",
                theme.tokens.destructive,
                destructive_muted,
            ),
            ("sidebar", theme.sidebar_foreground, theme.sidebar),
            ("user bubble", theme.tokens.foreground, theme.user_bubble),
            (
                "muted text on canvas",
                theme.tokens.muted_foreground,
                theme.tokens.background,
            ),
            (
                "muted text on card",
                theme.tokens.muted_foreground,
                theme.tokens.card,
            ),
            (
                "brand on canvas",
                theme.zode_purple,
                theme.tokens.background,
            ),
            ("brand on sidebar", theme.zode_purple, theme.sidebar),
            ("brand on card", theme.zode_purple, theme.tokens.card),
        ] {
            assert!(
                contrast_ratio(foreground, background) >= 4.5,
                "{name} contrast must be at least 4.5:1"
            );
        }
    }
}

fn composite(
    foreground: jian_widgets::Color,
    background: jian_widgets::Color,
) -> jian_widgets::Color {
    let alpha = foreground.a;
    jian_widgets::Color {
        r: foreground.r * alpha + background.r * (1.0 - alpha),
        g: foreground.g * alpha + background.g * (1.0 - alpha),
        b: foreground.b * alpha + background.b * (1.0 - alpha),
        a: 1.0,
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
