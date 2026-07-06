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
use std::sync::{Arc, OnceLock, RwLock};

use agent::error::AgentError;
use agent::file_cache::FileStateCache;
use agent::hook::HookRunner;
use agent::permission::PermissionManager;
use agent::provider::Provider;
use agent::tool::ToolRegistry;
use agent_tools_code::{TaskAgentConfig, TaskAgentFactory};
use async_trait::async_trait;

const SUBAGENT_MAX_ITERATIONS: usize = 8;

/// Shared late-bound handle to the parent's final gated tool registry.
pub type ParentToolsCell = Arc<OnceLock<Arc<ToolRegistry>>>;

#[derive(Clone)]
pub struct ModelRuntimeSnapshot {
    pub provider: Arc<dyn Provider>,
    pub model: String,
}

#[derive(Clone)]
pub struct ModelRuntimeState {
    inner: Arc<RwLock<ModelRuntimeSnapshot>>,
}

impl ModelRuntimeState {
    pub fn new(provider: Arc<dyn Provider>, model: String) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ModelRuntimeSnapshot { provider, model })),
        }
    }

    pub fn update(&self, provider: Arc<dyn Provider>, model: String) {
        match self.inner.write() {
            Ok(mut guard) => *guard = ModelRuntimeSnapshot { provider, model },
            Err(poisoned) => *poisoned.into_inner() = ModelRuntimeSnapshot { provider, model },
        }
    }

    pub fn snapshot(&self) -> ModelRuntimeSnapshot {
        match self.inner.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl std::fmt::Debug for ModelRuntimeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelRuntimeState")
            .field("model", &self.snapshot().model)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct ZodeTaskFactory {
    runtime: ModelRuntimeState,
    permissions: Arc<PermissionManager>,
    cwd: PathBuf,
    file_cache: Arc<FileStateCache>,
    hooks: Arc<HookRunner>,
    /// The parent's FINAL gated+sandboxed registry, set by the engine after
    /// assembly. The child gets these tools minus "Task".
    parent_tools: ParentToolsCell,
    /// User-defined agent definitions (`~/.zode/agents` etc.). Consulted before
    /// the built-in types so users can add/override sub-agents.
    defs: Vec<crate::agents::AgentDef>,
    /// Per-engine sub-agent registry; the Task observer writes here.
    subagents: crate::subagents::SubAgentRegistry,
}

impl ZodeTaskFactory {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime: ModelRuntimeState,
        permissions: Arc<PermissionManager>,
        cwd: PathBuf,
        file_cache: Arc<FileStateCache>,
        hooks: Arc<HookRunner>,
        parent_tools: ParentToolsCell,
        defs: Vec<crate::agents::AgentDef>,
        subagents: crate::subagents::SubAgentRegistry,
    ) -> Self {
        Self {
            runtime,
            permissions,
            cwd,
            file_cache,
            hooks,
            parent_tools,
            defs,
            subagents,
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

    fn builtin_system_for(agent_type: &str) -> Option<String> {
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

    /// All spawnable agent types (name, summary): user definitions first
    /// (overriding same-named built-ins), then the remaining built-ins.
    pub fn agent_types(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .defs
            .iter()
            .map(|d| (d.name.clone(), d.description.clone()))
            .collect();
        for (name, desc) in Self::AGENT_TYPES {
            if !out.iter().any(|(n, _)| n == name) {
                out.push((name.to_string(), desc.to_string()));
            }
        }
        out
    }
}

#[async_trait]
impl TaskAgentFactory for ZodeTaskFactory {
    async fn build(&self, agent_type: &str) -> Result<TaskAgentConfig, AgentError> {
        let runtime = self.runtime.snapshot();
        // User definitions take precedence over the built-in types, and may
        // override the model.
        let (system, model) = match self.defs.iter().find(|d| d.name == agent_type) {
            Some(def) => (
                def.system.clone(),
                def.model.clone().unwrap_or_else(|| runtime.model.clone()),
            ),
            None => {
                let system = Self::builtin_system_for(agent_type).ok_or_else(|| {
                    let known: Vec<String> =
                        self.agent_types().into_iter().map(|(n, _)| n).collect();
                    AgentError::other(format!(
                        "unknown agent_type '{agent_type}' (try: {})",
                        known.join(", ")
                    ))
                })?;
                (system, runtime.model.clone())
            }
        };
        Ok(TaskAgentConfig {
            provider: runtime.provider,
            model,
            tools: self.child_tools(),
            system: Some(system),
            // Child agents are for bounded delegation. Keep the main loop
            // autonomous, but cap sub-agents so a bad decomposition cannot
            // consume the whole harness budget.
            max_iterations: Some(SUBAGENT_MAX_ITERATIONS),
            // Same gate/sandbox/hooks/cwd/file_cache as the parent so the
            // child cannot bypass approvals, sandboxing, or hook blockers.
            permissions: Some(self.permissions.clone()),
            cwd: Some(self.cwd.clone()),
            file_cache: Some(self.file_cache.clone()),
            hooks: Some(self.hooks.clone()),
            observer: Some(self.subagents.observer()),
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
            ModelRuntimeState::new(provider(), "MiniMax-M1".into()),
            Arc::new(PermissionManager::new()),
            std::env::temp_dir(),
            file_cache,
            Arc::new(HookRunner::new()),
            cell,
            Vec::new(),
            crate::subagents::SubAgentRegistry::new(),
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
        assert_eq!(cfg.max_iterations, Some(SUBAGENT_MAX_ITERATIONS));
    }

    #[tokio::test]
    async fn runtime_model_update_changes_subagent_default_model() {
        let f = factory_with(Arc::new(OnceLock::new()));

        f.runtime
            .update(provider(), "next-hot-swapped-model".to_string());

        let cfg = f.build("general").await.unwrap();
        assert_eq!(cfg.model, "next-hot-swapped-model");
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
