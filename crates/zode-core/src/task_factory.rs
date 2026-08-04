//! Sub-agent factory for the Task tool. Resolves an `agent_type` string to a
//! child loop config that reuses the parent provider/model/permissions and —
//! critically — the parent's FINAL gated + sandboxed tool registry. Plain
//! children keep the Task tool so they can delegate further sub-tasks; the
//! runtime's explicit depth counter bounds recursion. The child therefore
//! inherits the same approval gate, sandbox, and hooks (edit history /
//! background-shell tracking / external blockers) as the parent — no security
//! bypass.
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
use agent::tool::{SafetyClass, ToolRegistry};
use agent_tools_code::{TaskAgentConfig, TaskAgentFactory, ToolSearchTool};
use async_trait::async_trait;

/// Leave Task-spawned child loops unbounded unless the user explicitly opts
/// into a positive safety cap. `None` and `0` both preserve QueryLoop's natural
/// stop condition: the model completes a turn without requesting another tool.
pub fn resolve_subagent_max_iterations(configured: Option<u32>) -> Option<usize> {
    match configured {
        Some(value) if value > 0 => Some(usize::try_from(value).unwrap_or(usize::MAX)),
        Some(_) | None => None,
    }
}

/// Shared late-bound handle to the parent's final gated tool registry.
pub type ParentToolsCell = Arc<OnceLock<Arc<ToolRegistry>>>;

/// An immutable execution policy for one Task child loop. Modes may only
/// preserve or narrow the parent's final capability ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentMode {
    Inherit,
    Plan,
    ReadOnly,
}

impl SubagentMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Plan => "plan",
            Self::ReadOnly => "read-only",
        }
    }

    const fn restricts_to_read_only(self) -> bool {
        matches!(self, Self::Plan | Self::ReadOnly)
    }
}

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

/// The team tool names an internal teammate may NOT hold (orchestration —
/// hiring, sending, dismissing, listing). Collaboration tools
/// (team_board_*, team_claim/release) are added back per-teammate,
/// identity-bound. See [`shared_child_tools`].
pub const TEAM_ORCHESTRATION_TOOLS: &[&str] = &["TeamHire", "TeamSend", "TeamDismiss", "TeamList"];

/// Team tools a plain Task sub-agent may not hold. Task itself remains
/// available so the child can spawn another bounded child.
pub const TEAM_TOOL_NAMES: [&str; 9] = [
    "TeamHire",
    "TeamSend",
    "TeamDismiss",
    "TeamList",
    "TeamBoardRead",
    "TeamBoardUpdate",
    "TeamBoardAppend",
    "TeamClaim",
    "TeamRelease",
];

/// Internal teammates also lose direct and workflow-mediated Task dispatch:
/// teammate orchestration stays with the leader, while plain one-shot Task
/// sub-agents may recursively delegate.
pub const TEAM_TOOL_NAMES_WITH_TASK: [&str; 11] = [
    "Task",
    "RunWorkflow",
    "TeamHire",
    "TeamSend",
    "TeamDismiss",
    "TeamList",
    "TeamBoardRead",
    "TeamBoardUpdate",
    "TeamBoardAppend",
    "TeamClaim",
    "TeamRelease",
];

/// Host-control channels belong to the owning loop, not a delegated child.
/// Although these tools are classified read-only for approval purposes, they
/// can stop the caller's autonomous goal loop or wait on the caller's UI.
const CHILD_HOST_CONTROL_TOOLS: &[&str] = &["GoalComplete", "AskUserQuestion"];

/// Read-classified tools that consume shared parent state are unsafe in an
/// immutable plan child even though they do not write external resources.
const READ_ONLY_MODE_EXCLUDED_TOOLS: &[&str] = &["BashOutput"];
const READ_ONLY_MODE_EXCLUDED_PREFIXES: &[&str] = &["Lsp"];

/// Build a child tool registry from the parent's final gated registry,
/// excluding `exclude` by name and rebuilding ToolSearch over the FILTERED
/// set (so an excluded tool can't leak back in through search).
pub fn shared_child_tools(parent_tools: &ParentToolsCell, exclude: &[&str]) -> Arc<ToolRegistry> {
    shared_child_tools_for_mode(parent_tools, exclude, SubagentMode::Inherit)
}

