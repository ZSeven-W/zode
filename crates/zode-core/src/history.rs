//! Undo/redo for file-mutating tools. Snapshots before/after content of
//! FileWrite / FileEdit / Remove (per design spec §7.6 — shell/git ops are
//! NOT tracked). Move is excluded: it's a rename (`from`/`to`), not a
//! content edit, so it doesn't fit the content-snapshot model; a dedicated
//! rename-undo is post-v1.
//!
//! The EditHistoryHook captures `before` at BeforeToolUse (which the
//! QueryLoop fires for every tool *before* any dispatch) and `after` at
//! AfterToolUse (ok). Paths are resolved through the same WorkspacePolicy
//! the fs tools use, and pending before-snapshots are a per-path stack so a
//! failed call pops one entry without misaligning others.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use agent::hook::{HookEvent, HookHandler, HookOutcome};
use agent_tools_code::WorkspacePolicy;
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::CoreError;

#[derive(Debug, Clone)]
pub struct EditSnapshot {
    pub tool: String,
    pub path: PathBuf,
    /// Content before the edit; None = file did not exist.
    pub before: Option<String>,
    /// Content after the edit; None = file was removed.
    pub after: Option<String>,
}

/// Tools whose effect is a single-path content change we can snapshot.
pub fn is_tracked(tool: &str) -> bool {
    matches!(tool, "FileWrite" | "FileEdit" | "Remove")
}

#[derive(Debug)]
pub struct EditHistory {
    capacity: usize,
    undo_stack: Vec<EditSnapshot>,
    redo_stack: Vec<EditSnapshot>,
}

impl EditHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn record(&mut self, snap: EditSnapshot) {
        self.redo_stack.clear(); // a new edit invalidates the redo branch
        self.undo_stack.push(snap);
        while self.undo_stack.len() > self.capacity {
            self.undo_stack.remove(0);
        }
    }

    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }
    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }

    /// Restore the most recent edit's `before` state.
    pub fn undo(&mut self) -> Result<PathBuf, CoreError> {
        let snap = self
            .undo_stack
            .pop()
            .ok_or_else(|| CoreError::Other("nothing to undo".into()))?;
        apply_state(&snap.path, &snap.before)?;
        let path = snap.path.clone();
        self.redo_stack.push(snap);
        Ok(path)
    }

    /// Re-apply the most recently undone edit's `after` state.
    pub fn redo(&mut self) -> Result<PathBuf, CoreError> {
        let snap = self
            .redo_stack
            .pop()
            .ok_or_else(|| CoreError::Other("nothing to redo".into()))?;
        apply_state(&snap.path, &snap.after)?;
        let path = snap.path.clone();
        self.undo_stack.push(snap);
        Ok(path)
    }
}

