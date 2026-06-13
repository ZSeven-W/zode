//! ZodeEngine — assembles the agent QueryLoop's shared state and drives
//! one turn at a time. `QueryLoop::run` consumes `self`, so each turn
//! rebuilds a loop from these Arcs (cheap — all fields are Arc).

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use agent::abort::AbortController;
use agent::compact::AutoCompactState;
use agent::file_cache::FileStateCache;
use agent::hook::HookRunner;
use agent::message::MessageStore;
use agent::permission::{PermissionManager, PermissionMode, RuleSource};
use agent::provider::Provider;
use agent::query::QueryLoop;
use agent::skills::SkillRegistry;
use agent::stream::EventStream;
use agent::tool::{SafetyClass, Tool, ToolRegistry, ToolUseContext};
use agent_tools_code::{
    register_default_with_todo, BashOutputTool, BashRunTool, BashSessionRegistry, KillShellTool,
    TaskTool, TodoState, ToolSearchTool, WorkspacePolicy,
};
// QueryLoop's builder takes std::sync::Mutex (not tokio's). We never hold
// these guards across an await — callers snapshot (MessageStore: Clone)
// before async work.
use std::sync::Mutex;

use crate::approval::ApprovalGate;
use crate::bg_shells::{BackgroundShellTracker, BgShellHook};
use crate::config::ZodeConfig;
use crate::cost::CostState;
use crate::error::CoreError;
use crate::gated_tool::PermissionGatedTool;
use crate::history::{EditHistory, EditHistoryHook};
use crate::hooks_config::load_hook_handlers;
use crate::instructions::{build_system_prompt, discover_instructions, gather_env};
use crate::provider::build_provider;
use crate::skills::{load_skills_from, skills_dirs, skills_index, SkillTool};
use crate::task_factory::ZodeTaskFactory;

const EDIT_HISTORY_CAPACITY: usize = 50;

const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 8192;
const DEFAULT_MODEL_MAX_TOKENS: u32 = 200_000;
const FILE_CACHE_ENTRIES: usize = 1024;
const FILE_CACHE_BYTES: usize = 16 * 1024 * 1024;

pub struct ZodeEngine {
    pub provider: Arc<dyn Provider>,
    pub tools: Arc<ToolRegistry>,
    pub permissions: Arc<PermissionManager>,
    pub hooks: Arc<HookRunner>,
    pub store: Arc<Mutex<MessageStore>>,
    pub file_cache: Arc<FileStateCache>,
    pub compact_state: Arc<Mutex<AutoCompactState>>,
    pub model: String,
    pub system: Option<String>,
    pub cwd: PathBuf,
    pub max_output_tokens: u32,
    /// Background shell registry (Phase 03/07 inspect this).
    pub bash_sessions: BashSessionRegistry,
    /// Shared TodoWrite state handle (Phase 07 reads the list for the UI).
    pub todo_state: TodoState,
    /// File-edit undo/redo history, fed by an EditHistoryHook on `hooks`.
    pub history: Arc<tokio::sync::Mutex<EditHistory>>,
    /// Host-side metadata for background shells (Phase 07 task panel).
    pub bg_shells_meta: BackgroundShellTracker,
    /// Loaded skills (the `/skills` command lists these).
    pub skills: Arc<SkillRegistry>,
    /// MCP lifecycle, if any servers were configured (`/mcp` reports state).
    pub mcp: Option<Arc<agent::mcp::Lifecycle>>,
    /// Token/cost tracking (fed Usage events by the consumer; `/cost`).
    pub cost: Arc<CostState>,
}

