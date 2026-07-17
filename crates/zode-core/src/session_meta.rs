//! Session metadata index. agent::Session owns the JSONL transcript;
//! this index tracks id/title/cwd/model/updated_at for listing and
//! resuming. Stored at `<config_dir>/sessions/index.json`.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use agent::message::{ContentBlock, Message};
use agent::session::{SessionHeader, SCHEMA_VERSION};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};

use crate::config::{ConfigManager, DEFAULT_STARTER_MODEL};
use crate::error::CoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub model: String,
    /// Unix seconds. Stamped by the caller.
    pub updated_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionIndex {
    pub sessions: Vec<SessionMeta>,
}

struct LoadedIndex {
    index: SessionIndex,
    repair_needed: bool,
    corrupt_primary: Option<Vec<u8>>,
}

impl SessionIndex {
    fn sessions_dir() -> Result<PathBuf, CoreError> {
        Ok(ConfigManager::config_dir()?.join("sessions"))
    }

    fn index_path() -> Result<PathBuf, CoreError> {
        Ok(Self::sessions_dir()?.join("index.json"))
    }

    fn backup_path() -> Result<PathBuf, CoreError> {
        Ok(Self::sessions_dir()?.join("index.json.bak"))
    }

    fn lock_path() -> Result<PathBuf, CoreError> {
        Ok(Self::sessions_dir()?.join(".index.lock"))
    }

    pub fn session_path(id: &str) -> Result<PathBuf, CoreError> {
        Ok(Self::sessions_dir()?.join(format!("{id}.jsonl")))
    }

    pub fn session_sidecar_dir(id: &str) -> Result<PathBuf, CoreError> {
        Ok(Self::sessions_dir()?.join(id))
    }

    /// Load the metadata cache under a cross-process lock. A malformed legacy
    /// index is archived and repaired from its valid prefix/backup. Metadata
    /// without a transcript is pruned and transcript files missing from the
    /// cache are re-indexed. The JSONL files are the durable conversation data;
    /// `index.json` is only a rebuildable listing cache.
    pub fn load() -> Result<Self, CoreError> {
        let dir = Self::sessions_dir()?;
        if !dir.exists() {
            return Ok(Self::default());
        }
        std::fs::create_dir_all(&dir)?;
        let _lock = lock_index()?;
        let mut loaded = load_and_reconcile_unlocked(&dir)?;
        persist_repair_unlocked(&mut loaded)?;
        Ok(loaded.index)
    }

    /// Publish a complete snapshot under the cross-process lock. Production
    /// read-modify-write paths should use [`Self::update`] so a snapshot loaded
    /// by another process cannot overwrite newer entries.
    pub fn save(&self) -> Result<(), CoreError> {
        let dir = Self::sessions_dir()?;
        std::fs::create_dir_all(&dir)?;
        let _lock = lock_index()?;
        save_unlocked(self)
    }

    /// Run an index read-modify-write transaction while holding the same OS
    /// lock used by every Zode process. This prevents last-writer-wins loss when
    /// multiple terminals finish turns at the same time.
    pub fn update<T>(
        mutate: impl FnOnce(&mut SessionIndex) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let dir = Self::sessions_dir()?;
        std::fs::create_dir_all(&dir)?;
        let _lock = lock_index()?;
        let mut loaded = load_and_reconcile_unlocked(&dir)?;
        let before = loaded.index.clone();
        let result = match mutate(&mut loaded.index) {
            Ok(result) => result,
            Err(error) => {
                if loaded.repair_needed {
                    persist_repair_unlocked(&mut loaded)?;
                }
                return Err(error);
            }
        };
        if loaded.index != before {
            loaded.repair_needed = true;
        }
        if loaded.repair_needed {
            persist_repair_unlocked(&mut loaded)?;
        }
        Ok(result)
    }

    pub fn upsert(&mut self, meta: SessionMeta) {
        if let Some(existing) = self.sessions.iter_mut().find(|m| m.id == meta.id) {
            *existing = meta;
        } else {
            self.sessions.push(meta);
        }
    }

