//! Zode configuration: `~/.zode/config.json` (global) shallow-merged
//! with `.zode/config.json` (project) and ANTHROPIC_API_KEY-style env
//! fallbacks. JSON uses camelCase to stay compatible with the
//! TS/Zig-era config files users already have.

use std::collections::HashMap;

use indexmap::IndexMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// serde `skip_serializing_if` helper: omit a value that's still at its default
/// (an all-`None`/empty nested block) so the saved config stays free of
/// redundant all-null noise. Round-trips, since every block is `#[serde(default)]`.
fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[default]
    Anthropic,
    Openai,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderConfig {
    /// Option so config merging can tell "explicitly set" from "absent"
    /// (serde would otherwise fill the default and erase that distinction).
    /// Use [`ProviderConfig::kind`] to read the effective kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<ProviderKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// openai dialect: standard | deepseek | moonshot | openrouter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dialect: Option<String>,
    /// The model's context window in tokens (per provider). Read by the engine
    /// in preference to the top-level `context_window`. None → top-level/default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    /// The model's max output tokens (per provider). Read in preference to the
    /// top-level `max_output_tokens`. None → top-level/default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Explicitly allow or block image input for this provider config.
    /// When unset, provider defaults decide.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_images: Option<bool>,
    /// Opt-in: forward `/effort` as an OpenAI-style `reasoning_effort` request
    /// parameter. Off by default because non-reasoning OpenAI-compatible
    /// endpoints reject the parameter outright.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    /// Capability profile: `"lite"` opts a weak/fast model (flash / mini /
    /// haiku class) into the accommodations bundle — earlier compaction, a
    /// capped context window, a narrowed tool surface, tighter output and
    /// loop budgets, and periodic task re-anchoring. `"standard"` opts out.
    /// Unset → inferred from the model name ([`is_lite_model_name`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Per-provider token prices in USD per million tokens ($/MTok — the form
    /// providers publish). Optional so cost is computed for models the built-in
    /// catalog doesn't know (e.g. DeepSeek), instead of showing "cost n/a". The
    /// displayed cost is converted to the configured `currency` (see config).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_price: Option<f64>,
    /// Models offered by this provider, sharing its `type`/`api_key`/`base_url`/
    /// `dialect`. Lets one provider entry expose several models without repeating
    /// the credentials. Keyed by model id; each value may override the model-
    /// specific fields (context window, max output, prices, image support).
    /// Order-preserving so the config file order is kept in the model picker.
    /// Empty for the common single-model provider — and skipped on serialize.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub models: IndexMap<String, ModelOverride>,
}

/// Per-model overrides under a multi-model provider's `models` map. Only the
/// model-specific fields live here; `type`/`api_key`/`base_url`/`dialect` are
/// inherited from the owning provider. An empty override (all `None`) means the
/// model just shares the provider's settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    /// See [`ProviderConfig::profile`] — per-model override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_images: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_price: Option<f64>,
}

/// Canonical (CamelCase) name for a tool, mapping the pre-unification
/// snake_case names. Applied to permission lists on config load so grants
/// and rules written before the rename keep working. MCP tools
/// (`mcp__server__tool`) are a protocol namespace and pass through.
pub fn canonical_tool_name(name: &str) -> &str {
    match name {
        "browser_act" => "BrowserAct",
        "browser_eval" => "BrowserEval",
        "browser_read" => "BrowserRead",
        "browser_tabs" => "BrowserTabs",
        "browser_upload" => "BrowserUpload",
        "define_agent" => "DefineAgent",
        "define_workflow" => "DefineWorkflow",
        "desktop_act" => "DesktopAct",
        "desktop_eval" => "DesktopEval",
        "desktop_read" => "DesktopRead",
        "desktop_screenshot" => "DesktopScreenshot",
        "goal_complete" => "GoalComplete",
        "lsp_definition" => "LspDefinition",
        "lsp_diagnostics" => "LspDiagnostics",
        "lsp_format" => "LspFormat",
        "lsp_hover" => "LspHover",
        "lsp_references" => "LspReferences",
        "lsp_rename" => "LspRename",
        "lsp_symbols" => "LspSymbols",
        "op_design" => "OpDesign",
        "op_read" => "OpRead",
        "op_write" => "OpWrite",
        "run_check" => "RunCheck",
        "run_workflow" => "RunWorkflow",
        "team_board_append" => "TeamBoardAppend",
        "team_board_read" => "TeamBoardRead",
        "team_board_update" => "TeamBoardUpdate",
        "team_claim" => "TeamClaim",
        "team_dismiss" => "TeamDismiss",
        "team_hire" => "TeamHire",
        "team_list" => "TeamList",
        "team_release" => "TeamRelease",
        "team_send" => "TeamSend",
        other => other,
    }
}

/// Capability tier the engine assembles for. `Lite` bundles the weak-model
/// accommodations; `Standard` is today's behavior, unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelProfile {
    Standard,
    Lite,
}

/// Name heuristic for the lite tier: the fast/distilled SKU names vendors use
/// (deepseek-…-flash, gemini-…-flash, gpt-…-mini/nano, claude-…-haiku,
/// glm-…-air, …-lite). Matches whole delimiter-separated segments, not
/// substrings — "MiniMax-M1" must NOT read as "mini". Deliberately
/// conservative — an explicit `profile: "standard"` always wins, and runtime
/// behavior learning catches what the names miss.
pub fn is_lite_model_name(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|seg| matches!(seg, "flash" | "lite" | "mini" | "nano" | "haiku" | "air"))
}

/// Resolve the profile for the ACTIVE model: explicit config first (provider
/// or per-model override, already folded into `provider`), then the LEARNED
/// verdicts (runtime loop-guard evidence — see [`learn_model_lite`]), then
/// the name heuristic. Unknown profile strings read as `Standard` (fail-open
/// to today's behavior). Users never need to configure lite: obvious SKUs
/// match by name, and everything else is caught by behavior at runtime.
pub fn model_profile(provider: &ProviderConfig, model: &str) -> ModelProfile {
    match provider.profile.as_deref().map(str::trim) {
        Some(p) if p.eq_ignore_ascii_case("lite") => ModelProfile::Lite,
        Some(_) => ModelProfile::Standard,
        None if is_lite_model_name(model) => ModelProfile::Lite,
        None if learned_model_is_lite(model) => ModelProfile::Lite,
        None => ModelProfile::Standard,
    }
}

/// Learned weak-model verdicts, persisted at
/// `<config-dir>/model-profiles.json` as `{"<model>": "lite"}`. Written when
/// runtime behavior (a loop-guard nudge or a tool-loop abort) exposes a weak
/// model the name heuristic missed; consulted by [`model_profile`] below the
/// explicit config, so `profile: "standard"` always overrides a learned
/// verdict.
static LEARNED_PROFILES: std::sync::Mutex<Option<HashMap<String, String>>> =
    std::sync::Mutex::new(None);

fn learned_profiles_path() -> Option<PathBuf> {
    ConfigManager::config_dir()
        .ok()
        .map(|d| d.join("model-profiles.json"))
}

fn with_learned_profiles<R>(f: impl FnOnce(&mut HashMap<String, String>) -> R) -> R {
    let mut guard = LEARNED_PROFILES.lock().expect("learned-profiles lock");
    let map = guard.get_or_insert_with(|| {
        learned_profiles_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    });
    f(map)
}

/// Whether runtime evidence has marked `model` as lite.
pub fn learned_model_is_lite(model: &str) -> bool {
    with_learned_profiles(|m| m.get(model).is_some_and(|v| v.eq_ignore_ascii_case("lite")))
}

/// Test hook: reset the in-process learned-profile cache so a test observes
/// only its own `ZODE_CONFIG_DIR` (the cache loads once per process).
#[cfg(test)]
pub(crate) fn reset_learned_profiles_for_test() {
    *LEARNED_PROFILES.lock().expect("learned-profiles lock") = Some(HashMap::new());
}

/// Record a runtime weak-model verdict for `model` (idempotent, best-effort
/// persistence). Returns `true` when this call newly recorded it.
pub fn learn_model_lite(model: &str) -> bool {
    let model = model.trim();
    if model.is_empty() {
        return false;
    }
    let newly = with_learned_profiles(|m| {
        if m.get(model).is_some_and(|v| v.eq_ignore_ascii_case("lite")) {
            return false;
        }
        m.insert(model.to_string(), "lite".to_string());
        true
    });
    if newly {
        if let Some(path) = learned_profiles_path() {
            let json = with_learned_profiles(|m| serde_json::to_string_pretty(m).ok());
            if let Some(json) = json {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::warn!(error = %e, "failed to persist learned model profile");
                }
            }
        }
    }
    newly
}

impl ModelOverride {
    /// Apply the set (non-`None`) overrides onto a resolved provider config.
    fn apply_to(&self, p: &mut ProviderConfig) {
        if self.context_window.is_some() {
            p.context_window = self.context_window;
        }
        if self.profile.is_some() {
            p.profile = self.profile.clone();
        }
        if self.max_output_tokens.is_some() {
            p.max_output_tokens = self.max_output_tokens;
        }
        if self.supports_images.is_some() {
            p.supports_images = self.supports_images;
        }
        if self.input_price.is_some() {
            p.input_price = self.input_price;
        }
        if self.output_price.is_some() {
            p.output_price = self.output_price;
        }
        if self.cache_read_price.is_some() {
            p.cache_read_price = self.cache_read_price;
        }
        if self.cache_write_price.is_some() {
            p.cache_write_price = self.cache_write_price;
        }
    }
}

impl ProviderConfig {
    /// Effective provider kind (defaults to Anthropic when unset).
    pub fn kind(&self) -> ProviderKind {
        self.r#type.unwrap_or_default()
    }

    /// Per-provider price overrides as agent-rs `ModelPrices`, if any price
    /// field is set. Unset rates default to 0 (e.g. no cache pricing given →
    /// cache tokens are free). Units are $/MTok.
    pub fn price_overrides(&self) -> Option<agent::cost::ModelPrices> {
        if self.input_price.is_none()
            && self.output_price.is_none()
            && self.cache_read_price.is_none()
            && self.cache_write_price.is_none()
        {
            return None;
        }
        Some(agent::cost::ModelPrices::from_usd_per_mtok(
            self.input_price.unwrap_or(0.0),
            self.output_price.unwrap_or(0.0),
            self.cache_read_price.unwrap_or(0.0),
            self.cache_write_price.unwrap_or(0.0),
        ))
    }

    /// Clear the per-model fields (everything a [`ModelOverride`] carries),
    /// keeping the shared credentials and `model`/`models`. Used when switching
    /// the active model to one the provider config doesn't describe, so the new
    /// model resolves its own context window, output cap, and prices from the
    /// catalog/defaults instead of inheriting the previous model's values.
    pub fn clear_model_overrides(&mut self) {
        self.context_window = None;
        self.max_output_tokens = None;
        self.supports_images = None;
        self.input_price = None;
        self.output_price = None;
        self.cache_read_price = None;
        self.cache_write_price = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ImageMode {
    #[default]
    Auto,
    Direct,
    VisionModel,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ImagesConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ImageMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_prompt: Option<String>,
}

impl ImagesConfig {
    pub fn effective_mode(&self) -> ImageMode {
        self.mode.unwrap_or_default()
    }

    pub fn effective_prompt(&self) -> &str {
        self.vision_prompt
            .as_deref()
            .unwrap_or(DEFAULT_VISION_PROMPT)
    }
}

pub const DEFAULT_VISION_PROMPT: &str = "Describe the image precisely for a coding assistant. Mention UI layout, visible text, colors, and error states.";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PermissionsConfig {
    /// Tool names always allowed without prompting.
    pub allow: Vec<String>,
    /// Tool names always denied (hard block).
    pub deny: Vec<String>,
    /// Tool names that require interactive approval.
    pub ask: Vec<String>,
    /// Input-scoped rules using JSON-pointer matchers.
    pub rules: Vec<crate::permission_rules::PermissionRuleSpec>,
}

/// OS-sandbox settings for shell commands. The sandbox is **on by default**
/// (workspace-write, network denied); these refine or disable it.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SandboxSettings {
    /// Run shell commands in an OS sandbox. `None` → enabled (default on).
    pub enabled: Option<bool>,
    /// `"workspace-write"` (default) confines writes to cwd + tmp + roots;
    /// `"read-only"` denies all writes.
    pub mode: Option<String>,
    /// Allow outbound network inside the sandbox. `None`/false → denied.
    pub network: Option<bool>,
    /// Extra writable roots in workspace-write mode (absolute paths).
    pub writable_roots: Vec<String>,
    /// Drop `/tmp` from the default writable roots (Codex `exclude_slash_tmp`).
    /// `None`/false → `/tmp` is writable in workspace-write.
    pub exclude_slash_tmp: Option<bool>,
    /// Drop `$TMPDIR` from the default writable roots (Codex
    /// `exclude_tmpdir_env_var`). `None`/false → `$TMPDIR` is writable.
    pub exclude_tmpdir_env_var: Option<bool>,
    /// Opt-in "strict read": also hide a curated set of credential dirs
    /// (`~/.ssh`, `~/.aws`, the zode config, …) from READS. `None`/false → reads
    /// unrestricted (the safe default for a coding agent that reads the repo).
    pub restrict_reads: Option<bool>,
    /// Windows sandbox tier: `auto`, `basic`, or `elevated` (Tier 2 AppContainer).
    pub windows_tier: Option<String>,
    /// Reusable named policies selected with `--sandbox-profile`.
    pub profiles: HashMap<String, SandboxProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct SandboxProfile {
    pub enabled: Option<bool>,
    pub mode: Option<String>,
    pub network: Option<bool>,
    pub writable_roots: Vec<String>,
    pub exclude_slash_tmp: Option<bool>,
    pub exclude_tmpdir_env_var: Option<bool>,
    pub restrict_reads: Option<bool>,
    pub windows_tier: Option<String>,
}

/// Plugin enable/disable state. Plugins are on by default, so only the
/// disabled ids are stored (e.g. `["tools:git", "mcp:foo", "lsp:rust"]`).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginsConfig {
    pub disabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolsConfig {
    /// Whether RTK-style Bash stdout compression is enabled. None -> true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compress_output: Option<bool>,
    /// Opt-in standard-profile tool narrowing: keep only the everyday core
    /// visible per request and defer the long tail (browser/desktop/op/team/
    /// LSP/…) behind ToolSearch — the same mechanism the lite profile always
    /// uses, with a broader core. None -> false (full surface).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_non_core: Option<bool>,
}

impl ToolsConfig {
    pub fn defer_non_core(&self) -> bool {
        self.defer_non_core.unwrap_or(false)
    }
}

/// Default OpenPencil release the installer/launcher targets. zode and
/// OpenPencil version independently — do NOT use zode's CARGO_PKG_VERSION.
pub const DEFAULT_OPENPENCIL_VERSION: &str = "0.8.0";

/// OpenPencil control-surface settings (the `op-bridge`). Every field is
/// `Option` so layered config can tell "absent" from "explicitly set" and a
/// project layer can reset a global value; read effective values via getters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenPencilConfig {
    pub enabled: Option<bool>,
    pub op_path: Option<String>,
    pub install_command: Option<String>,
    /// OpenPencil release tag to pin the installer/launcher to (no leading `v`).
    pub release_tag: Option<String>,
    pub auto_install: Option<bool>,
    pub auto_launch_gui: Option<bool>,
    pub launch_command: Option<String>,
    pub headless_fallback: Option<bool>,
    pub default_doc: Option<String>,
    pub connect_timeout_ms: Option<u64>,
}

impl OpenPencilConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
    pub fn auto_install(&self) -> bool {
        self.auto_install.unwrap_or(false)
    }
    pub fn auto_launch_gui(&self) -> bool {
        self.auto_launch_gui.unwrap_or(true)
    }
    pub fn launch_command(&self) -> &str {
        self.launch_command.as_deref().unwrap_or("op start")
    }
    pub fn headless_fallback(&self) -> bool {
        self.headless_fallback.unwrap_or(false)
    }
    pub fn connect_timeout_ms(&self) -> u64 {
        self.connect_timeout_ms.unwrap_or(10_000)
    }
    pub fn release_tag(&self) -> &str {
        self.release_tag
            .as_deref()
            .unwrap_or(DEFAULT_OPENPENCIL_VERSION)
    }
}