impl ZodeEngine {
    /// Build all shared state. `gate` is `BypassGate` for `--yolo`,
    /// `StdinGate` for headless, `QueueGate` for the TUI — that is the ONLY
    /// place interactive approval is decided.
    ///
    /// The internal PermissionManager always runs in `Bypass` so that
    /// unresolved tools resolve to Allow and actually reach the gated-tool
    /// decorator. (Under `Default`, agent-rs's QueryLoop turns an unresolved
    /// `Ask` into a failed synthetic ToolResult *before* dispatch, so the
    /// gate would never run — master plan §4.6①.) Explicit cfg deny rules
    /// are still honored: agent evaluates deny rules before the Bypass-mode
    /// short-circuit.
    pub async fn assemble(
        cfg: &ZodeConfig,
        cwd: PathBuf,
        gate: Arc<dyn ApprovalGate>,
        sandbox: Option<crate::sandbox::SandboxConfig>,
        date: &str,
    ) -> Result<Self, CoreError> {
        let provider = build_provider(&cfg.provider)?;
        let model = cfg
            .provider
            .model
            .clone()
            .ok_or_else(|| CoreError::Other("no model set in config".into()))?;

        // Writes are constrained to cwd by default (WorkspacePolicy).
        let policy = WorkspacePolicy::new(&cwd)
            .map_err(|e| CoreError::Other(format!("workspace policy: {e}")))?
            .into_arc();

        // 1. Default tools (fs/search/shell/web/notebook/todo) + the
        //    background-shell trio (not part of register_default). We pass
        //    a TodoState so we keep the handle (Phase 07) and avoid the
        //    "no caller-provided TodoState" startup warning.
        let mut base = ToolRegistry::new();
        let todo_state = TodoState::new();
        register_default_with_todo(&mut base, policy.clone(), todo_state.clone());
        let bash_sessions = BashSessionRegistry::new();
        base.register(Arc::new(BashRunTool::new(
            policy.clone(),
            bash_sessions.clone(),
        )));
        base.register(Arc::new(BashOutputTool::new(bash_sessions.clone())));
        base.register(Arc::new(KillShellTool::new(bash_sessions.clone())));

        // Git tools (Zode product tools, not in agent-tools-code).
        for tool in crate::tools::git::all_git_tools() {
            base.register(tool);
        }

        // Skills: load the three-level SKILL.md tree, register the read-only
        // Skill tool, and capture the index for the system prompt.
        let skills = Arc::new(load_skills_from(&skills_dirs(&cwd)));
        let skills_idx = skills_index(&skills);
        base.register(Arc::new(SkillTool::new(skills.clone())));

        // MCP: connect configured servers (blocking at startup) and register
        // a ZodeMcpTool per discovered tool. MCP tools are SafetyClass::Unknown
        // so they go through the approval gate like other mutating tools.
        let mcp = match crate::mcp::discover_mcp_config(&cwd) {
            Some(config) => {
                let lifecycle = crate::mcp::connect(config).await;
                for tool in crate::mcp::mcp_tools(&lifecycle) {
                    base.register(tool);
                }
                Some(lifecycle)
            }
            None => None,
        };

        // Permissions: Bypass mode (so non-denied tools reach the gate) plus
        // hard-deny rules (still enforced ahead of the bypass). Interactive
        // `ask` is handled entirely by the gate (master §4.6①). Built here so
        // the Task sub-agent factory can share it.
        let mut pm = PermissionManager::new().with_mode(PermissionMode::Bypass);
        for tool in &cfg.permissions.deny {
            pm = pm.deny(RuleSource::User, tool.clone());
        }
        let permissions = Arc::new(pm);

        // Task sub-agent tool: the factory snapshots the current tool set
        // (which has no Task yet — recursion guard) and shares the parent's
        // provider/model/permissions. Registered LAST among base tools.
        let task_factory = Arc::new(ZodeTaskFactory::new(
            provider.clone(),
            model.clone(),
            Arc::new(base.clone()),
            permissions.clone(),
        ));
        base.register(Arc::new(TaskTool::new(task_factory)));

        // --sandbox: wrap Bash/BashRun so writes are confined to cwd. Done
        // before gate-wrapping so the final shape is
        // PermissionGatedTool(SandboxedBashTool(Bash)).
        let base = match &sandbox {
            Some(sb) => crate::sandbox::apply_sandbox(base, sb),
            None => base,
        };

        // 2. Wrap mutating/destructive tools with the approval gate.
        let mut gated = wrap_mutating_tools(base, &gate);

        // 3. ToolSearch over the full set (candidates = snapshot of the
        //    gated registry, taken before ToolSearch itself is added).
        let candidates = Arc::new(gated.clone());
        gated.register(Arc::new(ToolSearchTool::new(candidates)));

        let tools = Arc::new(gated);

        let file_cache = Arc::new(FileStateCache::new(
            NonZeroUsize::new(FILE_CACHE_ENTRIES).expect("nonzero"),
            FILE_CACHE_BYTES,
        ));

        // Hooks: file-edit undo history. EditHistoryHook captures
        // before/after around FileWrite/FileEdit/Remove.
        let history = Arc::new(tokio::sync::Mutex::new(EditHistory::new(
            EDIT_HISTORY_CAPACITY,
        )));
        let bg_shells_meta = BackgroundShellTracker::new();
        let mut hooks = HookRunner::new();
        // EditHistoryHook resolves paths via the same policy the fs tools use.
        hooks.register(Arc::new(EditHistoryHook::new(
            history.clone(),
            policy.clone(),
        )));
        hooks.register(Arc::new(BgShellHook::new(bg_shells_meta.clone())));
        // External hooks.json scripts (global ⊕ project).
        for h in load_hook_handlers(&cwd) {
            hooks.register(h);
        }

        // System prompt: identity + env + three-level instructions + skills.
        let env = gather_env(&cwd, date);
        let instructions = discover_instructions(&cwd);
        let system = Some(build_system_prompt(&instructions, &skills_idx, &env));

        // Clone before `model` is moved into the struct's `model` field below.
        let model_for_cost = model.clone();

        Ok(Self {
            provider,
            tools,
            permissions,
            hooks: Arc::new(hooks),
            store: Arc::new(Mutex::new(MessageStore::new())),
            file_cache,
            compact_state: Arc::new(Mutex::new(AutoCompactState::default())),
            model,
            system,
            cwd,
            max_output_tokens: cfg.max_output_tokens.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS),
            bash_sessions,
            todo_state,
            history,
            bg_shells_meta,
            skills,
            mcp,
            cost: Arc::new(CostState::new(model_for_cost)),
        })
    }

    /// Undo the most recent tracked file edit. Returns the affected path.
    pub async fn undo(&self) -> Result<PathBuf, CoreError> {
        self.history.lock().await.undo()
    }

    /// Redo the most recently undone file edit. Returns the affected path.
    pub async fn redo(&self) -> Result<PathBuf, CoreError> {
        self.history.lock().await.redo()
    }

    /// Kill a background shell by id (the tasks panel's `k` action). Bypasses
    /// the approval gate on purpose — the user's keypress is the confirmation —
    /// by calling a fresh KillShell tool over the shared session registry, then
    /// reflects the kill in the host-side tracker (the hook path doesn't fire
    /// for direct calls).
    pub async fn kill_shell(&self, shell_id: &str) -> Result<(), CoreError> {
        let tool = KillShellTool::new(self.bash_sessions.clone());
        let ctx = ToolUseContext::new(self.cwd.clone());
        let result = tool
            .call(&ctx, serde_json::json!({ "shell_id": shell_id }))
            .await
            .map(|_| ())
            .map_err(|e| CoreError::Other(format!("kill_shell: {e}")));
        self.bg_shells_meta.mark_killed(shell_id).await;
        result
    }

    /// Inject a pre-loaded MessageStore (for `--continue` / `--resume`).
    pub fn with_store(mut self, store: MessageStore) -> Self {
        self.store = Arc::new(Mutex::new(store));
        self
    }

    /// Run one turn. Rebuilds a QueryLoop from the shared Arcs.
    pub async fn turn(
        &self,
        user_msg: &str,
        abort: AbortController,
    ) -> Result<Box<dyn EventStream>, agent::error::AgentError> {
        let mut builder = QueryLoop::builder(self.provider.clone(), self.model.clone())
            .tools(self.tools.clone())
            .permissions(self.permissions.clone())
            .hooks(self.hooks.clone())
            .store(self.store.clone())
            .file_cache(self.file_cache.clone())
            .compact_state(self.compact_state.clone())
            .max_output_tokens(self.max_output_tokens)
            .model_max_tokens(DEFAULT_MODEL_MAX_TOKENS)
            .cwd(self.cwd.clone())
            .auto_compact(true);
        if let Some(sys) = &self.system {
            builder = builder.system(sys.clone());
        }
        builder.build().run(user_msg.to_string(), abort).await
    }
}

