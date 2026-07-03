//! Real-Chrome integration tests. Opt-in:
//!   ZODE_BROWSER_IT=1 cargo test -p zode-core --test browser_it -- --ignored
use zode_core::browser::{BrowserSession, ManagedFactory};
use zode_core::config::BrowserConfig;

#[tokio::test]
#[ignore]
async fn managed_end_to_end() {
    if std::env::var("ZODE_BROWSER_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set ZODE_BROWSER_IT=1");
        return;
    }
    let cfg = BrowserConfig {
        headless: Some(true),
        ..Default::default()
    };
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
