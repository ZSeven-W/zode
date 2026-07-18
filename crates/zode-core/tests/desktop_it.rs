//! Opt-in macOS AX end-to-end tests. Require a real TCC-authorized desktop and
//! at least one on-screen app; ignored by default.
//!
//! Run with:
//!   ZODE_DESKTOP_IT=1 cargo test -p zode-core --test desktop_it -- --ignored

#![cfg(target_os = "macos")]

use zode_core::desktop::backend::{AppId, WindowId};
use zode_core::desktop::platform_factory;

fn enabled() -> bool {
    std::env::var("ZODE_DESKTOP_IT").is_ok()
}

#[tokio::test]
#[ignore]
async fn ax_lists_apps_windows_and_snapshots() {
    if !enabled() {
        return;
    }
    let factory = platform_factory(&zode_core::config::DesktopConfig::default());
    let backend = factory.create().await.expect("create ax backend");

    let apps = backend.list_apps().await.expect("list apps");
    assert!(!apps.is_empty(), "expected at least one on-screen app");
    eprintln!("apps: {}", apps.len());

    // Find an app that actually has AX windows.
    let mut snapped = false;
    for app in &apps {
        let app_id = AppId::new(0, 0, app.executable_identity.clone(), 0);
        let windows = match backend.list_windows(&app_id).await {
            Ok(w) => w,
            Err(_) => continue,
        };
        if windows.is_empty() {
            continue;
        }
        let token = &windows[0].token;
        let index: u64 = token.parse().unwrap();
        let win = WindowId::new(app_id.clone(), index, 0, 0);
        if let Ok(snap) = backend.snapshot(&win, None).await {
            eprintln!(
                "snapshot of {} window 0:\n{}",
                app.name,
                snap.outline.lines().take(8).collect::<Vec<_>>().join("\n")
            );
            assert!(
                snap.outline.contains("[e1]"),
                "outline should have a first ref"
            );
            snapped = true;
            break;
        }
    }
    assert!(snapped, "expected to snapshot at least one app window");
}

#[tokio::test]
#[ignore]
async fn ax_screenshot_produces_valid_png() {
    if !enabled() {
        return;
    }
    let backend = platform_factory(&zode_core::config::DesktopConfig::default())
        .create()
        .await
        .unwrap();
    let apps = backend.list_apps().await.unwrap();
    for app in &apps {
        let app_id = AppId::new(0, 0, app.executable_identity.clone(), 0);
        let Ok(windows) = backend.list_windows(&app_id).await else {
            continue;
        };
        if windows.is_empty() {
            continue;
        }
        let index: u64 = windows[0].token.parse().unwrap();
        let win = WindowId::new(app_id, index, 0, 0);
        match backend.screenshot(&win).await {
            Ok(shot) => {
                assert_eq!(shot.media_type, "image/png");
                // PNG magic number.
                assert_eq!(
                    &shot.bytes[..8],
                    &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
                );
                assert!(shot.bytes.len() > 100, "png suspiciously small");
                eprintln!("screenshot of {}: {} bytes", app.name, shot.bytes.len());
                return;
            }
            Err(e) => eprintln!("screenshot of {} failed: {e}", app.name),
        }
    }
    // Capture requires the Screen Recording TCC grant, which may be absent in a
    // headless/CI environment. The encoder itself is unit-tested separately;
    // here we only skip if no window could be captured (permission), never fail.
    eprintln!("skipped: no window captured (Screen Recording permission likely not granted)");
}

/// Find the `e<N>` ref number of the first outline line whose role matches.
fn find_ref(outline: &str, role: &str) -> Option<u64> {
    for line in outline.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("[e") {
            if let Some((num, tail)) = rest.split_once(']') {
                if tail.trim_start().starts_with(role) {
                    return num.parse().ok();
                }
            }
        }
    }
    None
}

#[tokio::test]
#[ignore]
async fn ax_set_value_round_trip() {
    if !enabled() {
        return;
    }
    let backend = platform_factory(&zode_core::config::DesktopConfig::default())
        .create()
        .await
        .unwrap();
    let apps = backend.list_apps().await.unwrap();
    let marker = "zode-ax-roundtrip";
    for app in &apps {
        let app_id = AppId::new(0, 0, app.executable_identity.clone(), 0);
        let Ok(windows) = backend.list_windows(&app_id).await else {
            continue;
        };
        if windows.is_empty() {
            continue;
        }
        let index: u64 = windows[0].token.parse().unwrap();
        let win = WindowId::new(app_id.clone(), index, 0, 0);
        let Ok(snap) = backend.snapshot(&win, None).await else {
            continue;
        };
        let Some(eno) = find_ref(&snap.outline, "AXTextArea") else {
            continue;
        };
        let r = zode_core::desktop::backend::ElementRef::new(
            win.clone(),
            snap.snapshot_generation,
            eno,
        );
        if backend.set_value(&r, marker).await.is_err() {
            continue;
        }
        // Some apps' text areas accept SetValue but don't reflect it (read-only
        // views, terminals). Only count an app where the marker round-trips;
        // otherwise move on. This keeps the opt-in E2E robust to whatever apps
        // happen to be on screen.
        let snap2 = backend.snapshot(&win, None).await.unwrap();
        if snap2.outline.contains(marker) {
            eprintln!("set_value round-trip OK on {}", app.name);
            return;
        }
    }
    // No writable text area round-tripped in the current desktop — skip rather
    // than fail (e.g. TextEdit not open). The set_value path is exercised above.
    eprintln!("skipped: no editable AXTextArea round-tripped (open TextEdit to exercise fully)");
}

#[tokio::test]
#[ignore]
async fn ax_stale_ref_without_snapshot() {
    if !enabled() {
        return;
    }
    let backend = platform_factory(&zode_core::config::DesktopConfig::default())
        .create()
        .await
        .unwrap();
    let app = AppId::new(0, 0, "Nonexistent#999999".into(), 0);
    let win = WindowId::new(app, 0, 0, 0);
    let r = zode_core::desktop::backend::ElementRef::new(win, 0, 1);
    // No snapshot has been taken for this window → StaleRef.
    let err = backend
        .element_action(&r, zode_core::desktop::backend::ElementActionKind::Click)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("stale"), "got: {err}");
}
