//! The harness never restarts — the compiled plugin does.
//!
//! This host loads `gene_v1` as a process-plugin fiber, dispatches a probe,
//! then **replaces it live**: dispose (kills the v1 process) → spawn
//! `gene_v2` → probe again. The host binary keeps running throughout.
//!
//! Run:
//!
//! \`\`\`sh
//! cargo build --examples -p cordis-rs
//! cargo run -p cordis-rs --example process_host
//! \`\`\`

use std::sync::Arc;

use cordis_rs::prelude::*;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), CordisError> {
    // The compiled plugin binaries sit next to the example host binary.
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .expect("exe dir");
    let gene_v1 = dir.join("gene_v1");
    let gene_v2 = dir.join("gene_v2");
    if !gene_v1.exists() || !gene_v2.exists() {
        eprintln!("missing plugin binaries — run: cargo build --examples -p cordis-rs");
        std::process::exit(1);
    }

    let root = Context::root();
    root.on_dyn("gene/result", |event| {
        println!("harness observed: {}", event.payload);
        async { Flow::Continue }
    })?;

    // Generation 1: load the compiled v1 binary as a gene.
    let gene = root.plugin(ProcessPlugin::new("gene", gene_v1.clone()), json!({}))?;
    gene.await_ready().await?;
    root.emit_dyn("probe", &json!({ "n": 1 }))?;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // LIVE REPLACEMENT: dispose the v1 fiber (kills its process), then load
    // the newly compiled v2 binary. The harness process never restarts.
    println!("--- replacing compiled plugin: gene_v1 -> gene_v2 ---");
    gene.dispose().await;

    let gene2 = root.plugin(ProcessPlugin::new("gene", gene_v2.clone()), json!({}))?;
    gene2.await_ready().await?;
    root.emit_dyn("probe", &json!({ "n": 2 }))?;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    println!("stats before shutdown: {:?}", root.memory_stats());
    root.dispose().await?; // also kills the v2 process
    println!("stats after shutdown: {:?}", root.memory_stats());
    let _ = Arc::new(());
    Ok(())
}
