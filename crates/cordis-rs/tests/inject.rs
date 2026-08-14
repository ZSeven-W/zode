//! Dependency scheduling: inject fibers load/reload with their services.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cordis_rs::prelude::*;
use serde_json::json;

async fn wait_for(predicate: impl Fn() -> bool) {
    for _ in 0..200 {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition not met within timeout");
}

#[tokio::test]
async fn inject_loads_when_dependencies_arrive() -> Result<(), CordisError> {
    let root = Context::root();
    let ran = Arc::new(AtomicUsize::new(0));
    let fiber = root.inject(&["db"], {
        let ran = ran.clone();
        move |ctx, _config| {
            let ran = ran.clone();
            async move {
                let db = ctx.use_service::<u32>("db")?;
                assert_eq!(*db, 1);
                ran.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
    })?;
    assert_eq!(fiber.state(), FiberState::Pending);
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    root.provide("db", 1u32)?;
    fiber.await_ready().await?;
    assert_eq!(fiber.state(), FiberState::Active);
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn inject_reruns_when_service_is_replaced() -> Result<(), CordisError> {
    let root = Context::root();
    let ran = Arc::new(AtomicUsize::new(0));
    root.provide("db", 1u32)?;
    let fiber = root.inject(&["db"], {
        let ran = ran.clone();
        move |_ctx, _config| {
            let ran = ran.clone();
            async move {
                ran.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
    })?;
    fiber.await_ready().await?;
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    root.provide("db", 2u32)?;
    wait_for(|| ran.load(Ordering::SeqCst) >= 2).await;
    Ok(())
}

#[tokio::test]
async fn inject_unloads_when_service_removed_then_reloads() -> Result<(), CordisError> {
    let root = Context::root();
    root.provide("db", 1u32)?;
    let fiber = root.inject(&["db"], |_ctx, _config| async { Ok(()) })?;
    fiber.await_ready().await?;
    assert_eq!(fiber.state(), FiberState::Active);
    assert!(root.remove_service("db"));
    wait_for(|| fiber.state() == FiberState::Pending).await;
    root.provide("db", 3u32)?;
    fiber.await_ready().await?;
    assert_eq!(fiber.state(), FiberState::Active);
    Ok(())
}

#[tokio::test]
async fn plugin_with_inject_metadata_waits_like_inject() -> Result<(), CordisError> {
    let root = Context::root();
    let ran = Arc::new(AtomicUsize::new(0));
    let plugin = plugin_fn("needs-db", {
        let ran = ran.clone();
        move |_ctx, _config| {
            let ran = ran.clone();
            async move {
                ran.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
    })
    .with_inject(&["db"]);
    let fiber = root.plugin(plugin, json!({}))?;
    assert_eq!(fiber.state(), FiberState::Pending);
    root.provide("db", 1u32)?;
    fiber.await_ready().await?;
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    Ok(())
}
