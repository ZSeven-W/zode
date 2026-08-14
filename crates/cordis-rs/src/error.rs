//! Harness errors with stable machine-readable codes.

use thiserror::Error;

/// Framework error with a stable code (`code()`), mirroring Cordis's
/// `CordisError`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CordisError {
    #[error("cannot create effect on inactive context")]
    InactiveEffect,
    #[error("service `{0}` is not provided")]
    ServiceNotFound(String),
    #[error("service `{0}` has a different type")]
    ServiceTypeMismatch(String),
    #[error("plugin `{0}` failed to start: {1}")]
    PluginStartup(String, String),
    #[error("invalid config for `{0}`: {1}")]
    ConfigInvalid(String, String),
    #[error("fiber `{0}` is disposed")]
    FiberDisposed(String),
    #[error("the harness context is disposed")]
    ContextDisposed,
    #[error("memory budget exceeded: {0}")]
    BudgetExceeded(&'static str),
    #[error("payload serialization failed: {0}")]
    Payload(String),
    #[error("a tokio runtime is required here")]
    NoRuntime,
    #[error("unsupported operation: {0}")]
    Unsupported(&'static str),
}

impl CordisError {
    /// Stable machine-readable code for this error.
    pub fn code(&self) -> &'static str {
        match self {
            CordisError::InactiveEffect => "INACTIVE_EFFECT",
            CordisError::ServiceNotFound(_) => "SERVICE_NOT_FOUND",
            CordisError::ServiceTypeMismatch(_) => "SERVICE_TYPE_MISMATCH",
            CordisError::PluginStartup(..) => "PLUGIN_STARTUP",
            CordisError::ConfigInvalid(..) => "CONFIG_INVALID",
            CordisError::FiberDisposed(_) => "FIBER_DISPOSED",
            CordisError::ContextDisposed => "CONTEXT_DISPOSED",
            CordisError::BudgetExceeded(_) => "BUDGET_EXCEEDED",
            CordisError::Payload(_) => "PAYLOAD",
            CordisError::NoRuntime => "NO_RUNTIME",
            CordisError::Unsupported(_) => "UNSUPPORTED",
        }
    }
}
