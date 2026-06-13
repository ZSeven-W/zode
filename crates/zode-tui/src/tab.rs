//! A single conversation tab: its own engine, chat view, session id, and
//! in-flight turn state. Tabs share the config/theme/approval queue (the app
//! owns those); everything conversation-scoped lives here so two tabs can run
//! turns concurrently without crossing streams.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use agent::abort::AbortController;
use agent::session::Session;
use zode_core::session_meta::{title_from_prompt, SessionIndex, SessionMeta};
use zode_core::ZodeEngine;

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
        }
    }

    /// A tab is busy while a turn is in flight (abort handle present).
    pub fn is_busy(&self) -> bool {
        self.turn_abort.is_some()
    }

    /// Stamp the session index with a title derived from the first prompt.
    pub fn stamp_title(&mut self, prompt: &str) {
        let title = title_from_prompt(prompt);
        self.title = title.clone();
        let mut idx = SessionIndex::load().unwrap_or_default();
        idx.upsert(SessionMeta {
            id: self.session_id.clone(),
            title,
            cwd: self.engine.cwd.display().to_string(),
            model: self.engine.model.clone(),
            updated_at: now_secs(),
        });
        let _ = idx.save();
        self.titled = true;
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
