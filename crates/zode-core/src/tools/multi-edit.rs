//! `MultiEdit` — several exact-substring replacements in ONE file as a
//! single atomic tool call.
//!
//! agent-tools-code ships only single-hunk `FileEdit`, so a multi-hunk
//! change costs N sequential calls (N approvals, N writes, and a torn file
//! if the model stops midway). `MultiEdit` validates EVERY edit against the
//! in-memory content first — edits apply sequentially, each seeing the
//! previous one's output — and only when all of them match does it write
//! the file once. Any failure leaves the file untouched.
//!
//! Semantics per edit mirror `FileEdit` exactly: `old_string` must be
//! non-empty and match exactly once unless `replace_all`. The tool is
//! `Mutating` (auto-gated), tracked by the undo history and reminder hooks
//! (they match the tool name), and shares `FileEdit`'s policy limits.

use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use agent_tools_code::WorkspacePolicy;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug)]
pub struct MultiEditTool {
    policy: Arc<WorkspacePolicy>,
}

impl MultiEditTool {
    pub fn new(policy: Arc<WorkspacePolicy>) -> Self {
        Self { policy }
    }
}

#[derive(Debug, Deserialize)]
struct EditSpec {
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

#[derive(Debug, Deserialize)]
struct MultiEditInput {
    path: String,
    edits: Vec<EditSpec>,
}

#[async_trait]
impl Tool for MultiEditTool {
    fn name(&self) -> &str {
        "MultiEdit"
    }
    fn description(&self) -> &str {
        "Apply several exact-substring replacements to one file atomically: edits are \
         validated in order against the in-memory content (each sees the previous edit's \
         result) and the file is written once only if every edit matches. Same matching \
         rules as FileEdit (old_string must match exactly once unless replace_all)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "edits": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": {"type": "string"},
                            "new_string": {"type": "string"},
                            "replace_all": {"type": "boolean", "default": false},
                        },
                        "required": ["old_string", "new_string"]
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let parsed: MultiEditInput = serde_json::from_value(input)
            .map_err(|e| AgentError::other(format!("MultiEdit invalid input: {e}")))?;
        if parsed.edits.is_empty() {
            return Err(AgentError::other("MultiEdit needs at least one edit"));
        }
        let resolved = self
            .policy
            .resolve(&parsed.path, true)
            .map_err(|e| AgentError::other(format!("MultiEdit: {e}")))?;
        let meta = tokio::fs::metadata(&resolved)
            .await
            .map_err(|e| AgentError::other(format!("MultiEdit stat '{}': {e}", parsed.path)))?;
        self.policy
            .check_size(meta.len())
            .map_err(|e| AgentError::other(format!("MultiEdit: {e}")))?;
        let bytes = tokio::fs::read(&resolved)
            .await
            .map_err(|e| AgentError::other(format!("MultiEdit read '{}': {e}", parsed.path)))?;
        self.policy
            .check_size(bytes.len() as u64)
            .map_err(|e| AgentError::other(format!("MultiEdit: {e}")))?;
        let mut text = String::from_utf8(bytes).map_err(|e| {
            AgentError::other(format!("MultiEdit '{}' is not UTF-8: {e}", parsed.path))
        })?;

        // Validate + apply every edit IN MEMORY; nothing touches disk until
        // all of them succeeded.
        let mut replacements = 0usize;
        for (index, edit) in parsed.edits.iter().enumerate() {
            let n = index + 1;
            if edit.old_string.is_empty() {
                return Err(AgentError::other(format!(
                    "MultiEdit edit {n}: old_string must be non-empty"
                )));
            }
            if edit.old_string == edit.new_string {
                return Err(AgentError::other(format!(
                    "MultiEdit edit {n}: old_string and new_string are identical"
                )));
            }
            let count = text.matches(&edit.old_string).count();
            if count == 0 {
                return Err(AgentError::other(format!(
                    "MultiEdit edit {n}: old_string not found in '{}'. Edits apply \
                     sequentially — an earlier edit may have changed this text; nothing \
                     was written. Re-read the file and retry with current text.",
                    parsed.path
                )));
            }
            if count > 1 && !edit.replace_all {
                return Err(AgentError::other(format!(
                    "MultiEdit edit {n}: old_string is ambiguous in '{}' ({count} \
                     matches). Add surrounding context or set replace_all; nothing was \
                     written.",
                    parsed.path
                )));
            }
            if edit.replace_all {
                text = text.replace(&edit.old_string, &edit.new_string);
                replacements += count;
            } else {
                text = text.replacen(&edit.old_string, &edit.new_string, 1);
                replacements += 1;
            }
        }
        // A replacement can grow the file past the policy cap.
        self.policy
            .check_size(text.len() as u64)
            .map_err(|e| AgentError::other(format!("MultiEdit: {e}")))?;
        self.policy
            .write_file_tracked(&resolved, text.as_bytes(), &ctx.abort)
            .await
            .map_err(|e| AgentError::other(format!("MultiEdit write '{}': {e}", parsed.path)))?;
        Ok(json!({
            "path": resolved.display().to_string(),
            "edits_applied": parsed.edits.len(),
            "replacements": replacements,
            "size_bytes": text.len(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_tools_code::WorkspacePolicy;

    fn tool_for(dir: &std::path::Path) -> (MultiEditTool, ToolUseContext) {
        let policy = Arc::new(WorkspacePolicy::new(dir).expect("policy"));
        (
            MultiEditTool::new(policy),
            ToolUseContext::new(dir.to_path_buf()),
        )
    }

    #[tokio::test]
    async fn applies_sequential_edits_in_one_write() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "alpha beta beta gamma").unwrap();
        let (tool, ctx) = tool_for(dir.path());

        let out = tool
            .call(
                &ctx,
                json!({
                    "path": "a.txt",
                    "edits": [
                        {"old_string": "alpha", "new_string": "ALPHA"},
                        {"old_string": "beta", "new_string": "B", "replace_all": true},
                        // Sees the output of the previous edits.
                        {"old_string": "ALPHA B B", "new_string": "done"},
                    ]
                }),
            )
            .await
            .unwrap();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), "done gamma");
        assert_eq!(out["edits_applied"], 3);
        assert_eq!(out["replacements"], 4);
    }

    #[tokio::test]
    async fn any_failing_edit_leaves_the_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("b.txt");
        std::fs::write(&file, "one two three").unwrap();
        let (tool, ctx) = tool_for(dir.path());

        let err = tool
            .call(
                &ctx,
                json!({
                    "path": "b.txt",
                    "edits": [
                        {"old_string": "one", "new_string": "1"},
                        {"old_string": "missing", "new_string": "x"},
                    ]
                }),
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("edit 2"), "{err}");
        assert!(err.to_string().contains("nothing was written"), "{err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one two three");
    }

    #[tokio::test]
    async fn ambiguous_match_without_replace_all_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("c.txt");
        std::fs::write(&file, "dup dup").unwrap();
        let (tool, ctx) = tool_for(dir.path());

        let err = tool
            .call(
                &ctx,
                json!({
                    "path": "c.txt",
                    "edits": [{"old_string": "dup", "new_string": "x"}]
                }),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("ambiguous"), "{err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "dup dup");
    }
}