    /// Bump `updated_at` for an existing session (preserving title/cwd/model)
    /// so `--continue` resumes the genuinely most-recent one. Returns false
    /// if the id isn't in the index (caller should upsert a fresh entry).
    pub fn touch_updated(&mut self, id: &str, now: u64) -> bool {
        if let Some(m) = self.sessions.iter_mut().find(|m| m.id == id) {
            m.updated_at = now;
            true
        } else {
            false
        }
    }

    /// Drop the session with `id` from the index. Returns true if removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.sessions.len();
        self.sessions.retain(|m| m.id != id);
        self.sessions.len() != before
    }

    pub async fn delete_session_file_and_index(id: &str) -> Result<(), CoreError> {
        validate_session_id(id)?;
        let path = Self::session_path(id)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) if e.kind() == std::io::ErrorKind::IsADirectory => {}
            Err(e) => return Err(CoreError::Io(e)),
        }
        let sidecar = Self::session_sidecar_dir(id)?;
        match tokio::fs::remove_dir_all(sidecar).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(CoreError::Io(error)),
        }
        Self::update(|idx| {
            idx.remove(id);
            Ok(())
        })?;
        Ok(())
    }

    pub fn set_title(id: &str, title: String) -> Result<(), CoreError> {
        Self::update(|idx| {
            let Some(meta) = idx.sessions.iter_mut().find(|m| m.id == id) else {
                return Err(CoreError::Other(format!("session not found: {id}")));
            };
            meta.title = title;
            Ok(())
        })
    }

    /// Most recently updated session.
    pub fn latest(&self) -> Option<&SessionMeta> {
        self.sessions.iter().max_by_key(|m| m.updated_at)
    }

    /// Session matching `prefix`: an exact id always wins (user-chosen ids
    /// may prefix each other), else the first prefix match.
    pub fn find_prefix(&self, prefix: &str) -> Option<&SessionMeta> {
        self.sessions
            .iter()
            .find(|m| m.id == prefix)
            .or_else(|| self.sessions.iter().find(|m| m.id.starts_with(prefix)))
    }

    /// Sessions newest-first (for the picker UI in Phase 07).
    pub fn newest_first(&self) -> Vec<&SessionMeta> {
        let mut v: Vec<&SessionMeta> = self.sessions.iter().collect();
        v.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
        v
    }
}

fn lock_index() -> Result<File, CoreError> {
    let path = SessionIndex::lock_path()?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    FileExt::lock_exclusive(&file)?;
    Ok(file)
}

fn load_and_reconcile_unlocked(dir: &Path) -> Result<LoadedIndex, CoreError> {
    let mut loaded = load_unlocked()?;
    if reconcile_transcripts(&mut loaded.index, dir)? {
        loaded.repair_needed = true;
    }
    Ok(loaded)
}

