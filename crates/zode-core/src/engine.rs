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
use agent::message::{ContentBlock, MessageStore};
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
use crate::instructions::{
    build_system_prompt, discover_instructions, gather_env, openspec_detected,
};
use crate::noema::ZodeNoema;
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

// A coding agent routinely emits large tool inputs (whole-file writes, big
// diffs). At ~4 chars/token, the old 8192 cap truncated a single FileWrite
// mid-JSON and failed the turn ("tool_use input JSON parse error … EOF").
// 16384 roughly doubles the headroom while staying within the output cap of
// current models (it is exactly GPT-4o's max; Claude 4.x / deepseek-v4 allow
// more). Models with a LOWER output cap (legacy Claude 3/3.5 at 4k–8k) will
// 400 on this — pin a smaller `max_output_tokens` in config for those; a turn
// that still hits the cap mid-tool-call now reports an actionable message.
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 16_384;
const DEFAULT_MODEL_MAX_TOKENS: u32 = 200_000;
const FILE_CACHE_ENTRIES: usize = 1024;

/// Resolve the effective context window: per-provider field wins over the
/// top-level config value; if both are absent, falls back to the default.
fn resolve_context_window(p: &crate::config::ProviderConfig, top: Option<u32>) -> u32 {
    p.context_window.or(top).unwrap_or(DEFAULT_MODEL_MAX_TOKENS)
}

/// Resolve the effective max output tokens: per-provider field wins over the
/// top-level config value; if both are absent, falls back to the default.
fn resolve_max_output(p: &crate::config::ProviderConfig, top: Option<u32>) -> u32 {
    p.max_output_tokens
        .or(top)
        .unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS)
}
const FILE_CACHE_BYTES: usize = 16 * 1024 * 1024;

/// Build the file-tool [`WorkspacePolicy`] from the active sandbox so file
/// writes obey the SAME policy as shell commands (Codex-style — the sandbox
/// governs all writes):
/// - **off** (no sandbox) → writes allowed anywhere (still gated by approval);
/// - **read-only** → every file write denied (reads stay allowed);
/// - **workspace-write** → writes confined to cwd + the extra writable roots.
fn build_workspace_policy(
    cwd: &std::path::Path,
    sandbox: &Option<crate::sandbox::SandboxConfig>,
) -> Result<WorkspacePolicy, CoreError> {
    let err = |e| CoreError::Other(format!("workspace policy: {e}"));
    let mut p = WorkspacePolicy::new(cwd).map_err(err)?;
    match sandbox {
        // Sandbox off: every absolute path is under "/", so writes are
        // unconfined (relative paths still resolve against cwd).
        None => p = p.with_allowed_root("/").map_err(err)?,
        // Read-only: no writable root → every write fails the inside-check.
        Some(sb) if sb.mode() == crate::sandbox::SandboxMode::ReadOnly => {
            p.allowed_roots.clear();
        }
        // Workspace-write: cwd (already a root) plus the configured roots, with
        // `.git` / `.zode` carved back out read-only — the policy-layer twin of
        // the shell sandbox's protected paths, so file tools can't rewrite git
        // history or edit `.zode/state.json` to self-escalate either.
        Some(sb) => {
            for root in sb.writable_roots() {
                p = p.with_allowed_root(root).map_err(err)?;
            }
            for denied in sb.protected_paths() {
                p = p.with_denied_subpath(denied);
            }
        }
    }
    // When an OS sandbox is active, route the file tools' mutating syscalls
    // through it too (Codex parity): the kernel — not just this in-process
    // policy check — enforces the write boundary. Off → keep the direct sink.
    if let Some(sb) = sandbox {
        p = p.with_fs_sink(std::sync::Arc::new(crate::sandbox::SandboxedFsSink::new(
            sb.clone(),
        )));
    }
    Ok(p)
}

