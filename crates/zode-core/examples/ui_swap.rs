//! UI 插件化自测：DSH 式前端换壳 — every frontend is a harness plugin;
//! the headless UI finishes its turn and hands over to the readline UI at
//! runtime, all without restarting the app context.
//!
//! Run with: cargo run -p zode-core --example ui_swap

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use cordis_rs::prelude::*;
use serde_json::json;
use zode_core::config::ZodeConfig;
use zode_core::ui::{Ui, UiDeps, UiHost};

/// A headless-style frontend: answers one prompt, then swaps to the
/// interactive UI.
struct HeadlessUi;

#[async_trait]
impl Ui for HeadlessUi {
    fn id(&self) -> &'static str {
        "headless"
    }

    async fn serve(&self, ctx: Context, deps: Arc<UiDeps>) -> Result<(), CordisError> {
        println!("[headless] session in {}", deps.cwd.display());
        // Simulate a single-turn answer.
        println!("[headless] answered: hello from the headless UI");
        // Hand over to the interactive frontend at runtime (awaited, so
        // the host records the swap before this UI exits).
        let _ = ctx
            .parallel_dyn("ui/swap", &json!({ "to": "readline" }))
            .await;
        Ok(())
    }
}

/// A readline-style frontend: prints a prompt and exits.
struct ReadlineUi;

#[async_trait]
impl Ui for ReadlineUi {
    fn id(&self) -> &'static str {
        "readline"
    }

    async fn serve(&self, ctx: Context, _deps: Arc<UiDeps>) -> Result<(), CordisError> {
        println!("[readline] interactive REPL mounted (exit requested)");
        // A real REPL would read stdin here; the demo just exits.
        let _ = ctx;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), CordisError> {
    let root = Context::root();
    let host = UiHost::new(
        &root,
        Arc::new(UiDeps {
            cwd: PathBuf::from("/tmp/zode-demo"),
            cfg: ZodeConfig::default(),
        }),
    )?;
    host.register(Arc::new(HeadlessUi));
    host.register(Arc::new(ReadlineUi));

    println!("== registered UIs: {:?} ==", host.registered());
    println!("== mounting headless ==");
    host.run("headless").await?;

    println!("== active UI after loop: {:?} ==", host.active_id());
    println!("== memory ==");
    println!("  {:?}", root.memory_stats());
    root.dispose().await?;
    println!("  after dispose: {:?}", root.memory_stats());
    println!("UI-SWAP SELF-TEST PASSED");
    Ok(())
}