fn load_unlocked() -> Result<LoadedIndex, CoreError> {
    let primary_path = SessionIndex::index_path()?;
    let backup_path = SessionIndex::backup_path()?;
    match std::fs::read(&primary_path) {
        Ok(raw) => match serde_json::from_slice::<SessionIndex>(&raw) {
            Ok(index) => Ok(LoadedIndex {
                index,
                repair_needed: false,
                corrupt_primary: None,
            }),
            Err(error) => {
                tracing::warn!("session index is malformed and will be repaired: {error}");
                let index = salvage_leading_index(&raw)
                    .or_else(|| load_valid_backup(&backup_path))
                    .unwrap_or_default();
                Ok(LoadedIndex {
                    index,
                    repair_needed: true,
                    corrupt_primary: Some(raw),
                })
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(index) = load_valid_backup(&backup_path) {
                Ok(LoadedIndex {
                    index,
                    repair_needed: true,
                    corrupt_primary: None,
                })
            } else {
                Ok(LoadedIndex {
                    index: SessionIndex::default(),
                    repair_needed: false,
                    corrupt_primary: None,
                })
            }
        }
        Err(error) => {
            if let Some(index) = load_valid_backup(&backup_path) {
                tracing::warn!("session index could not be read; restoring backup: {error}");
                Ok(LoadedIndex {
                    index,
                    repair_needed: true,
                    corrupt_primary: None,
                })
            } else {
                Err(CoreError::Io(error))
            }
        }
    }
}

fn salvage_leading_index(raw: &[u8]) -> Option<SessionIndex> {
    let mut stream = serde_json::Deserializer::from_slice(raw).into_iter::<SessionIndex>();
    stream.next()?.ok()
}

fn load_valid_backup(path: &Path) -> Option<SessionIndex> {
    std::fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
}

fn persist_repair_unlocked(loaded: &mut LoadedIndex) -> Result<(), CoreError> {
    if !loaded.repair_needed {
        return Ok(());
    }
    if let Some(raw) = loaded.corrupt_primary.take() {
        if let Err(error) = archive_corrupt_index(&raw) {
            tracing::warn!("could not archive corrupt session index: {error}");
        }
    }
    save_unlocked(&loaded.index)?;
    loaded.repair_needed = false;
    Ok(())
}

fn save_unlocked(index: &SessionIndex) -> Result<(), CoreError> {
    let json = serde_json::to_vec_pretty(index)?;
    write_atomic(&SessionIndex::index_path()?, &json)?;
    if let Err(error) = write_atomic(&SessionIndex::backup_path()?, &json) {
        tracing::warn!("could not refresh session index backup: {error}");
    }
    Ok(())
}

fn archive_corrupt_index(raw: &[u8]) -> std::io::Result<PathBuf> {
    let dir = SessionIndex::sessions_dir().map_err(core_error_to_io)?;
    for _ in 0..16 {
        let path = dir.join(format!(
            "index.corrupt-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        file.write_all(raw)?;
        file.sync_all()?;
        return Ok(path);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate corrupt session index archive",
    ))
}

fn core_error_to_io(error: CoreError) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

fn reconcile_transcripts(index: &mut SessionIndex, dir: &Path) -> Result<bool, CoreError> {
    let indexed_before = index.sessions.len();
    index.sessions.retain(|meta| {
        let transcript = dir.join(format!("{}.jsonl", meta.id));
        if transcript.is_file() {
            return true;
        }
        match std::fs::metadata(&transcript) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                tracing::warn!(path = %transcript.display(), %error, "could not verify indexed session transcript; keeping metadata");
                true
            }
            Ok(_) => false,
        }
    });
    let pruned = indexed_before - index.sessions.len();
    if pruned > 0 {
        tracing::warn!(
            count = pruned,
            "pruning session metadata without transcript files"
        );
    }

    let mut known: HashSet<String> = index.sessions.iter().map(|meta| meta.id.clone()).collect();
    let mut recovered = Vec::new();
    let mut defaults = None;

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let meta_path = path.join("meta.json");
            let Ok(raw) = std::fs::read(&meta_path) else {
                continue;
            };
            let Ok(meta) = serde_json::from_slice::<crate::sessions::DurableSessionMeta>(&raw)
            else {
                tracing::warn!(path = %meta_path.display(), "ignoring invalid durable session metadata");
                continue;
            };
            let matches_dir =
                path.file_name().and_then(|name| name.to_str()) == Some(meta.id.as_str());
            let transcript = dir.join(format!("{}.jsonl", meta.id));
            if !matches_dir || !transcript.is_file() {
                continue;
            }
            if known.insert(meta.id.clone()) {
                recovered.push(meta.index_meta());
            }
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(id) = path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        if known.contains(&id) {
            continue;
        }
        let Some((title, message_updated_at)) = transcript_summary(&path) else {
            tracing::warn!(path = %path.display(), "ignoring invalid orphan session transcript");
            continue;
        };
        let updated_at = path
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(message_updated_at);
        let (cwd, model) = defaults.get_or_insert_with(recovery_defaults);
        known.insert(id.clone());
        recovered.push(SessionMeta {
            id,
            title,
            cwd: cwd.clone(),
            model: model.clone(),
            updated_at,
        });
    }

    let recovered_any = !recovered.is_empty();
    if recovered_any {
        tracing::warn!(
            count = recovered.len(),
            "re-indexing session transcripts missing from the metadata cache"
        );
        index.sessions.extend(recovered);
    }
    Ok(pruned > 0 || recovered_any)
}

fn recovery_defaults() -> (String, String) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let model = ConfigManager::load(&cwd)
        .ok()
        .and_then(|config| config.provider.model)
        .unwrap_or_else(|| DEFAULT_STARTER_MODEL.to_string());
    (cwd.display().to_string(), model)
}

