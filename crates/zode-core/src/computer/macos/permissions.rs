//! macOS TCC (privacy) permission checks. Neither call prompts the user —
//! the doc explicitly treats "not yet granted" as a retryable tool result,
//! not a hard error, so we never pass `kAXTrustedCheckOptionPrompt` here;
//! the desktop app's Settings page (`SettingsCategory::ComputerUse`) is
//! responsible for guiding the user to System Settings.

use objc2_application_services::AXIsProcessTrusted;
use objc2_core_graphics::CGPreflightScreenCaptureAccess;

/// Whether zode is a trusted Accessibility client (System Settings →
/// Privacy & Security → Accessibility).
pub fn accessibility_trusted() -> bool {
    // SAFETY: `AXIsProcessTrusted` takes no arguments and only reads process
    // state; safe to call from any thread.
    unsafe { AXIsProcessTrusted() }
}

/// Whether zode has been granted Screen Recording access (System Settings →
/// Privacy & Security → Screen Recording). Required for
/// `SCScreenshotManager` captures.
pub fn screen_recording_trusted() -> bool {
    CGPreflightScreenCaptureAccess()
}

#[cfg(test)]
mod tests {
    use super::*;

    // These just assert the FFI calls don't panic/crash; the actual trust
    // state depends on the CI/dev machine's TCC database and is not
    // asserted either way (see the crate-level "not CI-verified" note).
    #[test]
    fn permission_checks_do_not_panic() {
        let _ = accessibility_trusted();
        let _ = screen_recording_trusted();
    }
}
