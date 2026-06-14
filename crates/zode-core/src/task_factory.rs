//! Sub-agent factory for the Task tool. Resolves an `agent_type` string to a
//! child loop config that reuses the parent provider/model/permissions and —
//! critically — the parent's FINAL gated + sandboxed tool registry (minus the
//! Task tool itself, for recursion safety). The child therefore inherits the
//! same approval gate, sandbox, and hooks (edit history / background-shell
//! tracking / external blockers) as the parent — no security bypass.
//!
//! The gated registry only exists after the engine finishes wrapping tools, so
//! it is late-bound through a shared `OnceLock` the engine populates at the end
//! of assembly.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use agent::error::AgentError;
use agent::file_cache::FileStateCache;
use agent::hook::HookRunner;
use agent::permission::PermissionManager;
use agent::provider::Provider;
use agent::tool::ToolRegistry;
use agent_tools_code::{TaskAgentConfig, TaskAgentFactory};
use async_trait::async_trait;

/// Shared late-bound handle to the parent's final gated tool registry.
pub type ParentToolsCell = Arc<OnceLock<Arc<ToolRegistry>>>;

#[derive(Debug)]
pub struct ZodeTaskFactory {
    provider: Arc<dyn Provider>,
    model: String,
    permissions: Arc<PermissionManager>,
    cwd: PathBuf,
    file_cache: Arc<FileStateCache>,
    hooks: Arc<HookRunner>,
    /// The parent's FINAL gated+sandboxed registry, set by the engine after
    /// assembly. The child gets these tools minus "Task".
    parent_tools: ParentToolsCell,
}

impl ZodeTaskFactory {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: Arc<dyn Provider>,
        model: String,
        permissions: Arc<PermissionManager>,
        cwd: PathBuf,
        file_cache: Arc<FileStateCache>,
        hooks: Arc<HookRunner>,
        parent_tools: ParentToolsCell,
    ) -> Self {
        Self {
            provider,
            model,
            permissions,
            cwd,
            file_cache,
            hooks,
            parent_tools,
        }
    }

    /// Child tool registry: the parent's gated+sandboxed tools minus "Task"
    /// (recursion guard). Empty until the engine has populated the cell.
    fn child_tools(&self) -> Arc<ToolRegistry> {
        let mut reg = ToolRegistry::new();
        if let Some(parent) = self.parent_tools.get() {
            for tool in parent.list() {
                if tool.name() != "Task" {
                    reg.register(tool);
                }
            }
        }
        Arc::new(reg)
    }

    /// The sub-agent types the Task tool can spawn: (name, one-line summary).
    /// Source of truth for `/agents` and the `system_for` prompts below.
    pub const AGENT_TYPES: &'static [(&'static str, &'static str)] = &[
        ("general", "Focused sub-task; returns a concise result"),
        ("researcher", "Investigates and reports findings; read-only"),
        ("reviewer", "Reviews code and reports issues; read-only"),
    ];

    fn system_for(agent_type: &str) -> Option<String> {
        let prompt = match agent_type {
            "general" => {
                "You are a focused sub-agent. Complete the assigned sub-task and return a concise result."
            }
            "researcher" => {
                "You are a research sub-agent. Investigate and report findings concisely; do not modify files."
            }
            "reviewer" => {
                "You are a code-review sub-agent. Read the target and report issues; do not modify files."
            }
            _ => return None,
        };
        Some(prompt.to_string())
    }
}

#[async_trait]
impl TaskAgentFactory for ZodeTaskFactory {
    async fn build(&self, agent_type: &str) -> Result<TaskAgentConfig, AgentError> {
        let system = Self::system_for(agent_type).ok_or_else(|| {
            AgentError::other(format!(
                "unknown agent_type '{agent_type}' (try: general, researcher, reviewer)"
            ))
        })?;
        Ok(TaskAgentConfig {
            provider: self.provider.clone(),
            model: self.model.clone(),
            tools: self.child_tools(),
            system: Some(system),
            max_iterations: Some(20),
            // Same gate/sandbox/hooks/cwd/file_cache as the parent so the
            // child cannot bypass approvals, sandboxing, or hook blockers.
            permissions: Some(self.permissions.clone()),
            cwd: Some(self.cwd.clone()),
            file_cache: Some(self.file_cache.clone()),
            hooks: Some(self.hooks.clone()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProviderConfig, ProviderKind};
    use crate::provider::build_provider;
    use agent::tool::{SafetyClass, Tool, ToolUseContext};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::num::NonZeroUsize;

    fn provider() -> Arc<dyn Provider> {
        build_provider(&ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            api_key: Some("sk".into()),
            base_url: Some("https://api.minimaxi.com/anthropic/v1".into()),
            model: Some("MiniMax-M1".into()),
            dialect: None,
            ..Default::default()
        })
        .unwrap()
    }

    /// Minimal stub so we can register a tool with an arbitrary name.
    #[derive(Debug)]
    struct StubTool(&'static str);
    #[async_trait]
    impl Tool for StubTool {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            "stub"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }
        fn safety_class(&self) -> SafetyClass {
            SafetyClass::ReadOnly
        }
        async fn call(&self, _ctx: &ToolUseContext, _input: Value) -> Result<Value, AgentError> {
            Ok(json!({}))
        }
    }

    fn factory_with(cell: ParentToolsCell) -> ZodeTaskFactory {
        let file_cache = Arc::new(FileStateCache::new(
            NonZeroUsize::new(8).unwrap(),
            1024 * 1024,
        ));
        ZodeTaskFactory::new(
            provider(),
            "MiniMax-M1".into(),
            Arc::new(PermissionManager::new()),
            std::env::temp_dir(),
            file_cache,
            Arc::new(HookRunner::new()),
            cell,
        )
    }

    #[tokio::test]
    async fn builds_known_agent_type_with_parent_context() {
        let f = factory_with(Arc::new(OnceLock::new()));
        let cfg = f.build("general").await.unwrap();
        assert_eq!(cfg.model, "MiniMax-M1");
        assert!(cfg.system.is_some());
        // Parent context is threaded into the child config.
        assert!(cfg.permissions.is_some());
        assert!(cfg.cwd.is_some());
        assert!(cfg.file_cache.is_some());
        assert!(cfg.hooks.is_some());
    }

    #[tokio::test]
    async fn unknown_agent_type_errors() {
        let f = factory_with(Arc::new(OnceLock::new()));
        assert!(f.build("nonexistent-shape").await.is_err());
    }

    #[tokio::test]
    async fn child_tools_exclude_task_but_keep_others() {
        // Populate the cell with a registry containing a "Task" tool and a
        // peer; the child set must drop Task and keep the peer.
        let mut parent = ToolRegistry::new();
        parent.register(Arc::new(StubTool("Task")));
        parent.register(Arc::new(StubTool("FileRead")));
        let cell: ParentToolsCell = Arc::new(OnceLock::new());
        cell.set(Arc::new(parent)).unwrap();

        let f = factory_with(cell);
        let cfg = f.build("researcher").await.unwrap();
        assert!(cfg.tools.get("Task").is_none());
        assert!(cfg.tools.get("FileRead").is_some());
    }
}
