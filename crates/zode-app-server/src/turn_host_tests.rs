use std::time::Duration;

use agent::abort::AbortController;
use agent::error::AgentError;
use agent::stream::Event;
use futures_util::stream;
use tokio::sync::mpsc;
use zode_app_server_protocol::types::{ApprovalPolicy, Thread, ThreadStatus};
use zode_core::config::ConfigManager;

use crate::accumulator::{TurnAccumulator, TurnEndState};
use crate::session::SessionMsg;
use crate::turn_host::{drive_stream, EngineHost, TurnHost};

#[tokio::test]
async fn drive_stream_emits_turn_error_and_returns_failed() {
    let stream = stream::iter([
        Ok(Event::TextDelta { delta: "hi".into() }),
        Err(AgentError::other("stream broke")),
    ]);
    let mut accumulator = TurnAccumulator::new("thread", "turn");
    let (msgs, mut rx) = mpsc::channel(4);

    let state = drive_stream(Box::new(stream), &mut accumulator, &msgs).await;

    assert_eq!(
        state,
        TurnEndState::Failed {
            error: "stream broke".into()
        }
    );
    assert!(
        matches!(rx.recv().await, Some(SessionMsg::TurnEvent { notification })
        if notification.method == "item/agentMessage/delta")
    );
    assert!(
        matches!(rx.recv().await, Some(SessionMsg::TurnEvent { notification })
        if notification.method == "turn/error"
            && notification.params.as_ref().unwrap()["error"]["message"] == "stream broke")
    );
}

#[tokio::test]
async fn drive_stream_returns_completed_with_accumulated_usage() {
    let stream = stream::iter([
        Ok(Event::TextDelta { delta: "hi".into() }),
        Ok(Event::Usage {
            input_tokens: 2,
            output_tokens: 3,
            cache_read: 4,
            cache_create: 5,
        }),
    ]);
    let mut accumulator = TurnAccumulator::new("thread", "turn");
    let (msgs, _rx) = mpsc::channel(4);

    let state = drive_stream(Box::new(stream), &mut accumulator, &msgs).await;
    let outcome = accumulator.finish(state);

    assert_eq!(outcome.state, TurnEndState::Completed);
    assert_eq!(outcome.final_text, "hi");
    assert_eq!(outcome.usage.input_tokens, 2);
    assert_eq!(outcome.usage.output_tokens, 3);
    assert_eq!(outcome.usage.cache_read, 4);
    assert_eq!(outcome.usage.cache_create, 5);
}

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
        None,
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

    host.start_turn(&thread, "hi".into(), None, AbortController::new(), msgs)
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

/// Clones the thread's engine Arc just long enough to read its current
/// model name (the clone is dropped immediately after `.clone()`).
async fn engine_model(host: &EngineHost, thread_id: &str) -> Option<String> {
    host.engine_arc(thread_id).await.map(|e| e.model.clone())
}