/// Viewport for the managed browser window (and screenshot size).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewportConfig {
    pub width: u32,
    pub height: u32,
}

/// Built-in browser control (`browser_*` tools, `/browser` command).
/// All fields optional; effective values come from the getters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BrowserConfig {
    pub enabled: Option<bool>,
    pub executable: Option<String>,
    pub headless: Option<bool>,
    pub profile_dir: Option<String>,
    pub default_target: Option<String>,
    pub viewport: Option<ViewportConfig>,
    /// The bridge extension IDs zode accepts (WebSocket Origin check and the
    /// native-messaging manifest). Unset → the published extension's ID. Set
    /// this to REPLACE the list; the FIRST entry is also the one the pairing
    /// popup URL targets, so a locally-keyed build pins its own ID here.
    pub extension_ids: Option<Vec<String>>,
}

impl BrowserConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
    /// Effective bridge-extension accept list (see `extension_ids`).
    pub fn extension_ids(&self) -> Vec<String> {
        match &self.extension_ids {
            Some(ids) if !ids.is_empty() => ids.clone(),
            _ => vec![crate::browser::bridge::server::EXTENSION_ID.to_string()],
        }
    }
    pub fn headless(&self) -> bool {
        self.headless.unwrap_or(false)
    }
    pub fn default_target(&self) -> &str {
        self.default_target.as_deref().unwrap_or("managed")
    }
    pub fn viewport(&self) -> (u32, u32) {
        self.viewport
            .as_ref()
            .map(|v| (v.width, v.height))
            .unwrap_or((1280, 800))
    }
}

/// Built-in desktop control (`desktop_*` tools, `/desktop` command).
/// All fields optional; effective values come from the getters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DesktopConfig {
    pub enabled: Option<bool>,
    pub snapshot_max_nodes: Option<usize>,
    /// Ghost-cursor overlay visualization (zode-overlay helper).
    pub ghost_cursor: Option<bool>,
    /// Global Esc interrupts the turn while desktop automation is active.
    pub esc_cancel: Option<bool>,
    /// Explicit helper path; default is `zode-overlay` next to current_exe.
    pub overlay_helper_path: Option<String>,
}

impl DesktopConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
    pub fn snapshot_max_nodes(&self) -> usize {
        self.snapshot_max_nodes.unwrap_or(500)
    }
    pub fn ghost_cursor(&self) -> bool {
        self.ghost_cursor.unwrap_or(true)
    }
    pub fn esc_cancel(&self) -> bool {
        self.esc_cancel.unwrap_or(true)
    }
    pub fn overlay_helper_path(&self) -> Option<&str> {
        self.overlay_helper_path.as_deref()
    }
}

/// Web search backend configuration (`webSearch` config key). Tavily is the
/// only built-in backend today; the key can also come from `$TAVILY_API_KEY`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebSearchConfig {
    /// Tavily Search API key. Absent (and no `$TAVILY_API_KEY`) → the
    /// `WebSearch` tool is not registered at all, so the model never sees a
    /// name it cannot call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tavily_api_key: Option<String>,
}

impl WebSearchConfig {
    /// Resolve the effective key: config first, then `$TAVILY_API_KEY`.
    pub fn resolved_tavily_key(&self) -> Option<String> {
        self.tavily_api_key
            .clone()
            .or_else(|| std::env::var("TAVILY_API_KEY").ok())
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
    }
}

/// Manually registered external agent CLIs exposed as Task `agent_type`s.
/// Known profile names receive preset defaults; arbitrary names use the custom
/// protocol fields below. Nothing is registered merely because it is on PATH.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExternalAgentsConfig {
    pub enabled: Option<bool>,
    pub timeout_secs: Option<u64>,
    /// Process-wide cap on concurrently running external CLI processes.
    pub max_concurrent: Option<u32>,
    /// Per-profile entries. Merge semantics: keys merge across config layers,
    /// but a same-key entry is replaced WHOLESALE (no field-level deep merge).
    pub agents: IndexMap<String, ExternalAgentEntry>,
}

impl ExternalAgentsConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout_secs.unwrap_or(1800))
    }
    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent.unwrap_or(2)
    }
}

/// One external agent profile. For known presets only
/// `enabled`/`command`/`extra_args`/`env_allow`/`trusted` are honored; the
/// remaining capability fields describe custom profiles.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExternalAgentEntry {
    pub enabled: Option<bool>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub extra_args: Option<Vec<String>>,
    /// "stdin" | "argv" | "file" (argv requires a "{prompt}" placeholder in
    /// `args`; file requires "{prompt_file}").
    pub prompt_transport: Option<String>,
    /// "text" | "jsonl" | "jsonl-claude" | "jsonl-codex" (default "text").
    pub output: Option<String>,
    pub resume_flag: Option<String>,
    /// Arguments appended on the first run with a Zode-generated session ID.
    /// Requires a standalone `{session_id}` token and resume support.
    pub new_session_args: Option<Vec<String>>,
    /// General resume argv template. Requires a standalone `{session_id}`
    /// token and takes precedence over `resumeFlag`.
    pub resume_args: Option<Vec<String>>,
    pub version_requirement: Option<String>,
    /// JSON pointer to a session id in generic JSONL events, e.g. "/id".
    pub session_id_source: Option<String>,
    /// JSON pointer to streamed text in generic JSONL events, e.g. "/text".
    pub text_source: Option<String>,
    /// "none" | "readOnly" | "workspaceWrite" | "unrestricted" | "unknown".
    pub effective_sandbox: Option<String>,
    /// Env var names this CLI needs for its own auth (loader vars rejected).
    pub auth_env: Option<Vec<String>>,
    pub env_allow: Option<Vec<String>>,
    /// Explicit user opt-in: allow non-interactive (--yolo) trust approval.
    pub trusted: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct NoemaSettings {
    /// Long-term memory is enabled by default. Set to false to keep zode fully
    /// memory-less even when the noema feature is compiled in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Optional memory root override. When unset, Noema uses `NOEMA_ROOT` or
    /// `~/.agent-memory`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// Optional personal Noema user id. When unset, Noema config / `$USER` wins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Automatically store explicit user memory requests such as
    /// "remember that ..." or "请记住...". Defaults to true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_remember: Option<bool>,
    /// Run an LLM pass after each turn to extract durable memories from the
    /// conversation (not just explicit "remember…" phrases). On by default —
    /// set to false to keep memory writes explicit; when on, the memory write
    /// policy defaults to `autoSafe` (see [`write_policy`](Self::write_policy))
    /// so high-confidence, novel candidates auto-store while the rest queue
    /// for `/memory review`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_extract: Option<bool>,
    /// Memory write policy applied to the noema root: `"manual"`, `"review"`,
    /// `"autoSafe"`, or `"auto"`. When unset, defaults to `autoSafe` if
    /// `auto_extract` is on, else leaves noema's own default (`review`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_policy: Option<String>,
    /// Model id for the extraction pass. When unset, the engine's active model
    /// runs it. Point this at a cheaper model to cut per-turn cost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract_model: Option<String>,
    /// Also mine the assistant's reply for durable memories (default false:
    /// scan only the user message — higher precision, fewer tokens).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract_scan_assistant: Option<bool>,
    /// Max memories accepted from a single turn (bounds cost + noise). Default 3.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_memories_per_turn: Option<u32>,
    /// Truncate the transcript slice fed to the extractor. Default 4000 chars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract_max_input_chars: Option<u32>,
}

impl NoemaSettings {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    pub fn auto_remember(&self) -> bool {
        self.auto_remember.unwrap_or(true)
    }

    /// Whether the post-turn LLM extraction pass runs. ON by default (memory
    /// is opt-out); set `autoExtract: false` to keep memory writes explicit.
    pub fn auto_extract(&self) -> bool {
        self.auto_extract.unwrap_or(true)
    }

    /// The effective write policy keyword (lowercased for the noema mapping):
    /// an explicit `write_policy` wins; otherwise `"autoSafe"` when extraction
    /// is on, else `"review"` (noema's own default). Always `Some` so the
    /// policy is *deterministic and reversible* — turning `auto_extract` back
    /// off restores `review` rather than leaving the root stuck at `autoSafe`.
    pub fn effective_write_policy(&self) -> String {
        if let Some(p) = self
            .write_policy
            .as_ref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
        {
            return p;
        }
        if self.auto_extract() {
            "autosafe".to_string()
        } else {
            "review".to_string()
        }
    }
}

/// Compaction-ladder + post-compaction restoration knobs (`compact` key).
/// All keys optional; absent keys default to ON so the layered pipeline
/// works out of the box.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CompactSettings {
    /// Ladder step ①: clear old tool results before resorting to LLM
    /// compaction. Default true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub microcompact: Option<bool>,
    /// Sink compaction analysis bullets into noema (also requires noema to
    /// be enabled). Default true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_sink: Option<bool>,
    /// Re-attach recently touched files on the turn after a compaction.
    /// Default true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore_files: Option<bool>,
    /// Token budget for restored file content. Default 50000.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore_files_budget: Option<u32>,
    /// Append a noema recall pack to the restoration message. Default true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recall_after_compact: Option<bool>,
    /// Context-occupancy percent (prompt tokens vs the model window — the
    /// badge value) at which the between-turn auto-compaction fires.
    /// Clamped to 50–97; absent → the built-in default (85).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_compact_percent: Option<u64>,
}

impl CompactSettings {
    pub fn microcompact(&self) -> bool {
        self.microcompact.unwrap_or(true)
    }
    pub fn memory_sink(&self) -> bool {
        self.memory_sink.unwrap_or(true)
    }
    pub fn restore_files(&self) -> bool {
        self.restore_files.unwrap_or(true)
    }
    pub fn restore_files_budget(&self) -> u32 {
        self.restore_files_budget.unwrap_or(50_000)
    }
    pub fn recall_after_compact(&self) -> bool {
        self.recall_after_compact.unwrap_or(true)
    }
    /// Configured auto-compact occupancy percent, bounded so a typo can
    /// neither compact every turn (< 50) nor push compaction past the
    /// validation-safety guard (> 97). `None` → caller default.
    pub fn auto_compact_percent(&self) -> Option<u64> {
        self.auto_compact_percent.map(|p| p.clamp(50, 97))
    }
}

/// Configuration for the built-in LSP plugin: a language server per language
/// key. The key (e.g. "rust", "python") is also the plugin id suffix
/// (`lsp:rust`). `extensions` maps the server to the file types it handles.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LspConfig {
    pub servers: HashMap<String, LspServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LspServerConfig {
    /// Executable to spawn (e.g. "rust-analyzer", "pyright-langserver").
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// File extensions this server handles (e.g. ["rs"], ["py"]).
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// Watchdog policy for unattended turns started by `/loop` and `/schedule`.
///
/// Every field is optional so global, project, and project-state config layers
/// can override one knob without resetting the others. Effective values are
/// exposed through the getters below and deliberately bounded: a malformed or
/// extreme config must not turn the 100 ms TUI tick into either an immediate
/// abort loop or an effectively disabled safety mechanism.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BackgroundWatchdogConfig {
    /// Master switch. Unset defaults to enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Bound claim-to-start queueing and abort a running turn after this many
    /// seconds without provider/tool activity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inactivity_timeout_secs: Option<u64>,
    /// Absolute wall-clock cap even when the turn keeps emitting activity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_runtime_secs: Option<u64>,
    /// Time allowed for cooperative cancellation before the tab is released.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abort_grace_secs: Option<u64>,
    /// Number of recovery attempts after the initial failed/timed-out run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    /// Delay before the first retry; later retries double from this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_backoff_secs: Option<u64>,
    /// Ceiling for exponential retry backoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_backoff_secs: Option<u64>,
}

