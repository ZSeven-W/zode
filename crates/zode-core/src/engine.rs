//! ZodeEngine — assembles the agent QueryLoop's shared state and drives
//! one turn at a time. `QueryLoop::run` consumes `self`, so each turn
//! rebuilds a loop from these Arcs (cheap — all fields are Arc).

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

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

use crate::approval::{ApprovalGate, ApprovalQueue, BypassGate, QueueGate};
use crate::bg_shells::{BackgroundShellTracker, BgShellHook};
use crate::config::ZodeConfig;
use crate::cost::CostState;
use crate::error::CoreError;
use crate::gated_tool::PermissionGatedTool;
use crate::history::{EditHistory, EditHistoryHook};
use crate::hooks_config::load_hook_handlers;
use crate::instructions::{build_system_prompt, discover_instructions, gather_env};
use crate::plugin::PluginManager;
use crate::provider::build_provider;
use crate::skills::{load_skills_filtered, load_skills_from, skills_dirs, skills_index, SkillTool};
use crate::task_factory::{ParentToolsCell, ZodeTaskFactory};

const EDIT_HISTORY_CAPACITY: usize = 50;

/// Appended to the system prompt in plan mode (read-only tools only).
const PLAN_MODE_PROMPT: &str = "\n\n# Plan mode\n\
You are in PLAN MODE. Only read-only tools are available — you cannot edit \
files, run shell commands, commit, or spawn sub-agents. Research the codebase \
thoroughly, then present a concise, concrete, step-by-step plan for the work. \
Do NOT attempt to make changes; there are no tools to do so. When the plan is \
ready, present it and tell the user to review it and run /plan to leave plan \
mode and execute.";

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
    /// Sampling temperature (None = provider default).
    pub temperature: Option<f32>,
    /// Whether to request provider prompt caching (default on).
    pub prompt_cache: bool,
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
    /// LSP manager, if any language server is enabled (`lsp_*` tools).
    pub lsp: Option<Arc<crate::lsp::LspManager>>,
    /// Token/cost tracking (fed Usage events by the consumer; `/cost`).
    pub cost: Arc<CostState>,
    /// Plugin enable/disable state (`/plugin`).
    pub plugins: PluginManager,
    /// All MCP server names discovered (incl. disabled), for the picker.
    pub all_mcp_servers: Vec<String>,
    /// All skills discovered (name, description; incl. disabled), for the picker.
    pub all_skill_meta: Vec<(String, String)>,
    /// Configured LSP language keys (for the picker).
    pub lsp_langs: Vec<String>,
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
        question_tool: Option<Arc<dyn Tool>>,
        plan_mode: bool,
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

        // Plugin manager: which tool groups / MCP servers / skills / LSP
        // servers are enabled (the `/plugin` picker toggles these).
        let plugins = PluginManager::from_config(cfg);
        // LSP servers: those auto-detected on PATH ∪ the user's config. All of
        // them are listed by the /plugin picker (each toggleable as lsp:<lang>);
        // only the enabled ones get a running server + registered tools below.
        let lsp_servers = crate::lsp::effective_servers(&cfg.lsp.servers);
        let mut lsp_langs: Vec<String> = lsp_servers.keys().cloned().collect();
        lsp_langs.sort();

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

        // AskUserQuestion, only when a UI question channel is wired. Read-only
        // (never permission-gated) and not in any plugin group (always-on).
        if let Some(tool) = question_tool {
            base.register(tool);
        }

        // Skills: load the three-level SKILL.md tree. Disabled skills are
        // dropped from the registry + index, but the full list is kept for the
        // /plugin picker.
        let skill_dirs = skills_dirs(&cwd);
        let mut all_skill_meta: Vec<(String, String)> = load_skills_from(&skill_dirs)
            .list()
            .iter()
            .map(|s| (s.name.clone(), s.description.clone()))
            .collect();
        all_skill_meta.sort();
        let skills = Arc::new(load_skills_filtered(&skill_dirs, |n| {
            plugins.skill_enabled(n)
        }));
        let skills_idx = skills_index(&skills);
        base.register(Arc::new(SkillTool::new(skills.clone())));

        // MCP: discover configured servers; connect only the enabled ones
        // (disabled ones are still listed by /plugin). Register a ZodeMcpTool
        // per discovered tool — they go through the approval gate.
        let mut all_mcp_servers: Vec<String> = Vec::new();
        let mcp = match crate::mcp::discover_mcp_config(&cwd) {
            Some(mut config) => {
                all_mcp_servers = config.servers.keys().cloned().collect();
                all_mcp_servers.sort();
                config.servers.retain(|name, _| plugins.mcp_enabled(name));
                // In plan mode, MCP tools (SafetyClass::Unknown) get filtered
                // out anyway — skip the connection (process spawn / network).
                if plan_mode || config.servers.is_empty() {
                    None
                } else {
                    let lifecycle = crate::mcp::connect(config).await;
                    for tool in crate::mcp::mcp_tools(&lifecycle) {
                        base.register(tool);
                    }
                    Some(lifecycle)
                }
            }
            None => None,
        };

        // LSP: register the lsp_* tools when at least one language server is
        // enabled. Servers spawn lazily (on first tool use), so this is cheap
        // even with rust-analyzer configured. Disabled languages are still
        // listed by /plugin (via `lsp_langs`) so they can be re-enabled.
        let lsp = {
            let enabled: std::collections::HashMap<String, crate::config::LspServerConfig> =
                lsp_servers
                    .iter()
                    .filter(|(lang, _)| plugins.lsp_enabled(lang))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
            if enabled.is_empty() {
                None
            } else {
                let mgr = Arc::new(crate::lsp::LspManager::new(
                    crate::config::LspConfig { servers: enabled },
                    cwd.clone(),
                ));
                for tool in crate::lsp::lsp_tools(&mgr) {
                    base.register(tool);
                }
                Some(mgr)
            }
        };

        // File cache + edit-history + background-shell tracker + hook runner.
        // Built BEFORE the Task factory so the child sub-agent can share them:
        // the same file_cache (read-before-write tracking) and the same hook
        // runner (edit history, bg-shell tracking, external hook blockers all
        // apply to the child too — no bypass).
        let file_cache = Arc::new(FileStateCache::new(
            NonZeroUsize::new(FILE_CACHE_ENTRIES).expect("nonzero"),
            FILE_CACHE_BYTES,
        ));
        let history = Arc::new(tokio::sync::Mutex::new(EditHistory::new(
            EDIT_HISTORY_CAPACITY,
        )));
        let bg_shells_meta = BackgroundShellTracker::new();
        let mut hook_runner = HookRunner::new();
        // EditHistoryHook resolves paths via the same policy the fs tools use.
        hook_runner.register(Arc::new(EditHistoryHook::new(
            history.clone(),
            policy.clone(),
        )));
        hook_runner.register(Arc::new(BgShellHook::new(bg_shells_meta.clone())));
        // External hooks.json scripts (global ⊕ project).
        for h in load_hook_handlers(&cwd) {
            hook_runner.register(h);
        }
        let hooks = Arc::new(hook_runner);

        // Permissions: Bypass mode (so non-denied tools reach the gate) plus
        // hard-deny rules (still enforced ahead of the bypass). Interactive
        // `ask` is handled entirely by the gate (master §4.6①). Built here so
        // the Task sub-agent factory can share it.
        let mut pm = PermissionManager::new().with_mode(PermissionMode::Bypass);
        for tool in &cfg.permissions.deny {
            pm = pm.deny(RuleSource::User, tool.clone());
        }
        let permissions = Arc::new(pm);

        // Task sub-agent tool. The child inherits the parent's FINAL gated +
        // sandboxed registry (minus Task — recursion guard), plus the same
        // permissions/hooks/cwd/file_cache. The gated registry only exists
        // after wrapping, so it is late-bound through a OnceLock the engine
        // populates below. Registered LAST among base tools.
        let task_tools: ParentToolsCell = Arc::new(OnceLock::new());
        let task_factory = Arc::new(ZodeTaskFactory::new(
            provider.clone(),
            model.clone(),
            permissions.clone(),
            cwd.clone(),
            file_cache.clone(),
            hooks.clone(),
            task_tools.clone(),
        ));
        base.register(Arc::new(TaskTool::new(task_factory)));

        // Drop tools whose plugin group is disabled (Skill / ToolSearch / MCP
        // tools are always-on and pass through).
        let base = filter_enabled_tools(base, &plugins);

        // Plan mode: keep only read-only tools so the agent can research but
        // not change anything until the user approves the plan and exits.
        let base = if plan_mode {
            filter_read_only(base)
        } else {
            base
        };

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
        // Late-bind the child sub-agent's tool set to the final gated+sandboxed
        // registry now that wrapping is complete.
        let _ = task_tools.set(tools.clone());

        // System prompt: identity + env + three-level instructions + skills,
        // plus a plan-mode preamble when only read-only tools are available.
        let mut env = gather_env(&cwd, date);
        // Tell the agent which model it's running on (so "what model are you?"
        // is answerable). Stable across a session, so it doesn't hurt caching.
        env.model = model.clone();
        let instructions = discover_instructions(&cwd);
        let mut system = build_system_prompt(&instructions, &skills_idx, &env);
        if plan_mode {
            system.push_str(PLAN_MODE_PROMPT);
        }
        // A persistent goal (`/goal`) keeps the agent focused on one objective.
        if let Some(goal) = cfg.goal.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
            system.push_str(&format!(
                "\n\n# Current goal\nKeep this objective in focus for every turn; \
                 if a request is ambiguous, resolve it toward the goal:\n{goal}"
            ));
        }
        // Effort level (`/effort`) tunes thoroughness vs. speed.
        match cfg.effort.as_deref().map(|e| e.trim().to_ascii_lowercase()).as_deref() {
            Some("high") => system.push_str(
                "\n\n# Effort: high\nBe thorough and exhaustive — explore broadly, \
                 verify carefully, and prefer completeness over brevity.",
            ),
            Some("low") => system.push_str(
                "\n\n# Effort: low\nBe fast and concise — minimal exploration, direct \
                 answers, and skip non-essential detail.",
            ),
            _ => {} // medium / unset → balanced default, no directive.
        }
        let system = Some(system);

        // Clone before `model` is moved into the struct's `model` field below.
        let model_for_cost = model.clone();
        // Seed the price catalog with the active provider's configured prices
        // (keyed on the exact model id) so models the built-ins don't know —
        // e.g. DeepSeek — get a real cost instead of "n/a". Display in the
        // configured currency (USD default).
        let mut catalog = agent::cost::ModelPriceCatalog::with_defaults();
        if let Some(prices) = cfg.provider.price_overrides() {
            catalog.insert(model_for_cost.clone(), prices);
        }
        let currency_code = cfg.currency.as_deref().unwrap_or("USD");
        let cost = Arc::new(CostState::new_with(model_for_cost, catalog, currency_code));

        Ok(Self {
            provider,
            tools,
            permissions,
            hooks,
            store: Arc::new(Mutex::new(MessageStore::new())),
            file_cache,
            compact_state: Arc::new(Mutex::new(AutoCompactState::default())),
            model,
            system,
            cwd,
            max_output_tokens: cfg.max_output_tokens.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS),
            temperature: cfg.temperature,
            prompt_cache: cfg.prompt_cache.unwrap_or(true),
            bash_sessions,
            todo_state,
            history,
            bg_shells_meta,
            skills,
            mcp,
            lsp,
            cost,
            plugins,
            all_mcp_servers,
            all_skill_meta,
            lsp_langs,
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

    /// Full plugin list (incl. disabled) for `/plugin` and the picker — tool
    /// groups, MCP servers (with live connection state), skills, LSP servers.
    pub fn plugin_list(&self) -> Vec<crate::plugin::Plugin> {
        let mcp_servers: Vec<(String, bool)> = self
            .all_mcp_servers
            .iter()
            .map(|s| {
                let connected = self
                    .mcp
                    .as_ref()
                    .map(|lc| {
                        lc.registry
                            .snapshot()
                            .iter()
                            .any(|sv| &sv.name == s && sv.state.is_connected())
                    })
                    .unwrap_or(false);
                (s.clone(), connected)
            })
            .collect();
        self.plugins
            .list(&mcp_servers, &self.all_skill_meta, &self.lsp_langs)
    }

    /// Inject a pre-loaded MessageStore (for `--continue` / `--resume`).
    pub fn with_store(mut self, store: MessageStore) -> Self {
        self.store = Arc::new(Mutex::new(store));
        self
    }

    /// Render the conversation to a Markdown transcript (`/export`). Returns an
    /// empty header-only document if the store mutex is poisoned.
    pub fn export_markdown(&self) -> String {
        match self.store.lock() {
            Ok(store) => crate::export::store_to_markdown(&store),
            Err(_) => "# Conversation\n\n".to_string(),
        }
    }

    /// The text of the most recent assistant message (`/copy`), or `None` if
    /// there is no assistant turn yet.
    pub fn last_assistant_text(&self) -> Option<String> {
        use agent::message::{ContentBlock, Message};
        let store = self.store.lock().ok()?;
        // `store.iter()` is forward-only; keep the most recent non-empty hit.
        let mut last = None;
        for m in store.iter() {
            if let Message::Assistant { content, .. } = m {
                let text: String = content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.trim().is_empty() {
                    last = Some(text);
                }
            }
        }
        last
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
            .auto_compact(true)
            .use_prompt_cache(self.prompt_cache);
        if let Some(t) = self.temperature {
            builder = builder.temperature(t);
        }
        if let Some(sys) = &self.system {
            builder = builder.system(sys.clone());
        }
        builder.build().run(user_msg.to_string(), abort).await
    }
}

