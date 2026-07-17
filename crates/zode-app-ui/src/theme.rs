use jian_widgets::{Color, Painter, Rect, Tokens};
use zode_app_model::{SystemTheme, ThemePreference, UiPreferences};

pub const ZODE_PURPLE: Color = Color::rgb_u8(124, 58, 237);

/// Codex's blue interactive accent (`--color-accent-blue` /
/// `--color-text-accent` at the `--blue-300` step), identical in both
/// themes. Distinct from `tokens.primary`/`tokens.ring` (zode's purple
/// brand color), which the design-reference audit intentionally keeps as a
/// brand departure rather than a parity target - this token exists for
/// call sites that want the Codex blue specifically (links, informational
/// accents), not for restyling zode's primary interactive color.
pub const ACCENT_BLUE: Color = Color::rgb_u8(0x33, 0x9c, 0xff);

/// Corner-radius scale lifted from the Codex design reference
/// (`docs/design/codex-reference.md`, table A4). `Tokens::radius` (the
/// single shadcn-style flat radius already consumed throughout the crate)
/// remains an alias for `sm`, so existing call sites keep compiling and
/// rendering unchanged; new call sites should reach for a named step here
/// instead of a bespoke literal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadiusScale {
    pub xs2: f32,
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
    pub xl2: f32,
    pub xl3: f32,
    pub xl4: f32,
}

impl RadiusScale {
    pub const fn codex() -> Self {
        Self {
            xs2: 2.0,
            xs: 4.0,
            sm: 6.0,
            md: 8.0,
            lg: 10.0,
            xl: 12.0,
            xl2: 16.0,
            xl3: 20.0,
            xl4: 24.0,
        }
    }
}

/// One layer of a multi-layer drop shadow: how far the blur spreads and
/// what color/opacity it paints at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowLayer {
    pub blur: f32,
    pub color: Color,
}

/// A named elevation level: the dual-shadow composite the Codex reference
/// calls `--elevation-prominent` (`docs/design/codex-reference.md`, table
/// A5) - used behind floating surfaces (popovers, menus, cards). Paired
/// with [`paint_elevated_surface`], which is the single place that turns
/// this token into paint calls, replacing the hand-rolled
/// offset/blur/alpha combinations that used to be copied into every
/// floating-surface widget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elevation {
    /// Tight, low-opacity contact shadow.
    pub shadow: ShadowLayer,
    /// Soft, wider ambient glow.
    pub glow: ShadowLayer,
}

impl Elevation {
    pub const fn prominent() -> Self {
        Self {
            shadow: ShadowLayer {
                blur: 7.5,
                color: Color::rgba_u8(0, 0, 0, 0.04),
            },
            glow: ShadowLayer {
                blur: 20.0,
                color: Color::rgba_u8(0, 0, 0, 0.05),
            },
        }
    }
}

/// Paints the shared "floating surface" shadow convention
/// ([`Elevation::prominent`]) behind `rect`. This replaces the hand-rolled
/// `painter.fill_drop_shadow(rect.origin.y + N, ...)` calls that used to be
/// copied into each popover/menu/card widget with inconsistent manual
/// offsets (1px, 2px, 3px) stacked on top of the backend's own baked-in
/// shadow offset - every caller now gets the same two-layer shadow at the
/// surface's true position. Callers still paint their own fill and border
/// on top (via `Popover::paint`, `fill_round_rect` + `stroke_round_rect`,
/// etc.) - this helper only paints the shadow.
pub fn paint_elevated_surface(
    painter: &mut dyn Painter,
    rect: Rect,
    radius: f32,
    theme: &ZodeTheme,
) {
    let elevation = theme.elevation_prominent;
    painter.fill_drop_shadow(rect, radius, elevation.shadow.blur, elevation.shadow.color);
    painter.fill_drop_shadow(rect, radius, elevation.glow.blur, elevation.glow.color);
}