fn transcript_summary(path: &Path) -> Option<(String, u64)> {
    let file = File::open(path).ok()?;
    let mut lines = BufReader::new(file).lines();
    let header: SessionHeader = serde_json::from_str(&lines.next()?.ok()?).ok()?;
    if header.schema_version != SCHEMA_VERSION {
        return None;
    }

    let mut title = None;
    let mut updated_at = 0;
    for line in lines {
        let message: Message = serde_json::from_str(&line.ok()?).ok()?;
        updated_at = updated_at.max(message.header().timestamp_ms / 1000);
        if title.is_none() {
            if let Message::User { content, .. } = &message {
                let prompt = content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !prompt.trim().is_empty() {
                    title = Some(title_from_prompt(&prompt));
                }
            }
        }
    }
    Some((
        title.unwrap_or_else(|| "(recovered session)".to_string()),
        updated_at,
    ))
}

/// Publish a complete file by staging it beside the destination and then
/// atomically replacing the destination. A unique, exclusively-created temp
/// prevents concurrent writers from sharing staging state; every failure after
/// creation removes that writer's temp file.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    const MAX_TEMP_ATTEMPTS: usize = 16;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "index.json".to_string());
    let mut last_collision = None;

    for _ in 0..MAX_TEMP_ATTEMPTS {
        let temp_path = dir.join(format!(
            ".{file_name}.{}.tmp",
            uuid::Uuid::new_v4().simple()
        ));
        let mut temp = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last_collision = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };

        let result = temp.write_all(bytes).and_then(|()| temp.sync_all());
        drop(temp);
        let result = result.and_then(|()| std::fs::rename(&temp_path, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        return result;
    }

    Err(last_collision.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a unique session index temp file",
        )
    }))
}

/// Title from the first user message: first line, trimmed to 48 chars.
/// Shared authority for on-disk session ids. Everything that derives a
/// filesystem path from a user-supplied id must pass this first; an empty or
/// path-like id would otherwise escape (or become) the sessions directory.
pub fn validate_session_id(id: &str) -> Result<(), CoreError> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(CoreError::Other(format!("invalid session id: {id:?}")));
    }
    Ok(())
}

