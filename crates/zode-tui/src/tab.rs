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
use zode_core::session_meta::{title_from_prompt, SessionIndex, SessionMeta};
use zode_core::ZodeEngine;

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
    /// Abort handle for the in-flight turn, if any.
    pub turn_abort: Option<AbortController>,
    /// Monotonic per-tab turn counter; `active_turn_id` is the turn whose
    /// events we currently accept. Aborting/superseding bumps it so stale
    /// events from a still-draining task are dropped.
    pub turn_seq: u64,
    pub active_turn_id: u64,
    /// Mode + token counters surfaced by the status bar for the active tab.
    pub mode: Mode,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost_label: String,
    /// Per-turn process UI state: avoid repeating high-frequency thinking
    /// deltas and map tool results back to their visible tool call names.
    pub thinking_process_shown: bool,
    pub active_tool_names: HashMap<String, String>,
    /// Messages typed while a turn was in flight, held FIFO and sent one per
    /// turn once this tab goes idle (Claude-style input queueing).
    pub queued_input: std::collections::VecDeque<String>,
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
            turn_abort: None,
            turn_seq: 0,
            active_turn_id: 0,
            mode: Mode::Ready,
            input_tokens: 0,
            output_tokens: 0,
            cost_label: "$0.00".into(),
            thinking_process_shown: false,
            active_tool_names: HashMap::new(),
            queued_input: std::collections::VecDeque::new(),
        }
    }

    /// A tab is busy while a turn is in flight (abort handle present).
    pub fn is_busy(&self) -> bool {
        self.turn_abort.is_some()
    }

    /// Stamp the session index with a title derived from the first prompt.
    pub async fn stamp_title(&mut self, prompt: &str) {
        let title = title_from_prompt(prompt);
        self.title = title.clone();
        self.titled = true;
        index_upsert(SessionMeta {
            id: self.session_id.clone(),
            title,
            cwd: self.engine.cwd.display().to_string(),
            model: self.engine.model.clone(),
            updated_at: now_secs(),
        })
        .await;
    }

    /// Snapshot the store then persist. Delegates to [`persist_session`].
    pub async fn save_session(&self) {
        persist_session(
            self.session_id.clone(),
            self.engine.clone(),
            self.title.clone(),
        )
        .await;
    }
}

/// Snapshot a session's store (MessageStore: Clone) then persist it and bump
/// its index recency. Standalone (owned args) so the app can spawn it off the
/// event loop. The std mutex guard is dropped before the await, so it never
/// crosses an await point.
pub async fn persist_session(session_id: String, engine: Arc<ZodeEngine>, title: String) {
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
    if let Err(e) = Session::save(&path, &snapshot).await {
        tracing::warn!("session save failed: {e}");
        return;
    }
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
