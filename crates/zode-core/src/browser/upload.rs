use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::approval::ApprovalGate;
use crate::gated_tool::PermissionGatedTool;

use super::backend::{BrowserTarget, ClickTarget};
use super::session::BrowserSession;

#[derive(Debug)]
pub struct BrowserUploadTool {
    session: Arc<BrowserSession>,
    gate: Arc<dyn ApprovalGate>,
    /// See `BrowserToolDeps::target_override`: pins every lease (preflight
    /// URL hint + execution) to this target instead of the session-wide one.
    target_override: Option<BrowserTarget>,
}

#[derive(Debug)]
struct UploadExecution {
    session: Arc<BrowserSession>,
    target_override: Option<BrowserTarget>,
}

#[async_trait]
impl Tool for UploadExecution {
    fn name(&self) -> &str {
        "browser_upload"
    }

    fn description(&self) -> &str {
        "Execute a preflighted browser file upload."
    }

    fn input_schema(&self) -> Value {
        json!({"type":"object"})
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let target = if let Some(selector) = input.get("selector").and_then(Value::as_str) {
            ClickTarget::Selector(selector.to_string())
        } else {
            ClickTarget::Ref(
                input
                    .get("ref")
                    .and_then(Value::as_u64)
                    .and_then(|reference| u32::try_from(reference).ok())
                    .ok_or_else(|| AgentError::other("preflighted upload target missing"))?,
            )
        };
        let paths = input
            .get("paths")
            .and_then(Value::as_array)
            .ok_or_else(|| AgentError::other("preflighted upload paths missing"))?
            .iter()
            .map(|path| {
                path.as_str()
                    .map(PathBuf::from)
                    .ok_or_else(|| AgentError::other("preflighted upload path is not a string"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let effective = self
            .target_override
            .clone()
            .unwrap_or_else(|| self.session.target());
        let lease = self
            .session
            .lease_as(effective)
            .await
            .map_err(|error| AgentError::other(error.to_string()))?;
        lease
            .backend()
            .set_file_input(&target, &paths)
            .await
            .map_err(|error| AgentError::other(error.to_string()))?;
        Ok(json!({"ok": true, "count": paths.len()}))
    }
}

impl BrowserUploadTool {
    pub fn new(session: Arc<BrowserSession>, gate: Arc<dyn ApprovalGate>) -> Self {
        Self {
            session,
            gate,
            target_override: None,
        }
    }

    /// Pin every lease this tool takes to `target` (see
    /// `BrowserToolDeps::target_override`).
    pub fn with_target_override(mut self, target: Option<BrowserTarget>) -> Self {
        self.target_override = target;
        self
    }

    fn effective_target(&self) -> BrowserTarget {
        self.target_override
            .clone()
            .unwrap_or_else(|| self.session.target())
    }

    fn preflight(input: &Value) -> Result<(ClickTarget, Vec<PathBuf>, Value), AgentError> {
        let selector = input.get("selector").and_then(Value::as_str);
        let reference = input.get("ref").and_then(Value::as_u64);
        let target = match (selector, reference) {
            (Some(selector), None) => ClickTarget::Selector(selector.to_string()),
            (None, Some(reference)) => {
                let reference = u32::try_from(reference)
                    .map_err(|_| AgentError::other("upload: 'ref' is out of range"))?;
                ClickTarget::Ref(reference)
            }
            _ => {
                return Err(AgentError::other(
                    "upload target requires exactly one of 'selector' or 'ref'",
                ));
            }
        };
        let raw_paths = input
            .get("paths")
            .and_then(Value::as_array)
            .ok_or_else(|| AgentError::other("upload: 'paths' array required"))?;
        if raw_paths.is_empty() {
            return Err(AgentError::other("upload: 'paths' must not be empty"));
        }

        let mut canonical = Vec::with_capacity(raw_paths.len());
        let mut seen = HashSet::with_capacity(raw_paths.len());
        let mut files = Vec::with_capacity(raw_paths.len());
        for raw in raw_paths {
            let raw = raw
                .as_str()
                .ok_or_else(|| AgentError::other("upload: every path must be a string"))?;
            let path = PathBuf::from(raw);
            if !path.is_absolute() {
                return Err(AgentError::other(format!(
                    "upload path must be absolute: {raw}"
                )));
            }
            let path = std::fs::canonicalize(&path)
                .map_err(|e| AgentError::other(format!("upload path {raw:?}: {e}")))?;
            let metadata = std::fs::metadata(&path)
                .map_err(|e| AgentError::other(format!("upload path {:?}: {e}", path)))?;
            if !metadata.file_type().is_file() {
                return Err(AgentError::other(format!(
                    "upload path is not a regular file: {}",
                    path.display()
                )));
            }
            if !seen.insert(path.clone()) {
                return Err(AgentError::other(format!(
                    "duplicate canonical upload path: {}",
                    path.display()
                )));
            }
            files.push(json!({"path": path, "size": metadata.len()}));
            canonical.push(path);
        }
        Ok((
            target,
            canonical,
            json!({"files": files, "count": files.len()}),
        ))
    }
}

#[async_trait]
impl Tool for BrowserUploadTool {
    fn name(&self) -> &str {
        "browser_upload"
    }

    fn description(&self) -> &str {
        "Upload local files to an HTML file input selected by CSS selector or snapshot ref. Requires approval every time."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["paths"],
            "properties": {
                "selector": {"type": "string"},
                "ref": {"type": "integer", "minimum": 0},
                "paths": {
                    "type": "array",
                    "minItems": 1,
                    "items": {"type": "string", "description": "Absolute local file path"}
                }
            },
            "oneOf": [
                {"required": ["selector"], "not": {"required": ["ref"]}},
                {"required": ["ref"], "not": {"required": ["selector"]}}
            ]
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let (target, paths, mut shown) = Self::preflight(&input)?;
        let effective = self.effective_target();
        let lease = self
            .session
            .lease_as(effective.clone())
            .await
            .map_err(|error| AgentError::other(error.to_string()))?;
        if let Some(obj) = shown.as_object_mut() {
            obj.insert(
                "_target".into(),
                json!(match effective {
                    BrowserTarget::Managed => "managed",
                    BrowserTarget::Bridge => "bridge",
                }),
            );
            if let Ok(url) = lease.backend().current_url().await {
                obj.insert("_page_url".into(), json!(url));
            }
            match &target {
                ClickTarget::Selector(selector) => {
                    obj.insert("selector".into(), json!(selector));
                }
                ClickTarget::Ref(reference) => {
                    obj.insert("ref".into(), json!(reference));
                }
                ClickTarget::Coords { .. } => unreachable!(),
            }
            obj.insert("paths".into(), json!(paths));
        }
        drop(lease);
        let inner = Arc::new(UploadExecution {
            session: self.session.clone(),
            target_override: self.target_override.clone(),
        });
        let gated = PermissionGatedTool::with_view(
            inner,
            self.gate.clone(),
            Arc::new(super::gate::BrowserGateView::with_target_override(
                self.session.clone(),
                self.target_override.clone(),
            )),
        );
        gated.call(ctx, shown).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::{Approval, ApprovalGate};
    use agent::tool::{SafetyClass, Tool, ToolUseContext};
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[derive(Debug)]
    struct Gate {
        seen: Mutex<Vec<serde_json::Value>>,
    }

    #[async_trait]
    impl ApprovalGate for Gate {
        async fn approve(&self, _tool: &str, input: &serde_json::Value) -> Approval {
            self.seen.lock().unwrap().push(input.clone());
            Approval::AllowAlways
        }
    }

    #[tokio::test]
    async fn target_override_bridge_fails_with_pairing_hint_before_gate() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("u.txt");
        std::fs::write(&file, b"x").unwrap();
        let gate = Arc::new(Gate {
            seen: Mutex::new(Vec::new()),
        });
        let session = crate::browser::BrowserSession::new(
            crate::config::BrowserConfig::default(),
            crate::browser::backend::mock::mock_factory(),
        );
        let tool = BrowserUploadTool::new(session, gate.clone())
            .with_target_override(Some(crate::browser::BrowserTarget::Bridge));
        let err = tool
            .call(
                &ToolUseContext::new(std::env::temp_dir()),
                json!({"selector": "input", "paths": [file.to_string_lossy()]}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("pair"));
        assert!(gate.seen.lock().unwrap().is_empty());
    }

    fn tool(gate: Arc<Gate>) -> BrowserUploadTool {
        let session = crate::browser::BrowserSession::new(
            crate::config::BrowserConfig::default(),
            crate::browser::backend::mock::mock_factory(),
        );
        BrowserUploadTool::new(session, gate)
    }

    fn ctx() -> ToolUseContext {
        ToolUseContext::new(std::env::temp_dir())
    }

    #[test]
    fn upload_is_mutating() {
        let gate = Arc::new(Gate {
            seen: Mutex::new(vec![]),
        });
        assert_eq!(tool(gate).safety_class(), SafetyClass::Mutating);
    }

    #[tokio::test]
    async fn invalid_inputs_do_not_prompt() {
        let gate = Arc::new(Gate {
            seen: Mutex::new(vec![]),
        });
        let tool = tool(gate.clone());
        for input in [
            json!({"selector": "#f", "paths": []}),
            json!({"paths": ["relative.txt"]}),
            json!({"selector": "#f", "ref": 1, "paths": ["/missing"]}),
            json!({"selector": "#f", "paths": ["/missing"]}),
        ] {
            assert!(tool.call(&ctx(), input).await.is_err());
        }
        assert!(gate.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_directory_and_duplicate_canonical_paths_without_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, b"hello").unwrap();
        let alias = dir.path().join("alias.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&file, &alias).unwrap();

        let gate = Arc::new(Gate {
            seen: Mutex::new(vec![]),
        });
        let tool = tool(gate.clone());
        assert!(tool
            .call(&ctx(), json!({"selector":"#f", "paths":[dir.path()]}))
            .await
            .unwrap_err()
            .to_string()
            .contains("regular file"));
        #[cfg(unix)]
        assert!(tool
            .call(&ctx(), json!({"selector":"#f", "paths":[file, alias]}))
            .await
            .unwrap_err()
            .to_string()
            .contains("duplicate"));
        assert!(gate.seen.lock().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_socket_device_and_fifo_without_prompt() {
        use std::os::unix::net::UnixListener;

        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("socket");
        let listener = UnixListener::bind(&socket).ok();
        let fifo = dir.path().join("fifo");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        let gate = Arc::new(Gate {
            seen: Mutex::new(vec![]),
        });
        let tool = tool(gate.clone());
        let mut paths = vec![fifo, PathBuf::from("/dev/null")];
        if listener.is_some() {
            paths.push(socket);
        }
        for path in paths {
            assert!(tool
                .call(&ctx(), json!({"selector":"#f", "paths":[path]}))
                .await
                .unwrap_err()
                .to_string()
                .contains("regular file"));
        }
        assert!(gate.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn canonical_paths_and_sizes_are_approved_every_time() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, b"hello").unwrap();
        let gate = Arc::new(Gate {
            seen: Mutex::new(vec![]),
        });
        let tool = tool(gate.clone());
        let input = json!({"selector":"#f", "paths":[file]});
        tool.call(&ctx(), input.clone()).await.unwrap();
        tool.call(&ctx(), input).await.unwrap();
        let seen = gate.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0]["files"][0]["size"], 5);
        assert!(seen[0]["files"][0]["path"]
            .as_str()
            .unwrap()
            .starts_with('/'));
        assert_eq!(seen[0]["_target"], "managed");
        assert_eq!(seen[0]["_page_url"], "https://example.test/");
    }
}