/// Write `content` to `path`, or delete it if None.
fn apply_state(path: &PathBuf, content: &Option<String>) -> Result<(), CoreError> {
    match content {
        Some(c) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, c)?;
        }
        None => {
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

/// HookHandler that captures before/after snapshots into a shared
/// EditHistory. Registered on the engine's HookRunner.
///
/// Paths are resolved through the SAME WorkspacePolicy the fs tools use, so
/// the recorded path is exactly where the tool reads/writes (a raw relative
/// `path` would otherwise resolve against the process cwd, not the engine
/// cwd, and undo could touch the wrong file).
///
/// Pending before-snapshots are a per-path stack so a failed call pops one
/// entry and concurrent edits don't lose entries. Note: two *concurrent*
/// edits to the *same* path in one turn can only be coalesced (the hook
/// events carry no tool-use id) — undo is best-effort there, not
/// transactional.
#[derive(Debug)]
pub struct EditHistoryHook {
    history: Arc<Mutex<EditHistory>>,
    policy: Arc<WorkspacePolicy>,
    pending: Mutex<HashMap<PathBuf, Vec<Option<String>>>>,
}

impl EditHistoryHook {
    pub fn new(history: Arc<Mutex<EditHistory>>, policy: Arc<WorkspacePolicy>) -> Self {
        Self {
            history,
            policy,
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Resolve the tool's `path` input the same way the fs tools do
    /// (absolute, workspace-relative). None if absent or outside the policy.
    fn resolve(&self, input: &serde_json::Value) -> Option<PathBuf> {
        let raw = input.get("path").and_then(|v| v.as_str())?;
        self.policy.resolve(raw, false).ok()
    }
}

#[async_trait]
impl HookHandler for EditHistoryHook {
    async fn handle(&self, event: &HookEvent) -> HookOutcome {
        match event {
            HookEvent::BeforeToolUse { tool, input } if is_tracked(tool) => {
                if let Some(path) = self.resolve(input) {
                    // Skip directories: the content-snapshot model can't
                    // restore a removed tree (CONCERN — Remove of a dir).
                    if path.exists() && !path.is_file() {
                        return HookOutcome::Ok;
                    }
                    let before = std::fs::read_to_string(&path).ok();
                    self.pending
                        .lock()
                        .await
                        .entry(path)
                        .or_default()
                        .push(before);
                }
            }
            HookEvent::AfterToolUse {
                tool, input, ok, ..
            } if *ok && is_tracked(tool) => {
                if let Some(path) = self.resolve(input) {
                    let before = {
                        let mut pending = self.pending.lock().await;
                        let popped = pending.get_mut(&path).and_then(|stack| stack.pop());
                        if pending.get(&path).is_some_and(|s| s.is_empty()) {
                            pending.remove(&path);
                        }
                        popped
                    };
                    if let Some(before) = before {
                        let after = std::fs::read_to_string(&path).ok();
                        self.history.lock().await.record(EditSnapshot {
                            tool: tool.clone(),
                            path,
                            before,
                            after,
                        });
                    }
                }
            }
            HookEvent::PostToolUseFailure { tool, input, .. } if is_tracked(tool) => {
                // Tool failed — drop one pending before-snapshot for the path
                // so a later edit isn't mis-paired.
                if let Some(path) = self.resolve(input) {
                    let mut pending = self.pending.lock().await;
                    if let Some(stack) = pending.get_mut(&path) {
                        stack.pop();
                        if stack.is_empty() {
                            pending.remove(&path);
                        }
                    }
                }
            }
            _ => {}
        }
        HookOutcome::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_tools_filter() {
        assert!(is_tracked("FileWrite"));
        assert!(is_tracked("FileEdit"));
        assert!(is_tracked("Remove"));
        assert!(!is_tracked("Move")); // excluded — rename, not content edit
        assert!(!is_tracked("Bash"));
        assert!(!is_tracked("GitCommit"));
    }

    #[test]
    fn undo_restores_before_redo_restores_after() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.txt");
        std::fs::write(&f, "after\n").unwrap();

        let mut h = EditHistory::new(50);
        h.record(EditSnapshot {
            tool: "FileWrite".into(),
            path: f.clone(),
            before: None, // file did not exist before
            after: Some("after\n".into()),
        });

        h.undo().unwrap();
        assert!(!f.exists(), "undo of a create should delete the file");

        h.redo().unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "after\n");
    }

    #[test]
    fn undo_then_edit_clears_redo() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("y.txt");
        let mut h = EditHistory::new(50);
        h.record(EditSnapshot {
            tool: "FileWrite".into(),
            path: f.clone(),
            before: None,
            after: Some("v1".into()),
        });
        h.undo().unwrap();
        assert_eq!(h.redo_depth(), 1);
        h.record(EditSnapshot {
            tool: "FileWrite".into(),
            path: f,
            before: None,
            after: Some("v2".into()),
        });
        assert_eq!(h.redo_depth(), 0, "new edit must clear redo");
    }

    #[test]
    fn undo_on_empty_is_err() {
        let mut h = EditHistory::new(50);
        assert!(h.undo().is_err());
    }

    #[test]
    fn capacity_evicts_oldest() {
        let mut h = EditHistory::new(2);
        let dir = tempfile::tempdir().unwrap();
        for i in 0..3 {
            h.record(EditSnapshot {
                tool: "FileWrite".into(),
                path: dir.path().join(format!("{i}.txt")),
                before: None,
                after: Some("x".into()),
            });
        }
        assert_eq!(h.undo_depth(), 2);
    }

    #[tokio::test]
    async fn hook_resolves_relative_path_against_policy_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let policy = Arc::new(WorkspacePolicy::new(dir.path()).unwrap());
        let history = Arc::new(Mutex::new(EditHistory::new(50)));
        let hook = EditHistoryHook::new(history.clone(), policy);
        // A RELATIVE path — must resolve against the policy cwd, not the
        // process cwd (BLOCK regression).
        let input = serde_json::json!({ "path": "z.txt" });
        let resolved = dir.path().join("z.txt");

        hook.handle(&HookEvent::BeforeToolUse {
            tool: "FileWrite".into(),
            input: input.clone(),
        })
        .await;
        std::fs::write(&resolved, "created\n").unwrap(); // tool "runs"
        hook.handle(&HookEvent::AfterToolUse {
            tool: "FileWrite".into(),
            input,
            output: serde_json::json!({}),
            ok: true,
        })
        .await;

        assert_eq!(history.lock().await.undo_depth(), 1);
        let undone = history.lock().await.undo().unwrap();
        assert!(undone.ends_with("z.txt"), "resolved path: {undone:?}");
        assert!(undone.is_absolute(), "must store an absolute path");
        assert!(!resolved.exists(), "undo should remove the created file");
    }

    #[tokio::test]
    async fn hook_failed_tool_does_not_record() {
        let dir = tempfile::tempdir().unwrap();
        let policy = Arc::new(WorkspacePolicy::new(dir.path()).unwrap());
        let history = Arc::new(Mutex::new(EditHistory::new(50)));
        let hook = EditHistoryHook::new(history.clone(), policy);
        let input = serde_json::json!({ "path": "f.txt" });

        hook.handle(&HookEvent::BeforeToolUse {
            tool: "FileWrite".into(),
            input: input.clone(),
        })
        .await;
        hook.handle(&HookEvent::PostToolUseFailure {
            tool: "FileWrite".into(),
            input,
            error: "boom".into(),
        })
        .await;
        assert_eq!(history.lock().await.undo_depth(), 0);
    }
}
