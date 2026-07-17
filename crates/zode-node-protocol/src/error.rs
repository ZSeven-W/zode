use thiserror::Error;

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