/// Everything needed to assemble a fresh `ZodeEngine`. The TUI keeps one of
/// these so it can spin up an independent engine per session tab. The gate is
/// shared (Arc) so every tab's approvals route to the same UI queue.
#[derive(Clone)]
pub struct EngineTemplate {
    cfg: ZodeConfig,
    cwd: PathBuf,
    gate: Arc<dyn ApprovalGate>,
    sandbox: Option<crate::sandbox::SandboxConfig>,
    date: String,
}

impl EngineTemplate {
    pub fn new(
        cfg: ZodeConfig,
        cwd: PathBuf,
        gate: Arc<dyn ApprovalGate>,
        sandbox: Option<crate::sandbox::SandboxConfig>,
        date: String,
    ) -> Self {
        Self {
            cfg,
            cwd,
            gate,
            sandbox,
            date,
        }
    }

    /// Assemble a fresh engine from the template (new MessageStore, new
    /// per-tab tool/permission/cost state; shared gate).
    pub async fn assemble(&self) -> Result<ZodeEngine, CoreError> {
        ZodeEngine::assemble(
            &self.cfg,
            self.cwd.clone(),
            self.gate.clone(),
            self.sandbox.clone(),
            &self.date,
        )
        .await
    }

    pub fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    pub fn model(&self) -> Option<&str> {
        self.cfg.provider.model.as_deref()
    }
}

