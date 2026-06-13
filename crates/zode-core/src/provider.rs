//! Provider factory: turn a `ProviderConfig` into an `Arc<dyn Provider>`.

use std::sync::Arc;

use agent::provider::{
    AnthropicProvider, OllamaConfig, OllamaProvider, OpenAiCompatConfig, OpenAiCompatProvider,
    OpenAiDialect, Provider,
};

use crate::config::{ProviderConfig, ProviderKind};
use crate::error::CoreError;

pub fn build_provider(cfg: &ProviderConfig) -> Result<Arc<dyn Provider>, CoreError> {
    match cfg.r#type {
        ProviderKind::Anthropic => {
            let key = cfg
                .api_key
                .clone()
                .ok_or(CoreError::MissingApiKey("ANTHROPIC_API_KEY"))?;
            let mut p = AnthropicProvider::new(key);
            if let Some(url) = &cfg.base_url {
                p = p.with_base_url(url.clone());
            }
            Ok(Arc::new(p))
        }
        ProviderKind::Openai => {
            let key = cfg
                .api_key
                .clone()
                .ok_or(CoreError::MissingApiKey("OPENAI_API_KEY"))?;
            let base = cfg
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            let dialect = parse_dialect(cfg.dialect.as_deref())?;
            let oc = OpenAiCompatConfig::new(key, base).with_dialect(dialect);
            Ok(Arc::new(OpenAiCompatProvider::new(oc)))
        }
        ProviderKind::Ollama => {
            let (host, port) = parse_ollama_host(cfg.base_url.as_deref());
            Ok(Arc::new(OllamaProvider::new(OllamaConfig::new(host, port))))
        }
    }
}

pub(crate) fn parse_dialect(s: Option<&str>) -> Result<OpenAiDialect, CoreError> {
    match s {
        None | Some("standard") => Ok(OpenAiDialect::Standard),
        Some("deepseek") => Ok(OpenAiDialect::DeepSeek),
        Some("moonshot") => Ok(OpenAiDialect::Moonshot),
        Some("openrouter") => Ok(OpenAiDialect::OpenRouter),
        Some(other) => Err(CoreError::UnknownDialect(other.to_string())),
    }
}

/// Split a base_url like "http://host:11434" into (host, port).
/// Falls back to ("localhost", 11434).
fn parse_ollama_host(url: Option<&str>) -> (String, u16) {
    let Some(url) = url else {
        return ("localhost".to_string(), 11434);
    };
    let stripped = url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    match stripped.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(11434)),
        None => (stripped.to_string(), 11434),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProviderConfig, ProviderKind};

    #[test]
    fn anthropic_with_base_url_builds() {
        let cfg = ProviderConfig {
            r#type: ProviderKind::Anthropic,
            api_key: Some("sk-test".into()),
            base_url: Some("https://api.minimaxi.com/anthropic/v1".into()),
            model: Some("MiniMax-M1".into()),
            dialect: None,
        };
        let p = build_provider(&cfg).unwrap();
        assert_eq!(p.id(), "anthropic");
    }

    #[test]
    fn anthropic_missing_key_errors() {
        let cfg = ProviderConfig {
            r#type: ProviderKind::Anthropic,
            api_key: None,
            ..Default::default()
        };
        assert!(matches!(
            build_provider(&cfg),
            Err(crate::CoreError::MissingApiKey(_))
        ));
    }

    #[test]
    fn openai_dialect_parses() {
        assert_eq!(
            parse_dialect(Some("deepseek")).unwrap(),
            OpenAiDialect::DeepSeek
        );
        assert_eq!(parse_dialect(None).unwrap(), OpenAiDialect::Standard);
        assert!(parse_dialect(Some("bogus")).is_err());
    }

    #[test]
    fn ollama_builds_without_key() {
        let cfg = ProviderConfig {
            r#type: ProviderKind::Ollama,
            base_url: Some("http://localhost:11434".into()),
            ..Default::default()
        };
        let p = build_provider(&cfg).unwrap();
        assert_eq!(p.id(), "ollama");
    }
}
