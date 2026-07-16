//! Desktop control subsystem: non-multimodal control of native apps via the
//! OS accessibility tree (macOS AX first, Windows UIA later), with Electron
//! upgradable to CDP. Mirrors the `browser/` four-layer shape but diverges
//! for actor threading, generation-bound identity, and permission-laddered
//! reads (see docs/superpowers/specs/2026-07-11-desktop-control-design.md).

pub mod backend;
#[cfg(test)]
pub mod mock;

pub use backend::{
    AppId, AppInfo, AppLaunchId, DesktopBackend, DesktopError, ElementActionKind, ElementRef,
    Screenshot, SnapshotResult, WindowId, WindowInfo,
};
