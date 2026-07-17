use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs4::fs_std::FileExt;

use zode_core::CoreError;

/// An exclusive advisory lock stored beside a persisted target.
pub struct AdvisoryFileLock {
    file: File,
    _lock_path: PathBuf,
}

impl AdvisoryFileLock {
    /// Wait until the target's advisory lock can be acquired.
    ///
    /// This is a blocking operation. Async callers must run it on a blocking
    /// worker before awaiting while holding the returned guard.
    pub fn acquire(target: &Path) -> Result<Self, CoreError> {
        let lock_path = lock_path_for(target);
        let file = open_lock_file(&lock_path)?;
        file.lock_exclusive()?;
        Ok(Self {
            file,
            _lock_path: lock_path,
        })
    }

    /// Acquire the target's advisory lock without waiting.
    pub fn try_acquire(target: &Path) -> Result<Self, CoreError> {
        let lock_path = lock_path_for(target);
        let file = open_lock_file(&lock_path)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Self {
                file,
                _lock_path: lock_path,
            }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                Err(CoreError::Busy(lock_path.display().to_string()))
            }
            Err(error) => Err(CoreError::Io(error)),
        }
    }
}

impl Drop for AdvisoryFileLock {
    fn drop(&mut self) {
        let _ = fs4::fs_std::FileExt::unlock(&self.file);
    }
}

fn lock_path_for(target: &Path) -> PathBuf {
    let mut path = target.as_os_str().to_os_string();
    path.push(".lock");
    PathBuf::from(path)
}

fn open_lock_file(path: &Path) -> Result<File, CoreError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?)
}

/// Publish bytes atomically using a synced sibling temporary file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.as_file_mut().write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|error| CoreError::Io(error.error))?;

    #[cfg(unix)]
    File::open(parent)?.sync_all()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_core::CoreError;

    #[test]
    fn second_writer_cannot_take_same_advisory_lock() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("index.json");

        let first = AdvisoryFileLock::acquire(&target).unwrap();
        assert!(matches!(
            AdvisoryFileLock::try_acquire(&target),
            Err(CoreError::Busy(_))
        ));

        drop(first);
        assert!(AdvisoryFileLock::try_acquire(&target).is_ok());
    }

    #[test]
    fn atomic_write_never_leaves_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("state.json");

        write_atomic(&target, br#"{"ok":true}"#).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), br#"{"ok":true}"#);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn advisory_lock_uses_target_dot_lock_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("index.json");

        let _lock = AdvisoryFileLock::acquire(&target).unwrap();
        let lock_path = lock_path_for(&target);

        assert_eq!(lock_path, dir.path().join("index.json.lock"));
        assert!(lock_path.is_file());
    }

    #[test]
    fn atomic_write_replaces_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("state.json");
        std::fs::write(&target, b"old").unwrap();

        write_atomic(&target, br#"{"new":true}"#).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), br#"{"new":true}"#);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_atomic_write_cleans_sibling_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("state.json");
        std::fs::create_dir(&target).unwrap();

        assert!(write_atomic(&target, b"cannot replace a directory").is_err());

        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from("state.json")]);
    }

    #[test]
    fn concurrent_atomic_writers_publish_one_complete_value() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("state.json");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let left = std::sync::Arc::new(vec![b'a'; 256 * 1024]);
        let right = std::sync::Arc::new(vec![b'b'; 256 * 1024]);
        let mut writers = Vec::new();
        for bytes in [left.clone(), right.clone()] {
            let target = target.clone();
            let barrier = barrier.clone();
            writers.push(std::thread::spawn(move || {
                barrier.wait();
                write_atomic(&target, &bytes).unwrap();
            }));
        }
        for writer in writers {
            writer.join().unwrap();
        }

        let published = std::fs::read(&target).unwrap();
        assert!(
            published.as_slice() == left.as_slice() || published.as_slice() == right.as_slice()
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn advisory_lock_is_exclusive_across_processes() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("index.json");
        let first = AdvisoryFileLock::acquire(&target).unwrap();

        run_child_lock_probe(&target, "busy");
        drop(first);
        run_child_lock_probe(&target, "free");
    }

    fn run_child_lock_probe(target: &Path, expected: &str) {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "persistence::tests::child_lock_probe",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("ZODE_CHILD_LOCK_TARGET", target)
            .env("ZODE_CHILD_LOCK_EXPECTED", expected)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child lock probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn child_lock_probe() {
        let Some(target) = std::env::var_os("ZODE_CHILD_LOCK_TARGET") else {
            return;
        };
        let expected = std::env::var("ZODE_CHILD_LOCK_EXPECTED").unwrap();
        let result = AdvisoryFileLock::try_acquire(Path::new(&target));
        match expected.as_str() {
            "busy" => assert!(matches!(result, Err(CoreError::Busy(_)))),
            "free" => assert!(result.is_ok()),
            other => panic!("unknown child probe expectation: {other}"),
        }
    }
}