/// System-prompt section declaring the current sandbox / write policy, so the
/// agent knows what it may do and retries when the policy is relaxed.
fn sandbox_prompt_note(sandbox: &Option<crate::sandbox::SandboxConfig>) -> String {
    use crate::sandbox::SandboxMode;
    match sandbox {
        None => "\n\n# Sandbox\nThe OS sandbox is OFF. Shell commands and file \
            writes are UNCONFINED — you may read, write, and execute ANYWHERE on \
            the filesystem (e.g. /tmp) and use the network, subject only to \
            per-tool approval. If an earlier action failed because of the \
            sandbox, RETRY it now — it will succeed."
            .to_string(),
        Some(sb) => {
            let mode = match sb.mode() {
                SandboxMode::ReadOnly => {
                    "READ-ONLY — file writes are denied everywhere (reads are fine)"
                }
                SandboxMode::WorkspaceWrite => {
                    "workspace-write — file writes are confined to the workspace directory (+ tmp)"
                }
            };
            let net = if sb.allow_network() {
                "Network is allowed."
            } else {
                "Outbound network is DENIED."
            };
            format!(
                "\n\n# Sandbox\nShell commands and file writes run in an OS sandbox: {mode}. {net} \
                 To write outside the workspace or reach the network, either ask the user to relax it \
                 with `/sandbox` (e.g. off / workspace-write / network on), or — for a shell command — \
                 set `dangerouslyDisableSandbox: true` to request running it outside the sandbox (the \
                 user will be asked to authorize the escape). Do not claim something is impossible \
                 without trying these."
            )
        }
    }
}

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
    /// The model's context window in tokens, driving auto-compaction / context
    /// thresholds. Configurable so 1M-context models use their full window
    /// instead of compacting at the conservative 200K default.
    pub model_max_tokens: u32,
    /// Sampling temperature (None = provider default).
    pub temperature: Option<f32>,
    /// Whether to request provider prompt caching (default on).
    pub prompt_cache: bool,
    /// Native Noema long-term memory adapter. Disabled adapters are cheap no-ops.
    pub noema: ZodeNoema,
    /// Background shell registry (Phase 03/07 inspect this).
    pub bash_sessions: BashSessionRegistry,
    /// Shared TodoWrite state handle (Phase 07 reads the list for the UI).
    pub todo_state: TodoState,
    /// Live registry of Task-spawned sub-agents, read by the TUI overlay.
    pub subagents: crate::subagents::SubAgentRegistry,
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
    /// Spawnable sub-agent types (name, summary): user defs + built-ins.
    /// Surfaced by `/agents`.
    pub agent_types: Vec<(String, String)>,
    /// Saved workflows (name, description). Surfaced by `/workflows`.
    pub workflows: Vec<(String, String)>,
    /// User/plugin slash commands (`commands/<name>.md`) — dynamic commands
    /// whose body is submitted as a turn.
    pub user_commands: Vec<crate::user_commands::UserCommand>,
    /// OpenPencil control-surface config (for the `/op` TUI command).
    pub openpencil: crate::config::OpenPencilConfig,
}