/// How a tab's engine obtains its approval gate.
/// Everything needed to assemble a fresh `ZodeEngine`. The TUI keeps one of
/// these so it can spin up an independent engine per session tab and rebuild a
/// tab's engine for a hot model/provider/yolo switch. The approval `queue` is
/// retained even under `--yolo` so toggling yolo back off has a channel to use;
/// each tab's gate is labeled with its id.
#[derive(Clone)]
pub struct EngineTemplate {
    cfg: ZodeConfig,
    cwd: PathBuf,
    /// Interactive approval channel (TUI). `None` → always bypass.
    queue: Option<ApprovalQueue>,
    /// Interactive question channel (TUI) for `AskUserQuestion`. `None` → the
    /// tool isn't registered (no UI to answer it).
    question_queue: Option<crate::question::QuestionQueue>,
    /// When true, tools auto-approve (BypassGate) regardless of `queue`.
    yolo: bool,
    /// Plan mode: only read-only tools are registered and the system prompt
    /// directs the agent to research and present a plan, not make changes.
    plan_mode: bool,
    sandbox: Option<crate::sandbox::SandboxConfig>,
    date: String,
}

impl EngineTemplate {
    pub fn new(
        cfg: ZodeConfig,
        cwd: PathBuf,
        queue: Option<ApprovalQueue>,
        yolo: bool,
        sandbox: Option<crate::sandbox::SandboxConfig>,
        date: String,
    ) -> Self {
        Self {
            cfg,
            cwd,
            queue,
            question_queue: None,
            yolo,
            plan_mode: false,
            sandbox,
            date,
        }
    }

