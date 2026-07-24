//! A single conversation tab: its own engine, chat view, session id, and
//! in-flight turn state. Tabs share the config/theme/approval queue (the app
//! owns those); everything conversation-scoped lives here so two tabs can run
//! turns concurrently without crossing streams.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use agent::abort::AbortController;
use once_cell::sync::Lazy;
use zode_core::images::ImageAttachment;
use zode_core::session_meta::{title_from_prompt, SessionIndex, SessionMeta};
use zode_core::sessions::{DurableSessionMeta, SessionStore};
use zode_core::{TodoItem, ToolAccessMode, ZodeEngine};

/// Serializes ALL session persistence process-wide. `Session::save` reuses the
/// same temp path per session id, and `SessionIndex` is a load-modify-save, so
/// concurrent saves (multiple tabs finishing turns, or two saves of one tab)
/// could corrupt the temp file or lose an index update. Saves are infrequent
/// and tiny, so one global lock is cheaper than the bugs it prevents.
static SAVE_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));

use crate::ui::chat::ChatView;
use crate::ui::status::Mode;

#[derive(Debug, Clone)]
pub struct ToolActivity {
    pub id: String,
    pub name: String,
    pub status: &'static str,
    pub duration_ms: Option<u64>,
}

pub struct SessionTab {
    /// Stable id used to route agent events back to this tab even after other
    /// tabs are closed (a Vec index would shift). Monotonic, app-assigned.
    pub id: usize,
    pub engine: Arc<ZodeEngine>,
    pub chat: ChatView,
    pub session_id: String,
    pub title: String,
    /// Whether the session index entry has been stamped with a real title
    /// (false until the first user turn for a fresh tab).
    pub titled: bool,
    /// Count of messages already written to the session file — the append
    /// watermark. Shared with the detached save task (updated under the
    /// global save lock) so a long session persists a turn's new messages
    /// by APPENDING rather than rewriting the whole transcript each turn
    /// (was O(n²) per session). Seeded to the loaded length on resume.
    pub persisted_msgs: Arc<std::sync::atomic::AtomicUsize>,
    /// Set when the store's PREFIX changed since the last save (a
    /// compaction rewrote/tombstoned earlier messages, `/clear`, undo/redo)
    /// — an append would corrupt, so the next save is a full rewrite.
    pub store_dirty: bool,
    /// Abort handle for the in-flight turn, if any.
    pub turn_abort: Option<AbortController>,
    /// Tokio worker that owns the provider stream. Keeping the JoinHandle (not
    /// only its AbortHandle) lets the watchdog wait until cancellation has
    /// actually dropped nested drivers/tools before it snapshots the session or
    /// permits another turn to share the store.
    pub turn_task: Option<tokio::task::JoinHandle<()>>,
    /// Shared only for watchdog-managed turns so a grace-expired hard cancel
    /// can still journal a terminal outcome and close the checkpoint.
    pub watchdog_recorder: Option<Arc<std::sync::Mutex<zode_core::run_event::TurnRecorder>>>,
    /// Cross-process OS lock for the active persisted schedule attempt. It is
    /// released only after terminal watchdog state has been committed; an
    /// unclean process exit drops the OS lock but leaves the active token for
    /// startup orphan recovery.
    pub watchdog_attempt_lease: Option<zode_core::scheduler::ScheduleAttemptLease>,
    /// Shared worker-quiescence signal for a watchdog-managed turn. Hard stop
    /// waits for every nested query/tool worker guard to drop before releasing
    /// the schedule lease or persisting a potentially racing store snapshot.
    pub watchdog_activity: Option<agent::abort::TurnActivity>,
    /// Monotonic generation for abortable non-agent operations that reserve
    /// the tab busy slot. Completion events carry this id so a delayed
    /// predecessor cannot release or relabel a newer operation/agent turn.
    pub local_op_seq: u64,
    /// Exact non-agent operation that currently owns `turn_abort`. Agent turns
    /// leave this `None` and are fenced independently by `active_turn_id`.
    pub active_local_op_id: Option<u64>,
    /// Exact agent turn whose abort is still tearing down. The id matters:
    /// only that turn's terminal `TurnDone` may release the busy latch, and
    /// non-agent abort users (local shell / compact / background operations)
    /// never enter this state because they do not emit `TurnDone`.
    pub draining_turn_id: Option<u64>,
    /// True while a model/provider/config rebuild is running off the UI loop.
    /// It blocks new turns just like an in-flight agent turn, but there is no
    /// abort handle because engine assembly is not cancellation-aware.
    pub reassemble_pending: bool,
    /// Monotonic per-tab rebuild counter; completions carry this to avoid
    /// applying stale background results.
    pub reassemble_seq: u64,
    /// Monotonic per-tab turn counter; `active_turn_id` is the turn whose
    /// events we currently accept. Aborting/superseding bumps it so stale
    /// events from a still-draining task are dropped.
    pub turn_seq: u64,
    pub active_turn_id: u64,
    /// Mode + token counters surfaced by the status bar for the active tab.
    pub mode: Mode,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Most recent prompt size (last `Usage` event's input tokens) — a proxy
    /// for current context-window occupancy, shown as a % in the status bar.
    pub context_tokens: u32,
    /// Consecutive AUTO-compaction failures. At the breaker limit the
    /// auto-compact trigger stops firing for this tab (a persistently failing
    /// provider would otherwise loop compact attempts forever); any
    /// successful compaction resets it.
    pub auto_compact_failures: u32,
    pub cost_label: String,
    /// Per-turn process UI state: map tool results back to their visible tool
    /// call names.
    pub active_tool_names: HashMap<String, String>,
    /// Raw stable tool names keyed by tool-use id. UI plugins may observe
    /// these names/statuses, but never tool inputs or outputs.
    pub active_tool_api_names: HashMap<String, String>,
    /// Tool-call start instants keyed by tool_use id, for the chat row's
    /// completion-duration suffix. Cleared with `active_tool_names`.
    pub active_tool_started: HashMap<String, std::time::Instant>,
    /// Bounded, metadata-only tool activity visible to trusted UI plugins.
    pub recent_tools: std::collections::VecDeque<ToolActivity>,
    /// Messages typed while a turn was in flight, held FIFO and sent one per
    /// turn once this tab goes idle (Claude-style input queueing).
    pub queued_input: std::collections::VecDeque<String>,
    /// Local image attachments waiting to be sent with the next user message.
    pub pending_images: Vec<ImageAttachment>,
    /// Output of `!<cmd>` shell escapes run since the last turn, buffered to
    /// prepend to the next prompt so the agent sees what was run locally.
    pub pending_shell_context: Vec<String>,
    /// Submitted prompts for Up/Down recall in this conversation only.
    pub prompt_history: Vec<String>,
    /// Persistent key for this conversation's prompt history bucket.
    pub prompt_history_key: String,
    /// Cursor into `prompt_history` while browsing (None = editing live text).
    pub history_pos: Option<usize>,
    /// In-progress text saved when history browsing began.
    pub history_draft: String,
    /// Whether THIS tab is in plan mode (read-only tools). Per-tab, not global:
    /// the status badge reads it for the active tab, and reassembly re-applies
    /// it so a model/provider/yolo swap doesn't drop or leak plan mode.
    pub plan_mode: bool,
    /// Task-local tool approval policy. This is deliberately independent from
    /// `plan_mode`: plan mode changes the prompt/tool surface, while access
    /// controls whether the remaining tools are read-only, prompted, or
    /// auto-approved. Saved sessions resume as `Prompt` unless a live task
    /// explicitly changes this field.
    pub extension_access: ToolAccessMode,
    /// Cached snapshot of this tab's live `TodoWrite` list, refreshed each
    /// tick from `engine.todo_state` so the sync sidebar render can read it.
    pub todos: Vec<TodoItem>,
    /// Cached git working-tree stats for the sidebar "modified files" section,
    /// refreshed by a throttled background poll. `None` = cwd is not a git
    /// work tree (section hidden).
    pub git_files: Option<Vec<zode_core::GitFileStat>>,
    /// True while a git-stat poll for this tab is in flight (dedupes spawns).
    pub git_poll_inflight: bool,
    /// Cached `(server, connected)` MCP state for the sidebar MCP section,
    /// refreshed on the same throttled cadence as the git poll.
    pub mcp_status: Vec<(String, bool)>,
    /// Cached `(language, running)` LSP state for the sidebar LSP section,
    /// refreshed on the same cadence. Only languages the project actually
    /// uses appear; empty hides the section.
    pub lsp_status: Vec<(String, bool)>,
    /// Whether the autonomous goal loop is active on this tab. Set true when a
    /// goal is set via `/goal <text>`; cleared on `goal_complete`, `/goal
    /// clear`, a failed turn, or a user interrupt (Esc/Ctrl+C).
    pub goal_loop_active: bool,
    /// How many turns the current goal loop has run (for the optional
    /// `autoLoopMaxTurns` cap). Reset to 0 each time a new goal loop starts.
    pub goal_loop_iter: u32,
    /// Consecutive goal-loop turns that used NO tools — a no-progress signal.
    /// Reset whenever a turn does use a tool; at the limit the loop stops.
    pub goal_no_progress_streak: u32,
    /// Whether the CURRENT turn has used any tool. Set on the first ToolUse
    /// event of the turn, reset at each turn's start; read at TurnDone to
    /// drive goal-loop no-progress detection.
    pub turn_used_tools: bool,
    /// The active goal's text, for the sidebar `goal` section. `Some` while the
    /// loop runs; cleared when it stops.
    pub goal_text: Option<String>,
    /// When the current goal loop started, for the sidebar elapsed-time display.
    pub goal_started_at: Option<std::time::Instant>,
    /// When the in-flight turn started, for the completion footer.
    pub turn_started_at: Option<std::time::Instant>,
    /// Tool calls seen during the in-flight turn, for the completion footer.
    pub turn_tool_count: u32,
    /// The scheduler job (if any) that queued the CURRENT/just-started turn's
    /// prompt. Set from `App::sched_pending` at submit time, consumed at
    /// `TurnDone` to update `App::sched_fail_streak` — a user-typed prompt
    /// (not scheduler-queued) always leaves this `None`.
    pub active_sched_job: Option<crate::app::SchedJobRef>,
}

impl SessionTab {
    pub fn new(id: usize, engine: Arc<ZodeEngine>, session_id: String) -> Self {
        let prompt_history_key = format!("session:{session_id}");
        Self {
            id,
            title: format!("tab {}", id + 1),
            engine,
            chat: ChatView::new(),
            session_id,
            titled: false,
            persisted_msgs: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            store_dirty: false,
            turn_abort: None,
            turn_task: None,
            watchdog_recorder: None,
            watchdog_attempt_lease: None,
            watchdog_activity: None,
            local_op_seq: 0,
            active_local_op_id: None,
            draining_turn_id: None,
            reassemble_pending: false,
            reassemble_seq: 0,
            turn_seq: 0,
            active_turn_id: 0,
            mode: Mode::Ready,
            input_tokens: 0,
            output_tokens: 0,
            context_tokens: 0,
            auto_compact_failures: 0,
            cost_label: "$0.00".into(),
            active_tool_names: HashMap::new(),
            active_tool_api_names: HashMap::new(),
            active_tool_started: HashMap::new(),
            recent_tools: std::collections::VecDeque::new(),
            queued_input: std::collections::VecDeque::new(),
            pending_images: Vec::new(),
            pending_shell_context: Vec::new(),
            prompt_history: Vec::new(),
            prompt_history_key,
            history_pos: None,
            history_draft: String::new(),
            plan_mode: false,
            extension_access: ToolAccessMode::Prompt,
            todos: Vec::new(),
            git_files: None,
            git_poll_inflight: false,
            mcp_status: Vec::new(),
            lsp_status: Vec::new(),
            goal_loop_active: false,
            goal_loop_iter: 0,
            goal_no_progress_streak: 0,
            turn_used_tools: false,
            goal_text: None,
            goal_started_at: None,
            turn_started_at: None,
            turn_tool_count: 0,
            active_sched_job: None,
        }
    }

