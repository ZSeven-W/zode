//! Shared session persistence for CLI, TUI, and desktop front ends.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent::message::MessageStore;
use agent::session::Session;

use crate::persistence::AdvisoryFileLock;
use zode_core::session_meta::{validate_session_id, SessionIndex, SessionMeta};
use zode_core::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionWriteMode {
    Full,
    Append { expected_existing: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct SessionSaveReservation {
    pub generation: u64,
    pub write_mode: SessionWriteMode,
}

/// A complete in-memory snapshot ordered by a caller-assigned generation.
#[derive(Debug, Clone)]
pub struct SessionSave {
    pub meta: SessionMeta,
    pub store: MessageStore,
    pub write_mode: SessionWriteMode,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum SessionSaveOutcome {
    Saved { persisted_messages: usize },
    Superseded,
}

#[derive(Debug, Default)]
struct SessionGenerationState {
    latest: u64,
    full_required_through: Option<u64>,
}

/// Persists sessions exclusively beneath one explicit Zode config directory.
#[derive(Debug, Clone)]
pub struct SessionRepository {
    config_dir: PathBuf,
    generation_states: Arc<Mutex<HashMap<String, SessionGenerationState>>>,
}

impl SessionRepository {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            generation_states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Return indexed sessions newest first.
    pub fn list(&self) -> Result<Vec<SessionMeta>, CoreError> {
        let mut sessions = SessionIndex::load_from_path(&self.index_path())?.sessions;
        sessions.sort_by_key(|meta| std::cmp::Reverse(meta.updated_at));
        Ok(sessions)
    }

    /// Reserve a process-local generation and resolve its safe write mode.
    /// Callers must reserve before detaching or spawning the save request.
    pub fn reserve_save(
        &self,
        id: &str,
        requested: SessionWriteMode,
    ) -> Result<SessionSaveReservation, CoreError> {
        validate_session_id(id)?;
        let mut states = self
            .generation_states
            .lock()
            .map_err(|_| CoreError::Other("session generation state poisoned".to_string()))?;
        let state = states.entry(id.to_string()).or_default();
        let next = state
            .latest
            .checked_add(1)
            .ok_or_else(|| CoreError::Other(format!("session generation exhausted: {id}")))?;
        state.latest = next;
        let write_mode = if matches!(requested, SessionWriteMode::Full)
            || state.full_required_through.is_some()
        {
            state.full_required_through = Some(next);
            SessionWriteMode::Full
        } else {
            requested
        };
        Ok(SessionSaveReservation {
            generation: next,
            write_mode,
        })
    }

    pub async fn load(&self, id: &str) -> Result<MessageStore, CoreError> {
        let path = SessionIndex::session_path_in(&self.config_dir, id)?;
        let _lock = acquire_index_lock(self.index_path()).await?;
        Ok(Session::load(path).await?)
    }

    /// Create an indexed empty session while rejecting an existing identity.
    pub async fn create(&self, meta: SessionMeta) -> Result<MessageStore, CoreError> {
        validate_session_id(&meta.id)?;
        let index_path = self.index_path();
        let transcript_path = SessionIndex::session_path_in(&self.config_dir, &meta.id)?;
        let _lock = acquire_index_lock(index_path.clone()).await?;
        let mut index = SessionIndex::load_from_path(&index_path)?;
        if index.sessions.iter().any(|current| current.id == meta.id)
            || tokio::fs::try_exists(&transcript_path).await?
        {
            let id = &meta.id;
            return Err(CoreError::Other(format!("session already exists: {id}")));
        }

        let store = MessageStore::new();
        Session::save(&transcript_path, &store).await?;
        index.upsert(meta);
        if let Err(error) = index.save_to_path_locked(&index_path) {
            let _ = tokio::fs::remove_file(&transcript_path).await;
            return Err(error);
        }
        Ok(store)
    }

    /// Persist a transcript and its index entry under one index lock.
    ///
    /// The blocking cross-process lock acquisition runs off the async
    /// executor. The returned guard then covers transcript I/O and the entire
    /// index load-modify-write transaction.
    pub async fn save(&self, save: SessionSave) -> Result<SessionSaveOutcome, CoreError> {
        validate_session_id(&save.meta.id)?;
        let write_mode = self.register_save(&save.meta.id, save.generation, save.write_mode)?;
        let index_path = self.index_path();
        let transcript_path = SessionIndex::session_path_in(&self.config_dir, &save.meta.id)?;
        let _lock = acquire_index_lock(index_path.clone()).await?;
        if self.is_superseded(&save.meta.id, save.generation)? {
            return Ok(SessionSaveOutcome::Superseded);
        }

        // Refuse to overwrite a corrupt index after successfully publishing a
        // transcript. Missing remains the only default-index case.
        let mut index = SessionIndex::load_from_path(&index_path)?;
        let total = save.store.len();
        let appended = match write_mode {
            SessionWriteMode::Append { expected_existing } if expected_existing <= total => {
                let tail: Vec<_> = save.store.iter().skip(expected_existing).cloned().collect();
                match Session::append(&transcript_path, &tail, expected_existing).await {
                    Ok(appended) => appended,
                    Err(error) => {
                        tracing::warn!("session append failed, rewriting: {error}");
                        false
                    }
                }
            }
            SessionWriteMode::Append { .. } | SessionWriteMode::Full => false,
        };
        if !appended {
            Session::save(&transcript_path, &save.store).await?;
        }

        let id = save.meta.id.clone();
        if !index.touch_updated(&id, save.meta.updated_at) {
            index.upsert(save.meta);
        }
        index.save_to_path_locked(&index_path)?;
        if matches!(write_mode, SessionWriteMode::Full) {
            self.mark_full_saved(&id, save.generation)?;
        }
        Ok(SessionSaveOutcome::Saved {
            persisted_messages: total,
        })
    }

    pub async fn delete(&self, id: &str) -> Result<(), CoreError> {
        let transcript_path = SessionIndex::session_path_in(&self.config_dir, id)?;
        let index_path = self.index_path();
        let _lock = acquire_index_lock(index_path.clone()).await?;
        let mut index = SessionIndex::load_from_path(&index_path)?;

        match tokio::fs::remove_file(transcript_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(CoreError::Io(error)),
        }
        if index.remove(id) {
            index.save_to_path_locked(&index_path)?;
        }
        Ok(())
    }

    pub async fn rename(&self, id: &str, title: String) -> Result<(), CoreError> {
        validate_session_id(id)?;
        let index_path = self.index_path();
        let _lock = acquire_index_lock(index_path.clone()).await?;
        let mut index = SessionIndex::load_from_path(&index_path)?;
        let Some(meta) = index.sessions.iter_mut().find(|meta| meta.id == id) else {
            return Err(CoreError::Other(format!("session not found: {id}")));
        };
        meta.title = title;
        index.save_to_path_locked(&index_path)
    }

    /// Update the selected model under the same lock as transcript saves.
    pub async fn update_model(&self, id: &str, model: String) -> Result<(), CoreError> {
        validate_session_id(id)?;
        let index_path = self.index_path();
        let _lock = acquire_index_lock(index_path.clone()).await?;
        let mut index = SessionIndex::load_from_path(&index_path)?;
        let Some(meta) = index.sessions.iter_mut().find(|meta| meta.id == id) else {
            return Err(CoreError::Other(format!("session not found: {id}")));
        };
        meta.model = model;
        index.save_to_path_locked(&index_path)
    }

    fn index_path(&self) -> PathBuf {
        SessionIndex::index_path_in(&self.config_dir)
    }

    fn register_save(
        &self,
        id: &str,
        generation: u64,
        requested: SessionWriteMode,
    ) -> Result<SessionWriteMode, CoreError> {
        let mut states = self
            .generation_states
            .lock()
            .map_err(|_| CoreError::Other("session generation state poisoned".to_string()))?;
        let state = states.entry(id.to_string()).or_default();
        let already_superseded = generation < state.latest;
        state.latest = state.latest.max(generation);
        let needs_full = state.full_required_through.is_some()
            || (matches!(requested, SessionWriteMode::Full) && !already_superseded);
        if needs_full {
            if !already_superseded {
                state.full_required_through = Some(
                    state
                        .full_required_through
                        .unwrap_or(generation)
                        .max(generation),
                );
            }
            Ok(SessionWriteMode::Full)
        } else {
            Ok(requested)
        }
    }

    fn is_superseded(&self, id: &str, generation: u64) -> Result<bool, CoreError> {
        let states = self
            .generation_states
            .lock()
            .map_err(|_| CoreError::Other("session generation state poisoned".to_string()))?;
        Ok(states
            .get(id)
            .is_some_and(|state| state.latest > generation))
    }

    fn mark_full_saved(&self, id: &str, generation: u64) -> Result<(), CoreError> {
        let mut states = self
            .generation_states
            .lock()
            .map_err(|_| CoreError::Other("session generation state poisoned".to_string()))?;
        if let Some(state) = states.get_mut(id) {
            if state
                .full_required_through
                .is_some_and(|required| required <= generation)
            {
                state.full_required_through = None;
            }
        }
        Ok(())
    }
}

async fn acquire_index_lock(target: PathBuf) -> Result<AdvisoryFileLock, CoreError> {
    tokio::task::spawn_blocking(move || AdvisoryFileLock::acquire(&target))
        .await
        .map_err(|error| CoreError::Other(format!("session lock worker failed: {error}")))?
}

#[cfg(test)]
mod explicit_metadata_tests {
    use super::*;

    fn meta(id: &str, model: &str, updated_at: u64) -> SessionMeta {
        SessionMeta {
            id: id.to_string(),
            title: "title".to_string(),
            cwd: "/workspace".to_string(),
            model: model.to_string(),
            updated_at,
        }
    }

    #[tokio::test]
    async fn explicit_model_update_survives_a_stale_transcript_save() {
        let dir = tempfile::tempdir().unwrap();
        let repository = SessionRepository::new(dir.path().to_path_buf());
        let stale = meta("model-update", "old-model", 1);
        repository.create(stale.clone()).await.unwrap();
        repository
            .update_model("model-update", "new-model".to_string())
            .await
            .unwrap();
        let reservation = repository
            .reserve_save("model-update", SessionWriteMode::Full)
            .unwrap();

        let outcome = repository
            .save(SessionSave {
                meta: stale,
                store: MessageStore::new(),
                write_mode: reservation.write_mode,
                generation: reservation.generation,
            })
            .await
            .unwrap();
        assert!(matches!(outcome, SessionSaveOutcome::Saved { .. }));

        assert_eq!(repository.list().unwrap()[0].model, "new-model");
    }
}

#[cfg(test)]
mod tests;
