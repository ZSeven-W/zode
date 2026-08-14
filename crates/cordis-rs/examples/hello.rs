//! Minimal harness composition: a lazy counter service, a plugin that
//! subscribes to `app/ready`, a typed dispatch, memory stats, and teardown.
//!
//! Run with: `cargo run -p cordis-rs --example hello`

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cordis_rs::prelude::*;
use serde_json::json;

struct AppReady;

impl EventDef for AppReady {
    const NAME: &'static str = "app/ready";
    type Payload = String;
    type Return = ();
}

#[tokio::main]
async fn main() -> Result<(), CordisError> {
    let root = Context::root();

    // Lazy service: never constructed unless something actually reads it.
    root.provide_lazy("counter", |_ctx| Ok(Arc::new(AtomicUsize::new(0))))?;

    let incrementer = root.plugin(
        plugin_fn("incrementer", |ctx, _config| async move {
            let counter = ctx.use_service::<AtomicUsize>("counter")?;
            let listener_ctx = ctx.clone();
            ctx.on_t::<AppReady, _, _>(move |message: &String| {
                let counter = counter.clone();
                let message = message.clone();
                let logger = listener_ctx.logger();
                async move {
                    let value = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    logger.info(format!("{message} #{}", value));
                    Flow::Continue
                }
            })?;
            Ok(())
        }),
        json!({}),
    )?;
    incrementer.await_ready().await?;

    // emit (fire-and-forget) and serial (awaited) dispatch.
    root.emit_t::<AppReady>(&"started".to_string())?;
    root.serial_t::<AppReady>(&"started".to_string()).await?;

    println!("stats after startup: {:?}", root.memory_stats());
    println!("plugins: {}", root.plugin_count());

    // Dispose the whole harness: listeners, services, and fibers are freed.
    root.dispose().await?;
    println!("stats after dispose: {:?}", root.memory_stats());
    Ok(())
}
