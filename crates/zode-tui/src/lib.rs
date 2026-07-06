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

/// Display prefix for application shortcuts. Zode documents Control chords
/// consistently across platforms; the key handler may accept additional
/// terminal-specific aliases, but the UI should not advertise them.
pub fn primary_key_prefix() -> &'static str {
    "Ctrl+"
}

#[cfg(test)]
mod tests {
    #[test]
    fn primary_key_prefix_uses_control_label() {
        assert_eq!(super::primary_key_prefix(), "Ctrl+");
    }
}
