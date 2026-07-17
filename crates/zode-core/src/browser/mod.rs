//! Built-in browser control: backend trait, managed (chromiumoxide)
//! implementation, process-wide session, and the browser_* tools.
//! See docs/superpowers/specs/2026-07-03-zode-browser-control-design.md.

pub mod backend;
pub mod bridge;
mod executable;
#[path = "file-input.rs"]
mod file_input;
pub mod gate;
pub mod managed;
#[path = "managed-downloads.rs"]
mod managed_downloads;
pub mod session;
pub(crate) mod snapshot_js;
pub mod tools;
pub mod upload;

pub use backend::{
    BrowserBackend, BrowserError, BrowserTarget, ClickTarget, ConsoleEntry, DownloadEntry,
    DownloadStatus, NetworkEntry, ScreencastFrame, Screenshot, TabInfo,
};
pub use managed::ManagedFactory;
pub use session::{BackendFactory, BackendLease, BrowserSession};
pub use tools::{
    BrowserActTool, BrowserEvalTool, BrowserReadTool, BrowserTabsTool, BrowserToolDeps,
};
pub use upload::BrowserUploadTool;
