#![forbid(unsafe_code)]

mod endpoint;
mod error;
mod types;

pub use endpoint::{AgentEndpoint, AgentEventStream, AgentQuery, AgentSnapshot};
pub use error::{EndpointError, EndpointErrorKind, ProtocolError};
pub use types::*;

pub const CRATE_READY: bool = true;
