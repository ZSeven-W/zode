//! Built-in computer-use control: backend trait, macOS (AX/CGEvent/
//! ScreenCaptureKit) implementation, session holder, and the computer_*
//! tools. See docs/proposals/computer-use.md — M1 scope is macOS-only;
//! other platforms get [`UnsupportedBackend`] so the crate still builds and
//! the tool group still registers (returning a clear error instead of
//! silently vanishing), per the doc's "Windows/Linux: M1 不做；接口按 trait
//! 抽象留位" note.

pub mod backend;
pub mod gate;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod outline;
pub mod session;
pub mod tools;

pub use backend::{ActTarget, AppInfo, AppState, ComputerBackend, ComputerError, Screenshot};
pub use gate::{computer_gated, ComputerGateView};
pub use session::ComputerSession;
pub use tools::{ComputerActTool, ComputerReadTool, ComputerToolDeps};

use async_trait::async_trait;
use std::sync::Arc;

/// Placeholder backend for platforms without a real implementation yet
/// (M1 targets macOS only). Every call reports [`ComputerError::Unsupported`]
/// so the tools stay registered — and thus visible/toggleable via
/// `tools:computer` — instead of conditionally disappearing per platform.
#[derive(Debug, Default)]
pub struct UnsupportedBackend;

const UNSUPPORTED_MSG: &str =
    "computer-use is only supported on macOS in this build; the AX/CGEvent/ScreenCaptureKit \
     backend has not been implemented for this platform yet";

#[async_trait]
impl ComputerBackend for UnsupportedBackend {
    async fn app_state(&self, _app: Option<&str>) -> Result<AppState, ComputerError> {
        Err(ComputerError::Unsupported(UNSUPPORTED_MSG.into()))
    }
    async fn list_apps(&self) -> Result<Vec<AppInfo>, ComputerError> {
        Err(ComputerError::Unsupported(UNSUPPORTED_MSG.into()))
    }
    async fn screenshot(&self) -> Result<Screenshot, ComputerError> {
        Err(ComputerError::Unsupported(UNSUPPORTED_MSG.into()))
    }
    async fn click(&self, _generation: u64, _target: &ActTarget) -> Result<(), ComputerError> {
        Err(ComputerError::Unsupported(UNSUPPORTED_MSG.into()))
    }
    async fn type_text(&self, _generation: u64, _text: &str) -> Result<(), ComputerError> {
        Err(ComputerError::Unsupported(UNSUPPORTED_MSG.into()))
    }
    async fn set_value(
        &self,
        _generation: u64,
        _target: &ActTarget,
        _value: &str,
    ) -> Result<(), ComputerError> {
        Err(ComputerError::Unsupported(UNSUPPORTED_MSG.into()))
    }
    async fn key(&self, _generation: u64, _key: &str) -> Result<(), ComputerError> {
        Err(ComputerError::Unsupported(UNSUPPORTED_MSG.into()))
    }
    async fn scroll(&self, _generation: u64, _dx: f64, _dy: f64) -> Result<(), ComputerError> {
        Err(ComputerError::Unsupported(UNSUPPORTED_MSG.into()))
    }
    async fn drag(
        &self,
        _generation: u64,
        _from: &ActTarget,
        _to: &ActTarget,
    ) -> Result<(), ComputerError> {
        Err(ComputerError::Unsupported(UNSUPPORTED_MSG.into()))
    }
    async fn describe_target(&self, _generation: u64, _target: &ActTarget) -> Option<String> {
        None
    }
    async fn frontmost_app_name(&self) -> Option<String> {
        None
    }
}

/// Build the platform's computer-use backend: [`macos::MacosBackend`] on
/// macOS, [`UnsupportedBackend`] everywhere else. Real-backend correctness on
/// macOS is not exercised by CI (it needs a live desktop session with TCC
/// permissions granted) — see `tests/computer_it.rs`.
pub fn default_backend() -> Arc<dyn ComputerBackend> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::MacosBackend::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Arc::new(UnsupportedBackend)
    }
}

/// One TCC permission's live grant state, as read directly from the OS —
/// never cached, never prompts the user (same no-prompt contract as
/// `macos::permissions`). `Unsupported` covers non-macOS builds, where
/// computer-use has no real backend (see [`UnsupportedBackend`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    NotGranted,
    Unsupported,
}

/// Live Accessibility + Screen Recording TCC state. The desktop Settings
/// page (`SettingsCategory::ComputerUse`) is the only UI surface
/// responsible for guiding the user to System Settings when either is
/// `NotGranted` — this query itself never prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputerPermissionStatus {
    pub accessibility: PermissionState,
    pub screen_recording: PermissionState,
}

pub fn permission_status() -> ComputerPermissionStatus {
    #[cfg(target_os = "macos")]
    {
        ComputerPermissionStatus {
            accessibility: if macos::accessibility_trusted() {
                PermissionState::Granted
            } else {
                PermissionState::NotGranted
            },
            screen_recording: if macos::screen_recording_trusted() {
                PermissionState::Granted
            } else {
                PermissionState::NotGranted
            },
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        ComputerPermissionStatus {
            accessibility: PermissionState::Unsupported,
            screen_recording: PermissionState::Unsupported,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unsupported_backend_reports_unsupported() {
        let b = UnsupportedBackend;
        let err = b.app_state(None).await.unwrap_err();
        assert!(matches!(err, ComputerError::Unsupported(_)));
        assert!(b.describe_target(0, &ActTarget::Element(1)).await.is_none());
        assert!(b.frontmost_app_name().await.is_none());
    }

    #[test]
    fn permission_status_does_not_panic() {
        let status = permission_status();
        let _ = status.accessibility;
        let _ = status.screen_recording;
    }
}
