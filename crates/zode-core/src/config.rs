//! Zode configuration: `~/.zode/config.json` (global) shallow-merged
//! with `.zode/config.json` (project) and ANTHROPIC_API_KEY-style env
//! fallbacks. JSON uses camelCase to stay compatible with the
//! TS/Zig-era config files users already have.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[default]
    Anthropic,
    Openai,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderConfig {
    /// Option so config merging can tell "explicitly set" from "absent"
    /// (serde would otherwise fill the default and erase that distinction).
    /// Use [`ProviderConfig::kind`] to read the effective kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<ProviderKind>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    /// openai dialect: standard | deepseek | moonshot | openrouter
    pub dialect: Option<String>,
    /// Explicitly allow or block image input for this provider config.
    /// When unset, provider defaults decide.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_images: Option<bool>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ImageMode {
    #[default]
    Auto,
    Direct,
    VisionModel,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PermissionsConfig {
    /// Tool names always allowed without prompting.
    pub allow: Vec<String>,
    /// Tool names always denied (hard block).
    pub deny: Vec<String>,
    /// Tool names that require interactive approval.
    pub ask: Vec<String>,
}

/// OS-sandbox settings for shell commands. The sandbox is **on by default**
/// (workspace-write, network denied); these refine or disable it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
}

/// Plugin enable/disable state. Plugins are on by default, so only the
/// disabled ids are stored (e.g. `["tools:git", "mcp:foo", "lsp:rust"]`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginsConfig {
    pub disabled: Vec<String>,
}

/// Default OpenPencil release the installer/launcher targets. zode and
/// OpenPencil version independently — do NOT use zode's CARGO_PKG_VERSION.
pub const DEFAULT_OPENPENCIL_VERSION: &str = "0.8.0";

/// OpenPencil control-surface settings (the `op-bridge`). Every field is
/// `Option` so layered config can tell "absent" from "explicitly set" and a
/// project layer can reset a global value; read effective values via getters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
}

impl NoemaSettings {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    pub fn auto_remember(&self) -> bool {
        self.auto_remember.unwrap_or(true)
    }
}

/// Configuration for the built-in LSP plugin: a language server per language
/// key. The key (e.g. "rust", "python") is also the plugin id suffix
/// (`lsp:rust`). `extensions` maps the server to the file types it handles.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LspConfig {
    pub servers: HashMap<String, LspServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ZodeConfig {
    pub provider: ProviderConfig,
    /// Named providers; `--provider <name>` selects one into `provider`.
    pub providers: HashMap<String, ProviderConfig>,
    pub images: ImagesConfig,
    pub theme: Option<String>,
    /// Display currency for cost (e.g. "USD", "CNY", "EUR"). `None` → USD.
    /// Per-provider prices are in USD/MTok; the total is converted for display.
    pub currency: Option<String>,
    /// UI language code (e.g. "en", "zh"). `None` → English. (Settings picker
    /// + full UI translation land in a follow-up; the value is read here.)
    pub language: Option<String>,
    /// A persistent objective injected into the system prompt so the agent
    /// keeps it in focus. Set/cleared via `/goal`. `None`/empty → no goal.
    pub goal: Option<String>,
    /// Effort level: "low" | "medium" | "high". Injects a thoroughness
    /// directive into the system prompt. Set via `/effort`. `None` → balanced.
    pub effort: Option<String>,
    /// Show the agent's thinking/reasoning output in the chat. `None` → shown.
    /// Toggled by `/thinking`.
    pub show_thinking: Option<bool>,
    /// Show tool-call detail lines in the chat. `None` → shown. Toggled by
    /// `/tool-details`.
    pub show_tool_details: Option<bool>,
    /// Autonomous orchestration: when on, the agent is told it may decompose a
    /// task and spawn sub-agents (Task tool) on its own, and the `define_agent`
    /// tool is registered so it can create new sub-agent types. `None` → ON
    /// (enabled by default). Toggled off via Settings / `/orchestration`.
    pub autonomous_orchestration: Option<bool>,
    /// Inject a "use skills first" discipline block into the system prompt when
    /// skills are available. `None` → ON. Set `"skillDiscipline": false` to disable.
    pub skill_discipline: Option<bool>,
    /// Inject an OpenSpec-workflow awareness block into the system prompt when
    /// the project uses OpenSpec (an `openspec/` dir at the project root). `None`
    /// → ON. Set `"openspecAwareness": false` to disable even when detected.
    pub openspec_awareness: Option<bool>,
    pub permissions: PermissionsConfig,
    pub sandbox: SandboxSettings,
    pub max_output_tokens: Option<u32>,
    /// The model's context window in tokens, used for auto-compaction /
    /// context-left math. `None` defaults to a conservative 200K. Set this to
    /// `1000000` for a 1M-context model (e.g. `claude-opus-4-8[1m]`) so zode
    /// uses the full window instead of compacting at 200K. Do NOT set it ABOVE
    /// the model's real window — overestimating makes the request overflow and
    /// the API reject the turn; underestimating only compacts earlier.
    pub context_window: Option<u32>,
    /// Sampling temperature. `None` uses the provider default; a low value
    /// (e.g. 0) makes coding output deterministic and more reliably correct.
    pub temperature: Option<f32>,
    /// Request provider prompt caching (`None` defaults to true). For Anthropic/
    /// MiniMax this adds `cache_control` so the stable system+tools prefix is
    /// cached across turns instead of re-billed every turn; OpenAI-compatible
    /// providers cache automatically.
    pub prompt_cache: Option<bool>,
    /// Plugin enable/disable state (`/plugin` manages MCP, skills, tool groups,
    /// LSP). Default-on, so this only records the disabled ones.
    pub plugins: PluginsConfig,
    /// Language-server configuration for the built-in LSP plugin.
    pub lsp: LspConfig,
    /// OpenPencil control-surface configuration (the `op-bridge`).
    pub openpencil: OpenPencilConfig,
    /// Native Noema long-term memory integration.
    pub noema: NoemaSettings,

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

impl ZodeConfig {
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
        if other.max_output_tokens.is_some() {
            self.max_output_tokens = other.max_output_tokens;
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

        let n = other.noema;
        self.noema.enabled = n.enabled.or(self.noema.enabled);
        self.noema.root = n.root.or(self.noema.root.take());
        self.noema.user = n.user.or(self.noema.user.take());
        self.noema.auto_remember = n.auto_remember.or(self.noema.auto_remember);
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

    /// Persist to the global config path (creates the dir if needed).
    pub fn save_global(cfg: &ZodeConfig) -> Result<(), CoreError> {
        let dir = Self::config_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("config.json");
        let json = serde_json::to_string_pretty(cfg)?;
        std::fs::write(path, json)?;
        Ok(())
    }
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
    fn project_state_persists_sandbox_and_permissions() {
        let global = tempfile::tempdir().unwrap(); // empty global config
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", global.path());

        ConfigManager::update_project_state(cwd.path(), |s| {
            s.insert("sandbox".into(), serde_json::json!({"enabled": false}));
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
        assert!(cfg.permissions.allow.contains(&"Bash".to_string()));
        assert!(cfg.permissions.allow.contains(&"FileWrite".to_string()));
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
}
