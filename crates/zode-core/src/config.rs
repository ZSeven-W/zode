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
    pub r#type: ProviderKind,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    /// openai dialect: standard | deepseek | moonshot | openrouter
    pub dialect: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ZodeConfig {
    pub provider: ProviderConfig,
    /// Named providers; `--provider <name>` selects one into `provider`.
    pub providers: HashMap<String, ProviderConfig>,
    pub theme: Option<String>,
    pub permissions: PermissionsConfig,
    pub max_output_tokens: Option<u32>,
}

impl ZodeConfig {
    /// Fill a missing `provider.api_key` from the matching env var.
    pub fn apply_env_fallbacks(&mut self) {
        if self.provider.api_key.is_none() {
            let var = env_key_for(self.provider.r#type);
            if let Ok(v) = std::env::var(var) {
                if !v.is_empty() {
                    self.provider.api_key = Some(v);
                }
            }
        }
    }

    /// Shallow-merge `other` (higher priority) onto self. Only
    /// `Some`/non-empty fields from `other` override.
    pub fn merge_from(&mut self, other: ZodeConfig) {
        if other.provider.api_key.is_some() {
            self.provider = other.provider;
        } else {
            // partial provider override on the active provider
            if other.provider.model.is_some() {
                self.provider.model = other.provider.model;
            }
            if other.provider.base_url.is_some() {
                self.provider.base_url = other.provider.base_url;
            }
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
        assert_eq!(cfg.provider.r#type, ProviderKind::Anthropic);
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
        assert_eq!(cfg.provider.r#type, ProviderKind::Anthropic);
        assert!(cfg.theme.is_none());
        let s = serde_json::to_string(&cfg).unwrap();
        let _back: ZodeConfig = serde_json::from_str(&s).unwrap();
    }

    #[test]
    fn config_dir_respects_env_override() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        assert_eq!(ConfigManager::config_dir().unwrap(), dir.path());
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
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
    fn env_api_key_fallback_fills_missing_key() {
        // default already leaves provider.api_key = None
        let mut cfg = ZodeConfig::default();
        std::env::set_var("ANTHROPIC_API_KEY", "env-key");
        cfg.apply_env_fallbacks();
        assert_eq!(cfg.provider.api_key.as_deref(), Some("env-key"));
        std::env::remove_var("ANTHROPIC_API_KEY");
    }
}
