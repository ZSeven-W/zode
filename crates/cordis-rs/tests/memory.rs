//! Memory accounting: stats, budget caps, and bounded event history.

use cordis_rs::prelude::*;
use serde_json::json;

#[tokio::test]
async fn stats_reflect_live_state() -> Result<(), CordisError> {
    let root = Context::root();
    root.provide("a", 1u32)?;
    root.on_dyn("evt", |_| async { Flow::Continue })?;
    let fiber = root.plugin(
        plugin_fn("p", |ctx, _config| async move {
            ctx.provide("owned", 2u32)?;
            Ok(())
        }),
        json!({}),
    )?;
    fiber.await_ready().await?;
    let stats = root.memory_stats();
    assert!(stats.services >= 2);
    assert!(stats.listeners >= 1);
    assert!(stats.fibers >= 2); // root fiber + plugin fiber
    assert!(stats.events >= 1);
    assert!(stats.estimated_bytes > 0);
    // Disposing the plugin fiber removes only ITS service.
    fiber.dispose().await;
    assert_eq!(root.memory_stats().services, 1);
    Ok(())
}

#[tokio::test]
async fn service_budget_enforced() -> Result<(), CordisError> {
    let root = Context::root();
    root.set_budget(MemoryBudget {
        max_services: 1,
        ..Default::default()
    });
    root.provide("only", 1u32)?;
    let err = root.provide("second", 2u32).unwrap_err();
    assert_eq!(err.code(), "BUDGET_EXCEEDED");
    // Replacing the existing service still fits within the cap.
    root.provide("only", 3u32)?;
    assert_eq!(*root.use_service::<u32>("only")?, 3);
    Ok(())
}

#[tokio::test]
async fn listener_budget_enforced() -> Result<(), CordisError> {
    let root = Context::root();
    root.set_budget(MemoryBudget {
        max_listeners: 1,
        ..Default::default()
    });
    root.on_dyn("a", |_| async { Flow::Continue })?;
    let err = root.on_dyn("b", |_| async { Flow::Continue }).unwrap_err();
    assert_eq!(err.code(), "BUDGET_EXCEEDED");
    Ok(())
}

#[tokio::test]
async fn fiber_budget_enforced() -> Result<(), CordisError> {
    let root = Context::root();
    // Root fiber + one plugin fit; the second plugin does not.
    root.set_budget(MemoryBudget {
        max_fibers: 2,
        ..Default::default()
    });
    let ok = root.plugin(plugin_fn("a", |_ctx, _config| async { Ok(()) }), json!({}))?;
    ok.await_ready().await?;
    let err = root
        .plugin(plugin_fn("b", |_ctx, _config| async { Ok(()) }), json!({}))
        .unwrap_err();
    assert_eq!(err.code(), "BUDGET_EXCEEDED");
    Ok(())
}

#[tokio::test]
async fn context_budget_enforced() -> Result<(), CordisError> {
    let root = Context::root();
    // root ctx + root-fiber ctx + one held child fit; the next does not.
    root.set_budget(MemoryBudget {
        max_contexts: 3,
        ..Default::default()
    });
    let child = root.extend()?;
    let err = root.extend().unwrap_err();
    assert_eq!(err.code(), "BUDGET_EXCEEDED");
    drop(child);
    // Dropping the child frees its slot: a new scope fits again.
    root.extend()?;
    Ok(())
}

#[tokio::test]
async fn event_history_is_bounded() -> Result<(), CordisError> {
    let root = Context::root();
    root.set_budget(MemoryBudget {
        max_event_history: 3,
        ..Default::default()
    });
    for i in 0..5 {
        root.emit_dyn("evt", &json!({ "i": i }))?;
    }
    let history = root.event_history();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].payload["i"], json!(2));
    assert_eq!(history[2].payload["i"], json!(4));
    assert!(root.memory_stats().history_bytes > 0);
    Ok(())
}
