use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use agent::hook::{HookEvent, HookHandler, HookOutcome};
use async_trait::async_trait;
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::sessions::journal::SessionJournal;
use crate::CoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointRecord {
    pub schema_version: u32,
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub created_at_ms: u64,
    pub cwd: PathBuf,
    pub event_head: Option<u64>,
    /// Transcript length before the turn began.
    #[serde(default)]
    pub message_count: usize,
    pub status: CheckpointStatus,
    pub files: Vec<FileCheckpoint>,
    pub non_reversible_effects: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointStatus {
    Active,
    Completed,
    Rewound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCheckpoint {
    pub path: PathBuf,
    pub before: PathState,
    pub after: Option<PathState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PathState {
    Missing,
    File {
        hash: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        unix_mode: Option<u32>,
    },
    Directory,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewindPreview {
    pub checkpoint_id: String,
    pub restore: Vec<PathBuf>,
    pub remove: Vec<PathBuf>,
    pub conflicts: Vec<PathBuf>,
    pub non_reversible_effects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewindResult {
    pub checkpoint_id: String,
    pub restored: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub partial: bool,
}

#[derive(Debug)]
pub struct CheckpointBuilder {
    session_dir: PathBuf,
    record: CheckpointRecord,
    captured: BTreeSet<PathBuf>,
}

/// Session-scoped coordinator registered as an engine hook. A surface starts
/// one checkpoint per user turn; mutating tools then contribute their
/// pre-state without needing tool-specific wrappers.
#[derive(Debug, Clone, Default)]
pub struct CheckpointTracker {
    active: Arc<Mutex<Option<CheckpointBuilder>>>,
}

impl CheckpointTracker {
    pub fn begin(
        &self,
        session_dir: PathBuf,
        session_id: impl Into<String>,
        turn_id: impl Into<String>,
        cwd: PathBuf,
        message_count: usize,
    ) -> Result<String, CoreError> {
        let builder = CheckpointBuilder::begin_with_message_count(
            session_dir,
            session_id,
            turn_id,
            cwd,
            message_count,
        )?;
        let id = builder.id().to_string();
        *self
            .active
            .lock()
            .map_err(|_| CoreError::Other("checkpoint tracker lock poisoned".into()))? =
            Some(builder);
        Ok(id)
    }

    pub fn finish(&self) -> Result<Option<CheckpointRecord>, CoreError> {
        let builder = self
            .active
            .lock()
            .map_err(|_| CoreError::Other("checkpoint tracker lock poisoned".into()))?
            .take();
        builder.map(CheckpointBuilder::finish).transpose()
    }

    pub fn cancel(&self) {
        if let Ok(mut active) = self.active.lock() {
            active.take();
        }
    }

    fn with_active(&self, action: impl FnOnce(&mut CheckpointBuilder)) {
        if let Ok(mut active) = self.active.lock() {
            if let Some(builder) = active.as_mut() {
                action(builder);
            }
        }
    }
}

#[async_trait]
impl HookHandler for CheckpointTracker {
    async fn handle(&self, event: &HookEvent) -> HookOutcome {
        let HookEvent::BeforeToolUse { tool, input } = event else {
            return HookOutcome::Ok;
        };
        self.with_active(|builder| {
            for path in mutation_paths(tool, input) {
                if let Err(error) = builder.capture_before(Path::new(path)) {
                    builder.note_external_effect(format!(
                        "could not checkpoint {path} for {tool}: {error}"
                    ));
                }
            }
            if is_external_effect(tool) {
                let summary = input
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("external side effect");
                builder.note_external_effect(format!("{tool}: {summary}"));
            }
        });
        HookOutcome::Ok
    }
}

fn mutation_paths<'a>(tool: &str, input: &'a serde_json::Value) -> Vec<&'a str> {
    let keys: &[&str] = match tool {
        "Move" => &["from", "to"],
        "FileWrite" | "FileEdit" | "Remove" | "Mkdir" | "NotebookEdit" => {
            &["path", "file_path", "notebook_path"]
        }
        _ => &[],
    };
    keys.iter()
        .filter_map(|key| input.get(key).and_then(serde_json::Value::as_str))
        .collect()
}

fn is_external_effect(tool: &str) -> bool {
    matches!(tool, "Bash" | "BashRun" | "KillShell") || tool.starts_with("Git")
}

impl CheckpointBuilder {
    pub fn begin(
        session_dir: PathBuf,
        session_id: impl Into<String>,
        turn_id: impl Into<String>,
        cwd: PathBuf,
    ) -> Result<Self, CoreError> {
        Self::begin_with_message_count(session_dir, session_id, turn_id, cwd, 0)
    }

    pub fn begin_with_message_count(
        session_dir: PathBuf,
        session_id: impl Into<String>,
        turn_id: impl Into<String>,
        cwd: PathBuf,
        message_count: usize,
    ) -> Result<Self, CoreError> {
        std::fs::create_dir_all(session_dir.join("snapshots"))?;
        let event_head = SessionJournal::new(session_dir.clone())
            .head()?
            .logical_head;
        Ok(Self {
            session_dir,
            record: CheckpointRecord {
                schema_version: 1,
                id: uuid::Uuid::new_v4().simple().to_string(),
                session_id: session_id.into(),
                turn_id: turn_id.into(),
                created_at_ms: now_ms(),
                cwd,
                event_head,
                message_count,
                status: CheckpointStatus::Active,
                files: Vec::new(),
                non_reversible_effects: Vec::new(),
            },
            captured: BTreeSet::new(),
        })
    }

    pub fn id(&self) -> &str {
        &self.record.id
    }

    pub fn capture_before(&mut self, path: &Path) -> Result<(), CoreError> {
        let relative = safe_relative(&self.record.cwd, path)?;
        if !self.captured.insert(relative.clone()) {
            return Ok(());
        }
        let absolute = self.record.cwd.join(&relative);
        let before = capture_state(&self.session_dir, &absolute)?;
        if matches!(before, PathState::Directory | PathState::Symlink) {
            self.record.non_reversible_effects.push(format!(
                "{} is not a regular file and cannot be fully restored",
                relative.display()
            ));
        }
        self.record.files.push(FileCheckpoint {
            path: relative,
            before,
            after: None,
        });
        Ok(())
    }

    pub fn note_external_effect(&mut self, effect: impl Into<String>) {
        self.record.non_reversible_effects.push(effect.into());
    }

    pub fn finish(mut self) -> Result<CheckpointRecord, CoreError> {
        for file in &mut self.record.files {
            file.after = Some(capture_state(
                &self.session_dir,
                &self.record.cwd.join(&file.path),
            )?);
        }
        self.record.status = CheckpointStatus::Completed;
        append_checkpoint(&self.session_dir, &self.record)?;
        SessionJournal::new(self.session_dir.clone())
            .append("checkpoint.completed", serde_json::to_value(&self.record)?)?;
        Ok(self.record)
    }
}

pub fn load_checkpoint(session_dir: &Path, id: &str) -> Result<CheckpointRecord, CoreError> {
    let path = session_dir.join("checkpoints.jsonl");
    let file = File::open(&path)?;
    let mut found = None;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let checkpoint: CheckpointRecord = serde_json::from_str(&line)?;
        if checkpoint.id == id {
            found = Some(checkpoint);
        }
    }
    found.ok_or_else(|| CoreError::Other(format!("checkpoint not found: {id}")))
}

pub fn list_checkpoints(session_dir: &Path) -> Result<Vec<CheckpointRecord>, CoreError> {
    let path = session_dir.join("checkpoints.jsonl");
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut by_id = std::collections::BTreeMap::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let checkpoint: CheckpointRecord = serde_json::from_str(&line)?;
        by_id.insert(checkpoint.id.clone(), checkpoint);
    }
    Ok(by_id.into_values().collect())
}

pub fn rewind_preview(
    session_dir: &Path,
    checkpoint: &CheckpointRecord,
) -> Result<RewindPreview, CoreError> {
    let mut preview = RewindPreview {
        checkpoint_id: checkpoint.id.clone(),
        restore: Vec::new(),
        remove: Vec::new(),
        conflicts: Vec::new(),
        non_reversible_effects: checkpoint.non_reversible_effects.clone(),
    };
    for file in &checkpoint.files {
        let absolute = checkpoint.cwd.join(&file.path);
        let current = capture_state(session_dir, &absolute)?;
        if file.after.as_ref().is_some_and(|after| after != &current) {
            preview.conflicts.push(file.path.clone());
            continue;
        }
        match &file.before {
            PathState::Missing => preview.remove.push(file.path.clone()),
            PathState::File { .. } | PathState::Directory => {
                preview.restore.push(file.path.clone())
            }
            PathState::Symlink => preview
                .non_reversible_effects
                .push(format!("cannot restore symlink {}", file.path.display())),
        }
    }
    Ok(preview)
}

pub fn rewind_apply(
    session_dir: &Path,
    checkpoint: &CheckpointRecord,
) -> Result<RewindResult, CoreError> {
    let preview = rewind_preview(session_dir, checkpoint)?;
    if !preview.conflicts.is_empty() {
        return Err(CoreError::Other(format!(
            "rewind conflicts with later changes: {}",
            preview
                .conflicts
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let mut restored = Vec::new();
    let mut removed = Vec::new();
    for file in &checkpoint.files {
        let absolute = checkpoint.cwd.join(&file.path);
        match &file.before {
            PathState::Missing => {
                if absolute.is_file() {
                    std::fs::remove_file(&absolute)?;
                } else if absolute.is_dir() {
                    std::fs::remove_dir(&absolute)?;
                }
                removed.push(file.path.clone());
            }
            PathState::File { hash, unix_mode } => {
                restore_file(session_dir, &absolute, hash, *unix_mode)?;
                restored.push(file.path.clone());
            }
            PathState::Directory => {
                std::fs::create_dir_all(&absolute)?;
                restored.push(file.path.clone());
            }
            PathState::Symlink => {}
        }
    }
    let journal = SessionJournal::new(session_dir.to_path_buf());
    if let Some(event_head) = checkpoint.event_head {
        journal.set_logical_head(event_head)?;
    }
    journal.append(
        "rewind.applied",
        serde_json::json!({
            "checkpointId": checkpoint.id,
            "partial": !preview.non_reversible_effects.is_empty(),
        }),
    )?;
    let mut rewound = checkpoint.clone();
    rewound.status = CheckpointStatus::Rewound;
    append_checkpoint(session_dir, &rewound)?;
    Ok(RewindResult {
        checkpoint_id: checkpoint.id.clone(),
        restored,
        removed,
        partial: !preview.non_reversible_effects.is_empty(),
    })
}

fn append_checkpoint(session_dir: &Path, record: &CheckpointRecord) -> Result<(), CoreError> {
    let lock_path = session_dir.join("checkpoint.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    FileExt::lock_exclusive(&lock)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(session_dir.join("checkpoints.jsonl"))?;
    serde_json::to_writer(&mut file, record)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn capture_state(session_dir: &Path, path: &Path) -> Result<PathState, CoreError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PathState::Missing)
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        return Ok(PathState::Symlink);
    }
    if metadata.is_dir() {
        return Ok(PathState::Directory);
    }
    let bytes = std::fs::read(path)?;
    let hash = store_snapshot(session_dir, &bytes)?;
    Ok(PathState::File {
        hash,
        unix_mode: unix_mode(&metadata),
    })
}

fn store_snapshot(session_dir: &Path, bytes: &[u8]) -> Result<String, CoreError> {
    let hash = format!("{:x}", Sha256::digest(bytes));
    let path = session_dir.join("snapshots").join(&hash);
    if !path.exists() {
        let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(path.parent().expect("snapshot parent"))?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if let Err(error) = std::fs::rename(&tmp, &path) {
            let _ = std::fs::remove_file(&tmp);
            if !path.exists() {
                return Err(error.into());
            }
        }
    }
    Ok(hash)
}

fn restore_file(
    session_dir: &Path,
    path: &Path,
    hash: &str,
    unix_mode: Option<u32>,
) -> Result<(), CoreError> {
    let bytes = std::fs::read(session_dir.join("snapshots").join(hash))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("{}.rewind", uuid::Uuid::new_v4().simple()));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    set_unix_mode(&tmp, unix_mode)?;
    #[cfg(windows)]
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn safe_relative(cwd: &Path, path: &Path) -> Result<PathBuf, CoreError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(CoreError::Other("path escapes workspace".into()));
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
        .strip_prefix(cwd)
        .map(Path::to_path_buf)
        .map_err(|_| CoreError::Other(format!("path escapes workspace: {}", path.display())))
}

#[cfg(unix)]
fn unix_mode(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(metadata.permissions().mode())
}

#[cfg(not(unix))]
fn unix_mode(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn set_unix_mode(path: &Path, mode: Option<u32>) -> Result<(), CoreError> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_unix_mode(_path: &Path, _mode: Option<u32>) -> Result<(), CoreError> {
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewind_restores_content_and_detects_later_conflicts() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let file = workspace.path().join("file.txt");
        std::fs::write(&file, b"before").unwrap();
        let mut builder = CheckpointBuilder::begin(
            root.path().to_path_buf(),
            "session",
            "turn",
            workspace.path().to_path_buf(),
        )
        .unwrap();
        builder.capture_before(&file).unwrap();
        std::fs::write(&file, b"after").unwrap();
        let checkpoint = builder.finish().unwrap();
        let preview = rewind_preview(root.path(), &checkpoint).unwrap();
        assert_eq!(preview.restore, [PathBuf::from("file.txt")]);
        rewind_apply(root.path(), &checkpoint).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"before");

        std::fs::write(&file, b"user change").unwrap();
        let conflict = rewind_preview(root.path(), &checkpoint).unwrap();
        assert_eq!(conflict.conflicts, [PathBuf::from("file.txt")]);
    }

    #[test]
    fn rewind_removes_a_file_created_after_checkpoint() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let file = workspace.path().join("new.txt");
        let mut builder = CheckpointBuilder::begin(
            root.path().to_path_buf(),
            "session",
            "turn",
            workspace.path().to_path_buf(),
        )
        .unwrap();
        builder.capture_before(&file).unwrap();
        std::fs::write(&file, b"created").unwrap();
        let checkpoint = builder.finish().unwrap();
        rewind_apply(root.path(), &checkpoint).unwrap();
        assert!(!file.exists());
    }

    #[tokio::test]
    async fn tracker_hook_captures_mutations_and_external_effects() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let file = workspace.path().join("tracked.txt");
        std::fs::write(&file, b"before").unwrap();
        let tracker = CheckpointTracker::default();
        tracker
            .begin(
                root.path().to_path_buf(),
                "session",
                "turn",
                workspace.path().to_path_buf(),
                0,
            )
            .unwrap();
        tracker
            .handle(&HookEvent::BeforeToolUse {
                tool: "FileEdit".into(),
                input: serde_json::json!({"path": file}),
            })
            .await;
        tracker
            .handle(&HookEvent::BeforeToolUse {
                tool: "Bash".into(),
                input: serde_json::json!({"command": "touch remote"}),
            })
            .await;
        std::fs::write(&file, b"after").unwrap();
        let checkpoint = tracker.finish().unwrap().unwrap();
        assert_eq!(checkpoint.files.len(), 1);
        assert_eq!(checkpoint.non_reversible_effects.len(), 1);
    }
}
