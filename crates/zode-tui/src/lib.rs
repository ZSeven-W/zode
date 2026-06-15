//! zode-tui — ratatui terminal chrome for Zode. Consumes
//! `zode_core::ZodeEngine` event streams; never talks to providers.

pub mod app;
pub mod event;
pub mod keymap;
pub mod tab;
pub mod theme;
pub mod ui;

pub use app::{TuiApp, UiConfig};

/// Short alias for the i18n lookup, ergonomic in dense render code:
/// `tr("Settings")` ≡ `zode_core::i18n::t("Settings")`.
pub fn tr(s: &'static str) -> &'static str {
    zode_core::i18n::t(s)
}

/// Display prefix for the primary chord modifier: `Cmd+` on macOS, `Ctrl+`
/// elsewhere. Used to render key hints (replace `"Ctrl+"` in label strings).
///
/// We deliberately spell the macOS modifier `Cmd+` rather than the `⌘` glyph:
/// `⌘` (U+2318) is East-Asian-Width *Ambiguous*, so `unicode-width` (and thus
/// ratatui's layout) counts it as 1 column while a CJK-configured terminal
/// renders it 2 columns wide. That 1-column drift makes every following glyph
/// overlap in fixed-width chrome (status bar, help columns). `Cmd+` is plain
/// ASCII, so its rendered width always matches the measured width.
pub fn primary_key_prefix() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd+"
    } else {
        "Ctrl+"
    }
}
