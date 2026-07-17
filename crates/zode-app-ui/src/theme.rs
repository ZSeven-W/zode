use jian_widgets::{Color, Tokens};
use zode_app_model::{SystemTheme, ThemePreference, UiPreferences};

pub const ZODE_PURPLE: Color = Color::rgb_u8(124, 58, 237);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

pub fn resolve_theme(system: SystemTheme, preferences: &UiPreferences) -> ThemeMode {
    match preferences.theme {
        ThemePreference::Light => ThemeMode::Light,
        ThemePreference::Dark => ThemeMode::Dark,
        ThemePreference::System => match system {
            SystemTheme::Light => ThemeMode::Light,
            SystemTheme::Dark => ThemeMode::Dark,
        },
    }
}

pub fn animation_duration_ms(duration_ms: u64, preferences: &UiPreferences) -> u64 {
    if preferences.reduced_motion {
        0
    } else {
        duration_ms
    }
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
        let mode = resolve_theme(system, preferences);
        if preferences.high_contrast {
            return Self::high_contrast(mode);
        }
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        }
    }

    pub fn high_contrast(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => {
                let mut theme = Self::light();
                theme.tokens.background = Color::WHITE;
                theme.tokens.foreground = Color::BLACK;
                theme.tokens.card = Color::WHITE;
                theme.tokens.card_foreground = Color::BLACK;
                theme.tokens.popover = Color::WHITE;
                theme.tokens.popover_foreground = Color::BLACK;
                theme.tokens.muted = Color::rgb_u8(242, 242, 240);
                theme.tokens.muted_foreground = Color::rgb_u8(55, 55, 53);
                theme.tokens.accent = Color::rgb_u8(229, 229, 226);
                theme.tokens.accent_foreground = Color::BLACK;
                theme.tokens.secondary = Color::rgb_u8(229, 229, 226);
                theme.tokens.secondary_foreground = Color::BLACK;
                theme.tokens.destructive = Color::rgb_u8(153, 27, 27);
                theme.tokens.destructive_foreground = Color::WHITE;
                theme.tokens.border = Color::rgb_u8(80, 80, 78);
                theme.sidebar = Color::rgb_u8(235, 235, 231);
                theme.sidebar_foreground = Color::BLACK;
                theme
            }
            ThemeMode::Dark => {
                let mut theme = Self::dark();
                theme.tokens.background = Color::BLACK;
                theme.tokens.foreground = Color::WHITE;
                theme.tokens.card = Color::rgb_u8(12, 12, 12);
                theme.tokens.card_foreground = Color::WHITE;
                theme.tokens.popover = Color::rgb_u8(12, 12, 12);
                theme.tokens.popover_foreground = Color::WHITE;
                theme.tokens.muted = Color::rgb_u8(22, 22, 22);
                theme.tokens.muted_foreground = Color::rgb_u8(224, 224, 222);
                theme.tokens.accent = Color::rgb_u8(38, 38, 38);
                theme.tokens.accent_foreground = Color::WHITE;
                theme.tokens.secondary = Color::rgb_u8(38, 38, 38);
                theme.tokens.secondary_foreground = Color::WHITE;
                theme.tokens.destructive = Color::rgb_u8(248, 113, 113);
                theme.tokens.destructive_foreground = Color::BLACK;
                theme.tokens.border = Color::rgb_u8(180, 180, 178);
                theme.sidebar = Color::rgb_u8(18, 18, 18);
                theme.sidebar_foreground = Color::WHITE;
                theme.zode_purple = Color::rgb_u8(196, 181, 253);
                theme.tokens.primary = theme.zode_purple;
                theme.tokens.primary_foreground = Color::BLACK;
                theme.tokens.ring = theme.zode_purple;
                theme
            }
        }
    }
    pub const fn light() -> Self {
        let mut tokens = Tokens::light();
        tokens.primary = ZODE_PURPLE;
        tokens.ring = ZODE_PURPLE;
        tokens.row_selected_primary = Color::rgba_u8(124, 58, 237, 0.14);
        Self {
            tokens,
            sidebar: Color::rgb_u8(238, 237, 234),
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
