//! 皮肤热替换自测：zode 运行中，agent（QuickJS 基因）自己写皮肤并实时
//! 换肤 —— 无需重启。前端（这里是模拟渲染器）轮询皮肤服务的版本号，
//! 变了就重渲染；真实 TUI 在 draw() 里做同样的事。
//!
//! Run with: cargo run -p zode-core --example skin_swap

use std::sync::Arc;

use cordis_rs::prelude::*;
use serde_json::json;
use zode_core::js_plugin::JsPlugin;
use zode_core::skin::SkinState;

const SKIN_A: &str = r#"{
  "name": "neon-midnight",
  "description": "agent skin A",
  "colors": { "accent": "199", "bg_primary": "232", "fg_text": "255" }
}"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Context::root();
    let skins: Arc<SkinState> = SkinState::new();
    root.provide("ui/skin", skins.clone())?;

    // The renderer frontends use: poll version, re-render on change.
    let render = |label: &str| {
        let skin = skins.current().unwrap_or_else(|| "{}".to_string());
        let accent = serde_json::from_str::<serde_json::Value>(&skin)
            .ok()
            .and_then(|v| {
                v.pointer("/colors/accent")
                    .and_then(|a| a.as_str().map(str::to_string))
            })
            .unwrap_or_else(|| "?".to_string());
        println!(
            "[{label}] render skin v{} (accent={accent})",
            skins.version()
        );
    };
    render("frame 1: no skin installed");

    // 1. A Rust-side installer swaps the skin at runtime.
    skins.install(SKIN_A)?;
    render("frame 2: after rust install");

    // 2. THE REAL LOOP: an agent-written JS gene writes its own skin and
    // installs it while zode runs.
    let gene_source = r#"(function () {
  return {
    apply: function (host) {
      host.log("info", "gene designing a skin...");
      host.setSkin(JSON.stringify({
        name: "paper-dawn",
        description: "agent skin B",
        colors: { accent: "114", bg_primary: "255", fg_text: "235" },
      }));
      host.log("info", "skin installed");
    },
  };
})"#;
    let gene = root.plugin(JsPlugin::new("skin-designer", gene_source), json!({}))?;
    gene.await_ready().await?;
    // Let the pump deliver the SetSkin message.
    for _ in 0..100 {
        if skins.version() >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    render("frame 3: after agent-written JS gene installed its skin");

    assert!(skins.version() >= 2, "JS gene must hot-swap the skin");
    println!("SKIN-SWAP SELF-TEST PASSED");
    Ok(())
}
