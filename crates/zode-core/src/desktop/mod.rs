//! Desktop control subsystem: non-multimodal control of native apps via the
//! OS accessibility tree (macOS AX first, Windows UIA later), with Electron
//! upgradable to CDP. Mirrors the `browser/` four-layer shape but diverges
//! for actor threading, generation-bound identity, and permission-laddered
//! reads (see docs/superpowers/specs/2026-07-11-desktop-control-design.md).

pub mod actor;
#[cfg(target_os = "linux")]
pub mod atspi;
#[cfg(target_os = "macos")]
pub mod ax;
pub mod backend;
pub mod cdp;
#[path = "esc-watch.rs"]
pub mod esc_watch;
pub mod gate;
#[cfg(test)]
pub mod mock;
pub mod overlay;
pub mod screenshot;
pub mod session;
pub mod tools;
#[cfg(windows)]
pub mod uia;

pub use backend::{
    AppId, AppInfo, AppLaunchId, DesktopBackend, DesktopBackendFactory, DesktopError,
    ElementActionKind, ElementRef, Screenshot, SnapshotResult, WindowId, WindowInfo,
};

/// The platform backend factory for this build: macOS → AX (with the optional
/// ghost-cursor overlay sink), Windows → UIA, Linux → AT-SPI2, else a graceful
/// `Unsupported` fallback.
pub fn platform_factory(
    cfg: &crate::config::DesktopConfig,
) -> std::sync::Arc<dyn DesktopBackendFactory> {
    #[cfg(target_os = "macos")]
    {
        std::sync::Arc::new(ax::AxFactory {
            overlay: overlay::global(cfg),
        })
    }
    #[cfg(windows)]
    {
        let _ = cfg;
        std::sync::Arc::new(uia::UiaFactory)
    }
    #[cfg(target_os = "linux")]
    {
        let _ = cfg;
        std::sync::Arc::new(atspi::AtspiFactory)
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = cfg;
        std::sync::Arc::new(backend::UnsupportedDesktopFactory)
    }
}