/// What a manual `/compact` run accomplished, for the UI to report.
#[derive(Debug, Clone, Copy)]
pub struct CompactOutcome {
    /// Estimated token total of the transcript BEFORE compaction.
    pub pre_tokens: u32,
    /// Estimated token total of (boundary + summary) AFTER compaction.
    pub post_tokens: u32,
    /// How many messages were folded into the summary.
    pub replaced: usize,
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
    // Cohesive constructor: every arg is shared engine state assembled at
    // startup; splitting into a params struct would only obscure the wiring.
    #[allow(clippy::too_many_arguments)]
    pub async fn assemble(
        cfg: &ZodeConfig,
        cwd: PathBuf,
        gate: Arc<dyn ApprovalGate>,
        sandbox: Option<crate::sandbox::SandboxConfig>,
        date: &str,
        question_tool: Option<Arc<dyn Tool>>,
        op_consent: Option<Arc<dyn crate::openpencil::Consent>>,
        plan_mode: bool,
    ) -> Result<Self, CoreError> {
        let provider = build_provider(&cfg.provider)?;
        let model = cfg
            .provider
            .model
            .clone()
            .ok_or_else(|| CoreError::Other("no model set in config".into()))?;

        // File-tool write confinement (WorkspacePolicy) follows the SAME sandbox
        // policy as shell commands, so `/sandbox off` actually lets file writes
        // escape cwd (matching Codex, where the sandbox governs ALL writes).
        let policy = build_workspace_policy(&cwd, &sandbox)?.into_arc();

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
        // Capture availability before the move so the system prompt can nudge
        // toward the tool only when it's actually registered.
        let has_question_tool = question_tool.is_some();
        if let Some(tool) = question_tool {
            base.register(tool);
        }

        // OpenPencil control surface (op-bridge). Disable via `tools:op`.
        //
        // Fallback consent: denies every lifecycle action when no UI question
        // channel is wired (so the bridge never installs or launches without an
        // interactive confirmation). Hoisted above the resolved-consent binding
        // so the consent can be shared with the later `op_design` registration
        // (which happens after the skills registry is built).
        #[derive(Debug)]
        struct DenyConsent;
        #[async_trait::async_trait]
        impl crate::openpencil::Consent for DenyConsent {
            async fn confirm(&self, _prompt: &str) -> bool {
                false
            }
        }
        // Resolve consent once; reused by op_read/op_write here and op_design
        // after skills are loaded.
        let op_consent_resolved: Arc<dyn crate::openpencil::Consent> =
            op_consent.unwrap_or_else(|| Arc::new(DenyConsent));
        if cfg.openpencil.enabled() {
            use crate::openpencil::tools::{OpReadTool, OpToolDeps, OpWriteTool};
            let deps = OpToolDeps {
                cfg: cfg.openpencil.clone(),
                consent: op_consent_resolved.clone(),
                tag: cfg.openpencil.release_tag().to_string(),
            };
            base.register(Arc::new(OpReadTool::new(deps.clone())));
            base.register(Arc::new(OpWriteTool::new(deps)));
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

        // OpenPencil design pipeline (op-bridge T6). Registered HERE — after the
        // skills registry exists — because it loads design guidance from skills.
        // It reuses the consent resolved above and is Mutating (auto-gated by
        // `wrap_mutating_tools`). Disable via `tools:op`.
        if cfg.openpencil.enabled() {
            use crate::openpencil::tools::{OpDesignDeps, OpDesignTool};
            let design_deps = OpDesignDeps {
                cfg: cfg.openpencil.clone(),
                consent: op_consent_resolved.clone(),
                tag: cfg.openpencil.release_tag().to_string(),
                provider: provider.clone(),
                model: model.clone(),
                skills: skills.clone(),
            };
            base.register(Arc::new(OpDesignTool::new(design_deps)));
        }

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
        // Per-engine sub-agent registry; shared between engine and factory so
        // the Task observer writes here and the TUI reads a snapshot.
        let subagents = crate::subagents::SubAgentRegistry::new();
        // User-defined sub-agents (~/.zode/agents etc.), available alongside the
        // built-in types for the Task tool and listed by `/agents`.
        let agent_defs = crate::agents::load_agent_defs(&cwd);
        let task_factory = Arc::new(ZodeTaskFactory::new(
            provider.clone(),
            model.clone(),
            permissions.clone(),
            cwd.clone(),
            file_cache.clone(),
            hooks.clone(),
            task_tools.clone(),
            agent_defs,
            subagents.clone(),
        ));
        let agent_type_list = task_factory.agent_types();
        base.register(Arc::new(TaskTool::new(task_factory)));

        // Autonomous orchestration: let the agent define new sub-agent types
        // and workflows. Default ON (unset → enabled); toggle off via Settings.
        let orchestration = cfg.autonomous_orchestration.unwrap_or(true);
        if orchestration {
            base.register(Arc::new(crate::agents::DefineAgentTool));
            base.register(Arc::new(crate::workflows::DefineWorkflowTool));
        }
        // Saved workflows (~/.zode/workflows etc.), for `/workflows` + the
        // orchestration directive. Loaded regardless of the toggle so listing
        // works; only advertised in the prompt when orchestration is on.
        let workflow_defs = crate::workflows::load_workflow_defs(&cwd);

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
            Some(sb) => crate::sandbox::apply_sandbox(base, sb, &gate),
            None => base,
        };

        // 2. Wrap mutating/destructive tools with the approval gate.
        let mut gated = wrap_mutating_tools(base, &gate, &cfg.permissions.allow);

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
        let mut system = build_system_prompt(
            &instructions,
            &skills_idx,
            &env,
            cfg.skill_discipline(),
            cfg.openspec_awareness() && openspec_detected(&cwd),
            // Nudge toward the AskUserQuestion tool only when it's actually
            // present (the `question_tool` this assembly was handed).
            has_question_tool,
        );
        // Declare the live sandbox / write policy so the agent knows whether it
        // may write outside cwd or reach the network — and, crucially, RETRIES
        // when the policy changed (e.g. the user ran `/sandbox off`) instead of
        // giving up on a stale earlier failure.
        system.push_str(&sandbox_prompt_note(&sandbox));
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
        match cfg
            .effort
            .as_deref()
            .map(|e| e.trim().to_ascii_lowercase())
            .as_deref()
        {
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
        // Autonomous orchestration directive: encourage task decomposition via
        // sub-agents and advertise the available types + the define_agent tool.
        if orchestration {
            let types = agent_type_list
                .iter()
                .map(|(n, d)| format!("  - {n}: {d}"))
                .collect::<Vec<_>>()
                .join("\n");
            system.push_str(&format!(
                "\n\n# Autonomous orchestration\nFor multi-part or large tasks, decompose the \
                 work and delegate independent sub-tasks to sub-agents with the Task tool \
                 instead of doing everything in one context. Available sub-agent types:\n{types}\n\
                 If no existing type fits, create one with the define_agent tool, then spawn it. \
                 Keep the main thread focused on planning and integrating the results.",
            ));
            // Workflows: always advertise that the agent can CREATE reusable
            // multi-step workflows (define_workflow), and list any saved ones.
            system.push_str(
                "\nFor a repeatable multi-step process, capture it as a reusable workflow \
                 with the define_workflow tool (ordered steps, each with a sub-agent type), \
                 then follow it by running each step via Task.",
            );
            if !workflow_defs.is_empty() {
                let wfs = workflow_defs
                    .iter()
                    .map(|w| {
                        let steps = w
                            .steps
                            .iter()
                            .map(|s| format!("[{}] {}", s.agent_type, s.prompt))
                            .collect::<Vec<_>>()
                            .join(" → ");
                        format!("  - {} ({}): {}", w.name, w.description, steps)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                system.push_str(&format!("\nSaved workflows:\n{wfs}"));
            }
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
            cwd: cwd.clone(),
            max_output_tokens: resolve_max_output(&cfg.provider, cfg.max_output_tokens),
            model_max_tokens: resolve_context_window(&cfg.provider, cfg.context_window),
            temperature: cfg.temperature,
            prompt_cache: cfg.prompt_cache.unwrap_or(true),
            noema: ZodeNoema::from_settings(&cfg.noema),
            bash_sessions,
            todo_state,
            subagents,
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
            agent_types: agent_type_list,
            workflows: workflow_defs
                .into_iter()
                .map(|w| (w.name, w.description))
                .collect(),
            user_commands: crate::user_commands::load_user_commands(&cwd),
            openpencil: cfg.openpencil.clone(),
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

    /// Manually compact the conversation (the `/compact` command).
    ///
    /// Summarizes the whole transcript into a single boundary + summary pair
    /// via the model, then splices it back into the shared store — tombstoning
    /// the replaced messages so later turns send the compact summary instead of
    /// the full history. This is the same machinery the QueryLoop runs
    /// automatically near the context limit, exposed on demand. Returns the
    /// before/after token estimate so the UI can report what was reclaimed.
    pub async fn compact(&self, abort: AbortController) -> Result<CompactOutcome, CoreError> {
        use agent::compact::{
            apply_compaction_to_store, compact_conversation, PartialCompactDirection,
        };
        // Snapshot the transcript so the store lock is never held across the
        // provider await below.
        let messages: Vec<agent::message::Message> = {
            let store = self
                .store
                .lock()
                .map_err(|_| CoreError::Other("compact: message store poisoned".into()))?;
            store.iter().cloned().collect()
        };
        let result = compact_conversation(
            &messages,
            self.provider.as_ref(),
            self.model.clone(),
            None,
            PartialCompactDirection::Full,
            abort,
        )
        .await
        .map_err(|e| CoreError::Other(e.to_string()))?;
        {
            let mut store = self
                .store
                .lock()
                .map_err(|_| CoreError::Other("compact: message store poisoned".into()))?;
            apply_compaction_to_store(&mut store, &result)?;
        }
        Ok(CompactOutcome {
            pre_tokens: result.pre_compact_tokens,
            post_tokens: result.post_compact_tokens,
            replaced: result.replaced_uuids.len(),
        })
    }

    /// Run one turn. Rebuilds a QueryLoop from the shared Arcs.
    pub async fn turn(
        &self,
        user_msg: &str,
        abort: AbortController,
    ) -> Result<Box<dyn EventStream>, agent::error::AgentError> {
        self.turn_blocks(
            vec![ContentBlock::Text {
                text: user_msg.to_string(),
            }],
            abort,
        )
        .await
    }

    /// Run one turn with rich user content blocks such as text plus images.
    pub async fn turn_blocks(
        &self,
        content: Vec<ContentBlock>,
        abort: AbortController,
    ) -> Result<Box<dyn EventStream>, agent::error::AgentError> {
        let query = text_query(&content);
        self.auto_remember_noema(&query);
        let content = self.inject_noema_memory(content, &query);
        self.turn_blocks_raw(content, abort).await
    }

    /// Run one turn without dynamic memory injection. This is for internal
    /// helper turns such as vision-model image description, where user memory
    /// would pollute the auxiliary model task.
    pub async fn turn_blocks_raw(
        &self,
        content: Vec<ContentBlock>,
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
            .model_max_tokens(self.model_max_tokens)
            .cwd(self.cwd.clone())
            .auto_compact(true)
            .use_prompt_cache(self.prompt_cache);
        if let Some(t) = self.temperature {
            builder = builder.temperature(t);
        }
        if let Some(sys) = &self.system {
            builder = builder.system(sys.clone());
        }
        builder.build().run_blocks(content, abort).await
    }

    fn auto_remember_noema(&self, query: &str) {
        if let Err(err) = self
            .noema
            .auto_remember_from_turn(query, Some(self.cwd.as_path()))
        {
            tracing::debug!(error = %err, "auto memory unavailable");
        }
    }

    fn inject_noema_memory(
        &self,
        mut content: Vec<ContentBlock>,
        query: &str,
    ) -> Vec<ContentBlock> {
        if query.trim().is_empty() {
            return content;
        }
        let Some(memory) = self
            .noema
            .recall_for_turn(query, Some(self.cwd.as_path()))
            .ok()
            .flatten()
        else {
            return content;
        };
        if let Some(ContentBlock::Text { text }) = content
            .iter_mut()
            .find(|block| matches!(block, ContentBlock::Text { .. }))
        {
            *text = format!("{memory}\n\n{text}");
        }
        content
    }

    pub fn supports_images(&self) -> bool {
        self.provider.capabilities().supports_images
    }
}

fn text_query(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
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
        // Reuse the tab label for consent routing, exactly like AskUserQuestionTool.
        let op_consent: Option<Arc<dyn crate::openpencil::Consent>> =
            self.question_queue.as_ref().map(|q| {
                Arc::new(crate::openpencil::tools::QueueConsent::new(
                    q.clone(),
                    label.clone(),
                )) as Arc<dyn crate::openpencil::Consent>
            });
        let cwd = cwd_override.unwrap_or_else(|| self.cwd.clone());
        // Rebase the sandbox onto this tab's cwd: a resumed session can run in a
        // different repo, and the sandbox must confine to THAT directory (its
        // writable roots + .git/.zode carveouts), not the launch cwd.
        let sandbox = self.sandbox.as_ref().map(|sb| sb.clone().with_cwd(&cwd));
        ZodeEngine::assemble(
            &self.cfg,
            cwd,
            gate,
            sandbox,
            &self.date,
            question_tool,
            op_consent,
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

    pub fn images(&self) -> &crate::config::ImagesConfig {
        &self.cfg.images
    }

    pub fn provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.cfg.providers.keys().cloned().collect();
        names.sort();
        names
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

    /// The current sandbox config (for the `/sandbox` command to show + toggle).
    pub fn sandbox(&self) -> Option<&crate::sandbox::SandboxConfig> {
        self.sandbox.as_ref()
    }

    /// Clone with the sandbox replaced (for runtime `/sandbox` toggles). The
    /// next `reassemble_active` re-wraps Bash/BashRun accordingly.
    pub fn with_sandbox(&self, sandbox: Option<crate::sandbox::SandboxConfig>) -> Self {
        let mut t = self.clone();
        t.sandbox = sandbox;
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

    pub fn with_images_config(&self, images: crate::config::ImagesConfig) -> Self {
        let mut t = self.clone();
        t.cfg.images = images;
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
    /// Re-loads the effective config (global ⊕ project) for `cwd` — the active
    /// tab's working directory, which may differ from the template's launch cwd —
    /// and adopts its `plugins.disabled`; all other in-session state is preserved.
    /// Errors propagate so the caller can tell the user the reload failed.
    pub fn reload_plugins_from_disk(&self, cwd: &std::path::Path) -> Result<Self, CoreError> {
        let fresh = crate::config::ConfigManager::load(cwd)?;
        Ok(self.with_plugins_disabled(fresh.plugins.disabled))
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

    /// Whether the chat shows thinking/reasoning output (`/thinking`). Default on.
    pub fn show_thinking(&self) -> bool {
        self.cfg.show_thinking.unwrap_or(true)
    }

    /// Whether the chat shows tool-call detail (`/tool-details`). Default on.
    pub fn show_tool_details(&self) -> bool {
        self.cfg.show_tool_details.unwrap_or(true)
    }

    /// The configured UI language code (`language` in config / Settings).
    pub fn language(&self) -> Option<&str> {
        self.cfg.language.as_deref()
    }

    /// Whether autonomous orchestration is on (`/orchestration`). Default ON
    /// (unset → enabled); toggle off via Settings / `/orchestration`.
    pub fn autonomous_orchestration(&self) -> bool {
        self.cfg.autonomous_orchestration.unwrap_or(true)
    }

    /// Clone with autonomous orchestration toggled.
    pub fn with_autonomous_orchestration(&self, on: bool) -> Self {
        let mut t = self.clone();
        t.cfg.autonomous_orchestration = Some(on);
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
/// Gate every mutating tool behind the approval gate, EXCEPT those the user
/// has permanently allowed (`permissions.allow` — e.g. a persisted
/// "allow always" decision), which run un-prompted. Hard-deny rules are
/// enforced separately by the `PermissionManager`, so an allowed-but-denied
/// tool is still blocked.
fn wrap_mutating_tools(
    src: ToolRegistry,
    gate: &Arc<dyn ApprovalGate>,
    allow: &[String],
) -> ToolRegistry {
    let mut out = ToolRegistry::new();
    for tool in src.list() {
        let auto_allowed = allow.iter().any(|a| a == tool.name());
        if matches!(tool.safety_class(), SafetyClass::ReadOnly) || auto_allowed {
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

    #[test]
    fn sandbox_prompt_note_tells_model_the_policy() {
        use crate::sandbox::{SandboxConfig, SandboxMode};
        // OFF: explicitly tells the model writes are unconfined and to retry.
        let off = sandbox_prompt_note(&None);
        assert!(off.contains("OFF"));
        assert!(off.to_uppercase().contains("RETRY"), "{off}");
        assert!(off.contains("ANYWHERE"));
        // workspace-write: says writes confined + how to escape.
        if let Ok(ww) = SandboxConfig::new(
            std::path::Path::new("/tmp"),
            SandboxMode::WorkspaceWrite,
            false,
            &[],
        ) {
            let note = sandbox_prompt_note(&Some(ww));
            assert!(note.contains("workspace-write"));
            assert!(note.contains("dangerouslyDisableSandbox"));
            assert!(note.contains("DENIED"), "network denied by default: {note}");
        }
    }

    #[test]
    fn workspace_policy_follows_sandbox_mode() {
        use crate::sandbox::{SandboxConfig, SandboxMode};
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        // Sandbox OFF → writes allowed anywhere (the user's `/sandbox off` bug).
        let off = build_workspace_policy(cwd, &None).unwrap();
        assert!(
            off.resolve("/tmp/hello.txt", false).is_ok(),
            "off: /tmp must be writable"
        );

        // workspace-write → cwd writable, outside the workspace denied.
        let ww = SandboxConfig::new(cwd, SandboxMode::WorkspaceWrite, false, &[]).unwrap();
        let ww = build_workspace_policy(cwd, &Some(ww)).unwrap();
        assert!(ww.resolve("inside.txt", false).is_ok(), "ww: cwd writable");
        assert!(
            ww.resolve("/tmp/outside.txt", false).is_err(),
            "ww: /tmp denied"
        );

        // read-only → every write denied; reads still resolve.
        let ro = SandboxConfig::new(cwd, SandboxMode::ReadOnly, false, &[]).unwrap();
        let ro = build_workspace_policy(cwd, &Some(ro)).unwrap();
        assert!(
            ro.resolve("inside.txt", false).is_err(),
            "ro: cwd write denied"
        );
        assert!(
            ro.resolve("/tmp/x.txt", false).is_err(),
            "ro: /tmp write denied"
        );
    }

    #[test]
    fn workspace_policy_protects_git_and_zode_from_file_tools() {
        use crate::sandbox::{SandboxConfig, SandboxMode};
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();
        let ww = SandboxConfig::new(cwd, SandboxMode::WorkspaceWrite, false, &[]).unwrap();
        let p = build_workspace_policy(cwd, &Some(ww)).unwrap();
        // Normal workspace writes are fine...
        assert!(p.resolve("src/main.rs", false).is_ok());
        // ...but .git / .zode are read-only even though they sit inside cwd
        // (the file-tool twin of the shell-sandbox carveout). Blocked even
        // before the dirs exist, so the agent can't create state.json to
        // self-escalate.
        assert!(
            p.resolve(".git/config", false).is_err(),
            ".git write must be denied by the policy"
        );
        assert!(
            p.resolve(".zode/state.json", false).is_err(),
            ".zode write must be denied by the policy"
        );
    }
    use agent::message::{ContentBlock, ImageSource, Message};
    use agent::provider::ProviderCapabilities;
    use futures::StreamExt;
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
            noema: crate::config::NoemaSettings {
                enabled: Some(false),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn minimal_engine(provider: Arc<dyn Provider>) -> ZodeEngine {
        ZodeEngine {
            provider,
            tools: Arc::new(ToolRegistry::new()),
            permissions: Arc::new(PermissionManager::new().with_mode(PermissionMode::Bypass)),
            hooks: Arc::new(HookRunner::new()),
            store: Arc::new(Mutex::new(MessageStore::new())),
            file_cache: Arc::new(FileStateCache::new(
                NonZeroUsize::new(1).expect("nonzero"),
                1024,
            )),
            compact_state: Arc::new(Mutex::new(AutoCompactState::default())),
            model: "mock-model".into(),
            system: None,
            cwd: PathBuf::from("."),
            max_output_tokens: 128,
            model_max_tokens: DEFAULT_MODEL_MAX_TOKENS,
            temperature: None,
            prompt_cache: false,
            noema: ZodeNoema::disabled(),
            bash_sessions: BashSessionRegistry::new(),
            todo_state: TodoState::new(),
            subagents: crate::subagents::SubAgentRegistry::new(),
            history: Arc::new(tokio::sync::Mutex::new(EditHistory::new(1))),
            bg_shells_meta: BackgroundShellTracker::new(),
            skills: Arc::new(SkillRegistry::new()),
            mcp: None,
            lsp: None,
            cost: Arc::new(CostState::new("mock-model".into())),
            plugins: PluginManager::default(),
            all_mcp_servers: Vec::new(),
            all_skill_meta: Vec::new(),
            lsp_langs: Vec::new(),
            agent_types: Vec::new(),
            workflows: Vec::new(),
            user_commands: Vec::new(),
            openpencil: Default::default(),
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
    async fn turn_blocks_preserves_rich_user_content() {
        let mut caps = ProviderCapabilities::default();
        caps.supports_images = true;
        let provider = agent::testing::MockProvider::new(Vec::new()).with_capabilities(caps);
        let eng = minimal_engine(Arc::new(provider));
        let content = vec![
            ContentBlock::Text {
                text: "describe".into(),
            },
            ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "abc123".into(),
                },
            },
        ];

        let mut stream = eng
            .turn_blocks(content.clone(), AbortController::new())
            .await
            .unwrap();
        while stream.next().await.is_some() {}

        let store = eng.store.lock().unwrap();
        let Message::User {
            content: observed, ..
        } = store.iter().next().unwrap()
        else {
            panic!("expected first message to be user content");
        };
        assert_eq!(observed, &content);
    }

    #[cfg(feature = "noema")]
    #[tokio::test]
    async fn turn_blocks_injects_noema_memory_into_text_block() {
        let provider = agent::testing::MockProvider::new(Vec::new());
        let mut eng = minimal_engine(Arc::new(provider));
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        eng.cwd = cwd.path().to_path_buf();
        eng.noema = ZodeNoema::from_root_with_user(root.path(), Some("kay".to_string()));
        eng.noema
            .remember(
                "Prefer Rust for zode Noema integration work.",
                crate::noema::ZodeMemoryScope::User,
                None,
            )
            .unwrap();

        let mut stream = eng
            .turn_blocks(
                vec![ContentBlock::Text {
                    text: "rust integration preference?".into(),
                }],
                AbortController::new(),
            )
            .await
            .unwrap();
        while stream.next().await.is_some() {}

        let store = eng.store.lock().unwrap();
        let Message::User { content, .. } = store.iter().next().unwrap() else {
            panic!("expected first message to be user content");
        };
        let ContentBlock::Text { text } = &content[0] else {
            panic!("expected text content");
        };
        assert!(text.contains("## Relevant Memories"), "{text}");
        assert!(
            text.contains("Prefer Rust for zode Noema integration work."),
            "{text}"
        );
        assert!(text.contains("rust integration preference?"), "{text}");
    }

    #[cfg(feature = "noema")]
    #[tokio::test]
    async fn turn_blocks_auto_remembers_explicit_memory_request() {
        let provider = agent::testing::MockProvider::new(Vec::new());
        let mut eng = minimal_engine(Arc::new(provider));
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        eng.cwd = cwd.path().to_path_buf();
        eng.noema = ZodeNoema::from_root_with_user(root.path(), Some("kay".to_string()));

        let mut stream = eng
            .turn_blocks(
                vec![ContentBlock::Text {
                    text: "请记住我喜欢 Rust 工具".into(),
                }],
                AbortController::new(),
            )
            .await
            .unwrap();
        while stream.next().await.is_some() {}

        let recalled = eng
            .noema
            .recall_for_turn("Rust 工具", Some(cwd.path()))
            .unwrap()
            .expect("auto memory recalled");
        assert!(recalled.contains("我喜欢 Rust 工具"), "{recalled}");
    }

    #[tokio::test]
    async fn compact_folds_transcript_into_summary() {
        use agent::message::Header;
        use agent::stream::Event;

        // A valid summarization response: <analysis> then <summary>.
        let response =
            "<analysis>covered the basics</analysis>\n<summary>We set up the project.</summary>";
        let provider = agent::testing::MockProvider::new(vec![Event::TextDelta {
            delta: response.into(),
        }]);
        let eng = minimal_engine(Arc::new(provider));
        {
            let mut store = eng.store.lock().unwrap();
            store
                .push(Message::User {
                    header: Header::new(),
                    content: vec![ContentBlock::Text {
                        text: "set up the project".into(),
                    }],
                })
                .unwrap();
            store
                .push(Message::Assistant {
                    header: Header::new(),
                    content: vec![ContentBlock::Text {
                        text: "done, here is the scaffold".into(),
                    }],
                })
                .unwrap();
        }

        let outcome = eng.compact(AbortController::new()).await.unwrap();
        assert_eq!(outcome.replaced, 2, "both messages folded in");
        assert!(outcome.post_tokens > 0);

        let store = eng.store.lock().unwrap();
        let tombstones = store
            .iter()
            .filter(|m| matches!(m, Message::Tombstone { .. }))
            .count();
        assert_eq!(tombstones, 2, "originals tombstoned");
        // The summary is spliced back as a User message prefixed with
        // "[Context summary]" (so Anthropic, which drops System body text,
        // still sees it).
        let has_summary = store.iter().any(|m| {
            matches!(
                m,
                Message::User { content, .. }
                    if content.iter().any(|b| matches!(
                        b,
                        ContentBlock::Text { text }
                            if text.contains("[Context summary]")
                                && text.contains("We set up the project")
                    ))
            )
        });
        assert!(has_summary, "summary message spliced in");
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
    async fn orchestration_is_on_by_default() {
        // Unset autonomous_orchestration → ON: define_agent registered + the
        // orchestration directive injected into the system prompt.
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg();
        assert!(cfg.autonomous_orchestration.is_none());
        let eng = ZodeEngine::assemble(
            &cfg,
            dir.path().to_path_buf(),
            Arc::new(BypassGate),
            None,
            "2026-06-13",
            None,
            None,
            false,
        )
        .await
        .unwrap();
        let names: Vec<String> = eng.tools.names().map(|s| s.to_string()).collect();
        assert!(names.contains(&"define_agent".to_string()), "{names:?}");
        assert!(eng
            .system
            .as_deref()
            .unwrap_or("")
            .contains("Autonomous orchestration"));
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

    #[test]
    fn limits_prefer_provider_over_top_level() {
        use crate::config::ProviderConfig;
        let provider = ProviderConfig {
            context_window: Some(1_000_000),
            max_output_tokens: Some(8192),
            ..Default::default()
        };
        assert_eq!(resolve_context_window(&provider, Some(200_000)), 1_000_000);
        assert_eq!(resolve_max_output(&provider, Some(4096)), 8192);

        let bare = ProviderConfig::default();
        assert_eq!(resolve_context_window(&bare, Some(200_000)), 200_000); // top-level
        assert_eq!(
            resolve_context_window(&bare, None),
            DEFAULT_MODEL_MAX_TOKENS
        ); // default
        assert_eq!(resolve_max_output(&bare, None), DEFAULT_MAX_OUTPUT_TOKENS);
    }
}
