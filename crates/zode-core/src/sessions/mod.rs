//! Durable, V1-compatible session store.
//!
//! The original `<id>.jsonl` transcript remains a live read/write compatibility
//! surface. Journal, checkpoint, and worktree data live in an additive sidecar
//! directory; old clients can keep reading and writing the V1 JSONL contract.

pub mod checkpoint;
pub mod journal;
pub mod worktree;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent::message::MessageStore;
use agent::session::Session;
use serde::{Deserialize, Serialize};

use crate::config::ConfigManager;
use crate::run_event::RunEventEnvelope;
use crate::session_meta::{SessionIndex, SessionMeta};
use crate::sessions::journal::{write_json_atomic, JournalEntry, SessionJournal};
use crate::CoreError;

pub const SESSION_SIDECAR_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableSessionMeta {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub model: String,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_checkpoint_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeMeta>,
    #[serde(default)]
    pub archived: bool,
}

impl DurableSessionMeta {
    pub fn new(meta: SessionMeta) -> Self {
        Self {
            schema_version: SESSION_SIDECAR_SCHEMA,
            id: meta.id,
            title: meta.title,
            cwd: meta.cwd,
            model: meta.model,
            created_at: meta.updated_at,
            updated_at: meta.updated_at,
            parent_session_id: None,
            parent_checkpoint_id: None,
            worktree: None,
            archived: false,
        }
    }

