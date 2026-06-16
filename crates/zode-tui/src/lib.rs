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

/// Display prefix for the primary chord modifier: `Cmd+` on macOS, `Ctrl+` on
/// Windows/Linux — matching each platform's native convention. The key handler
/// (`is_primary_mod`) accepts the Command (⌘ / SUPER) key on macOS and Ctrl
/// elsewhere (it also still accepts Ctrl on macOS as a fallback for terminals
/// that don't forward ⌘).
///
/// We spell the macOS modifier `Cmd+` rather than the `⌘` glyph: `⌘` (U+2318)
/// is East-Asian-Width *Ambiguous*, so `unicode-width` counts it as 1 column
/// while a CJK-configured terminal renders it 2 — that drift overlaps following
/// glyphs in fixed-width chrome. `Cmd+` is plain ASCII, width always correct.
pub fn primary_key_prefix() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd+"
    } else {
        "Ctrl+"
    }
}
