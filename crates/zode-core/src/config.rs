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

/// Plugin enable/disable state. Plugins are on by default, so only the
/// disabled ids are stored (e.g. `["tools:git", "mcp:foo", "lsp:rust"]`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginsConfig {
    pub disabled: Vec<String>,
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
    pub permissions: PermissionsConfig,
    pub max_output_tokens: Option<u32>,
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
        self.providers.extend(other.providers);
        if other.theme.is_some() {
            self.theme = other.theme;
        }
        if !other.permissions.allow.is_empty() {
            self.permissions.allow = other.permissions.allow;
        }
        if !other.permissions.deny.is_empty() {
            self.permissions.deny = other.permissions.deny;
        }
        if !other.permissions.ask.is_empty() {
            self.permissions.ask = other.permissions.ask;
        }
        if other.max_output_tokens.is_some() {
            self.max_output_tokens = other.max_output_tokens;
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
    /// env api-key fallback. Project overrides global.
    pub fn load(cwd: &Path) -> Result<ZodeConfig, CoreError> {
        let mut cfg = Self::load_global()?;
        let project = cwd.join(".zode").join("config.json");
        if project.exists() {
            cfg.merge_from(Self::load_file(&project)?);
        }
        cfg.normalize_legacy();
        cfg.apply_env_fallbacks();
        Ok(cfg)
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
