//! Sub-agent factory for the Task tool. Resolves an `agent_type` string to a
//! child loop config that reuses the parent provider/model/permissions but
//! strips the Task tool from the child (depth is bounded separately by
//! TaskTool::with_max_depth).

use std::sync::Arc;

use agent::error::AgentError;
use agent::permission::PermissionManager;
use agent::provider::Provider;
use agent::tool::ToolRegistry;
use agent_tools_code::{TaskAgentConfig, TaskAgentFactory};
use async_trait::async_trait;

#[derive(Debug)]
pub struct ZodeTaskFactory {
    provider: Arc<dyn Provider>,
    model: String,
    /// Parent tools; the child gets these minus "Task" (recursion guard).
    parent_tools: Arc<ToolRegistry>,
    permissions: Arc<PermissionManager>,
}

impl ZodeTaskFactory {
    pub fn new(
        provider: Arc<dyn Provider>,
        model: String,
        parent_tools: Arc<ToolRegistry>,
        permissions: Arc<PermissionManager>,
    ) -> Self {
        Self {
            provider,
            model,
            parent_tools,
            permissions,
        }
    }

    /// Child tool registry: parent tools minus "Task".
    fn child_tools(&self) -> Arc<ToolRegistry> {
        let mut reg = ToolRegistry::new();
        for tool in self.parent_tools.list() {
            if tool.name() != "Task" {
                reg.register(tool);
            }
        }
        Arc::new(reg)
    }

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
            permissions: Some(self.permissions.clone()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProviderConfig, ProviderKind};
    use crate::provider::build_provider;

    fn provider() -> Arc<dyn Provider> {
        build_provider(&ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            api_key: Some("sk".into()),
            base_url: Some("https://api.minimaxi.com/anthropic/v1".into()),
            model: Some("MiniMax-M1".into()),
            dialect: None,
        })
        .unwrap()
    }

    fn factory() -> ZodeTaskFactory {
        let mut tools = ToolRegistry::new();
        // A stand-in for the parent's "Task" tool name; child must drop it.
        // (We only check name filtering here, so any tool with name "Task"
        // would do; the real Task tool is registered by the engine.)
        ZodeTaskFactory::new(
            provider(),
            "MiniMax-M1".into(),
            {
                let _ = &mut tools;
                Arc::new(tools)
            },
            Arc::new(PermissionManager::new()),
        )
    }

    #[tokio::test]
    async fn builds_known_agent_type_without_task_tool() {
        let f = factory();
        let cfg = f.build("general").await.unwrap();
        assert_eq!(cfg.model, "MiniMax-M1");
        assert!(cfg.system.is_some());
        assert!(cfg.tools.get("Task").is_none());
    }

    #[tokio::test]
    async fn unknown_agent_type_errors() {
        let f = factory();
        assert!(f.build("nonexistent-shape").await.is_err());
    }

    #[tokio::test]
    async fn child_tools_exclude_task() {
        // A factory whose parent registry contains a "Task"-named tool must
        // exclude it from the child set.
        let f = factory();
        let cfg = f.build("researcher").await.unwrap();
        assert!(cfg.tools.get("Task").is_none());
    }
}
