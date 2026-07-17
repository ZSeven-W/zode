use thiserror::Error;

/// Stable categories for failures at an agent endpoint boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EndpointErrorKind {
    Unavailable,
    InvalidRequest,
    CapabilityDenied,
    NotFound,
    Busy,
    /// The receiver for a one-shot request disappeared after the caller had
    /// already chosen a response; retrying the stale request cannot succeed.
    RequestExpired,
    /// The requested durable effect failed, but the endpoint completed a
    /// documented safe fallback and the caller must reconcile its UI.
    PartialSuccess,
    Internal,
}

/// A stable error envelope returned by an agent endpoint.
///
/// Runtime error text must pass through the caller's redactor before it is
/// placed in `message`; `message` must not contain secrets, API keys, or complete
/// tool arguments.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[error("{kind:?}: {message}")]
#[serde(rename_all = "camelCase")]
pub struct EndpointError {
    pub kind: EndpointErrorKind,
    pub message: String,
}

/// Errors produced while constructing or decoding the node protocol.
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// A workspace URI used a scheme that this protocol version does not support.
    #[error("invalid workspace URI `{0}`; expected a file:// or zode-node:// URI")]
    InvalidWorkspaceUri(String),

    /// The JSON payload did not match a known protocol message shape.
    #[error("failed to decode protocol JSON: {0}")]
    Decode(#[from] serde_json::Error),

    /// The message used a protocol version that this implementation cannot handle.
    #[error("unsupported protocol version {actual}; expected {expected}")]
    UnsupportedVersion { expected: u16, actual: u16 },

    /// A command that targets a turn omitted its caller-allocated turn identity.
    #[error("{command} requires a caller-allocated turnId")]
    MissingTurnId { command: &'static str },
}
