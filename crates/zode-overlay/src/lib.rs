//! zode-overlay: ghost-cursor overlay helper for zode desktop automation.
//! The bin (`main.rs`) is macOS-only; this lib holds the platform-neutral
//! pieces (wire protocol, cursor motion math) so they test everywhere.

pub mod proto;