/// Fixed-class colors for fenced-code syntax highlighting (see
/// `widgets/transcript/code-syntax.rs`). Deliberately built from the
/// theme's existing semantic colors (`zode_purple`/`success`/`warning`/
/// `tokens.muted_foreground`) rather than new literals, so light/dark
/// contrast stays whatever those tokens were already tuned to. Plain
/// identifiers keep using `tokens.foreground` directly - the same color a
/// code block used before highlighting existed - so there is no separate
/// "plain" field here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodeSyntaxColors {
    pub keyword: Color,
    pub string: Color,
    pub comment: Color,
    pub number: Color,
    pub punctuation: Color,
}

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
    /// Codex's 9-step corner-radius scale. `tokens.radius` remains the flat
    /// `sm` alias existing call sites already consume.
    pub radii: RadiusScale,
    /// Shared floating-surface elevation (see [`paint_elevated_surface`]).
    pub elevation_prominent: Elevation,
    /// Codex's blue interactive accent (`--color-accent-blue`). See
    /// [`ACCENT_BLUE`] for why this is distinct from `tokens.primary`.
    pub accent_blue: Color,
    /// Fenced-code-block syntax highlighting palette. See [`CodeSyntaxColors`].
    pub code_syntax: CodeSyntaxColors,
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
            // `tokens.border` is already `foreground.with_alpha(0.08)` -
            // `with_alpha` replaces rather than multiplies alpha, so calling
            // it again here would discard that 8% and recomposite the same
            // near-black `foreground` RGB at 72% alpha, rendering a solid
            // dark line instead of a subtle divider. Compute straight from
            // `foreground` at a modest alpha that keeps the same soft
            // weight the old flat `#e5e5e5`-based border had at 0.72.
            self.tokens.foreground.with_alpha(0.16)
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
                theme.code_syntax.comment = theme.tokens.muted_foreground;
                theme.code_syntax.punctuation = theme.tokens.muted_foreground;
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
                theme.code_syntax.keyword = theme.zode_purple;
                theme.code_syntax.comment = theme.tokens.muted_foreground;
                theme.code_syntax.punctuation = theme.tokens.muted_foreground;
                theme
            }
        }
    }
    pub const fn light() -> Self {
        let mut tokens = Tokens::light();
        tokens.primary = ZODE_PURPLE;
        tokens.ring = ZODE_PURPLE;
        tokens.row_selected_primary = Color::rgba_u8(124, 58, 237, 0.14);
        // Codex reference primary text is `#1a1c1f` (`--color-text-foreground`),
        // darker/warmer than the shadcn-default near-black this palette was
        // copied from. Every foreground that mirrors body ink on a
        // near-white surface moves with it so text stays visually
        // consistent across card/popover/secondary/accent surfaces.
        let foreground = Color::rgb_u8(0x1a, 0x1c, 0x1f);
        tokens.foreground = foreground;
        tokens.card_foreground = foreground;
        tokens.popover_foreground = foreground;
        tokens.secondary_foreground = foreground;
        tokens.accent_foreground = foreground;
        // Codex borders are an 8%-opacity blend of the foreground ink over
        // whatever surface they sit on (`--color-border`), not a flat gray -
        // computing it from `foreground` keeps the two in lockstep and
        // renders correctly via ordinary alpha compositing.
        tokens.border = foreground.with_alpha(0.08);
        Self {
            mode: ThemeMode::Light,
            sidebar_material: true,
            native_sidebar_material: false,
            tokens,
            // Softbuffer presents opaque RGB words, so use the pre-composited
            // visual equivalent of the macOS light sidebar material. Keeping
            // this opaque also prevents nested shell painters from darkening
            // the rail when they repaint the same background. This is
            // deliberately close to but not identical to the Codex
            // reference's flat `#f9f9f9` sidebar-under token - it already
            // encodes the macOS vibrancy equivalent and was left alone.
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
            radii: RadiusScale::codex(),
            elevation_prominent: Elevation::prominent(),
            accent_blue: ACCENT_BLUE,
            code_syntax: CodeSyntaxColors {
                keyword: ZODE_PURPLE,
                string: Color::rgb_u8(22, 163, 74),
                comment: tokens.muted_foreground,
                number: Color::rgb_u8(217, 119, 6),
                punctuation: tokens.muted_foreground,
            },
        }
    }

    pub const fn dark() -> Self {
        let mut tokens = Tokens::dark();
        tokens.primary = ZODE_PURPLE;
        tokens.ring = ZODE_PURPLE;
        tokens.row_selected_primary = Color::rgba_u8(124, 58, 237, 0.22);
        // Codex's dark main surface (`--color-background-surface`) is
        // `#181818`; the shadcn-default `#0a0a0a` this palette was copied
        // from renders noticeably darker with no separate "surface-under"
        // step behind it.
        tokens.background = Color::rgb_u8(0x18, 0x18, 0x18);
        // Same 8%-opacity-over-foreground border model as light mode; dark
        // foreground is already near-white, so this lands within a hair of
        // the reference's literal `color-mix(white 8%)` value.
        tokens.border = tokens.foreground.with_alpha(0.08);
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
            radii: RadiusScale::codex(),
            elevation_prominent: Elevation::prominent(),
            accent_blue: ACCENT_BLUE,
            code_syntax: CodeSyntaxColors {
                keyword: ZODE_PURPLE,
                string: Color::rgb_u8(74, 222, 128),
                comment: tokens.muted_foreground,
                number: Color::rgb_u8(251, 191, 36),
                punctuation: tokens.muted_foreground,
            },
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
    use super::{paint_elevated_surface, Elevation, RadiusScale, ZodeTheme, ACCENT_BLUE};
    use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};

    #[derive(Default)]
    struct RecordingPainter {
        shadows: Vec<(Rect, f32, f32, Color)>,
    }

    impl Painter for RecordingPainter {
        fn begin_frame(&mut self) {}
        fn end_frame(&mut self) {}
        fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
        fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
        fn draw_text(&mut self, _layout: &TextLayout, _origin: Point2D) {}
        fn clip_rect(&mut self, _rect: Rect) {}
        fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
        fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color) {}
        fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
        fn stroke_svg_path(
            &mut self,
            _d: &str,
            _top_left: Point2D,
            _size: f32,
            _color: Color,
            _width: f32,
        ) {
        }
        fn fill_drop_shadow(&mut self, rect: Rect, radius: f32, blur: f32, color: Color) {
            self.shadows.push((rect, radius, blur, color));
        }
        fn save(&mut self) {}
        fn restore(&mut self) {}
        fn translate(&mut self, _offset: Point2D) {}
        fn resize(&mut self, _width: u32, _height: u32) {}
        fn dpi_scale(&self) -> f32 {
            1.0
        }
    }

    #[test]
    fn radius_scale_keeps_the_flat_tokens_radius_as_its_sm_alias() {
        let light = ZodeTheme::light();
        let dark = ZodeTheme::dark();
        assert_eq!(light.radii, RadiusScale::codex());
        assert_eq!(dark.radii, RadiusScale::codex());
        assert_eq!(light.tokens.radius, light.radii.sm);
        assert_eq!(dark.tokens.radius, dark.radii.sm);
    }

    #[test]
    fn border_is_an_eight_percent_alpha_blend_of_the_foreground() {
        let light = ZodeTheme::light();
        let dark = ZodeTheme::dark();
        assert_eq!(
            light.tokens.border,
            light.tokens.foreground.with_alpha(0.08)
        );
        assert_eq!(dark.tokens.border, dark.tokens.foreground.with_alpha(0.08));
    }

    #[test]
    fn light_primary_text_and_dark_surface_match_the_codex_reference() {
        let light = ZodeTheme::light();
        let dark = ZodeTheme::dark();
        assert_eq!(light.tokens.foreground, Color::rgb_u8(0x1a, 0x1c, 0x1f));
        assert_eq!(light.tokens.card_foreground, light.tokens.foreground);
        assert_eq!(light.tokens.popover_foreground, light.tokens.foreground);
        assert_eq!(light.tokens.secondary_foreground, light.tokens.foreground);
        assert_eq!(light.tokens.accent_foreground, light.tokens.foreground);
        assert_eq!(dark.tokens.background, Color::rgb_u8(0x18, 0x18, 0x18));
    }

    #[test]
    fn accent_blue_is_identical_across_themes_and_distinct_from_the_brand_purple() {
        let light = ZodeTheme::light();
        let dark = ZodeTheme::dark();
        assert_eq!(light.accent_blue, ACCENT_BLUE);
        assert_eq!(dark.accent_blue, ACCENT_BLUE);
        assert_ne!(light.accent_blue, light.zode_purple);
    }

    #[test]
    fn paint_elevated_surface_paints_the_two_prominent_shadow_layers_at_the_surfaces_rect() {
        let theme = ZodeTheme::light();
        let mut painter = RecordingPainter::default();
        let rect = Rect::xywh(4.0, 8.0, 200.0, 100.0);

        paint_elevated_surface(&mut painter, rect, 12.0, &theme);

        let expected = Elevation::prominent();
        assert_eq!(
            painter.shadows,
            vec![
                (rect, 12.0, expected.shadow.blur, expected.shadow.color),
                (rect, 12.0, expected.glow.blur, expected.glow.color),
            ]
        );
    }

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
            dark.tokens.foreground.with_alpha(0.16)
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
