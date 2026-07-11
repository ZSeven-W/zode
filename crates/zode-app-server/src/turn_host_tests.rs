use std::time::Duration;

use agent::abort::AbortController;
use tokio::sync::mpsc;
use zode_app_server_protocol::types::{ApprovalPolicy, Thread, ThreadStatus};
use zode_core::config::ConfigManager;

use crate::accumulator::TurnEndState;
use crate::session::SessionMsg;
use crate::turn_host::{EngineHost, TurnHost};

/// RAII guard that saves an env var's prior value on construction and
/// restores it (set or remove) on drop, so a test panic never leaks a
/// mutated/removed var into later tests in the same binary.
struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[tokio::test]
#[serial_test::serial]
async fn engine_host_runs_turn_to_failed_without_credentials() {
    let config_dir = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    std::fs::write(
        config_dir.path().join("config.json"),
        r#"{"provider":{"type":"anthropic"}}"#,
    )
    .unwrap();
    let _config_dir_guard = EnvVarGuard::set("ZODE_CONFIG_DIR", config_dir.path());
    let _anthropic_key_guard = EnvVarGuard::remove("ANTHROPIC_API_KEY");
    let _openai_key_guard = EnvVarGuard::remove("OPENAI_API_KEY");

    let cfg = ConfigManager::load(cwd.path()).unwrap();
    let (turn_ids, turn_ids_rx) = mpsc::unbounded_channel();
    let mut host = EngineHost::new(
        cfg,
        cwd.path().to_path_buf(),
        None,
        "2026-07-11".into(),
        ApprovalPolicy::Auto,
        turn_ids_rx,
    );
    let thread = Thread {
        id: "thread-real".into(),
        name: "test".into(),
        cwd: cwd.path().to_string_lossy().into_owned(),
        model: String::new(),
        status: ThreadStatus::Loaded,
    };
    let (msgs, mut rx) = mpsc::channel(8);
    turn_ids.send("turn-real".into()).unwrap();

    host.start_turn(&thread, "hi".into(), AbortController::new(), msgs)
        .await;

    let finished = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(SessionMsg::TurnFinished {
                thread_id,
                turn_id,
                outcome,
            }) = rx.recv().await
            {
                break (thread_id, turn_id, outcome);
            }
        }
    })
    .await
    .expect("engine turn did not finish within 10 seconds");

    assert_eq!(finished.0, "thread-real");
    assert_eq!(finished.1, "turn-real");
    assert!(matches!(finished.2.state, TurnEndState::Failed { .. }));
}