impl BackgroundWatchdogConfig {
    pub const DEFAULT_INACTIVITY_TIMEOUT_SECS: u64 = 15 * 60;
    pub const DEFAULT_MAX_RUNTIME_SECS: u64 = 60 * 60;
    pub const DEFAULT_ABORT_GRACE_SECS: u64 = 10;
    pub const DEFAULT_MAX_RETRIES: u32 = 3;
    pub const DEFAULT_INITIAL_BACKOFF_SECS: u64 = 5;
    pub const DEFAULT_MAX_BACKOFF_SECS: u64 = 5 * 60;

    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    pub fn inactivity_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.inactivity_timeout_secs
                .unwrap_or(Self::DEFAULT_INACTIVITY_TIMEOUT_SECS)
                .clamp(5, 24 * 60 * 60),
        )
    }

    pub fn max_runtime(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.max_runtime_secs
                .unwrap_or(Self::DEFAULT_MAX_RUNTIME_SECS)
                .clamp(30, 7 * 24 * 60 * 60),
        )
    }

    pub fn abort_grace(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.abort_grace_secs
                .unwrap_or(Self::DEFAULT_ABORT_GRACE_SECS)
                .clamp(1, 5 * 60),
        )
    }

    pub fn max_retries(&self) -> u32 {
        self.max_retries
            .unwrap_or(Self::DEFAULT_MAX_RETRIES)
            .min(20)
    }

    pub fn initial_backoff(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.initial_backoff_secs
                .unwrap_or(Self::DEFAULT_INITIAL_BACKOFF_SECS)
                .clamp(1, 60 * 60),
        )
    }

    pub fn max_backoff(&self) -> std::time::Duration {
        let initial = self.initial_backoff().as_secs();
        std::time::Duration::from_secs(
            self.max_backoff_secs
                .unwrap_or(Self::DEFAULT_MAX_BACKOFF_SECS)
                .clamp(initial, 24 * 60 * 60),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ZodeConfig {
    /// Active provider. Omitted from the saved file when empty (the common case
    /// when only a `providers` map is configured) so no all-null block is written.
    #[serde(skip_serializing_if = "is_default")]
    pub provider: ProviderConfig,
    /// Named providers; `--provider <name>` selects one into `provider`.
    /// Order-preserving (`IndexMap`) so the user's config file order is kept —
    /// the first entry is the default when no provider is explicitly selected.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub providers: IndexMap<String, ProviderConfig>,
    #[serde(skip_serializing_if = "is_default")]
    pub images: ImagesConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Display currency for cost (e.g. "USD", "CNY", "EUR"). `None` → USD.
    /// Per-provider prices are in USD/MTok; the total is converted for display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// UI language code (e.g. "en", "zh"). `None` → English. (Settings picker
    /// + full UI translation land in a follow-up; the value is read here.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// A persistent objective injected into the system prompt so the agent
    /// keeps it in focus. Set/cleared via `/goal`. `None`/empty → no goal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    /// Optional cap on how many turns the autonomous goal loop runs before it
    /// stops on its own (the agent can still end it early via `GoalComplete`,
    /// and the user can always interrupt). `None` → unbounded (the default).
    #[serde(rename = "autoLoopMaxTurns", skip_serializing_if = "Option::is_none")]
    pub auto_loop_max_turns: Option<u32>,
    /// Effort level: "low" | "medium" | "high". Injects a thoroughness
    /// directive into the system prompt. Set via `/effort`. `None` → balanced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Show the agent's thinking/reasoning output in the chat. `None` → shown.
    /// Toggled by `/thinking`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_thinking: Option<bool>,
    /// Show tool-call detail lines in the chat. `None` → shown. Toggled by
    /// `/tool-details`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_tool_details: Option<bool>,
    /// Capture mouse events in the TUI. `None` → ON: wheel-scrolls the chat
    /// and enables in-app drag selection with copy-on-select. Without capture
    /// an alternate-screen TUI can't consume wheel events, so terminals
    /// scroll their own viewport and shear the UI (seen in Warp). Set
    /// `"mouseCapture": false` to hand the mouse back to the terminal
    /// (native drag selection + the terminal's own ⌘C) at the cost of wheel
    /// scrolling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mouse_capture: Option<bool>,
    /// Autonomous orchestration: when on, the agent is told it may decompose a
    /// task and spawn sub-agents (Task tool) on its own, and the `DefineAgent`
    /// tool is registered so it can create new sub-agent types. `None` → ON
    /// (enabled by default). Toggled off via Settings / `/orchestration`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autonomous_orchestration: Option<bool>,
    /// Inject a "use skills first" discipline block into the system prompt when
    /// skills are available. `None` → ON. Set `"skillDiscipline": false` to disable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_discipline: Option<bool>,
    /// Inject an OpenSpec-workflow awareness block into the system prompt when
    /// the project uses OpenSpec (an `openspec/` dir at the project root). `None`
    /// → ON. Set `"openspecAwareness": false` to disable even when detected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openspec_awareness: Option<bool>,
    /// Inject a compact repository map (tracked-file counts per directory)
    /// into the system prompt so the model targets reads/searches instead of
    /// exploring blind. `None` → ON. Set `"repoMap": false` to disable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_map: Option<bool>,
    #[serde(skip_serializing_if = "is_default")]
    pub permissions: PermissionsConfig,
    #[serde(skip_serializing_if = "is_default")]
    pub sandbox: SandboxSettings,
    /// Persisted `/yolo` toggle: tools auto-approve without prompting (deny
    /// rules still apply). The TUI writes it to the GLOBAL `~/.zode/config.json`
    /// on toggle — like the sandbox toggle — so the choice applies to every
    /// workspace's next launch (project config/state layers can still override
    /// it per project). An explicit `--yolo` or `--permission-mode` CLI flag
    /// overrides it for that run. `None` → off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yolo: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Runaway backstop on agent-loop iterations. Absent or `0` = UNBOUNDED: the
    /// loop runs until the model returns a turn with no tool calls (the natural
    /// "model is done" stop). Set a positive value to force a finite cap — handy
    /// for headless `-p` runs, which can't be interrupted with Ctrl+C.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
    /// Optional per-child runaway backstop for Task-spawned sub-agents. Absent
    /// or `0` leaves child loops unbounded so they run until the model naturally
    /// finishes; a positive value opts into a finite model/tool round-trip cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_max_iterations: Option<u32>,
    /// How many times to retry a transient API failure (rate limit / 5xx /
    /// network) with exponential backoff before the turn fails. Absent = 10;
    /// `0` disables retries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_api_retries: Option<u32>,
    /// Per-call timeout (seconds) for an MCP tool. A remote tool must not hold a
    /// scheduler turn forever, but slow-but-valid servers (build/deploy/scrape)
    /// need more than the default. Absent = 60; `0` disables the local timeout
    /// (the call then relies on the turn's own cancellation only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_tool_timeout_secs: Option<u64>,
    /// Whether zode silently checks GitHub Releases in the background at startup
    /// and swaps in a newer build (applied on the next launch). Default ON; set
    /// `false` to disable. Skipped automatically for dev builds and when the
    /// install location isn't writable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,
    /// The model's context window in tokens, used for auto-compaction /
    /// context-left math. `None` defaults to a conservative 200K. Set this to
    /// `1000000` for a 1M-context model (e.g. `claude-opus-4-8[1m]`) so zode
    /// uses the full window instead of compacting at 200K. Do NOT set it ABOVE
    /// the model's real window — overestimating makes the request overflow and
    /// the API reject the turn; underestimating only compacts earlier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    /// Sampling temperature. `None` uses the provider default; a low value
    /// (e.g. 0) makes coding output deterministic and more reliably correct.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Request provider prompt caching (`None` defaults to true). For Anthropic/
    /// MiniMax this adds `cache_control` so the stable system+tools prefix is
    /// cached across turns instead of re-billed every turn; OpenAI-compatible
    /// providers cache automatically.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache: Option<bool>,
    /// Plugin enable/disable state (`/plugin` manages MCP, skills, tool groups,
    /// LSP). Default-on, so this only records the disabled ones.
    #[serde(skip_serializing_if = "is_default")]
    pub plugins: PluginsConfig,
    /// Built-in tool behavior knobs.
    #[serde(skip_serializing_if = "is_default")]
    pub tools: ToolsConfig,
    /// Language-server configuration for the built-in LSP plugin.
    #[serde(skip_serializing_if = "is_default")]
    pub lsp: LspConfig,
    /// OpenPencil control-surface configuration (the `op-bridge`).
    #[serde(skip_serializing_if = "is_default")]
    pub openpencil: OpenPencilConfig,
    /// Built-in browser control configuration (the `browser_*` tools).
    #[serde(skip_serializing_if = "is_default")]
    pub browser: BrowserConfig,
    /// Built-in desktop control configuration (the `desktop_*` tools).
    #[serde(skip_serializing_if = "is_default")]
    pub desktop: DesktopConfig,
    /// Web search backend configuration (the `WebSearch` tool). The tool is
    /// registered only when a key is available — here or `$TAVILY_API_KEY`.
    #[serde(skip_serializing_if = "is_default")]
    pub web_search: WebSearchConfig,
    /// External agent CLIs exposed as Task `agent_type`s.
    #[serde(skip_serializing_if = "ExternalAgentsConfig::is_default")]
    pub external_agents: ExternalAgentsConfig,
    /// Native Noema long-term memory integration.
    #[serde(skip_serializing_if = "is_default")]
    pub noema: NoemaSettings,
    /// Compaction ladder / post-compaction restoration knobs.
    #[serde(skip_serializing_if = "is_default")]
    pub compact: CompactSettings,
    /// Liveness, cancellation, and retry policy for unattended scheduler turns.
    #[serde(skip_serializing_if = "is_default")]
    pub background_watchdog: BackgroundWatchdogConfig,

    // --- Legacy (Zig/TS-era) flat fields, read-only for backward compat.
    // Mapped into `provider` by `normalize_legacy()` and dropped on save
    // (skip_serializing), so an old config migrates to the new shape the
    // first time it's written back.
    #[serde(default, skip_serializing, rename = "anthropic_api_key")]
    pub legacy_anthropic_api_key: Option<String>,
    #[serde(default, skip_serializing, rename = "openai_api_key")]
    pub legacy_openai_api_key: Option<String>,
    #[serde(default, skip_serializing, rename = "openai_base_url")]
    pub legacy_openai_base_url: Option<String>,
    #[serde(default, skip_serializing, rename = "model")]
    pub legacy_model: Option<String>,
}

/// Model written into the starter config and used as the interactive-launch
/// fallback when the user hasn't picked one. A balanced coding default; the
/// user can change it in `config.json` or via `/connect`.
pub const DEFAULT_STARTER_MODEL: &str = "claude-sonnet-4-6";

impl ZodeConfig {
    /// Effective TUI mouse-capture setting (see the `mouse_capture` field):
    /// explicit value if set, otherwise ON.
    pub fn mouse_capture_enabled(&self) -> bool {
        self.mouse_capture.unwrap_or(true)
    }

    /// Make this config assemble for the interactive TUI even when the user
    /// hasn't finished provider setup, so they always land in the UI (where
    /// `/connect` finishes setup) instead of being blocked at startup:
    /// - a missing model gets [`DEFAULT_STARTER_MODEL`];
    /// - a key-requiring provider (Anthropic / OpenAI) with no key gets an
    ///   empty key, so the provider *builds* — the first request then fails
    ///   with an auth error until a real key is configured.
    ///
    /// Returns `true` when the provider still lacks credentials (i.e. the user
    /// must run `/connect`), so the caller can surface a setup hint. Ollama
    /// needs no key, so it never reports as needing setup. TUI-only: headless
    /// surfaces keep the strict `MissingApiKey` / no-model errors.
    pub fn prepare_for_interactive_launch(&mut self) -> bool {
        if self.provider.model.is_none() {
            self.provider.model = Some(DEFAULT_STARTER_MODEL.to_string());
        }
        let needs_key = self.provider.kind() != ProviderKind::Ollama
            && self.provider.api_key.as_deref().unwrap_or("").is_empty();
        if needs_key {
            self.provider.api_key = Some(String::new());
        }
        needs_key
    }

    /// Map legacy flat fields (anthropic_api_key / openai_* / top-level
    /// model) into the modern `provider` block, but only where the modern
    /// block left a gap. No-op for configs already in the new shape.
    pub fn normalize_legacy(&mut self) {
        if self.provider.model.is_none() {
            if let Some(m) = self.legacy_model.take() {
                self.provider.model = Some(m);
            }
        }
        if self.provider.api_key.is_none() {
            match self.provider.kind() {
                ProviderKind::Anthropic => {
                    if let Some(k) = self.legacy_anthropic_api_key.take() {
                        self.provider.api_key = Some(k);
                    }
                }
                ProviderKind::Openai => {
                    if let Some(k) = self.legacy_openai_api_key.take() {
                        self.provider.api_key = Some(k);
                    }
                    if self.provider.base_url.is_none() {
                        if let Some(u) = self.legacy_openai_base_url.take() {
                            self.provider.base_url = Some(u);
                        }
                    }
                }
                ProviderKind::Ollama => {}
            }
        }
        // Tool names were unified to CamelCase (browser_read → BrowserRead,
        // run_check → RunCheck, …). Permission lists written before that —
        // user config and the machine-managed project state's always-allow
        // grants — keep working by normalizing here, at the single choke
        // point every load path funnels through.
        for list in [
            &mut self.permissions.allow,
            &mut self.permissions.deny,
            &mut self.permissions.ask,
        ] {
            for entry in list.iter_mut() {
                let canonical = canonical_tool_name(entry);
                if canonical != entry {
                    *entry = canonical.to_string();
                }
            }
        }
    }

    /// Whether the background self-updater runs (checks GitHub Releases and
    /// swaps in a newer build for the next launch). Defaults to `true`.
    pub fn auto_update(&self) -> bool {
        self.auto_update.unwrap_or(true)
    }

    /// Whether to inject the skill-invocation discipline block into the system
    /// prompt (when skills are available). Defaults to `true`.
    pub fn skill_discipline(&self) -> bool {
        self.skill_discipline.unwrap_or(true)
    }

    /// Whether to inject the OpenSpec-workflow awareness block into the system
    /// prompt (when the project uses OpenSpec). Defaults to `true`.
    pub fn openspec_awareness(&self) -> bool {
        self.openspec_awareness.unwrap_or(true)
    }

    /// Whether to inject the repository map (tracked-file counts per
    /// directory) into the system prompt. Defaults to `true`.
    pub fn repo_map(&self) -> bool {
        self.repo_map.unwrap_or(true)
    }

    /// Whether RTK-style Bash stdout compression is on. Default true.
    pub fn compress_output(&self) -> bool {
        self.tools.compress_output.unwrap_or(true)
    }

    /// Per-call MCP tool timeout. `None` means the built-in 60s default; a
    /// configured `0` disables the local timeout entirely.
    pub fn mcp_tool_timeout(&self) -> Option<std::time::Duration> {
        match self.mcp_tool_timeout_secs {
            None => Some(std::time::Duration::from_secs(
                crate::mcp::DEFAULT_TOOL_TIMEOUT_SECS,
            )),
            Some(0) => None,
            Some(secs) => Some(std::time::Duration::from_secs(secs)),
        }
    }

    /// Fill missing provider connection details from env vars. Anthropic /
    /// OpenAI take an api key; Ollama takes a host (which build_provider
    /// reads from base_url, NOT api_key).
    pub fn apply_env_fallbacks(&mut self) {
        match self.provider.kind() {
            ProviderKind::Ollama => {
                if self.provider.base_url.is_none() {
                    if let Ok(v) = std::env::var("OLLAMA_HOST") {
                        if !v.is_empty() {
                            self.provider.base_url = Some(v);
                        }
                    }
                }
            }
            kind => {
                if self.provider.api_key.is_none() {
                    if let Ok(v) = std::env::var(env_key_for(kind)) {
                        if !v.is_empty() {
                            self.provider.api_key = Some(v);
                        }
                    }
                }
            }
        }
    }

    /// Build a complete active `ProviderConfig` for `model_id` by finding its
    /// owning entry in `providers` and combining the shared provider credentials
    /// with the per-model override. Match priority: a provider whose `models`
    /// map contains the id, then an entry keyed by the id, then an entry whose
    /// `model` equals the id (the legacy flat-per-model shape). Returns `None`
    /// when no provider owns the model. The result never carries a `models` map.
    ///
    /// When the same model id appears under more than one provider, prefer the
    /// owner whose resolved config matches the active provider. This preserves
    /// an explicit named-provider selection instead of silently rebinding the
    /// model to the first group. If the active config cannot disambiguate the
    /// owners, insertion order remains the deterministic fallback.
    /// The `providers`-map key owning the active model — its `models` map
    /// contains it, its `model` equals it, or it is keyed by it. `None` when the
    /// active model belongs to no configured group. For catalog-backed providers
    /// this key equals the models.dev provider id, so it scopes catalog lookups.
    pub fn active_provider_key(&self) -> Option<&str> {
        let model = self.provider.model.as_deref()?;
        let owns_model = |key: &str, entry: &ProviderConfig| {
            entry.models.contains_key(model)
                || entry.model.as_deref() == Some(model)
                || key == model
        };

        // A provider selected through `resolve_named_provider[_model]` carries
        // the fully resolved shared + model-specific fields. Match that exact
        // shape first; this also distinguishes two endpoints exposing the same
        // wire model id but declaring different per-model overrides.
        let mut active = self.provider.clone();
        active.models.clear();
        for (key, entry) in &self.providers {
            if !owns_model(key, entry) {
                continue;
            }
            if self
                .resolve_named_provider_model(key, model)
                .is_some_and(|resolved| resolved == active)
            {
                return Some(key.as_str());
            }
        }

        // A top-level active provider may add model-specific fields or an env
        // fallback after it was resolved. The endpoint identity still pins the
        // intended owner even when the complete structs no longer compare equal.
        for (key, entry) in &self.providers {
            if !owns_model(key, entry) {
                continue;
            }
            if self
                .resolve_named_provider_model(key, model)
                .is_some_and(|resolved| {
                    resolved.kind() == active.kind()
                        && resolved.api_key == active.api_key
                        && resolved.base_url == active.base_url
                        && resolved.dialect == active.dialect
                })
            {
                return Some(key.as_str());
            }
        }

        self.providers
            .iter()
            .find(|(key, entry)| owns_model(key, entry))
            .map(|(key, _)| key.as_str())
    }

    pub fn resolve_model_provider(&self, model_id: &str) -> Option<ProviderConfig> {
        // 1. A multi-model provider whose `models` map lists the id.
        for entry in self.providers.values() {
            if let Some(ov) = entry.models.get(model_id) {
                let mut p = entry.clone();
                p.models = IndexMap::new();
                p.model = Some(model_id.to_string());
                ov.apply_to(&mut p);
                return Some(p);
            }
        }
        // 2. An entry keyed by the id (the common single-model shape).
        if let Some(entry) = self.providers.get(model_id) {
            let mut p = entry.clone();
            p.models = IndexMap::new();
            if p.model.is_none() {
                p.model = Some(model_id.to_string());
            }
            return Some(p);
        }
        // 3. An entry whose own `model` equals the id.
        if let Some(entry) = self
            .providers
            .values()
            .find(|p| p.model.as_deref() == Some(model_id))
        {
            let mut p = entry.clone();
            p.models = IndexMap::new();
            return Some(p);
        }
        None
    }

    /// Record a freshly connected provider+model into the `providers` map so
    /// one provider entry accumulates several models instead of duplicating
    /// credentials per model. `group_key` is the provider's id; `active` is the
    /// resolved session config (its `model` is the model being connected);
    /// `model_override` is stored under `providers[group_key].models[model]`.
    /// A pre-existing single-`model` entry is migrated into the `models` map
    /// first. The active `provider` is then set to JUST the model name (no
    /// duplicated credentials) — the full config is reconstructed from the map
    /// at load time.
    pub fn connect_provider(
        &mut self,
        group_key: &str,
        active: ProviderConfig,
        model_override: ModelOverride,
    ) {
        let model = active.model.clone();
        {
            let entry = self.providers.entry(group_key.to_string()).or_default();
            // Share the credentials at the provider level (latest wins).
            if active.r#type.is_some() {
                entry.r#type = active.r#type;
            }
            if active.api_key.is_some() {
                entry.api_key = active.api_key.clone();
            }
            if active.base_url.is_some() {
                entry.base_url = active.base_url.clone();
            }
            if active.dialect.is_some() {
                entry.dialect = active.dialect.clone();
            }
            // Migrate a legacy single-model entry into the map before adding the
            // new one. Its per-model fields (context/max-output/prices) move into
            // ITS override — leaving them on the parent would silently apply them
            // to every model under this provider.
            if let Some(old) = entry.model.take() {
                let old_override = ModelOverride {
                    context_window: entry.context_window.take(),
                    profile: entry.profile.take(),
                    max_output_tokens: entry.max_output_tokens.take(),
                    supports_images: entry.supports_images.take(),
                    input_price: entry.input_price.take(),
                    output_price: entry.output_price.take(),
                    cache_read_price: entry.cache_read_price.take(),
                    cache_write_price: entry.cache_write_price.take(),
                };
                entry.models.entry(old).or_insert(old_override);
            }
            if let Some(ref m) = model {
                entry.models.insert(m.clone(), model_override);
            }
        }
        // The active selection records ONLY the model name; the full config
        // (credentials + per-model override) lives in the `providers` map above
        // and is reconstructed by `resolve_provider_from_map` at load time.
        self.provider = ProviderConfig {
            model,
            ..Default::default()
        };
    }

    /// Resolve a named provider entry into a complete active `ProviderConfig`:
    /// adopt its shared credentials, default to its first model (its top-level
    /// `model`, else the first key in its `models` map), apply that model's
    /// override, and drop the `models` map. `None` if the name isn't configured.
    pub fn resolve_named_provider(&self, name: &str) -> Option<ProviderConfig> {
        let entry = self.providers.get(name)?;
        let default_model = entry
            .model
            .clone()
            .or_else(|| entry.models.keys().next().cloned());
        match default_model {
            Some(m) => self.resolve_named_provider_model(name, &m),
            None => {
                let mut p = entry.clone();
                p.models = IndexMap::new();
                Some(p)
            }
        }
    }

    /// Resolve a named provider entry to a SPECIFIC model: adopt its shared
    /// credentials, set the active model to `model_id`, apply that model's
    /// override (if it lives in the entry's `models` map; absent → no override,
    /// for a custom model under the provider's credentials), and drop the map.
    /// `None` if the name isn't configured. Used for `--provider X --model Y` so
    /// Y gets Y's per-model settings, not the default model's.
    pub fn resolve_named_provider_model(
        &self,
        name: &str,
        model_id: &str,
    ) -> Option<ProviderConfig> {
        let entry = self.providers.get(name)?;
        let mut p = entry.clone();
        if let Some(ov) = entry.models.get(model_id) {
            ov.apply_to(&mut p);
        }
        p.model = Some(model_id.to_string());
        p.models = IndexMap::new();
        Some(p)
    }

    /// When the active `provider` has no API key but the `providers` map has a
    /// matching entry, adopt that entry's settings into the active provider so a
    /// configured `providers` map "just works" without an explicit `--provider`.
    /// With an active model, the owning entry is found via
    /// [`Self::resolve_model_provider`] (its `models` map, its key, then its
    /// `model`); with no active model, the FIRST configured provider (file order)
    /// is resolved via [`Self::resolve_named_provider`]. Already-set active
    /// fields win; the entry only fills the gaps. No-op when the active provider
    /// already has a key.
    /// Record a `/model` switch in the active `provider`. When the model is
    /// owned by a configured provider, the active provider is reduced to JUST
    /// its name (the full config is reconstructed from the `providers` map on
    /// load — matching [`Self::connect_provider`]). For a custom model not in
    /// the map, the model is set on the current active provider so its
    /// credentials are preserved.
    pub fn set_active_model(&mut self, model_id: &str) {
        if self.resolve_model_provider(model_id).is_some() {
            self.provider = ProviderConfig {
                model: Some(model_id.to_string()),
                ..Default::default()
            };
        } else {
            self.provider.model = Some(model_id.to_string());
        }
    }

    pub fn resolve_provider_from_map(&mut self) {
        if self.provider.api_key.is_some() || self.providers.is_empty() {
            return;
        }
        let pick = match self.provider.model.clone() {
            // An explicit active model: adopt the entry that owns it (its map
            // key, its `model`, or its `models` map). If none matches, leave it
            // (the user named a model that isn't configured — don't silently
            // swap in a different provider).
            Some(m) => self.resolve_model_provider(&m),
            // No active model: default to the FIRST configured provider (file
            // order, preserved by IndexMap), resolved to its default model and
            // that model's override.
            None => self
                .providers
                .keys()
                .next()
                .cloned()
                .and_then(|k| self.resolve_named_provider(&k)),
        };
        let Some(p) = pick else {
            return;
        };
        let active = &mut self.provider;
        // Active fields win; the map entry fills the gaps (esp. the api key).
        active.api_key = active.api_key.take().or(p.api_key);
        active.r#type = active.r#type.or(p.r#type);
        active.base_url = active.base_url.take().or(p.base_url);
        active.model = active.model.take().or(p.model);
        active.dialect = active.dialect.take().or(p.dialect);
        active.context_window = active.context_window.or(p.context_window);
        active.max_output_tokens = active.max_output_tokens.or(p.max_output_tokens);
        active.supports_images = active.supports_images.or(p.supports_images);
        active.reasoning = active.reasoning.or(p.reasoning);
        active.input_price = active.input_price.or(p.input_price);
        active.output_price = active.output_price.or(p.output_price);
        active.cache_read_price = active.cache_read_price.or(p.cache_read_price);
        active.cache_write_price = active.cache_write_price.or(p.cache_write_price);
    }

    /// Shallow-merge `other` (higher priority) onto self. Each provider
    /// field overrides only when explicitly present in `other` — including
    /// `type`, so a project config that switches provider kind without
    /// repeating the api key is honored (it inherits the global key).
    pub fn merge_from(&mut self, other: ZodeConfig) {
        let op = other.provider;
        if op.r#type.is_some() {
            self.provider.r#type = op.r#type;
        }
        if op.api_key.is_some() {
            self.provider.api_key = op.api_key;
        }
        if op.base_url.is_some() {
            self.provider.base_url = op.base_url;
        }
        if op.model.is_some() {
            self.provider.model = op.model;
        }
        if op.dialect.is_some() {
            self.provider.dialect = op.dialect;
        }
        if op.supports_images.is_some() {
            self.provider.supports_images = op.supports_images;
        }
        if op.reasoning.is_some() {
            self.provider.reasoning = op.reasoning;
        }
        if op.profile.is_some() {
            self.provider.profile = op.profile;
        }
        // Sizing knobs merge like every other provider field — a project
        // config that pins a smaller window or output cap for its model
        // must not be silently dropped (the active provider would keep the
        // global layer's values, and compaction/trim decisions would run
        // against the wrong window).
        if op.context_window.is_some() {
            self.provider.context_window = op.context_window;
        }
        if op.max_output_tokens.is_some() {
            self.provider.max_output_tokens = op.max_output_tokens;
        }
        self.providers.extend(other.providers);
        if other.images.mode.is_some() {
            self.images.mode = other.images.mode;
        }
        if other.images.vision_provider.is_some() {
            self.images.vision_provider = other.images.vision_provider;
        }
        if other.images.vision_prompt.is_some() {
            self.images.vision_prompt = other.images.vision_prompt;
        }
        if other.theme.is_some() {
            self.theme = other.theme;
        }
        // Permission lists ACCUMULATE across layers (global → project →
        // project state) — a deeper layer adds rules, it doesn't drop the
        // outer ones. Dedup so repeated entries don't pile up.
        extend_dedup(&mut self.permissions.allow, other.permissions.allow);
        extend_dedup(&mut self.permissions.deny, other.permissions.deny);
        extend_dedup(&mut self.permissions.ask, other.permissions.ask);
        for rule in other.permissions.rules {
            if !self.permissions.rules.contains(&rule) {
                self.permissions.rules.push(rule);
            }
        }
        if other.sandbox.enabled.is_some() {
            self.sandbox.enabled = other.sandbox.enabled;
        }
        if other.sandbox.mode.is_some() {
            self.sandbox.mode = other.sandbox.mode;
        }
        if other.sandbox.network.is_some() {
            self.sandbox.network = other.sandbox.network;
        }
        if !other.sandbox.writable_roots.is_empty() {
            self.sandbox.writable_roots = other.sandbox.writable_roots;
        }
        if other.sandbox.restrict_reads.is_some() {
            self.sandbox.restrict_reads = other.sandbox.restrict_reads;
        }
        if other.sandbox.windows_tier.is_some() {
            self.sandbox.windows_tier = other.sandbox.windows_tier;
        }
        self.sandbox.profiles.extend(other.sandbox.profiles);
        if other.yolo.is_some() {
            self.yolo = other.yolo;
        }
        if other.max_output_tokens.is_some() {
            self.max_output_tokens = other.max_output_tokens;
        }
        if other.max_iterations.is_some() {
            self.max_iterations = other.max_iterations;
        }
        if other.subagent_max_iterations.is_some() {
            self.subagent_max_iterations = other.subagent_max_iterations;
        }
        if other.max_api_retries.is_some() {
            self.max_api_retries = other.max_api_retries;
        }
        if other.mcp_tool_timeout_secs.is_some() {
            self.mcp_tool_timeout_secs = other.mcp_tool_timeout_secs;
        }
        if other.auto_update.is_some() {
            self.auto_update = other.auto_update;
        }
        if other.context_window.is_some() {
            self.context_window = other.context_window;
        }
        if other.currency.is_some() {
            self.currency = other.currency;
        }
        if other.language.is_some() {
            self.language = other.language;
        }
        if other.goal.is_some() {
            self.goal = other.goal;
        }
        if other.effort.is_some() {
            self.effort = other.effort;
        }
        if other.show_thinking.is_some() {
            self.show_thinking = other.show_thinking;
        }
        if other.show_tool_details.is_some() {
            self.show_tool_details = other.show_tool_details;
        }
        if other.mouse_capture.is_some() {
            self.mouse_capture = other.mouse_capture;
        }
        if other.repo_map.is_some() {
            self.repo_map = other.repo_map;
        }
        if other.autonomous_orchestration.is_some() {
            self.autonomous_orchestration = other.autonomous_orchestration;
        }
        if other.skill_discipline.is_some() {
            self.skill_discipline = other.skill_discipline;
        }
        if other.openspec_awareness.is_some() {
            self.openspec_awareness = other.openspec_awareness;
        }
        if other.temperature.is_some() {
            self.temperature = other.temperature;
        }
        if other.prompt_cache.is_some() {
            self.prompt_cache = other.prompt_cache;
        }
        if !other.plugins.disabled.is_empty() {
            self.plugins.disabled = other.plugins.disabled;
        }
        if other.tools.compress_output.is_some() {
            self.tools.compress_output = other.tools.compress_output;
        }
        // Project LSP servers extend (and override same-key) the global set.
        self.lsp.servers.extend(other.lsp.servers);
        // OpenPencil: each field present in the project layer overrides global.
        let o = other.openpencil;
        self.openpencil.enabled = o.enabled.or(self.openpencil.enabled);
        self.openpencil.op_path = o.op_path.or(self.openpencil.op_path.take());
        self.openpencil.install_command =
            o.install_command.or(self.openpencil.install_command.take());
        self.openpencil.release_tag = o.release_tag.or(self.openpencil.release_tag.take());
        self.openpencil.auto_install = o.auto_install.or(self.openpencil.auto_install);
        self.openpencil.auto_launch_gui = o.auto_launch_gui.or(self.openpencil.auto_launch_gui);
        self.openpencil.launch_command = o.launch_command.or(self.openpencil.launch_command.take());
        self.openpencil.headless_fallback =
            o.headless_fallback.or(self.openpencil.headless_fallback);
        self.openpencil.default_doc = o.default_doc.or(self.openpencil.default_doc.take());
        self.openpencil.connect_timeout_ms =
            o.connect_timeout_ms.or(self.openpencil.connect_timeout_ms);

        // Browser: each field present in the project layer overrides global.
        let b = other.browser;
        self.browser.enabled = b.enabled.or(self.browser.enabled);
        self.browser.executable = b.executable.or(self.browser.executable.take());
        self.browser.headless = b.headless.or(self.browser.headless);
        self.browser.profile_dir = b.profile_dir.or(self.browser.profile_dir.take());
        self.browser.default_target = b.default_target.or(self.browser.default_target.take());
        self.browser.viewport = b.viewport.or(self.browser.viewport.take());
        self.browser.extension_ids = b.extension_ids.or(self.browser.extension_ids.take());

        // Desktop: each field present in the project layer overrides global.
        let d = other.desktop;
        self.desktop.enabled = d.enabled.or(self.desktop.enabled);
        self.desktop.snapshot_max_nodes = d.snapshot_max_nodes.or(self.desktop.snapshot_max_nodes);
        self.desktop.ghost_cursor = d.ghost_cursor.or(self.desktop.ghost_cursor);
        self.desktop.esc_cancel = d.esc_cancel.or(self.desktop.esc_cancel);
        self.desktop.overlay_helper_path = d
            .overlay_helper_path
            .or(self.desktop.overlay_helper_path.take());

        // externalAgents: scalars overlay by presence; the agents map merges
        // by key with same-key entries replaced wholesale (no field-level
        // deep merge — avoids extraArgs accumulation ambiguity).
        let ea = other.external_agents;
        self.external_agents.enabled = ea.enabled.or(self.external_agents.enabled);
        self.external_agents.timeout_secs = ea.timeout_secs.or(self.external_agents.timeout_secs);
        self.external_agents.max_concurrent =
            ea.max_concurrent.or(self.external_agents.max_concurrent);
        self.external_agents.agents.extend(ea.agents);

        let n = other.noema;
        self.noema.enabled = n.enabled.or(self.noema.enabled);
        self.noema.root = n.root.or(self.noema.root.take());
        self.noema.user = n.user.or(self.noema.user.take());
        self.noema.auto_remember = n.auto_remember.or(self.noema.auto_remember);
        self.noema.auto_extract = n.auto_extract.or(self.noema.auto_extract);
        self.noema.write_policy = n.write_policy.or(self.noema.write_policy.take());
        self.noema.extract_model = n.extract_model.or(self.noema.extract_model.take());
        self.noema.extract_scan_assistant = n
            .extract_scan_assistant
            .or(self.noema.extract_scan_assistant);
        self.noema.max_memories_per_turn =
            n.max_memories_per_turn.or(self.noema.max_memories_per_turn);
        self.noema.extract_max_input_chars = n
            .extract_max_input_chars
            .or(self.noema.extract_max_input_chars);

        let c = other.compact;
        self.compact.microcompact = c.microcompact.or(self.compact.microcompact);
        self.compact.memory_sink = c.memory_sink.or(self.compact.memory_sink);
        self.compact.restore_files = c.restore_files.or(self.compact.restore_files);
        self.compact.restore_files_budget =
            c.restore_files_budget.or(self.compact.restore_files_budget);
        self.compact.recall_after_compact =
            c.recall_after_compact.or(self.compact.recall_after_compact);
        self.compact.auto_compact_percent =
            c.auto_compact_percent.or(self.compact.auto_compact_percent);

        let w = other.background_watchdog;
        self.background_watchdog.enabled = w.enabled.or(self.background_watchdog.enabled);
        self.background_watchdog.inactivity_timeout_secs = w
            .inactivity_timeout_secs
            .or(self.background_watchdog.inactivity_timeout_secs);
        self.background_watchdog.max_runtime_secs = w
            .max_runtime_secs
            .or(self.background_watchdog.max_runtime_secs);
        self.background_watchdog.abort_grace_secs = w
            .abort_grace_secs
            .or(self.background_watchdog.abort_grace_secs);
        self.background_watchdog.max_retries =
            w.max_retries.or(self.background_watchdog.max_retries);
        self.background_watchdog.initial_backoff_secs = w
            .initial_backoff_secs
            .or(self.background_watchdog.initial_backoff_secs);
        self.background_watchdog.max_backoff_secs = w
            .max_backoff_secs
            .or(self.background_watchdog.max_backoff_secs);
    }
}

