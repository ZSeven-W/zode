//! zode-tui — ratatui-based terminal chrome for Zode.
//!
//! Implemented from Phase 04 onward. This crate consumes
//! `agent::stream::Event` streams from `zode_core::ZodeEngine` and
//! renders them; it never talks to providers directly.

/// Placeholder until Phase 04 lands `TuiApp::run`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
