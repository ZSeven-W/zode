//! Desktop control subsystem: non-multimodal control of native apps via the
//! OS accessibility tree (macOS AX first, Windows UIA later), with Electron
//! upgradable to CDP. Mirrors the `browser/` four-layer shape but diverges
//! for actor threading, generation-bound identity, and permission-laddered
//! reads (see docs/superpowers/specs/2026-07-11-desktop-control-design.md).

pub mod actor;
pub mod backend;
pub mod gate;
#[cfg(test)]
pub mod mock;
pub mod screenshot;
pub mod session;
pub mod tools;

pub use backend::{
    AppId, AppInfo, AppLaunchId, DesktopBackend, DesktopBackendFactory, DesktopError,
    ElementActionKind, ElementRef, Screenshot, SnapshotResult, WindowId, WindowInfo,
};

/// The platform backend factory for this build. Every platform currently
/// returns the graceful `Unsupported` fallback; Task 12 flips macOS to the
/// real AX factory (`ax::AxFactory`).
pub fn platform_factory() -> std::sync::Arc<dyn DesktopBackendFactory> {
    std::sync::Arc::new(backend::UnsupportedDesktopFactory)
}