    /// Wire the interactive question channel (TUI). Carried across reassembly
    /// clones, so `AskUserQuestion` survives provider/model/plugin swaps.
    pub fn with_question_queue(mut self, queue: Option<crate::question::QuestionQueue>) -> Self {
        self.question_queue = queue;
        self
    }

    /// Assemble a fresh engine using the template's default cwd and no source
    /// label.
    pub async fn assemble(&self) -> Result<ZodeEngine, CoreError> {
        self.assemble_tab(None, None).await
    }

    /// Assemble a fresh engine for a tab. `cwd_override` lets a resumed session
    /// run in its original directory; `label` tags approval prompts with the
    /// requesting tab's id. The gate is bypass when `yolo` (or no queue), else
    /// a labeled `QueueGate`.
    pub async fn assemble_tab(
        &self,
        cwd_override: Option<PathBuf>,
        label: Option<String>,
    ) -> Result<ZodeEngine, CoreError> {
        let gate: Arc<dyn ApprovalGate> = match (&self.queue, self.yolo) {
            (Some(q), false) => Arc::new(QueueGate::with_label(q.clone(), label.clone())),
            _ => Arc::new(BypassGate),
        };
        // AskUserQuestion is registered only when a UI question channel exists.
        let question_tool: Option<Arc<dyn Tool>> = self.question_queue.as_ref().map(|q| {
            Arc::new(crate::question::AskUserQuestionTool::new(
                q.clone(),
                label.clone(),
            )) as Arc<dyn Tool>
        });
        let cwd = cwd_override.unwrap_or_else(|| self.cwd.clone());
        ZodeEngine::assemble(
            &self.cfg,
            cwd,
            gate,
            self.sandbox.clone(),
            &self.date,
            question_tool,
            self.plan_mode,
        )
        .await
    }

