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
    mode: ThemeMode,
    sidebar_material: bool,
    native_sidebar_material: bool,
    pub tokens: Tokens,
    pub sidebar: Color,
    pub sidebar_foreground: Color,
    pub sidebar_muted_foreground: Color,
    pub sidebar_disabled_foreground: Color,
    pub sidebar_row_selected: Color,
    pub sidebar_row_hover: Color,
    pub user_bubble: Color,
    pub success: Color,
    pub warning: Color,
    pub composer_permission: Color,
    pub zode_purple: Color,
}

impl ZodeTheme {
    pub(crate) const fn mode(&self) -> ThemeMode {
        self.mode
    }

    pub const fn uses_sidebar_material(&self) -> bool {
        self.sidebar_material
    }

    pub const fn uses_native_sidebar_material(&self) -> bool {
        self.native_sidebar_material
    }

    pub fn with_native_sidebar_material(mut self) -> Self {
        if self.sidebar_material {
            self.native_sidebar_material = true;
            self.sidebar = Color::TRANSPARENT;
            // Native macOS sidebar material supplies the luminance variation.
            // Keep semantic ink and interaction layers translucent so they
            // preserve that material instead of reintroducing flat fills.
            self.sidebar_foreground = Color::rgb_u8(52, 56, 58);
            self.sidebar_muted_foreground = Color::rgb_u8(142, 143, 144);
            self.sidebar_disabled_foreground = Color::rgb_u8(171, 171, 173);
            self.sidebar_row_selected = Color::BLACK.with_alpha(0.05);
            self.sidebar_row_hover = Color::BLACK.with_alpha(0.025);
        }
        self
    }

    pub fn sidebar_footer_divider(&self) -> Color {
        if self.sidebar_material {
            Color::BLACK.with_alpha(0.09)
        } else {
            self.tokens.border.with_alpha(0.72)
        }
    }

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
                theme.sidebar_material = false;
                theme.sidebar = Color::rgb_u8(235, 235, 231);
                theme.sidebar_foreground = Color::BLACK;
                theme.sidebar_muted_foreground = Color::rgb_u8(55, 55, 53);
                theme.sidebar_disabled_foreground = Color::rgb_u8(80, 80, 78);
                theme.sidebar_row_selected = Color::rgb_u8(208, 208, 204);
                theme.sidebar_row_hover = Color::rgb_u8(222, 222, 218);
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
                theme.sidebar_muted_foreground = Color::rgb_u8(224, 224, 222);
                theme.sidebar_disabled_foreground = Color::rgb_u8(180, 180, 178);
                theme.sidebar_row_selected = Color::rgb_u8(65, 65, 65);
                theme.sidebar_row_hover = Color::rgb_u8(44, 44, 44);
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
            mode: ThemeMode::Light,
            sidebar_material: true,
            native_sidebar_material: false,
            tokens,
            // Softbuffer presents opaque RGB words, so use the pre-composited
            // visual equivalent of the macOS light sidebar material. Keeping
            // this opaque also prevents nested shell painters from darkening
            // the rail when they repaint the same background.
            sidebar: Color::rgb_u8(245, 246, 246),
            sidebar_foreground: Color::rgb_u8(58, 61, 63),
            sidebar_muted_foreground: Color::rgb_u8(164, 165, 165),
            sidebar_disabled_foreground: Color::rgb_u8(192, 192, 193),
            sidebar_row_selected: Color::rgb_u8(233, 234, 235),
            sidebar_row_hover: Color::rgb_u8(239, 240, 240),
            user_bubble: Color::rgb_u8(245, 245, 246),
            success: Color::rgb_u8(22, 163, 74),
            warning: Color::rgb_u8(217, 119, 6),
            composer_permission: Color::rgb_u8(255, 79, 10),
            zode_purple: ZODE_PURPLE,
        }
    }

    pub const fn dark() -> Self {
        let mut tokens = Tokens::dark();
        tokens.primary = ZODE_PURPLE;
        tokens.ring = ZODE_PURPLE;
        tokens.row_selected_primary = Color::rgba_u8(124, 58, 237, 0.22);
        Self {
            mode: ThemeMode::Dark,
            sidebar_material: false,
            native_sidebar_material: false,
            tokens,
            sidebar: Color::rgb_u8(21, 21, 22),
            sidebar_foreground: Color::rgb_u8(232, 232, 230),
            sidebar_muted_foreground: Color::rgb_u8(160, 160, 162),
            sidebar_disabled_foreground: Color::rgb_u8(113, 113, 116),
            sidebar_row_selected: Color::rgb_u8(41, 41, 43),
            sidebar_row_hover: Color::rgb_u8(32, 32, 34),
            user_bubble: Color::rgb_u8(42, 42, 44),
            success: Color::rgb_u8(74, 222, 128),
            warning: Color::rgb_u8(251, 191, 36),
            composer_permission: Color::rgb_u8(255, 138, 76),
            zode_purple: ZODE_PURPLE,
        }
    }
}

impl Default for ZodeTheme {
    fn default() -> Self {
        Self::light()
    }
}

#[cfg(test)]
mod tests {
    use super::ZodeTheme;
    use jian_widgets::Color;

