//! Event bus: dispatch modes, filtering, typed events, waterfall.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cordis_rs::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn emit_fire_and_forget() -> Result<(), CordisError> {
    let root = Context::root();
    let hits = Arc::new(AtomicUsize::new(0));
    root.on_dyn("tick", {
        let hits = hits.clone();
        move |_event| {
            hits.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;
    root.emit_dyn("tick", &json!({ "n": 1 }))?;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(hits.load(Ordering::SeqCst) >= 1);
    Ok(())
}

#[tokio::test]
async fn parallel_waits_for_all_listeners() -> Result<(), CordisError> {
    let root = Context::root();
    let hits = Arc::new(AtomicUsize::new(0));
    for _ in 0..3 {
        root.on_dyn("join", {
            let hits = hits.clone();
            move |_event| {
                hits.fetch_add(1, Ordering::SeqCst);
                async { Flow::Continue }
            }
        })?;
    }
    root.parallel_dyn("join", &json!({})).await?;
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    Ok(())
}

#[tokio::test]
async fn serial_returns_first_bail_and_runs_all() -> Result<(), CordisError> {
    let root = Context::root();
    let calls = Arc::new(AtomicUsize::new(0));
    root.on_dyn("scan", {
        let calls = calls.clone();
        move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;
    root.on_dyn("scan", {
        let calls = calls.clone();
        move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Flow::bail_typed("stop") }
        }
    })?;
    root.on_dyn("scan", {
        let calls = calls.clone();
        move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;
    let result = root.serial_dyn("scan", &json!({})).await?;
    assert_eq!(result, Some(json!("stop")));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    Ok(())
}

#[tokio::test]
async fn bail_stops_at_first_bail() -> Result<(), CordisError> {
    let root = Context::root();
    let calls = Arc::new(AtomicUsize::new(0));
    root.on_dyn("gate", {
        let calls = calls.clone();
        move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;
    root.on_dyn("gate", {
        let calls = calls.clone();
        move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Flow::bail_typed(1) }
        }
    })?;
    root.on_dyn("gate", {
        let calls = calls.clone();
        move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;
    let result = root.bail_dyn("gate", &json!({})).await?;
    assert_eq!(result, Some(json!(1)));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn once_fires_once() -> Result<(), CordisError> {
    let root = Context::root();
    let hits = Arc::new(AtomicUsize::new(0));
    root.once_dyn("boot", {
        let hits = hits.clone();
        move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;
    root.parallel_dyn("boot", &json!({})).await?;
    root.parallel_dyn("boot", &json!({})).await?;
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn context_filtering() -> Result<(), CordisError> {
    let root = Context::root();
    let hits = Arc::new(AtomicUsize::new(0));
    root.on_dyn("evt", {
        let hits = hits.clone();
        move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;
    let child = root.extend()?;
    child.on_dyn("evt", {
        let hits = hits.clone();
        move |_| {
            hits.fetch_add(10, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;
    // Child dispatch reaches both listeners (root + child).
    child.parallel_dyn("evt", &json!({})).await?;
    assert_eq!(hits.load(Ordering::SeqCst), 11);
    // Root dispatch must not reach the child listener.
    hits.store(0, Ordering::SeqCst);
    root.parallel_dyn("evt", &json!({})).await?;
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn global_listener_ignores_context_filter() -> Result<(), CordisError> {
    let root = Context::root();
    let child = root.extend()?;
    let hits = Arc::new(AtomicUsize::new(0));
    child.on_dyn_global("evt", {
        let hits = hits.clone();
        move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;
    root.parallel_dyn("evt", &json!({})).await?;
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn listener_disposer_removes_early() -> Result<(), CordisError> {
    let root = Context::root();
    let hits = Arc::new(AtomicUsize::new(0));
    let disposer = root.on_dyn("evt", {
        let hits = hits.clone();
        move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;
    disposer.dispose().await;
    root.parallel_dyn("evt", &json!({})).await?;
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert_eq!(root.memory_stats().listeners, 0);
    Ok(())
}

#[tokio::test]
async fn waterfall_composes_around_core() -> Result<(), CordisError> {
    let root = Context::root();
    let trace: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    root.on_waterfall_dyn("wrap", {
        let trace = trace.clone();
        move |_event, next| {
            let trace = trace.clone();
            async move {
                trace.lock().unwrap().push("outer-before");
                let flow = next.call().await;
                trace.lock().unwrap().push("outer-after");
                flow
            }
        }
    })?;
    root.on_waterfall_dyn("wrap", {
        let trace = trace.clone();
        move |_event, next| {
            let trace = trace.clone();
            async move {
                trace.lock().unwrap().push("inner-before");
                let flow = next.call().await;
                trace.lock().unwrap().push("inner-after");
                flow
            }
        }
    })?;
    let flow = root
        .waterfall_dyn("wrap", &json!({}), {
            let trace = trace.clone();
            move |_event| {
                trace.lock().unwrap().push("core");
                Flow::Continue
            }
        })
        .await?;
    assert_eq!(flow, Flow::Continue);
    assert_eq!(
        *trace.lock().unwrap(),
        vec![
            "outer-before",
            "inner-before",
            "core",
            "inner-after",
            "outer-after"
        ]
    );
    Ok(())
}

#[tokio::test]
async fn waterfall_veto_skips_the_rest() -> Result<(), CordisError> {
    let root = Context::root();
    let core = Arc::new(AtomicUsize::new(0));
    root.on_waterfall_dyn("wrap", |_event, _next| async { Flow::bail_typed("veto") })?;
    let flow = root
        .waterfall_dyn("wrap", &json!({}), {
            let core = core.clone();
            move |_event| {
                core.fetch_add(1, Ordering::SeqCst);
                Flow::Continue
            }
        })
        .await?;
    assert_eq!(flow, Flow::Bail(json!("veto")));
    assert_eq!(core.load(Ordering::SeqCst), 0);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Ping {
    n: u32,
}

struct PingEvent;

impl EventDef for PingEvent {
    const NAME: &'static str = "ping";
    type Payload = Ping;
    type Return = String;
}

#[tokio::test]
async fn typed_events_round_trip() -> Result<(), CordisError> {
    let root = Context::root();
    let seen: Arc<Mutex<Vec<Ping>>> = Arc::new(Mutex::new(Vec::new()));
    root.on_t::<PingEvent, _, _>({
        let seen = seen.clone();
        move |ping: &Ping| {
            let seen = seen.clone();
            let ping = ping.clone();
            async move {
                seen.lock().unwrap().push(ping);
                Flow::bail_typed("pong")
            }
        }
    })?;
    let result = root.serial_t::<PingEvent>(&Ping { n: 2 }).await?;
    assert_eq!(result, Some("pong".to_string()));
    assert_eq!(*seen.lock().unwrap(), vec![Ping { n: 2 }]);
    Ok(())
}
