//! Real-desktop integration tests for the macOS computer-use backend.
//! Opt-in, macOS-only, and read-only by design:
//!   ZODE_COMPUTER_IT=1 cargo test -p zode-core --test computer_it -- --ignored
//!
//! Unlike `browser_it` (which drives an isolated, throwaway headless
//! Chrome), this backend acts on the REAL desktop — a developer running the
//! full suite locally should never have their actual mouse/keyboard
//! hijacked by an opt-in test they didn't specifically ask to run. So this
//! only exercises the read paths (`app_state`, `list_apps`, `screenshot`);
//! `click`/`type_text`/`key`/`scroll`/`drag` are covered by the mock-backed
//! unit tests in `src/computer/` and are NOT re-verified here against the
//! real OS — see the M1 report for how to manually verify them.
//!
//! Requires the Accessibility and Screen Recording TCC permissions to
//! already be granted to the test binary (typically the terminal/IDE that
//! launches `cargo test`) — otherwise every assertion here fails with a
//! `PermissionPending` error rather than a `panic`, which is itself a
//! useful manual signal that the TCC wiring in `permissions.rs` works.

#![cfg(target_os = "macos")]

use zode_core::computer::macos::MacosBackend;
use zode_core::computer::ComputerBackend;

fn skip_unless_enabled() -> bool {
    if std::env::var("ZODE_COMPUTER_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set ZODE_COMPUTER_IT=1");
        return true;
    }
    false
}

#[tokio::test]
#[ignore]
async fn app_state_reads_the_frontmost_app() {
    if skip_unless_enabled() {
        return;
    }
    let backend = MacosBackend::new();
    let state = backend
        .app_state(None)
        .await
        .expect("app_state (grant Accessibility permission to the test binary if this fails)");
    assert!(state.generation > 0);
    assert!(!state.app.is_empty());
}

#[tokio::test]
#[ignore]
async fn list_apps_includes_a_frontmost_entry() {
    if skip_unless_enabled() {
        return;
    }
    let backend = MacosBackend::new();
    let apps = backend.list_apps().await.expect("list_apps");
    assert!(!apps.is_empty());
    assert!(
        apps.iter().any(|a| a.frontmost),
        "expected exactly one frontmost app: {apps:?}"
    );
}

#[tokio::test]
#[ignore]
async fn screenshot_captures_the_main_display() {
    if skip_unless_enabled() {
        return;
    }
    let backend = MacosBackend::new();
    let shot = backend
        .screenshot()
        .await
        .expect("screenshot (grant Screen Recording permission to the test binary if this fails)");
    assert!(
        shot.bytes.len() > 1000,
        "screenshot too small: {}",
        shot.bytes.len()
    );
    assert_eq!(shot.media_type, "image/jpeg");
}

#[tokio::test]
#[ignore]
async fn stale_generation_is_rejected_against_real_state() {
    if skip_unless_enabled() {
        return;
    }
    let backend = MacosBackend::new();
    let first = backend.app_state(None).await.expect("first app_state");
    let second = backend.app_state(None).await.expect("second app_state");
    assert!(second.generation > first.generation);
    let err = backend
        .scroll(first.generation, 0.0, 0.0)
        .await
        .expect_err("acting on the superseded generation must be rejected");
    assert!(matches!(
        err,
        zode_core::computer::ComputerError::StaleGeneration { .. }
    ));
}
