//! Live macOS TCC permission status for computer-use, queried on demand by
//! the desktop settings page (`SettingsCategory::ComputerUse`). A thin
//! wrapper over `zode_core::computer::permission_status()` so the app
//! crate's settings loader stays symmetric with its other `zode_core`-backed
//! refreshers (config/browser/hooks/git/worktree facts) rather than reaching
//! into `zode_core::computer` directly.

/// One TCC permission's live grant state. `Unsupported` covers non-macOS
/// builds, where computer-use has no real backend (see
/// `zode_core::computer::UnsupportedBackend`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerPermissionState {
    Granted,
    NotGranted,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputerPermissionStatus {
    pub accessibility: ComputerPermissionState,
    pub screen_recording: ComputerPermissionState,
}

/// Queries the live Accessibility + Screen Recording TCC grant state. Never
/// prompts the user (matches `zode_core::computer::macos::permissions`'s own
/// no-prompt contract) — the desktop Settings page is the only UI surface
/// responsible for guiding the user to System Settings.
pub fn computer_permission_status() -> ComputerPermissionStatus {
    let status = zode_core::computer::permission_status();
    ComputerPermissionStatus {
        accessibility: map_state(status.accessibility),
        screen_recording: map_state(status.screen_recording),
    }
}

fn map_state(state: zode_core::computer::PermissionState) -> ComputerPermissionState {
    match state {
        zode_core::computer::PermissionState::Granted => ComputerPermissionState::Granted,
        zode_core::computer::PermissionState::NotGranted => ComputerPermissionState::NotGranted,
        zode_core::computer::PermissionState::Unsupported => ComputerPermissionState::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_status_does_not_panic() {
        let status = computer_permission_status();
        let _ = status.accessibility;
        let _ = status.screen_recording;
    }
}