fn env_key_for(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "ANTHROPIC_API_KEY",
        ProviderKind::Openai => "OPENAI_API_KEY",
        ProviderKind::Ollama => "OLLAMA_HOST",
    }
}

pub struct ConfigManager;

impl ConfigManager {
    /// Config dir: `$ZODE_CONFIG_DIR` or `~/.zode`.
    pub fn config_dir() -> Result<PathBuf, CoreError> {
        if let Ok(dir) = std::env::var("ZODE_CONFIG_DIR") {
            if !dir.is_empty() {
                return Ok(PathBuf::from(dir));
            }
        }
        let home = dirs::home_dir()
            .ok_or_else(|| CoreError::Other("cannot resolve home directory".into()))?;
        Ok(home.join(".zode"))
    }

    fn global_path() -> Result<PathBuf, CoreError> {
        Ok(Self::config_dir()?.join("config.json"))
    }

    /// Load only the global config (missing file -> defaults).
    pub fn load_global() -> Result<ZodeConfig, CoreError> {
        let path = Self::global_path()?;
        Self::load_file(&path)
    }

    /// Persist the browser target selected from the interactive `/browser`
    /// controls. This is a read-modify-write so unrelated global settings are
    /// preserved, and the normal atomic config writer publishes the update.
    pub fn persist_browser_default_target(target: &str) -> Result<(), CoreError> {
        if !matches!(target, "managed" | "bridge") {
            return Err(CoreError::Other(format!(
                "invalid browser target {target:?} (expected managed | bridge)"
            )));
        }
        let mut cfg = Self::load_global()?;
        cfg.browser.default_target = Some(target.to_string());
        Self::save_global(&cfg)
    }

