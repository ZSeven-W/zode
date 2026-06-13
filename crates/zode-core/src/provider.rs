//! Provider factory: turn a `ProviderConfig` into an `Arc<dyn Provider>`.

use std::sync::Arc;

use agent::provider::{
    AnthropicProvider, OllamaConfig, OllamaProvider, OpenAiCompatConfig, OpenAiCompatProvider,
    OpenAiDialect, Provider,
};

use crate::config::{ProviderConfig, ProviderKind};
use crate::error::CoreError;

pub fn build_provider(cfg: &ProviderConfig) -> Result<Arc<dyn Provider>, CoreError> {
    match cfg.kind() {
        ProviderKind::Anthropic => {
            let key = cfg
                .api_key
                .clone()
                .ok_or(CoreError::MissingApiKey("ANTHROPIC_API_KEY"))?;
            let mut p = AnthropicProvider::new(key);
            if let Some(url) = &cfg.base_url {
                // agent's AnthropicProvider appends "/v1/messages" to base_url,
                // so base_url must be the origin WITHOUT a trailing /v1. Users'
                // existing MiniMax config stores ".../anthropic/v1"; normalize
                // it to ".../anthropic" so we don't double the /v1.
                p = p.with_base_url(normalize_anthropic_base_url(url));
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

/// Normalize an Anthropic-compatible base_url for agent's provider, which
/// unconditionally appends "/v1/messages". Strips a trailing slash and a
/// trailing "/v1" so both ".../anthropic" and ".../anthropic/v1" yield the
/// correct ".../anthropic/v1/messages".
///
/// Note: a gateway base like "https://gw/api/v1" is also stripped to
/// "https://gw/api", which is correct — the agent then targets
/// "https://gw/api/v1/messages". base_url must therefore be the path
/// *prefix* on which the gateway mounts the Anthropic API, optionally
/// ending in "/v1".
pub(crate) fn normalize_anthropic_base_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    let without_v1 = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
    without_v1.trim_end_matches('/').to_string()
}

/// Split a base_url into (host_with_scheme, port) for agent's OllamaConfig.
/// ollama-rs expects the host to include the scheme (default "http://localhost"),
/// so we KEEP/ADD the scheme and only peel a trailing :port. Falls back to
/// ("http://localhost", 11434).
fn parse_ollama_host(url: Option<&str>) -> (String, u16) {
    // Trim a trailing slash first, else "http://h:12345/" fails the port
    // peel and silently falls back to the default port.
    let raw = url.unwrap_or("http://localhost").trim_end_matches('/');
    let with_scheme = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("http://{raw}")
    };
    // Peel a trailing ":<port>" only if it parses as a port (the scheme's
    // "://" colon never parses as a u16, so it's safe to rsplit on ':').
    if let Some((host, maybe_port)) = with_scheme.rsplit_once(':') {
        if let Ok(port) = maybe_port.parse::<u16>() {
            return (host.to_string(), port);
        }
    }
    (with_scheme, 11434)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProviderConfig, ProviderKind};

    #[test]
    fn anthropic_with_base_url_builds() {
        let cfg = ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
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
            r#type: Some(ProviderKind::Anthropic),
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
            r#type: Some(ProviderKind::Ollama),
            base_url: Some("http://localhost:11434".into()),
            ..Default::default()
        };
        let p = build_provider(&cfg).unwrap();
        assert_eq!(p.id(), "ollama");
    }

    #[test]
    fn anthropic_base_url_strips_trailing_v1() {
        // MiniMax-style base ending in /v1 must not double the /v1 that
        // agent appends as /v1/messages.
        assert_eq!(
            normalize_anthropic_base_url("https://api.minimaxi.com/anthropic/v1"),
            "https://api.minimaxi.com/anthropic"
        );
        assert_eq!(
            normalize_anthropic_base_url("https://api.minimaxi.com/anthropic/v1/"),
            "https://api.minimaxi.com/anthropic"
        );
        // A base without /v1 is left as-is (minus trailing slash).
        assert_eq!(
            normalize_anthropic_base_url("https://api.anthropic.com/"),
            "https://api.anthropic.com"
        );
        assert_eq!(
            normalize_anthropic_base_url("https://proxy.example/anthropic"),
            "https://proxy.example/anthropic"
        );
    }

    #[test]
    fn ollama_host_keeps_or_adds_scheme() {
        assert_eq!(
            parse_ollama_host(Some("http://localhost:11434")),
            ("http://localhost".to_string(), 11434)
        );
        // No scheme -> http:// is added.
        assert_eq!(
            parse_ollama_host(Some("localhost:11434")),
            ("http://localhost".to_string(), 11434)
        );
        // No port -> default 11434, scheme preserved.
        assert_eq!(
            parse_ollama_host(Some("http://gpu-box")),
            ("http://gpu-box".to_string(), 11434)
        );
        // None -> default host+port.
        assert_eq!(
            parse_ollama_host(None),
            ("http://localhost".to_string(), 11434)
        );
        // https preserved.
        assert_eq!(
            parse_ollama_host(Some("https://ollama.example:443")),
            ("https://ollama.example".to_string(), 443)
        );
        // Trailing slash + non-default port must not lose the port.
        assert_eq!(
            parse_ollama_host(Some("http://gpu-box:12345/")),
            ("http://gpu-box".to_string(), 12345)
        );
    }
}
