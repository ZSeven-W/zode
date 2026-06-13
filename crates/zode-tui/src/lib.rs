//! zode-tui — ratatui terminal chrome for Zode. Consumes
//! `zode_core::ZodeEngine` event streams; never talks to providers.

pub mod app;
pub mod event;
pub mod keymap;
pub mod tab;
pub mod theme;
pub mod ui;

pub use app::{TuiApp, UiConfig};
