#![forbid(unsafe_code)]

mod engine_backend;
mod event_sink;
mod local_endpoint;
mod node;
mod node_identity;

pub use engine_backend::EventNormalizer;
pub use event_sink::EventSink;
pub use local_endpoint::LocalAgentEndpoint;
pub use node::NodeBackend;
pub use node_identity::NodeIdentityStore;

pub const CRATE_READY: bool = true;