fn shared_child_tools_for_mode(
    parent_tools: &ParentToolsCell,
    exclude: &[&str],
    mode: SubagentMode,
) -> Arc<ToolRegistry> {
    let mut reg = ToolRegistry::new();
    let mut rebuild_tool_search = false;
    if let Some(parent) = parent_tools.get() {
        rebuild_tool_search = parent.get("ToolSearch").is_some();
        for tool in parent.list() {
            let name = tool.name();
            if name == "ToolSearch"
                || exclude.contains(&name)
                || CHILD_HOST_CONTROL_TOOLS.contains(&name)
            {
                continue;
            }
            if mode.restricts_to_read_only()
                && (!matches!(tool.safety_class(), SafetyClass::ReadOnly)
                    || READ_ONLY_MODE_EXCLUDED_TOOLS.contains(&name)
                    || READ_ONLY_MODE_EXCLUDED_PREFIXES
                        .iter()
                        .any(|prefix| name.starts_with(prefix)))
            {
                continue;
            }
            reg.register(tool);
        }
    }
    if rebuild_tool_search {
        let candidates = Arc::new(reg.clone());
        reg.register(Arc::new(ToolSearchTool::new(candidates)));
    }
    Arc::new(reg)
}

#[derive(Debug)]
pub struct ZodeTaskFactory {
    runtime: ModelRuntimeState,
    permissions: Arc<PermissionManager>,
    cwd: PathBuf,
    file_cache: Arc<FileStateCache>,
    hooks: Arc<HookRunner>,
    /// The parent's FINAL gated+sandboxed registry, set by the engine after
    /// assembly. Plain children retain its gated Task tool.
    parent_tools: ParentToolsCell,
    /// User-defined agent definitions (`~/.zode/agents` etc.). Consulted before
    /// the built-in types so users can add/override sub-agents.
    defs: Vec<crate::agents::AgentDef>,
    /// Exact enabled-skill index and invocation discipline shown to the root
    /// agent. Appended only when the final child registry still contains the
    /// shared Skill tool.
    skills_prompt: String,
    /// Host routes discovered after factory construction (currently external
    /// CLI profiles). Nested children use this shared catalog in their prompt.
    additional_agent_types: RwLock<Vec<(String, String)>>,
    /// Per-engine sub-agent registry; the Task observer writes here.
    subagents: crate::subagents::SubAgentRegistry,
    /// Per-child model/tool round-trip cap. `None` means unbounded.
    max_iterations: Option<usize>,
}

/// Adapts the same persona/model factory to a specific child execution mode.
/// A separate upstream TaskTool owns each adapter, while
/// [`crate::task_mode::TaskModeRouter`] selects the adapter from the
/// provider-facing `Task.mode` field.
#[derive(Debug)]
pub struct ZodeTaskModeFactory {
    inner: Arc<ZodeTaskFactory>,
    mode: SubagentMode,
}

