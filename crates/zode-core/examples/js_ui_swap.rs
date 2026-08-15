//! 前端自举演示：agent 用 JS 写自己的前端（无需编译器），并在运行时
//! 换上去。rust 写的 headless 前端完成后，交棒给 JS 前端 v1 —— 它打印
//! 菜单、读一行 stdin、再交棒给 JS 前端 v2，v2 打印后退出。
//!
//! 交互运行（可以输入命令，比如 next / hello）:
//!   cargo run -p zode-core --example js_ui_swap
//! 管道运行（确定性自测）:
//!   printf 'next\n' | cargo run -p zode-core --example js_ui_swap

use std::sync::Arc;

use async_trait::async_trait;
use cordis_rs::prelude::*;
use serde_json::json;
use zode_core::config::ZodeConfig;
use zode_core::js_ui::JsUi;
use zode_core::ui::{Ui, UiDeps, UiHost};

/// A rust frontend that just hands over to the JS frontend.
struct RustHeadless;

#[async_trait]
impl Ui for RustHeadless {
    fn id(&self) -> &'static str {
        "rust-headless"
    }

    async fn serve(&self, ctx: Context, _deps: Arc<UiDeps>) -> Result<(), CordisError> {
        println!("[rust-headless] turn done, handing over to js-v1");
        let _ = ctx.parallel_dyn("ui/swap", &json!({ "to": "js-v1" })).await;
        Ok(())
    }
}

/// The agent's first frontend draft, in JavaScript.
const JS_UI_V1: &str = r#"(function () {
  return {
    serve: function (host) {
      host.println("╭─ js-v1 (agent-written frontend) ─╮");
      host.println("│ commands: next -> v2, hello, quit│");
      host.println("╰──────────────────────────────────╯");
      var step = function (line) {
        if (line === null) { // EOF: hand over anyway
          host.println("(stdin closed, handing over)");
          host.swapTo("js-v2");
          return;
        }
        if (line === "next") { host.swapTo("js-v2"); return; }
        if (line === "quit") { host.exit(); return; }
        host.println("echo: " + line);
        host.readLine("js-v1> ", step);
      };
      host.readLine("js-v1> ", step);
    },
  };
})"#;

/// The agent's second frontend draft, also JavaScript.
const JS_UI_V2: &str = r#"(function () {
  return {
    serve: function (host) {
      host.println("[js-v2] replacement frontend mounted (no compiler involved)");
      host.setSkin(JSON.stringify({
        name: "js-v2-skin",
        description: "installed by the replacement frontend",
        colors: { accent: "141", bg_primary: "235", fg_text: "255" },
      }));
      host.exit();
    },
  };
})"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Context::root();
    let host = UiHost::new(
        &root,
        Arc::new(UiDeps {
            cwd: std::path::PathBuf::from("/tmp"),
            cfg: ZodeConfig::default(),
        }),
    )?;
    host.register(Arc::new(RustHeadless));
    host.register(Arc::new(JsUi::new("js-v1", JS_UI_V1)));
    host.register(Arc::new(JsUi::new("js-v2", JS_UI_V2)));

    println!("== registered UIs: {:?} ==", host.registered());
    host.run("rust-headless").await?;

    let skin = root.use_service::<Arc<zode_core::skin::SkinState>>("ui/skin")?;
    println!("== skin after the JS frontends ran: v{} ==", skin.version());
    assert!(skin.version() >= 1, "js-v2 must install its skin");
    println!("JS-UI-SWAP SELF-TEST PASSED");
    Ok(())
}
