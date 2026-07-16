//! ZodeEngine — assembles the agent QueryLoop's shared state and drives
//! one turn at a time. `QueryLoop::run` consumes `self`, so each turn
//! rebuilds a loop from these Arcs (cheap — all fields are Arc).

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
    register_default_with_todo, BashOutputTool, BashRunTool, BashSessionRegistry, BashTool,
    KillShellTool, TaskTool, TodoState, ToolSearchTool, WorkspacePolicy,
};
// QueryLoop's builder takes std::sync::Mutex (not tokio's). We never hold
// these guards across an await — callers snapshot (MessageStore: Clone)
// before async work.
use std::sync::Mutex;

use crate::approval::{ApprovalGate, ApprovalQueue, BypassGate, QueueGate};
use crate::bg_shells::{BackgroundShellTracker, BgShellHook};
use crate::browser::{
    BrowserActTool, BrowserEvalTool, BrowserReadTool, BrowserSession, BrowserTabsTool,
    BrowserTarget, BrowserToolDeps, BrowserUploadTool, ManagedFactory,
};
use crate::config::ZodeConfig;
use crate::cost::CostState;
use crate::error::CoreError;
use crate::gated_tool::PermissionGatedTool;
use crate::history::{EditHistory, EditHistoryHook};
use crate::hooks_config::load_hook_handlers;
use crate::instructions::{
    build_system_prompt, discover_instructions, gather_env, gather_env_with_branch,
    openspec_detected, PromptFlags,
};
use crate::noema::ZodeNoema;
use crate::plugin::PluginManager;
use crate::provider::build_provider;
use crate::skills::{skills_dirs, skills_index, SkillTool};
use crate::task_factory::{
    resolve_subagent_max_iterations, ModelRuntimeState, ParentToolsCell, ZodeTaskFactory,
};

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
const PRE_TURN_COMPACT_PERCENT: u64 = 98;

/// Process-cached models.dev catalog (parsed once) used to look up a model's
/// published context window when the config doesn't pin one.
fn cached_catalog() -> &'static crate::Catalog {
    static CATALOG: std::sync::OnceLock<crate::Catalog> = std::sync::OnceLock::new();
    CATALOG.get_or_init(crate::Catalog::load_blocking)
}

/// Resolve the effective context window: an explicit per-provider field wins,
/// then the top-level config value, then the model's published window from the
/// models.dev `catalog` (so different models get their real, differing max
/// context), and finally the conservative default. `provider_id` scopes the
/// catalog lookup to the active provider so a model id shared across providers
/// (different windows) resolves the right one.
fn resolve_context_window(
    p: &crate::config::ProviderConfig,
    top: Option<u32>,
    provider_id: Option<&str>,
    catalog: &crate::Catalog,
) -> u32 {
    p.context_window
        .or(top)
        .or_else(|| {
            p.model
                .as_deref()
                .and_then(|m| catalog.context_for_model_scoped(provider_id, m))
        })
        .unwrap_or(DEFAULT_MODEL_MAX_TOKENS)
}

/// Resolve the effective max output tokens, AUTOMATICALLY where possible so the
/// user rarely needs to pin it. Order: an explicit per-provider / top-level
/// value wins, then the model's published cap from the models.dev `catalog`,
/// then a conservative default — and the result is clamped to strictly below
/// the context window.
///
/// The explicit value is honored ONLY if it is physically sane: output tokens
/// are part of the context window, so a value `>= context_window` (almost always
/// a copy of `contextWindow`) is a mistake and is ignored, falling through to
/// auto-resolution. That turns the common "maxOutputTokens == contextWindow"
/// misconfig from a hard API error (e.g. xfyun 10163) into a working default.
fn resolve_max_output(
    p: &crate::config::ProviderConfig,
    top: Option<u32>,
    provider_id: Option<&str>,
    catalog: &crate::Catalog,
    context_window: u32,
) -> u32 {
    let sane = |n: u32| n > 0 && (context_window == 0 || n < context_window);
    let explicit = p.max_output_tokens.or(top);
    // Surface why a deliberately-set value is being ignored (vs silently
    // overriding it) — the usual cause is copying `contextWindow`.
    if let Some(n) = explicit {
        if !sane(n) {
            // Fires for both `n == 0` and `n >= context_window` (output tokens
            // are part of the window — the usual cause is copying contextWindow).
            tracing::warn!(
                "ignoring invalid maxOutputTokens={n} (context window {context_window}); \
                 it must be a positive value below the context window — \
                 auto-resolving the cap instead"
            );
        }
    }
    let resolved = explicit
        .filter(|&n| sane(n))
        .or_else(|| {
            p.model
                .as_deref()
                .and_then(|m| catalog.max_output_for_model_scoped(provider_id, m))
                .filter(|&n| sane(n))
        })
        .unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS);
    // Never request output >= the context window. A window of 0/1 is degenerate
    // (no real model has one) — skip the clamp so we don't return 0, which every
    // provider rejects as `max_tokens=0`.
    if context_window <= 1 {
        resolved
    } else {
        resolved.min(context_window - 1)
    }
}

/// Fill provider capabilities that the user did not explicitly configure from
/// models.dev metadata for the active model. Explicit `supportsImages` always
/// wins; catalog inference is only the safe default for otherwise-unannotated
/// OpenAI-compatible providers, whose runtime default is intentionally false.
fn resolve_provider_capabilities(
    provider: &crate::config::ProviderConfig,
    provider_id: Option<&str>,
    catalog: &crate::Catalog,
) -> crate::config::ProviderConfig {
    let mut resolved = provider.clone();
    if resolved.supports_images.is_none() {
        resolved.supports_images = resolved
            .model
            .as_deref()
            .and_then(|model| catalog.supports_images_for_model_scoped(provider_id, model));
    }
    resolved
}

fn effective_provider_supports_images(
    provider: &crate::config::ProviderConfig,
    provider_id: Option<&str>,
    catalog: &crate::Catalog,
) -> bool {
    let resolved = resolve_provider_capabilities(provider, provider_id, catalog);
    build_provider(&resolved)
        .map(|provider| provider.capabilities().supports_images)
        .unwrap_or(false)
}

/// Map the `/effort` setting onto real provider reasoning knobs. Prose in the
/// system prompt stays as fallback for providers with no knob.
pub(crate) fn map_effort(
    effort: Option<&str>,
    supports_thinking: bool,
    reasoning_opt_in: bool,
) -> (Option<agent::provider::ThinkingConfig>, Option<String>) {
    let norm = effort.map(str::trim).map(str::to_ascii_lowercase);
    // Precedence: a native thinking budget (Anthropic) wins for "high" — the
    // effort string rides along too, so the vendor adaptive-thinking path
    // can emit output_config.effort for models that require adaptive
    // thinking. Otherwise the opt-in OpenAI-style effort string is
    // forwarded verbatim (including an explicit "medium"); "medium" is the
    // provider default and maps to no knob at all when not opted in.
    match norm.as_deref() {
        Some("high") if supports_thinking => (
            Some(agent::provider::ThinkingConfig::new(8192)),
            Some("high".to_string()),
        ),
        Some(e @ ("low" | "medium" | "high")) if reasoning_opt_in => (None, Some(e.to_string())),
        _ => (None, None),
    }
}

fn pre_turn_compact_needed(input_tokens: u32, context_window: u32, max_output_tokens: u32) -> bool {
    if context_window == 0 {
        return false;
    }
    let threshold = ((context_window as u64) * PRE_TURN_COMPACT_PERCENT / 100) as u32;
    let output_budget = if context_window <= 1 {
        max_output_tokens
    } else {
        max_output_tokens.min(context_window - 1)
    };
    input_tokens.saturating_add(output_budget) >= threshold
}

/// Resolve the agent loop's runaway backstop. The loop already stops the moment
/// the model returns a turn with no tool calls, so this only guards against a
/// model that never converges. Absent or `0` → effectively unbounded
/// (`usize::MAX`); a positive value imposes that finite cap.
fn resolve_max_iterations(top: Option<u32>) -> usize {
    match top {
        Some(n) if n > 0 => n as usize,
        _ => usize::MAX,
    }
}

/// Transient-API-error retry count. Absent → 10; `0` disables retries.
fn resolve_max_api_retries(cfg: Option<u32>) -> u32 {
    cfg.unwrap_or(10)
}
const FILE_CACHE_BYTES: usize = 16 * 1024 * 1024;

/// Test-only re-export so `fs_escalate`'s end-to-end test can build the very
/// same confined / unconfined policies the engine assembles.
#[cfg(test)]
pub(crate) fn build_workspace_policy_for_test(
    cwd: &std::path::Path,
    sandbox: &Option<crate::sandbox::SandboxConfig>,
) -> Result<WorkspacePolicy, CoreError> {
    build_workspace_policy(cwd, sandbox)
}

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
        // Strict-read: also hide credential dirs from the in-process FILE-READ
        // tools (the OS sandbox only covers shell commands, so without this the
        // agent could `Read` ~/.ssh directly). resolve_read rejects these.
        if sb.restrict_reads() {
            for dir in crate::sandbox::read_denied_dirs() {
                p = p.with_read_denied_subpath(dir);
            }
        }
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
                    "READ-ONLY — file writes are denied everywhere (reads are fine)".to_string()
                }
                SandboxMode::WorkspaceWrite => format!(
                    "workspace-write — file writes are confined to {}",
                    sb.write_scope_summary()
                ),
            };
            let net = if sb.is_windows_tier_one() {
                if sb.is_windows_tier_two() {
                    "Network is DENIED by AppContainer capability omission (Tier 2), including loopback."
                } else {
                    "network unenforced (Windows Tier 1)"
                }
            } else if sb.allow_network() {
                "Network is allowed."
            } else {
                "Outbound network is DENIED."
            };
            let reads = if sb.restrict_reads() {
                " Strict-read is ON: credential dirs (~/.ssh, ~/.aws, the zode config, …) are hidden from reads (both shell commands and the file tools)."
            } else {
                ""
            };
            format!(
                "\n\n# Sandbox\nShell commands and file writes run in an OS sandbox: {mode}. {net}{reads} \
                 When the sandbox blocks a file write, zode asks the user whether to perform it outside \
                 the workspace and completes it on consent — so do NOT retry the same write, and do NOT \
                 route around it with a shell command. For a shell command that genuinely needs the \
                 network or an outside write, set `sandbox_permissions: \"require_escalated\"` with a \
                 short `justification`; the user is asked to authorize that escape before it runs. \
                 A refusal means the user declined: report it, do not look for another way around. \
                 The user can also relax the sandbox with `/sandbox` (off / workspace-write / network on)."
            )
        }
    }
}

/// Long-lived, session-scoped shared state carried across an engine
/// reassembly so a model/provider/plugin/sandbox hot-swap doesn't reset it.
/// Every field is `None` on a fresh build (assemble makes new instances);
/// reassembly fills them from the outgoing engine. See
/// [`ZodeEngine::assemble_with_carry`].
#[derive(Clone, Default)]
pub struct CarryState {
    pub cost: Option<Arc<CostState>>,
    pub history: Option<Arc<tokio::sync::Mutex<crate::history::EditHistory>>>,
    pub bash_sessions: Option<BashSessionRegistry>,
    pub todo_state: Option<TodoState>,
    pub compact_state: Option<Arc<Mutex<AutoCompactState>>>,
    pub subagents: Option<crate::subagents::SubAgentRegistry>,
    pub file_cache: Option<Arc<FileStateCache>>,
    pub bg_shells_meta: Option<BackgroundShellTracker>,
    pub recent_files: Option<crate::compact_memory::RecentFiles>,
    pub verification: Option<crate::verification::VerificationState>,
    pub tool_trace: Option<crate::tool_trace::ToolTrace>,
    pub reminders: Option<crate::reminders::ReminderTracker>,
}

impl ZodeEngine {
    /// Snapshot this engine's long-lived shared state for carry-over into a
    /// reassembled engine (see [`CarryState`]).
    pub fn carry_state(&self) -> CarryState {
        CarryState {
            cost: Some(self.cost.clone()),
            history: Some(self.history.clone()),
            bash_sessions: Some(self.bash_sessions.clone()),
            todo_state: Some(self.todo_state.clone()),
            compact_state: Some(self.compact_state.clone()),
            subagents: Some(self.subagents.clone()),
            file_cache: Some(self.file_cache.clone()),
            bg_shells_meta: Some(self.bg_shells_meta.clone()),
            recent_files: Some(self.recent_files.clone()),
            verification: Some(self.verification.clone()),
            tool_trace: Some(self.tool_trace.clone()),
            reminders: Some(self.reminders.clone()),
        }
    }
}

fn cost_state_for_model(cfg: &ZodeConfig, model: String) -> Arc<CostState> {
    let mut catalog = agent::cost::ModelPriceCatalog::with_defaults();
    if let Some(prices) = cfg.provider.price_overrides() {
        catalog.insert(model.clone(), prices);
    }
    let currency_code = cfg.currency.as_deref().unwrap_or("USD");
    Arc::new(CostState::new_with(model, catalog, currency_code))
}

