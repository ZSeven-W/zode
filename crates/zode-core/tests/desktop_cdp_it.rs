//! Opt-in CDP backend end-to-end test. Launches a headless Chromium on a
//! debug port, attaches the CDP DesktopBackend, and drives it. Ignored by
//! default; requires a Chrome/Chromium binary.
//!
//! Run with:
//!   ZODE_DESKTOP_IT=1 cargo test -p zode-core --test desktop_cdp_it -- --ignored --nocapture

use std::process::{Child, Command};

use zode_core::desktop::backend::{AppId, DesktopBackend, WindowId};
use zode_core::desktop::cdp::CdpBackend;

fn enabled() -> bool {
    std::env::var("ZODE_DESKTOP_IT").is_ok()
}

fn chrome_bin() -> Option<String> {
    for p in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ] {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    None
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
#[ignore]
async fn cdp_attaches_snapshots_and_evals() {
    if !enabled() {
        return;
    }
    let Some(bin) = chrome_bin() else {
        eprintln!("skipped: no Chrome/Chromium binary");
        return;
    };
    let port = ized_port();
    let profile = tempfile::tempdir().unwrap();
    let data_url = "data:text/html,<button id=go onclick=\"document.title='clicked'\">Go</button>\
                    <input id=field value=start>";
    let child = Command::new(&bin)
        .args([
            "--headless=new",
            &format!("--remote-debugging-port={port}"),
            &format!("--user-data-dir={}", profile.path().display()),
            "--no-first-run",
            "--no-default-browser-check",
            data_url,
        ])
        .spawn()
        .expect("launch chromium");
    let _guard = Kill(child);

    // Wait for the debug endpoint to accept connections.
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    // Small extra settle so /json/version is served.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let backend = CdpBackend::attach(port).await.expect("cdp attach");

    let apps = backend.list_apps().await.unwrap();
    assert_eq!(apps.len(), 1);
    assert!(apps[0].is_electron);
    eprintln!("attached product: {}", apps[0].name);

    let app = AppId::new(0, 0, apps[0].executable_identity.clone(), 0);
    let windows = backend.list_windows(&app).await.unwrap();
    assert!(!windows.is_empty(), "expected at least one page target");

    let win = WindowId::new(app, 0, 0, 0);
    let snap = backend.snapshot(&win, None).await.unwrap();
    eprintln!("cdp snapshot:\n{}", snap.outline);
    assert!(snap.outline.contains("<button>") || snap.outline.contains("<input>"));

    // eval verifies the desktop_eval path.
    let v = backend.evaluate(&win, "1+2").await.unwrap();
    assert_eq!(v, serde_json::json!(3));

    // set_value round-trip on the tagged input.
    if let Some(eno) = snap
        .outline
        .lines()
        .find(|l| l.contains("<input>"))
        .and_then(ref_num)
    {
        let r = zode_core::desktop::backend::ElementRef::new(win.clone(), 0, eno);
        backend.set_value(&r, "changed").await.unwrap();
        let readback = backend
            .evaluate(&win, "document.getElementById('field').value")
            .await
            .unwrap();
        assert_eq!(readback, serde_json::json!("changed"));
        eprintln!("cdp set_value round-trip OK");
    }

    // screenshot yields valid PNG.
    let shot = backend.screenshot(&win).await.unwrap();
    assert_eq!(
        &shot.bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );
    eprintln!("cdp screenshot: {} bytes", shot.bytes.len());
}

fn ref_num(line: &str) -> Option<u64> {
    let t = line.trim_start();
    let rest = t.strip_prefix('[')?;
    let (num, _) = rest.split_once(']')?;
    num.parse().ok()
}

fn ized_port() -> u16 {
    // Bind an ephemeral port, read it, release — good enough for a test.
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

/// Verifies the two usability fixes together through the real tool layer:
/// (#1) the permission ladder is populated via the approval gate, and (#2)
/// `desktop_read` routes a `cdp#…` app identity to the CDP backend.
#[tokio::test]
#[ignore]
async fn desktop_read_tool_routes_to_cdp_after_attach() {
    if !enabled() {
        return;
    }
    let Some(bin) = chrome_bin() else {
        return;
    };
    let port = ized_port();
    let profile = tempfile::tempdir().unwrap();
    let child = Command::new(&bin)
        .args([
            "--headless=new",
            &format!("--remote-debugging-port={port}"),
            &format!("--user-data-dir={}", profile.path().display()),
            "--no-first-run",
            "--no-default-browser-check",
            "data:text/html,<button>Go</button><input value=x>",
        ])
        .spawn()
        .expect("launch chromium");
    let _guard = Kill(child);
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Build a session + read tool with an auto-approving gate, then attach CDP.
    let session = zode_core::desktop::session::DesktopSession::new(
        zode_core::config::DesktopConfig::default(),
        zode_core::desktop::platform_factory(&zode_core::config::DesktopConfig::default()),
    );
    session.attach_cdp(port).await.expect("attach cdp");
    let shots = tempfile::tempdir().unwrap();
    let deps = zode_core::desktop::tools::DesktopToolDeps {
        session: session.clone(),
        shots_dir: shots.path().to_path_buf(),
        gate: std::sync::Arc::new(zode_core::approval::BypassGate),
    };
    let read = zode_core::desktop::tools::DesktopReadTool::new(deps);
    let ctx = agent::tool::ToolUseContext::new(std::env::temp_dir());
    use agent::tool::Tool;

    // #1: apps works (consent auto-granted via gate) and includes the CDP app.
    let apps = read
        .call(&ctx, serde_json::json!({"action":"apps"}))
        .await
        .unwrap();
    let cdp_id = apps["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|a| {
            let id = a["executable_identity"].as_str()?;
            id.starts_with("cdp").then(|| id.to_string())
        })
        .expect("CDP app should appear in desktop_read apps");
    assert!(session.scopes().subsystem_consented());

    // #2: snapshot with the cdp identity routes to the CDP backend.
    let snap = read
        .call(
            &ctx,
            serde_json::json!({"action":"snapshot","app":cdp_id,"window":"0"}),
        )
        .await
        .unwrap();
    let outline = snap["outline"].as_str().unwrap();
    assert!(
        outline.contains("<button>") || outline.contains("<input>"),
        "expected CDP DOM outline, got: {outline}"
    );
    eprintln!("desktop_read → CDP routing OK: {cdp_id}");
}
