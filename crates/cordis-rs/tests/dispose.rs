//! Disposal: root teardown and drop-based leak prevention.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cordis_rs::prelude::*;
use serde_json::json;

#[tokio::test]
async fn root_dispose_tears_down_everything() -> Result<(), CordisError> {
    let root = Context::root();
    let fiber = root.plugin(
        plugin_fn("p", |ctx, _config| async move {
            ctx.provide("s", 1u32)?;
            ctx.on_dyn("evt", |_| async { Flow::Continue })?;
            Ok(())
        }),
        json!({}),
    )?;
    fiber.await_ready().await?;
    let cleaned = Arc::new(AtomicUsize::new(0));
    root.on_dispose({
        let cleaned = cleaned.clone();
        || async move {
            cleaned.fetch_add(1, Ordering::SeqCst);
        }
    })?;
    root.dispose().await?;
    assert_eq!(fiber.state(), FiberState::Disposed);
    assert_eq!(root.memory_stats().services, 0);
    assert_eq!(root.memory_stats().listeners, 0);
    assert_eq!(cleaned.load(Ordering::SeqCst), 1);
    // A disposed root rejects further work.
    assert_eq!(
        root.provide("x", 1u32).unwrap_err().code(),
        "CONTEXT_DISPOSED"
    );
    assert_eq!(
        root.plugin(
            plugin_fn("late", |_ctx, _config| async { Ok(()) }),
            json!({})
        )
        .unwrap_err()
        .code(),
        "CONTEXT_DISPOSED"
    );
    Ok(())
}

#[tokio::test]
async fn dispose_is_idempotent() -> Result<(), CordisError> {
    let root = Context::root();
    root.dispose().await?;
    root.dispose().await?;
    Ok(())
}

#[tokio::test]
async fn non_root_dispose_is_rejected() -> Result<(), CordisError> {
    let root = Context::root();
    let child = root.extend()?;
    assert_eq!(child.dispose().await.unwrap_err().code(), "UNSUPPORTED");
    Ok(())
}

#[tokio::test]
async fn dropping_the_root_context_frees_plugin_state() -> Result<(), CordisError> {
    let root = Context::root();
    let fiber = root.plugin(
        plugin_fn("p", |ctx, _config| async move {
            ctx.provide("s", 1u32)?;
            ctx.on_dyn("evt", |_| async { Flow::Continue })?;
            Ok(())
        }),
        json!({}),
    )?;
    fiber.await_ready().await?;
    let weak = root.downgrade();
    drop(fiber);
    drop(root);
    // Every strong handle is gone: the harness must be fully collected.
    assert!(weak.upgrade().is_none());
    Ok(())
}

#[tokio::test]
async fn dropping_root_with_live_child_cleans_fibers() -> Result<(), CordisError> {
    let root = Context::root();
    let child = root.extend()?;
    let fiber = root.plugin(
        plugin_fn("p", |ctx, _config| async move {
            ctx.provide("s", 1u32)?;
            Ok(())
        }),
        json!({}),
    )?;
    fiber.await_ready().await?;
    drop(fiber);
    drop(root);
    // The child keeps the root state alive, but the plugin fiber and its
    // services were owned by the root context and must be gone.
    assert_eq!(child.memory_stats().services, 0);
    assert_eq!(child.memory_stats().fibers, 0);
    Ok(())
}