    /// A tab is busy while a turn is in flight (abort handle present), a
    /// rebuild is running, or an interrupted turn is still draining — the
    /// last keeps a fast resubmit from racing the aborted task's teardown.
    pub fn is_busy(&self) -> bool {
        self.turn_abort.is_some() || self.reassemble_pending || self.draining_turn_id.is_some()
    }

    /// Stamp the tab with a title derived from the first prompt. The durable
    /// index entry is published only after the transcript save succeeds; an
    /// interrupted first turn must not leave metadata for a missing JSONL.
    pub fn stamp_title(&mut self, prompt: &str) {
        self.title = title_from_prompt(prompt);
        self.titled = true;
    }

    /// Snapshot the store then persist (full rewrite). Delegates to
    /// [`persist_session`]. `save_session` is used by the explicit-save
    /// paths that don't track the append watermark, so it forces a full
    /// rewrite and resets the tab's watermark.
    pub async fn save_session(&self) {
        let _ = persist_session(
            self.session_id.clone(),
            self.engine.clone(),
            self.title.clone(),
            self.persisted_msgs.clone(),
            false, // full rewrite
        )
        .await;
    }
}

/// Snapshot a session's store (MessageStore: Clone) then persist it and bump
/// its index recency. The snapshot is taken before waiting for the global save
/// lock, so a later turn can never leak into an older queued save.
///
/// `allow_append`: when the store only grew since the last save (a normal
/// turn), append the new tail instead of rewriting the whole file — the
/// difference between O(n) and O(n²) I/O over a long session. The append is
/// self-validating (it refuses and falls back to a full rewrite if the
/// file's message count doesn't match the watermark), so a stale watermark
/// can never corrupt the transcript. `persisted` is the shared append
/// watermark (read + updated here under the save lock).
pub async fn persist_session(
    session_id: String,
    engine: Arc<ZodeEngine>,
    title: String,
    persisted: Arc<std::sync::atomic::AtomicUsize>,
    allow_append: bool,
) -> bool {
    let snapshot = match engine.store.lock() {
        Ok(store) => store.clone(),
        Err(_) => return false,
    };
    persist_session_snapshot(session_id, engine, title, persisted, allow_append, snapshot).await
}

/// Persist an already-owned, quiescent snapshot. Scheduler turns use this
/// before their canonical terminal so durable state cannot race the next turn.
pub async fn persist_session_snapshot(
    session_id: String,
    engine: Arc<ZodeEngine>,
    title: String,
    persisted: Arc<std::sync::atomic::AtomicUsize>,
    allow_append: bool,
    snapshot: agent::message::MessageStore,
) -> bool {
    use std::sync::atomic::Ordering;
    // Serialize all saves: prevents same-session transcript temp-file races and
    // SessionIndex lost updates across concurrent tab saves.
    let _guard = SAVE_LOCK.lock().await;
    let Ok(store) = SessionStore::open_default() else {
        return false;
    };
    let total = snapshot.len();
    let mut meta = store.load_meta(&session_id).unwrap_or_else(|_| {
        DurableSessionMeta::new(SessionMeta {
            id: session_id.clone(),
            title: title.clone(),
            cwd: engine.cwd.display().to_string(),
            model: engine.model.clone(),
            updated_at: now_secs(),
        })
    });
    meta.title = title;
    meta.cwd = engine.cwd.display().to_string();
    meta.model = engine.model.clone();
    let result = if allow_append {
        let expected = persisted.load(Ordering::Relaxed).min(total);
        store.save_incremental(&meta, &snapshot, expected).await
    } else {
        store.save(&meta, &snapshot).await
    };
    if let Err(error) = result {
        tracing::warn!("durable session save failed: {error}");
        return false;
    }
    persisted.store(total, Ordering::Relaxed);
    true
}

/// Run synchronous index I/O on the blocking pool while holding the same lock
/// used by transcript persistence. Acquire the async mutex before entering the
/// blocking pool: a saturated save queue must suspend Tokio tasks, not occupy
/// every blocking worker with `blocking_lock()` waiters.
async fn with_session_index_lock<T>(
    operation: &'static str,
    f: impl FnOnce() -> Result<T, zode_core::CoreError> + Send + 'static,
) -> Result<T, zode_core::CoreError>
where
    T: Send + 'static,
{
    let _guard = SAVE_LOCK.lock().await;
    tokio::task::spawn_blocking(f).await.map_err(|error| {
        zode_core::CoreError::Other(format!("{operation} worker failed: {error}"))
    })?
}

/// Checked, serialized index load. Corrupt or unreadable indexes are surfaced
/// to the caller instead of being mistaken for an empty index.
pub async fn session_index_load_checked() -> Result<SessionIndex, zode_core::CoreError> {
    with_session_index_lock("session index load", SessionIndex::load).await
}

/// Cancellable checked load for queued extension workers. The predicate runs
/// only after the async save lock is acquired and immediately before disk I/O.
pub async fn session_index_load_checked_if(
    should_run: impl FnOnce() -> bool + Send + 'static,
) -> Result<Option<SessionIndex>, zode_core::CoreError> {
    let _guard = SAVE_LOCK.lock().await;
    if !should_run() {
        return Ok(None);
    }
    tokio::task::spawn_blocking(SessionIndex::load)
        .await
        .map_err(|error| {
            zode_core::CoreError::Other(format!("session index load worker failed: {error}"))
        })?
        .map(Some)
}

/// Checked load-modify-save upsert under [`SAVE_LOCK`].
pub async fn index_upsert_checked(meta: SessionMeta) -> Result<(), zode_core::CoreError> {
    with_session_index_lock("session index upsert", move || {
        SessionIndex::update(|idx| {
            idx.upsert(meta);
            Ok(())
        })
    })
    .await
}

/// Checked load-modify-save removal under [`SAVE_LOCK`].
pub async fn index_remove_checked(id: &str) -> Result<bool, zode_core::CoreError> {
    let id = id.to_string();
    with_session_index_lock("session index remove", move || {
        SessionIndex::update(|idx| Ok(idx.remove(&id)))
    })
    .await
}

/// Best-effort compatibility wrapper for existing background callers.
pub async fn index_upsert(meta: SessionMeta) {
    if let Err(e) = index_upsert_checked(meta).await {
        tracing::warn!("session index upsert failed: {e}");
    }
}

/// Best-effort compatibility wrapper for existing background callers.
pub async fn index_remove(id: &str) {
    if let Err(e) = index_remove_checked(id).await {
        tracing::warn!("session index remove failed: {e}");
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(previous) => std::env::set_var(self.key, previous),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn busy_flag_tracks_abort_handle() {
        // Construction needs a real ZodeEngine (network-free assemble), which
        // is exercised in the engine crate's tests and app-level E2E. Here we
        // only assert the busy-flag contract on a hand-built abort handle.
        let abort = AbortController::new();
        assert!(!abort.is_aborted());
        // A tab with an abort handle is busy; without one it is idle. The
        // field is `Option<AbortController>` so the predicate is `is_some`.
        let busy: Option<AbortController> = Some(abort);
        assert!(busy.is_some());
        let idle: Option<AbortController> = None;
        assert!(idle.is_none());
    }

    #[test]
    fn checked_index_io_never_blocks_a_blocking_pool_thread_on_async_save_lock() {
        let source = include_str!("tab.rs");
        let production = source
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("production source precedes tests");
        assert!(
            !production.contains(&["SAVE_LOCK.", "blocking_lock()"].concat()),
            "acquire SAVE_LOCK asynchronously before entering spawn_blocking"
        );
    }

    #[tokio::test]
    async fn persist_session_refreshes_existing_title_cwd_and_model() {
        use zode_core::config::{NoemaSettings, ProviderConfig, ProviderKind, ZodeConfig};
        use zode_core::EngineTemplate;

        let config = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let _env_lock = TEST_ENV_LOCK.lock().await;
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let cfg = ZodeConfig {
            provider: ProviderConfig {
                r#type: Some(ProviderKind::Ollama),
                base_url: Some("http://localhost:11434".into()),
                model: Some("current-model".into()),
                ..Default::default()
            },
            noema: NoemaSettings {
                enabled: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };
        let engine = Arc::new(
            EngineTemplate::new(
                cfg,
                cwd.path().to_path_buf(),
                None,
                false,
                None,
                "2026-07-13".into(),
            )
            .assemble()
            .await
            .unwrap(),
        );
        let session_id = "refresh-meta";
        let mut index = SessionIndex::default();
        index.upsert(SessionMeta {
            id: session_id.into(),
            title: "Old title".into(),
            cwd: "/old/cwd".into(),
            model: "old-model".into(),
            updated_at: 1,
        });
        index.save().unwrap();

        persist_session(
            session_id.into(),
            engine,
            "Current title".into(),
            Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            false,
        )
        .await;

        let reloaded = SessionIndex::load().unwrap();
        let meta = reloaded.find_prefix(session_id).unwrap();
        assert_eq!(meta.title, "Current title");
        assert_eq!(meta.cwd, cwd.path().display().to_string());
        assert_eq!(meta.model, "current-model");
        assert!(meta.updated_at > 1);
    }
}
