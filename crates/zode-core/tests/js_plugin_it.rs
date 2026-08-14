//! QuickJS gene layer integration tests: events, cleanup, live source
//! replacement (no compiler), interrupt deadlines, and memory limits.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cordis_rs::prelude::*;
use serde_json::json;
use zode_core::js_plugin::JsPlugin;

fn gene(version: &'static str) -> String {
    format!(
        r#"(function () {{
  return {{
    apply: function (host) {{
      host.on("probe", function (payload) {{
        host.emit("gene/result", JSON.stringify({{ version: "{version}", saw: payload }}));
        return null;
      }});
      host.effect(function () {{
        host.emit("gene/cleaned", JSON.stringify({{ version: "{version}" }}));
      }});
    }},
  }};
}})"#
    )
}

async fn probe_until(ctx: &Context, hits: &AtomicUsize, expected: usize) {
    for _ in 0..100 {
        if hits.load(Ordering::SeqCst) >= expected {
            return;
        }
        let _ = ctx.parallel_dyn("probe", &json!({})).await;
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn js_gene_handles_events_and_cleans_up() -> Result<(), CordisError> {
    let root = Context::root();
    let hits = Arc::new(AtomicUsize::new(0));
    root.on_dyn("gene/result", {
        let hits = hits.clone();
        move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;
    let cleaned = Arc::new(std::sync::Mutex::new(false));
    root.on_dyn("gene/cleaned", {
        let cleaned = cleaned.clone();
        move |_| {
            *cleaned.lock().unwrap() = true;
            async { Flow::Continue }
        }
    })?;

    let fiber = root.plugin(JsPlugin::new("gene", gene("v1")), json!({}))?;
    fiber.await_ready().await?;
    probe_until(&root, &hits, 1).await;
    assert!(hits.load(Ordering::SeqCst) >= 1);

    fiber.dispose().await;
    for _ in 0..100 {
        if *cleaned.lock().unwrap() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(*cleaned.lock().unwrap(), "guest cleanup never ran");
    Ok(())
}

#[tokio::test]
async fn live_replacement_swaps_the_source() -> Result<(), CordisError> {
    let root = Context::root();
    let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    root.on_dyn("gene/result", {
        let seen = seen.clone();
        move |event| {
            let version = event
                .payload
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            seen.lock().unwrap().push(version);
            async { Flow::Continue }
        }
    })?;

    // No compiler: the "artifact" is the source text itself.
    let v1 = root.plugin(JsPlugin::new("gene", gene("v1")), json!({}))?;
    v1.await_ready().await?;
    probe_until_version(&root, &seen, "v1").await;

    // LIVE REPLACEMENT: dispose the v1 fiber (its QuickJS runtime is torn
    // down), then load the new source.
    v1.dispose().await;
    let v2 = root.plugin(JsPlugin::new("gene", gene("v2")), json!({}))?;
    v2.await_ready().await?;
    probe_until_version(&root, &seen, "v2").await;

    let seen = seen.lock().unwrap().clone();
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

async fn probe_until_version(ctx: &Context, seen: &std::sync::Mutex<Vec<String>>, version: &str) {
    for _ in 0..100 {
        if seen.lock().unwrap().iter().any(|v| v == version) {
            return;
        }
        let _ = ctx.parallel_dyn("probe", &json!({})).await;
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn runaway_handler_is_interrupted() -> Result<(), CordisError> {
    let root = Context::root();
    let source = r#"(function () {
  return {
    apply: function (host) {
      host.on("spin", function () { while (true) {} });
    },
  };
})"#;
    let fiber = root.plugin(
        JsPlugin::new("runaway", source).with_call_timeout(100),
        json!({}),
    )?;
    fiber.await_ready().await?;

    // The dispatch must return quickly despite the infinite loop.
    let started = Instant::now();
    let _ = root.parallel_dyn("spin", &json!({})).await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "interrupt deadline did not fire"
    );
    Ok(())
}

#[tokio::test]
async fn memory_limit_fails_the_gene() -> Result<(), CordisError> {
    let root = Context::root();
    // Allocate far beyond the cap inside apply(): the runtime must kill it
    // and the fiber must end up Failed instead of OOMing the host.
    let source = r#"(function () {
  return {
    apply: function () { var a = []; for (var i = 0; i < 10000000; i++) { a.push(i); } },
  };
})"#;
    let fiber = root.plugin(
        JsPlugin::new("greedy", source).with_memory_limit(2 * 1024 * 1024),
        json!({}),
    )?;
    let err = fiber.await_ready().await.unwrap_err();
    assert_eq!(err.code(), "PLUGIN_STARTUP");
    assert_eq!(fiber.state(), FiberState::Failed);
    Ok(())
}
