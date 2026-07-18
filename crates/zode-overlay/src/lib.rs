//! zode-overlay: ghost-cursor overlay helper for zode desktop automation.
//! The overlay windows (`app`, `draw`) are macOS-only; the platform-neutral
//! pieces (wire protocol, cursor motion math) test everywhere.

#[cfg(target_os = "macos")]
pub mod app;
#[cfg(target_os = "macos")]
pub mod draw;
pub mod motion;
pub mod proto;