    /// Persist whether built-in browser control is enabled by default.
    /// Keep this separate from the `tools:browser` plugin-group switch: the
    /// `/browser` panel exposes the browser config itself, while `/plugin`
    /// remains responsible for plugin-group policy.
    pub fn persist_browser_enabled(enabled: bool) -> Result<(), CoreError> {
        let mut cfg = Self::load_global()?;
        cfg.browser.enabled = Some(enabled);
        Self::save_global(&cfg)
    }

    /// On first run, drop a starter `config.json` into the global config dir so
    /// the user has a real file to edit (provider + a default model; no api key,
    /// so an env key still wins via the fallback). Returns `Ok(true)` when a
    /// file was written, `Ok(false)` when one already existed. Callers treat
    /// this as best-effort — a read-only home must never block startup.
    pub fn ensure_default_global() -> Result<bool, CoreError> {
        let path = Self::global_path()?;
        if path.exists() {
            return Ok(false);
        }
        let mut cfg = ZodeConfig::default();
        cfg.provider.r#type = Some(ProviderKind::Anthropic);
        cfg.provider.model = Some(DEFAULT_STARTER_MODEL.to_string());
        Self::save_global(&cfg)?;
        Ok(true)
    }

    fn load_file(path: &Path) -> Result<ZodeConfig, CoreError> {
        match std::fs::read_to_string(path) {
            Ok(s) => Ok(serde_json::from_str(&s)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ZodeConfig::default()),
            Err(e) => Err(CoreError::Io(e)),
        }
    }

    /// Effective config: global + project (`<cwd>/.zode/config.json`) +
    /// project STATE (`<cwd>/.zode/state.json`, machine-managed) + env api-key
    /// fallback. Later layers override / accumulate over earlier ones.
    pub fn load(cwd: &Path) -> Result<ZodeConfig, CoreError> {
        let mut cfg = Self::load_global()?;
        let project = cwd.join(".zode").join("config.json");
        if project.exists() {
            cfg.merge_from(Self::load_file(&project)?);
        }
        // Project state: zode writes this (sandbox toggles, allow-always
        // permissions) — read it last so it reflects the user's latest choices.
        let state = Self::project_state_path(cwd);
        if state.exists() {
            cfg.merge_from(Self::load_file(&state)?);
        }
        cfg.normalize_legacy();
        // Reconstruct a minimal active provider (just `{ model }`) from the
        // `providers` map BEFORE the env fallback — otherwise an env API key
        // would fill `provider.api_key` and make `resolve_provider_from_map`
        // early-return, stranding the session without the provider's base URL /
        // dialect / context window.
        cfg.resolve_provider_from_map();
        cfg.apply_env_fallbacks();
        Ok(cfg)
    }

    /// Machine-managed project state file (separate from the user-edited
    /// `config.json`, like Claude Code's `settings.local.json`).
    pub fn project_state_path(cwd: &Path) -> PathBuf {
        cwd.join(".zode").join("state.json")
    }

    /// Read the project state file as a JSON object (empty object if missing
    /// or unparseable, so a corrupt file never blocks an update).
    fn read_project_state(cwd: &Path) -> serde_json::Value {
        std::fs::read_to_string(Self::project_state_path(cwd))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .filter(|v| v.is_object())
            .unwrap_or_else(|| serde_json::json!({}))
    }

    /// Update `<cwd>/.zode/state.json` via a closure on its JSON object,
    /// creating `.zode/` as needed. Used to persist sandbox state and
    /// allow-always tool permissions per project so they survive restarts.
    pub fn update_project_state(
        cwd: &Path,
        f: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
    ) -> Result<(), CoreError> {
        std::fs::create_dir_all(cwd.join(".zode"))?;
        let mut state = Self::read_project_state(cwd);
        if let Some(obj) = state.as_object_mut() {
            f(obj);
        }
        std::fs::write(
            Self::project_state_path(cwd),
            serde_json::to_string_pretty(&state)?,
        )?;
        Ok(())
    }

    /// Persist to the global config path (creates the dir if needed). Writes
    /// ATOMICALLY (temp file + rename) so a crash / disk-full mid-write can
    /// never leave a partial `config.json` that a later startup would abort on
    /// — which would lock the user out, the very thing this must avoid. The
    /// temp name is process-unique so concurrent writers don't clobber each
    /// other's temp; the rename then atomically publishes one complete file.
    pub fn save_global(cfg: &ZodeConfig) -> Result<(), CoreError> {
        let dir = Self::config_dir()?;
        std::fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(cfg)?;
        write_atomic(&dir.join("config.json"), json.as_bytes())?;
        Ok(())
    }
}

/// Write `bytes` to `path` atomically: stage in a sibling temp file (same
/// directory, so the rename stays on one filesystem) then rename over the
/// target. `std::fs::rename` replaces the destination atomically on both Unix
/// and Windows, so readers see either the old file or the complete new one,
/// never a half-written mix.
///
/// `pub(crate)` so other stores needing the same guarantee (e.g. the
/// scheduler's `schedules.json`) reuse this instead of a fixed, shared temp
/// path that concurrent writers could interleave (O_TRUNC, no O_EXCL).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    // Per-call counter so two concurrent saves IN THIS PROCESS get distinct
    // temps (pid alone only separates processes); together pid+seq are unique.
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.json".to_string());
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{seq}.tmp", std::process::id()));
    // Best-effort cleanup of our own temp on any failure after creation.
    let result = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Append `add` to `target`, skipping values already present (preserves order).
