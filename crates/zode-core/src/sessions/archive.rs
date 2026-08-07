//! Compacted-originals sidecar archive.
//!
//! Compaction tombstones replaced messages IN the live store (their content
//! is destroyed so later turns send the summary instead of the history), and
//! the transcript `.jsonl` persists that live view verbatim. Without a second
//! copy, exiting after a compaction and resuming would lose the original
//! conversation for good — the restored transcript would show only the
//! `[Context summary]` message.
//!
//! This module preserves those originals in an additive sidecar file,
//! `<sessions>/<id>/compacted.jsonl` (same schema as the transcript). Before
//! any full transcript rewrite, the new snapshot is diffed against the
//! on-disk transcript: every message that became a [`Message::Tombstone`] in
//! the snapshot while the disk still holds its content is appended to the
//! archive. The live transcript keeps its compacted shape — a resumed session
//! gets exactly the context the live session had — and the display layer
//! merges the archive back over the tombstones via
//! [`overlay_compacted_originals`].
//!
//! Best-effort by design: an archive failure must never block the real save
//! (losing the current turn to protect old history would be a bad trade).

use agent::message::{Message, MessageStore};
use agent::session::Session;

use super::SessionStore;

/// Name of the sidecar file inside the session directory.
const ARCHIVE_FILE: &str = "compacted.jsonl";

impl SessionStore {
    /// Path of the compacted-originals archive for `id`.
    pub fn compacted_archive_path(&self, id: &str) -> Result<std::path::PathBuf, crate::CoreError> {
        Ok(self.session_dir(id)?.join(ARCHIVE_FILE))
    }

    /// Load the compacted-originals archive. Missing or unreadable archives
    /// load as empty — display merging is strictly best-effort.
    pub async fn load_compacted_archive(&self, id: &str) -> MessageStore {
        let Ok(path) = self.compacted_archive_path(id) else {
            return MessageStore::new();
        };
        if !path.is_file() {
            return MessageStore::new();
        }
        match Session::load(&path).await {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!("compacted archive load failed for {id}: {error}");
                MessageStore::new()
            }
        }
    }

    /// Before a full transcript rewrite, copy into the archive every message
    /// that `new` tombstones while an original is still available — from the
    /// on-disk transcript, or from `extra_originals` (the engine's display
    /// overlay), which covers messages that were compacted BEFORE their
    /// first save ever reached disk (e.g. a long first turn that
    /// auto-compacted mid-flight). Never fails the caller: archiving
    /// problems are logged and the save proceeds.
    pub(super) async fn preserve_compacted_originals(
        &self,
        id: &str,
        new: &MessageStore,
        extra_originals: &MessageStore,
    ) {
        let tombstoned: std::collections::HashSet<uuid::Uuid> = new
            .iter()
            .filter(|message| matches!(message, Message::Tombstone { .. }))
            .map(Message::uuid)
            .collect();
        if tombstoned.is_empty() {
            return;
        }
        let mut candidates: Vec<Message> = Vec::new();
        if let Ok(transcript) = self.transcript_path(id) {
            if transcript.is_file() {
                match Session::load(&transcript).await {
                    Ok(previous) => candidates.extend(previous.iter().cloned()),
                    Err(error) => {
                        tracing::warn!(
                            "compacted archive: previous transcript unreadable for {id}: {error}"
                        );
                    }
                }
            }
        }
        candidates.extend(extra_originals.iter().cloned());
        if candidates.is_empty() {
            return;
        }
        let mut archive = self.load_compacted_archive(id).await;
        let mut changed = false;
        for message in candidates {
            if !tombstoned.contains(&message.uuid()) || matches!(message, Message::Tombstone { .. })
            {
                continue;
            }
            // DuplicateUuid means the uuid is already archived (earlier
            // rewrite, or the same original present in both sources).
            if archive.push(message).is_ok() {
                changed = true;
            }
        }
        if !changed {
            return;
        }
        match self.compacted_archive_path(id) {
            Ok(path) => {
                if let Err(error) = Session::save(&path, &archive).await {
                    tracing::warn!("compacted archive save failed for {id}: {error}");
                }
            }
            Err(error) => {
                tracing::warn!("compacted archive path failed for {id}: {error}");
            }
        }
    }

    /// Copy the compacted archive from `source_id` to `target_id` (fork):
    /// the forked transcript carries the same tombstones, so without its own
    /// archive copy the fork's history would display as summary-only. Only
    /// records whose uuid is actually tombstoned in `target_messages` are
    /// copied — a checkpoint fork truncates the transcript, and originals
    /// from beyond that boundary must not remain recoverable in the child
    /// sidecar. Best-effort like every archive operation.
    pub(super) async fn copy_compacted_archive(
        &self,
        source_id: &str,
        target_id: &str,
        target_messages: &MessageStore,
    ) {
        let Ok(target) = self.compacted_archive_path(target_id) else {
            return;
        };
        let source_archive = self.load_compacted_archive(source_id).await;
        if source_archive.is_empty() {
            return;
        }
        let tombstoned: std::collections::HashSet<uuid::Uuid> = target_messages
            .iter()
            .filter(|message| matches!(message, Message::Tombstone { .. }))
            .map(Message::uuid)
            .collect();
        let mut filtered = MessageStore::new();
        for message in source_archive.iter() {
            if tombstoned.contains(&message.uuid()) {
                let _ = filtered.push(message.clone());
            }
        }
        if filtered.is_empty() {
            return;
        }
        if let Err(error) = Session::save(&target, &filtered).await {
            tracing::warn!("compacted archive copy failed for fork {target_id}: {error}");
        }
    }
}