pub fn title_from_prompt(prompt: &str) -> String {
    let first = prompt.lines().next().unwrap_or("").trim();
    if first.chars().count() > 48 {
        let truncated: String = first.chars().take(47).collect();
        format!("{truncated}…")
    } else if first.is_empty() {
        "(untitled)".to_string()
    } else {
        first.to_string()
    }
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
            if let Some(previous) = self.previous.take() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn write_empty_transcript(id: &str) {
        let path = SessionIndex::session_path(id).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            format!(
                "{}\n",
                serde_json::to_string(&SessionHeader::current()).unwrap()
            ),
        )
        .unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn upsert_then_latest_and_find() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        write_empty_transcript("aaaa1111");
        write_empty_transcript("bbbb2222");

        let mut idx = SessionIndex::load().unwrap();
        idx.upsert(SessionMeta {
            id: "aaaa1111".into(),
            title: "first".into(),
            cwd: "/tmp".into(),
            model: "MiniMax-M1".into(),
            updated_at: 100,
        });
        idx.upsert(SessionMeta {
            id: "bbbb2222".into(),
            title: "second".into(),
            cwd: "/tmp".into(),
            model: "MiniMax-M1".into(),
            updated_at: 200,
        });
        idx.save().unwrap();

        let reloaded = SessionIndex::load().unwrap();
        assert_eq!(reloaded.latest().unwrap().id, "bbbb2222");
        assert_eq!(reloaded.find_prefix("aaaa").unwrap().title, "first");
        assert!(reloaded.find_prefix("zzz").is_none());
    }

    #[test]
    fn find_prefix_prefers_an_exact_id_over_a_longer_prefix_match() {
        let mut idx = SessionIndex::default();
        for id in ["abc2", "abc"] {
            idx.upsert(SessionMeta {
                id: id.into(),
                title: id.into(),
                cwd: "/p".into(),
                model: "m".into(),
                updated_at: 1,
            });
        }
        assert_eq!(idx.find_prefix("abc").unwrap().id, "abc");
        assert_eq!(idx.find_prefix("abc2").unwrap().id, "abc2");
    }

    #[test]
    fn touch_updates_recency_preserving_fields() {
        let mut idx = SessionIndex::default();
        idx.upsert(SessionMeta {
            id: "s1".into(),
            title: "keep me".into(),
            cwd: "/p".into(),
            model: "m".into(),
            updated_at: 100,
        });
        assert!(idx.touch_updated("s1", 500));
        let m = idx.find_prefix("s1").unwrap();
        assert_eq!(m.updated_at, 500);
        assert_eq!(m.title, "keep me"); // preserved
        assert!(!idx.touch_updated("missing", 999));
    }

    #[test]
    #[serial_test::serial]
    fn session_path_under_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        let p = SessionIndex::session_path("abc").unwrap();
        assert!(p.ends_with("sessions/abc.jsonl"));
    }

    #[test]
    fn title_truncates() {
        assert_eq!(title_from_prompt("hello world"), "hello world");
        assert_eq!(title_from_prompt(""), "(untitled)");
        assert_eq!(title_from_prompt("line one\nline two"), "line one");
        let long = "x".repeat(60);
        assert!(title_from_prompt(&long).chars().count() <= 48);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn delete_session_file_and_index_removes_both() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        let id = "deadbeef";
        let path = SessionIndex::session_path(id).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{}\n").unwrap();
        let mut idx = SessionIndex::default();
        idx.upsert(SessionMeta {
            id: id.to_string(),
            title: "delete me".to_string(),
            cwd: "/tmp".to_string(),
            model: "m".to_string(),
            updated_at: 1,
        });
        idx.save().unwrap();
        SessionIndex::delete_session_file_and_index(id)
            .await
            .unwrap();
        assert!(!path.exists());
        assert!(SessionIndex::load().unwrap().find_prefix(id).is_none());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn delete_rejects_invalid_ids_without_touching_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(sessions.join("survivor")).unwrap();
        std::fs::write(sessions.join("survivor.jsonl"), "{}\n").unwrap();
        std::fs::create_dir_all(dir.path().join("outside")).unwrap();

        for id in ["", "../outside", "a/b", "."] {
            assert!(
                SessionIndex::delete_session_file_and_index(id)
                    .await
                    .is_err(),
                "id {id:?} must be rejected"
            );
        }

        assert!(sessions.join("survivor").is_dir());
        assert!(sessions.join("survivor.jsonl").is_file());
        assert!(dir.path().join("outside").is_dir());
    }

    #[test]
    #[serial_test::serial]
    fn set_title_updates_existing_session() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        write_empty_transcript("rename-me");
        let mut idx = SessionIndex::default();
        idx.upsert(SessionMeta {
            id: "rename-me".to_string(),
            title: "old".to_string(),
            cwd: "/tmp".to_string(),
            model: "m".to_string(),
            updated_at: 1,
        });
        idx.save().unwrap();
        SessionIndex::set_title("rename-me", "new".to_string()).unwrap();
        assert_eq!(
            SessionIndex::load()
                .unwrap()
                .find_prefix("rename-me")
                .unwrap()
                .title,
            "new"
        );
    }

    #[test]
    #[serial_test::serial]
    fn load_repairs_trailing_writer_garbage_and_archives_it() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        write_empty_transcript("kept-session");
        let mut index = SessionIndex::default();
        index.upsert(SessionMeta {
            id: "kept-session".to_string(),
            title: "kept".to_string(),
            cwd: "/tmp".to_string(),
            model: "m".to_string(),
            updated_at: 7,
        });
        let mut corrupt = serde_json::to_vec_pretty(&index).unwrap();
        corrupt.extend_from_slice(b"  }\n  ]\n}");
        let sessions_dir = SessionIndex::sessions_dir().unwrap();
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::write(SessionIndex::index_path().unwrap(), &corrupt).unwrap();

        let loaded = SessionIndex::load().unwrap();

        assert_eq!(loaded, index);
        let repaired = std::fs::read(SessionIndex::index_path().unwrap()).unwrap();
        assert_eq!(
            serde_json::from_slice::<SessionIndex>(&repaired).unwrap(),
            index
        );
        assert_eq!(
            serde_json::from_slice::<SessionIndex>(
                &std::fs::read(SessionIndex::backup_path().unwrap()).unwrap()
            )
            .unwrap(),
            index
        );
        let archives: Vec<_> = std::fs::read_dir(sessions_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("index.corrupt-")
            })
            .collect();
        assert_eq!(archives.len(), 1);
        assert_eq!(std::fs::read(archives[0].path()).unwrap(), corrupt);
    }

    #[test]
    #[serial_test::serial]
    fn load_restores_a_valid_backup_when_primary_has_no_salvageable_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        write_empty_transcript("from-backup");
        let mut backup = SessionIndex::default();
        backup.upsert(SessionMeta {
            id: "from-backup".to_string(),
            title: "backup".to_string(),
            cwd: "/tmp".to_string(),
            model: "m".to_string(),
            updated_at: 11,
        });
        let sessions_dir = SessionIndex::sessions_dir().unwrap();
        std::fs::create_dir_all(sessions_dir).unwrap();
        std::fs::write(SessionIndex::index_path().unwrap(), b"{broken").unwrap();
        std::fs::write(
            SessionIndex::backup_path().unwrap(),
            serde_json::to_vec_pretty(&backup).unwrap(),
        )
        .unwrap();

        let loaded = SessionIndex::load().unwrap();

        assert_eq!(loaded, backup);
        assert_eq!(
            serde_json::from_slice::<SessionIndex>(
                &std::fs::read(SessionIndex::index_path().unwrap()).unwrap()
            )
            .unwrap(),
            backup
        );
    }

    #[test]
    #[serial_test::serial]
    fn load_reindexes_an_orphan_jsonl_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        let id = "orphan-session";
        let path = SessionIndex::session_path(id).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let user = Message::User {
            header: agent::message::Header::new(),
            content: vec![ContentBlock::Text {
                text: "recover this conversation\nwith all of its messages".to_string(),
            }],
        };
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&SessionHeader::current()).unwrap(),
                serde_json::to_string(&user).unwrap()
            ),
        )
        .unwrap();

        let loaded = SessionIndex::load().unwrap();

        let meta = loaded.find_prefix(id).unwrap();
        assert_eq!(meta.id, id);
        assert_eq!(meta.title, "recover this conversation");
        assert!(!meta.model.is_empty());
        assert!(SessionIndex::index_path().unwrap().exists());
        assert!(SessionIndex::backup_path().unwrap().exists());
    }

    #[test]
    #[serial_test::serial]
    fn load_prunes_metadata_without_transcript_and_persists_repair() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        write_empty_transcript("live-session");

        let mut index = SessionIndex::default();
        for (id, title) in [("live-session", "live"), ("ghost-session", "ghost")] {
            index.upsert(SessionMeta {
                id: id.to_string(),
                title: title.to_string(),
                cwd: "/tmp".to_string(),
                model: "m".to_string(),
                updated_at: 1,
            });
        }
        index.save().unwrap();

        let loaded = SessionIndex::load().unwrap();

        assert!(loaded.find_prefix("live-session").is_some());
        assert!(loaded.find_prefix("ghost-session").is_none());
        for path in [
            SessionIndex::index_path().unwrap(),
            SessionIndex::backup_path().unwrap(),
        ] {
            let repaired: SessionIndex =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert!(repaired.find_prefix("live-session").is_some());
            assert!(repaired.find_prefix("ghost-session").is_none());
        }
    }

    #[test]
    #[serial_test::serial]
    fn concurrent_update_transactions_preserve_every_session() {
        use std::sync::{Arc, Barrier};

        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        SessionIndex::default().save().unwrap();
        let worker_count = 12;
        let start = Arc::new(Barrier::new(worker_count + 1));
        let mut workers = Vec::new();
        for worker in 0..worker_count {
            let start = Arc::clone(&start);
            workers.push(std::thread::spawn(move || {
                start.wait();
                SessionIndex::update(|index| {
                    write_empty_transcript(&format!("session-{worker}"));
                    index.upsert(SessionMeta {
                        id: format!("session-{worker}"),
                        title: format!("worker {worker}"),
                        cwd: "/tmp".to_string(),
                        model: "m".to_string(),
                        updated_at: worker as u64,
                    });
                    Ok(())
                })
                .unwrap();
            }));
        }
        start.wait();
        for worker in workers {
            worker.join().unwrap();
        }

        let loaded = SessionIndex::load().unwrap();
        assert_eq!(loaded.sessions.len(), worker_count);
        for worker in 0..worker_count {
            assert!(loaded.find_prefix(&format!("session-{worker}")).is_some());
        }
    }

    #[test]
    #[serial_test::serial]
    fn index_lock_is_exclusive_across_processes() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        std::fs::create_dir_all(SessionIndex::sessions_dir().unwrap()).unwrap();
        let lock = lock_index().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "session_meta::tests::child_index_lock_probe",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(
                "ZODE_CHILD_INDEX_LOCK_PATH",
                SessionIndex::lock_path().unwrap(),
            )
            .output()
            .unwrap();
        drop(lock);
        assert!(
            output.status.success(),
            "child index lock probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn child_index_lock_probe() {
        let Some(path) = std::env::var_os("ZODE_CHILD_INDEX_LOCK_PATH") else {
            return;
        };
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        let error = FileExt::try_lock_exclusive(&file).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    }

    #[test]
    #[serial_test::serial]
    fn concurrent_saves_never_publish_malformed_json() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Barrier};

        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());

        fn large_index(marker: char) -> SessionIndex {
            let mut index = SessionIndex::default();
            index.upsert(SessionMeta {
                id: marker.to_string(),
                title: marker.to_string().repeat(2 * 1024 * 1024),
                cwd: "/tmp".to_string(),
                model: "m".to_string(),
                updated_at: marker as u64,
            });
            index
        }

        let first = Arc::new(large_index('a'));
        let second = Arc::new(large_index('b'));
        first.save().unwrap();

        let start = Arc::new(Barrier::new(3));
        let writers_done = Arc::new(AtomicBool::new(false));
        let spawn_writer = |index: Arc<SessionIndex>| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                for _ in 0..32 {
                    index.save().unwrap();
                }
            })
        };
        let writer_one = spawn_writer(first);
        let writer_two = spawn_writer(second);

        start.wait();
        let mut reads = 0;
        while !writers_done.load(Ordering::Acquire) || reads < 32 {
            let raw = std::fs::read_to_string(SessionIndex::index_path().unwrap()).unwrap();
            serde_json::from_str::<SessionIndex>(&raw)
                .unwrap_or_else(|error| panic!("reader observed malformed index: {error}"));
            reads += 1;
            if writer_one.is_finished() && writer_two.is_finished() {
                writers_done.store(true, Ordering::Release);
            }
            std::thread::yield_now();
        }
        writer_one.join().unwrap();
        writer_two.join().unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn save_roundtrips_without_temp_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        write_empty_transcript("roundtrip");
        let mut index = SessionIndex::default();
        index.upsert(SessionMeta {
            id: "roundtrip".to_string(),
            title: "saved atomically".to_string(),
            cwd: "/tmp".to_string(),
            model: "m".to_string(),
            updated_at: 42,
        });

        index.save().unwrap();

        let loaded = SessionIndex::load().unwrap();
        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(loaded.sessions[0].id, "roundtrip");
        let entries: Vec<_> = std::fs::read_dir(SessionIndex::sessions_dir().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(entries.contains(&std::ffi::OsString::from(".index.lock")));
        assert!(entries.contains(&std::ffi::OsString::from("index.json")));
        assert!(entries.contains(&std::ffi::OsString::from("index.json.bak")));
        assert!(entries.contains(&std::ffi::OsString::from("roundtrip.jsonl")));
        assert_eq!(entries.len(), 4);
    }

    #[test]
    #[serial_test::serial]
    fn failed_save_removes_its_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
        let sessions_dir = SessionIndex::sessions_dir().unwrap();
        std::fs::create_dir_all(sessions_dir.join("index.json")).unwrap();

        assert!(SessionIndex::default().save().is_err());

        let entries: Vec<_> = std::fs::read_dir(&sessions_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(entries.contains(&std::ffi::OsString::from(".index.lock")));
        assert!(entries.contains(&std::ffi::OsString::from("index.json")));
        assert_eq!(entries.len(), 2);
        assert!(!entries
            .iter()
            .any(|entry| entry.to_string_lossy().ends_with(".tmp")));
    }
}