    pub fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    pub fn model(&self) -> Option<&str> {
        self.cfg.provider.model.as_deref()
    }

    pub fn model_ids(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(model) = self.cfg.provider.model.as_deref() {
            out.push(model.to_string());
        }

        let mut provider_models: Vec<String> = self
            .cfg
            .providers
            .values()
            .filter_map(|p| p.model.as_deref())
            .filter(|model| !out.iter().any(|existing| existing == model))
            .map(str::to_string)
            .collect();
        provider_models.sort();
        provider_models.dedup();
        out.extend(provider_models);
        out
    }

    pub fn yolo(&self) -> bool {
        self.yolo
    }

    pub fn plan_mode(&self) -> bool {
        self.plan_mode
    }

    /// Clone with plan mode toggled (for `/plan`). Read-only tools only + a
    /// plan-mode system prompt; carried across reassembly clones.
    pub fn with_plan_mode(&self, plan_mode: bool) -> Self {
        let mut t = self.clone();
        t.plan_mode = plan_mode;
        t
    }

    /// Clone with the model overridden (for `/model <id>`).
    pub fn with_model(&self, model: String) -> Self {
        let mut t = self.clone();
        t.cfg.provider.model = Some(model);
        t
    }

    /// Clone with yolo toggled (for `/yolo` and the settings mode switch).
    pub fn with_yolo(&self, yolo: bool) -> Self {
        let mut t = self.clone();
        t.yolo = yolo;
        t
    }