    #[test]
    fn composer_permission_uses_a_dedicated_orange_for_each_color_scheme() {
        let light = ZodeTheme::light();
        let dark = ZodeTheme::dark();

        assert_eq!(light.composer_permission, Color::rgb_u8(255, 79, 10));
        assert_eq!(dark.composer_permission, Color::rgb_u8(255, 138, 76));
        assert_ne!(light.composer_permission, light.warning);
        assert_ne!(dark.composer_permission, dark.warning);
    }

    #[test]
    fn light_sidebar_uses_the_precomposited_reference_material_color() {
        let theme = ZodeTheme::light();
        let material = theme.sidebar;

        assert_eq!(material, Color::rgb_u8(245, 246, 246));
        assert_eq!(material.a, 1.0, "Softbuffer requires an opaque final color");
        assert_eq!(theme.sidebar_foreground, Color::rgb_u8(58, 61, 63));
        assert_eq!(theme.sidebar_muted_foreground, Color::rgb_u8(164, 165, 165));
        assert_eq!(
            theme.sidebar_disabled_foreground,
            Color::rgb_u8(192, 192, 193)
        );
        assert_eq!(theme.sidebar_row_selected, Color::rgb_u8(233, 234, 235));
        assert_eq!(theme.sidebar_row_hover, Color::rgb_u8(239, 240, 240));
        assert!(theme.uses_sidebar_material());
        assert!(!ZodeTheme::dark().uses_sidebar_material());
        assert!(!ZodeTheme::high_contrast(super::ThemeMode::Light).uses_sidebar_material());
        assert!(!ZodeTheme::high_contrast(super::ThemeMode::Dark).uses_sidebar_material());
    }

    #[test]
    fn native_material_variant_only_makes_supported_sidebar_transparent() {
        let light = ZodeTheme::light().with_native_sidebar_material();
        let dark = ZodeTheme::dark().with_native_sidebar_material();

        assert!(light.uses_native_sidebar_material());
        assert_eq!(light.sidebar, Color::TRANSPARENT);
        assert_eq!(light.sidebar_foreground, Color::rgb_u8(52, 56, 58));
        assert_eq!(light.sidebar_muted_foreground, Color::rgb_u8(142, 143, 144));
        assert_eq!(
            light.sidebar_disabled_foreground,
            Color::rgb_u8(171, 171, 173)
        );
        assert_eq!(light.sidebar_row_selected, Color::BLACK.with_alpha(0.05));
        assert_eq!(light.sidebar_row_hover, Color::BLACK.with_alpha(0.025));
        assert!(!dark.uses_native_sidebar_material());
        assert_eq!(dark.sidebar, ZodeTheme::dark().sidebar);
    }

    #[test]
    fn material_footer_divider_preserves_the_reference_hairline() {
        let flat = ZodeTheme::light();
        let native = flat.with_native_sidebar_material();
        let dark = ZodeTheme::dark();

        assert_eq!(flat.sidebar_footer_divider(), Color::BLACK.with_alpha(0.09));
        assert_eq!(
            native.sidebar_footer_divider(),
            Color::BLACK.with_alpha(0.09)
        );
        assert_eq!(
            dark.sidebar_footer_divider(),
            dark.tokens.border.with_alpha(0.72)
        );
    }

    #[test]
    fn sidebar_semantic_text_colors_are_scheme_specific() {
        let light_high_contrast = ZodeTheme::high_contrast(super::ThemeMode::Light);
        let dark = ZodeTheme::dark();
        let dark_high_contrast = ZodeTheme::high_contrast(super::ThemeMode::Dark);

        assert_eq!(dark.sidebar_muted_foreground, Color::rgb_u8(160, 160, 162));
        assert_eq!(
            dark.sidebar_disabled_foreground,
            Color::rgb_u8(113, 113, 116)
        );
        assert_eq!(
            light_high_contrast.sidebar_muted_foreground,
            Color::rgb_u8(55, 55, 53)
        );
        assert_eq!(
            light_high_contrast.sidebar_disabled_foreground,
            Color::rgb_u8(80, 80, 78)
        );
        assert_eq!(
            dark_high_contrast.sidebar_muted_foreground,
            Color::rgb_u8(224, 224, 222)
        );
        assert_eq!(
            dark_high_contrast.sidebar_disabled_foreground,
            Color::rgb_u8(180, 180, 178)
        );
    }

    #[test]
    fn sidebar_interaction_colors_are_scheme_specific() {
        let light = ZodeTheme::light();
        let dark = ZodeTheme::dark();
        let light_high_contrast = ZodeTheme::high_contrast(super::ThemeMode::Light);
        let dark_high_contrast = ZodeTheme::high_contrast(super::ThemeMode::Dark);

        assert_eq!(light.sidebar_row_selected, Color::rgb_u8(233, 234, 235));
        assert_eq!(light.sidebar_row_hover, Color::rgb_u8(239, 240, 240));
        assert_eq!(dark.sidebar_row_selected, Color::rgb_u8(41, 41, 43));
        assert_eq!(dark.sidebar_row_hover, Color::rgb_u8(32, 32, 34));
        assert_eq!(
            light_high_contrast.sidebar_row_selected,
            Color::rgb_u8(208, 208, 204)
        );
        assert_eq!(
            light_high_contrast.sidebar_row_hover,
            Color::rgb_u8(222, 222, 218)
        );
        assert_eq!(
            dark_high_contrast.sidebar_row_selected,
            Color::rgb_u8(65, 65, 65)
        );
        assert_eq!(
            dark_high_contrast.sidebar_row_hover,
            Color::rgb_u8(44, 44, 44)
        );
    }
}
