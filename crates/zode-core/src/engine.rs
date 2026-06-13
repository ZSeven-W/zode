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
use agent::stream::EventStream;
use agent::tool::{SafetyClass, ToolRegistry};
use agent_tools_code::{
    register_default_with_todo, BashOutputTool, BashRunTool, BashSessionRegistry, KillShellTool,
    TodoState, ToolSearchTool, WorkspacePolicy,
};
// QueryLoop's builder takes std::sync::Mutex (not tokio's). We never hold
// these guards across an await — callers snapshot (MessageStore: Clone)
// before async work.
use std::sync::Mutex;

use crate::approval::ApprovalGate;
use crate::config::ZodeConfig;
use crate::error::CoreError;
use crate::gated_tool::PermissionGatedTool;
use crate::provider::build_provider;

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
    pub fn assemble(
        cfg: &ZodeConfig,
        cwd: PathBuf,
        gate: Arc<dyn ApprovalGate>,
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

        // 2. Wrap mutating/destructive tools with the approval gate.
        let mut gated = wrap_mutating_tools(base, &gate);

        // 3. ToolSearch over the full set (candidates = snapshot of the
        //    gated registry, taken before ToolSearch itself is added).
        let candidates = Arc::new(gated.clone());
        gated.register(Arc::new(ToolSearchTool::new(candidates)));

        let tools = Arc::new(gated);

        // Permissions: Bypass mode (so non-denied tools reach the gate)
        // plus hard-deny rules (still enforced ahead of the bypass).
        // Interactive `ask` is handled entirely by the gate (master §4.6①).
        let mut pm = PermissionManager::new().with_mode(PermissionMode::Bypass);
        for tool in &cfg.permissions.deny {
            pm = pm.deny(RuleSource::User, tool.clone());
        }
        let permissions = Arc::new(pm);

        let file_cache = Arc::new(FileStateCache::new(
            NonZeroUsize::new(FILE_CACHE_ENTRIES).expect("nonzero"),
            FILE_CACHE_BYTES,
        ));

        Ok(Self {
            provider,
            tools,
            permissions,
            hooks: Arc::new(HookRunner::new()),
            store: Arc::new(Mutex::new(MessageStore::new())),
            file_cache,
            compact_state: Arc::new(Mutex::new(AutoCompactState::default())),
            model,
            system: None,
            cwd,
            max_output_tokens: cfg.max_output_tokens.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS),
            bash_sessions,
            todo_state,
        })
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

    #[test]
    fn assemble_registers_core_tools() {
        let dir = tempfile::tempdir().unwrap();
        let eng = ZodeEngine::assemble(&test_cfg(), dir.path().to_path_buf(), Arc::new(BypassGate))
            .unwrap();
        let names: Vec<String> = eng.tools.names().map(|s| s.to_string()).collect();
        assert!(names.contains(&"FileRead".to_string()), "names: {names:?}");
        assert!(names.contains(&"Bash".to_string()), "names: {names:?}");
        assert!(names.contains(&"BashRun".to_string()), "names: {names:?}");
        assert!(
            names.contains(&"ToolSearch".to_string()),
            "names: {names:?}"
        );
        assert_eq!(eng.model, "MiniMax-M1");
    }

    #[test]
    fn unconfigured_tool_resolves_to_allow_so_the_gate_runs() {
        // BLOCK regression: under Bypass the loop must NOT pre-empt an
        // unconfigured mutating tool with Ask — it must reach Allow so the
        // PermissionGatedTool decorator can prompt.
        let dir = tempfile::tempdir().unwrap();
        let eng = ZodeEngine::assemble(&test_cfg(), dir.path().to_path_buf(), Arc::new(BypassGate))
            .unwrap();
        let decision =
            eng.permissions
                .evaluate("FileWrite", &serde_json::json!({"path": "x"}), None);
        assert!(decision.is_allow(), "expected Allow, got {decision:?}");
    }

    #[test]
    fn deny_rule_still_wins_under_bypass() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_cfg();
        cfg.permissions.deny = vec!["Bash".into()];
        let eng =
            ZodeEngine::assemble(&cfg, dir.path().to_path_buf(), Arc::new(BypassGate)).unwrap();
        let decision = eng
            .permissions
            .evaluate("Bash", &serde_json::json!({}), None);
        assert!(decision.is_deny());
    }
}