impl ZodeTaskModeFactory {
    pub fn new(inner: Arc<ZodeTaskFactory>, mode: SubagentMode) -> Self {
        Self { inner, mode }
    }
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
        skills_prompt: String,
        subagents: crate::subagents::SubAgentRegistry,
        max_iterations: Option<usize>,
    ) -> Self {
        Self {
            runtime,
            permissions,
            cwd,
            file_cache,
            hooks,
            parent_tools,
            defs,
            skills_prompt,
            additional_agent_types: RwLock::new(Vec::new()),
            subagents,
            max_iterations,
        }
    }

    /// Child tool registry: the parent's gated+sandboxed tools, including
    /// Task, minus team orchestration/collaboration tools. Empty until the
    /// engine has populated the cell.
    fn child_tools(&self, mode: SubagentMode) -> Arc<ToolRegistry> {
        // A plain Task sub-agent may recursively delegate through the same
        // gated Task tool. It still gets no team_* tools, preventing recursive
        // hiring or cross-team collaboration leaks. Plan/read-only mode applies
        // a second read-only filter to this already-gated parent ceiling, which
        // also removes Task itself because Task is mutating.
        shared_child_tools_for_mode(&self.parent_tools, &TEAM_TOOL_NAMES, mode)
    }

    /// The sub-agent types the Task tool can spawn: (name, one-line summary).
    /// Source of truth for `/agents` and the `system_for` prompts below.
    pub const AGENT_TYPES: &'static [(&'static str, &'static str)] = &[
        ("general", "Focused sub-task; returns a concise result"),
        ("researcher", "Investigates and reports findings; read-only"),
        ("reviewer", "Reviews code and reports issues; read-only"),
    ];

    /// Appended to every child system prompt (built-in and user-defined types).
    /// The child's final text is a tool result for the parent, not a human-facing
    /// message — without this, children pad results with greetings and questions.
    pub(crate) const SUBAGENT_CONTRACT: &str = "\n\n## Sub-agent contract\n\
You are running as a sub-agent inside an orchestrator. Your final assistant \
text is returned VERBATIM to the calling agent as the tool result — it is not \
shown to a human. Return exactly the findings, data, or diff the task asked \
for: no greetings, no questions, no offers of further help. The prompt you \
received is your ONLY context — you cannot see the parent conversation and \
nobody can answer questions mid-run. If the task cannot be completed with the \
given context, state precisely what is missing as your result.";

    const NESTED_DELEGATION_CONTRACT: &str = "\n\nYou may use the Task tool to \
delegate independent sub-tasks to another sub-agent; nested delegation is \
bounded by the runtime recursion limit.";

    const PLAN_MODE_PROMPT: &str = "\n\n## Plan mode\n\
You are in PLAN MODE for this delegated task. Only read-only tools are \
available. Investigate the supplied task thoroughly, then return a concise, \
concrete implementation plan to the calling agent. Do not modify files, run \
commands with side effects, or ask the human to switch modes. Completing this \
Task automatically returns control to the caller's unchanged mode.";

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
        let additional = match self.additional_agent_types.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        for (name, description) in additional {
            if !out.iter().any(|(existing, _)| existing == &name) {
                out.push((name, description));
            }
        }
        out
    }

    /// Publish host routes that become known after factory construction.
    pub fn set_additional_agent_types(&self, types: Vec<(String, String)>) {
        match self.additional_agent_types.write() {
            Ok(mut guard) => *guard = types,
            Err(poisoned) => *poisoned.into_inner() = types,
        }
    }

    fn build_for_mode(
        &self,
        agent_type: &str,
        mode: SubagentMode,
    ) -> Result<TaskAgentConfig, AgentError> {
        let runtime = self.runtime.snapshot();
        // User definitions take precedence over the built-in types, and may
        // override the model. Mode policy is applied afterwards, so even a
        // custom agent named "plan" cannot replace the read-only boundary.
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
        let tools = self.child_tools(mode);
        let mut system = format!("{system}{}", Self::SUBAGENT_CONTRACT);
        if tools.get("Skill").is_some() {
            system.push_str(&self.skills_prompt);
        }
        if tools.names().any(|name| name.starts_with("mcp__")) {
            system.push_str(
                "\n\nRegistered MCP tools inherited from the caller are available under \
                 their `mcp__<server>__<tool>` names. Call them directly, or use \
                 ToolSearch to discover them when ToolSearch is available.",
            );
        }
        if mode == SubagentMode::Inherit {
            let nested_types = self
                .agent_types()
                .into_iter()
                .map(|(name, description)| format!("- {name}: {description}"))
                .collect::<Vec<_>>()
                .join("\n");
            system.push_str(Self::NESTED_DELEGATION_CONTRACT);
            system.push_str("\n\n## Available sub-agent types\n");
            system.push_str(&nested_types);
        } else if mode == SubagentMode::Plan {
            system.push_str(Self::PLAN_MODE_PROMPT);
        }

        Ok(TaskAgentConfig {
            provider: runtime.provider,
            model,
            tools,
            system: Some(system),
            // `None` deliberately leaves the child's QueryLoop unbounded so
            // iterative engineering can reach its natural completion. A user
            // may still opt into a positive safety cap through config.
            max_iterations: self.max_iterations,
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

#[async_trait]
impl TaskAgentFactory for ZodeTaskFactory {
    async fn build(&self, agent_type: &str) -> Result<TaskAgentConfig, AgentError> {
        self.build_for_mode(agent_type, SubagentMode::Inherit)
    }
}

#[async_trait]
impl TaskAgentFactory for ZodeTaskModeFactory {
    async fn build(&self, agent_type: &str) -> Result<TaskAgentConfig, AgentError> {
        self.inner.build_for_mode(agent_type, self.mode)
    }
}

#[cfg(test)]
#[path = "task-factory-mode-tests.rs"]
mod mode_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProviderConfig, ProviderKind};
    use crate::provider::build_provider;
    use agent::abort::AbortController;
    use agent::permission::PermissionMode;
    use agent::stream::{Event as AgentEvent, ResultData};
    use agent::testing::{FakeTool, MockProvider};
    use agent::tool::{Tool, ToolUseContext};
    use agent_tools_code::TaskTool;
    use serde_json::json;
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

    fn factory_with_limit(cell: ParentToolsCell, max_iterations: Option<usize>) -> ZodeTaskFactory {
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
            String::new(),
            crate::subagents::SubAgentRegistry::new(),
            max_iterations,
        )
    }

    fn factory_with(cell: ParentToolsCell) -> ZodeTaskFactory {
        factory_with_limit(cell, resolve_subagent_max_iterations(None))
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
        assert_eq!(
            cfg.max_iterations, None,
            "the default child loop must remain unbounded"
        );
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

    #[test]
    fn subagent_iteration_budget_defaults_unbounded_and_can_be_overridden() {
        assert_eq!(resolve_subagent_max_iterations(None), None);
        assert_eq!(resolve_subagent_max_iterations(Some(64)), Some(64));
        assert_eq!(resolve_subagent_max_iterations(Some(0)), None);
    }

    #[tokio::test]
    async fn explicit_subagent_iteration_budget_reaches_the_child_config() {
        let factory = factory_with_limit(Arc::new(OnceLock::new()), Some(64));
        let cfg = factory.build("general").await.unwrap();
        assert_eq!(cfg.max_iterations, Some(64));
    }

    #[tokio::test]
    async fn unbounded_default_completes_more_than_legacy_32_tool_rounds() {
        const TOOL_ROUNDS: usize = 33;

        let mut turns: Vec<Vec<AgentEvent>> = (0..TOOL_ROUNDS)
            .map(|round| {
                vec![
                    AgentEvent::ToolUse {
                        id: format!("engineering-step-{round}"),
                        name: "EngineeringStep".into(),
                        input: json!({"round": round}),
                    },
                    AgentEvent::Result {
                        data: ResultData {
                            stop_reason: Some("tool_use".into()),
                            ..Default::default()
                        },
                    },
                ]
            })
            .collect();
        turns.push(vec![
            AgentEvent::TextDelta {
                delta: "loop complete".into(),
            },
            AgentEvent::Result {
                data: ResultData {
                    stop_reason: Some("end_turn".into()),
                    ..Default::default()
                },
            },
        ]);

        let provider = Arc::new(MockProvider::with_turns(turns));
        let engineering_step = Arc::new(FakeTool::new("EngineeringStep", json!({"ok": true})));
        let mut parent = ToolRegistry::new();
        parent.register(engineering_step.clone());
        let parent_tools: ParentToolsCell = Arc::new(OnceLock::new());
        parent_tools.set(Arc::new(parent)).unwrap();

        let cwd = std::env::temp_dir();
        let file_cache = Arc::new(FileStateCache::new(
            NonZeroUsize::new(8).unwrap(),
            1024 * 1024,
        ));
        let permissions = Arc::new(PermissionManager::new().with_mode(PermissionMode::Bypass));
        let hooks = Arc::new(HookRunner::new());
        let factory = ZodeTaskFactory::new(
            ModelRuntimeState::new(provider.clone(), "loop-model".into()),
            permissions.clone(),
            cwd.clone(),
            file_cache.clone(),
            hooks.clone(),
            parent_tools,
            Vec::new(),
            String::new(),
            crate::subagents::SubAgentRegistry::new(),
            resolve_subagent_max_iterations(None),
        );
        let task = TaskTool::new(Arc::new(factory));
        let ctx = ToolUseContext {
            cwd,
            abort: AbortController::new(),
            file_cache,
            permissions,
            hooks,
            task_depth: 0,
        };

        let output = task
            .call(
                &ctx,
                json!({"prompt": "iterate until complete", "agent_type": "general"}),
            )
            .await
            .unwrap();

        assert_eq!(output["output"], "loop complete");
        assert_eq!(output["stop_reason"], "end_turn");
        assert_eq!(engineering_step.call_count(), TOOL_ROUNDS);
        assert_eq!(provider.remaining_turns(), 0);
    }

    #[tokio::test]
    async fn child_can_spawn_a_nested_plan_subagent_and_resume_its_mode() {
        let provider = Arc::new(MockProvider::with_turns(vec![
            vec![
                AgentEvent::ToolUse {
                    id: "spawn-grandchild".into(),
                    name: "Task".into(),
                    input: json!({
                        "description": "nested work",
                        "prompt": "return the nested result",
                        "agent_type": "general",
                        "mode": "plan"
                    }),
                },
                AgentEvent::Result {
                    data: ResultData {
                        stop_reason: Some("tool_use".into()),
                        ..Default::default()
                    },
                },
            ],
            vec![
                AgentEvent::TextDelta {
                    delta: "grandchild result".into(),
                },
                AgentEvent::Result {
                    data: ResultData {
                        stop_reason: Some("end_turn".into()),
                        ..Default::default()
                    },
                },
            ],
            vec![
                AgentEvent::ToolUse {
                    id: "call-inherited-mcp".into(),
                    name: "mcp__registered__inspect".into(),
                    input: json!({"subject": "nested result"}),
                },
                AgentEvent::Result {
                    data: ResultData {
                        stop_reason: Some("tool_use".into()),
                        ..Default::default()
                    },
                },
            ],
            vec![
                AgentEvent::ToolUse {
                    id: "resume-parent-mode".into(),
                    name: "EngineeringStep".into(),
                    input: json!({"after": "plan"}),
                },
                AgentEvent::Result {
                    data: ResultData {
                        stop_reason: Some("tool_use".into()),
                        ..Default::default()
                    },
                },
            ],
            vec![
                AgentEvent::TextDelta {
                    delta: "child result".into(),
                },
                AgentEvent::Result {
                    data: ResultData {
                        stop_reason: Some("end_turn".into()),
                        ..Default::default()
                    },
                },
            ],
        ]));
        let cwd = std::env::temp_dir();
        let file_cache = Arc::new(FileStateCache::new(
            NonZeroUsize::new(8).unwrap(),
            1024 * 1024,
        ));
        let permissions = Arc::new(PermissionManager::new().with_mode(PermissionMode::Bypass));
        let hooks = Arc::new(HookRunner::new());
        let registry = crate::subagents::SubAgentRegistry::new();
        let parent_tools: ParentToolsCell = Arc::new(OnceLock::new());
        let factory = Arc::new(ZodeTaskFactory::new(
            ModelRuntimeState::new(provider.clone(), "nested-model".into()),
            permissions.clone(),
            cwd.clone(),
            file_cache.clone(),
            hooks.clone(),
            parent_tools.clone(),
            Vec::new(),
            String::new(),
            registry.clone(),
            resolve_subagent_max_iterations(None),
        ));
        let inherit_task: Arc<dyn Tool> = Arc::new(TaskTool::new(factory.clone()));
        let plan_task: Arc<dyn Tool> = Arc::new(TaskTool::new(Arc::new(ZodeTaskModeFactory::new(
            factory,
            SubagentMode::Plan,
        ))));
        let task: Arc<dyn Tool> = Arc::new(
            crate::task_mode::TaskModeRouter::new(inherit_task)
                .with_mode(SubagentMode::Plan.as_str(), plan_task),
        );
        let engineering_step = Arc::new(FakeTool::new("EngineeringStep", json!({"ok": true})));
        let inherited_mcp = Arc::new(FakeTool::new(
            "mcp__registered__inspect",
            json!({"ok": true}),
        ));
        let mut parent = ToolRegistry::new();
        parent.register(task.clone());
        parent.register(engineering_step.clone());
        parent.register(inherited_mcp.clone());
        parent_tools.set(Arc::new(parent)).unwrap();

        let ctx = ToolUseContext {
            cwd,
            abort: AbortController::new(),
            file_cache,
            permissions,
            hooks,
            task_depth: 0,
        };
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            task.call(
                &ctx,
                json!({
                    "description": "outer work",
                    "prompt": "delegate the nested part",
                    "agent_type": "general"
                }),
            ),
        )
        .await
        .expect("nested Task chain should not deadlock")
        .unwrap();

        assert_eq!(output["output"], "child result");
        assert_eq!(provider.remaining_turns(), 0);
        assert_eq!(inherited_mcp.call_count(), 1);
        assert_eq!(
            engineering_step.call_count(),
            1,
            "returning from plan mode must restore the caller's normal tools"
        );
        let agents = registry.snapshot();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].description.as_deref(), Some("outer work"));
        assert_eq!(agents[0].depth, 0);
        assert_eq!(agents[0].status, crate::subagents::SubAgentStatus::Done);
        assert_eq!(agents[1].description.as_deref(), Some("nested work"));
        assert_eq!(agents[1].depth, 1);
        assert_eq!(agents[1].status, crate::subagents::SubAgentStatus::Done);
    }

    #[tokio::test]
    async fn child_system_prompt_carries_subagent_contract() {
        let factory = factory_with(Arc::new(OnceLock::new()));
        let cfg = factory.build("general").await.unwrap();
        let system = cfg.system.unwrap();
        assert!(system.contains("returned VERBATIM to the calling agent"));
        assert!(system.contains("cannot see the parent conversation"));
        assert!(system.contains("Task tool to delegate independent sub-tasks"));
        assert!(system.contains("- general:"));
        assert!(system.contains("- researcher:"));
        assert!(system.contains("- reviewer:"));
        // The base persona is still present, contract is appended not replacing.
        assert!(system.contains("focused sub-agent"));
    }
}
