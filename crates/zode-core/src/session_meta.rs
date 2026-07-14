//! Session metadata index. agent::Session owns the JSONL transcript;
//! this index tracks id/title/cwd/model/updated_at for listing and
//! resuming. Stored at `<config_dir>/sessions/index.json`.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ConfigManager;
use crate::error::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub model: String,
    /// Unix seconds. Stamped by the caller.
    pub updated_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionIndex {
    pub sessions: Vec<SessionMeta>,
}

impl SessionIndex {
    fn sessions_dir() -> Result<PathBuf, CoreError> {
        Ok(ConfigManager::config_dir()?.join("sessions"))
    }

    fn index_path() -> Result<PathBuf, CoreError> {
        Ok(Self::sessions_dir()?.join("index.json"))
    }

    pub fn session_path(id: &str) -> Result<PathBuf, CoreError> {
        Ok(Self::sessions_dir()?.join(format!("{id}.jsonl")))
    }

    pub fn load() -> Result<Self, CoreError> {
        let path = Self::index_path()?;
        match std::fs::read_to_string(&path) {
            Ok(s) => Ok(serde_json::from_str(&s)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(CoreError::Io(e)),
        }
    }

    pub fn save(&self) -> Result<(), CoreError> {
        let dir = Self::sessions_dir()?;
        std::fs::create_dir_all(&dir)?;
        let json = serde_json::to_vec_pretty(self)?;
        write_atomic(&Self::index_path()?, &json)?;
        Ok(())
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
        let path = Self::session_path(id)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(CoreError::Io(e)),
        }
        let mut idx = Self::load()?;
        if idx.remove(id) {
            idx.save()?;
        }
        Ok(())
    }

    pub fn set_title(id: &str, title: String) -> Result<(), CoreError> {
        let mut idx = Self::load()?;
        let Some(meta) = idx.sessions.iter_mut().find(|m| m.id == id) else {
            return Err(CoreError::Other(format!("session not found: {id}")));
        };
        meta.title = title;
        idx.save()
    }

    /// Most recently updated session.
    pub fn latest(&self) -> Option<&SessionMeta> {
        self.sessions.iter().max_by_key(|m| m.updated_at)
    }

    /// First session whose id starts with `prefix`.
    pub fn find_prefix(&self, prefix: &str) -> Option<&SessionMeta> {
        self.sessions.iter().find(|m| m.id.starts_with(prefix))
    }

    /// Sessions newest-first (for the picker UI in Phase 07).
    pub fn newest_first(&self) -> Vec<&SessionMeta> {
        let mut v: Vec<&SessionMeta> = self.sessions.iter().collect();
        v.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
        v
    }
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

    #[test]
    #[serial_test::serial]
    fn upsert_then_latest_and_find() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());

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

    #[test]
    #[serial_test::serial]
    fn set_title_updates_existing_session() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", dir.path());
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
        assert_eq!(entries, [std::ffi::OsString::from("index.json")]);
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
        assert_eq!(entries, [std::ffi::OsString::from("index.json")]);
    }
}
