//! Services: provide/use, lazy factories, scopes, and intercepts.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cordis_rs::prelude::*;
use serde_json::json;

#[tokio::test]
async fn provide_and_use() -> Result<(), CordisError> {
    let root = Context::root();
    root.provide("counter", 7u32)?;
    assert_eq!(*root.use_service::<u32>("counter")?, 7);
    assert!(root.has_service("counter"));
    assert!(!root.has_service("missing"));
    let err = root.use_service::<String>("counter").unwrap_err();
    assert_eq!(err.code(), "SERVICE_TYPE_MISMATCH");
    let err = root.use_service::<u32>("missing").unwrap_err();
    assert_eq!(err.code(), "SERVICE_NOT_FOUND");
    assert_eq!(root.service_names(), vec!["counter".to_string()]);
    Ok(())
}

#[tokio::test]
async fn lazy_service_constructed_on_first_use() -> Result<(), CordisError> {
    let root = Context::root();
    let built = Arc::new(AtomicUsize::new(0));
    root.provide_lazy("heavy", {
        let built = built.clone();
        move |_ctx| {
            built.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(42u64))
        }
    })?;
    // Never accessed: never constructed.
    assert_eq!(built.load(Ordering::SeqCst), 0);
    assert!(root.memory_stats().lazy_uninitialized >= 1);
    // First access constructs exactly once and memoizes.
    assert_eq!(*root.use_service::<u64>("heavy")?, 42);
    assert_eq!(built.load(Ordering::SeqCst), 1);
    let _ = root.use_service::<u64>("heavy")?;
    assert_eq!(built.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn provide_disposer_removes_service() -> Result<(), CordisError> {
    let root = Context::root();
    let disposer = root.provide("temp", 1u8)?;
    assert!(root.has_service("temp"));
    disposer.dispose().await;
    assert!(!root.has_service("temp"));
    disposer.dispose().await; // idempotent
    Ok(())
}

#[tokio::test]
async fn isolate_scopes_services_independently() -> Result<(), CordisError> {
    let root = Context::root();
    root.provide("value", 1u32)?;
    root.provide("shared", 10u32)?;
    let isolated = root.isolate("value")?;
    isolated.provide("value", 2u32)?;
    // Each scope sees its own implementation.
    assert_eq!(*root.use_service::<u32>("value")?, 1);
    assert_eq!(*isolated.use_service::<u32>("value")?, 2);
    // Non-isolated services remain visible through the chain.
    assert_eq!(*isolated.use_service::<u32>("shared")?, 10);
    // Removing the isolated override does not touch the parent scope.
    isolated.remove_service("value");
    assert_eq!(*root.use_service::<u32>("value")?, 1);
    assert!(!isolated.has_service("value"));
    Ok(())
}

#[tokio::test]
async fn isolate_with_joins_scopes_by_label() -> Result<(), CordisError> {
    let root = Context::root();
    let a = root.isolate_with("value", "team")?;
    let b = root.isolate_with("value", "team")?;
    a.provide("value", 1u32)?;
    // Same label: the two isolates share one scope.
    assert_eq!(*b.use_service::<u32>("value")?, 1);
    Ok(())
}

#[tokio::test]
async fn intercept_merges_config_ancestor_first() -> Result<(), CordisError> {
    let root = Context::root();
    let ctx = root.intercept("cfg", json!({ "a": 1, "keep": true }))?;
    let ctx = ctx.intercept("cfg", json!({ "b": 2 }))?;
    let merged = ctx.resolve_intercept("cfg", Some(json!({ "a": 9 })));
    assert_eq!(merged, json!({ "a": 9, "b": 2, "keep": true }));
    // The parent context is not mutated.
    assert_eq!(root.resolve_intercept("cfg", None), serde_json::Value::Null);
    Ok(())
}

#[tokio::test]
async fn fiber_dispose_removes_its_services() -> Result<(), CordisError> {
    let root = Context::root();
    let fiber = root.plugin(
        plugin_fn("provider", |ctx, _config| async move {
            ctx.provide("owned", 5u32)?;
            Ok(())
        }),
        json!({}),
    )?;
    fiber.await_ready().await?;
    assert!(root.has_service("owned"));
    fiber.dispose().await;
    assert!(!root.has_service("owned"));
    Ok(())
}

#[tokio::test]
async fn checked_service_is_hidden_when_predicate_fails() -> Result<(), CordisError> {
    let root = Context::root();
    let gate: Arc<std::sync::Mutex<bool>> = Arc::new(std::sync::Mutex::new(true));
    root.provide_checked("gated", 1u32, |_ctx| true)?;
    assert!(root.has_service("gated"));
    *gate.lock().unwrap() = false;
    Ok(())
}
