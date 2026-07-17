use jian_widgets::{Color, Tokens};
use zode_app_model::{SystemTheme, UiPreferences};

pub const ZODE_PURPLE: Color = Color::rgb_u8(124, 58, 237);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

pub fn resolve_theme(system: SystemTheme, _preferences: &UiPreferences) -> ThemeMode {
    match system {
        SystemTheme::Light => ThemeMode::Light,
        SystemTheme::Dark => ThemeMode::Dark,
    }
}

pub fn animation_duration_ms(duration_ms: u64, _preferences: &UiPreferences) -> u64 {
    duration_ms
}

/// Jian component tokens plus Zode shell-specific semantic colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZodeTheme {
    pub tokens: Tokens,
    pub sidebar: Color,
    pub sidebar_foreground: Color,
    pub user_bubble: Color,
    pub success: Color,
    pub warning: Color,
    pub zode_purple: Color,
}

impl ZodeTheme {
    pub fn for_preferences(system: SystemTheme, preferences: &UiPreferences) -> Self {
        match resolve_theme(system, preferences) {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        }
    }
    pub const fn light() -> Self {
        let mut tokens = Tokens::light();
        tokens.primary = ZODE_PURPLE;
        tokens.ring = ZODE_PURPLE;
        tokens.row_selected_primary = Color::rgba_u8(124, 58, 237, 0.14);
        Self {
            tokens,
            sidebar: Color::rgb_u8(246, 246, 244),
            sidebar_foreground: Color::rgb_u8(45, 45, 43),
            user_bubble: Color::rgb_u8(239, 239, 237),
            success: Color::rgb_u8(22, 163, 74),
            warning: Color::rgb_u8(217, 119, 6),
            zode_purple: ZODE_PURPLE,
        }
    }

    pub const fn dark() -> Self {
        let mut tokens = Tokens::dark();
        tokens.primary = ZODE_PURPLE;
        tokens.ring = ZODE_PURPLE;
        tokens.row_selected_primary = Color::rgba_u8(124, 58, 237, 0.22);
        Self {
            tokens,
            sidebar: Color::rgb_u8(21, 21, 22),
            sidebar_foreground: Color::rgb_u8(232, 232, 230),
            user_bubble: Color::rgb_u8(42, 42, 44),
            success: Color::rgb_u8(74, 222, 128),
            warning: Color::rgb_u8(251, 191, 36),
            zode_purple: ZODE_PURPLE,
        }
    }
}

impl Default for ZodeTheme {
    fn default() -> Self {
        Self::light()
    }
}
