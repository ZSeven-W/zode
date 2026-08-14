//! Process plugins: any executable speaks the JSON-lines protocol and
//! becomes a swappable gene; disposing the fiber kills the child process.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cordis_rs::prelude::*;
use serde_json::json;

/// A shell-script "plugin": subscribes to `probe`, echoes every event it
/// receives as a `child/reply` emit. Proves the medium is irrelevant —
/// the plugin is any compiled/interpreted program.
fn shell_plugin() -> ProcessPlugin {
    let script = "printf '{\"op\":\"listen\",\"event\":\"probe\"}\n';
                  while read -r line; do
                    printf '{\"op\":\"emit\",\"event\":\"child/reply\",\"payload\":{\"n\":1}}\n';
                  done";
    ProcessPlugin::new("shell-gene", "bash").with_args(vec!["-c".to_string(), script.to_string()])
}

/// The child's `listen` subscription arrives asynchronously after the
/// fiber is active, so dispatch in a retry loop until the child answers.
async fn probe_until(root: &Context, replies: &AtomicUsize, expected: usize) {
    for _ in 0..100 {
        if replies.load(Ordering::SeqCst) >= expected {
            return;
        }
        let _ = root.parallel_dyn("probe", &json!({})).await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

#[tokio::test]
#[cfg(unix)]
async fn child_receives_events_and_emits_back() -> Result<(), CordisError> {
    let root = Context::root();
    let replies = Arc::new(AtomicUsize::new(0));
    root.on_dyn("child/reply", {
        let replies = replies.clone();
        move |_event| {
            replies.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;

    let gene = root.plugin(shell_plugin(), json!({}))?;
    gene.await_ready().await?;
    probe_until(&root, &replies, 2).await;
    assert!(replies.load(Ordering::SeqCst) >= 2);
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn dispose_kills_the_child_process() -> Result<(), CordisError> {
    let root = Context::root();
    let gene = root.plugin(shell_plugin(), json!({}))?;
    gene.await_ready().await?;

    // The child is alive and has a different pid than the harness.
    let before = std::process::Command::new("pgrep")
        .args(["-f", "while read -r line"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    assert!(!before.is_empty(), "child process not found before dispose");

    gene.dispose().await;

    // After disposal the child must be gone.
    for _ in 0..100 {
        let after = std::process::Command::new("pgrep")
            .args(["-f", "while read -r line"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        if after.is_empty() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("child process survived fiber dispose");
}

#[tokio::test]
#[cfg(unix)]
async fn live_replacement_swaps_the_binary() -> Result<(), CordisError> {
    let root = Context::root();
    let versions = Arc::new(std::sync::Mutex::new(Vec::new()));
    root.on_dyn("gene/result", {
        let versions = versions.clone();
        move |event| {
            let version = event
                .payload
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            versions.lock().unwrap().push(version);
            async { Flow::Continue }
        }
    })?;

    // "Compile" v1 and v2 as tiny standalone programs (shell here, any
    // compiled language in production — the harness does not care).
    let script = |version: &str| {
        format!(
            "printf '{{\"op\":\"listen\",\"event\":\"probe\"}}\n';
             while read -r line; do
               printf '{{\"op\":\"emit\",\"event\":\"gene/result\",\"payload\":{{\"version\":\"{version}\"}}}}\n';
             done"
        )
    };
    let v1 = root.plugin(
        ProcessPlugin::new("gene", "bash").with_args(vec!["-c".to_string(), script("v1")]),
        json!({}),
    )?;
    v1.await_ready().await?;
    probe_until_contains(&root, &versions, "v1").await;

    // LIVE REPLACEMENT: dispose v1 (kills its process) and load v2.
    v1.dispose().await;
    let v2 = root.plugin(
        ProcessPlugin::new("gene", "bash").with_args(vec!["-c".to_string(), script("v2")]),
        json!({}),
    )?;
    v2.await_ready().await?;
    probe_until_contains(&root, &versions, "v2").await;

    let seen = versions.lock().unwrap().clone();
    assert!(
        seen.contains(&"v1".to_string()),
        "v1 never answered: {seen:?}"
    );
    assert!(
        seen.contains(&"v2".to_string()),
        "v2 never answered: {seen:?}"
    );
    assert_eq!(seen.first().map(String::as_str), Some("v1"));
    Ok(())
}

/// Dispatch probes until the child's `gene/result` emit reports the given
/// version (the child's subscription lands asynchronously after spawn).
async fn probe_until_contains(
    root: &Context,
    versions: &std::sync::Mutex<Vec<String>>,
    version: &str,
) {
    for _ in 0..100 {
        if versions.lock().unwrap().iter().any(|v| v == version) {
            return;
        }
        let _ = root.parallel_dyn("probe", &json!({})).await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}
