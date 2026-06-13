//! Crate-wide error type for zode-core.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("config io: {0}")]
    Io(#[from] std::io::Error),
    #[error("config parse: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no api key: set provider.apiKey in config or the {0} env var")]
    MissingApiKey(&'static str),
    #[error("unknown provider type: {0:?} (expected anthropic | openai | ollama)")]
    UnknownProvider(String),
    #[error("unknown openai dialect: {0:?}")]
    UnknownDialect(String),
    #[error("agent: {0}")]
    Agent(#[from] agent::error::AgentError),
    #[error("{0}")]
    Other(String),
}