    pub fn index_meta(&self) -> SessionMeta {
        SessionMeta {
            id: self.id.clone(),
            title: self.title.clone(),
            cwd: self.cwd.clone(),
            model: self.model.clone(),
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeMeta {
    pub path: String,
    pub branch: String,
    pub base_commit: String,
}

#[derive(Debug)]
pub struct LoadedSession {
    pub meta: DurableSessionMeta,
    pub messages: MessageStore,
}

#[derive(Debug, Clone)]
pub struct ForkRequest {
    pub source_id: String,
    pub target_id: String,
    pub parent_checkpoint_id: Option<String>,
    pub worktree: Option<WorktreeMeta>,
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    pub fn open_default() -> Result<Self, CoreError> {
        Ok(Self::at(ConfigManager::config_dir()?.join("sessions")))
    }

    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn session_dir(&self, id: &str) -> Result<PathBuf, CoreError> {
        validate_session_id(id)?;
        Ok(self.root.join(id))
    }

    pub fn transcript_path(&self, id: &str) -> Result<PathBuf, CoreError> {
        validate_session_id(id)?;
        Ok(self.root.join(format!("{id}.jsonl")))
    }

    pub fn has_sidecar(&self, id: &str) -> bool {
        self.session_dir(id)
            .is_ok_and(|dir| dir.join("meta.json").is_file())
    }

    pub fn load_meta(&self, id: &str) -> Result<DurableSessionMeta, CoreError> {
        let path = self.session_dir(id)?.join("meta.json");
        let meta: DurableSessionMeta = serde_json::from_slice(&std::fs::read(path)?)?;
        if meta.schema_version != SESSION_SIDECAR_SCHEMA || meta.id != id {
            return Err(CoreError::Other(format!(
                "invalid session sidecar metadata for {id}"
            )));
        }
        Ok(meta)
    }

    pub fn update_meta(
        &self,
        id: &str,
        mutate: impl FnOnce(&mut DurableSessionMeta),
    ) -> Result<DurableSessionMeta, CoreError> {
        let mut meta = self.load_meta(id)?;
        mutate(&mut meta);
        meta.updated_at = now_secs();
        write_json_atomic(&self.session_dir(id)?.join("meta.json"), &meta)?;
        self.journal(id)?
            .append("session.meta_updated", serde_json::to_value(&meta)?)?;
        SessionIndex::update(|index| {
            index.upsert(meta.index_meta());
            Ok(())
        })?;
        Ok(meta)
    }

    pub async fn load(&self, id: &str) -> Result<LoadedSession, CoreError> {
        if !self.has_sidecar(id) {
            self.ensure_sidecar(id).await?;
        }
        let meta = self.load_meta(id)?;
        let transcript = self.transcript_path(id)?;
        // A sidecar can exist before the first transcript write (create()
        // runs before the first turn persists). Treat that as an empty
        // session instead of permanently bricking the id.
        let messages = if transcript.is_file() {
            Session::load(transcript)
                .await
                .map_err(|error| CoreError::Other(error.to_string()))?
        } else {
            MessageStore::new()
        };
        Ok(LoadedSession { meta, messages })
    }

    pub async fn save(
        &self,
        meta: &DurableSessionMeta,
        messages: &MessageStore,
    ) -> Result<(), CoreError> {
        validate_session_id(&meta.id)?;
        if meta.schema_version != SESSION_SIDECAR_SCHEMA {
            return Err(CoreError::Other(
                "unsupported session sidecar schema".into(),
            ));
        }
        let dir = self.session_dir(&meta.id)?;
        std::fs::create_dir_all(&dir)?;
        Session::save(self.transcript_path(&meta.id)?, messages)
            .await
            .map_err(|error| CoreError::Other(error.to_string()))?;
        self.publish_saved_meta(meta)
    }

    /// Append the new transcript tail when the expected watermark matches,
    /// falling back to a verified full rewrite if another writer or a
    /// compaction changed the prefix.
    pub async fn save_incremental(
        &self,
        meta: &DurableSessionMeta,
        messages: &MessageStore,
        expected: usize,
    ) -> Result<(), CoreError> {
        validate_session_id(&meta.id)?;
        if meta.schema_version != SESSION_SIDECAR_SCHEMA {
            return Err(CoreError::Other(
                "unsupported session sidecar schema".into(),
            ));
        }
        let dir = self.session_dir(&meta.id)?;
        std::fs::create_dir_all(&dir)?;
        let tail = messages.iter().skip(expected).cloned().collect::<Vec<_>>();
        // An append error (e.g. an unreadable existing transcript) must not
        // make persistence permanently fail; recover with a full rewrite,
        // exactly like the count-mismatch path.
        let appended = match Session::append(self.transcript_path(&meta.id)?, &tail, expected).await
        {
            Ok(appended) => appended,
            Err(error) => {
                tracing::warn!("session append failed, rewriting: {error}");
                false
            }
        };
        if !appended {
            Session::save(self.transcript_path(&meta.id)?, messages)
                .await
                .map_err(|error| CoreError::Other(error.to_string()))?;
        }
        self.publish_saved_meta(meta)
    }

    fn publish_saved_meta(&self, meta: &DurableSessionMeta) -> Result<(), CoreError> {
        let mut published = meta.clone();
        published.updated_at = now_secs();
        write_json_atomic(&self.session_dir(&meta.id)?.join("meta.json"), &published)?;
        self.journal(&meta.id)?.append(
            "transcript.saved",
            serde_json::json!({"updatedAt": published.updated_at}),
        )?;
        SessionIndex::update(|index| {
            index.upsert(published.index_meta());
            Ok(())
        })?;
        Ok(())
    }

    pub fn create(&self, meta: DurableSessionMeta) -> Result<(), CoreError> {
        validate_session_id(&meta.id)?;
        let dir = self.session_dir(&meta.id)?;
        if dir.exists() {
            return Err(CoreError::Other(format!(
                "session already exists: {}",
                meta.id
            )));
        }
        std::fs::create_dir_all(dir.join("snapshots"))?;
        write_json_atomic(&dir.join("meta.json"), &meta)?;
        self.journal(&meta.id)?
            .append("session.created", serde_json::to_value(&meta)?)?;
        Ok(())
    }

    /// Journal a run event. Per-token streaming deltas are deliberately not
    /// durable — journaling one entry (with two fsyncs) per token would make
    /// streaming O(journal size) and bloat the sidecar with a token-by-token
    /// copy of every response. Returns `Ok(None)` for skipped deltas.
    pub fn append_run_event(
        &self,
        id: &str,
        event: &RunEventEnvelope,
    ) -> Result<Option<JournalEntry>, CoreError> {
        if matches!(
            event.event,
            crate::run_event::RunEvent::MessageDelta { .. }
                | crate::run_event::RunEvent::ThinkingDelta { .. }
        ) {
            return Ok(None);
        }
        self.journal(id)?
            .append("run.event", serde_json::to_value(event)?)
            .map(Some)
    }

    pub fn journal(&self, id: &str) -> Result<SessionJournal, CoreError> {
        Ok(SessionJournal::new(self.session_dir(id)?))
    }

    pub fn checkpoints(&self, id: &str) -> Result<Vec<checkpoint::CheckpointRecord>, CoreError> {
        checkpoint::list_checkpoints(&self.session_dir(id)?)
    }

    pub fn preview_rewind(
        &self,
        id: &str,
        checkpoint_id: &str,
    ) -> Result<checkpoint::RewindPreview, CoreError> {
        let dir = self.session_dir(id)?;
        let record = checkpoint::load_checkpoint(&dir, checkpoint_id)?;
        checkpoint::rewind_preview(&dir, &record)
    }

    pub async fn apply_rewind(
        &self,
        id: &str,
        checkpoint_id: &str,
    ) -> Result<checkpoint::RewindResult, CoreError> {
        let dir = self.session_dir(id)?;
        let record = checkpoint::load_checkpoint(&dir, checkpoint_id)?;
        let result = checkpoint::rewind_apply(&dir, &record)?;
        let loaded = self.load(id).await?;
        let messages = prefix_messages(&loaded.messages, record.message_count)?;
        self.save(&loaded.meta, &messages).await?;
        Ok(result)
    }

    pub async fn fork(&self, request: ForkRequest) -> Result<DurableSessionMeta, CoreError> {
        validate_session_id(&request.source_id)?;
        validate_session_id(&request.target_id)?;
        let source = self.load(&request.source_id).await?;
        if self.has_sidecar(&request.target_id)
            || self
                .root
                .join(format!("{}.jsonl", request.target_id))
                .exists()
        {
            return Err(CoreError::Other(format!(
                "fork target already exists: {}",
                request.target_id
            )));
        }
        let now = now_secs();
        let mut target = source.meta.clone();
        target.id = request.target_id.clone();
        target.created_at = now;
        target.updated_at = now;
        target.parent_session_id = Some(request.source_id.clone());
        target.parent_checkpoint_id = request.parent_checkpoint_id.clone();
        target.worktree = request.worktree;
        target.archived = false;
        self.create(target.clone())?;
        let messages = if let Some(checkpoint_id) = &request.parent_checkpoint_id {
            let checkpoint = crate::sessions::checkpoint::load_checkpoint(
                &self.session_dir(&request.source_id)?,
                checkpoint_id,
            )?;
            prefix_messages(&source.messages, checkpoint.message_count)?
        } else {
            source.messages.clone()
        };
        self.save(&target, &messages).await?;
        copy_snapshot_store(
            &self.session_dir(&request.source_id)?.join("snapshots"),
            &self.session_dir(&request.target_id)?.join("snapshots"),
        )?;
        self.journal(&request.target_id)?.append(
            "session.forked",
            serde_json::json!({
                "parentSessionId": request.source_id,
                "parentCheckpointId": target.parent_checkpoint_id,
            }),
        )?;
        Ok(target)
    }

    pub async fn ensure_sidecar(&self, id: &str) -> Result<DurableSessionMeta, CoreError> {
        validate_session_id(id)?;
        if self.has_sidecar(id) {
            return self.load_meta(id);
        }
        let transcript = self.transcript_path(id)?;
        if !transcript.is_file() {
            return Err(CoreError::Other(format!("session not found: {id}")));
        }
        let messages = Session::load(&transcript)
            .await
            .map_err(|error| CoreError::Other(error.to_string()))?;
        let index = SessionIndex::load()?;
        let index_meta = index
            .sessions
            .iter()
            .find(|meta| meta.id == id)
            .cloned()
            .unwrap_or(SessionMeta {
                id: id.to_string(),
                title: "(session)".into(),
                cwd: std::env::current_dir()?.display().to_string(),
                model: crate::config::DEFAULT_STARTER_MODEL.into(),
                updated_at: now_secs(),
            });
        let meta = DurableSessionMeta::new(index_meta);
        self.create(meta.clone())?;
        let verified = Session::load(self.transcript_path(id)?)
            .await
            .map_err(|error| CoreError::Other(error.to_string()))?;
        if verified.len() != messages.len() {
            return Err(CoreError::Other(format!(
                "session migration verification failed for {id}"
            )));
        }
        SessionIndex::update(|index| {
            index.upsert(meta.index_meta());
            Ok(())
        })?;
        self.journal(id)?.append(
            "session.sidecar_initialized",
            serde_json::json!({"transcriptPath": transcript, "messageCount": messages.len()}),
        )?;
        Ok(meta)
    }
}

fn prefix_messages(source: &MessageStore, count: usize) -> Result<MessageStore, CoreError> {
    if count > source.len() {
        return Err(CoreError::Other(format!(
            "checkpoint transcript length {count} exceeds session length {}",
            source.len()
        )));
    }
    let mut target = MessageStore::new();
    for message in source.iter().take(count) {
        target
            .push(message.clone())
            .map_err(|error| CoreError::Other(error.to_string()))?;
    }
    Ok(target)
}

pub use crate::session_meta::validate_session_id;

fn copy_snapshot_store(source: &Path, target: &Path) -> Result<(), CoreError> {
    if !source.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let destination = target.join(entry.file_name());
        if std::fs::hard_link(entry.path(), &destination).is_err() {
            std::fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use agent::message::{ContentBlock, Header, Message};

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
            if let Some(previous) = self.previous.take() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn store_with_message(text: &str) -> MessageStore {
        let mut store = MessageStore::new();
        store
            .push(Message::User {
                header: Header::new(),
                content: vec![ContentBlock::Text { text: text.into() }],
            })
            .unwrap();
        store
    }

    fn meta(id: &str, cwd: &Path) -> DurableSessionMeta {
        DurableSessionMeta::new(SessionMeta {
            id: id.into(),
            title: "title".into(),
            cwd: cwd.display().to_string(),
            model: "model".into(),
            updated_at: now_secs(),
        })
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn saves_loads_and_forks_without_mutating_source() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        let source_meta = meta("source", config.path());
        store.create(source_meta.clone()).unwrap();
        store
            .save(&source_meta, &store_with_message("hello"))
            .await
            .unwrap();
        let fork = store
            .fork(ForkRequest {
                source_id: "source".into(),
                target_id: "target".into(),
                parent_checkpoint_id: None,
                worktree: None,
            })
            .await
            .unwrap();
        assert_eq!(fork.parent_session_id.as_deref(), Some("source"));
        assert_eq!(store.load("target").await.unwrap().messages.len(), 1);
        assert_eq!(store.load("source").await.unwrap().messages.len(), 1);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn checkpoint_fork_uses_the_pre_turn_transcript_prefix() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        let source_meta = meta("source-cp", config.path());
        store.create(source_meta.clone()).unwrap();
        let mut messages = store_with_message("before");
        for message in store_with_message("after").iter() {
            messages.push(message.clone()).unwrap();
        }
        store.save(&source_meta, &messages).await.unwrap();
        let checkpoint = checkpoint::CheckpointBuilder::begin_with_message_count(
            store.session_dir("source-cp").unwrap(),
            "source-cp",
            "turn",
            config.path().to_path_buf(),
            1,
        )
        .unwrap()
        .finish()
        .unwrap();
        store
            .fork(ForkRequest {
                source_id: "source-cp".into(),
                target_id: "target-cp".into(),
                parent_checkpoint_id: Some(checkpoint.id),
                worktree: None,
            })
            .await
            .unwrap();
        assert_eq!(store.load("target-cp").await.unwrap().messages.len(), 1);
        assert_eq!(store.load("source-cp").await.unwrap().messages.len(), 2);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn durable_features_keep_the_v1_jsonl_as_the_single_transcript() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        let session_meta = meta("v1-compatible", config.path());
        store.create(session_meta.clone()).unwrap();
        store
            .save(&session_meta, &store_with_message("original"))
            .await
            .unwrap();

        let v1_path = config.path().join("sessions/v1-compatible.jsonl");
        assert!(v1_path.is_file());
        assert!(!config
            .path()
            .join("sessions/v1-compatible/transcript.jsonl")
            .exists());
        assert_eq!(Session::load(&v1_path).await.unwrap().len(), 1);

        let mut changed_by_v1 = Session::load(&v1_path).await.unwrap();
        for message in store_with_message("written by a V1 client").iter() {
            changed_by_v1.push(message.clone()).unwrap();
        }
        Session::save(&v1_path, &changed_by_v1).await.unwrap();
        assert_eq!(store.load("v1-compatible").await.unwrap().messages.len(), 2);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn adding_a_sidecar_does_not_rewrite_an_existing_v1_transcript() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let root = config.path().join("sessions");
        std::fs::create_dir_all(&root).unwrap();
        let v1_path = root.join("existing-v1.jsonl");
        Session::save(&v1_path, &store_with_message("keep bytes"))
            .await
            .unwrap();
        let before = std::fs::read(&v1_path).unwrap();
        let store = SessionStore::at(root);

        store.ensure_sidecar("existing-v1").await.unwrap();

        assert_eq!(std::fs::read(v1_path).unwrap(), before);
        assert!(store.has_sidecar("existing-v1"));
        assert!(!store
            .session_dir("existing-v1")
            .unwrap()
            .join("transcript.jsonl")
            .exists());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn run_event_journal_skips_per_token_deltas() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        store.create(meta("deltas", config.path())).unwrap();
        let mut context = crate::run_event::RunEventContext::new("deltas", None, None);
        let before = store.journal("deltas").unwrap().read_all().unwrap().len();
        assert!(store
            .append_run_event(
                "deltas",
                &context.envelope(crate::run_event::RunEvent::MessageDelta {
                    delta: "tok".into()
                }),
            )
            .unwrap()
            .is_none());
        assert!(store
            .append_run_event(
                "deltas",
                &context.envelope(crate::run_event::RunEvent::ThinkingDelta {
                    delta: "tok".into()
                }),
            )
            .unwrap()
            .is_none());
        let journal = store.journal("deltas").unwrap().read_all().unwrap();
        assert_eq!(journal.len(), before);
        assert!(store
            .append_run_event(
                "deltas",
                &context.envelope(crate::run_event::RunEvent::RunStarted)
            )
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn save_incremental_recovers_from_a_failing_append_with_a_rewrite() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        let session_meta = meta("recover", config.path());
        store.create(session_meta.clone()).unwrap();
        let mut messages = store_with_message("first");
        store.save(&session_meta, &messages).await.unwrap();
        // Invalid UTF-8 makes Session::append fail with a real I/O error
        // (not the Ok(false) count-mismatch path).
        std::fs::write(
            store.transcript_path("recover").unwrap(),
            [0xff, 0xfe, 0xfd],
        )
        .unwrap();
        for message in store_with_message("second").iter() {
            messages.push(message.clone()).unwrap();
        }
        store
            .save_incremental(&session_meta, &messages, 1)
            .await
            .unwrap();
        assert_eq!(store.load("recover").await.unwrap().messages.len(), 2);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn loads_a_created_session_whose_transcript_was_never_written() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        store.create(meta("fresh", config.path())).unwrap();
        // A crash (or startup error) between create() and the first save()
        // leaves a sidecar without a transcript; the id must stay usable.
        let loaded = store.load("fresh").await.unwrap();
        assert_eq!(loaded.messages.len(), 0);
        assert_eq!(loaded.meta.id, "fresh");
    }

    #[test]
    fn rejects_path_traversal_session_ids() {
        let store = SessionStore::at(PathBuf::from("/tmp/sessions"));
        assert!(store.session_dir("../escape").is_err());
        assert!(store.session_dir("valid-id_1").is_ok());
    }
}