#[tokio::test]
#[serial_test::serial]
async fn restore_model_survives_busy_engine_and_applies_on_retry() {
    let config_dir = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    // `apiKey` is a dummy value: this test never calls `engine.turn()` (which
    // would need it to actually talk to a provider), only `assemble_tab` /
    // `hot_swap_model`, which just build a provider object in memory.
    std::fs::write(
        config_dir.path().join("config.json"),
        r#"{"provider":{"type":"anthropic","model":"first","apiKey":"test-key"},"providers":{"alt":{"type":"anthropic","model":"second","apiKey":"test-key"}}}"#,
    )
    .unwrap();
    let _config_dir_guard = EnvVarGuard::set("ZODE_CONFIG_DIR", config_dir.path());
    let _anthropic_key_guard = EnvVarGuard::remove("ANTHROPIC_API_KEY");
    let _openai_key_guard = EnvVarGuard::remove("OPENAI_API_KEY");

    let cfg = ConfigManager::load(cwd.path()).unwrap();
    let (_turn_ids, turn_ids_rx) = mpsc::unbounded_channel();
    let mut host = EngineHost::new(
        cfg,
        cwd.path().to_path_buf(),
        None,
        "2026-07-11".into(),
        ApprovalPolicy::Auto,
        turn_ids_rx,
        None,
    );
    let thread_id = "thread-restore";

    // Assemble a real engine for the thread (model "first"), then hot-swap
    // it to "second" via the normal `set_model` path, and finally seed a
    // `restore_models` entry as if a turn-level override had just recorded
    // "first" as the model to restore afterward. This reaches the exact
    // state `restore_model` operates on, without a real network turn (whose
    // provider retry-with-backoff on transport failure can take much longer
    // than a unit test should).
    host.assemble_engine_for_test(thread_id, cwd.path()).await;
    host.set_model(thread_id, "second").await.unwrap();
    host.set_restore_pending_for_test(thread_id, "first").await;

    assert_eq!(
        host.restore_model_pending(thread_id).await.as_deref(),
        Some("first")
    );
    assert_eq!(
        engine_model(&host, thread_id).await.as_deref(),
        Some("second")
    );

    // Hold a second Arc clone alive, simulating a concurrent user of the
    // engine (e.g. a post-turn extraction task), so `Arc::get_mut` inside
    // `restore_model` cannot get exclusive access.
    let busy_clone = host.engine_arc(thread_id).await.unwrap();

    host.restore_model(thread_id).await;

    // A busy restore attempt must not discard the retry data, and must not
    // have touched the live model.
    assert_eq!(
        host.restore_model_pending(thread_id).await.as_deref(),
        Some("first"),
        "a busy restore attempt must leave the pending entry in place for a later retry"
    );
    assert_eq!(
        engine_model(&host, thread_id).await.as_deref(),
        Some("second")
    );

    drop(busy_clone);

    // Retrying once the engine is no longer busy must apply the preserved
    // restore and clear the pending entry.
    host.restore_model(thread_id).await;

    assert_eq!(host.restore_model_pending(thread_id).await, None);
    assert_eq!(
        engine_model(&host, thread_id).await.as_deref(),
        Some("first")
    );
}

#[tokio::test]
#[serial_test::serial]
async fn set_model_rejects_unknown_model() {
    let config_dir = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    std::fs::write(
        config_dir.path().join("config.json"),
        r#"{"provider":{"type":"anthropic","model":"known"}}"#,
    )
    .unwrap();
    let _config_dir_guard = EnvVarGuard::set("ZODE_CONFIG_DIR", config_dir.path());
    let cfg = ConfigManager::load(cwd.path()).unwrap();
    let (_turn_ids, turn_ids_rx) = mpsc::unbounded_channel();
    let mut host = EngineHost::new(
        cfg,
        cwd.path().to_path_buf(),
        None,
        "2026-07-11".into(),
        ApprovalPolicy::Auto,
        turn_ids_rx,
        None,
    );

    let error = host
        .set_model("never-assembled", "unknown")
        .await
        .unwrap_err();

    assert_eq!(error.code, zode_app_server_protocol::rpc::INVALID_PARAMS);
}

#[tokio::test]
#[serial_test::serial]
async fn set_model_records_pending_model_for_unassembled_thread() {
    let config_dir = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    std::fs::write(
        config_dir.path().join("config.json"),
        r#"{"provider":{"type":"anthropic","model":"first"},"providers":{"alt":{"type":"anthropic","model":"second"}}}"#,
    )
    .unwrap();
    let _config_dir_guard = EnvVarGuard::set("ZODE_CONFIG_DIR", config_dir.path());
    let cfg = ConfigManager::load(cwd.path()).unwrap();
    let (_turn_ids, turn_ids_rx) = mpsc::unbounded_channel();
    let mut host = EngineHost::new(
        cfg,
        cwd.path().to_path_buf(),
        None,
        "2026-07-11".into(),
        ApprovalPolicy::Auto,
        turn_ids_rx,
        None,
    );

    host.set_model("never-assembled", "second").await.unwrap();

    assert_eq!(
        host.pending_model("never-assembled").await.as_deref(),
        Some("second")
    );
}
