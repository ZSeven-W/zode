#![forbid(unsafe_code)]

mod event_sink;
mod local_endpoint;
mod node;

pub use event_sink::EventSink;
pub use local_endpoint::LocalAgentEndpoint;
pub use node::NodeBackend;

pub const CRATE_READY: bool = true;