/// Re-register every tool, wrapping mutating/destructive ones in a
/// PermissionGatedTool. Read-only tools pass through unwrapped.
fn wrap_mutating_tools(src: ToolRegistry, gate: &Arc<dyn ApprovalGate>) -> ToolRegistry {
    let mut out = ToolRegistry::new();
    for tool in src.list() {
        if matches!(tool.safety_class(), SafetyClass::ReadOnly) {
            out.register(tool);
        } else {
            out.register(Arc::new(PermissionGatedTool::new(tool, gate.clone())));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::BypassGate;
    use crate::config::{ProviderConfig, ProviderKind, ZodeConfig};
    use std::sync::Arc;

    fn test_cfg() -> ZodeConfig {
        ZodeConfig {
            provider: ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-test".into()),
                base_url: Some("https://api.minimaxi.com/anthropic/v1".into()),
                model: Some("MiniMax-M1".into()),
                dialect: None,
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn assemble_registers_core_tools() {
        let dir = tempfile::tempdir().unwrap();
        let eng = ZodeEngine::assemble(
            &test_cfg(),
            dir.path().to_path_buf(),
            Arc::new(BypassGate),
            None,
            "2026-06-13",
        )
        .await
        .unwrap();
        let names: Vec<String> = eng.tools.names().map(|s| s.to_string()).collect();
        assert!(names.contains(&"FileRead".to_string()), "names: {names:?}");
        assert!(names.contains(&"Bash".to_string()), "names: {names:?}");
        assert!(names.contains(&"BashRun".to_string()), "names: {names:?}");
        assert!(
            names.contains(&"ToolSearch".to_string()),
            "names: {names:?}"
        );
        assert!(names.contains(&"Task".to_string()), "names: {names:?}");
        assert_eq!(eng.model, "MiniMax-M1");
    }

    #[tokio::test]
    async fn task_tool_is_registered_and_gated() {
        // Task is SafetyClass::Mutating, so it must reach the gate (be
        // wrapped) rather than pass through unwrapped like a read-only tool.
        let dir = tempfile::tempdir().unwrap();
        let eng = ZodeEngine::assemble(
            &test_cfg(),
            dir.path().to_path_buf(),
            Arc::new(BypassGate),
            None,
            "2026-06-13",
        )
        .await
        .unwrap();
        let task = eng.tools.get("Task").expect("Task tool registered");
        assert!(!matches!(task.safety_class(), SafetyClass::ReadOnly));
        // Cost tracker is wired to the configured model.
        assert!(eng.cost.report().await.contains("MiniMax-M1"));
    }

    #[tokio::test]
    async fn unconfigured_tool_resolves_to_allow_so_the_gate_runs() {
        // BLOCK regression: under Bypass the loop must NOT pre-empt an
        // unconfigured mutating tool with Ask — it must reach Allow so the
        // PermissionGatedTool decorator can prompt.
        let dir = tempfile::tempdir().unwrap();
        let eng = ZodeEngine::assemble(
            &test_cfg(),
            dir.path().to_path_buf(),
            Arc::new(BypassGate),
            None,
            "2026-06-13",
        )
        .await
        .unwrap();
        let decision =
            eng.permissions
                .evaluate("FileWrite", &serde_json::json!({"path": "x"}), None);
        assert!(decision.is_allow(), "expected Allow, got {decision:?}");
    }

    #[tokio::test]
    async fn assemble_registers_edit_history_hook() {
        let dir = tempfile::tempdir().unwrap();
        let eng = ZodeEngine::assemble(
            &test_cfg(),
            dir.path().to_path_buf(),
            Arc::new(BypassGate),
            None,
            "2026-06-13",
        )
        .await
        .unwrap();
        assert_eq!(eng.hooks.len(), 2); // EditHistory + BgShell
        assert!(eng.undo().await.is_err()); // empty history
    }

    #[tokio::test]
    async fn deny_rule_still_wins_under_bypass() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_cfg();
        cfg.permissions.deny = vec!["Bash".into()];
        let eng = ZodeEngine::assemble(
            &cfg,
            dir.path().to_path_buf(),
            Arc::new(BypassGate),
            None,
            "2026-06-13",
        )
        .await
        .unwrap();
        let decision = eng
            .permissions
            .evaluate("Bash", &serde_json::json!({}), None);
        assert!(decision.is_deny());
    }
}
