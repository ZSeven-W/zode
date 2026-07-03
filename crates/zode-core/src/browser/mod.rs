//! Built-in browser control: backend trait, managed (chromiumoxide)
//! implementation, process-wide session, and the browser_* tools.
//! See docs/superpowers/specs/2026-07-03-zode-browser-control-design.md.

pub mod backend;
pub mod managed;
pub mod session;

pub use backend::{
    BrowserBackend, BrowserError, BrowserTarget, ClickTarget, ConsoleEntry, NetworkEntry,
    Screenshot, TabInfo,
};
pub use managed::ManagedFactory;
pub use session::{BackendFactory, BackendLease, BrowserSession};
