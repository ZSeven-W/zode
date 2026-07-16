//! Opt-in end-to-end test against a REAL external agent CLI.
//!
//! Run with: `ZODE_EXTAGENT_IT=1 cargo test -p zode-core --test extagent_it -- --ignored`
//! Requires the `claude` CLI on PATH and a logged-in session; costs real
//! tokens. Kept `#[ignore]`d so plain `cargo test --workspace` never touches
//! the network.

use std::sync::Arc;
use std::time::Duration;

use zode_core::external_agents::{discover, runner::run_external, runner::RunSpec};

#[tokio::test]
#[ignore = "requires a real external CLI; run with ZODE_EXTAGENT_IT=1"]
async fn real_claude_one_shot() {
    if std::env::var("ZODE_EXTAGENT_IT").is_err() {
        eprintln!("ZODE_EXTAGENT_IT not set; skipping");
        return;
    }
    let cfg = zode_core::config::ExternalAgentsConfig::default();
    let reg = Arc::new(discover(&cfg, &[]));
    let Some(def) = reg.get("claude-code") else {
        panic!("claude CLI not found on PATH");
    };
    let cwd = tempfile::tempdir().unwrap();
    let spec = RunSpec {
        def: def.clone(),
        prompt: "输出单词 pong，不要做任何其他事。".to_string(),
        cwd: cwd.path().to_path_buf(),
        timeout: Duration::from_secs(300),
        extra_args: vec![],
        file_cache: None,
    };
    let out = run_external(spec, |_| {}, agent::abort::AbortController::new())
        .await
        .expect("external run should succeed");
    assert!(
        out.result.text.to_lowercase().contains("pong"),
        "unexpected result: {}",
        out.result.text
    );
    assert!(
        out.result.session_id.is_some(),
        "session id must be captured"
    );
}