fn extend_dedup(target: &mut Vec<String>, add: Vec<String>) {
    for v in add {
        if !target.contains(&v) {
            target.push(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_capture_defaults_on_and_overrides() {
        // Unset → capture ON: an alt-screen TUI must consume wheel events or
        // terminals scroll their own viewport and shear the UI.
        let cfg = ZodeConfig::default();
        assert!(cfg.mouse_capture_enabled());

        // Explicit value wins on every platform, camelCase key, and merge
        // layers are presence-based.
        let on: ZodeConfig = serde_json::from_str(r#"{"mouseCapture":true}"#).unwrap();
        assert!(on.mouse_capture_enabled());
        let off: ZodeConfig = serde_json::from_str(r#"{"mouseCapture":false}"#).unwrap();
        assert!(!off.mouse_capture_enabled());
        let mut base = ZodeConfig::default();
        base.merge_from(on);
        assert_eq!(base.mouse_capture, Some(true));
        base.merge_from(ZodeConfig::default());
        assert_eq!(base.mouse_capture, Some(true)); // unset layer preserves
    }

    #[test]
    fn background_watchdog_parses_clamps_and_merges_by_presence() {
        let mut global: ZodeConfig = serde_json::from_str(
            r#"{"backgroundWatchdog":{"enabled":true,"inactivityTimeoutSecs":1,"maxRuntimeSecs":2,"abortGraceSecs":0,"maxRetries":99,"initialBackoffSecs":20,"maxBackoffSecs":2}}"#,
        )
        .unwrap();
        assert!(global.background_watchdog.enabled());
        assert_eq!(global.background_watchdog.inactivity_timeout().as_secs(), 5);
        assert_eq!(global.background_watchdog.max_runtime().as_secs(), 30);
        assert_eq!(global.background_watchdog.abort_grace().as_secs(), 1);
        assert_eq!(global.background_watchdog.max_retries(), 20);
        assert_eq!(global.background_watchdog.max_backoff().as_secs(), 20);

        let project: ZodeConfig = serde_json::from_str(
            r#"{"backgroundWatchdog":{"enabled":false,"maxRetries":0,"initialBackoffSecs":3}}"#,
        )
        .unwrap();
        global.merge_from(project);
        assert!(!global.background_watchdog.enabled());
        assert_eq!(global.background_watchdog.max_retries(), 0);
        assert_eq!(global.background_watchdog.initial_backoff().as_secs(), 3);
        assert_eq!(
            global.background_watchdog.inactivity_timeout().as_secs(),
            5,
            "omitted project field preserves the global layer"
        );

        global.merge_from(ZodeConfig::default());
        assert_eq!(global.background_watchdog.enabled, Some(false));
        assert_eq!(global.background_watchdog.max_retries, Some(0));
    }

    #[test]
    fn merge_from_carries_provider_sizing_knobs() {
        // Regression: a project config pinning a smaller context window or
        // output cap for the active provider must survive the merge — the
        // active provider would otherwise keep the global layer's values
        // and compaction/trim decisions would run against the wrong window.
        let mut global: ZodeConfig = serde_json::from_str(
            r#"{"provider":{"model":"m","contextWindow":200000,"maxOutputTokens":8192}}"#,
        )
        .unwrap();
        let project: ZodeConfig = serde_json::from_str(
            r#"{"provider":{"contextWindow":32000,"maxOutputTokens":4096}}"#,
        )
        .unwrap();
        global.merge_from(project);
        assert_eq!(global.provider.context_window, Some(32000));
        assert_eq!(global.provider.max_output_tokens, Some(4096));
        // Omitted project fields preserve the global layer.
        assert_eq!(global.provider.model.as_deref(), Some("m"));
        // An empty layer leaves everything intact.
        global.merge_from(ZodeConfig::default());
        assert_eq!(global.provider.context_window, Some(32000));
    }

    #[test]
    fn openpencil_camelcase_and_getters() {
        let json = r#"{"openpencil":{"autoLaunchGui":false,"connectTimeoutMs":5000,"releaseTag":"0.9.0"}}"#;
        let cfg: ZodeConfig = serde_json::from_str(json).unwrap();
        assert!(!cfg.openpencil.auto_launch_gui());
        assert_eq!(cfg.openpencil.connect_timeout_ms(), 5000);
        assert_eq!(cfg.openpencil.release_tag(), "0.9.0");
        assert!(cfg.openpencil.enabled()); // absent → default true
        assert_eq!(cfg.openpencil.launch_command(), "op start");
    }

    #[test]
    fn openpencil_merge_is_presence_based() {
        let mut base = ZodeConfig::default();
        base.openpencil.auto_launch_gui = Some(false); // global turns it off
        let mut proj = ZodeConfig::default();
        proj.openpencil.auto_launch_gui = Some(true); // project resets it on
        base.merge_from(proj);
        assert!(base.openpencil.auto_launch_gui()); // project wins (presence)
                                                    // unset-in-project preserves global
        let mut base2 = ZodeConfig::default();
        base2.openpencil.op_path = Some("/g/op".into());
        base2.merge_from(ZodeConfig::default());
        assert_eq!(base2.openpencil.op_path.as_deref(), Some("/g/op"));
    }

    #[test]
    fn noema_config_defaults_on_and_merges_by_presence() {
        let cfg: ZodeConfig = serde_json::from_str(
            r#"{"noema":{"root":"/tmp/mem","user":"kay","autoRemember":false}}"#,
        )
        .unwrap();
        assert!(cfg.noema.enabled());
        assert!(!cfg.noema.auto_remember());
        assert_eq!(cfg.noema.root.as_deref(), Some("/tmp/mem"));
        assert_eq!(cfg.noema.user.as_deref(), Some("kay"));

        let mut base = ZodeConfig::default();
        base.noema.root = Some("/global/mem".into());
        base.noema.enabled = Some(true);
        let mut project = ZodeConfig::default();
        project.noema.enabled = Some(false);
        project.noema.user = Some("project-user".into());
        project.noema.auto_remember = Some(false);
        base.merge_from(project);
        assert!(!base.noema.enabled());
        assert!(!base.noema.auto_remember());
        assert_eq!(base.noema.root.as_deref(), Some("/global/mem"));
        assert_eq!(base.noema.user.as_deref(), Some("project-user"));
    }

    #[test]
    fn noema_auto_extract_defaults_on_and_drives_write_policy() {
        // Absent → on, and the policy is `autosafe` (deterministic & reversible:
        // not `None`, so turning extraction off actively restores review).
        let bare: ZodeConfig = serde_json::from_str(r#"{"noema":{}}"#).unwrap();
        assert!(bare.noema.auto_extract());
        assert_eq!(bare.noema.effective_write_policy(), "autosafe");

        // Enabling extraction implies the autoSafe policy.
        let on: ZodeConfig = serde_json::from_str(r#"{"noema":{"autoExtract":true}}"#).unwrap();
        assert!(on.noema.auto_extract());
        assert_eq!(on.noema.effective_write_policy(), "autosafe");

        // An explicit writePolicy wins (normalized to lowercase) even with
        // extraction off.
        let explicit: ZodeConfig =
            serde_json::from_str(r#"{"noema":{"autoExtract":false,"writePolicy":"Review"}}"#)
                .unwrap();
        assert!(!explicit.noema.auto_extract());
        assert_eq!(explicit.noema.effective_write_policy(), "review");
    }

    #[test]
    fn noema_new_fields_round_trip_and_merge_by_presence() {
        // Absent keys are omitted on serialize (skip_serializing_if).
        let default_json = serde_json::to_string(&NoemaSettings::default()).unwrap();
        assert!(!default_json.contains("autoExtract"));
        assert!(!default_json.contains("writePolicy"));

        // Project layer overrides extraction; absent writePolicy preserves global.
        let mut base = ZodeConfig::default();
        base.noema.write_policy = Some("auto".into());
        let mut project = ZodeConfig::default();
        project.noema.auto_extract = Some(true);
        base.merge_from(project);
        assert!(base.noema.auto_extract());
        assert_eq!(base.noema.write_policy.as_deref(), Some("auto"));
    }

    #[test]
    fn merge_unions_permission_lists() {
        let mut a = ZodeConfig::default();
        a.permissions.allow = vec!["A".into()];
        let mut b = ZodeConfig::default();
        b.permissions.allow = vec!["A".into(), "B".into()]; // overlaps A
        a.merge_from(b);
        // Accumulated + deduped (not replaced, not duplicated).
        assert_eq!(a.permissions.allow, vec!["A".to_string(), "B".to_string()]);
    }

    #[test]
    #[serial_test::serial]
    fn load_resolves_minimal_provider_before_env_fallback() {
        let global = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::fs::write(
            global.path().join("config.json"),
            r#"{
                "provider": { "model": "deepseek-v4-pro" },
                "providers": { "deepseek": {
                    "type": "anthropic",
                    "apiKey": "sk-saved",
                    "baseUrl": "https://api.deepseek.com/anthropic",
                    "models": { "deepseek-v4-pro": { "contextWindow": 1000000 } }
                } }
            }"#,
        )
        .unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", global.path());
        std::env::set_var("ANTHROPIC_API_KEY", "sk-env-should-not-win");

        let cfg = ConfigManager::load(cwd.path()).unwrap();

        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("ZODE_CONFIG_DIR");

        // The minimal active provider must be reconstructed from the providers
        // map BEFORE the env fallback runs — so base URL/context come from the
        // saved provider and the saved key wins over the env var.
        assert_eq!(cfg.provider.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(cfg.provider.api_key.as_deref(), Some("sk-saved"));
        assert_eq!(
            cfg.provider.base_url.as_deref(),
            Some("https://api.deepseek.com/anthropic")
        );
        assert_eq!(cfg.provider.context_window, Some(1_000_000));
    }

    #[test]
    #[serial_test::serial]
    fn project_state_persists_sandbox_and_permissions() {
        let global = tempfile::tempdir().unwrap(); // empty global config
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", global.path());

        ConfigManager::update_project_state(cwd.path(), |s| {
            s.insert("sandbox".into(), serde_json::json!({"enabled": false}));
            s.insert("yolo".into(), serde_json::json!(true));
            s.insert(
                "permissions".into(),
                serde_json::json!({"allow": ["Bash", "FileWrite"]}),
            );
        })
        .unwrap();
        assert!(ConfigManager::project_state_path(cwd.path()).exists());

        let cfg = ConfigManager::load(cwd.path()).unwrap();
        std::env::remove_var("ZODE_CONFIG_DIR");

        assert_eq!(cfg.sandbox.enabled, Some(false), "sandbox state loaded");
        assert_eq!(cfg.yolo, Some(true), "yolo state loaded");
        assert!(cfg.permissions.allow.contains(&"Bash".to_string()));
        assert!(cfg.permissions.allow.contains(&"FileWrite".to_string()));
    }

    #[test]
    #[serial_test::serial]
    fn model_profile_resolves_override_then_name_heuristic() {
        reset_learned_profiles_for_test();
        let mut p = ProviderConfig::default();
        // Heuristic: flash/mini/haiku-class names are lite.
        assert_eq!(model_profile(&p, "deepseek-v4-flash"), ModelProfile::Lite);
        assert_eq!(model_profile(&p, "gpt-5-mini"), ModelProfile::Lite);
        assert_eq!(model_profile(&p, "claude-haiku-4-5"), ModelProfile::Lite);
        assert_eq!(model_profile(&p, "deepseek-v4"), ModelProfile::Standard);
        assert_eq!(model_profile(&p, "claude-opus-5"), ModelProfile::Standard);
        // Segment matching, not substrings: MiniMax is not "mini".
        assert_eq!(model_profile(&p, "MiniMax-M1"), ModelProfile::Standard);
        assert_eq!(model_profile(&p, "kimi-flashback"), ModelProfile::Standard);
        // Explicit config beats the heuristic, both ways.
        p.profile = Some("standard".into());
        assert_eq!(
            model_profile(&p, "deepseek-v4-flash"),
            ModelProfile::Standard
        );
        p.profile = Some("lite".into());
        assert_eq!(model_profile(&p, "deepseek-v4"), ModelProfile::Lite);
        // Unknown strings fail open to standard.
        p.profile = Some("???".into());
        assert_eq!(model_profile(&p, "x-flash"), ModelProfile::Standard);
    }

    #[test]
    #[serial_test::serial]
    fn learned_verdicts_flow_into_model_profile_but_lose_to_explicit_config() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        reset_learned_profiles_for_test();

        let p = ProviderConfig::default();
        assert_eq!(model_profile(&p, "glm-5.2"), ModelProfile::Standard);
        // Runtime evidence marks it lite — idempotently, and persisted.
        assert!(learn_model_lite("glm-5.2"));
        assert!(!learn_model_lite("glm-5.2"), "second learn is a no-op");
        assert_eq!(model_profile(&p, "glm-5.2"), ModelProfile::Lite);
        assert!(dir.path().join("model-profiles.json").exists());
        // Explicit standard still wins over the learned verdict.
        let forced = ProviderConfig {
            profile: Some("standard".into()),
            ..Default::default()
        };
        assert_eq!(model_profile(&forced, "glm-5.2"), ModelProfile::Standard);

        std::env::remove_var("ZODE_CONFIG_DIR");
        reset_learned_profiles_for_test();
    }

    #[test]
    fn price_overrides_from_config_fields() {
        // No price fields → None (cost falls back to the built-in catalog).
        assert!(ProviderConfig::default().price_overrides().is_none());
        // Any field set → Some, parsed from a camelCase config.
        let cfg: ProviderConfig =
            serde_json::from_str(r#"{"inputPrice":0.28,"outputPrice":1.1}"#).unwrap();
        assert_eq!(cfg.input_price, Some(0.28));
        assert_eq!(cfg.output_price, Some(1.1));
        assert!(cfg.price_overrides().is_some());
    }

    #[test]
    fn parse_minimax_anthropic_config() {
        let json = r#"{
            "provider": {
                "type": "anthropic",
                "apiKey": "sk-test",
                "baseUrl": "https://api.minimaxi.com/anthropic/v1",
                "model": "MiniMax-M1"
            }
        }"#;
        let cfg: ZodeConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.provider.kind(), ProviderKind::Anthropic);
        assert_eq!(cfg.provider.api_key.as_deref(), Some("sk-test"));
        assert_eq!(
            cfg.provider.base_url.as_deref(),
            Some("https://api.minimaxi.com/anthropic/v1")
        );
        assert_eq!(cfg.provider.model.as_deref(), Some("MiniMax-M1"));
    }

    #[test]
    fn empty_config_roundtrips_with_defaults() {
        let cfg: ZodeConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.provider.kind(), ProviderKind::Anthropic);
        assert!(cfg.theme.is_none());
        let s = serde_json::to_string(&cfg).unwrap();
        let _back: ZodeConfig = serde_json::from_str(&s).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn config_dir_respects_env_override() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        assert_eq!(ConfigManager::config_dir().unwrap(), dir.path());
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
    #[serial_test::serial]
    fn ensure_default_global_writes_once_then_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        let path = dir.path().join("config.json");
        assert!(!path.exists());
        // First call writes a parseable starter config with a default model.
        assert!(ConfigManager::ensure_default_global().unwrap());
        assert!(path.exists());
        let cfg = ConfigManager::load_global().unwrap();
        assert_eq!(cfg.provider.kind(), ProviderKind::Anthropic);
        assert_eq!(cfg.provider.model.as_deref(), Some(DEFAULT_STARTER_MODEL));
        assert!(cfg.provider.api_key.is_none(), "no key, so env still wins");
        // Second call must not overwrite (and reports it did nothing).
        std::fs::write(&path, "{\"theme\":\"mono\"}").unwrap();
        assert!(!ConfigManager::ensure_default_global().unwrap());
        assert_eq!(
            ConfigManager::load_global().unwrap().theme.as_deref(),
            Some("mono")
        );
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
    #[serial_test::serial]
    fn save_global_is_atomic_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        let cfg = ZodeConfig {
            theme: Some("mono".into()),
            ..Default::default()
        };
        ConfigManager::save_global(&cfg).unwrap();
        // The published file loads, and no staging temp is left behind.
        assert_eq!(
            ConfigManager::load_global().unwrap().theme.as_deref(),
            Some("mono")
        );
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "stray temp files: {leftovers:?}");
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
    fn prepare_for_interactive_launch_reports_and_patches_missing_creds() {
        // Anthropic with no key/model: patched to launch, reports needs-setup.
        let mut cfg = ZodeConfig::default();
        assert!(cfg.prepare_for_interactive_launch());
        assert_eq!(cfg.provider.model.as_deref(), Some(DEFAULT_STARTER_MODEL));
        assert_eq!(cfg.provider.api_key.as_deref(), Some(""));

        // A real key means no setup needed (model still defaulted if absent).
        let mut keyed = ZodeConfig::default();
        keyed.provider.api_key = Some("sk-real".into());
        assert!(!keyed.prepare_for_interactive_launch());
        assert_eq!(keyed.provider.api_key.as_deref(), Some("sk-real"));

        // Ollama needs no key, so it never reports as needing setup.
        let mut ollama = ZodeConfig::default();
        ollama.provider.r#type = Some(ProviderKind::Ollama);
        assert!(!ollama.prepare_for_interactive_launch());
        assert!(ollama.provider.api_key.is_none());
    }

    #[test]
    #[serial_test::serial]
    fn save_then_load_global_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        let cfg = ZodeConfig {
            theme: Some("cyberpunk".to_string()),
            ..Default::default()
        };
        ConfigManager::save_global(&cfg).unwrap();
        let loaded = ConfigManager::load_global().unwrap();
        assert_eq!(loaded.theme.as_deref(), Some("cyberpunk"));
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
    fn legacy_flat_config_normalizes_into_provider() {
        // The Zig/TS-era flat shape that real users still have on disk.
        let json = r#"{
            "anthropic_api_key": "sk-legacy",
            "openai_api_key": "sk-openai",
            "model": "claude-sonnet-4-6",
            "theme": "cyberpunk",
            "max_turns": 50
        }"#;
        let mut cfg: ZodeConfig = serde_json::from_str(json).unwrap();
        cfg.normalize_legacy();
        assert_eq!(cfg.provider.kind(), ProviderKind::Anthropic);
        assert_eq!(cfg.provider.api_key.as_deref(), Some("sk-legacy"));
        assert_eq!(cfg.provider.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(cfg.theme.as_deref(), Some("cyberpunk"));
        // Migrates on save: legacy fields are not re-serialized.
        let out = serde_json::to_string(&cfg).unwrap();
        assert!(!out.contains("anthropic_api_key"));
        assert!(out.contains("\"apiKey\""));
    }

    #[test]
    fn modern_config_unaffected_by_normalize() {
        let mut cfg = ZodeConfig {
            provider: ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-modern".into()),
                model: Some("MiniMax-M1".into()),
                ..Default::default()
            },
            legacy_anthropic_api_key: Some("sk-should-not-win".into()),
            ..Default::default()
        };
        cfg.normalize_legacy();
        // Modern provider values are not overwritten by legacy ones.
        assert_eq!(cfg.provider.api_key.as_deref(), Some("sk-modern"));
        assert_eq!(cfg.provider.model.as_deref(), Some("MiniMax-M1"));
    }

    #[test]
    fn merge_switches_provider_type_without_repeating_key() {
        let mut global = ZodeConfig {
            provider: ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk".into()),
                model: Some("claude".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        // Project switches to ollama with only type + baseUrl.
        let project: ZodeConfig = serde_json::from_str(
            r#"{"provider":{"type":"ollama","baseUrl":"http://gpu-box:11434"}}"#,
        )
        .unwrap();
        global.merge_from(project);
        assert_eq!(global.provider.kind(), ProviderKind::Ollama);
        assert_eq!(
            global.provider.base_url.as_deref(),
            Some("http://gpu-box:11434")
        );
        // api key / model inherited from global (project omitted them).
        assert_eq!(global.provider.api_key.as_deref(), Some("sk"));
        assert_eq!(global.provider.model.as_deref(), Some("claude"));
    }

    #[test]
    fn merge_adopts_project_session_and_plugin_fields() {
        let mut global = ZodeConfig {
            goal: Some("global goal".into()),
            effort: Some("low".into()),
            ..Default::default()
        };
        let project: ZodeConfig = serde_json::from_str(
            r#"{
                "currency":"CNY","language":"zh","goal":"ship it","effort":"high",
                "temperature":0.2,"promptCache":false,
                "plugins":{"disabled":["tools:git"]},
                "lsp":{"servers":{"rust":{"command":"rust-analyzer","args":[]}}}
            }"#,
        )
        .unwrap();
        global.merge_from(project);
        assert_eq!(global.currency.as_deref(), Some("CNY"));
        assert_eq!(global.language.as_deref(), Some("zh"));
        assert_eq!(global.goal.as_deref(), Some("ship it"));
        assert_eq!(global.effort.as_deref(), Some("high"));
        assert_eq!(global.temperature, Some(0.2));
        assert_eq!(global.prompt_cache, Some(false));
        assert_eq!(global.plugins.disabled, vec!["tools:git".to_string()]);
        assert!(global.lsp.servers.contains_key("rust"));
    }

    #[test]
    fn merge_keeps_global_session_fields_when_project_omits_them() {
        let mut global = ZodeConfig {
            goal: Some("keep me".into()),
            effort: Some("high".into()),
            temperature: Some(0.7),
            prompt_cache: Some(true),
            ..Default::default()
        };
        global.plugins.disabled = vec!["tools:web".into()];
        global.lsp.servers.insert(
            "rust".into(),
            serde_json::from_str(r#"{"command":"rust-analyzer","args":[]}"#).unwrap(),
        );
        // Project sets only an unrelated field; everything above must survive.
        let project: ZodeConfig = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        global.merge_from(project);
        assert_eq!(global.goal.as_deref(), Some("keep me"));
        assert_eq!(global.effort.as_deref(), Some("high"));
        assert_eq!(global.temperature, Some(0.7));
        assert_eq!(global.prompt_cache, Some(true));
        assert_eq!(global.plugins.disabled, vec!["tools:web".to_string()]);
        assert!(global.lsp.servers.contains_key("rust"));
    }

    #[test]
    #[serial_test::serial]
    fn ollama_env_fallback_fills_base_url_not_key() {
        let mut cfg = ZodeConfig {
            provider: ProviderConfig {
                r#type: Some(ProviderKind::Ollama),
                ..Default::default()
            },
            ..Default::default()
        };
        std::env::set_var("OLLAMA_HOST", "http://gpu-box:11434");
        cfg.apply_env_fallbacks();
        assert_eq!(
            cfg.provider.base_url.as_deref(),
            Some("http://gpu-box:11434")
        );
        assert!(cfg.provider.api_key.is_none());
        std::env::remove_var("OLLAMA_HOST");
    }

    #[test]
    #[serial_test::serial]
    fn env_api_key_fallback_fills_missing_key() {
        // default already leaves provider.api_key = None
        let mut cfg = ZodeConfig::default();
        std::env::set_var("ANTHROPIC_API_KEY", "env-key");
        cfg.apply_env_fallbacks();
        assert_eq!(cfg.provider.api_key.as_deref(), Some("env-key"));
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    #[test]
    fn image_config_parses_and_serializes() {
        let cfg: ZodeConfig = serde_json::from_str(
            r#"{
                "images": {
                    "mode": "vision-model",
                    "visionProvider": "openai-vision",
                    "visionPrompt": "Describe UI screenshots."
                },
                "providers": {
                    "openai-vision": {
                        "type": "openai",
                        "apiKey": "sk-test",
                        "baseUrl": "https://api.openai.com/v1",
                        "model": "gpt-4.1",
                        "supportsImages": true
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(cfg.images.mode, Some(ImageMode::VisionModel));
        assert_eq!(cfg.images.vision_provider.as_deref(), Some("openai-vision"));
        assert_eq!(
            cfg.images.vision_prompt.as_deref(),
            Some("Describe UI screenshots.")
        );
        assert_eq!(cfg.providers["openai-vision"].supports_images, Some(true));

        let out = serde_json::to_string(&cfg).unwrap();
        assert!(out.contains("\"supportsImages\":true"));
        assert!(out.contains("\"visionProvider\":\"openai-vision\""));
    }

    #[test]
    fn provider_reasoning_opt_in_roundtrips() {
        let cfg: ZodeConfig = serde_json::from_str(
            r#"{
                "provider": {
                    "type": "openai",
                    "apiKey": "sk-test",
                    "baseUrl": "https://api.deepseek.com/v1",
                    "model": "deepseek-reasoner",
                    "reasoning": true
                }
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.provider.reasoning, Some(true));

        let out = serde_json::to_string(&cfg).unwrap();
        assert!(out.contains("\"reasoning\":true"));

        // Absent by default and omitted from the serialized form (no null noise).
        let default_cfg = ZodeConfig::default();
        assert_eq!(default_cfg.provider.reasoning, None);
        let default_out = serde_json::to_string(&default_cfg).unwrap();
        assert!(!default_out.contains("\"reasoning\""));
    }

    #[test]
    fn merge_adopts_project_reasoning_opt_in() {
        let mut global = ZodeConfig {
            provider: ProviderConfig {
                r#type: Some(ProviderKind::Openai),
                api_key: Some("sk".into()),
                model: Some("deepseek-reasoner".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let project: ZodeConfig =
            serde_json::from_str(r#"{"provider":{"reasoning":true}}"#).unwrap();
        global.merge_from(project);
        assert_eq!(global.provider.reasoning, Some(true));
        // Unrelated fields inherited from global untouched.
        assert_eq!(global.provider.api_key.as_deref(), Some("sk"));
    }

    #[test]
    fn image_config_merges_field_by_field() {
        let mut global: ZodeConfig = serde_json::from_str(
            r#"{
                "images": {
                    "mode": "auto",
                    "visionProvider": "openai-vision",
                    "visionPrompt": "Global prompt"
                }
            }"#,
        )
        .unwrap();
        let project: ZodeConfig = serde_json::from_str(
            r#"{
                "images": {
                    "mode": "direct",
                    "visionPrompt": "Project prompt"
                }
            }"#,
        )
        .unwrap();

        global.merge_from(project);

        assert_eq!(global.images.mode, Some(ImageMode::Direct));
        assert_eq!(
            global.images.vision_provider.as_deref(),
            Some("openai-vision")
        );
        assert_eq!(
            global.images.vision_prompt.as_deref(),
            Some("Project prompt")
        );
    }

    #[test]
    fn subagent_iteration_budget_parses_serializes_and_merges() {
        let cfg: ZodeConfig = serde_json::from_str(r#"{"subagentMaxIterations":64}"#).unwrap();
        assert_eq!(cfg.subagent_max_iterations, Some(64));
        assert!(serde_json::to_string(&cfg)
            .unwrap()
            .contains("\"subagentMaxIterations\":64"));

        let mut global: ZodeConfig =
            serde_json::from_str(r#"{"subagentMaxIterations":32}"#).unwrap();
        let project: ZodeConfig = serde_json::from_str(r#"{"subagentMaxIterations":0}"#).unwrap();
        global.merge_from(project);
        assert_eq!(global.subagent_max_iterations, Some(0));
    }

    #[test]
    fn skill_discipline_camelcase_default_and_merge() {
        let cfg: ZodeConfig = serde_json::from_str(r#"{"skillDiscipline":false}"#).unwrap();
        assert!(!cfg.skill_discipline());
        assert!(ZodeConfig::default().skill_discipline()); // absent → default true
                                                           // presence-based merge: project value wins, absent preserves global
        let mut base = ZodeConfig {
            skill_discipline: Some(false),
            ..Default::default()
        };
        base.merge_from(ZodeConfig::default());
        assert!(!base.skill_discipline()); // global false preserved (project absent)
        let mut base2 = ZodeConfig {
            skill_discipline: Some(false),
            ..Default::default()
        };
        let proj = ZodeConfig {
            skill_discipline: Some(true),
            ..Default::default()
        };
        base2.merge_from(proj);
        assert!(base2.skill_discipline()); // project true wins
    }

    #[test]
    fn compress_output_defaults_true_and_parses() {
        let cfg: ZodeConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.compress_output());
        let cfg2: ZodeConfig =
            serde_json::from_str(r#"{"tools":{"compressOutput":false}}"#).unwrap();
        assert!(!cfg2.compress_output());
    }

    #[test]
    fn provider_config_roundtrips_per_provider_limits() {
        let json = r#"{
            "type": "anthropic",
            "apiKey": "sk-x",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "model": "deepseek-v4-pro",
            "contextWindow": 1000000,
            "maxOutputTokens": 8192
        }"#;
        let p: ProviderConfig = serde_json::from_str(json).unwrap();
        assert_eq!(p.context_window, Some(1_000_000));
        assert_eq!(p.max_output_tokens, Some(8192));
        // Round-trips back out under camelCase.
        let out = serde_json::to_string(&p).unwrap();
        assert!(out.contains("\"contextWindow\":1000000"));
        assert!(out.contains("\"maxOutputTokens\":8192"));
    }

    #[test]
    fn resolve_provider_from_map_fills_key_by_model() {
        let mut cfg = ZodeConfig::default();
        cfg.provider.model = Some("deepseek-v4-pro".into()); // active model, NO key
        cfg.providers.insert(
            "deepseek-v4-pro".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-x".into()),
                base_url: Some("https://api.deepseek.com/anthropic".into()),
                model: Some("deepseek-v4-pro".into()),
                context_window: Some(1_000_000),
                ..Default::default()
            },
        );
        assert!(cfg.provider.api_key.is_none());
        cfg.resolve_provider_from_map();
        assert_eq!(cfg.provider.api_key.as_deref(), Some("sk-x"));
        assert_eq!(
            cfg.provider.base_url.as_deref(),
            Some("https://api.deepseek.com/anthropic")
        );
        assert_eq!(cfg.provider.context_window, Some(1_000_000));
    }

    #[test]
    fn resolve_provider_from_map_noop_when_active_has_key() {
        let mut cfg = ZodeConfig::default();
        cfg.provider.api_key = Some("already".into());
        cfg.providers.insert(
            "x".into(),
            ProviderConfig {
                api_key: Some("other".into()),
                ..Default::default()
            },
        );
        cfg.resolve_provider_from_map();
        assert_eq!(cfg.provider.api_key.as_deref(), Some("already"));
    }

    #[test]
    fn resolve_provider_from_map_picks_first_when_no_active_model() {
        // The real-world case: no top-level `provider`, several `providers`
        // entries. With no active model, adopt the FIRST (file/insertion order).
        let mut cfg = ZodeConfig::default(); // provider.model == None
        cfg.providers.insert(
            "deepseek-v4-pro".into(),
            ProviderConfig {
                api_key: Some("sk-pro".into()),
                model: Some("deepseek-v4-pro".into()),
                base_url: Some("https://api.deepseek.com/anthropic".into()),
                ..Default::default()
            },
        );
        cfg.providers.insert(
            "deepseek-v4-flash".into(),
            ProviderConfig {
                api_key: Some("sk-flash".into()),
                model: Some("deepseek-v4-flash".into()),
                ..Default::default()
            },
        );
        cfg.resolve_provider_from_map();
        assert_eq!(cfg.provider.api_key.as_deref(), Some("sk-pro")); // first entry
        assert_eq!(cfg.provider.model.as_deref(), Some("deepseek-v4-pro"));
    }

    #[test]
    fn empty_and_default_blocks_are_omitted_on_serialize() {
        let mut cfg = ZodeConfig::default();
        // The realistic case: only a providers map is configured.
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk".into()),
                base_url: Some("https://x/anthropic".into()),
                model: Some("deepseek-v4-pro".into()),
                ..Default::default()
            },
        );
        cfg.theme = Some("minimal".into());
        let json = serde_json::to_string_pretty(&cfg).unwrap();

        // The all-null active provider block and every all-default nested block
        // must be omitted (no "全部 null" noise).
        assert!(
            !json.contains("\"provider\":"),
            "empty active provider must be omitted: {json}"
        );
        for key in [
            "openpencil",
            "sandbox",
            "permissions",
            "lsp",
            "noema",
            "images",
            "plugins",
        ] {
            assert!(
                !json.contains(&format!("\"{key}\":")),
                "default `{key}` block must be omitted: {json}"
            );
        }
        // Null scalar options are dropped too.
        for key in [
            "currency",
            "goal",
            "effort",
            "maxOutputTokens",
            "contextWindow",
            "temperature",
        ] {
            assert!(
                !json.contains(&format!("\"{key}\":")),
                "null `{key}` must be omitted: {json}"
            );
        }
        // No `"dialect": null` noise inside providers entries.
        assert!(!json.contains("\"dialect\": null"));
        // Set values are kept and the config round-trips.
        assert!(json.contains("\"providers\":"));
        assert!(json.contains("\"theme\":"));
        assert!(json.contains("deepseek-v4-pro"));
        let back: ZodeConfig = serde_json::from_str(&json).unwrap();
        assert!(back.providers.contains_key("deepseek"));
        assert_eq!(back.theme.as_deref(), Some("minimal"));
    }

    #[test]
    fn provider_config_roundtrips_models_map() {
        let json = r#"{
            "type": "anthropic",
            "apiKey": "sk-x",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "models": {
                "deepseek-v4-pro":   { "contextWindow": 1000000 },
                "deepseek-v4-flash": {}
            }
        }"#;
        let p: ProviderConfig = serde_json::from_str(json).unwrap();
        assert_eq!(p.models.len(), 2);
        assert_eq!(p.models["deepseek-v4-pro"].context_window, Some(1_000_000));
        // Insertion order is preserved (IndexMap).
        let keys: Vec<&String> = p.models.keys().collect();
        assert_eq!(keys, vec!["deepseek-v4-pro", "deepseek-v4-flash"]);
        // An empty models map is skipped on serialize.
        let out = serde_json::to_string(&ProviderConfig::default()).unwrap();
        assert!(
            !out.contains("models"),
            "empty models map must not serialize"
        );
    }

    #[test]
    fn resolve_model_provider_uses_shared_creds_and_per_model_override() {
        let mut cfg = ZodeConfig::default();
        let mut models = IndexMap::new();
        models.insert(
            "deepseek-v4-pro".to_string(),
            ModelOverride {
                context_window: Some(1_000_000),
                input_price: Some(0.28),
                ..Default::default()
            },
        );
        models.insert("deepseek-v4-flash".to_string(), ModelOverride::default());
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-shared".into()),
                base_url: Some("https://api.deepseek.com/anthropic".into()),
                models,
                ..Default::default()
            },
        );

        let pro = cfg
            .resolve_model_provider("deepseek-v4-pro")
            .expect("pro model resolves");
        assert_eq!(pro.api_key.as_deref(), Some("sk-shared"));
        assert_eq!(
            pro.base_url.as_deref(),
            Some("https://api.deepseek.com/anthropic")
        );
        assert_eq!(pro.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(pro.context_window, Some(1_000_000)); // per-model override
        assert_eq!(pro.input_price, Some(0.28));
        assert!(
            pro.models.is_empty(),
            "resolved active provider must not carry the models map"
        );

        let flash = cfg
            .resolve_model_provider("deepseek-v4-flash")
            .expect("flash model resolves");
        assert_eq!(flash.api_key.as_deref(), Some("sk-shared"));
        assert_eq!(flash.model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(flash.context_window, None); // no override → unset

        assert!(cfg.resolve_model_provider("nope").is_none());
    }

    #[test]
    fn connect_provider_groups_models_under_one_entry() {
        let mut cfg = ZodeConfig::default();
        let pro = ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            api_key: Some("sk-shared".into()),
            base_url: Some("https://api.deepseek.com/anthropic".into()),
            model: Some("deepseek-v4-pro".into()),
            context_window: Some(1_000_000),
            ..Default::default()
        };
        cfg.connect_provider(
            "deepseek",
            pro,
            ModelOverride {
                context_window: Some(1_000_000),
                ..Default::default()
            },
        );
        // Connect a SECOND model under the same provider.
        let flash = ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            api_key: Some("sk-shared".into()),
            base_url: Some("https://api.deepseek.com/anthropic".into()),
            model: Some("deepseek-v4-flash".into()),
            ..Default::default()
        };
        cfg.connect_provider("deepseek", flash, ModelOverride::default());

        assert_eq!(cfg.providers.len(), 1, "both models share one entry");
        let entry = &cfg.providers["deepseek"];
        assert_eq!(entry.api_key.as_deref(), Some("sk-shared"));
        assert!(
            entry.model.is_none(),
            "models live in the map, not top-level"
        );
        let keys: Vec<&String> = entry.models.keys().collect();
        assert_eq!(keys, vec!["deepseek-v4-pro", "deepseek-v4-flash"]);
        assert_eq!(
            entry.models["deepseek-v4-pro"].context_window,
            Some(1_000_000)
        );
        // Active provider is the most-recently connected model.
        assert_eq!(cfg.provider.model.as_deref(), Some("deepseek-v4-flash"));
        // The grouped entry resolves back to working credentials.
        let resolved = cfg.resolve_model_provider("deepseek-v4-pro").unwrap();
        assert_eq!(resolved.api_key.as_deref(), Some("sk-shared"));
        assert_eq!(resolved.context_window, Some(1_000_000));
    }

    #[test]
    fn set_active_model_minimal_when_in_map_else_keeps_creds() {
        let mut cfg = ZodeConfig::default();
        let mut models = IndexMap::new();
        models.insert("deepseek-v4-pro".to_string(), ModelOverride::default());
        models.insert("deepseek-chat".to_string(), ModelOverride::default());
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk".into()),
                base_url: Some("https://api.deepseek.com/anthropic".into()),
                models,
                ..Default::default()
            },
        );
        cfg.provider = ProviderConfig {
            model: Some("deepseek-chat".into()),
            ..Default::default()
        };

        // Switching to a model owned by the providers map → active provider is
        // reduced to just the model name, and still resolves to full creds.
        cfg.set_active_model("deepseek-v4-pro");
        assert_eq!(cfg.provider.model.as_deref(), Some("deepseek-v4-pro"));
        assert!(cfg.provider.api_key.is_none());
        assert_eq!(
            cfg.resolve_model_provider("deepseek-v4-pro")
                .unwrap()
                .api_key
                .as_deref(),
            Some("sk")
        );

        // A custom model not in the map → set on the active provider, creds kept.
        cfg.provider = ProviderConfig {
            api_key: Some("sk-custom".into()),
            base_url: Some("https://custom/v1".into()),
            model: Some("old".into()),
            ..Default::default()
        };
        cfg.set_active_model("totally-custom");
        assert_eq!(cfg.provider.model.as_deref(), Some("totally-custom"));
        assert_eq!(cfg.provider.api_key.as_deref(), Some("sk-custom"));
    }

    #[test]
    fn connect_provider_active_records_only_model_name() {
        let mut cfg = ZodeConfig::default();
        let active = ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            api_key: Some("sk-x".into()),
            base_url: Some("https://api.deepseek.com/anthropic".into()),
            model: Some("deepseek-v4-pro".into()),
            dialect: Some("deepseek".into()),
            context_window: Some(1_000_000),
            input_price: Some(0.435),
            ..Default::default()
        };
        cfg.connect_provider(
            "deepseek",
            active,
            ModelOverride {
                context_window: Some(1_000_000),
                input_price: Some(0.435),
                ..Default::default()
            },
        );

        // The active provider records ONLY the model name — no duplicated creds.
        assert_eq!(cfg.provider.model.as_deref(), Some("deepseek-v4-pro"));
        assert!(cfg.provider.api_key.is_none());
        assert!(cfg.provider.base_url.is_none());
        assert!(cfg.provider.dialect.is_none());
        assert!(cfg.provider.context_window.is_none());
        assert!(cfg.provider.input_price.is_none());
        // The providers map is the single source of truth (creds + override).
        let entry = &cfg.providers["deepseek"];
        assert_eq!(entry.api_key.as_deref(), Some("sk-x"));
        assert_eq!(
            entry.models["deepseek-v4-pro"].context_window,
            Some(1_000_000)
        );
        assert_eq!(entry.models["deepseek-v4-pro"].input_price, Some(0.435));
        // …and the minimal active provider resolves back to the full config.
        let resolved = cfg.resolve_model_provider("deepseek-v4-pro").unwrap();
        assert_eq!(resolved.api_key.as_deref(), Some("sk-x"));
        assert_eq!(
            resolved.base_url.as_deref(),
            Some("https://api.deepseek.com/anthropic")
        );
        assert_eq!(resolved.context_window, Some(1_000_000));
        assert_eq!(resolved.input_price, Some(0.435));
    }

    #[test]
    fn connect_provider_migration_preserves_legacy_model_settings() {
        let mut cfg = ZodeConfig::default();
        // Legacy entry with per-model settings ON the entry itself.
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-old".into()),
                base_url: Some("https://api.deepseek.com/anthropic".into()),
                model: Some("deepseek-v4-pro".into()),
                context_window: Some(1_000_000),
                input_price: Some(0.28),
                ..Default::default()
            },
        );
        let flash = ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            api_key: Some("sk-old".into()),
            base_url: Some("https://api.deepseek.com/anthropic".into()),
            model: Some("deepseek-v4-flash".into()),
            ..Default::default()
        };
        cfg.connect_provider("deepseek", flash, ModelOverride::default());
        let entry = &cfg.providers["deepseek"];
        // The legacy model's per-model settings move into ITS override…
        assert_eq!(
            entry.models["deepseek-v4-pro"].context_window,
            Some(1_000_000)
        );
        assert_eq!(entry.models["deepseek-v4-pro"].input_price, Some(0.28));
        // …and must NOT linger on the parent (else every model inherits them).
        assert_eq!(entry.context_window, None);
        assert_eq!(entry.input_price, None);
        // The newly added flash model does not inherit pro's context.
        assert_eq!(
            cfg.resolve_model_provider("deepseek-v4-flash")
                .unwrap()
                .context_window,
            None
        );
        assert_eq!(
            cfg.resolve_model_provider("deepseek-v4-pro")
                .unwrap()
                .context_window,
            Some(1_000_000)
        );
    }

    #[test]
    fn resolve_named_provider_applies_default_model_override() {
        let mut cfg = ZodeConfig::default();
        let mut models = IndexMap::new();
        models.insert(
            "deepseek-v4-pro".to_string(),
            ModelOverride {
                context_window: Some(1_000_000),
                ..Default::default()
            },
        );
        models.insert("deepseek-v4-flash".to_string(), ModelOverride::default());
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-shared".into()),
                models,
                ..Default::default()
            },
        );
        let p = cfg
            .resolve_named_provider("deepseek")
            .expect("named provider");
        assert_eq!(p.model.as_deref(), Some("deepseek-v4-pro")); // first listed
        assert_eq!(p.context_window, Some(1_000_000)); // its override applied
        assert_eq!(p.api_key.as_deref(), Some("sk-shared"));
        assert!(p.models.is_empty());
        assert!(cfg.resolve_named_provider("nope").is_none());
    }

    #[test]
    fn resolve_named_provider_model_applies_specified_model_override() {
        let mut cfg = ZodeConfig::default();
        let mut models = IndexMap::new();
        models.insert(
            "deepseek-v4-pro".to_string(),
            ModelOverride {
                context_window: Some(1_000_000),
                ..Default::default()
            },
        );
        models.insert(
            "deepseek-v4-flash".to_string(),
            ModelOverride {
                context_window: Some(131_072),
                ..Default::default()
            },
        );
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-shared".into()),
                models,
                ..Default::default()
            },
        );
        // The chosen model's OWN override applies, not the default's.
        let flash = cfg
            .resolve_named_provider_model("deepseek", "deepseek-v4-flash")
            .expect("model in provider");
        assert_eq!(flash.model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(flash.context_window, Some(131_072));
        assert_eq!(flash.api_key.as_deref(), Some("sk-shared"));
        assert!(flash.models.is_empty());
        // A model not in the map → provider creds + that model, no override.
        let custom = cfg
            .resolve_named_provider_model("deepseek", "custom-x")
            .expect("provider exists");
        assert_eq!(custom.model.as_deref(), Some("custom-x"));
        assert_eq!(custom.context_window, None);
        // Unknown provider → None.
        assert!(cfg.resolve_named_provider_model("nope", "x").is_none());
    }

    #[test]
    fn resolve_provider_from_map_applies_default_model_override() {
        let mut cfg = ZodeConfig::default(); // no active model
        let mut models = IndexMap::new();
        models.insert(
            "deepseek-v4-pro".to_string(),
            ModelOverride {
                context_window: Some(1_000_000),
                ..Default::default()
            },
        );
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-shared".into()),
                base_url: Some("https://api.deepseek.com/anthropic".into()),
                models,
                ..Default::default()
            },
        );
        cfg.resolve_provider_from_map();
        assert_eq!(cfg.provider.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(cfg.provider.context_window, Some(1_000_000)); // override applied
        assert!(cfg.provider.models.is_empty());
    }

    #[test]
    fn connect_provider_migrates_legacy_single_model_entry() {
        let mut cfg = ZodeConfig::default();
        // A pre-existing entry keyed by the group with a single top-level model.
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-old".into()),
                base_url: Some("https://api.deepseek.com/anthropic".into()),
                model: Some("deepseek-v4-pro".into()),
                ..Default::default()
            },
        );
        let flash = ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            api_key: Some("sk-old".into()),
            base_url: Some("https://api.deepseek.com/anthropic".into()),
            model: Some("deepseek-v4-flash".into()),
            ..Default::default()
        };
        cfg.connect_provider("deepseek", flash, ModelOverride::default());
        let entry = &cfg.providers["deepseek"];
        assert!(entry.model.is_none());
        let keys: Vec<&String> = entry.models.keys().collect();
        assert_eq!(
            keys,
            vec!["deepseek-v4-pro", "deepseek-v4-flash"],
            "legacy model is migrated into the map before the new one is added"
        );
    }

    #[test]
    fn real_world_minimal_provider_resolves_per_model_context_window() {
        // The exact on-disk shape: a minimal active `provider` recording only
        // the model name, with the real config living in the `providers` map and
        // BOTH models pinning contextWindow=1_000_000. Proves the status-bar
        // denominator resolves to 1M (not the 200K default) for this user, and
        // that switching between the two models keeps it 1M (both are 1M) — i.e.
        // identical %, by design, not a bug.
        let json = r#"{
          "provider": { "model": "deepseek-v4-pro" },
          "providers": {
            "deepseek": {
              "type": "anthropic",
              "apiKey": "sk-x",
              "baseUrl": "https://api.deepseek.com/anthropic",
              "dialect": "deepseek",
              "models": {
                "deepseek-v4-pro": { "contextWindow": 1000000, "maxOutputTokens": 384000 },
                "deepseek-chat":   { "contextWindow": 1000000, "maxOutputTokens": 384000 }
              }
            }
          }
        }"#;
        let mut cfg: ZodeConfig = serde_json::from_str(json).expect("parse config");
        cfg.resolve_provider_from_map();
        assert_eq!(cfg.provider.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(cfg.provider.context_window, Some(1_000_000));
        // Resolving the sibling model yields the same window (both pinned 1M).
        let chat = cfg
            .resolve_model_provider("deepseek-chat")
            .expect("resolve");
        assert_eq!(chat.context_window, Some(1_000_000));
    }

    #[test]
    fn resolve_provider_from_map_resolves_model_inside_models_map() {
        let mut cfg = ZodeConfig::default();
        cfg.provider.model = Some("deepseek-v4-flash".into()); // active model, NO key
        let mut models = IndexMap::new();
        models.insert("deepseek-v4-pro".to_string(), ModelOverride::default());
        models.insert("deepseek-v4-flash".to_string(), ModelOverride::default());
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-shared".into()),
                base_url: Some("https://api.deepseek.com/anthropic".into()),
                models,
                ..Default::default()
            },
        );
        assert!(cfg.provider.api_key.is_none());
        cfg.resolve_provider_from_map();
        assert_eq!(cfg.provider.api_key.as_deref(), Some("sk-shared"));
        assert_eq!(cfg.provider.model.as_deref(), Some("deepseek-v4-flash"));
        assert!(cfg.provider.models.is_empty());
    }

    #[test]
    fn openspec_awareness_camelcase_default_and_merge() {
        let cfg: ZodeConfig = serde_json::from_str(r#"{"openspecAwareness":false}"#).unwrap();
        assert!(!cfg.openspec_awareness());
        assert!(ZodeConfig::default().openspec_awareness()); // absent → default true
                                                             // presence-based merge: project value wins, absent preserves global
        let mut base = ZodeConfig {
            openspec_awareness: Some(false),
            ..Default::default()
        };
        base.merge_from(ZodeConfig::default());
        assert!(!base.openspec_awareness()); // global false preserved (project absent)
        let mut base2 = ZodeConfig {
            openspec_awareness: Some(false),
            ..Default::default()
        };
        let proj = ZodeConfig {
            openspec_awareness: Some(true),
            ..Default::default()
        };
        base2.merge_from(proj);
        assert!(base2.openspec_awareness()); // project true wins
    }

    #[test]
    fn compact_settings_defaults_camelcase_and_merge() {
        // All-on defaults.
        let bare = ZodeConfig::default();
        assert!(bare.compact.microcompact());
        assert!(bare.compact.memory_sink());
        assert!(bare.compact.restore_files());
        assert_eq!(bare.compact.restore_files_budget(), 50_000);
        assert!(bare.compact.recall_after_compact());
        assert_eq!(bare.compact.auto_compact_percent(), None);

        // The percent knob parses, clamps typos into the sane band, and
        // merges by presence like the other keys.
        let pct: ZodeConfig =
            serde_json::from_str(r#"{"compact":{"autoCompactPercent":75}}"#).unwrap();
        assert_eq!(pct.compact.auto_compact_percent(), Some(75));
        let low: ZodeConfig =
            serde_json::from_str(r#"{"compact":{"autoCompactPercent":5}}"#).unwrap();
        assert_eq!(low.compact.auto_compact_percent(), Some(50));
        let high: ZodeConfig =
            serde_json::from_str(r#"{"compact":{"autoCompactPercent":100}}"#).unwrap();
        assert_eq!(high.compact.auto_compact_percent(), Some(97));
        let mut merged = ZodeConfig::default();
        merged.merge_from(pct);
        assert_eq!(merged.compact.auto_compact_percent, Some(75));

        // camelCase keys parse; explicit values win.
        let cfg: ZodeConfig = serde_json::from_str(
            r#"{"compact":{"microcompact":false,"memorySink":false,"restoreFilesBudget":10000}}"#,
        )
        .unwrap();
        assert!(!cfg.compact.microcompact());
        assert!(!cfg.compact.memory_sink());
        assert_eq!(cfg.compact.restore_files_budget(), 10_000);
        assert!(cfg.compact.restore_files()); // untouched key keeps default

        // Presence-based merge: project layer overrides only set keys.
        let mut base = ZodeConfig::default();
        base.merge_from(cfg);
        assert_eq!(base.compact.microcompact, Some(false));
        assert_eq!(base.compact.restore_files_budget, Some(10_000));
        base.merge_from(ZodeConfig::default());
        assert_eq!(base.compact.microcompact, Some(false)); // unset layer preserves
    }

    #[test]
    fn auto_extract_defaults_on() {
        let bare = ZodeConfig::default();
        assert!(bare.noema.auto_extract());
        assert_eq!(bare.noema.effective_write_policy(), "autosafe");
        // Explicit false still wins and restores review policy.
        let off: ZodeConfig = serde_json::from_str(r#"{"noema":{"autoExtract":false}}"#).unwrap();
        assert!(!off.noema.auto_extract());
        assert_eq!(off.noema.effective_write_policy(), "review");
    }

    #[test]
    fn browser_config_parses_and_defaults() {
        let cfg: ZodeConfig = serde_json::from_str(
            r#"{"browser":{"headless":true,"defaultTarget":"managed",
             "viewport":{"width":1440,"height":900}}}"#,
        )
        .unwrap();
        assert!(cfg.browser.enabled()); // absent -> default true
        assert!(cfg.browser.headless());
        assert_eq!(cfg.browser.viewport(), (1440, 900));
        let d = ZodeConfig::default();
        assert_eq!(d.browser.viewport(), (1280, 800));
        assert_eq!(d.browser.default_target(), "managed");
    }

    #[test]
    fn browser_config_merges_presence_based() {
        let mut base: ZodeConfig =
            serde_json::from_str(r#"{"browser":{"headless":true}}"#).unwrap();
        let over: ZodeConfig =
            serde_json::from_str(r#"{"browser":{"executable":"/opt/chrome"}}"#).unwrap();
        base.merge_from(over);
        assert!(base.browser.headless()); // kept
        assert_eq!(base.browser.executable.as_deref(), Some("/opt/chrome")); // merged
    }

    #[test]
    fn desktop_config_defaults_and_merge() {
        let d = crate::config::DesktopConfig::default();
        assert!(d.enabled()); // absent -> default true
        assert_eq!(d.snapshot_max_nodes(), 500);

        // project layer field present overrides global; absent keeps global
        let mut base: ZodeConfig =
            serde_json::from_str(r#"{"desktop":{"enabled":true,"snapshotMaxNodes":500}}"#).unwrap();
        let over: ZodeConfig = serde_json::from_str(r#"{"desktop":{"enabled":false}}"#).unwrap();
        base.merge_from(over);
        assert!(!base.desktop.enabled()); // overridden
        assert_eq!(base.desktop.snapshot_max_nodes(), 500); // kept
    }

    #[test]
    fn desktop_overlay_defaults_and_merge() {
        let d = DesktopConfig::default();
        assert!(d.ghost_cursor());
        assert!(d.esc_cancel());
        assert_eq!(d.overlay_helper_path(), None);

        // Layer merge: overrides survive, absent keys keep base values.
        let mut base = ZodeConfig::default();
        base.desktop.ghost_cursor = Some(false);
        let mut over = ZodeConfig::default();
        over.desktop.esc_cancel = Some(false);
        over.desktop.overlay_helper_path = Some("/opt/zode-overlay".into());
        base.merge_from(over);
        assert!(!base.desktop.ghost_cursor());
        assert!(!base.desktop.esc_cancel());
        assert_eq!(
            base.desktop.overlay_helper_path(),
            Some("/opt/zode-overlay")
        );
    }

    #[test]
    #[serial_test::serial]
    fn browser_default_target_persists_without_clobbering_global_config() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        ConfigManager::save_global(&ZodeConfig {
            theme: Some("mono".into()),
            ..Default::default()
        })
        .unwrap();

        ConfigManager::persist_browser_default_target("bridge").unwrap();
        let saved = ConfigManager::load_global().unwrap();
        assert_eq!(saved.browser.default_target(), "bridge");
        assert_eq!(saved.theme.as_deref(), Some("mono"));

        let error = ConfigManager::persist_browser_default_target("other").unwrap_err();
        assert!(error.to_string().contains("managed | bridge"));
        assert_eq!(
            ConfigManager::load_global()
                .unwrap()
                .browser
                .default_target(),
            "bridge"
        );
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
    #[serial_test::serial]
    fn browser_enabled_persists_without_clobbering_global_config() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        ConfigManager::save_global(&ZodeConfig {
            theme: Some("mono".into()),
            ..Default::default()
        })
        .unwrap();

        ConfigManager::persist_browser_enabled(false).unwrap();
        let saved = ConfigManager::load_global().unwrap();
        assert_eq!(saved.browser.enabled, Some(false));
        assert_eq!(saved.theme.as_deref(), Some("mono"));

        ConfigManager::persist_browser_enabled(true).unwrap();
        assert_eq!(
            ConfigManager::load_global().unwrap().browser.enabled,
            Some(true)
        );
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
    fn windows_sandbox_tier_parses_camelcase_and_merges_by_presence() {
        let mut base: ZodeConfig =
            serde_json::from_str(r#"{"sandbox":{"windowsTier":"auto"}}"#).unwrap();
        assert_eq!(base.sandbox.windows_tier.as_deref(), Some("auto"));
        let over: ZodeConfig =
            serde_json::from_str(r#"{"sandbox":{"windowsTier":"elevated"}}"#).unwrap();
        base.merge_from(over);
        assert_eq!(base.sandbox.windows_tier.as_deref(), Some("elevated"));
    }

    #[test]
    fn external_agents_config_defaults_and_merge() {
        let json = r#"{"externalAgents":{"timeoutSecs":600,
            "agents":{"codex":{"extraArgs":["--model","gpt-x"]},
                      "my-cli":{"command":"my-agent","args":["run","{prompt}"],"output":"text"}}}}"#;
        let cfg: ZodeConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.external_agents.enabled());
        assert_eq!(cfg.external_agents.timeout().as_secs(), 600);
        assert_eq!(cfg.external_agents.max_concurrent(), 2);
        assert_eq!(
            cfg.external_agents.agents["my-cli"].args.as_deref(),
            Some(&["run".to_string(), "{prompt}".to_string()][..])
        );

        // merge: project layer replaces same-key entries wholesale; absent
        // scalars keep the global value.
        let mut global: ZodeConfig = serde_json::from_str(
            r#"{"externalAgents":{"maxConcurrent":4,
                "agents":{"codex":{"trusted":true,"extraArgs":["--old"]}}}}"#,
        )
        .unwrap();
        let project: ZodeConfig = serde_json::from_str(
            r#"{"externalAgents":{"agents":{"codex":{"extraArgs":["--new"]}}}}"#,
        )
        .unwrap();
        global.merge_from(project);
        let codex = &global.external_agents.agents["codex"];
        assert_eq!(
            codex.extra_args.as_deref(),
            Some(&["--new".to_string()][..])
        );
        assert_eq!(
            codex.trusted, None,
            "entry replaced wholesale, not field-merged"
        );
        assert_eq!(
            global.external_agents.max_concurrent(),
            4,
            "absent scalar keeps global"
        );
    }
}
