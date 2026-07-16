//! Real-Chrome integration tests. Opt-in:
//!   ZODE_BROWSER_IT=1 cargo test -p zode-core --test browser_it -- --ignored
use agent::tool::{Tool, ToolUseContext};
use zode_core::browser::{
    BrowserReadTool, BrowserSession, BrowserToolDeps, ClickTarget, ManagedFactory,
};
use zode_core::config::BrowserConfig;

/// Builds a headless `BrowserConfig` pointed at a fresh temp profile dir.
///
/// Each test gets its OWN profile dir rather than the default
/// `~/.zode/browser-profile`: Chrome takes an exclusive `SingletonLock` on
/// its profile dir, so two tests sharing the default (the harness runs
/// `#[tokio::test]` fns concurrently by default) would race for the lock —
/// one launch fails or hangs waiting on the other. The returned `TempDir`
/// must be kept alive for the duration of the test (it deletes on drop).
fn isolated_headless_cfg() -> (tempfile::TempDir, BrowserConfig) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = BrowserConfig {
        headless: Some(true),
        profile_dir: Some(dir.path().to_string_lossy().into_owned()),
        ..Default::default()
    };
    (dir, cfg)
}

#[tokio::test]
#[ignore]
async fn managed_end_to_end() {
    if std::env::var("ZODE_BROWSER_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set ZODE_BROWSER_IT=1");
        return;
    }
    let (_profile_dir, cfg) = isolated_headless_cfg();
    let session = BrowserSession::new(cfg, std::sync::Arc::new(ManagedFactory));
    let lease = session.lease().await.expect("launch");
    let b = lease.backend();
    let url = b.navigate("data:text/html,<h1 id=t>hi</h1>").await.unwrap();
    assert!(url.starts_with("data:"));
    assert_eq!(b.evaluate("1+1").await.unwrap(), serde_json::json!(2));
    let shot = b.screenshot().await.unwrap();
    assert!(
        shot.bytes.len() > 1000,
        "screenshot too small: {}",
        shot.bytes.len()
    );

    let outline = b.snapshot().await.unwrap();
    assert!(outline.contains("[1]"), "outline: {outline}");
    b.click(&zode_core::browser::ClickTarget::Ref(1))
        .await
        .unwrap();
    b.evaluate("console.log('probe-42')").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let logs = b.console_logs(10).await.unwrap();
    assert!(
        logs.iter().any(|l| l.text.contains("probe-42")),
        "logs: {logs:?}"
    );
    let tabs = b.tabs().await.unwrap();
    assert_eq!(tabs.len(), 1);

    drop(lease);
    session.close().await;
}

/// Regression test for the M1 review finding: `tab_new`/`tab_select`/
/// `tab_close` re-attach console/network listeners on every page swap but
/// previously never stopped the PREVIOUS page's listener tasks, so after
/// A -> B -> A there were two live listeners writing into the shared
/// `console_buf`, doubling every console entry (and letting a backgrounded
/// tab keep contaminating the logs). `ManagedBackend::replace_listeners`
/// now aborts the old page's tasks before attaching the new page's.
#[tokio::test]
#[ignore]
async fn tab_switch_a_b_a_does_not_duplicate_console_entries() {
    if std::env::var("ZODE_BROWSER_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set ZODE_BROWSER_IT=1");
        return;
    }
    let (_profile_dir, cfg) = isolated_headless_cfg();
    let session = BrowserSession::new(cfg, std::sync::Arc::new(ManagedFactory));
    let lease = session.lease().await.expect("launch");
    let b = lease.backend();

    // Tab A: the initial tab from launch().
    let tab_a = b.tabs().await.unwrap().remove(0);
    b.navigate("data:text/html,<h1>A</h1>").await.unwrap();

    // Tab B: open a second tab (tab_new makes it current and re-attaches
    // listeners — this is where the old code leaked A's listener tasks).
    let tab_b = b.tab_new(Some("data:text/html,<h1>B</h1>")).await.unwrap();

    // A -> B -> A: select back to A, re-attaching listeners a second time.
    b.tab_select(&tab_a.id).await.unwrap();
    b.tab_select(&tab_b.id).await.unwrap();
    b.tab_select(&tab_a.id).await.unwrap();

    // Emit exactly one console entry with a unique marker on the CURRENT
    // (A) page. If a stale listener from an earlier attach is still alive,
    // it independently observes the same event and the marker appears more
    // than once in the buffer.
    b.evaluate("console.log('switch-marker-77')").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let logs = b.console_logs(50).await.unwrap();
    let hits = logs
        .iter()
        .filter(|l| l.text.contains("switch-marker-77"))
        .count();
    assert_eq!(
        hits, 1,
        "expected exactly one console entry after A->B->A switching, got {hits}: {logs:?}"
    );

    drop(lease);
    session.close().await;
}

#[tokio::test]
#[ignore]
async fn upload_and_download_end_to_end() {
    if std::env::var("ZODE_BROWSER_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set ZODE_BROWSER_IT=1");
        return;
    }
    let (_profile_dir, cfg) = isolated_headless_cfg();
    let upload_dir = tempfile::tempdir().expect("upload tempdir");
    let upload = upload_dir.path().join("upload.txt");
    std::fs::write(&upload, b"upload-probe").unwrap();
    let upload = std::fs::canonicalize(upload).unwrap();
    let session = BrowserSession::new(cfg, std::sync::Arc::new(ManagedFactory));
    let lease = session.lease().await.expect("launch");
    let backend = lease.backend();
    backend
        .navigate("data:text/html,<input id=f type=file><button id=d>download</button>")
        .await
        .unwrap();
    backend
        .set_file_input(&ClickTarget::Selector("#f".into()), &[upload])
        .await
        .unwrap();
    assert_eq!(
        backend
            .evaluate("document.querySelector('#f').files.length")
            .await
            .unwrap(),
        serde_json::json!(1)
    );
    backend
        .evaluate(
            "(()=>{const a=document.createElement('a');a.download='zode-download.txt';a.href=URL.createObjectURL(new Blob(['download-probe']));a.click();return true})()",
        )
        .await
        .unwrap();
    drop(lease);

    let read = BrowserReadTool::new(BrowserToolDeps {
        session: session.clone(),
        shots_dir: upload_dir.path().join("shots"),
        target_override: None,
    });
    let ctx = ToolUseContext::new(std::env::temp_dir());
    let mut completed = None;
    for _ in 0..100 {
        let value = read
            .call(&ctx, serde_json::json!({"action":"downloads", "limit":10}))
            .await
            .unwrap();
        completed = value["entries"]
            .as_array()
            .and_then(|entries| entries.iter().find(|entry| entry["status"] == "complete"))
            .cloned();
        if completed.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let completed = completed.expect("download did not complete");
    let path = completed["path"].as_str().expect("completed download path");
    assert!(
        std::path::Path::new(path).is_file(),
        "missing download: {path}"
    );
    session.close().await;
}