#[allow(clippy::too_many_arguments)]
fn render_runtime_system_prompt(
    cfg: &ZodeConfig,
    cwd: &std::path::Path,
    date: &str,
    sandbox: &Option<crate::sandbox::SandboxConfig>,
    plan_mode: bool,
    has_question_tool: bool,
    has_todo_tool: bool,
    has_run_check_tool: bool,
    model: &str,
    skills: &SkillRegistry,
    agent_type_list: &[(String, String)],
    workflow_defs: &[crate::workflows::WorkflowDef],
    // Enabled LSP languages — advertised so the agent actually reaches for the
    // `lsp_*` tools instead of grepping.
    lsp_langs: &[String],
    // `Some(branch)` when the caller precomputed it off-thread; `None` →
    // detect inline (the synchronous hot-swap path).
    git_branch: Option<Option<String>>,
) -> String {
    let skills_idx = skills_index(skills);
    let mut env = match git_branch {
        Some(b) => gather_env_with_branch(cwd, date, b),
        None => gather_env(cwd, date),
    };
    // Tell the agent which model it's running on (so "what model are you?"
    // is answerable). Stable across a session, so it doesn't hurt caching.
    env.model = model.to_string();
    let instructions = discover_instructions(cwd);
    let flags = PromptFlags {
        skill_discipline: cfg.skill_discipline(),
        openspec: cfg.openspec_awareness() && openspec_detected(cwd),
        // Nudge toward the AskUserQuestion tool only when it's actually present.
        ask_user_question: has_question_tool,
        todo: has_todo_tool,
        // Reflect actual tool availability, exactly like `has_todo_tool`.
        // `run_check` is Mutating and is filtered both by plan mode and by
        // read-only mode (`filter_read_only`) — deriving this from the final
        // gated registry keeps the prompt in sync in both cases instead of
        // hardcoding only the plan-mode gate.
        verify_tool: has_run_check_tool,
    };
    let mut system = build_system_prompt(&instructions, &skills_idx, &env, &flags);
    // Declare the live sandbox / write policy so the agent knows whether it may
    // write outside cwd or reach the network.
    system.push_str(&sandbox_prompt_note(sandbox));
    system.push_str(&crate::instructions::lsp_prompt_note(lsp_langs));
    if plan_mode {
        system.push_str(PLAN_MODE_PROMPT);
    }
    // A persistent goal (`/goal`) keeps the agent focused on one objective AND
    // drives an autonomous multi-turn loop.
    if let Some(goal) = cfg.goal.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
        // `run_check` is Mutating and can be filtered out of the gated
        // registry (plan mode / read-only mode) — only tell the model to
        // rely on it when it's actually available, same signal as
        // `flags.verify_tool` above.
        let verify_clause = if has_run_check_tool {
            "Before claiming completion, \
             run the `run_check` tool with the exact verification command or \
             invariant that proves the work is done. When — and only when — the \
             goal is FULLY achieved and `run_check` has fresh passing evidence, \
             call the `goal_complete` tool with a short summary to end the loop."
        } else {
            "Before claiming completion, verify the goal is achieved by the \
             means available to you. When — and only when — the goal is FULLY \
             achieved, call the `goal_complete` tool with a short summary to \
             end the loop."
        };
        system.push_str(&format!(
            "\n\n# Current goal\nKeep this objective in focus for every turn; \
             if a request is ambiguous, resolve it toward the goal:\n{goal}\n\n\
             You are working AUTONOMOUSLY toward this goal across multiple turns. \
             Each turn, take the next concrete step (research, edit, run, verify) — \
             do not just describe what you would do; actually do it. The loop \
             continues automatically after every turn. {verify_clause} \
             Do not call `goal_complete` prematurely, and do not stop early \
             otherwise; if work remains, keep going on the next turn."
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
    if cfg.autonomous_orchestration.unwrap_or(true) {
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
        system.push_str(
            "\nFor a repeatable multi-step process, capture it as a reusable JS \
             workflow with the define_workflow tool (an orchestration script using \
             agent()/parallel()/pipeline()), then execute it with the run_workflow \
             tool — zode runs the script deterministically; do not re-follow its \
             steps by hand.",
        );
        if !workflow_defs.is_empty() {
            let wfs = workflow_defs
                .iter()
                .map(|w| format!("  - {}: {}", w.name, w.description))
                .collect::<Vec<_>>()
                .join("\n");
            system.push_str(&format!(
                "\nSaved workflows (execute via run_workflow):\n{wfs}"
            ));
        }
    }
    system
}

pub struct ZodeEngine {
    pub provider: Arc<dyn Provider>,
    pub tools: Arc<ToolRegistry>,
    pub permissions: Arc<PermissionManager>,
    pub hooks: Arc<HookRunner>,
    pub store: Arc<Mutex<MessageStore>>,
    pub file_cache: Arc<FileStateCache>,
    pub compact_state: Arc<Mutex<AutoCompactState>>,
    /// Mid-turn steering: the host sends user messages here while a turn is
    /// running; the QueryLoop drains them between round-trips and injects
    /// them as user turns. `sender` for the host, `receiver` (shared) handed
    /// to each turn's loop.
    steer_tx: futures::channel::mpsc::UnboundedSender<Vec<ContentBlock>>,
    steer_rx: Arc<std::sync::Mutex<futures::channel::mpsc::UnboundedReceiver<Vec<ContentBlock>>>>,
    pub model: String,
    pub system: Option<String>,
    pub cwd: PathBuf,
    pub max_output_tokens: u32,
    /// The model's context window in tokens, driving auto-compaction / context
    /// thresholds. Configurable so 1M-context models use their full window
    /// instead of compacting at the conservative 200K default.
    pub model_max_tokens: u32,
    /// Runaway backstop on agent-loop iterations. The loop's real stop condition
    /// is "the model returned a turn with no tool calls" — this only guards
    /// against a model that never converges. Default is effectively unbounded;
    /// set `maxIterations` in config to impose a finite cap (useful for headless
    /// `-p` runs that can't be interrupted).
    pub max_iterations: usize,
    /// Transient-API-error retries (rate limit / 5xx / network) with exponential
    /// backoff before a turn fails. Default 10.
    pub max_api_retries: u32,
    /// Sampling temperature (None = provider default).
    pub temperature: Option<f32>,
    /// Anthropic-style extended-thinking budget, derived from `/effort` when
    /// the active provider declares `supports_thinking`. See [`map_effort`].
    pub thinking: Option<agent::provider::ThinkingConfig>,
    /// OpenAI-style `reasoning_effort` string ("low" | "high"), derived from
    /// `/effort` when the provider config opts in via `reasoning: true`. See
    /// [`map_effort`].
    pub reasoning_effort: Option<String>,
    /// Whether to request provider prompt caching (default on).
    pub prompt_cache: bool,
    /// Native Noema long-term memory adapter. Disabled adapters are cheap no-ops.
    pub noema: ZodeNoema,
    /// Resolved knobs for the post-turn LLM extraction pass (on by default;
    /// disable via `noema.autoExtract`).
    pub extract_config: crate::noema_extract::ExtractConfig,
    /// Resolved compaction-ladder / restoration knobs (`compact` config key).
    pub compact_settings: crate::config::CompactSettings,
    /// Durable sink for compaction analysis bullets. `None` when the sink
    /// or noema itself is disabled.
    pub session_store: Option<Arc<crate::compact_memory::NoemaSessionStore>>,
    /// Most-recently-touched files, fed by the compact-tracker hook.
    pub recent_files: crate::compact_memory::RecentFiles,
    /// Latched by the tracker hook when a compaction replaced messages;
    /// consumed (swap-false) at the start of the next turn.
    restore_pending: Arc<std::sync::atomic::AtomicBool>,
    /// UI note produced by the last restoration, consumed by the front-end
    /// via [`Self::take_restore_note`].
    last_restore_note: Arc<std::sync::Mutex<Option<String>>>,
    /// Shared provider/model snapshot used by tools that call an LLM internally.
    model_runtime: ModelRuntimeState,
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
    /// Built-in browser control session (shared by all `browser_*` tools and
    /// the `/browser` TUI panel). All tabs from the same `EngineTemplate`
    /// share one `Arc` — one browser process per zode run.
    pub browser: Arc<BrowserSession>,
    /// Per-engine browser target pin baked into this engine's `browser_*`
    /// tools at assembly (extension task engines pin `Bridge`). `None` means
    /// the tools follow the session-wide `/browser target` selection.
    pub browser_target_override: Option<BrowserTarget>,
    /// Shared completion signal for the autonomous goal loop. The registered
    /// `goal_complete` tool flips this; the TUI polls it after each turn to
    /// decide whether to stop looping. Created fresh in `assemble` so a rebuilt
    /// engine and its tool always share the same `Arc`.
    goal_completed: Arc<AtomicBool>,
    /// Verification evidence produced by `run_check`; mutating tools make it
    /// stale and `goal_complete` requires it to be fresh.
    pub verification: crate::verification::VerificationState,
    /// Durable JSONL trace file for full tool inputs/outputs, referenced by export.
    pub tool_trace: crate::tool_trace::ToolTrace,
    /// Per-turn system reminders: external file drift, todo staleness, and
    /// (via `check_branch_drift`) git branch drift.
    pub reminders: crate::reminders::ReminderTracker,
    /// Optional cap on autonomous goal-loop turns (`autoLoopMaxTurns`). `None`
    /// means unbounded — the loop runs until `goal_complete` or user interrupt.
    auto_loop_max_turns: Option<u32>,
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

/// Context occupancy (percent of the model window) above which a
/// full-transcript summarize request is unsafe: the request carries the whole
/// transcript plus prompt and reserved output, so near/over the window it
/// gets a 400 context-overflow from the provider — the very condition
/// compaction is meant to fix, a deadlock. Above the line, compact the
/// EARLIEST HALF instead: the request carries roughly half the transcript and
/// the recent half survives verbatim (run compact again to halve further).
const FULL_COMPACT_SAFE_PERCENT: u64 = 60;

/// Pick the compaction direction for a transcript of `context_tokens` against
/// a `window`-token model. `window == 0` means "unknown" → Full (no basis to
/// restrict).
fn compact_direction(context_tokens: u32, window: u32) -> agent::compact::PartialCompactDirection {
    use agent::compact::PartialCompactDirection;
    if window != 0 && (context_tokens as u64 * 100 / window as u64) >= FULL_COMPACT_SAFE_PERCENT {
        PartialCompactDirection::EarliestHalf
    } else {
        PartialCompactDirection::Full
    }
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
        browser: Option<Arc<BrowserSession>>,
    ) -> Result<Self, CoreError> {
        Self::assemble_with_carry(
            cfg,
            cwd,
            gate,
            sandbox,
            date,
            question_tool,
            op_consent,
            plan_mode,
            browser,
            CarryState::default(),
        )
        .await
    }

    /// Like [`assemble`] but reusing the caller-supplied long-lived session
    /// state (`carry`) instead of building fresh instances. Used by
    /// reassembly (model/provider/plugin/sandbox hot-swap) so the accumulated
    /// cost, undo history, background-shell registry, todo list, sub-agent
    /// overlay, compaction latches, and file-read cache SURVIVE the rebuild —
    /// and crucially so the new engine's internal wiring (edit-history hook,
    /// bg-shell hook, Task factory, cost observer) targets the SAME carried
    /// instances the UI reads, avoiding a split-brain where new work writes
    /// one copy while the UI shows another.
    #[allow(clippy::too_many_arguments)]
    pub async fn assemble_with_carry(
        cfg: &ZodeConfig,
        cwd: PathBuf,
        gate: Arc<dyn ApprovalGate>,
        sandbox: Option<crate::sandbox::SandboxConfig>,
        date: &str,
        question_tool: Option<Arc<dyn Tool>>,
        op_consent: Option<Arc<dyn crate::openpencil::Consent>>,
        plan_mode: bool,
        browser: Option<Arc<BrowserSession>>,
        carry: CarryState,
    ) -> Result<Self, CoreError> {
        Self::assemble_with_carry_and_access(
            cfg,
            cfg.active_provider_key(),
            cwd,
            gate,
            sandbox,
            date,
            question_tool,
            op_consent,
            plan_mode,
            false,
            browser,
            None,
            carry,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn assemble_with_carry_and_access(
        cfg: &ZodeConfig,
        selected_provider_key: Option<&str>,
        cwd: PathBuf,
        gate: Arc<dyn ApprovalGate>,
        sandbox: Option<crate::sandbox::SandboxConfig>,
        date: &str,
        question_tool: Option<Arc<dyn Tool>>,
        op_consent: Option<Arc<dyn crate::openpencil::Consent>>,
        plan_mode: bool,
        read_only_tools: bool,
        browser: Option<Arc<BrowserSession>>,
        browser_target_override: Option<BrowserTarget>,
        carry: CarryState,
    ) -> Result<Self, CoreError> {
        // Reuse the caller's session (all tabs share ONE browser process) or
        // build a fresh one — cheap: the managed backend only launches
        // chromium lazily, on the first `lease()`.
        let browser_session = browser
            .unwrap_or_else(|| BrowserSession::new(cfg.browser.clone(), Arc::new(ManagedFactory)));
        let active_provider_key = selected_provider_key.or_else(|| cfg.active_provider_key());
        let provider_cfg =
            resolve_provider_capabilities(&cfg.provider, active_provider_key, cached_catalog());
        let provider = build_provider(&provider_cfg)?;
        let model = cfg
            .provider
            .model
            .clone()
            .ok_or_else(|| CoreError::Other("no model set in config".into()))?;
        let model_runtime = ModelRuntimeState::new(provider.clone(), model.clone());

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
        let todo_state = carry.todo_state.clone().unwrap_or_default();
        let verification = carry.verification.clone().unwrap_or_default();
        let tool_trace = carry
            .tool_trace
            .clone()
            .unwrap_or_else(|| crate::tool_trace::ToolTrace::new(&cwd));
        let reminders = carry.reminders.clone().unwrap_or_default();
        register_default_with_todo(&mut base, policy.clone(), todo_state.clone());
        // A sandbox-blocked file write must ask the user to escalate rather than
        // dead-end: otherwise the model retries the same write and finally works
        // around it with a `Bash` heredoc + `require_escalated`, which is both
        // noisy and a worse audit trail than a single, explicit prompt. Wrap the
        // mutating file tools with an unconfined twin they can replay onto once
        // the user consents. Only meaningful when a sandbox is actually active.
        if sandbox.is_some() {
            let mut unconfined = ToolRegistry::new();
            let unconfined_policy = build_workspace_policy(&cwd, &None)?.into_arc();
            register_default_with_todo(&mut unconfined, unconfined_policy, todo_state.clone());
            base = crate::fs_escalate::apply_fs_escalation(base, &unconfined, &gate);
        }
        base.register(Arc::new(BashTool::with_compress_output(
            policy.clone(),
            cfg.compress_output(),
        )));
        let bash_sessions = carry.bash_sessions.clone().unwrap_or_default();
        base.register(Arc::new(BashRunTool::new(
            policy.clone(),
            bash_sessions.clone(),
        )));
        base.register(Arc::new(BashOutputTool::with_compress_output(
            bash_sessions.clone(),
            cfg.compress_output(),
        )));
        base.register(Arc::new(KillShellTool::new(bash_sessions.clone())));

        // Goal auto-loop completion signal. Created BEFORE tool registration so
        // the `goal_complete` tool and the engine struct share the SAME `Arc`:
        // the tool flips it, the host polls it after each turn to stop looping.
        // Always registered (cheap, read-only) so a goal can be set mid-session.
        let goal_completed = Arc::new(AtomicBool::new(false));
        // `run_check` is Mutating (see below), so plan-mode / read-only
        // sessions never register it — requiring fresh evidence from it would
        // make `goal_complete` permanently unreachable there. Only demand
        // evidence when the session can actually produce it.
        base.register(Arc::new(crate::goal::GoalCompleteTool::new(
            goal_completed.clone(),
            verification.clone(),
            !(plan_mode || read_only_tools),
        )));
        base.register(Arc::new(crate::verification::RunCheckTool::new(
            verification.clone(),
        )));

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

        // MCP discovery + connect kicked off HERE (network) so it overlaps the
        // skills-tree walk and LSP detection (disk) below — startup latency
        // becomes max(disk, connect) instead of their sum. The lifecycle is
        // awaited just before tools are wrapped, where its tools register.
        let mut all_mcp_servers: Vec<String> = Vec::new();
        let mcp_connect: Option<tokio::task::JoinHandle<Arc<agent::mcp::Lifecycle>>> =
            match crate::mcp::discover_mcp_config(&cwd) {
                Some(mut config) => {
                    all_mcp_servers = config.servers.keys().cloned().collect();
                    all_mcp_servers.sort();
                    config.servers.retain(|name, _| plugins.mcp_enabled(name));
                    // Plan mode filters MCP tools out anyway → skip the connect
                    // (process spawn / network).
                    if plan_mode || config.servers.is_empty() {
                        None
                    } else {
                        Some(tokio::spawn(
                            async move { crate::mcp::connect(config).await },
                        ))
                    }
                }
                None => None,
            };

        // Built-in browser control (browser_*). Disable via `tools:browser`.
        // browser_read is ReadOnly and registered un-gated, like op_read.
        // The mutating trio (act/eval/tabs) is wrapped in `browser_gated`
        // HERE, before `wrap_mutating_tools` runs below, so their approval
        // prompts carry the live target/URL (session state the model's
        // input cannot be trusted to report) instead of the generic view.
        if cfg.browser.enabled() {
            // Same fallback convention as `resolve_profile_dir` (managed.rs):
            // `$ZODE_CONFIG_DIR`/`~/.zode`, or a relative `.zode` if the home
            // directory can't be resolved.
            let shots_dir = crate::config::ConfigManager::config_dir()
                .unwrap_or_else(|_| PathBuf::from(".zode"))
                .join("screenshots");
            let deps = BrowserToolDeps {
                session: browser_session.clone(),
                shots_dir,
                target_override: browser_target_override.clone(),
            };
            base.register(Arc::new(BrowserReadTool::new(deps.clone())));
            base.register(Arc::new(
                BrowserUploadTool::new(browser_session.clone(), gate.clone())
                    .with_target_override(browser_target_override.clone()),
            ));
            for tool in [
                Arc::new(BrowserActTool::new(deps.clone())) as Arc<dyn Tool>,
                Arc::new(BrowserEvalTool::new(deps.clone())),
                Arc::new(BrowserTabsTool::new(deps)),
            ] {
                base.register(crate::browser::gate::browser_gated_as(
                    tool,
                    gate.clone(),
                    browser_session.clone(),
                    browser_target_override.clone(),
                ));
            }
        }

        // Skills: load the three-level SKILL.md tree. Disabled skills are
        // dropped from the registry + index, but the full list is kept for the
        // /plugin picker.
        // Scan the skills tree ONCE: derive the full picker list and the
        // enabled registry from a single walk (was two full walks + parses).
        let skill_dirs = skills_dirs(&cwd);
        let (all_skill_meta, skills_registry) =
            crate::skills::load_skills_meta_and_registry(&skill_dirs, |n| plugins.skill_enabled(n));
        let skills = Arc::new(skills_registry);
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
                model_runtime: model_runtime.clone(),
                skills: skills.clone(),
            };
            base.register(Arc::new(OpDesignTool::new(design_deps)));
        }

        // MCP: await the connect started above (overlapped the disk work) and
        // register a ZodeMcpTool per discovered tool — they go through the
        // approval gate.
        let mcp = match mcp_connect {
            Some(handle) => match handle.await {
                Ok(lifecycle) => {
                    for tool in crate::mcp::mcp_tools(&lifecycle) {
                        base.register(tool);
                    }
                    Some(lifecycle)
                }
                Err(e) => {
                    tracing::warn!("mcp connect task failed: {e}");
                    None
                }
            },
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
        let file_cache = carry.file_cache.clone().unwrap_or_else(|| {
            Arc::new(FileStateCache::new(
                NonZeroUsize::new(FILE_CACHE_ENTRIES).expect("nonzero"),
                FILE_CACHE_BYTES,
            ))
        });
        let history = carry.history.clone().unwrap_or_else(|| {
            Arc::new(tokio::sync::Mutex::new(EditHistory::new(
                EDIT_HISTORY_CAPACITY,
            )))
        });
        let bg_shells_meta = carry.bg_shells_meta.clone().unwrap_or_default();
        let mut hook_runner = HookRunner::new();
        // EditHistoryHook resolves paths via the same policy the fs tools use.
        hook_runner.register(Arc::new(EditHistoryHook::new(
            history.clone(),
            policy.clone(),
        )));
        hook_runner.register(Arc::new(BgShellHook::new(bg_shells_meta.clone())));
        let recent_files = carry.recent_files.clone().unwrap_or_default();
        let restore_pending = Arc::new(std::sync::atomic::AtomicBool::new(false));
        hook_runner.register(Arc::new(crate::compact_memory::compact_tracker_hook(
            recent_files.clone(),
            restore_pending.clone(),
        )));
        hook_runner.register(Arc::new(crate::verification::verification_hook(
            verification.clone(),
        )));
        hook_runner.register(Arc::new(tool_trace.hook()));
        hook_runner.register(Arc::new(reminders.hook()));
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
        let subagents = carry.subagents.clone().unwrap_or_default();
        // User-defined sub-agents (~/.zode/agents etc.), available alongside the
        // built-in types for the Task tool and listed by `/agents`.
        let agent_defs = crate::agents::load_agent_defs(&cwd);
        let task_factory = Arc::new(ZodeTaskFactory::new(
            model_runtime.clone(),
            permissions.clone(),
            cwd.clone(),
            file_cache.clone(),
            hooks.clone(),
            task_tools.clone(),
            agent_defs,
            subagents.clone(),
            resolve_subagent_max_iterations(cfg.subagent_max_iterations),
        ));
        let agent_type_list = task_factory.agent_types();
        base.register(Arc::new(TaskTool::new(task_factory)));

        // Autonomous orchestration: let the agent define new sub-agent types
        // and workflows. Default ON (unset → enabled); toggle off via Settings.
        let orchestration = cfg.autonomous_orchestration.unwrap_or(true);
        if orchestration {
            base.register(Arc::new(crate::agents::DefineAgentTool));
            base.register(Arc::new(crate::workflows::DefineWorkflowTool));
            // run_workflow drives saved JS workflows; its agent() bridge calls
            // the final gated Task through the same late-bound cell.
            base.register(Arc::new(crate::workflows::RunWorkflowTool::new(
                task_tools.clone(),
            )));
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
        let base = if plan_mode || read_only_tools {
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

        // 2. Wrap mutating/destructive tools with the approval gate. `ask`
        //    force-gates its tools even when read-only / allowed.
        // browser_* mutating tools are pre-wrapped via browser_gated() above
        // (context-aware view); list them in the allow set so wrap_mutating_tools
        // does not double-gate them behind a second, plain PermissionGatedTool.
        let mut mutating_allow = cfg.permissions.allow.clone();
        if cfg.browser.enabled() {
            for name in [
                "browser_act",
                "browser_eval",
                "browser_tabs",
                "browser_upload",
            ] {
                mutating_allow.push(name.to_string());
            }
        }
        let mut gated = wrap_mutating_tools(base, &gate, &mutating_allow, &cfg.permissions.ask);

        // 3. ToolSearch over the full set (candidates = snapshot of the
        //    gated registry, taken before ToolSearch itself is added).
        let candidates = Arc::new(gated.clone());
        gated.register(Arc::new(ToolSearchTool::new(candidates)));

        let tools = Arc::new(gated);
        // Late-bind the child sub-agent's tool set to the final gated+sandboxed
        // registry now that wrapping is complete.
        let _ = task_tools.set(tools.clone());

        // Detect the git branch off the runtime thread — `git rev-parse`
        // is a subprocess that shouldn't block a tokio worker during
        // startup (slow on a huge repo / network filesystem).
        let git_branch = {
            let cwd = cwd.clone();
            tokio::task::spawn_blocking(move || crate::instructions::detect_git_branch(&cwd))
                .await
                .ok()
                .flatten()
        };
        // Seed the drift-tracker baseline with the branch that's about to be
        // baked into the system prompt, so `check_branch_drift` only fires on
        // a REAL change observed after this prompt was rendered.
        reminders.note_git_branch(git_branch.clone());
        let has_todo_tool = tools.get("TodoWrite").is_some();
        let has_run_check_tool = tools.get("run_check").is_some();
        let system = Some(render_runtime_system_prompt(
            cfg,
            &cwd,
            date,
            &sandbox,
            plan_mode,
            has_question_tool,
            has_todo_tool,
            has_run_check_tool,
            &model,
            &skills,
            &agent_type_list,
            &workflow_defs,
            &lsp.as_ref().map(|m| m.langs()).unwrap_or_default(),
            Some(git_branch),
        ));
        // Carry the accumulated cost across a reassembly so `/cost` doesn't
        // reset to $0 on a plugin/sandbox/provider toggle.
        let cost = carry
            .cost
            .clone()
            .unwrap_or_else(|| cost_state_for_model(cfg, model.clone()));

        // Mid-turn steering channel: host → running loop.
        let (steer_tx, steer_rx) = futures::channel::mpsc::unbounded::<Vec<ContentBlock>>();
        let steer_rx = Arc::new(std::sync::Mutex::new(steer_rx));

        // Map /effort onto real provider reasoning knobs (system-prompt prose
        // stays as the fallback for providers with no such knob).
        let (thinking, reasoning_effort) = map_effort(
            cfg.effort.as_deref(),
            provider.capabilities().supports_thinking,
            cfg.provider.reasoning.unwrap_or(false),
        );

        let noema = ZodeNoema::from_settings(&cfg.noema);
        let session_store = (cfg.compact.memory_sink() && noema.is_enabled()).then(|| {
            Arc::new(crate::compact_memory::NoemaSessionStore::new(
                noema.clone(),
                cwd.clone(),
            ))
        });

        Ok(Self {
            provider,
            tools,
            permissions,
            hooks,
            store: Arc::new(Mutex::new(MessageStore::new())),
            file_cache,
            // Carry compaction latches (no-progress / failure breaker) so a
            // reassembly mid-conversation doesn't silently re-arm them.
            compact_state: carry
                .compact_state
                .clone()
                .unwrap_or_else(|| Arc::new(Mutex::new(AutoCompactState::default()))),
            steer_tx,
            steer_rx,
            model,
            system,
            cwd: cwd.clone(),
            max_output_tokens: resolve_max_output(
                &cfg.provider,
                cfg.max_output_tokens,
                active_provider_key,
                cached_catalog(),
                resolve_context_window(
                    &cfg.provider,
                    cfg.context_window,
                    active_provider_key,
                    cached_catalog(),
                ),
            ),
            model_max_tokens: resolve_context_window(
                &cfg.provider,
                cfg.context_window,
                active_provider_key,
                cached_catalog(),
            ),
            max_iterations: resolve_max_iterations(cfg.max_iterations),
            max_api_retries: resolve_max_api_retries(cfg.max_api_retries),
            temperature: cfg.temperature,
            thinking,
            reasoning_effort,
            prompt_cache: cfg.prompt_cache.unwrap_or(true),
            noema,
            extract_config: crate::noema_extract::ExtractConfig::from_settings(&cfg.noema),
            compact_settings: cfg.compact.clone(),
            session_store,
            recent_files,
            restore_pending,
            last_restore_note: Arc::new(std::sync::Mutex::new(None)),
            model_runtime,
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
            goal_completed,
            verification,
            tool_trace,
            reminders,
            auto_loop_max_turns: cfg.auto_loop_max_turns,
            browser: browser_session,
            browser_target_override,
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

    /// `(server, connected)` for every configured MCP server. Feeds both the
    /// plugin list and the sidebar MCP section (refreshed on the UI tick).
    /// `(language, client-running)` for the enabled LSP servers the project
    /// actually uses (workspace holds a matching file, or the client is
    /// running); empty otherwise — the sidebar hides the section.
    pub fn lsp_status(&self) -> Vec<(String, bool)> {
        self.lsp.as_ref().map(|m| m.status()).unwrap_or_default()
    }

    pub fn mcp_status(&self) -> Vec<(String, bool)> {
        self.all_mcp_servers
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
            .collect()
    }

    /// Run a saved JS workflow by name (the `/workflows` dialog's Enter).
    /// `agent()` calls dispatch through this engine's final gated registry, so
    /// approvals and the sidebar sub-agent view behave like model-run tasks.
    pub async fn run_workflow_named(
        &self,
        name: &str,
        args: serde_json::Value,
        log: crate::workflows_js::LogSink,
        abort: agent::abort::AbortController,
    ) -> Result<serde_json::Value, String> {
        let def = crate::workflows::load_workflow_defs(&self.cwd)
            .into_iter()
            .find(|w| w.name == name)
            .ok_or_else(|| format!("no workflow named '{name}'"))?;
        let runner = Arc::new(crate::workflows_js::GatedTaskRunner::new(
            self.tools.clone(),
            self.cwd.clone(),
            self.file_cache.clone(),
            self.permissions.clone(),
            self.hooks.clone(),
            abort,
        ));
        crate::workflows_js::run_js_workflow(&def.script, args, runner, log).await
    }

    /// Full plugin list (incl. disabled) for `/plugin` and the picker — tool
    /// groups, MCP servers (with live connection state), skills, LSP servers.
    pub fn plugin_list(&self) -> Vec<crate::plugin::Plugin> {
        let mcp_servers = self.mcp_status();
        self.plugins
            .list(&mcp_servers, &self.all_skill_meta, &self.lsp_langs)
    }

    /// Inject a pre-loaded MessageStore (for `--continue` / `--resume`).
    pub fn with_store(mut self, store: MessageStore) -> Self {
        self.store = Arc::new(Mutex::new(store));
        self
    }

    /// Pre-turn safety compaction for headless callers (`-p` on a large
    /// resumed context, long `--no-tui` sessions). The QueryLoop already
    /// auto-compacts on a byte estimate, but that under-counts CJK; when the
    /// caller has the provider-reported prompt size (`context_tokens`, the last
    /// turn's Usage), this checks the ACCURATE request budget (prompt tokens
    /// plus the configured completion budget) and compacts before the next turn
    /// so the conversation can't sail past provider `prompt + max_tokens`
    /// validation and hard-400. `None` falls back to a byte estimate of the
    /// store.
    /// Best-effort: a compaction failure is logged, not surfaced. Returns
    /// whether a compaction ran.
    pub async fn auto_compact_if_needed(&self, context_tokens: Option<u32>) -> bool {
        let window = self.model_max_tokens;
        if window == 0 {
            return false;
        }
        let tokens = context_tokens.unwrap_or_else(|| {
            self.store
                .lock()
                .map(|s| {
                    s.iter()
                        .map(agent::compact::estimate_tokens)
                        .fold(0u32, u32::saturating_add)
                })
                .unwrap_or(0)
        });
        if !pre_turn_compact_needed(tokens, window, self.max_output_tokens) {
            return false;
        }
        match self
            .compact_sized((tokens > 0).then_some(tokens), AbortController::new())
            .await
        {
            Ok(o) => {
                tracing::debug!(
                    "headless pre-turn auto-compact: {} → {} tokens",
                    o.pre_tokens,
                    o.post_tokens
                );
                true
            }
            Err(e) => {
                tracing::warn!("headless pre-turn auto-compact failed: {e}");
                false
            }
        }
    }

    /// The provider-reported prompt size of the most recent turn (input +
    /// cache-read + cache-create from the last `Usage`), or `None` if no turn
    /// has reported usage yet. Feeds [`auto_compact_if_needed`].
    pub async fn last_prompt_tokens(&self) -> Option<u32> {
        self.cost.last_prompt_tokens().await
    }

    /// Read-and-clear the goal-loop completion signal. Returns `true` exactly
    /// once per `goal_complete` call: the host polls this after each turn to
    /// decide whether the autonomous goal loop should stop.
    pub fn take_goal_completed(&self) -> bool {
        self.goal_completed.swap(false, Ordering::SeqCst)
    }

    /// Clear the completion signal before starting a fresh goal loop, so a stale
    /// flag from an earlier goal can't short-circuit the new one.
    pub fn reset_goal_completed(&self) {
        self.goal_completed.store(false, Ordering::SeqCst);
    }

    /// Optional cap on autonomous goal-loop turns (`autoLoopMaxTurns`). `None`
    /// OR `0` means unbounded — matching zode's `max_iterations` convention where
    /// 0 is "no cap" (a 0 cap that stopped after one turn would be surprising).
    pub fn auto_loop_max_turns(&self) -> Option<u32> {
        self.auto_loop_max_turns.filter(|&n| n > 0)
    }

    /// Render the conversation to a Markdown transcript (`/export`). Returns an
    /// empty header-only document if the store mutex is poisoned.
    pub fn export_markdown(&self) -> String {
        match self.store.lock() {
            Ok(store) => {
                crate::export::store_to_markdown_with_trace(&store, Some(self.tool_trace.path()))
            }
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
        self.compact_sized(None, abort).await
    }

    /// Compact with a caller-supplied figure for the LIVE context size.
    /// `context_tokens` should be the provider-reported usage (the TUI's "%
    /// ctx" badge); `None` falls back to a byte estimate of the store (which
    /// under-counts CJK). The figure picks the compaction direction — see
    /// [`compact_direction`]: a transcript already near/over the input window
    /// must NOT be sent whole, or the summarize request itself gets a 400
    /// context-overflow and compaction can never succeed.
    pub async fn compact_sized(
        &self,
        context_tokens: Option<u32>,
        abort: AbortController,
    ) -> Result<CompactOutcome, CoreError> {
        use agent::compact::{
            apply_compaction_to_store, compact_with_hooks, estimate_tokens, promote_to_store,
            CompactTrigger, CompactWithHooksRequest,
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
        let tokens = context_tokens.unwrap_or_else(|| {
            messages
                .iter()
                .map(estimate_tokens)
                .fold(0u32, u32::saturating_add)
        });
        let mut request = CompactWithHooksRequest::new(&messages, self.model.clone())
            .with_trigger(CompactTrigger::Manual)
            .with_direction(compact_direction(tokens, self.model_max_tokens))
            .with_abort(abort);
        if self.session_store.is_some() {
            request = request
                .with_custom_instructions(crate::compact_memory::COMPACT_MEMORY_INSTRUCTIONS);
        }
        let result = compact_with_hooks(self.hooks.as_ref(), self.provider.as_ref(), request)
            .await
            .map_err(|e| CoreError::Other(e.to_string()))?;
        {
            let mut store = self
                .store
                .lock()
                .map_err(|_| CoreError::Other("compact: message store poisoned".into()))?;
            apply_compaction_to_store(&mut store, &result)?;
        }
        // This compaction rewrote the store, so a previously latched
        // runtime "no progress" verdict is stale — clear it and let the
        // QueryLoop re-evaluate on the next turn (it re-latches if the
        // transcript really can't shrink).
        if let Ok(mut s) = self.compact_state.lock() {
            s.reset_no_progress();
        }
        if let Some(sm) = &self.session_store {
            if let Err(err) = promote_to_store(sm.as_ref(), &result).await {
                tracing::debug!(error = %err, "manual compact: memory sink failed");
            }
        }
        Ok(CompactOutcome {
            pre_tokens: result.pre_compact_tokens,
            post_tokens: result.post_compact_tokens,
            replaced: result.replaced_uuids.len(),
        })
    }

    /// Inject a user message into the CURRENTLY RUNNING multi-step turn
    /// (mid-turn steering). The QueryLoop drains this between round-trips and
    /// appends it as a user turn, so the model sees the new instruction on
    /// its next call without the user having to interrupt and restart. A
    /// no-op if no turn is running (the message is buffered and drained by
    /// the next turn that starts). Returns whether the send succeeded.
    pub fn steer(&self, content: Vec<ContentBlock>) -> bool {
        self.steer_tx.unbounded_send(content).is_ok()
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
        self.restore_after_compact(&query);
        self.auto_remember_noema(&query);
        let mut notices = self.reminders.pre_turn(&self.todo_state).await;
        if let Some(n) = self.check_branch_drift().await {
            notices.push(n);
        }
        let content = if notices.is_empty() {
            content
        } else {
            crate::reminders::prepend_reminder(content, &notices)
        };
        let content = self.inject_noema_memory(content, &query);
        self.turn_blocks_raw(content, abort).await
    }

    /// Detect a git-branch change since the prompt was rendered. Runs `git
    /// rev-parse` off-thread; the tracker baseline is seeded at assembly time.
    async fn check_branch_drift(&self) -> Option<String> {
        let cwd = self.cwd.clone();
        let current =
            tokio::task::spawn_blocking(move || crate::instructions::detect_git_branch(&cwd))
                .await
                .ok()?;
        self.reminders.note_git_branch(current)
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
            .max_iterations(self.max_iterations)
            .max_api_retries(self.max_api_retries)
            .cwd(self.cwd.clone())
            .auto_compact(true)
            .microcompact(self.compact_settings.microcompact())
            .use_prompt_cache(self.prompt_cache)
            .steer(self.steer_rx.clone());
        if let Some(t) = self.temperature {
            builder = builder.temperature(t);
        }
        if let Some(tc) = self.thinking {
            builder = builder.thinking(tc);
        }
        if let Some(e) = self.reasoning_effort.clone() {
            builder = builder.reasoning_effort(e);
        }
        if let Some(store) = &self.session_store {
            builder = builder
                .compact_instructions(crate::compact_memory::COMPACT_MEMORY_INSTRUCTIONS)
                .session_memory(store.clone() as Arc<dyn agent::compact::SessionMemoryStore>);
        }
        if let Some(sys) = &self.system {
            builder = builder.system(sys.clone());
        }
        builder.build().run_blocks(content, abort).await
    }

    /// One-shot post-compaction restoration: when the tracker hook latched a
    /// compaction, push a synthetic user message (recent files re-read from
    /// disk + noema recall pack) into the store BEFORE this turn's user
    /// prompt. Best-effort: any failure just skips that part.
    fn restore_after_compact(&self, query: &str) {
        use std::sync::atomic::Ordering;
        if !self.restore_pending.swap(false, Ordering::SeqCst) {
            return;
        }
        let cfg = &self.compact_settings;
        let files = if cfg.restore_files() {
            let paths = self.recent_files.top(5);
            crate::compact_memory::read_attachments(&paths, 5)
        } else {
            Vec::new()
        };
        let recall = if cfg.recall_after_compact() {
            let goal = self
                .store
                .lock()
                .ok()
                .and_then(|s| crate::compact_memory::latest_summary_goal(&s));
            let recall_query = match goal {
                Some(g) => format!("{g}\n{query}"),
                None => query.to_string(),
            };
            self.noema
                .recall_for_turn(&recall_query, Some(self.cwd.as_path()))
                .ok()
                .flatten()
        } else {
            None
        };
        let pc_config = agent::compact::PostCompactConfig {
            token_budget: cfg.restore_files_budget(),
            ..Default::default()
        };
        let Some((message, note)) =
            crate::compact_memory::build_restore_message(files, recall, &pc_config)
        else {
            return;
        };
        if let Ok(mut store) = self.store.lock() {
            if store.push(message).is_ok() {
                if let Ok(mut n) = self.last_restore_note.lock() {
                    *n = Some(note);
                }
            }
        }
    }

    /// Take (and clear) the UI note produced by the last restoration.
    pub fn take_restore_note(&self) -> Option<String> {
        self.last_restore_note
            .lock()
            .ok()
            .and_then(|mut n| n.take())
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

    /// Kick off the post-turn LLM memory-extraction pass on a detached task so
    /// it never adds turn latency. No-op unless `extract_config.enabled` and
    /// memory is on. The transcript slice is captured *synchronously here*
    /// (before the spawn) so a rapid next turn can't race the snapshot. A
    /// failure is logged, never surfaced. Call this from each front-end's
    /// turn-completion site (the engine returns the undrained event stream, so
    /// it can't self-trigger).
    pub fn spawn_post_turn_extraction(self: &Arc<Self>) {
        let Some(slice) = self.capture_extraction_slice() else {
            return;
        };
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            engine.extract_from_slice(slice).await;
        });
    }

    /// Inline (await) variant for callers that own the engine by value and run
    /// turns sequentially (the headless `-p` / REPL paths). Awaits the pass so
    /// the process doesn't exit before the memory write lands.
    pub async fn extract_post_turn_inline(&self) {
        let Some(slice) = self.capture_extraction_slice() else {
            return;
        };
        self.extract_from_slice(slice).await;
    }

    /// Snapshot the transcript and build the extractor input *synchronously*.
    /// Returns `None` when extraction is disabled, memory is off, or there is
    /// nothing to extract (empty user text). Holds the store's std mutex only
    /// for the snapshot — no `.await` while locked, and the snapshot is taken
    /// before any detached task so it reflects the turn that just completed.
    fn capture_extraction_slice(&self) -> Option<String> {
        if !self.extract_config.enabled || !self.noema.is_enabled() {
            return None;
        }
        let cfg = &self.extract_config;
        let (user_text, assistant_text) = {
            let store = self.store.lock().ok()?;
            last_user_assistant_text(&store)
        };
        crate::noema_extract::build_transcript_slice(
            &user_text?,
            assistant_text.as_deref(),
            cfg.scan_assistant,
            cfg.max_input_chars,
        )
    }

    /// The extraction body: run the cheap LLM pass over an already-captured
    /// transcript slice, parse candidates, and submit them through noema's
    /// governance. Best-effort — every failure path logs and returns.
    ///
    /// Uses the tracked one-shot so the extraction call retries transient
    /// failures and its token usage folds into `self.cost` (no more `/cost`
    /// undercount for the per-turn extraction when `autoExtract` is on).
    async fn extract_from_slice(&self, slice: String) {
        let cfg = &self.extract_config;
        let model = cfg.model.clone().unwrap_or_else(|| self.model.clone());
        let abort = AbortController::new();
        let raw = match crate::openpencil::design::llm_oneshot_tracked(
            &self.provider,
            &model,
            crate::noema_extract::EXTRACT_SYSTEM_PROMPT,
            &slice,
            &abort,
            Some(&self.cost),
        )
        .await
        {
            Ok(text) => text,
            Err(err) => {
                tracing::debug!(error = %err, "memory extraction call failed");
                return;
            }
        };

        let parsed = crate::noema_extract::parse_extraction(&raw, cfg.max_memories_per_turn);
        let items = parsed.items;
        if items.is_empty() {
            return;
        }
        #[cfg(feature = "noema")]
        {
            let outcomes = self
                .noema
                .submit_extracted(&items, Some(self.cwd.as_path()));
            let stored = outcomes.iter().filter(|o| o.is_ok()).count();
            tracing::debug!(
                candidates = items.len(),
                submitted = stored,
                degradation_count = parsed.degradation_count,
                "post-turn memory extraction complete"
            );
        }
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

/// Pull the text of the most recent user message and the most recent assistant
/// message from the store, for post-turn memory extraction. Either may be
/// `None` (e.g. a tool-only turn). Scans from the tail so it reflects the turn
/// that just completed.
fn last_user_assistant_text(store: &MessageStore) -> (Option<String>, Option<String>) {
    use agent::message::Message;
    let mut user = None;
    let mut assistant = None;
    let messages: Vec<&Message> = store.iter().collect();
    for msg in messages.into_iter().rev() {
        match msg {
            Message::User { content, .. } if user.is_none() => {
                let text = text_query(content);
                if !text.trim().is_empty() {
                    user = Some(text);
                }
            }
            Message::Assistant { content, .. } if assistant.is_none() => {
                let text = text_query(content);
                if !text.trim().is_empty() {
                    assistant = Some(text);
                }
            }
            _ => {}
        }
        if user.is_some() && assistant.is_some() {
            break;
        }
    }
    (user, assistant)
}

/// Access policy for tools exposed by an engine assembled for one task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolAccessMode {
    ReadOnly,
    Prompt,
    Auto,
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
    /// Exact named-provider selection. The resolved active `ProviderConfig`
    /// alone cannot recover this when two groups expose the same model through
    /// otherwise identical endpoint settings.
    selected_provider_key: Option<String>,
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
    /// Restrict the registry to read-only tools without enabling plan mode or
    /// changing the system prompt.
    read_only_tools: bool,
    sandbox: Option<crate::sandbox::SandboxConfig>,
    date: String,
    /// Process-wide browser session, shared by every tab assembled from this
    /// template — one browser process per zode run, not one per tab.
    pub browser: Arc<BrowserSession>,
    /// Pin the `browser_*` tools of engines assembled from this template to
    /// an explicit target. Extension-task templates pin `Bridge` so
    /// side-panel turns drive the page beside the panel, regardless of the
    /// session-wide `/browser target` selection.
    browser_target_override: Option<BrowserTarget>,
}

fn provider_key_owns_model(cfg: &ZodeConfig, key: &str, model: &str) -> bool {
    cfg.providers.get(key).is_some_and(|entry| {
        entry.models.contains_key(model) || entry.model.as_deref() == Some(model) || key == model
    })
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
        let browser = BrowserSession::new(cfg.browser.clone(), Arc::new(ManagedFactory));
        let selected_provider_key = cfg.active_provider_key().map(str::to_string);
        Self {
            cfg,
            selected_provider_key,
            cwd,
            queue,
            question_queue: None,
            yolo,
            plan_mode: false,
            read_only_tools: false,
            sandbox,
            date,
            browser,
            browser_target_override: None,
        }
    }

    /// Pin engines assembled from this template to an explicit browser
    /// target (see the field doc). Returns the template for chaining.
    pub fn with_browser_target_override(mut self, target: Option<BrowserTarget>) -> Self {
        self.browser_target_override = target;
        self
    }

    /// Wire the interactive question channel (TUI). Carried across reassembly
    /// clones, so `AskUserQuestion` survives provider/model/plugin swaps.
    pub fn with_question_queue(mut self, queue: Option<crate::question::QuestionQueue>) -> Self {
        self.question_queue = queue;
        self
    }

    /// Clone this template with a new base configuration for engines assembled
    /// later, preserving its runtime queues, sandbox, date, and mode flags.
    pub fn with_config(&self, cfg: ZodeConfig) -> Self {
        let mut template = self.clone();
        template.browser = BrowserSession::new(cfg.browser.clone(), Arc::new(ManagedFactory));
        // `with_config` has no explicit provider-key argument. Preserve the
        // current exact selection while it still owns the replacement config's
        // active model; otherwise derive the best available fallback.
        template.selected_provider_key = cfg
            .provider
            .model
            .as_deref()
            .and_then(|model| {
                self.selected_provider_key
                    .as_deref()
                    .filter(|key| {
                        provider_key_owns_model(&cfg, key, model)
                            && cfg
                                .resolve_named_provider_model(key, model)
                                .is_some_and(|resolved| resolved == cfg.provider)
                    })
                    .map(str::to_string)
            })
            .or_else(|| cfg.active_provider_key().map(str::to_string));
        template.cfg = cfg;
        template
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
        self.assemble_tab_with_carry(cwd_override, label, CarryState::default())
            .await
    }

    /// Like [`assemble_tab`] but carrying long-lived session state (cost,
    /// history, bg shells, todos, sub-agents, compaction latches, file
    /// cache) into the rebuilt engine — used by reassembly so a hot-swap
    /// preserves them. See [`ZodeEngine::assemble_with_carry`].
    pub async fn assemble_tab_with_carry(
        &self,
        cwd_override: Option<PathBuf>,
        label: Option<String>,
        carry: CarryState,
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
        ZodeEngine::assemble_with_carry_and_access(
            &self.cfg,
            self.provider_scope(),
            cwd,
            gate,
            sandbox,
            &self.date,
            question_tool,
            op_consent,
            self.plan_mode,
            self.read_only_tools,
            Some(self.browser.clone()),
            self.browser_target_override.clone(),
            carry,
        )
        .await
    }

    pub fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    /// Bind approval requests from `source` to the active turn. Cloned and
    /// reconfigured templates share the queue's registry.
    pub fn bind_approval_turn(&self, source: &str, turn_id: u64) {
        if let Some(queue) = &self.queue {
            queue.bind_turn(source, turn_id);
        }
    }

    /// Bind approval requests from `source` to a local operation. This is a
    /// distinct owner domain from agent turns even when their ids are equal.
    pub fn bind_approval_local_operation(&self, source: &str, local_op_id: u64) {
        if let Some(queue) = &self.queue {
            queue.bind_local_operation(source, local_op_id);
        }
    }

    /// Clear an approval binding only if it still belongs to `expected`.
    pub fn clear_approval_turn_if(&self, source: &str, expected: u64) {
        if let Some(queue) = &self.queue {
            queue.clear_turn_if(source, expected);
        }
    }

    /// Clear a local-operation approval binding only for the exact generation.
    pub fn clear_approval_local_operation_if(&self, source: &str, expected: u64) {
        if let Some(queue) = &self.queue {
            queue.clear_local_operation_if(source, expected);
        }
    }

    /// Remove approval turn ownership for a source that no longer exists.
    pub fn remove_approval_source(&self, source: &str) {
        if let Some(queue) = &self.queue {
            queue.remove_source(source);
        }
    }

    pub fn model(&self) -> Option<&str> {
        self.cfg.provider.model.as_deref()
    }

    fn provider_scope(&self) -> Option<&str> {
        self.selected_provider_key
            .as_deref()
            .filter(|key| {
                self.cfg
                    .provider
                    .model
                    .as_deref()
                    .is_some_and(|model| provider_key_owns_model(&self.cfg, key, model))
            })
            .or_else(|| self.cfg.active_provider_key())
    }

    pub fn images(&self) -> &crate::config::ImagesConfig {
        &self.cfg.images
    }

    pub fn provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.cfg.providers.keys().cloned().collect();
        names.sort();
        names
    }

    /// Named providers that can actually accept image input. For multi-model
    /// provider groups, any image-capable model makes the provider eligible;
    /// the vision path will select that model instead of blindly taking the
    /// group's first (often text-only) model.
    pub fn vision_provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .cfg
            .providers
            .keys()
            .filter(|name| self.resolve_vision_provider(name).is_some())
            .cloned()
            .collect();
        names.sort();
        names
    }

    pub fn model_ids(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(model) = self.cfg.provider.model.as_deref() {
            out.push(model.to_string());
        }

        // Every provider entry contributes its default `model` plus any models
        // listed under its `models` map (multi-model providers).
        let mut provider_models: Vec<String> = Vec::new();
        for p in self.cfg.providers.values() {
            if let Some(model) = p.model.as_deref() {
                provider_models.push(model.to_string());
            }
            provider_models.extend(p.models.keys().cloned());
        }
        provider_models.retain(|model| !out.iter().any(|existing| existing == model));
        provider_models.sort();
        provider_models.dedup();
        out.extend(provider_models);
        out
    }

    /// Read the resolved active provider (test-only assertion helper).
    #[cfg(test)]
    pub(crate) fn active_provider(&self) -> &crate::config::ProviderConfig {
        &self.cfg.provider
    }

    pub fn yolo(&self) -> bool {
        self.yolo
    }

    pub fn plan_mode(&self) -> bool {
        self.plan_mode
    }

    pub fn tool_access(&self) -> ToolAccessMode {
        if self.read_only_tools {
            ToolAccessMode::ReadOnly
        } else if self.yolo {
            ToolAccessMode::Auto
        } else {
            ToolAccessMode::Prompt
        }
    }

    /// Clone with a task-local tool access policy. Read-only access filters
    /// the registry independently from plan mode; prompt and auto select the
    /// existing queue and bypass gate paths respectively.
    pub fn with_tool_access(&self, access: ToolAccessMode) -> Self {
        let mut t = self.clone();
        t.read_only_tools = matches!(access, ToolAccessMode::ReadOnly);
        t.yolo = matches!(access, ToolAccessMode::Auto);
        t
    }

    /// Clone with plan mode toggled (for `/plan`). Read-only tools only + a
    /// plan-mode system prompt; carried across reassembly clones.
    pub fn with_plan_mode(&self, plan_mode: bool) -> Self {
        let mut t = self.clone();
        t.plan_mode = plan_mode;
        t
    }

    /// Clone with the model overridden (for `/model <id>`). When the model
    /// belongs to a configured provider (its map key, its `model`, or its
    /// `models` map), the active provider adopts that provider's shared
    /// credentials and the per-model overrides — so switching to another
    /// provider's model "just works" without re-entering the API key. An
    /// unknown model is simply set on the current active provider.
    pub fn with_model(&self, model: String) -> Self {
        let mut t = self.clone();
        // When several provider groups expose the same wire model id, keep an
        // explicitly selected provider instead of letting the generic lookup
        // rebound the model to the first group. This is especially important
        // for provider-scoped catalog capabilities (one endpoint may support
        // image input while another does not).
        let preferred_key = t
            .selected_provider_key
            .clone()
            .or_else(|| t.cfg.active_provider_key().map(str::to_string))
            .filter(|key| provider_key_owns_model(&t.cfg, key, &model));
        if let Some((key, resolved)) = preferred_key.and_then(|key| {
            t.cfg
                .resolve_named_provider_model(&key, &model)
                .map(|provider| (key, provider))
        }) {
            t.cfg.provider = resolved;
            t.selected_provider_key = Some(key);
        } else if let Some(resolved) = t.cfg.resolve_model_provider(&model) {
            t.cfg.provider = resolved;
            t.selected_provider_key = t.cfg.active_provider_key().map(str::to_string);
        } else {
            // Unknown model on the current provider's credentials: drop the
            // previous model's per-model overrides so the new model resolves
            // its own context window / output cap / prices instead of
            // inheriting stale values (the status-bar % ctx denominator in
            // particular must follow the new model's real max context).
            t.cfg.provider.clear_model_overrides();
            t.cfg.provider.model = Some(model);
            t.selected_provider_key = t.cfg.active_provider_key().map(str::to_string);
        }
        t
    }

    /// Clone with yolo toggled (for `/yolo` and the settings mode switch).
    pub fn with_yolo(&self, yolo: bool) -> Self {
        let mut t = self.clone();
        t.yolo = yolo;
        // `/yolo` is the legacy normal-tool access switch. Clear a previous
        // task-local read-only override so the result remains one of the three
        // explicit access modes instead of an unrepresentable read-only+yolo
        // hybrid. Plan mode remains independent and can still filter tools.
        t.read_only_tools = false;
        t
    }

    /// The current sandbox config (for the `/sandbox` command to show + toggle).
    /// The persisted `sandbox` config section. Runtime `/sandbox` toggles use
    /// it to rebuild a config with the SAME writable roots / temp policy /
    /// strict-read as startup, instead of bare defaults.
    pub fn sandbox_settings(&self) -> &crate::config::SandboxSettings {
        &self.cfg.sandbox
    }

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
    /// `None` if the name isn't in `cfg.providers`. A multi-model provider (no
    /// top-level `model`, several under `models`) defaults to its first listed
    /// model with that model's override applied, and the active provider never
    /// carries the `models` map.
    pub fn with_provider(&self, name: &str) -> Option<Self> {
        let provider = self.cfg.resolve_named_provider(name)?;
        let mut t = self.clone();
        t.cfg.provider = provider;
        t.selected_provider_key = Some(name.to_string());
        Some(t)
    }

    /// Select a named provider for image description. When the provider groups
    /// several models, prefer the first model whose explicit/config-catalog
    /// capability accepts images. If none qualifies, retain the normal default
    /// so the existing submission-time check can return the precise
    /// "does not declare image support" error for stale/manual configs.
    pub fn with_vision_provider(&self, name: &str) -> Option<Self> {
        let mut t = self.with_provider(name)?;
        if let Some(provider) = self.resolve_vision_provider(name) {
            t.cfg.provider = provider;
        } else if t.model().is_none() {
            return None;
        }
        Some(t)
    }

    fn resolve_vision_provider(&self, name: &str) -> Option<crate::config::ProviderConfig> {
        self.resolve_vision_provider_with_catalog(name, cached_catalog())
    }

    fn resolve_vision_provider_with_catalog(
        &self,
        name: &str,
        catalog: &crate::Catalog,
    ) -> Option<crate::config::ProviderConfig> {
        let entry = self.cfg.providers.get(name)?;
        let mut models = Vec::new();
        if let Some(model) = entry.model.clone() {
            models.push(model);
        }
        for model in entry.models.keys() {
            if !models.contains(model) {
                models.push(model.clone());
            }
        }

        if models.is_empty() {
            // Image description still needs a concrete model id. A provider
            // transport may advertise image support by default (Anthropic in
            // particular), but assembling it without a model can only fail.
            return None;
        }

        models.into_iter().find_map(|model| {
            let provider = self.cfg.resolve_named_provider_model(name, &model)?;
            effective_provider_supports_images(&provider, Some(name), catalog).then_some(provider)
        })
    }

    /// The `providers`-map key that owns the active model — its `models` map
    /// contains it, its `model` equals it, or it is keyed by it. `None` when the
    /// active model isn't part of any configured group. Used to display
    /// "model(provider)" in the status bar.
    pub fn active_provider_name(&self) -> Option<String> {
        self.provider_scope().map(str::to_string)
    }

    /// Clone with the `providers` map replaced — keeps `active_provider_name`
    /// accurate after `/connect` adds a new group during the session.
    pub fn with_providers_map(
        &self,
        providers: indexmap::IndexMap<String, crate::config::ProviderConfig>,
    ) -> Self {
        let mut t = self.clone();
        t.cfg.providers = providers;
        t.selected_provider_key = t
            .cfg
            .provider
            .model
            .as_deref()
            .and_then(|model| {
                self.selected_provider_key
                    .as_deref()
                    .filter(|key| {
                        provider_key_owns_model(&t.cfg, key, model)
                            && t.cfg
                                .resolve_named_provider_model(key, model)
                                .is_some_and(|resolved| resolved == t.cfg.provider)
                    })
                    .map(str::to_string)
            })
            .or_else(|| t.cfg.active_provider_key().map(str::to_string));
        t
    }

    /// Variant used by `/connect`, which knows the exact group key before the
    /// freshly saved providers map is installed on the template.
    pub fn with_provider_config_for_key(
        &self,
        provider: crate::config::ProviderConfig,
        provider_key: String,
    ) -> Self {
        let mut t = self.clone();
        t.cfg.provider = provider;
        t.selected_provider_key = Some(provider_key);
        t
    }

    /// Clone with the active provider replaced by a complete provider config
    /// (for `/connect`, which writes a fresh provider into the global config).
    pub fn with_provider_config(&self, provider: crate::config::ProviderConfig) -> Self {
        let mut t = self.clone();
        t.cfg.provider = provider;
        t.selected_provider_key = t
            .cfg
            .provider
            .model
            .as_deref()
            .and_then(|model| {
                self.selected_provider_key
                    .as_deref()
                    .filter(|key| {
                        provider_key_owns_model(&t.cfg, key, model)
                            && t.cfg
                                .resolve_named_provider_model(key, model)
                                .is_some_and(|resolved| resolved == t.cfg.provider)
                    })
                    .map(str::to_string)
            })
            .or_else(|| t.cfg.active_provider_key().map(str::to_string));
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

    /// Apply a model change to an idle engine without rebuilding tools, MCP, or
    /// LSP state. This is the fast path for `/model`: the main conversation
    /// uses the new provider/model immediately, while tool topology remains the
    /// same until a setting that actually changes tools triggers reassembly.
    pub fn hot_swap_model(
        &self,
        engine: &mut ZodeEngine,
        model: String,
    ) -> Result<Self, CoreError> {
        let template = self.with_model(model);
        let active_provider_key = template.provider_scope();
        let provider_cfg = resolve_provider_capabilities(
            &template.cfg.provider,
            active_provider_key,
            cached_catalog(),
        );
        let provider = build_provider(&provider_cfg)?;
        let model = template
            .cfg
            .provider
            .model
            .clone()
            .ok_or_else(|| CoreError::Other("no model set in config".into()))?;
        let context_window = resolve_context_window(
            &template.cfg.provider,
            template.cfg.context_window,
            active_provider_key,
            cached_catalog(),
        );
        let max_output_tokens = resolve_max_output(
            &template.cfg.provider,
            template.cfg.max_output_tokens,
            active_provider_key,
            cached_catalog(),
            context_window,
        );
        let system = template.runtime_system_for_engine(engine, &model);
        // Re-derive the /effort knobs for the NEW provider — same inputs and
        // same precedence as `assemble` (see `map_effort`). Without this the
        // engine keeps whatever thinking/reasoning_effort the OLD provider
        // resolved to, which is wrong (and unsafe) once the provider kind
        // changes: e.g. swapping from an Anthropic-capable provider to an
        // OpenAI-compat one must drop a stale thinking budget that the new
        // provider would never accept.
        let (thinking, reasoning_effort) = map_effort(
            template.cfg.effort.as_deref(),
            provider.capabilities().supports_thinking,
            template.cfg.provider.reasoning.unwrap_or(false),
        );

        engine.provider = provider;
        engine.model = model.clone();
        engine.model_max_tokens = context_window;
        engine.max_output_tokens = max_output_tokens;
        engine
            .model_runtime
            .update(engine.provider.clone(), model.clone());
        // Retarget cost to the new model IN PLACE so the accumulated session
        // total survives the swap (was: replaced with a fresh $0 tracker).
        engine.cost.set_model(model);
        engine.system = Some(system);
        engine.thinking = thinking;
        engine.reasoning_effort = reasoning_effort;
        Ok(template)
    }

    /// Apply a goal change by rebuilding only the system prompt for the existing
    /// engine. The goal tool is always registered, so no tool reassembly is
    /// needed for `/goal`.
    pub fn hot_swap_goal(&self, engine: &mut ZodeEngine, goal: Option<String>) -> Self {
        let template = self.with_goal(goal);
        engine.system = Some(template.runtime_system_for_engine(engine, &engine.model));
        template
    }

    fn runtime_system_for_engine(&self, engine: &ZodeEngine, model: &str) -> String {
        let sandbox = self
            .sandbox
            .as_ref()
            .map(|sb| sb.clone().with_cwd(&engine.cwd));
        let workflow_defs = crate::workflows::load_workflow_defs(&engine.cwd);
        // Sync hot-swap path (user-triggered, off the startup critical path) —
        // detect the branch inline, then re-seed the drift tracker so its
        // baseline matches what's about to be baked into this fresh prompt.
        // `engine.reminders` is carried across the hot-swap (not rebuilt), so
        // without re-seeding, a branch change already reflected in this new
        // prompt would still fire a stale/false drift notice on the next turn.
        let git_branch = crate::instructions::detect_git_branch(&engine.cwd);
        engine.reminders.note_git_branch(git_branch.clone());
        render_runtime_system_prompt(
            &self.cfg,
            &engine.cwd,
            &self.date,
            &sandbox,
            self.plan_mode,
            self.question_queue.is_some(),
            engine.tools.get("TodoWrite").is_some(),
            engine.tools.get("run_check").is_some(),
            model,
            &engine.skills,
            &engine.agent_types,
            &workflow_defs,
            &engine.lsp.as_ref().map(|m| m.langs()).unwrap_or_default(),
            Some(git_branch),
        )
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

    /// Whether the TUI captures mouse events (`mouseCapture` in config).
    /// Default ON (wheel scroll + in-app selection); `false` hands the mouse
    /// back to the terminal for native selection + its own ⌘C.
    pub fn mouse_capture(&self) -> bool {
        self.cfg.mouse_capture_enabled()
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
    ask: &[String],
) -> ToolRegistry {
    let mut out = ToolRegistry::new();
    for tool in src.list() {
        // browser_upload performs canonical-path preflight before its own
        // per-call approval and must never be wrapped by a gate outside that
        // validation boundary.
        if tool.name() == "browser_upload" {
            out.register(tool);
            continue;
        }
        // `ask` wins over everything: a tool the user explicitly wants to be
        // prompted on is gated even if it's read-only or in `allow`. This is
        // how a user forces confirmation on a normally auto-allowed tool or
        // overrides a broad allow rule.
        let force_ask = ask.iter().any(|a| a == tool.name());
        let auto_allowed = !force_ask && allow.iter().any(|a| a == tool.name());
        let read_only = matches!(tool.safety_class(), SafetyClass::ReadOnly);
        if !force_ask && (read_only || auto_allowed) {
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
    use crate::approval::{Approval, ApprovalGate, BypassGate};
    use crate::config::{ProviderConfig, ProviderKind, ZodeConfig};

    #[derive(Debug)]
    struct DenyGate;
    #[async_trait::async_trait]
    impl ApprovalGate for DenyGate {
        async fn approve(&self, _tool: &str, _input: &serde_json::Value) -> Approval {
            Approval::Deny
        }
    }

    #[derive(Debug)]
    struct RoTool(&'static str);
    #[async_trait::async_trait]
    impl Tool for RoTool {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            "read-only test tool"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        fn safety_class(&self) -> SafetyClass {
            SafetyClass::ReadOnly
        }
        async fn call(
            &self,
            _ctx: &agent::tool::ToolUseContext,
            _input: serde_json::Value,
        ) -> Result<serde_json::Value, agent::error::AgentError> {
            Ok(serde_json::json!({"ran": true}))
        }
    }

    #[tokio::test]
    async fn ask_forces_gate_on_a_read_only_tool() {
        // A read-only tool would normally bypass the gate. Listing it in
        // `ask` must force it through — proven by a deny gate turning its
        // call into an error, while a sibling read-only tool NOT in `ask`
        // still runs.
        let gate: Arc<dyn ApprovalGate> = Arc::new(DenyGate);
        let mut src = ToolRegistry::new();
        src.register(Arc::new(RoTool("Peek")));
        src.register(Arc::new(RoTool("Glance")));
        let out = wrap_mutating_tools(src, &gate, &[], &["Peek".to_string()]);
        let ctx = agent::tool::ToolUseContext::new(std::env::temp_dir());

        let peek = out.get("Peek").unwrap();
        assert!(
            peek.call(&ctx, serde_json::json!({})).await.is_err(),
            "ask-listed read-only tool must be gated (deny → error)"
        );
        let glance = out.get("Glance").unwrap();
        assert!(
            glance.call(&ctx, serde_json::json!({})).await.is_ok(),
            "a read-only tool not in ask still bypasses the gate"
        );
    }

    #[tokio::test]
    async fn ask_overrides_allow() {
        // `ask` beats `allow`: a tool in both is still prompted.
        let gate: Arc<dyn ApprovalGate> = Arc::new(DenyGate);
        let mut src = ToolRegistry::new();
        src.register(Arc::new(RoTool("Edit")));
        let out = wrap_mutating_tools(src, &gate, &["Edit".to_string()], &["Edit".to_string()]);
        let ctx = agent::tool::ToolUseContext::new(std::env::temp_dir());
        assert!(
            out.get("Edit")
                .unwrap()
                .call(&ctx, serde_json::json!({}))
                .await
                .is_err(),
            "ask must override allow"
        );
    }

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
            // The canonical escape flag is advertised (not the legacy boolean),
            // and a blocked write must steer the model to the escalation prompt
            // rather than to retries / shell workarounds.
            assert!(note.contains("require_escalated"), "{note}");
            assert!(!note.contains("dangerouslyDisableSandbox"), "{note}");
            assert!(note.contains("do NOT retry"), "{note}");
            assert!(note.contains("DENIED"), "network denied by default: {note}");
        }
    }

    #[test]
    fn sandbox_prompt_note_names_tmp_only_when_writable() {
        use crate::sandbox::{SandboxConfig, SandboxMode};
        let Ok(ww) = SandboxConfig::new(
            std::path::Path::new("/x"),
            SandboxMode::WorkspaceWrite,
            false,
            &[],
        ) else {
            return; // unsupported OS
        };
        // /tmp is writable by default — the model must be told exactly that.
        let note = sandbox_prompt_note(&Some(ww.clone()));
        assert!(note.contains("/tmp"), "{note}");
        // Excluded → the note must not advertise /tmp as writable.
        let note = sandbox_prompt_note(&Some(ww.with_temp_policy(true, true)));
        assert!(!note.contains("+ /tmp"), "{note}");
    }

    #[test]
    fn effort_maps_to_thinking_and_reasoning_effort() {
        use crate::engine::map_effort;
        // Anthropic-style provider: high effort enables a thinking budget,
        // and the effort string now rides along too so the vendor adaptive
        // path can emit output_config.effort.
        let (thinking, effort) = map_effort(Some("high"), true, false);
        assert_eq!(thinking.map(|t| t.max_tokens), Some(8192));
        assert_eq!(effort.as_deref(), Some("high"));
        // OpenAI-compat with reasoning opt-in: effort string is forwarded.
        let (thinking, effort) = map_effort(Some("low"), false, true);
        assert!(thinking.is_none());
        assert_eq!(effort.as_deref(), Some("low"));
        // OpenAI-compat opt-in now also forwards an explicit "medium".
        assert_eq!(
            map_effort(Some("medium"), false, true).1.as_deref(),
            Some("medium")
        );
        // medium without opt-in still maps to nothing (pure prose fallback).
        let (t, e) = map_effort(Some("medium"), true, false);
        assert!(t.is_none() && e.is_none());
        let (t, e) = map_effort(None, true, true);
        assert!(t.is_none() && e.is_none());
        let (t, e) = map_effort(Some("high"), false, false);
        assert!(t.is_none() && e.is_none());
        // High + opt-in on an OpenAI-compat provider forwards "high".
        assert_eq!(
            map_effort(Some("high"), false, true).1.as_deref(),
            Some("high")
        );
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
        let model_runtime = ModelRuntimeState::new(provider.clone(), "mock-model".into());
        ZodeEngine {
            provider,
            tools: Arc::new(ToolRegistry::new()),
            permissions: Arc::new(PermissionManager::new().with_mode(PermissionMode::Bypass)),
            hooks: Arc::new(HookRunner::new()),
            store: Arc::new(Mutex::new(MessageStore::new())),
            browser_target_override: None,
            file_cache: Arc::new(FileStateCache::new(
                NonZeroUsize::new(1).expect("nonzero"),
                1024,
            )),
            compact_state: Arc::new(Mutex::new(AutoCompactState::default())),
            steer_tx: {
                let (tx, _rx) = futures::channel::mpsc::unbounded();
                tx
            },
            steer_rx: Arc::new(std::sync::Mutex::new(futures::channel::mpsc::unbounded().1)),
            model: "mock-model".into(),
            system: None,
            cwd: PathBuf::from("."),
            max_output_tokens: 128,
            model_max_tokens: DEFAULT_MODEL_MAX_TOKENS,
            max_iterations: usize::MAX,
            goal_completed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            verification: crate::verification::VerificationState::default(),
            tool_trace: crate::tool_trace::ToolTrace::with_path(
                std::env::temp_dir().join("zode-test-tool-trace.jsonl"),
            ),
            reminders: crate::reminders::ReminderTracker::default(),
            auto_loop_max_turns: None,
            max_api_retries: 10,
            temperature: None,
            thinking: None,
            reasoning_effort: None,
            prompt_cache: false,
            noema: ZodeNoema::disabled(),
            extract_config: crate::noema_extract::ExtractConfig::default(),
            compact_settings: crate::config::CompactSettings::default(),
            session_store: None,
            recent_files: crate::compact_memory::RecentFiles::default(),
            restore_pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_restore_note: Arc::new(std::sync::Mutex::new(None)),
            model_runtime,
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
            browser: BrowserSession::new(
                crate::config::BrowserConfig::default(),
                Arc::new(ManagedFactory),
            ),
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

    #[tokio::test]
    async fn template_clones_share_approval_turn_bindings() {
        let (queue, mut approvals) = crate::approval::approval_queue();
        let template = EngineTemplate::new(
            test_cfg(),
            std::path::PathBuf::from("/tmp/zode"),
            Some(queue.clone()),
            false,
            None,
            "2026-06-14".into(),
        );
        let rebuilt = template.with_config(test_cfg());

        template.bind_approval_turn("tab-7", 41);
        rebuilt.bind_approval_turn("tab-7", 42);
        template.clear_approval_turn_if("tab-7", 41);

        let pending = tokio::spawn(async move {
            queue
                .request("Bash", &serde_json::json!({}), Some("tab-7".into()))
                .await
        });
        let request = approvals.next().await.expect("request should be queued");
        assert_eq!(request.turn_id, Some(42));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn template_clones_share_typed_local_approval_bindings() {
        let (queue, mut approvals) = crate::approval::approval_queue();
        let template = EngineTemplate::new(
            test_cfg(),
            std::path::PathBuf::from("/tmp/zode"),
            Some(queue.clone()),
            false,
            None,
            "2026-06-14".into(),
        );
        let rebuilt = template.with_config(test_cfg());

        template.bind_approval_local_operation("tab-7", 41);
        rebuilt.bind_approval_local_operation("tab-7", 42);
        template.clear_approval_local_operation_if("tab-7", 41);

        let pending = tokio::spawn(async move {
            queue
                .request("Bash", &serde_json::json!({}), Some("tab-7".into()))
                .await
        });
        let request = approvals.next().await.expect("request should be queued");
        assert_eq!(request.turn_id, None);
        assert_eq!(request.local_op_id, Some(42));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn template_remove_approval_source_clears_shared_binding() {
        let (queue, mut approvals) = crate::approval::approval_queue();
        let template = EngineTemplate::new(
            test_cfg(),
            std::path::PathBuf::from("/tmp/zode"),
            Some(queue.clone()),
            false,
            None,
            "2026-06-14".into(),
        );
        let clone = template.clone();
        template.bind_approval_turn("tab-7", 41);
        clone.remove_approval_source("tab-7");

        let pending = tokio::spawn(async move {
            queue
                .request("Bash", &serde_json::json!({}), Some("tab-7".into()))
                .await
        });
        let request = approvals.next().await.expect("request should be queued");
        assert_eq!(request.turn_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
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

    fn deepseek_multi_model_cfg() -> ZodeConfig {
        let mut cfg = test_cfg();
        let mut models = indexmap::IndexMap::new();
        models.insert(
            "deepseek-v4-pro".to_string(),
            crate::config::ModelOverride {
                context_window: Some(1_000_000),
                ..Default::default()
            },
        );
        models.insert(
            "deepseek-v4-flash".to_string(),
            crate::config::ModelOverride::default(),
        );
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-deepseek".into()),
                base_url: Some("https://api.deepseek.com/anthropic".into()),
                models,
                ..Default::default()
            },
        );
        cfg
    }

    #[test]
    fn active_provider_name_finds_owning_group() {
        // Active model not owned by any group → None.
        let template = EngineTemplate::new(
            deepseek_multi_model_cfg(),
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-06-14".into(),
        );
        assert_eq!(template.active_provider_name(), None); // MiniMax-M1 unowned

        // Active model inside the deepseek group's `models` map → "deepseek".
        let mut cfg = deepseek_multi_model_cfg();
        cfg.provider.model = Some("deepseek-v4-pro".into());
        let template = EngineTemplate::new(
            cfg,
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-06-14".into(),
        );
        assert_eq!(template.active_provider_name().as_deref(), Some("deepseek"));
    }

    #[test]
    fn vision_provider_selects_image_model_in_multi_model_group() {
        let mut cfg = test_cfg();
        let mut qwen_models = indexmap::IndexMap::new();
        // Deliberately put the text-only model first: ordinary provider
        // selection keeps this default, while vision selection must skip it.
        qwen_models.insert(
            "qwen3-coder-plus".to_string(),
            crate::config::ModelOverride::default(),
        );
        qwen_models.insert(
            "qwen3-vl-plus".to_string(),
            crate::config::ModelOverride::default(),
        );
        cfg.providers.insert(
            "qwen".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Openai),
                api_key: Some("sk-qwen".into()),
                base_url: Some("https://dashscope.example/v1".into()),
                models: qwen_models,
                ..Default::default()
            },
        );
        cfg.providers.insert(
            "text-only".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Openai),
                api_key: Some("sk-text".into()),
                model: Some("qwen3-coder-plus".into()),
                ..Default::default()
            },
        );
        cfg.providers.insert(
            "missing-model".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-no-model".into()),
                supports_images: Some(true),
                ..Default::default()
            },
        );
        let template = EngineTemplate::new(
            cfg,
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-07-15".into(),
        );

        assert_eq!(
            template.with_provider("qwen").unwrap().model(),
            Some("qwen3-coder-plus")
        );
        assert_eq!(
            template.with_vision_provider("qwen").unwrap().model(),
            Some("qwen3-vl-plus")
        );
        assert_eq!(template.vision_provider_names(), vec!["qwen".to_string()]);
        assert!(template.with_vision_provider("missing-model").is_none());
        assert!(template.resolve_vision_provider("missing-model").is_none());
    }

    fn duplicate_model_image_scope_fixture() -> (ZodeConfig, crate::Catalog) {
        let mut cfg = test_cfg();
        for name in ["alpha", "beta"] {
            let mut models = indexmap::IndexMap::new();
            models.insert(
                "shared".to_string(),
                crate::config::ModelOverride::default(),
            );
            cfg.providers.insert(
                name.to_string(),
                ProviderConfig {
                    r#type: Some(ProviderKind::Openai),
                    // Deliberately identical endpoint identity: only the
                    // explicit provider key can distinguish these groups.
                    api_key: Some("sk-shared".to_string()),
                    base_url: Some("https://shared.example/v1".to_string()),
                    models,
                    ..Default::default()
                },
            );
        }
        cfg.provider = cfg.resolve_named_provider("alpha").unwrap();

        let catalog = crate::Catalog::from_json(
            r#"{
              "alpha": { "id":"alpha", "name":"Alpha", "models": {
                "shared": { "id":"shared", "name":"Shared",
                  "modalities":{"input":["text"],"output":["text"]} }
              } },
              "beta": { "id":"beta", "name":"Beta", "models": {
                "shared": { "id":"shared", "name":"Shared",
                  "modalities":{"input":["text","image"],"output":["text"]} }
              } }
            }"#,
        )
        .expect("parse duplicate-model capability fixture");
        (cfg, catalog)
    }

    #[test]
    fn explicit_provider_scope_survives_assemble_hot_swap_and_vision_selection() {
        let (cfg, catalog) = duplicate_model_image_scope_fixture();

        // Ordinary assembly resolves capabilities from the active provider key.
        // Selecting beta must not be rebound to the first owner (alpha) merely
        // because both endpoints use the same wire model id.
        let template = EngineTemplate::new(
            cfg,
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-07-15".into(),
        )
        .with_provider("beta")
        .unwrap();
        assert_eq!(
            template.cfg.active_provider_key(),
            Some("alpha"),
            "the resolved configs are intentionally indistinguishable"
        );
        assert_eq!(template.provider_scope(), Some("beta"));
        let resolved = resolve_provider_capabilities(
            &template.cfg.provider,
            template.provider_scope(),
            &catalog,
        );
        assert!(
            build_provider(&resolved)
                .unwrap()
                .capabilities()
                .supports_images
        );

        // The vision picker scopes each duplicate model to the provider being
        // inspected, and therefore exposes beta but not text-only alpha.
        assert!(template
            .resolve_vision_provider_with_catalog("alpha", &catalog)
            .is_none());
        assert_eq!(
            template
                .resolve_vision_provider_with_catalog("beta", &catalog)
                .and_then(|provider| provider.base_url),
            Some("https://shared.example/v1".to_string())
        );

        // hot_swap_model starts with `with_model`; repeating/switching to a
        // model owned by the selected provider must preserve beta's scope.
        let hot_swap_template = template
            .with_provider("beta")
            .unwrap()
            .with_model("shared".into());
        assert_eq!(
            hot_swap_template.active_provider_name().as_deref(),
            Some("beta")
        );
        let hot_resolved = resolve_provider_capabilities(
            hot_swap_template.active_provider(),
            hot_swap_template.provider_scope(),
            &catalog,
        );
        assert!(
            build_provider(&hot_resolved)
                .unwrap()
                .capabilities()
                .supports_images
        );

        // Generic template/config replacement helpers must not discard an
        // exact selection while beta still owns the active model.
        assert_eq!(
            template
                .with_config(template.cfg.clone())
                .active_provider_name()
                .as_deref(),
            Some("beta")
        );
        assert_eq!(
            template
                .with_provider_config(template.cfg.provider.clone())
                .active_provider_name()
                .as_deref(),
            Some("beta")
        );
        assert_eq!(
            template
                .with_providers_map(template.cfg.providers.clone())
                .active_provider_name()
                .as_deref(),
            Some("beta")
        );

        // `/connect` knows the new group key before it installs the updated
        // providers map. Carry that key explicitly so an otherwise identical
        // newly connected group is not rebound to alpha.
        let connected = ProviderConfig {
            r#type: Some(ProviderKind::Openai),
            api_key: Some("sk-shared".to_string()),
            base_url: Some("https://shared.example/v1".to_string()),
            model: Some("shared".to_string()),
            ..Default::default()
        };
        let mut saved = template.cfg.clone();
        saved.connect_provider(
            "gamma",
            connected.clone(),
            crate::config::ModelOverride::default(),
        );
        let connected_template = template
            .with_provider_config_for_key(connected, "gamma".to_string())
            .with_providers_map(saved.providers);
        assert_eq!(
            connected_template.active_provider_name().as_deref(),
            Some("gamma")
        );
    }

    #[test]
    fn hot_swap_model_recomputes_thinking_for_new_provider() {
        // Two named providers with different `supports_thinking` capability
        // (Anthropic vs. an OpenAI-compat endpoint), each owning a distinct
        // model id — same shape as `duplicate_model_image_scope_fixture`
        // above, but exercising the REAL `hot_swap_model` path (which builds
        // a live provider and must recompute engine.thinking/reasoning_effort
        // from it) instead of just `with_model` on the template.
        let mut cfg = test_cfg();
        cfg.effort = Some("high".into());
        cfg.providers.insert(
            "anthropic-p".to_string(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-anthropic".into()),
                model: Some("claude-thinking".into()),
                ..Default::default()
            },
        );
        cfg.providers.insert(
            "openai-p".to_string(),
            ProviderConfig {
                r#type: Some(ProviderKind::Openai),
                api_key: Some("sk-openai".into()),
                model: Some("gpt-compat".into()),
                ..Default::default()
            },
        );
        cfg.provider = cfg.resolve_named_provider("anthropic-p").unwrap();

        let template = EngineTemplate::new(
            cfg,
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-07-16".into(),
        )
        .with_provider("anthropic-p")
        .unwrap();

        let seed_provider = build_provider(&template.cfg.provider).unwrap();
        let mut engine = minimal_engine(seed_provider);
        assert!(
            engine.thinking.is_none(),
            "minimal_engine starts with no knobs computed"
        );

        // Hot-swap onto the anthropic model: /effort=high + supports_thinking
        // must populate a thinking budget for THIS provider.
        let template = template
            .hot_swap_model(&mut engine, "claude-thinking".into())
            .unwrap();
        assert_eq!(
            template.active_provider_name().as_deref(),
            Some("anthropic-p")
        );
        assert!(
            engine.thinking.is_some(),
            "anthropic provider with /effort=high must get a thinking budget"
        );

        // Hot-swap to the openai-compat model (supports_thinking=false): the
        // stale anthropic thinking budget must be dropped, not carried over.
        let template = template
            .hot_swap_model(&mut engine, "gpt-compat".into())
            .unwrap();
        assert_eq!(template.active_provider_name().as_deref(), Some("openai-p"));
        assert!(
            engine.thinking.is_none(),
            "switching to a non-thinking provider must clear the stale thinking budget"
        );
    }

    #[test]
    fn with_provider_on_multi_model_picks_first_model() {
        let template = EngineTemplate::new(
            deepseek_multi_model_cfg(),
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-06-14".into(),
        );
        // Selecting a multi-model provider via the settings picker adopts its
        // shared creds and defaults to its first listed model — no map leak.
        let switched = template.with_provider("deepseek").expect("named provider");
        assert_eq!(switched.model(), Some("deepseek-v4-pro"));
        let p = switched.active_provider();
        assert_eq!(p.api_key.as_deref(), Some("sk-deepseek"));
        assert_eq!(p.context_window, Some(1_000_000)); // default model's override applied
        assert!(p.models.is_empty());
    }

    #[test]
    fn model_ids_include_multi_model_provider_models() {
        let template = EngineTemplate::new(
            deepseek_multi_model_cfg(),
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-06-14".into(),
        );
        let ids = template.model_ids();
        assert!(
            ids.contains(&"MiniMax-M1".to_string()),
            "active model listed"
        );
        assert!(ids.contains(&"deepseek-v4-pro".to_string()));
        assert!(ids.contains(&"deepseek-v4-flash".to_string()));
    }

    #[test]
    fn with_model_adopts_owning_provider_creds_and_override() {
        let template = EngineTemplate::new(
            deepseek_multi_model_cfg(),
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-06-14".into(),
        );
        // Switching to a model under the deepseek provider adopts its shared
        // credentials plus the per-model override.
        let switched = template.with_model("deepseek-v4-pro".into());
        assert_eq!(switched.model(), Some("deepseek-v4-pro"));
        let p = switched.active_provider();
        assert_eq!(p.api_key.as_deref(), Some("sk-deepseek"));
        assert_eq!(
            p.base_url.as_deref(),
            Some("https://api.deepseek.com/anthropic")
        );
        assert_eq!(p.context_window, Some(1_000_000));
        assert!(p.models.is_empty(), "active provider drops the models map");

        // An unknown model just sets the model; active creds are untouched.
        let unknown = template.with_model("totally-unknown".into());
        assert_eq!(unknown.model(), Some("totally-unknown"));
        assert_eq!(
            unknown.active_provider().api_key.as_deref(),
            Some("sk-test")
        );
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

    #[test]
    fn prepend_reminder_lands_in_first_text_block() {
        let content = vec![agent::message::ContentBlock::Text { text: "hi".into() }];
        let out = crate::reminders::prepend_reminder(content, &["note".into()]);
        match &out[0] {
            agent::message::ContentBlock::Text { text } => {
                assert!(text.starts_with("<system-reminder>"));
                assert!(text.contains("- note"));
                assert!(text.ends_with("hi"));
            }
            _ => panic!("expected text block"),
        }
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

    #[test]
    fn compact_direction_halves_when_near_the_window() {
        use agent::compact::PartialCompactDirection;
        // Small transcript → Full (maximal compaction).
        assert!(matches!(
            compact_direction(50_000, 200_000),
            PartialCompactDirection::Full
        ));
        // Near/over the window → a Full request would itself overflow the
        // provider input limit (the observed 220k-into-196k deadlock), so
        // compact the earliest half instead.
        assert!(matches!(
            compact_direction(120_000, 200_000),
            PartialCompactDirection::EarliestHalf
        ));
        assert!(matches!(
            compact_direction(220_000, 196_000),
            PartialCompactDirection::EarliestHalf
        ));
        // Unknown window → no basis to restrict.
        assert!(matches!(
            compact_direction(1_000_000, 0),
            PartialCompactDirection::Full
        ));
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn restore_pending_injects_files_before_the_user_prompt() {
        use agent::stream::Event;
        use agent::testing::MockProvider;
        use std::sync::atomic::Ordering;

        let dir = tempfile::tempdir().unwrap();
        let touched = dir.path().join("recent.rs");
        std::fs::write(&touched, "pub fn recently_touched() {}").unwrap();

        let provider: Arc<dyn Provider> = Arc::new(MockProvider::new(vec![
            Event::TextDelta { delta: "ok".into() },
            Event::Result {
                data: agent::stream::ResultData {
                    stop_reason: Some("end_turn".into()),
                    ..Default::default()
                },
            },
        ]));
        let engine = minimal_engine(provider);
        engine.recent_files.record(touched.clone());
        engine.restore_pending.store(true, Ordering::SeqCst);

        let mut stream = engine
            .turn("continue please", AbortController::new())
            .await
            .unwrap();
        while let Some(item) = stream.next().await {
            item.unwrap();
        }

        let snap: Vec<Message> = engine.store.lock().unwrap().iter().cloned().collect();
        // Restoration message sits BEFORE the user prompt.
        let restore_idx = snap.iter().position(|m| matches!(
            m,
            Message::User { content, .. } if content.iter().any(|b| matches!(
                b,
                ContentBlock::Text { text } if text.contains("[Post-compaction file restoration")
            ))
        ));
        let prompt_idx = snap.iter().position(|m| {
            matches!(
                m,
                Message::User { content, .. } if content.iter().any(|b| matches!(
                    b,
                    ContentBlock::Text { text } if text.contains("continue please")
                ))
            )
        });
        assert!(
            restore_idx.is_some(),
            "restoration message missing: {snap:?}"
        );
        assert!(restore_idx.unwrap() < prompt_idx.unwrap());
        assert_eq!(
            engine.take_restore_note().as_deref(),
            Some("post-compact restore: 1 file(s)")
        );
        assert!(engine.take_restore_note().is_none()); // take clears
                                                       // Latch consumed — next turn will not re-inject.
        assert!(!engine.restore_pending.load(Ordering::SeqCst));
    }

    #[cfg(feature = "noema")]
    #[tokio::test]
    #[serial_test::serial]
    async fn post_turn_extraction_stores_high_confidence_memory() {
        use agent::message::Header;
        use agent::stream::Event;

        // The MockProvider returns a JSON array as if the extraction model
        // identified one durable, high-confidence preference.
        let json = r#"[{"body":"User prefers tabs over spaces","kind":"preference","scope":"user","sensitivity":"internal","importance":0.7,"confidence":0.95}]"#;
        let provider =
            agent::testing::MockProvider::new(vec![Event::TextDelta { delta: json.into() }]);

        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("NOEMA_ROOT", dir.path());
        let noema = ZodeNoema::from_settings(&crate::config::NoemaSettings {
            auto_extract: Some(true), // applies autoSafe
            user: Some("kay".into()),
            ..Default::default()
        });
        std::env::remove_var("NOEMA_ROOT");

        let mut eng = minimal_engine(Arc::new(provider));
        eng.noema = noema;
        eng.extract_config = crate::noema_extract::ExtractConfig {
            enabled: true,
            ..Default::default()
        };
        {
            let mut store = eng.store.lock().unwrap();
            store
                .push(Message::User {
                    header: Header::new(),
                    content: vec![ContentBlock::Text {
                        text: "I always use tabs, never spaces.".into(),
                    }],
                })
                .unwrap();
        }

        // Use the awaited inline path for deterministic assertion.
        eng.extract_post_turn_inline().await;

        // The extracted preference is now recallable.
        let recalled = eng.noema.recall_for_turn("tabs spaces", None).unwrap();
        assert!(
            recalled.is_some_and(|m| m.contains("tabs over spaces")),
            "extracted memory should be recallable"
        );
    }

    #[cfg(feature = "noema")]
    #[tokio::test]
    #[serial_test::serial]
    async fn post_turn_extraction_noop_when_disabled() {
        let provider = agent::testing::MockProvider::new(Vec::new());
        let eng = Arc::new(minimal_engine(Arc::new(provider)));
        // extract_config defaults to disabled → must not touch the provider/store.
        eng.spawn_post_turn_extraction();
        // Nothing to assert beyond "did not panic / did not block"; the disabled
        // guard returns before any await.
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
            None,
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
            None,
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
            None,
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

    #[test]
    fn tool_access_mode_serializes_as_camel_case() {
        assert_eq!(
            serde_json::to_value(ToolAccessMode::ReadOnly).unwrap(),
            serde_json::json!("readOnly")
        );
        assert_eq!(
            serde_json::from_value::<ToolAccessMode>(serde_json::json!("prompt")).unwrap(),
            ToolAccessMode::Prompt
        );
        assert_eq!(
            serde_json::from_value::<ToolAccessMode>(serde_json::json!("auto")).unwrap(),
            ToolAccessMode::Auto
        );
    }

    #[tokio::test]
    async fn read_only_tools_does_not_add_plan_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let template = EngineTemplate::new(
            test_cfg(),
            dir.path().to_path_buf(),
            None,
            false,
            None,
            "2026-06-13".into(),
        )
        .with_tool_access(ToolAccessMode::ReadOnly);

        let eng = template.assemble().await.unwrap();
        let names: Vec<String> = eng.tools.names().map(str::to_string).collect();
        assert!(names.contains(&"FileRead".to_string()), "{names:?}");
        assert!(!names.contains(&"FileWrite".to_string()), "{names:?}");
        assert!(
            eng.tools
                .list()
                .iter()
                .all(|tool| matches!(tool.safety_class(), SafetyClass::ReadOnly)),
            "read-only access exposed a mutating tool"
        );
        assert!(!template.plan_mode());
        assert!(
            !eng.system.as_deref().unwrap_or("").contains("# Plan mode"),
            "read-only access must not inject the plan-mode prompt"
        );
        // read_only_tools filters `run_check` (Mutating) same as plan_mode;
        // the prompt must not advertise a tool the model doesn't have.
        assert!(
            eng.tools.get("run_check").is_none(),
            "run_check must be filtered under read-only tool access"
        );
        assert!(
            !eng.system.as_deref().unwrap_or("").contains("run_check"),
            "run_check must not be advertised when read-only access filtered it out"
        );
    }

    #[tokio::test]
    async fn read_only_tool_access_uses_queue_gate_for_ask_listed_reads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("input.txt"), "secret").unwrap();
        let mut cfg = test_cfg();
        cfg.permissions.ask.push("FileRead".into());
        let (queue, mut approvals) = crate::approval::approval_queue();
        let template = EngineTemplate::new(
            cfg,
            dir.path().to_path_buf(),
            Some(queue),
            true,
            None,
            "2026-06-13".into(),
        )
        .with_tool_access(ToolAccessMode::ReadOnly);
        let eng = template.assemble().await.unwrap();
        let tool = eng.tools.get("FileRead").expect("FileRead registered");
        let ctx = ToolUseContext::new(dir.path());

        let call = tokio::spawn(async move {
            tool.call(&ctx, serde_json::json!({"path": "input.txt"}))
                .await
        });
        let request = tokio::time::timeout(std::time::Duration::from_secs(1), approvals.next())
            .await
            .expect("read-only access should retain the queue gate")
            .expect("approval queue should remain open");
        assert_eq!(request.tool, "FileRead");
        request.respond(Approval::Deny).unwrap();
        assert!(call.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn plan_mode_still_adds_prompt_with_explicit_tool_access() {
        let dir = tempfile::tempdir().unwrap();
        let template = EngineTemplate::new(
            test_cfg(),
            dir.path().to_path_buf(),
            None,
            false,
            None,
            "2026-06-13".into(),
        )
        .with_tool_access(ToolAccessMode::Auto)
        .with_plan_mode(true);

        let eng = template.assemble().await.unwrap();
        assert!(eng.system.as_deref().unwrap_or("").contains("# Plan mode"));
        assert!(
            eng.tools
                .list()
                .iter()
                .all(|tool| matches!(tool.safety_class(), SafetyClass::ReadOnly)),
            "plan mode must remain read-only regardless of task access"
        );
    }

    #[test]
    fn legacy_yolo_switch_replaces_explicit_tool_access() {
        let template = EngineTemplate::new(
            test_cfg(),
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-06-13".into(),
        )
        .with_tool_access(ToolAccessMode::ReadOnly);

        let yolo = template.with_yolo(true);
        assert!(yolo.yolo());
        assert_eq!(yolo.tool_access(), ToolAccessMode::Auto);
        let prompt = yolo.with_yolo(false);
        assert!(!prompt.yolo());
        assert_eq!(prompt.tool_access(), ToolAccessMode::Prompt);
    }

    #[tokio::test]
    async fn prompt_tool_access_uses_approval_queue() {
        let dir = tempfile::tempdir().unwrap();
        let (queue, mut approvals) = crate::approval::approval_queue();
        let template = EngineTemplate::new(
            test_cfg(),
            dir.path().to_path_buf(),
            Some(queue),
            true,
            None,
            "2026-06-13".into(),
        )
        .with_tool_access(ToolAccessMode::Prompt);
        let eng = template.assemble().await.unwrap();
        let tool = eng.tools.get("FileWrite").expect("FileWrite registered");
        let ctx = ToolUseContext::new(dir.path());

        let call = tokio::spawn(async move {
            tool.call(
                &ctx,
                serde_json::json!({"path": "prompt.txt", "content": "blocked"}),
            )
            .await
        });
        let request = tokio::time::timeout(std::time::Duration::from_secs(1), approvals.next())
            .await
            .expect("prompt access should request approval")
            .expect("approval queue should remain open");
        assert_eq!(request.tool, "FileWrite");
        request.respond(Approval::Deny).unwrap();
        assert!(call.await.unwrap().is_err());
        assert!(!dir.path().join("prompt.txt").exists());
    }

    #[tokio::test]
    async fn auto_tool_access_bypasses_approval_queue() {
        let dir = tempfile::tempdir().unwrap();
        let (queue, mut approvals) = crate::approval::approval_queue();
        let template = EngineTemplate::new(
            test_cfg(),
            dir.path().to_path_buf(),
            Some(queue),
            false,
            None,
            "2026-06-13".into(),
        )
        .with_tool_access(ToolAccessMode::Auto);
        let eng = template.assemble().await.unwrap();
        let tool = eng.tools.get("FileWrite").expect("FileWrite registered");
        let ctx = ToolUseContext::new(dir.path());

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            tool.call(
                &ctx,
                serde_json::json!({"path": "auto.txt", "content": "allowed"}),
            ),
        )
        .await
        .expect("auto access must not wait for approval");
        assert!(
            result.is_ok(),
            "auto access should execute the tool: {result:?}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("auto.txt")).unwrap(),
            "allowed"
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), approvals.next())
                .await
                .is_err(),
            "auto access must not enqueue an approval"
        );
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
            None,
        )
        .await
        .unwrap();
        let sys = eng.system.as_deref().unwrap_or("");
        assert!(sys.contains("Current goal"), "{sys}");
        assert!(sys.contains("ship v1 of the parser"), "{sys}");
        assert!(sys.contains("run_check"), "{sys}");
        assert!(sys.contains("Effort: high"), "{sys}");
    }

    #[tokio::test]
    async fn goal_prompt_omits_run_check_when_tool_unavailable() {
        // read-only access filters `run_check` (Mutating) the same way plan
        // mode does — the goal block must not tell the model to reach for a
        // tool it doesn't have, but must still push it toward `goal_complete`.
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_cfg();
        cfg.goal = Some("ship v1 of the parser".into());
        let template = EngineTemplate::new(
            cfg,
            dir.path().to_path_buf(),
            None,
            false,
            None,
            "2026-06-13".into(),
        )
        .with_tool_access(ToolAccessMode::ReadOnly);
        let eng = template.assemble().await.unwrap();
        assert!(
            eng.tools.get("run_check").is_none(),
            "run_check must be filtered under read-only tool access"
        );
        let sys = eng.system.as_deref().unwrap_or("");
        assert!(sys.contains("Current goal"), "{sys}");
        assert!(!sys.contains("run_check"), "{sys}");
        assert!(sys.contains("goal_complete"), "{sys}");
        assert!(sys.contains("AUTONOMOUSLY"), "{sys}");
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
            None,
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
            None,
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
            None,
        )
        .await
        .unwrap();
        // EditHistory + BgShell + compact-tracker + verification + tool-trace + reminders.
        assert_eq!(eng.hooks.len(), 6);
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
            None,
        )
        .await
        .unwrap();
        let decision = eng
            .permissions
            .evaluate("Bash", &serde_json::json!({}), None);
        assert!(decision.is_deny());
    }

    #[tokio::test]
    async fn browser_tools_registered_when_enabled() {
        // BrowserConfig.enabled() defaults true (test_cfg() leaves it unset).
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
            None,
        )
        .await
        .unwrap();
        let names: Vec<String> = eng.tools.names().map(|s| s.to_string()).collect();
        for t in [
            "browser_read",
            "browser_act",
            "browser_eval",
            "browser_tabs",
            "browser_upload",
        ] {
            assert!(names.iter().any(|n| n == t), "missing {t}: {names:?}");
        }
        // browser_read stays ReadOnly and un-gated; the mutating trio is
        // pre-wrapped via browser_gated (not double-wrapped by
        // wrap_mutating_tools).
        let read = eng.tools.get("browser_read").expect("browser_read");
        assert_eq!(read.safety_class(), SafetyClass::ReadOnly);
        for t in [
            "browser_act",
            "browser_eval",
            "browser_tabs",
            "browser_upload",
        ] {
            let tool = eng.tools.get(t).unwrap_or_else(|| panic!("missing {t}"));
            assert_eq!(tool.safety_class(), SafetyClass::Mutating);
        }
    }

    #[tokio::test]
    async fn browser_target_override_pins_engine_tools_to_bridge() {
        let dir = tempfile::tempdir().unwrap();
        let template = EngineTemplate::new(
            test_cfg(),
            dir.path().to_path_buf(),
            None,
            false,
            None,
            "2026-07-16".into(),
        )
        .with_browser_target_override(Some(crate::browser::BrowserTarget::Bridge));
        let eng = template.assemble().await.unwrap();
        let read = eng.tools.get("browser_read").expect("browser_read");
        // The session default is managed; the pinned bridge target is
        // unpaired, so the tool must fail with the pairing hint instead of
        // launching a managed Chrome.
        let err = read
            .call(
                &agent::tool::ToolUseContext::new(dir.path()),
                serde_json::json!({"action": "tabs"}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("pair"), "{err}");
    }

    #[tokio::test]
    async fn browser_tools_absent_when_group_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_cfg();
        cfg.plugins.disabled.push("tools:browser".into());
        let eng = ZodeEngine::assemble(
            &cfg,
            dir.path().to_path_buf(),
            Arc::new(BypassGate),
            None,
            "2026-06-13",
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();
        let names: Vec<String> = eng.tools.names().map(|s| s.to_string()).collect();
        assert!(
            !names.iter().any(|n| n.starts_with("browser_")),
            "{names:?}"
        );
    }

    #[test]
    fn limits_prefer_provider_over_top_level() {
        use crate::config::ProviderConfig;
        // A model id shared across two providers with DIFFERENT windows.
        let cat = crate::Catalog::from_json(
            r#"{
              "alpha": { "id":"alpha","name":"Alpha","models": {
                "shared": { "id":"shared","name":"Shared","limit": { "context": 128000 } } } },
              "beta": { "id":"beta","name":"Beta","models": {
                "shared": { "id":"shared","name":"Shared","limit": { "context": 1000000 } } } }
            }"#,
        )
        .expect("parse fixture catalog");

        // Explicit per-provider window wins over top-level and catalog.
        let provider = ProviderConfig {
            context_window: Some(1_000_000),
            max_output_tokens: Some(8192),
            ..Default::default()
        };
        assert_eq!(
            resolve_context_window(&provider, Some(200_000), None, &cat),
            1_000_000
        );
        assert_eq!(
            resolve_max_output(&provider, Some(4096), None, &cat, 1_000_000),
            8192
        );

        // No per-provider window → top-level value.
        let bare = ProviderConfig::default();
        assert_eq!(
            resolve_context_window(&bare, Some(200_000), None, &cat),
            200_000
        );

        // No config window → models.dev catalog, scoped to the active provider
        // (so the shared id resolves to the right provider's window).
        let shared = ProviderConfig {
            model: Some("shared".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_context_window(&shared, None, Some("beta"), &cat),
            1_000_000
        );
        assert_eq!(
            resolve_context_window(&shared, None, Some("alpha"), &cat),
            128_000
        );

        // Unknown model with no config → conservative default.
        assert_eq!(
            resolve_context_window(&bare, None, None, &cat),
            DEFAULT_MODEL_MAX_TOKENS
        );
        assert_eq!(
            resolve_max_output(&bare, None, None, &cat, DEFAULT_MODEL_MAX_TOKENS),
            DEFAULT_MAX_OUTPUT_TOKENS
        );
    }

    #[test]
    fn provider_image_capability_is_inferred_without_overriding_user_choice() {
        use crate::config::ProviderConfig;
        let cat = crate::Catalog::from_json(
            r#"{
              "alibaba": { "id":"alibaba", "name":"Alibaba", "models": {
                "qwen-vl": { "id":"qwen-vl", "name":"Qwen VL",
                  "modalities":{"input":["text","image"],"output":["text"]} },
                "qwen-text": { "id":"qwen-text", "name":"Qwen Text",
                  "modalities":{"input":["text"],"output":["text"]} }
              } }
            }"#,
        )
        .expect("parse fixture catalog");

        let vision = ProviderConfig {
            model: Some("qwen-vl".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_provider_capabilities(&vision, Some("custom-qwen"), &cat).supports_images,
            Some(true),
            "custom provider names should fall back to matching model metadata"
        );

        let text = ProviderConfig {
            model: Some("qwen-text".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_provider_capabilities(&text, Some("alibaba"), &cat).supports_images,
            Some(false)
        );

        let explicit_off = ProviderConfig {
            model: Some("qwen-vl".into()),
            supports_images: Some(false),
            ..Default::default()
        };
        assert_eq!(
            resolve_provider_capabilities(&explicit_off, Some("alibaba"), &cat).supports_images,
            Some(false),
            "an explicit supportsImages override must win over the catalog"
        );
    }

    #[test]
    fn resolve_max_output_is_automatic_and_self_correcting() {
        let cat = crate::Catalog::from_json(
            r#"{ "xfyun": { "id":"xfyun","name":"X","models": {
                "astron-code-latest": { "id":"astron-code-latest","name":"A",
                  "limit": { "context": 200000, "output": 8192 } } } } }"#,
        )
        .expect("parse fixture catalog");

        // The reported misconfig: maxOutputTokens == contextWindow. The
        // physically-impossible value is ignored; the catalog's real cap wins.
        let bad = ProviderConfig {
            model: Some("astron-code-latest".into()),
            max_output_tokens: Some(200_000),
            ..Default::default()
        };
        assert_eq!(
            resolve_max_output(&bad, None, Some("xfyun"), &cat, 200_000),
            8192,
            "max_output >= context must be rejected and auto-resolved from catalog"
        );

        // No explicit value, known model → catalog cap (fully automatic).
        let auto = ProviderConfig {
            model: Some("astron-code-latest".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_max_output(&auto, None, Some("xfyun"), &cat, 200_000),
            8192
        );

        // Unknown model, NO explicit value → conservative default (clean auto path).
        let unknown_auto = ProviderConfig {
            model: Some("mystery".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_max_output(&unknown_auto, None, None, &cat, 200_000),
            DEFAULT_MAX_OUTPUT_TOKENS
        );

        // Unknown model, bad explicit value → rejected → conservative default.
        let unknown = ProviderConfig {
            model: Some("mystery".into()),
            max_output_tokens: Some(999_999),
            ..Default::default()
        };
        assert_eq!(
            resolve_max_output(&unknown, None, None, &cat, 200_000),
            DEFAULT_MAX_OUTPUT_TOKENS
        );

        // Degenerate context window (0 or 1) must NOT clamp the cap to 0 — every
        // provider rejects max_tokens=0.
        let bare = ProviderConfig::default();
        assert_eq!(
            resolve_max_output(&bare, None, None, &cat, 0),
            DEFAULT_MAX_OUTPUT_TOKENS
        );
        assert_eq!(
            resolve_max_output(&bare, None, None, &cat, 1),
            DEFAULT_MAX_OUTPUT_TOKENS
        );

        // A sane explicit value is honored as-is.
        let sane = ProviderConfig {
            max_output_tokens: Some(32_768),
            ..Default::default()
        };
        assert_eq!(resolve_max_output(&sane, None, None, &cat, 200_000), 32_768);
    }

    #[test]
    fn pre_turn_compact_reserves_completion_budget() {
        // Reported Anthropic failure:
        // messages=871_190, completion=384_000, window=1_048_565.
        // Input alone is only ~83% of the window, so an input-only 98% guard
        // would skip compaction, but providers validate prompt + max_tokens.
        assert!(pre_turn_compact_needed(871_190, 1_048_565, 384_000));

        // The same prompt with a normal 16k completion still has enough room.
        assert!(!pre_turn_compact_needed(871_190, 1_048_565, 16_384));

        // Preserve the old near-full prompt behavior even with small outputs.
        assert!(pre_turn_compact_needed(1_030_000, 1_048_565, 512));
    }

    #[test]
    fn with_model_to_unknown_clears_previous_model_overrides() {
        // Active provider currently describes a 1M-window model with prices.
        let mut cfg = test_cfg();
        cfg.provider.context_window = Some(1_000_000);
        cfg.provider.max_output_tokens = Some(64_000);
        cfg.provider.input_price = Some(3.0);
        let template = EngineTemplate::new(
            cfg,
            std::path::PathBuf::from("/tmp/zode"),
            None,
            false,
            None,
            "2026-06-14".into(),
        );

        // Switching to a model no provider group describes drops those stale
        // per-model fields so the new model resolves its own context window.
        let switched = template.with_model("brand-new-model".into());
        let p = switched.active_provider();
        assert_eq!(p.model.as_deref(), Some("brand-new-model"));
        assert_eq!(p.context_window, None);
        assert_eq!(p.max_output_tokens, None);
        assert_eq!(p.input_price, None);
        // Shared credentials are preserved across the model switch.
        assert_eq!(p.api_key.as_deref(), Some("sk-test"));
    }

    #[cfg(feature = "noema")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial_test::serial]
    async fn manual_compact_sinks_analysis_into_noema() {
        use agent::stream::Event;
        use agent::testing::MockProvider;

        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("NOEMA_ROOT", dir.path());
        let adapter = crate::noema::ZodeNoema::from_settings(&crate::config::NoemaSettings {
            auto_extract: Some(true), // → autoSafe write policy
            user: Some("tester".into()),
            ..Default::default()
        });
        std::env::remove_var("NOEMA_ROOT");

        let tagged = "<analysis>\n\
            - REQUIREMENT: dark mode must be the default theme.\n\
            </analysis>\n\
            <summary>User asked for dark mode; work continues.</summary>";
        let provider: Arc<dyn Provider> = Arc::new(MockProvider::new(vec![
            Event::TextDelta {
                delta: tagged.into(),
            },
            Event::Result {
                data: Default::default(),
            },
        ]));
        let mut engine = minimal_engine(provider);
        engine.session_store = Some(Arc::new(crate::compact_memory::NoemaSessionStore::new(
            adapter.clone(),
            PathBuf::from("."),
        )));
        {
            let mut store = engine.store.lock().unwrap();
            store
                .push(Message::User {
                    header: agent::message::Header::new(),
                    content: vec![ContentBlock::Text {
                        text: "please default to dark mode".into(),
                    }],
                })
                .unwrap();
            store
                .push(Message::Assistant {
                    header: agent::message::Header::new(),
                    content: vec![ContentBlock::Text {
                        text: "done".into(),
                    }],
                })
                .unwrap();
        }

        let outcome = engine.compact(AbortController::new()).await.unwrap();
        assert_eq!(outcome.replaced, 2);

        // The REQUIREMENT bullet (0.85 confidence, autoSafe) is stored and
        // recallable straight away. `candidate_from_entry` sinks compact
        // bullets at `ZodeMemoryScope::Project` (they describe THIS
        // project's decisions/constraints, not user-global preferences),
        // and noema only loads the project cortex when a `cwd` is given —
        // so recall must pass the same cwd the sink wrote under (`engine.cwd`,
        // ".", matching `NoemaSessionStore::new`'s second arg above). This
        // mirrors every real call site (e.g. `inject_noema_memory`), which
        // always recalls with `Some(self.cwd.as_path())`.
        let recalled = adapter
            .recall_for_turn("dark mode theme", Some(engine.cwd.as_path()))
            .unwrap();
        assert!(recalled.is_some(), "expected the sunk memory to recall");
    }

    #[tokio::test]
    async fn branch_drift_uses_tracker_baseline() {
        let tracker = crate::reminders::ReminderTracker::default();
        // Baseline = what the prompt was rendered with.
        assert!(tracker.note_git_branch(Some("main".into())).is_none());
        // Simulate a checkout observed at the next turn.
        let note = tracker.note_git_branch(Some("feature/y".into())).unwrap();
        assert!(note.contains("feature/y"));
        assert!(note.contains("main"));
    }
}
