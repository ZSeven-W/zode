use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use agent::hook::{HookEvent, HookOutcome, RustHookHandler};
use serde_json::json;

/// Durable JSONL trace of tool calls for post-run diagnosis and exports.
#[derive(Debug, Clone)]
pub struct ToolTrace {
    path: Arc<PathBuf>,
}

impl ToolTrace {
    pub fn new(cwd: impl AsRef<Path>) -> Self {
        let path = cwd
            .as_ref()
            .join(".zode")
            .join("traces")
            .join(format!("tool-{}.jsonl", uuid::Uuid::new_v4()));
        Self::with_path(path)
    }

    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Arc::new(path.into()),
        }
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn hook(&self) -> RustHookHandler {
        let path = self.path.clone();
        RustHookHandler::new("tool-trace-jsonl", move |event| {
            let Some(record) = trace_record(event) else {
                return HookOutcome::Ok;
            };
            if let Some(parent) = path.parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    tracing::warn!(
                        path = %parent.display(),
                        error = %err,
                        "failed to create tool trace directory",
                    );
                    return HookOutcome::Ok;
                }
            }
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path.as_ref())
            {
                Ok(mut file) => {
                    if let Err(err) = serde_json::to_writer(&mut file, &record)
                        .and_then(|_| file.write_all(b"\n").map_err(serde_json::Error::io))
                    {
                        tracing::warn!(
                            path = %path.display(),
                            error = %err,
                            "failed to append tool trace record",
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "failed to open tool trace file",
                    );
                }
            }
            HookOutcome::Ok
        })
    }
}

fn trace_record(event: &HookEvent) -> Option<serde_json::Value> {
    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    match event {
        HookEvent::AfterToolUse {
            tool,
            input,
            output,
            ok,
        } => Some(json!({
            "ts_ms": ts_ms,
            "event": "after_tool_use",
            "tool": tool,
            "input": input,
            "output": output,
            "ok": ok,
        })),
        HookEvent::PostToolUseFailure { tool, input, error } => Some(json!({
            "ts_ms": ts_ms,
            "event": "post_tool_use_failure",
            "tool": tool,
            "input": input,
            "error": error,
            "ok": false,
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::hook::HookHandler;

    #[tokio::test]
    async fn writes_full_successful_tool_output_as_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let trace = ToolTrace::with_path(dir.path().join("trace.jsonl"));
        let hook = trace.hook();
        let long_output = "x".repeat(1024);

        hook.handle(&HookEvent::AfterToolUse {
            tool: "Bash".into(),
            input: json!({"cmd": "printf long"}),
            output: json!({"stdout": long_output, "stderr": "", "exit_code": 0}),
            ok: true,
        })
        .await;

        let content = std::fs::read_to_string(trace.path()).unwrap();
        let value: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(value["event"], "after_tool_use");
        assert_eq!(value["tool"], "Bash");
        assert_eq!(value["output"]["stdout"].as_str().unwrap().len(), 1024);
        assert_eq!(value["ok"], true);
    }

    #[tokio::test]
    async fn writes_failed_tool_error_as_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let trace = ToolTrace::with_path(dir.path().join("trace.jsonl"));
        let hook = trace.hook();

        hook.handle(&HookEvent::PostToolUseFailure {
            tool: "FileEdit".into(),
            input: json!({"path": "src/lib.rs"}),
            error: "old_string was not found".into(),
        })
        .await;

        let content = std::fs::read_to_string(trace.path()).unwrap();
        let value: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(value["event"], "post_tool_use_failure");
        assert_eq!(value["tool"], "FileEdit");
        assert_eq!(value["error"], "old_string was not found");
        assert_eq!(value["ok"], false);
    }
}