/// Build a DISPLAY view of `store`: tombstoned messages are swapped back to
/// their archived originals when the archive has them. The result is for
/// rendering only — it must never be fed to the model or persisted as the
/// transcript, or the compaction would be undone.
pub fn overlay_compacted_originals(store: &MessageStore, archive: &MessageStore) -> MessageStore {
    if archive.is_empty() {
        return store.clone();
    }
    let mut merged = MessageStore::new();
    for message in store.iter() {
        let shown = match message {
            Message::Tombstone { .. } => archive.get(message.uuid()).unwrap_or(message),
            _ => message,
        };
        // Push can only fail on a duplicate uuid, which `store` (already a
        // valid MessageStore) cannot contain.
        let _ = merged.push(shown.clone());
    }
    merged
}

#[cfg(test)]
mod tests {
    use agent::message::{ContentBlock, Header};

    use super::*;
    use crate::session_meta::SessionMeta;
    use crate::sessions::DurableSessionMeta;

    /// `SessionStore::save` publishes to the global `SessionIndex`, which
    /// resolves through `ZODE_CONFIG_DIR` — point it at the test TempDir so
    /// tests never touch the real `~/.zode` (mirrors the mod.rs test guard).
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

    fn user(text: &str) -> Message {
        Message::User {
            header: Header::new(),
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    fn tombstone_of(message: &Message) -> Message {
        Message::Tombstone {
            header: message.header().clone(),
            reason: "compacted".into(),
        }
    }

    fn summary() -> Message {
        Message::User {
            header: Header::new(),
            content: vec![ContentBlock::Text {
                text: "[Context summary]\nWe did things.".into(),
            }],
        }
    }

    fn meta(id: &str, cwd: &std::path::Path) -> DurableSessionMeta {
        DurableSessionMeta::new(SessionMeta {
            id: id.into(),
            title: "title".into(),
            cwd: cwd.display().to_string(),
            model: "model".into(),
            updated_at: 1,
        })
    }

    fn text_of(message: &Message) -> Option<&str> {
        match message {
            Message::User { content, .. } | Message::Assistant { content, .. } => {
                content.iter().find_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
            }
            _ => None,
        }
    }

    /// Compaction then a full rewrite must keep the originals recoverable:
    /// the transcript stays compacted (resume context unchanged) while the
    /// archive + overlay restore the display.
    #[tokio::test]
    #[serial_test::serial]
    async fn full_rewrite_archives_tombstoned_originals() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        let session_meta = meta("compacted", config.path());
        store.create(session_meta.clone()).unwrap();

        let original_a = user("original question");
        let original_b = user("original follow-up");
        let mut before = MessageStore::new();
        before.push(original_a.clone()).unwrap();
        before.push(original_b.clone()).unwrap();
        store.save(&session_meta, &before).await.unwrap();

        // Simulate what apply_compaction_to_store leaves behind.
        let mut after = MessageStore::new();
        after.push(tombstone_of(&original_a)).unwrap();
        after.push(tombstone_of(&original_b)).unwrap();
        after.push(summary()).unwrap();
        store.save(&session_meta, &after).await.unwrap();

        let reloaded = store.load("compacted").await.unwrap().messages;
        assert!(
            matches!(reloaded.iter().next(), Some(Message::Tombstone { .. })),
            "transcript keeps the compacted shape"
        );

        let archive = store.load_compacted_archive("compacted").await;
        assert_eq!(archive.len(), 2, "both originals archived");

        let display = overlay_compacted_originals(&reloaded, &archive);
        assert_eq!(display.len(), 3);
        assert_eq!(
            text_of(display.iter().next().unwrap()),
            Some("original question")
        );
        assert_eq!(
            text_of(display.iter().nth(1).unwrap()),
            Some("original follow-up")
        );
    }

    /// A second compaction (tombstoning more messages, including re-saving
    /// existing tombstones) must extend the archive without duplicating it.
    #[tokio::test]
    #[serial_test::serial]
    async fn repeated_rewrites_do_not_duplicate_archive_entries() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        let session_meta = meta("twice", config.path());
        store.create(session_meta.clone()).unwrap();

        let original_a = user("first");
        let original_b = user("second");
        let mut v1 = MessageStore::new();
        v1.push(original_a.clone()).unwrap();
        v1.push(original_b.clone()).unwrap();
        store.save(&session_meta, &v1).await.unwrap();

        let mut v2 = MessageStore::new();
        v2.push(tombstone_of(&original_a)).unwrap();
        v2.push(original_b.clone()).unwrap();
        store.save(&session_meta, &v2).await.unwrap();

        let mut v3 = MessageStore::new();
        v3.push(tombstone_of(&original_a)).unwrap();
        v3.push(tombstone_of(&original_b)).unwrap();
        v3.push(summary()).unwrap();
        store.save(&session_meta, &v3).await.unwrap();
        // Re-save the same snapshot (e.g. an explicit-save path).
        store.save(&session_meta, &v3).await.unwrap();

        let archive = store.load_compacted_archive("twice").await;
        assert_eq!(archive.len(), 2, "each original archived exactly once");
    }