    /// Clone with a named provider selected (for the settings provider switch).
    /// `None` if the name isn't in `cfg.providers`.
    pub fn with_provider(&self, name: &str) -> Option<Self> {
        let provider = self.cfg.providers.get(name).cloned()?;
        let mut t = self.clone();
        t.cfg.provider = provider;
        Some(t)
    }

    /// Clone with the active provider replaced by a complete provider config
    /// (for `/connect`, which writes a fresh provider into the global config).
    pub fn with_provider_config(&self, provider: crate::config::ProviderConfig) -> Self {
        let mut t = self.clone();
        t.cfg.provider = provider;
        t
    }

    /// Clone with the disabled-plugin set replaced (for the `/plugin` picker,
    /// which reassembles so the new tool/MCP/skill/LSP set takes effect live).
    pub fn with_plugins_disabled(&self, disabled: Vec<String>) -> Self {
        let mut t = self.clone();
        t.cfg.plugins.disabled = disabled;
        t
    }

    /// Clone with the plugin enable/disable set re-read from disk (`/reload-plugins`).
    /// Re-loads the effective config (global ⊕ project) for `cwd` and adopts its
    /// `plugins.disabled`; all other in-session state is preserved. Falls back to
    /// the current template unchanged if the config can't be loaded.
    pub fn reload_plugins_from_disk(&self) -> Self {
        match crate::config::ConfigManager::load(&self.cwd) {
            Ok(fresh) => self.with_plugins_disabled(fresh.plugins.disabled),
            Err(_) => self.clone(),
        }
    }

    /// The persistent goal injected into the system prompt (`/goal`).
    pub fn goal(&self) -> Option<&str> {
        self.cfg.goal.as_deref().filter(|g| !g.trim().is_empty())
    }

    /// Clone with the goal set/cleared (`/goal [text]`). Empty → cleared.
    pub fn with_goal(&self, goal: Option<String>) -> Self {
        let mut t = self.clone();
        t.cfg.goal = goal.filter(|g| !g.trim().is_empty());
        t
    }

    /// The effort level ("low" | "medium" | "high") (`/effort`).
    pub fn effort(&self) -> Option<&str> {
        self.cfg.effort.as_deref()
    }

    /// Clone with the effort level set/cleared (`/effort [level]`).
    pub fn with_effort(&self, effort: Option<String>) -> Self {
        let mut t = self.clone();
        t.cfg.effort = effort;
        t
    }

    /// Human-readable permission rules (`/permissions`): allow / ask / deny.
    pub fn permissions_summary(&self) -> Vec<String> {
        let p = &self.cfg.permissions;
        let mut out = Vec::new();
        let mut section = |label: &str, rules: &[String]| {
            if rules.is_empty() {
                out.push(format!("{label}: (none)"));
            } else {
                out.push(format!("{label}: {}", rules.join(", ")));
            }
        };
        section("allow", &p.allow);
        section("ask", &p.ask);
        section("deny", &p.deny);
        out
    }

    /// Configured hooks (`/hooks`), read fresh from disk (global ⊕ project).
    pub fn hooks_summary(&self) -> Vec<String> {
        crate::hooks_config::load_hook_entries(&self.cwd)
            .iter()
            .map(|e| match &e.tool {
                Some(tool) => format!("{} [{}] → {}", e.event, tool, e.script),
                None => format!("{} → {}", e.event, e.script),
            })
            .collect()
    }
}

/// The sub-agent types the Task tool can spawn (`/agents`): (name, summary).
pub fn agent_types() -> &'static [(&'static str, &'static str)] {
    crate::task_factory::ZodeTaskFactory::AGENT_TYPES
}

