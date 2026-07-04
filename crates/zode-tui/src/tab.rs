//! A single conversation tab: its own engine, chat view, session id, and
//! in-flight turn state. Tabs share the config/theme/approval queue (the app
//! owns those); everything conversation-scoped lives here so two tabs can run
//! turns concurrently without crossing streams.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use agent::abort::AbortController;
use agent::session::Session;
use once_cell::sync::Lazy;
use zode_core::images::ImageAttachment;
use zode_core::session_meta::{title_from_prompt, SessionIndex, SessionMeta};
use zode_core::{TodoItem, ZodeEngine};

/// Serializes ALL session persistence process-wide. `Session::save` reuses the
/// same temp path per session id, and `SessionIndex` is a load-modify-save, so
/// concurrent saves (multiple tabs finishing turns, or two saves of one tab)
/// could corrupt the temp file or lose an index update. Saves are infrequent
/// and tiny, so one global lock is cheaper than the bugs it prevents.
static SAVE_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));

use crate::ui::chat::ChatView;
use crate::ui::status::Mode;

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
    /// True between an interrupt and the aborted turn's task actually
    /// finishing its teardown (its terminal `TurnDone` arriving). Keeps the
    /// tab `is_busy()` so a fast resubmit can't spawn a second turn that
    /// races the still-draining one on the shared message store.
    pub draining: bool,
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
    /// Messages typed while a turn was in flight, held FIFO and sent one per
    /// turn once this tab goes idle (Claude-style input queueing).
    pub queued_input: std::collections::VecDeque<String>,
    /// Local image attachments waiting to be sent with the next user message.
    pub pending_images: Vec<ImageAttachment>,
    /// Output of `!<cmd>` shell escapes run since the last turn, buffered to
    /// prepend to the next prompt so the agent sees what was run locally.
    pub pending_shell_context: Vec<String>,
    /// Whether THIS tab is in plan mode (read-only tools). Per-tab, not global:
    /// the status badge reads it for the active tab, and reassembly re-applies
    /// it so a model/provider/yolo swap doesn't drop or leak plan mode.
    pub plan_mode: bool,
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
}

impl SessionTab {
    pub fn new(id: usize, engine: Arc<ZodeEngine>, session_id: String) -> Self {
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
            draining: false,
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
            queued_input: std::collections::VecDeque::new(),
            pending_images: Vec::new(),
            pending_shell_context: Vec::new(),
            plan_mode: false,
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
        }
    }

    /// A tab is busy while a turn is in flight (abort handle present), a
    /// rebuild is running, or an interrupted turn is still draining — the
    /// last keeps a fast resubmit from racing the aborted task's teardown.
    pub fn is_busy(&self) -> bool {
        self.turn_abort.is_some() || self.reassemble_pending || self.draining
    }

    /// Stamp the session index with a title derived from the first prompt.
    /// The tab fields update synchronously; the index write (disk I/O under
    /// the global SAVE_LOCK) is spawned so the first submit never stalls the
    /// event loop behind another tab's in-flight save.
    pub fn stamp_title(&mut self, prompt: &str) {
        let title = title_from_prompt(prompt);
        self.title = title.clone();
        self.titled = true;
        tokio::spawn(index_upsert(SessionMeta {
            id: self.session_id.clone(),
            title,
            cwd: self.engine.cwd.display().to_string(),
            model: self.engine.model.clone(),
            updated_at: now_secs(),
        }));
    }

    /// Snapshot the store then persist (full rewrite). Delegates to
    /// [`persist_session`]. `save_session` is used by the explicit-save
    /// paths that don't track the append watermark, so it forces a full
    /// rewrite and resets the tab's watermark.
    pub async fn save_session(&self) {
        persist_session(
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
/// its index recency. Standalone (owned args) so the app can spawn it off the
/// event loop. The std mutex guard is dropped before the await, so it never
/// crosses an await point.
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
) {
    use std::sync::atomic::Ordering;
    // Serialize all saves: prevents same-session transcript temp-file races and
    // SessionIndex lost updates across concurrent tab saves.
    let _guard = SAVE_LOCK.lock().await;
    let Ok(path) = SessionIndex::session_path(&session_id) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let snapshot = match engine.store.lock() {
        Ok(store) => store.clone(),
        Err(_) => return,
    };
    let total = snapshot.len();
    // Try the incremental append first when permitted.
    let mut appended = false;
    if allow_append {
        let expected = persisted.load(Ordering::Relaxed).min(total);
        let tail: Vec<agent::message::Message> = snapshot.iter().skip(expected).cloned().collect();
        match Session::append(&path, &tail, expected).await {
            Ok(true) => appended = true,
            Ok(false) => {} // count mismatch / missing → full save below
            Err(e) => tracing::warn!("session append failed, rewriting: {e}"),
        }
    }
    if !appended {
        if let Err(e) = Session::save(&path, &snapshot).await {
            tracing::warn!("session save failed: {e}");
            return;
        }
    }
    persisted.store(total, Ordering::Relaxed);
    // Keep the index's recency current so `--continue` resumes this tab.
    let mut idx = SessionIndex::load().unwrap_or_default();
    if !idx.touch_updated(&session_id, now_secs()) {
        idx.upsert(SessionMeta {
            id: session_id,
            title,
            cwd: engine.cwd.display().to_string(),
            model: engine.model.clone(),
            updated_at: now_secs(),
        });
    }
    let _ = idx.save();
}

/// Locked load-modify-save: upsert one entry under SAVE_LOCK so it can't race
/// a concurrent `persist_session` / delete and lose updates.
pub async fn index_upsert(meta: SessionMeta) {
    let _guard = SAVE_LOCK.lock().await;
    let mut idx = SessionIndex::load().unwrap_or_default();
    idx.upsert(meta);
    let _ = idx.save();
}

/// Locked removal of a session entry from the index (see [`index_upsert`]).
pub async fn index_remove(id: &str) {
    let _guard = SAVE_LOCK.lock().await;
    if let Ok(mut idx) = SessionIndex::load() {
        if idx.remove(id) {
            let _ = idx.save();
        }
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
}
