//! Live LSP smoke test against a real language server (clangd). Ignored by
//! default — it spawns clangd, which must be installed. Run explicitly:
//!
//!   cargo test -p zode-core --test lsp_live -- --ignored --nocapture
//!
//! Exercises the full stack: LspManager → lazily-spawned LspClient → the
//! `lsp_diagnostics` / `lsp_hover` / `lsp_symbols` tools over JSON-RPC. clangd
//! is used because it analyzes a single self-contained file without needing a
//! build system, so the test is fast and deterministic.

use std::collections::HashMap;
use std::sync::Arc;

use agent::tool::{Tool, ToolUseContext};
use serde_json::{json, Value};
use zode_core::config::{LspConfig, LspServerConfig};
use zode_core::lsp::{lsp_tools, LspManager};

fn tool<'a>(tools: &'a [Arc<dyn Tool>], name: &str) -> &'a Arc<dyn Tool> {
    tools.iter().find(|t| t.name() == name).expect("tool present")
}

#[tokio::test]
#[ignore]
async fn clangd_diagnostics_hover_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    // `greet` is fine; `bad` returns a string literal where an int is expected.
    std::fs::write(
        root.join("probe.c"),
        "int greet(void) {\n    int n = 42;\n    return n;\n}\n\nint bad(void) {\n    return \"oops\";\n}\n",
    )
    .unwrap();

    let mut servers = HashMap::new();
    servers.insert(
        "c".to_string(),
        LspServerConfig {
            command: "clangd".into(),
            args: vec!["--log=error".into()],
            extensions: vec!["c".into()],
        },
    );
    let mgr = Arc::new(LspManager::new(LspConfig { servers }, root.clone()));
    let tools = lsp_tools(&mgr);
    let ctx = ToolUseContext::new(root);

    // Diagnostics: also gives clangd time to parse before hover.
    let diags = tool(&tools, "lsp_diagnostics")
        .call(&ctx, json!({ "file": "probe.c" }))
        .await
        .expect("diagnostics call ok");
    println!("diagnostics: {}", serde_json::to_string_pretty(&diags).unwrap());
    assert!(diags.get("diagnostics").is_some(), "has a diagnostics array");

    // Hover over `greet` (line 0, the `g` of greet at column 4).
    let hover = tool(&tools, "lsp_hover")
        .call(&ctx, json!({ "file": "probe.c", "line": 0, "character": 4 }))
        .await
        .expect("hover call ok");
    println!("hover: {hover}");
    let text = hover.get("hover").and_then(Value::as_str).unwrap_or("");
    assert!(
        text.contains("greet") || text.contains("int"),
        "hover should mention the function or its type, got: {text:?}"
    );

    // Document symbols: greet + bad should both appear.
    let syms = tool(&tools, "lsp_symbols")
        .call(&ctx, json!({ "file": "probe.c" }))
        .await
        .expect("symbols call ok");
    println!("symbols: {}", serde_json::to_string_pretty(&syms).unwrap());
    let names: Vec<&str> = syms
        .get("symbols")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|s| s.get("name").and_then(Value::as_str)).collect())
        .unwrap_or_default();
    assert!(names.contains(&"greet"), "symbols include greet: {names:?}");
    assert!(names.contains(&"bad"), "symbols include bad: {names:?}");
}

/// Auto-install mechanism: with no server preinstalled, `install::ensure`
/// should `npm install` the server into a throwaway `<config>/lsp` dir and
/// resolve the runnable binary there. Needs npm + network. Run with:
///   cargo test -p zode-core --test lsp_live -- --ignored --nocapture
#[test]
#[ignore]
fn auto_installs_bash_language_server_via_npm() {
    use zode_core::lsp::install;

    let cfgdir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", cfgdir.path());

    let spec = install::spec_for_lang("bash").expect("bash spec exists");
    // Nothing is installed in the fresh cfgdir, so this performs a real
    // `npm install --prefix <cfgdir>/lsp bash-language-server`.
    let path = install::ensure(spec).expect("npm install + resolve bash-language-server");
    println!("resolved server at {}", path.display());
    assert!(path.exists(), "installed binary exists at {}", path.display());
    assert!(
        path.to_string_lossy()
            .contains("node_modules/.bin/bash-language-server"),
        "resolved to the managed npm bin: {}",
        path.display()
    );

    std::env::remove_var("ZODE_CONFIG_DIR");
}