/// Re-register only the tools whose plugin group is enabled. Tools outside any
/// group (Skill, ToolSearch, MCP tools) are always kept.
fn filter_enabled_tools(src: ToolRegistry, plugins: &PluginManager) -> ToolRegistry {
    let mut out = ToolRegistry::new();
    for tool in src.list() {
        if plugins.tool_enabled(tool.name()) {
            out.register(tool);
        }
    }
    out
}

/// Keep only read-only tools (plan mode). Mutating/destructive tools — file
/// writes/edits, shell, git mutations, sub-agents — are dropped so the agent
/// can research but not change anything until the plan is approved.
fn filter_read_only(src: ToolRegistry) -> ToolRegistry {
    let mut out = ToolRegistry::new();
    for tool in src.list() {
        if matches!(tool.safety_class(), SafetyClass::ReadOnly) {
            out.register(tool);
        }
    }
    out
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
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn template_model_ids_include_current_and_named_provider_models() {
        let mut cfg = test_cfg();
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                model: Some("deepseek-chat".into()),
                ..Default::default()
            },
        );
        cfg.providers.insert(
            "duplicate".into(),
            ProviderConfig {
                model: Some("MiniMax-M1".into()),
                ..Default::default()
            },
        );
        let template = EngineTemplate::new(
            cfg,
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-06-14".into(),
        );

        assert_eq!(
            template.model_ids(),
            vec!["MiniMax-M1".to_string(), "deepseek-chat".to_string()]
        );
    }

    #[test]
    fn template_with_provider_config_replaces_active_provider() {
        let template = EngineTemplate::new(
            test_cfg(),
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-06-14".into(),
        );
        let switched = template.with_provider_config(ProviderConfig {
            r#type: Some(ProviderKind::Openai),
            api_key: Some("sk".into()),
            base_url: Some("https://api.deepseek.com/v1".into()),
            model: Some("deepseek-v4-pro".into()),
            dialect: Some("deepseek".into()),
            ..Default::default()
        });

        assert_eq!(switched.model(), Some("deepseek-v4-pro"));
        assert_eq!(template.model(), Some("MiniMax-M1"));
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
            None,
            false,
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
            None,
            false,
        )
        .await
        .unwrap();
        let task = eng.tools.get("Task").expect("Task tool registered");
        assert!(!matches!(task.safety_class(), SafetyClass::ReadOnly));
        // Cost tracker is wired to the configured model.
        assert!(eng.cost.report().await.contains("MiniMax-M1"));
    }

    #[tokio::test]
    async fn plan_mode_keeps_only_read_only_tools() {
        let dir = tempfile::tempdir().unwrap();
        let eng = ZodeEngine::assemble(
            &test_cfg(),
            dir.path().to_path_buf(),
            Arc::new(BypassGate),
            None,
            "2026-06-13",
            None,
            true, // plan_mode
        )
        .await
        .unwrap();
        let names: Vec<String> = eng.tools.names().map(|s| s.to_string()).collect();
        // Read-only research tools survive.
        assert!(names.contains(&"FileRead".to_string()), "{names:?}");
        assert!(names.contains(&"Glob".to_string()), "{names:?}");
        // Mutating/destructive tools and sub-agents are dropped.
        assert!(!names.contains(&"FileWrite".to_string()), "{names:?}");
        assert!(!names.contains(&"Bash".to_string()), "{names:?}");
        assert!(!names.contains(&"Task".to_string()), "{names:?}");
        // The plan-mode preamble is in the system prompt.
        assert!(eng.system.as_deref().unwrap_or("").contains("PLAN MODE"));
    }

    #[tokio::test]
    async fn goal_and_effort_inject_into_system_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_cfg();
        cfg.goal = Some("ship v1 of the parser".into());
        cfg.effort = Some("high".into());
        let eng = ZodeEngine::assemble(
            &cfg,
            dir.path().to_path_buf(),
            Arc::new(BypassGate),
            None,
            "2026-06-13",
            None,
            false,
        )
        .await
        .unwrap();
        let sys = eng.system.as_deref().unwrap_or("");
        assert!(sys.contains("Current goal"), "{sys}");
        assert!(sys.contains("ship v1 of the parser"), "{sys}");
        assert!(sys.contains("Effort: high"), "{sys}");
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
            None,
            false,
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
            None,
            false,
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
            None,
            false,
        )
        .await
        .unwrap();
        let decision = eng
            .permissions
            .evaluate("Bash", &serde_json::json!({}), None);
        assert!(decision.is_deny());
    }
}
