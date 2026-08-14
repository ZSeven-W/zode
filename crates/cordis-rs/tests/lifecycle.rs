//! Fiber lifecycle: apply, effects, status events, and disposal.

use std::sync::{Arc, Mutex};

use cordis_rs::prelude::*;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn apply_effect_cleanup_and_dispose() -> Result<(), CordisError> {
    let root = Context::root();
    let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let states: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let states = states.clone();
        root.on_dyn("internal/status", move |event| {
            let state = event
                .payload
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            states.lock().unwrap().push(state);
            async { Flow::Continue }
        })?;
    }

    let plugin = plugin_fn("demo", {
        let order = order.clone();
        move |ctx, _config| {
            let order = order.clone();
            async move {
                let first = order.clone();
                ctx.effect_fn(
                    "first",
                    Cleanup::sync(move || first.lock().unwrap().push("first")),
                )?;
                let second = order.clone();
                ctx.effect_fn(
                    "second",
                    Cleanup::sync(move || second.lock().unwrap().push("second")),
                )?;
                Ok(())
            }
        }
    });
    let fiber = root.plugin(plugin, json!({}))?;
    assert_eq!(fiber.state(), FiberState::Pending);
    fiber.await_ready().await?;
    assert_eq!(fiber.state(), FiberState::Active);
    assert_eq!(
        fiber.effects(),
        vec![Arc::from("first"), Arc::from("second")]
    );

    fiber.dispose().await;
    assert_eq!(fiber.state(), FiberState::Disposed);
    // Cleanup runs in reverse registration order.
    assert_eq!(*order.lock().unwrap(), vec!["second", "first"]);

    // Status events are observable through the event bus.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let seen = states.lock().unwrap().clone();
    for expected in ["loading", "active", "unloading", "disposed"] {
        assert!(
            seen.iter().any(|s| s == expected),
            "missing {expected} in states {seen:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn failed_plugin_reports_error() -> Result<(), CordisError> {
    let root = Context::root();
    let plugin = plugin_fn("broken", |_ctx, _config| async {
        Err(CordisError::ServiceNotFound("nope".to_string()))
    });
    let fiber = root.plugin(plugin, json!({}))?;
    let err = fiber.await_ready().await.unwrap_err();
    assert_eq!(err.code(), "PLUGIN_STARTUP");
    assert_eq!(fiber.state(), FiberState::Failed);
    assert!(fiber.error().is_some());
    Ok(())
}

#[tokio::test]
async fn config_is_validated_and_updatable() -> Result<(), CordisError> {
    let root = Context::root();
    let seen: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
    let plugin = plugin_fn("conf", {
        let seen = seen.clone();
        move |_ctx, config| {
            let seen = seen.clone();
            async move {
                seen.lock()
                    .unwrap()
                    .push(config["n"].as_u64().unwrap_or(0) as u32);
                Ok(())
            }
        }
    });
    let fiber = root.plugin(plugin, json!({ "n": 1 }))?;
    fiber.await_ready().await?;
    assert_eq!(*seen.lock().unwrap(), vec![1]);
    fiber.update(json!({ "n": 2 })).await?;
    fiber.await_ready().await?;
    assert_eq!(*seen.lock().unwrap(), vec![1, 2]);
    Ok(())
}

#[tokio::test]
async fn plugin_runtime_inspection() -> Result<(), CordisError> {
    let root = Context::root();
    let fiber = root.plugin(
        plugin_fn("inspect-me", |_ctx, _config| async { Ok(()) }),
        json!({}),
    )?;
    fiber.await_ready().await?;
    let mut names: Vec<(String, usize)> = Vec::new();
    root.for_each_plugin(|name, active| names.push((name.to_string(), active)));
    assert!(names
        .iter()
        .any(|(n, active)| n == "inspect-me" && *active == 1));
    assert_eq!(root.plugin_count(), 1);
    Ok(())
}