    /// The incremental-save fallback (append refused because a compaction
    /// changed the prefix) must archive through the same path.
    #[tokio::test]
    #[serial_test::serial]
    async fn incremental_fallback_rewrite_archives_originals() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        let session_meta = meta("fallback", config.path());
        store.create(session_meta.clone()).unwrap();

        let original = user("keep me recoverable");
        let mut v1 = MessageStore::new();
        v1.push(original.clone()).unwrap();
        store.save(&session_meta, &v1).await.unwrap();

        let mut v2 = MessageStore::new();
        v2.push(tombstone_of(&original)).unwrap();
        v2.push(summary()).unwrap();
        // Watermark (0) mismatches the on-disk count (1) → append refuses →
        // the fallback full-rewrite path must archive the original.
        store.save_incremental(&session_meta, &v2, 0).await.unwrap();

        let archive = store.load_compacted_archive("fallback").await;
        assert_eq!(archive.len(), 1);
        assert_eq!(
            text_of(archive.iter().next().unwrap()),
            Some("keep me recoverable")
        );
    }

    /// Pure-append saves (no tombstones) must not pay the diff cost or touch
    /// the archive.
    #[tokio::test]
    #[serial_test::serial]
    async fn appends_without_tombstones_leave_no_archive() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        let session_meta = meta("plain", config.path());
        store.create(session_meta.clone()).unwrap();

        let mut v1 = MessageStore::new();
        v1.push(user("a")).unwrap();
        store.save(&session_meta, &v1).await.unwrap();
        v1.push(user("b")).unwrap();
        store.save_incremental(&session_meta, &v1, 1).await.unwrap();

        assert!(!store.compacted_archive_path("plain").unwrap().is_file());
    }

    /// A long FIRST turn can auto-compact before any save: the very first
    /// save already contains tombstones and the disk has no originals — the
    /// caller-provided overlay must be an archiving source too.
    #[tokio::test]
    #[serial_test::serial]
    async fn overlay_originals_are_archived_when_disk_never_saw_them() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        let session_meta = meta("firstsave", config.path());
        store.create(session_meta.clone()).unwrap();

        let original = user("compacted before the first save");
        let mut snapshot = MessageStore::new();
        snapshot.push(tombstone_of(&original)).unwrap();
        snapshot.push(summary()).unwrap();
        let mut overlay = MessageStore::new();
        overlay.push(original.clone()).unwrap();

        store
            .save_with_originals(&session_meta, &snapshot, &overlay)
            .await
            .unwrap();

        let archive = store.load_compacted_archive("firstsave").await;
        assert_eq!(archive.len(), 1);
        assert_eq!(
            text_of(archive.iter().next().unwrap()),
            Some("compacted before the first save")
        );
    }

    /// A fork of a compacted session must carry the archive so its
    /// tombstoned history stays displayable.
    #[tokio::test]
    #[serial_test::serial]
    async fn fork_copies_the_compacted_archive() {
        let config = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let store = SessionStore::at(config.path().join("sessions"));
        let session_meta = meta("fork-src", config.path());
        store.create(session_meta.clone()).unwrap();

        let original = user("history worth keeping");
        let mut v1 = MessageStore::new();
        v1.push(original.clone()).unwrap();
        store.save(&session_meta, &v1).await.unwrap();

        let mut v2 = MessageStore::new();
        v2.push(tombstone_of(&original)).unwrap();
        v2.push(summary()).unwrap();
        store.save(&session_meta, &v2).await.unwrap();
        assert_eq!(store.load_compacted_archive("fork-src").await.len(), 1);

        store
            .fork(crate::sessions::ForkRequest {
                source_id: "fork-src".into(),
                target_id: "fork-dst".into(),
                parent_checkpoint_id: None,
                worktree: None,
            })
            .await
            .unwrap();

        let forked_archive = store.load_compacted_archive("fork-dst").await;
        assert_eq!(forked_archive.len(), 1);
        let forked = store.load("fork-dst").await.unwrap().messages;
        let display = overlay_compacted_originals(&forked, &forked_archive);
        assert_eq!(
            text_of(display.iter().next().unwrap()),
            Some("history worth keeping")
        );
    }

    #[test]
    fn overlay_returns_store_clone_when_archive_is_empty() {
        let mut store = MessageStore::new();
        store.push(user("hello")).unwrap();
        let merged = overlay_compacted_originals(&store, &MessageStore::new());
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn overlay_keeps_unarchived_tombstones_as_tombstones() {
        let original = user("gone forever");
        let mut store = MessageStore::new();
        store.push(tombstone_of(&original)).unwrap();
        let mut archive = MessageStore::new();
        archive.push(user("unrelated")).unwrap();
        let merged = overlay_compacted_originals(&store, &archive);
        assert!(matches!(
            merged.iter().next(),
            Some(Message::Tombstone { .. })
        ));
    }
}
